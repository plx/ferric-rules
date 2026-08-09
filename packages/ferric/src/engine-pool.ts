/**
 * EnginePool — concurrent evaluation pool using multiple Worker threads.
 *
 * Each worker manages multiple named engine instances (one per EngineSpec).
 * Workers lazily create their engine instances on first use for a given spec.
 * Requests are dispatched round-robin across workers.
 *
 * ## Usage patterns
 *
 * ### Stateless evaluation (recommended)
 * `evaluate()` performs reset → assert → run → collect facts in one round-trip.
 * This is efficient and safe for concurrent use.
 *
 * ### Stateful operations
 * `do()` reserves one whole worker slot in the slot's root FIFO and invokes a
 * callback with an `EngineProxy`. The callback runs on the main thread; proxy
 * operations cross the worker boundary one at a time in invocation order. No
 * unrelated pool work can use that worker until the pool observes the
 * callback's returned Promise settle and its accepted proxy operations drain.
 *
 * ## Terminal worker failures
 * The first unexpected Worker error or exit permanently poisons future pool
 * admission; workers are not respawned. Work already accepted by healthy slots
 * may finish, including owner calls from an admitted callback. The failed slot
 * rejects everything it owns and cannot accept more work.
 *
 * ## Cooperative cancellation
 *
 * `evaluate()` and `do()` accept an `AbortSignal`. Cancellation sets a
 * SharedArrayBuffer flag that the worker checks between batches of
 * RUN_BATCH_SIZE rule firings. The first batch starts a fresh logical run and
 * later batches continue it. A host abort stops between batches without
 * calling native halt() merely to represent cancellation; existing public
 * result and rejection behavior remains unchanged.
 */

import { Worker } from "node:worker_threads";
import { AsyncLocalStorage } from "node:async_hooks";
import { resolve } from "node:path";
import type { WorkerRequest, WorkerResponse, PoolWorkerInit } from "./wire";
import { ABORT_BUFFER_SIZE, ABORT_FLAG_INDEX, toWire, fromWire } from "./wire";
import { FerricSymbol } from "./native";
import { normalizeEvaluateLimit, normalizeRunLimit } from "./limit-validation";
import type {
  ClipsValue,
  RunResult,
  FiredRule,
  Fact,
  FactId,
  FactIdInput,
  EvaluateRequest,
  EvaluateResult,
  EngineSpec,
} from "./types";
import { FerricError, ERROR_REGISTRY } from "./types";

// Re-export types for consumers.
export type { EngineSpec, EvaluateRequest, EvaluateResult };

// ---------------------------------------------------------------------------
// EngineProxy interface
// ---------------------------------------------------------------------------

/**
 * Proxy object passed to EnginePool.do() callbacks.
 *
 * Each method dispatches a single round-trip message to the pool worker.
 * Calls are serialized on the callback's exclusive worker lease. Once the pool
 * observes the callback's returned Promise settle, every later method rejects
 * with a deterministic lifetime error. Promise reactions that the callback
 * registers before returning can run before that observation and remain inside
 * the lease. If the callback's AbortSignal fires first, later method calls
 * reject with AbortError while calls already accepted by the lease drain in
 * invocation order.
 */
export interface EngineProxy {
  load(source: string): Promise<void>;
  assertString(source: string): Promise<FactId[]>;
  assertFact(relation: string, ...fields: ClipsValue[]): Promise<FactId>;
  assertTemplate(
    templateName: string,
    slots: Record<string, ClipsValue>,
  ): Promise<FactId>;
  retract(factId: FactIdInput): Promise<void>;
  getFact(factId: FactIdInput): Promise<Fact | null>;
  facts(): Promise<Fact[]>;
  findFacts(relation: string): Promise<Fact[]>;
  /**
   * Start a fresh logical run. The worker uses continuation only after its
   * first batch, preserving exact-boundary halt state and diagnostics. A zero
   * limit still starts fresh while firing no rules.
   */
  run(options?: { limit?: number }): Promise<RunResult>;
  step(): Promise<FiredRule | null>;
  halt(): Promise<void>;
  reset(): Promise<void>;
  clear(): Promise<void>;
  getOutput(channel: string): Promise<string | null>;
  clearOutput(channel: string): Promise<void>;
  pushInput(line: string): Promise<void>;
}

// ---------------------------------------------------------------------------
// Internal types
// ---------------------------------------------------------------------------

interface PendingEntry {
  resolve: (value: unknown) => void;
  reject: (error: unknown) => void;
}

/** A request waiting to be dispatched to a worker. */
interface QueuedRequest {
  kind: "request";
  req: WorkerRequest;
  entry: PendingEntry;
  signal?: AbortSignal;
  onAbort?: () => void;
  /** Extra message transfer list (e.g. SharedArrayBuffer). */
  transferList?: ArrayBuffer[];
}

