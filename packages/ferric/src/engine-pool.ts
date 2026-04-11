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
 * `do()` dispatches a callback that receives an `EngineProxy`. Each proxy method
 * sends one round-trip message to the worker. The callback runs on the main
 * thread; only individual operations cross the thread boundary.
 *
 * ## Cooperative cancellation
 *
 * `evaluate()` and `do()` accept an `AbortSignal`. Cancellation sets a
 * SharedArrayBuffer flag that the worker checks between batches of
 * RUN_BATCH_SIZE rule firings.
 */

import { Worker } from "node:worker_threads";
import { resolve } from "node:path";
import type { WorkerRequest, WorkerResponse, PoolWorkerInit } from "./wire";
import { ABORT_BUFFER_SIZE, ABORT_FLAG_INDEX, toWire, fromWire } from "./wire";
import { FerricSymbol } from "./native";
import type {
  ClipsValue,
  RunResult,
  FiredRule,
  Fact,
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
 * Do not retain the proxy beyond the lifetime of the callback.
 */
export interface EngineProxy {
  load(source: string): Promise<void>;
  assertString(source: string): Promise<number[]>;
  assertFact(relation: string, ...fields: ClipsValue[]): Promise<number>;
  assertTemplate(
    templateName: string,
    slots: Record<string, ClipsValue>,
  ): Promise<number>;
  retract(factId: number): Promise<void>;
  getFact(factId: number): Promise<Fact | null>;
  facts(): Promise<Fact[]>;
  findFacts(relation: string): Promise<Fact[]>;
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
  reject: (error: Error) => void;
}

interface WorkerSlot {
  worker: Worker;
  nextId: number;
  pending: Map<number, PendingEntry>;
}

// ---------------------------------------------------------------------------
// Error reconstruction
// ---------------------------------------------------------------------------

function reconstructError(payload: WorkerResponse["error"]): Error {
  if (!payload) return new Error("Unknown pool worker error");

  const Ctor = ERROR_REGISTRY[payload.name];
  if (Ctor) {
    return new Ctor(payload.message);
  }

  if (payload.name === "AbortError") {
    return new DOMException(payload.message, "AbortError");
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
  private roundRobin = 0;
  private closed = false;

  private constructor(slots: WorkerSlot[]) {
    this.slots = slots;
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
   * @param options.threads Number of worker threads. Default: 1.
   */
  static async create(
    specs: EngineSpec[],
    options?: { threads?: number },
  ): Promise<EnginePool> {
    const threadCount = Math.max(1, options?.threads ?? 1);
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

    for (let i = 0; i < threadCount; i++) {
      const slot = EnginePool.createSlot(new Worker(workerPath));
      slots.push(slot);
      initPromises.push(EnginePool.initSlot(slot, init));
    }

    await Promise.all(initPromises);
    return new EnginePool(slots);
  }

  // ---------------------------------------------------------------------------
  // Internal slot management
  // ---------------------------------------------------------------------------

  private static createSlot(worker: Worker): WorkerSlot {
    const slot: WorkerSlot = {
      worker,
      nextId: 0,
      pending: new Map(),
    };

    worker.on("message", (resp: WorkerResponse) => {
      const entry = slot.pending.get(resp.id);
      if (!entry) return;
      slot.pending.delete(resp.id);

      if (resp.error) {
        entry.reject(reconstructError(resp.error));
      } else {
        entry.resolve(fromWire(resp.result, FerricSymbol));
      }
    });

    worker.on("error", (err: Error) => {
      const snapshot = [...slot.pending.values()];
      slot.pending.clear();
      for (const entry of snapshot) {
        entry.reject(err);
      }
    });

    worker.on("exit", (code: number) => {
      if (code !== 0 && slot.pending.size > 0) {
        const err = new Error(`Pool worker exited unexpectedly with code ${code}`);
        const snapshot = [...slot.pending.values()];
        slot.pending.clear();
        for (const entry of snapshot) {
          entry.reject(err);
        }
      }
    });

    return slot;
  }

  private static initSlot(slot: WorkerSlot, init: PoolWorkerInit): Promise<void> {
    const id = slot.nextId++;
    const req: WorkerRequest = { id, method: "__init", args: [init] };
    const promise = new Promise<void>((resolve, reject) => {
      slot.pending.set(id, {
        resolve: () => resolve(),
        reject,
      });
    });
    slot.worker.postMessage(req);
    return promise;
  }

  /** Pick the next worker slot via round-robin. */
  private pickSlot(): WorkerSlot {
    const slot = this.slots[this.roundRobin % this.slots.length];
    this.roundRobin = (this.roundRobin + 1) % this.slots.length;
    return slot;
  }

  /** Send a request to a specific slot and return a promise for the result. */
  private sendToSlot(slot: WorkerSlot, method: string, args: unknown[]): Promise<unknown> {
    if (this.closed) {
      return Promise.reject(new Error("EnginePool has been closed"));
    }
    const id = slot.nextId++;
    const req: WorkerRequest = { id, method, args };
    const promise = new Promise<unknown>((resolve, reject) => {
      slot.pending.set(id, { resolve, reject });
    });
    slot.worker.postMessage(req);
    return promise;
  }

  // ---------------------------------------------------------------------------
  // EngineProxy builder
  // ---------------------------------------------------------------------------

  private makeProxy(specName: string, slot: WorkerSlot): EngineProxy {
    const send = (method: string, args: unknown[]): Promise<unknown> =>
      this.sendToSlot(slot, method, [specName, ...args]);

    return {
      load: (source) => send("load", [source]) as Promise<void>,
      assertString: (source) => send("assertString", [source]) as Promise<number[]>,
      assertFact: (relation, ...fields) =>
        send("assertFact", [relation, ...fields.map(toWire)]) as Promise<number>,
      assertTemplate: (templateName, slots) =>
        send("assertTemplate", [templateName, toWire(slots)]) as Promise<number>,
      retract: (factId) => send("retract", [factId]) as Promise<void>,
      getFact: (factId) => send("getFact", [factId]) as Promise<Fact | null>,
      facts: () => send("facts", []) as Promise<Fact[]>,
      findFacts: (relation) => send("findFacts", [relation]) as Promise<Fact[]>,
      run: (options) =>
        send("__batched_run", [options?.limit ?? null, null]) as Promise<RunResult>,
      step: () => send("step", []) as Promise<FiredRule | null>,
      halt: () => send("halt", []) as Promise<void>,
      reset: () => send("reset", []) as Promise<void>,
      clear: () => send("clear", []) as Promise<void>,
      getOutput: (channel) => send("getOutput", [channel]) as Promise<string | null>,
      clearOutput: (channel) => send("clearOutput", [channel]) as Promise<void>,
      pushInput: (line) => send("pushInput", [line]) as Promise<void>,
    };
  }

  // ---------------------------------------------------------------------------
  // Public API
  // ---------------------------------------------------------------------------

  /**
   * Stateless one-shot evaluation: reset → assert → run → return facts.
   *
   * This is the primary entry point for concurrent rule evaluation. Each call
   * dispatches to a worker round-robin.
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
    if (this.closed) throw new Error("EnginePool has been closed");

    const signal = options?.signal;
    if (signal?.aborted) {
      throw new DOMException("The operation was aborted", "AbortError");
    }

    const slot = this.pickSlot();

    // Allocate a shared abort buffer for cooperative cancellation.
    const sab = new SharedArrayBuffer(ABORT_BUFFER_SIZE * Int32Array.BYTES_PER_ELEMENT);
    const abortFlag = new Int32Array(sab);

    const id = slot.nextId++;
    // Convert FerricSymbol instances in the request to wire format.
    const wireRequest = {
      ...request,
      facts: request.facts?.map((f) => {
        if (f.kind === "ordered") {
          return { ...f, fields: f.fields.map(toWire) };
        }
        return { ...f, slots: toWire(f.slots) };
      }),
    };

    const req: WorkerRequest = {
      id,
      method: "__evaluate",
      args: [specName, wireRequest, sab],
    };

    const promise = new Promise<EvaluateResult>((resolve, reject) => {
      slot.pending.set(id, {
        resolve: (val) => resolve(val as EvaluateResult),
        reject,
      });
    });

    let onAbort: (() => void) | undefined;
    if (signal) {
      onAbort = () => { Atomics.store(abortFlag, ABORT_FLAG_INDEX, 1); };
      signal.addEventListener("abort", onAbort, { once: true });
    }

    slot.worker.postMessage(req);

    try {
      return await promise;
    } finally {
      if (signal && onAbort) {
        signal.removeEventListener("abort", onAbort);
      }
    }
  }

  /**
   * Dispatch a function to run using a pooled engine.
   *
   * The callback receives an `EngineProxy` whose methods each send a single
   * round-trip to the worker. The callback executes on the main thread —
   * only individual operations cross the thread boundary.
   *
   * The proxy must not be retained beyond the callback's return value.
   *
   * @param specName Engine spec to use.
   * @param fn Callback receiving an EngineProxy.
   * @param options.signal AbortSignal for cancellation.
   *
   * Note: `T` must be structured-clonable (or a primitive) because the
   * return value of individual proxy methods crosses the thread boundary.
   * The callback itself returns its value directly on the main thread.
   */
  async do<T>(
    specName: string,
    fn: (engine: EngineProxy) => Promise<T>,
    options?: { signal?: AbortSignal },
  ): Promise<T> {
    if (this.closed) throw new Error("EnginePool has been closed");

    const signal = options?.signal;
    if (signal?.aborted) {
      throw new DOMException("The operation was aborted", "AbortError");
    }

    const slot = this.pickSlot();
    const proxy = this.makeProxy(specName, slot);

    // If the signal aborts during fn execution, we can only signal intent —
    // in-flight proxy calls will still complete normally (the proxy has no
    // knowledge of the signal). For run() calls issued through the proxy,
    // callers should pass { signal } directly if they want cancellation.
    return fn(proxy);
  }

  // ---------------------------------------------------------------------------
  // Lifecycle
  // ---------------------------------------------------------------------------

  /**
   * Shut down all worker threads.
   *
   * In-flight requests will reject. Pending promises are rejected with a
   * "pool closed" error.
   */
  async close(): Promise<void> {
    if (this.closed) return;
    this.closed = true;

    const closeErr = new Error("EnginePool closed");

    await Promise.all(
      this.slots.map(async (slot) => {
        // Drain pending promises.
        const snapshot = [...slot.pending.values()];
        slot.pending.clear();
        for (const entry of snapshot) {
          entry.reject(closeErr);
        }

        await slot.worker.terminate();
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