interface WorkerLease {
  /** Whether the callback may still submit new proxy operations. */
  active: boolean;
  /** Whether this lease has reached its single terminal release. */
  released: boolean;
  /** Proxy requests accepted by this lease, in invocation order. */
  queue: QueuedRequest[];
  /** Accepted proxy requests that have not settled yet. */
  pendingCalls: number;
  /**
   * Waiters used after the pool observes settlement with accepted work
   * outstanding.
   */
  drainWaiters: Array<() => void>;
  /** Completion barrier used by close() for an already-admitted callback. */
  releasedPromise: Promise<void>;
  resolveReleased: () => void;
  releasePromise?: Promise<void>;
}

interface QueuedLease {
  kind: "lease";
  lease: WorkerLease;
  resolve: (lease: WorkerLease) => void;
  reject: (error: Error) => void;
  signal?: AbortSignal;
  onAbort?: () => void;
}

type QueuedWork = QueuedRequest | QueuedLease;

interface PoolCallbackContext {
  /** Mutable so descendants stop being reentrant at pool-observed settlement. */
  active: boolean;
}

type WorkerSlotState =
  | { kind: "running" }
  | { kind: "failed"; error: Error }
  | { kind: "terminating" }
  | { kind: "closed" };

interface WorkerSlotListeners {
  message: (response: WorkerResponse) => void;
  error: (error: Error) => void;
  exit: (code: number) => void;
}

interface WorkerSlot {
  worker: Worker;
  state: WorkerSlotState;
  nextId: number;
  pending: Map<number, PendingEntry>;
  /** Number of requests currently being processed by the worker. */
  inflight: number;
  /** Root requests and lease admissions waiting in per-slot FIFO order. */
  queue: QueuedWork[];
  /** Exclusive callback lease currently admitted on this whole worker slot. */
  activeLease?: WorkerLease;
  /** Close waiters that wake when every already-dispatched call is settled. */
  pendingDrainWaiters: Array<() => void>;
  /** Exact owned listener references, retained for deterministic detachment. */
  listeners: WorkerSlotListeners;
  listenersAttached: boolean;
  /** Installed once the slot belongs to a published EnginePool. */
  onTerminal?: (slot: WorkerSlot, error: Error) => void;
}

const INACTIVE_PROXY_MESSAGE =
  "EngineProxy is no longer valid outside its EnginePool.do callback";
const ABORTED_OPERATION_MESSAGE = "The operation was aborted";
const REENTRANT_POOL_MESSAGE =
  "EnginePool.do, EnginePool.evaluate, and EnginePool.close cannot be called " +
  "from within an active EnginePool.do callback on the same pool";
const MAX_ENGINE_POOL_THREADS = 64;

// ---------------------------------------------------------------------------
// Error reconstruction
// ---------------------------------------------------------------------------

function reconstructError(payload: WorkerResponse["error"]): Error {
  if (!payload) return new Error("Unknown pool worker error");

  const make = ERROR_REGISTRY[payload.name];
  if (make) {
    return make(payload.message);
  }

  if (payload.name === "AbortError") {
    return new DOMException(payload.message, "AbortError");
  }

  if (payload.name === "TypeError") {
    return new TypeError(payload.message);
  }

  const err = new FerricError(payload.message, payload.code);
  err.name = payload.name;
  return err;
}

// ---------------------------------------------------------------------------
// EnginePool
// ---------------------------------------------------------------------------

/**
 * A pool of Worker threads for concurrent engine evaluation.
 *
 * @example
 * ```ts
 * await using pool = await EnginePool.create(
 *   [{ name: "rules", source: clpSource }],
 *   { threads: 4 },
 * );
 *
 * const result = await pool.evaluate("rules", {
 *   facts: [{ kind: "ordered", relation: "input", fields: [42] }],
 * });
 * ```
 */
export class EnginePool {
  private readonly slots: WorkerSlot[];
  private readonly callbackContext = new AsyncLocalStorage<PoolCallbackContext>();
  private roundRobin = 0;
  private closed = false;
  /** The first terminal Worker failure poisons every later root admission. */
  private terminalError?: Error;

  private constructor(slots: WorkerSlot[]) {
    this.slots = slots;
    for (const slot of slots) {
      if (this.terminalError === undefined && slot.state.kind === "failed") {
        this.terminalError = slot.state.error;
      }
      if (slot.state.kind === "running") {
        slot.onTerminal = (failedSlot, error) => {
          this.handleSlotTerminal(failedSlot, error);
        };
      }
    }
  }

  // ---------------------------------------------------------------------------
  // Factory
  // ---------------------------------------------------------------------------

  /**
   * Create a pool with the given engine specs and thread count.
   *
   * Each thread lazily creates engine instances on first use for each spec.
   *
   * @param specs Named engine configurations.
   * @param options.threads Number of worker threads, from 1 through 64.
   * Default: 1.
   */
  static create(
    specs: EngineSpec[],
    options?: { threads?: number },
  ): Promise<EnginePool> {
    const configuredThreadCount = options?.threads;
    const threadCount =
      configuredThreadCount === undefined ? 1 : configuredThreadCount;
    if (
      !Number.isSafeInteger(threadCount) ||
      threadCount < 1 ||
      threadCount > MAX_ENGINE_POOL_THREADS
    ) {
      throw new RangeError(
        "EnginePool.create: 'threads' must be a safe integer between 1 and 64",
      );
    }

    return EnginePool.createValidated(specs, threadCount);
  }

  private static async createValidated(
    specs: EngineSpec[],
    threadCount: number,
  ): Promise<EnginePool> {
    const workerPath = resolve(__dirname, "pool-worker.js");

    const init: PoolWorkerInit = {
      specs: specs.map((s) => ({
        name: s.name,
        options: s.options
          ? {
              strategy: s.options.strategy,
              encoding: s.options.encoding,
              maxCallDepth: s.options.maxCallDepth,
            }
          : undefined,
        source: s.source,
      })),
    };

    const initPromises: Promise<void>[] = [];
    const slots: WorkerSlot[] = [];

    try {
      for (let i = 0; i < threadCount; i++) {
        const slot = EnginePool.createSlot(new Worker(workerPath));
        slots.push(slot);
        initPromises.push(EnginePool.initSlot(slot, init));
      }

      await Promise.all(initPromises);
      const pool = new EnginePool(slots);
      // Worker events are normally delivered on later event-loop turns, but a
      // deterministic Worker seam can report init success and then fail from
      // one synchronous postMessage call. Never publish that prefailed pool.
      if (pool.terminalError) throw pool.terminalError;
      return pool;
    } catch (error) {
      // A Worker constructor can throw after earlier initialization promises
      // were created but before Promise.all() attached its handlers.
      for (const initPromise of initPromises) {
        void initPromise.catch(() => undefined);
      }

      const cleanupError =
        error instanceof Error
          ? error
          : new Error("EnginePool creation failed during initialization");
      await Promise.all(
        slots.map(async (slot) => {
          EnginePool.failSlot(slot, cleanupError);
          try {
            await EnginePool.terminateSlot(slot);
          } catch {
            // Ignore best-effort cleanup failures while unwinding create().
          }
        }),
      );
      throw error;
    }
  }

  // ---------------------------------------------------------------------------
  // Internal slot management
  // ---------------------------------------------------------------------------

  private static createSlot(worker: Worker): WorkerSlot {
    let slot!: WorkerSlot;
    const listeners: WorkerSlotListeners = {
      message: (resp: WorkerResponse) => {
        if (slot.state.kind !== "running") return;

        const entry = slot.pending.get(resp.id);
        if (!entry) return;
        slot.pending.delete(resp.id);
        slot.inflight--;

        if ("error" in resp) {
          entry.reject(reconstructError(resp.error));
        } else {
          entry.resolve(fromWire(resp.result, FerricSymbol));
        }

        EnginePool.notifyPendingDrained(slot);

        // Dispatch the next queued request, if any.
        EnginePool.drainQueue(slot);
      },
      error: (err: Error) => {
        EnginePool.signalSlotFailure(slot, err);
      },
      exit: (code: number) => {
        if (slot.state.kind === "terminating") {
          slot.state = { kind: "closed" };
          EnginePool.detachSlotListeners(slot);
          EnginePool.notifyPendingDrained(slot);
          return;
        }
        if (slot.state.kind !== "running") return;

        const err = new Error(
          code === 0
            ? slot.pending.size > 0
              ? "Pool worker exited before responding to pending request"
              : "Pool worker exited unexpectedly with code 0"
            : `Pool worker exited unexpectedly with code ${code}`,
        );
        EnginePool.signalSlotFailure(slot, err);
      },
    };

    slot = {
      worker,
      state: { kind: "running" },
      nextId: 0,
      pending: new Map(),
      inflight: 0,
      queue: [],
      pendingDrainWaiters: [],
      listeners,
      listenersAttached: false,
    };

    EnginePool.attachSlotListeners(slot);

    return slot;
  }

  private static attachSlotListeners(slot: WorkerSlot): void {
    if (slot.listenersAttached) return;
    slot.worker.on("message", slot.listeners.message);
    slot.worker.on("error", slot.listeners.error);
    slot.worker.on("exit", slot.listeners.exit);
    slot.listenersAttached = true;
  }

  private static detachSlotListeners(slot: WorkerSlot): void {
    if (!slot.listenersAttached) return;
    slot.worker.off("message", slot.listeners.message);
    slot.worker.off("error", slot.listeners.error);
    slot.worker.off("exit", slot.listeners.exit);
    slot.listenersAttached = false;
  }

  private static signalSlotFailure(slot: WorkerSlot, error: Error): void {
    if (slot.state.kind !== "running") return;
    if (slot.onTerminal) {
      slot.onTerminal(slot, error);
    } else {
      EnginePool.failSlot(slot, error);
    }
  }

  private handleSlotTerminal(slot: WorkerSlot, error: Error): void {
    if (slot.state.kind !== "running") return;
    this.terminalError ??= error;
    EnginePool.failSlot(slot, this.terminalError);
  }

  /** Atomically make one Worker slot unusable and settle all work it owns. */
  private static failSlot(slot: WorkerSlot, error: Error): void {
    if (slot.state.kind !== "running") return;

    // Publish failure before rejecting anything so Promise reactions can never
    // enqueue more work onto this Worker.
    slot.state = { kind: "failed", error };
    slot.onTerminal = undefined;
    EnginePool.detachSlotListeners(slot);

    const pending = [...slot.pending.values()];
    const rootQueue = slot.queue.splice(0);
    const ownerQueue = slot.activeLease?.queue.splice(0) ?? [];
    slot.pending.clear();
    slot.inflight = 0;

    for (const queued of rootQueue) {
      EnginePool.rejectQueuedWork(queued, error);
    }
    for (const queued of ownerQueue) {
      EnginePool.removeAbortListener(queued);
      queued.entry.reject(error);
    }
    for (const entry of pending) {
      entry.reject(error);
    }

    EnginePool.notifyPendingDrained(slot);
  }

  private static rejectQueuedWork(queued: QueuedWork, error: Error): void {
    EnginePool.removeAbortListener(queued);
    if (queued.kind === "lease") {
      EnginePool.markLeaseReleased(queued.lease);
      queued.reject(error);
    } else {
      queued.entry.reject(error);
    }
  }

  private static notifyPendingDrained(slot: WorkerSlot): void {
    if (slot.pending.size !== 0) return;
    const waiters = slot.pendingDrainWaiters.splice(0);
    for (const resolve of waiters) resolve();
  }

  private static waitForPending(slot: WorkerSlot): Promise<void> {
    if (slot.pending.size === 0) return Promise.resolve();
    return new Promise<void>((resolve) => {
      slot.pendingDrainWaiters.push(resolve);
    });
  }

  private static async terminateSlot(slot: WorkerSlot): Promise<void> {
    if (slot.state.kind === "closed" || slot.state.kind === "terminating") return;
    if (slot.state.kind === "running" || slot.state.kind === "failed") {
      slot.state = { kind: "terminating" };
    }

    try {
      await slot.worker.terminate();
    } finally {
      if (slot.state.kind === "terminating") {
        slot.state = { kind: "closed" };
      }
      slot.onTerminal = undefined;
      EnginePool.detachSlotListeners(slot);
      EnginePool.notifyPendingDrained(slot);
    }
  }

  private static initSlot(slot: WorkerSlot, init: PoolWorkerInit): Promise<void> {
    const id = slot.nextId++;
    const req: WorkerRequest = { id, method: "__init", args: [init] };
    return new Promise<void>((resolve, reject) => {
      const queued: QueuedRequest = {
        kind: "request",
        req,
        entry: {
          resolve: () => resolve(),
          reject,
        },
      };

      if (!EnginePool.dispatchRequest(slot, queued)) {
        EnginePool.drainQueue(slot);
      }
    });
  }

  /** Pick the next worker slot via round-robin. */
  private pickSlot(): WorkerSlot {
    const slot = this.slots[this.roundRobin % this.slots.length];
    this.roundRobin = (this.roundRobin + 1) % this.slots.length;
    return slot;
  }

  private assertNotInActiveCallback(): void {
    if (this.callbackContext.getStore()?.active) {
      throw new Error(REENTRANT_POOL_MESSAGE);
    }
  }

  private static createLease(): WorkerLease {
    let resolveReleased!: () => void;
    const releasedPromise = new Promise<void>((resolve) => {
      resolveReleased = resolve;
    });
    return {
      active: false,
      released: false,
      queue: [],
      pendingCalls: 0,
      drainWaiters: [],
      releasedPromise,
      resolveReleased,
    };
  }

  private static markLeaseReleased(lease: WorkerLease): void {
    if (lease.released) return;
    lease.active = false;
    lease.released = true;
    lease.resolveReleased();
  }

  private static removeAbortListener(
    queued: Pick<QueuedRequest, "signal" | "onAbort">,
  ): void {
    if (queued.signal && queued.onAbort) {
      queued.signal.removeEventListener("abort", queued.onAbort);
      queued.onAbort = undefined;
    }
  }

  /** Register and send once; false means this call owned and rolled back a throw. */
  private static dispatchRequest(slot: WorkerSlot, queued: QueuedRequest): boolean {
    slot.pending.set(queued.req.id, queued.entry);
    slot.inflight++;

    try {
      slot.worker.postMessage(queued.req);
      return true;
    } catch (error) {
      // A queued request's dequeue-cancellation listener is no longer useful
      // after any attempted send, including when a synchronous Worker event
      // settled the request before postMessage threw.
      EnginePool.removeAbortListener(queued);

      // Preserve first settlement across synchronous response/error/exit
      // seams. Those paths already reconciled the counter and any owned work.
      if (slot.pending.get(queued.req.id) !== queued.entry) return true;

      slot.pending.delete(queued.req.id);
      slot.inflight--;
      queued.entry.reject(error);
      EnginePool.notifyPendingDrained(slot);
      return false;
    }
  }

  private static settleLeaseCall(lease: WorkerLease): void {
    lease.pendingCalls--;
    if (lease.pendingCalls !== 0) return;
    const waiters = lease.drainWaiters.splice(0);
    for (const resolve of waiters) resolve();
  }

  private static trackLeaseCall<T>(
    lease: WorkerLease,
    promise: Promise<T>,
  ): Promise<T> {
    lease.pendingCalls++;
    promise.then(
      () => EnginePool.settleLeaseCall(lease),
      () => EnginePool.settleLeaseCall(lease),
    );
    return promise;
  }

  private static waitForLeaseCalls(lease: WorkerLease): Promise<void> {
    if (lease.pendingCalls === 0) return Promise.resolve();
    return new Promise<void>((resolve) => {
      lease.drainWaiters.push(resolve);
    });
  }

  /** Dispatch the next owner request or root FIFO item for an idle slot. */
  private static drainQueue(slot: WorkerSlot): void {
    if (slot.state.kind !== "running") return;
    if (slot.inflight !== 0) return;

    const lease = slot.activeLease;
    if (lease) {
      while (lease.queue.length > 0) {
        const queued = lease.queue.shift()!;

        if (queued.signal?.aborted) {
          EnginePool.removeAbortListener(queued);
          queued.entry.reject(
            new DOMException("The operation was aborted", "AbortError"),
          );
          continue;
        }

        // This request is no longer dequeue-cancellable once dispatched. Its
        // method-specific in-flight cancellation (notably batched run) remains
        // independently wired by the proxy.
        EnginePool.removeAbortListener(queued);
        if (EnginePool.dispatchRequest(slot, queued)) return;
      }

      // An exclusive callback retains the worker even while it is between
      // awaits and has no native request in flight.
      return;
    }

    while (slot.queue.length > 0) {
      const queued = slot.queue.shift()!;

      if (queued.kind === "lease") {
        if (queued.signal?.aborted) {
          EnginePool.removeAbortListener(queued);
          EnginePool.markLeaseReleased(queued.lease);
          queued.reject(
            new DOMException("The operation was aborted", "AbortError"),
          );
          continue;
        }

        EnginePool.removeAbortListener(queued);
        slot.activeLease = queued.lease;
        queued.lease.active = true;
        queued.resolve(queued.lease);
        return;
      }

      const request = queued as QueuedRequest;

      // If the request was aborted while queued, reject it immediately.
      if (request.signal?.aborted) {
        EnginePool.removeAbortListener(request);
        request.entry.reject(
          new DOMException("The operation was aborted", "AbortError"),
        );
        continue;
      }

      if (EnginePool.dispatchRequest(slot, request)) return;
    }
  }

  /** Reserve a whole worker slot in root FIFO order for one do() callback. */
  private acquireLease(
    slot: WorkerSlot,
    signal?: AbortSignal,
  ): Promise<WorkerLease> {
    if (this.closed) {
      return Promise.reject(new Error("EnginePool has been closed"));
    }
    if (this.terminalError) {
      return Promise.reject(this.terminalError);
    }
    if (signal?.aborted) {
      return Promise.reject(
        new DOMException("The operation was aborted", "AbortError"),
      );
    }

    const lease = EnginePool.createLease();
    return new Promise<WorkerLease>((resolve, reject) => {
      if (
        slot.inflight === 0 &&
        slot.activeLease === undefined &&
        slot.queue.length === 0
      ) {
        slot.activeLease = lease;
        lease.active = true;
        resolve(lease);
        return;
      }

      const queued: QueuedLease = {
        kind: "lease",
        lease,
        resolve,
        reject,
        signal,
      };
      if (signal) {
        queued.onAbort = () => {
          const idx = slot.queue.indexOf(queued);
          if (idx !== -1) {
            slot.queue.splice(idx, 1);
            EnginePool.markLeaseReleased(lease);
            reject(new DOMException("The operation was aborted", "AbortError"));
          }
        };
        signal.addEventListener("abort", queued.onAbort, { once: true });
      }
      slot.queue.push(queued);
    });
  }

  /** Invalidate one callback proxy, drain accepted owner work, and release once. */
  private releaseLease(slot: WorkerSlot, lease: WorkerLease): Promise<void> {
    if (lease.releasePromise) return lease.releasePromise;

    lease.active = false;
    lease.releasePromise = (async () => {
      await EnginePool.waitForLeaseCalls(lease);
      if (slot.activeLease === lease) {
        slot.activeLease = undefined;
      }
      EnginePool.markLeaseReleased(lease);
      EnginePool.drainQueue(slot);
    })();
    return lease.releasePromise;
  }

  /** Send a request to a specific slot, queueing if busy. */
  private sendToSlot(
    slot: WorkerSlot,
    method: string,
    args: unknown[],
    signal?: AbortSignal,
  ): Promise<unknown> {
    if (this.closed) {
      return Promise.reject(new Error("EnginePool has been closed"));
    }
    if (this.terminalError) {
      return Promise.reject(this.terminalError);
    }
    const id = slot.nextId++;
    const req: WorkerRequest = { id, method, args };

    return new Promise<unknown>((resolve, reject) => {
      const entry: PendingEntry = { resolve, reject };

      if (
        slot.inflight === 0 &&
        slot.activeLease === undefined &&
        slot.queue.length === 0
      ) {
        // Dispatch immediately.
        const queued: QueuedRequest = {
          kind: "request",
          req,
          entry,
          signal,
        };
        if (!EnginePool.dispatchRequest(slot, queued)) {
          EnginePool.drainQueue(slot);
        }
      } else {
        // Queue and set up abort listener for queued cancellation.
        const queued: QueuedRequest = {
          kind: "request",
          req,
          entry,
          signal,
        };
        if (signal) {
          queued.onAbort = () => {
            const idx = slot.queue.indexOf(queued);
            if (idx !== -1) {
              slot.queue.splice(idx, 1);
              reject(new DOMException("The operation was aborted", "AbortError"));
            }
          };
          signal.addEventListener("abort", queued.onAbort, { once: true });
        }
        slot.queue.push(queued);
      }
    });
  }

  /** Send an owner request through its lease-private FIFO. */
  private sendOnLease(
    slot: WorkerSlot,
    lease: WorkerLease,
    method: string,
    args: unknown[],
    signal?: AbortSignal,
  ): Promise<unknown> {
    if (!lease.active || lease.released || slot.activeLease !== lease) {
      return Promise.reject(new Error(INACTIVE_PROXY_MESSAGE));
    }
    if (slot.state.kind === "failed") {
      return Promise.reject(slot.state.error);
    }
    if (slot.state.kind !== "running") {
      return Promise.reject(new Error("EnginePool has been closed"));
    }
    if (signal?.aborted) {
      return Promise.reject(
        new DOMException(ABORTED_OPERATION_MESSAGE, "AbortError"),
      );
    }

    const id = slot.nextId++;
    const req: WorkerRequest = { id, method, args };
    const promise = new Promise<unknown>((resolve, reject) => {
      const entry: PendingEntry = { resolve, reject };
      const queued: QueuedRequest = {
        kind: "request",
        req,
        entry,
      };

      if (slot.inflight === 0 && lease.queue.length === 0) {
        if (!EnginePool.dispatchRequest(slot, queued)) {
          EnginePool.drainQueue(slot);
        }
      } else {
        lease.queue.push(queued);
      }
    });

    return EnginePool.trackLeaseCall(lease, promise);
  }

  // ---------------------------------------------------------------------------
  // EngineProxy builder
  // ---------------------------------------------------------------------------

  private makeProxy(
    specName: string,
    slot: WorkerSlot,
    lease: WorkerLease,
    signal?: AbortSignal,
  ): EngineProxy {
    const send = (method: string, args: unknown[]): Promise<unknown> =>
      this.sendOnLease(slot, lease, method, [specName, ...args], signal);

    const withActiveLease = <T>(operation: () => Promise<T>): Promise<T> => {
      if (!lease.active || lease.released || slot.activeLease !== lease) {
        return Promise.reject(new Error(INACTIVE_PROXY_MESSAGE));
      }
      if (slot.state.kind === "failed") {
        return Promise.reject(slot.state.error);
      }
      if (slot.state.kind !== "running") {
        return Promise.reject(new Error("EnginePool has been closed"));
      }
      if (signal?.aborted) {
        return Promise.reject(
          new DOMException(ABORTED_OPERATION_MESSAGE, "AbortError"),
        );
      }
      return operation();
    };

    const runWithAbort = (options?: { limit?: number }): Promise<RunResult> =>
      withActiveLease(async () => {
        const limit = normalizeRunLimit(
          (options as { limit?: unknown } | undefined)?.limit,
          "EngineProxy.run",
        );

        // Allocate a per-call abort buffer for out-of-band host cancellation;
        // the worker polls it between logical-run continuation chunks.
        const sab = new SharedArrayBuffer(
          ABORT_BUFFER_SIZE * Int32Array.BYTES_PER_ELEMENT,
        );
        const abortFlag = new Int32Array(sab);

        let onAbort: (() => void) | undefined;
        if (signal) {
          if (signal.aborted) {
            Atomics.store(abortFlag, ABORT_FLAG_INDEX, 1);
          } else {
            onAbort = () => { Atomics.store(abortFlag, ABORT_FLAG_INDEX, 1); };
            signal.addEventListener("abort", onAbort, { once: true });
          }
        }

        try {
          return (await send("__batched_run", [
            limit ?? null,
            sab,
          ])) as RunResult;
        } finally {
          if (signal && onAbort) {
            signal.removeEventListener("abort", onAbort);
          }
        }
      });

    return {
      load: (source) =>
        withActiveLease(() => send("load", [source]) as Promise<void>),
      assertString: (source) =>
        withActiveLease(
          () => send("assertString", [source]) as Promise<FactId[]>,
        ),
      assertFact: (relation, ...fields) =>
        withActiveLease(
          () => send(
            "assertFact",
            [relation, ...fields.map(toWire)],
          ) as Promise<FactId>,
        ),
      assertTemplate: (templateName, slots) =>
        withActiveLease(
          () => send(
            "assertTemplate",
            [templateName, toWire(slots)],
          ) as Promise<FactId>,
        ),
      retract: (factId) =>
        withActiveLease(() => send("retract", [factId]) as Promise<void>),
      getFact: (factId) =>
        withActiveLease(
          () => send("getFact", [factId]) as Promise<Fact | null>,
        ),
      facts: () =>
        withActiveLease(() => send("facts", []) as Promise<Fact[]>),
      findFacts: (relation) =>
        withActiveLease(
          () => send("findFacts", [relation]) as Promise<Fact[]>,
        ),
      run: runWithAbort,
      step: () =>
        withActiveLease(
          () => send("step", []) as Promise<FiredRule | null>,
        ),
      halt: () =>
        withActiveLease(() => send("halt", []) as Promise<void>),
      reset: () =>
        withActiveLease(() => send("reset", []) as Promise<void>),
      clear: () =>
        withActiveLease(() => send("clear", []) as Promise<void>),
      getOutput: (channel) =>
        withActiveLease(
          () => send("getOutput", [channel]) as Promise<string | null>,
        ),
      clearOutput: (channel) =>
        withActiveLease(
          () => send("clearOutput", [channel]) as Promise<void>,
        ),
      pushInput: (line) =>
        withActiveLease(() => send("pushInput", [line]) as Promise<void>),
    };
  }

  // ---------------------------------------------------------------------------
  // Public API
  // ---------------------------------------------------------------------------

  /**
   * Stateless one-shot evaluation: reset → assert → run → return facts.
   *
   * This is the primary entry point for concurrent rule evaluation. Each call
   * dispatches to a worker round-robin. Its run phase starts one fresh logical
   * run and uses continuation for later batches, preserving exact-boundary
   * halt state and diagnostics. In-flight host abort retains the existing
   * partial HaltRequested result without setting the native halt latch merely
   * to represent cancellation.
   *
   * @param specName Engine spec to use.
   * @param request Facts and parameters for the evaluation.
   * @param options.signal AbortSignal for cancellation.
   */
  async evaluate(
    specName: string,
    request: EvaluateRequest,
    options?: { signal?: AbortSignal },
  ): Promise<EvaluateResult> {
    this.assertNotInActiveCallback();
    if (this.closed) throw new Error("EnginePool has been closed");
    if (this.terminalError) throw this.terminalError;

    const signal = options?.signal;
    const limit = normalizeEvaluateLimit(
      (request as { limit?: unknown }).limit,
      "EnginePool.evaluate",
    );
    if (signal?.aborted) {
      throw new DOMException("The operation was aborted", "AbortError");
    }

    const slot = this.pickSlot();

    // Allocate a shared abort buffer for cooperative cancellation.
    const sab = new SharedArrayBuffer(ABORT_BUFFER_SIZE * Int32Array.BYTES_PER_ELEMENT);
    const abortFlag = new Int32Array(sab);

    // Convert FerricSymbol instances in the request to wire format.
    const wireRequest = {
      ...request,
      limit,
      facts: request.facts?.map((f) => {
        if (f.kind === "ordered") {
          return { ...f, fields: f.fields.map(toWire) };
        }
        return { ...f, slots: toWire(f.slots) };
      }),
    };

    // Set up in-flight abort: set the out-of-band host-cancellation flag.
    let onAbort: (() => void) | undefined;
    if (signal) {
      onAbort = () => { Atomics.store(abortFlag, ABORT_FLAG_INDEX, 1); };
      signal.addEventListener("abort", onAbort, { once: true });
    }

    try {
      const result = await this.sendToSlot(
        slot,
        "__evaluate",
        [specName, wireRequest, sab],
        signal,
      );
      return result as EvaluateResult;
    } finally {
      if (signal && onAbort) {
        signal.removeEventListener("abort", onAbort);
      }
    }
  }

  /**
   * Run a callback with an exclusive lease on one pooled worker slot.
   *
   * Root work enters each selected slot in FIFO order. While this callback's
   * Promise is pending, no unrelated `do()` or `evaluate()` work can execute on
   * that worker, even when the callback is between proxy calls. Concurrent
   * proxy calls are serialized in invocation order.
   *
   * This is scheduling isolation, not rollback: successful mutations remain if
   * the callback later rejects. The proxy becomes invalid when the pool's
   * registered Promise reaction observes the callback outcome, before `do()`
   * delivers that value or error. Promise reactions registered by the callback
   * before it returns can run first and remain inside the lease. An AbortSignal
   * can reject the public Promise earlier and immediately prevents new proxy
   * calls. Calls accepted before abort retain their outcomes and drain in order;
   * the lease remains exclusive until the pool observes the callback outcome.
   *
   * Calling `do()`, `evaluate()`, or `close()` on this same pool from inside an
   * active callback rejects; use the supplied proxy instead. Other pools remain
   * independent.
   *
   * @param specName Engine spec to use.
   * @param fn Callback receiving an EngineProxy.
   * @param options.signal AbortSignal for cancellation.
   *
   * Only proxy arguments and results cross the worker boundary. The callback's
   * `T` value remains on the main thread and need not be structured-clonable.
   */
  async do<T>(
    specName: string,
    fn: (engine: EngineProxy) => Promise<T>,
    options?: { signal?: AbortSignal },
  ): Promise<T> {
    this.assertNotInActiveCallback();
    if (this.closed) throw new Error("EnginePool has been closed");
    if (this.terminalError) throw this.terminalError;

    const signal = options?.signal;
    if (signal?.aborted) {
      throw new DOMException("The operation was aborted", "AbortError");
    }

    const slot = this.pickSlot();
    const lease = await this.acquireLease(slot, signal);

    // Abort can race with admission after the queued listener is removed but
    // before this async continuation resumes. Such a callback never starts.
    if (signal?.aborted) {
      await this.releaseLease(slot, lease);
      throw new DOMException("The operation was aborted", "AbortError");
    }

    const proxy = this.makeProxy(specName, slot, lease, signal);
    const context: PoolCallbackContext = { active: true };
    let callbackPromise: Promise<T>;
    try {
      callbackPromise = this.callbackContext.run(
        context,
        () => Promise.resolve(fn(proxy)),
      );
    } catch (error) {
      callbackPromise = Promise.reject(error);
    }

    // Cleanup follows the pool-observed callback lifetime, not the
    // caller-visible AbortError race. Promise reactions registered by callback
    // code before it returns may run before this reaction and remain inside the
    // lease. Once this reaction begins, the proxy is invalidated before accepted
    // work drains and before do() delivers the callback's value or error.
    const completion = callbackPromise.then(
      async (value) => {
        context.active = false;
        await this.releaseLease(slot, lease);
        return value;
      },
      async (error: unknown) => {
        context.active = false;
        await this.releaseLease(slot, lease);
        throw error;
      },
    );

    if (!signal) return completion;

    // E-006: abort rejects the public do() promise promptly, but completion
    // above remains attached and owns the eventual lease cleanup.
    return new Promise<T>((resolve, reject) => {
      const onAbort = () => {
        reject(new DOMException("The operation was aborted", "AbortError"));
      };
      if (signal.aborted) {
        onAbort();
      } else {
        signal.addEventListener("abort", onAbort, { once: true });
      }
      const stopListening = () => {
        signal.removeEventListener("abort", onAbort);
      };
      // The cancellation contract is phrased against pool-observed callback
      // completion, not the later accepted-work drain barrier. Once the pool's
      // callback reaction runs, a subsequent signal change cannot replace the
      // callback result with AbortError.
      callbackPromise.then(stopListening, stopListening);
      completion.then(
        (value) => resolve(value),
        (error: unknown) => reject(error),
      );
    });
  }

  // ---------------------------------------------------------------------------
  // Lifecycle
  // ---------------------------------------------------------------------------

  /**
   * Gracefully shut down all worker threads.
   *
   * - New requests are rejected immediately after close() is called.
   * - Queued requests and not-yet-admitted leases reject with "EnginePool closed".
   * - Already-admitted callbacks retain owner dispatch access and may settle.
   * - Already-dispatched ordinary requests are allowed to settle.
   * - Workers are terminated after all in-flight requests complete.
   * - Idempotent — safe to call multiple times.
   */
  async close(): Promise<void> {
    this.assertNotInActiveCallback();
    if (this.closed) return;
    this.closed = true;

    const closeErr = new Error("EnginePool closed");

    await Promise.all(
      this.slots.map(async (slot) => {
        // Reject all queued (not yet dispatched) requests.
        const rootQueue = slot.queue.splice(0);
        for (const queued of rootQueue) {
          EnginePool.rejectQueuedWork(queued, closeErr);
        }

        // A do() callback is admitted work even while it has no native request
        // in flight. Its owner calls remain valid after close starts, and close
        // waits for pool-observed callback settlement plus accepted-call
        // draining.
        const activeLease = slot.activeLease;
        if (activeLease) {
          await activeLease.releasedPromise;
        }

        // A slot-owned waiter is resolved by either its last response or its
        // atomic terminal-failure cleanup. It does not depend on a later
        // Worker message that can never arrive after error/exit.
        await EnginePool.waitForPending(slot);

        await EnginePool.terminateSlot(slot);
      }),
    );
  }

  /**
   * Async dispose for `await using pool = await EnginePool.create(...)`.
   */
  async [Symbol.asyncDispose](): Promise<void> {
    await this.close();
  }
}
