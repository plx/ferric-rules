/**
 * EngineHandle — async wrapper around a synchronous Engine running on a
 * dedicated Worker thread.
 *
 * All methods return Promises and are safe to call from the main thread or
 * any other thread without blocking the event loop.
 *
 * ## Cooperative cancellation
 *
 * EngineHandle.run() supports cancellation via AbortSignal. Cancellation uses
 * a SharedArrayBuffer flag checked by the worker between batches of
 * RUN_BATCH_SIZE rule firings. The first batch starts a fresh logical run and
 * later batches continue it without clearing halt state or diagnostics. When
 * the signal fires, the worker stops between batches and returns a partial
 * result with HaltReason.HaltRequested; it does not call native halt() merely
 * to represent host cancellation.
 *
 * ## Thread affinity
 *
 * The underlying Engine is created on the worker's OS thread and never
 * touched from any other thread. This satisfies the Ferric engine's
 * thread-affinity contract.
 */

import { Worker } from "node:worker_threads";
import { resolve } from "node:path";
import type { WorkerRequest, WorkerResponse, WorkerInit } from "./wire";
import { ABORT_BUFFER_SIZE, ABORT_FLAG_INDEX, toWire, fromWire } from "./wire";
import { normalizeRunLimit } from "./limit-validation";
import { FerricSymbol } from "./native";
import type {
  ClipsValue,
  RunResult,
  FiredRule,
  Fact,
  FactId,
  FactIdInput,
  RuleInfo,
  EngineHandleOptions,
  Format,
} from "./types";
import { FerricError, ERROR_REGISTRY } from "./types";

// Re-export EngineHandleOptions so callers can import it from this module.
export type { EngineHandleOptions };

// ---------------------------------------------------------------------------
// Pending-request map entry
// ---------------------------------------------------------------------------

interface PendingEntry {
  resolve: (value: unknown) => void;
  reject: (error: unknown) => void;
}

// ---------------------------------------------------------------------------
// Error reconstruction
// ---------------------------------------------------------------------------

function reconstructError(payload: WorkerResponse["error"]): Error {
  if (!payload) return new Error("Unknown worker error");

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

/** Preserve a create failure, retaining a secondary teardown failure when possible. */
function attachCreateCleanupCause(primary: unknown, cleanupFailure: unknown): void {
  if (!(primary instanceof Error)) return;

  try {
    const errorWithCause = primary as Error & { cause?: unknown };
    const ownCause = Object.getOwnPropertyDescriptor(errorWithCause, "cause");
    // Defining a new own cause needs an extensible object. Redefining one needs
    // its existing descriptor to be configurable; otherwise identity wins.
    if (ownCause?.configurable === false) return;
    if (ownCause === undefined && !Object.isExtensible(errorWithCause)) return;

    const cause =
      "cause" in errorWithCause
        ? new AggregateError(
            [errorWithCause.cause, cleanupFailure],
            "EngineHandle.create cleanup also failed",
          )
        : cleanupFailure;

    Object.defineProperty(errorWithCause, "cause", {
      value: cause,
      configurable: true,
      writable: true,
    });
  } catch {
    // Frozen errors and locked cause metadata must still leave the exact
    // primary object as the rejection.
  }
}

// ---------------------------------------------------------------------------
// EngineHandle
// ---------------------------------------------------------------------------

/**
 * Async wrapper around a synchronous Engine on a dedicated Worker thread.
 *
 * @example
 * ```ts
 * await using handle = await EngineHandle.create({
 *   source: "(defrule hello (initial-fact) => (printout t \"hi\" crlf))",
 * });
 * await handle.reset();
 * const result = await handle.run();
 * ```
 */
export class EngineHandle {
  private worker: Worker | null;
  private nextId = 0;
  private readonly pending = new Map<number, PendingEntry>();
  private closed = false;

  private readonly onWorkerMessage = (resp: WorkerResponse): void => {
    const entry = this.pending.get(resp.id);
    if (!entry) return;
    this.pending.delete(resp.id);

    if ("error" in resp) {
      entry.reject(reconstructError(resp.error));
    } else {
      entry.resolve(fromWire(resp.result, FerricSymbol));
    }
  };

  private readonly onWorkerError = (err: Error): void => {
    const snapshot = [...this.pending.values()];
    this.pending.clear();
    for (const entry of snapshot) {
      entry.reject(err);
    }
  };

  private readonly onWorkerExit = (code: number): void => {
    // Any pending request when the worker exits — even with code 0 —
    // will never be answered. Without this rejection those promises
    // would hang forever.
    if (this.pending.size > 0) {
      const err = new Error(
        code === 0
          ? "Worker exited before responding to pending request"
          : `Worker exited unexpectedly with code ${code}`,
      );
      const snapshot = [...this.pending.values()];
      this.pending.clear();
      for (const entry of snapshot) {
        entry.reject(err);
      }
    }
    this.worker = null;
  };

  private constructor(worker: Worker, attachListeners = true) {
    this.worker = worker;
    if (attachListeners) this.attachWorkerListeners(worker);
  }

  // ---------------------------------------------------------------------------
  // Factory
  // ---------------------------------------------------------------------------

  /**
   * Create an EngineHandle backed by a new Worker thread.
   *
   * The Engine is created on the worker's OS thread, satisfying thread affinity.
   * If `options.source` is provided, the source is loaded and reset() is called.
   * If `options.snapshot` is provided, the engine is restored from the snapshot.
   * If initialization fails after the Worker starts, create() awaits termination
   * before rejecting with the original error. A termination failure is attached
   * when cleanup can define or redefine an own writable/configurable cause on
   * the primary Error; errors that reject that update retain exact identity.
   */
  static async create(options?: EngineHandleOptions): Promise<EngineHandle> {
    const source = options?.source;
    const snapshot = options?.snapshot;

    // D-003: source and snapshot are mutually exclusive.
    if (source !== undefined && snapshot !== undefined) {
      throw new TypeError(
        "EngineHandle.create: 'source' and 'snapshot' are mutually exclusive"
      );
    }

    const init: WorkerInit = {
      options: options
        ? {
            strategy: options.strategy,
            encoding: options.encoding,
            maxCallDepth: options.maxCallDepth,
          }
        : undefined,
      source,
    };
    let transferList: readonly ArrayBuffer[] | undefined;

    if (snapshot) {
      // Transfer the ArrayBuffer for zero-copy.
      const ab = snapshot.data.buffer.slice(
        snapshot.data.byteOffset,
        snapshot.data.byteOffset + snapshot.data.byteLength,
      );
      init.snapshot = { data: ab as ArrayBuffer, format: snapshot.format };
      transferList = [ab as ArrayBuffer];
    }

    const workerPath = resolve(__dirname, "worker.js");
    const worker = new Worker(workerPath);
    // Keep factory ownership until the initialization acknowledgement arrives.
    // Delaying listener attachment until after construction lets every
    // post-spawn failure unwind through the same cleanup transaction.
    const handle = new EngineHandle(worker, false);

    try {
      handle.attachWorkerListeners(worker);
      await handle.initialize(init, transferList);
      return handle;
    } catch (error) {
      await handle.cleanupFailedCreate(worker, error);
      throw error;
    }
  }

  // ---------------------------------------------------------------------------
  // Internal helpers
  // ---------------------------------------------------------------------------

  private makePromise(id: number): Promise<unknown> {
    return new Promise<unknown>((resolve, reject) => {
      this.pending.set(id, { resolve, reject });
    });
  }

  /** Send the factory-only initialization request with atomic send rollback. */
  private initialize(
    init: WorkerInit,
    transferList?: readonly ArrayBuffer[],
  ): Promise<void> {
    const worker = this.worker;
    if (!worker) {
      return Promise.reject(new Error("Worker exited before initialization"));
    }

    const id = this.nextId++;
    const req: WorkerRequest = { id, method: "__init", args: [init] };

    return new Promise<void>((resolve, reject) => {
      const entry: PendingEntry = {
        resolve: () => resolve(),
        reject,
      };
      this.pending.set(id, entry);

      try {
        if (transferList) {
          worker.postMessage(req, transferList);
        } else {
          worker.postMessage(req);
        }
      } catch (error) {
        // postMessage may throw synchronously. Remove the bookkeeping before
        // rejecting the same Promise returned to create(), so no orphaned
        // pending request or unhandled rejection remains behind.
        if (this.pending.get(id) === entry) {
          this.pending.delete(id);
        }
        reject(error);
      }
    });
  }

  private attachWorkerListeners(worker: Worker): void {
    worker.on("message", this.onWorkerMessage);
    worker.on("error", this.onWorkerError);
    worker.on("exit", this.onWorkerExit);
  }

  private detachWorkerListeners(worker: Worker): void {
    worker.off("message", this.onWorkerMessage);
    worker.off("error", this.onWorkerError);
    worker.off("exit", this.onWorkerExit);
  }

  /** Release a Worker that was never published through a successful create(). */
  private async cleanupFailedCreate(worker: Worker, primary: unknown): Promise<void> {
    this.closed = true;

    const snapshot = [...this.pending.values()];
    this.pending.clear();
    for (const entry of snapshot) {
      entry.reject(primary);
    }

    let terminationFailed = false;
    let terminationFailure: unknown;
    try {
      await worker.terminate();
    } catch (error) {
      terminationFailed = true;
      terminationFailure = error;
    } finally {
      try {
        this.detachWorkerListeners(worker);
      } catch {
        // The primary initialization error must remain the rejection.
      }
      this.worker = null;
    }

    if (terminationFailed) {
      attachCreateCleanupCause(primary, terminationFailure);
    }
  }

  private call(method: string, args: unknown[]): Promise<unknown> {
    if (this.closed || !this.worker) {
      return Promise.reject(new Error("EngineHandle has been closed"));
    }
    const id = this.nextId++;
    const req: WorkerRequest = { id, method, args };
    const promise = this.makePromise(id);
    this.worker.postMessage(req);
    return promise;
  }

  // ---------------------------------------------------------------------------
  // Loading
  // ---------------------------------------------------------------------------

  /** Parse and compile CLIPS source into the engine. */
  async load(source: string): Promise<void> {
    await this.call("load", [source]);
  }

  /** Parse and compile CLIPS source from a file. */
  async loadFile(path: string): Promise<void> {
    await this.call("loadFile", [path]);
  }

  // ---------------------------------------------------------------------------
  // Fact operations
  // ---------------------------------------------------------------------------

  /**
   * Assert one or more facts from a CLIPS source string.
   * @returns Array of fact IDs for the asserted facts.
   */
  async assertString(source: string): Promise<FactId[]> {
    return this.call("assertString", [source]) as Promise<FactId[]>;
  }

  /**
   * Assert an ordered fact.
   * @returns The fact ID.
   */
  async assertFact(relation: string, ...fields: ClipsValue[]): Promise<FactId> {
    return this.call("assertFact", [relation, ...fields.map(toWire)]) as Promise<FactId>;
  }

  /**
   * Assert a template fact with named slots.
   * @returns The fact ID.
   */
  async assertTemplate(
    templateName: string,
    slots: Record<string, ClipsValue>,
  ): Promise<FactId> {
    return this.call("assertTemplate", [templateName, toWire(slots)]) as Promise<FactId>;
  }

  /** Retract a fact by ID. */
  async retract(factId: FactIdInput): Promise<void> {
    await this.call("retract", [factId]);
  }

  /** Get a snapshot of a single fact, or null if not found. */
  async getFact(factId: FactIdInput): Promise<Fact | null> {
    return this.call("getFact", [factId]) as Promise<Fact | null>;
  }

  /** Get snapshots of all user-visible facts. */
  async facts(): Promise<Fact[]> {
    return this.call("facts", []) as Promise<Fact[]>;
  }

  /** Get snapshots of facts matching a relation name. */
  async findFacts(relation: string): Promise<Fact[]> {
    return this.call("findFacts", [relation]) as Promise<Fact[]>;
  }

  // ---------------------------------------------------------------------------
  // Execution
  // ---------------------------------------------------------------------------

  /**
   * Run the engine to completion or until the limit is reached.
   *
   * Cancellation is cooperative: the worker checks a SharedArrayBuffer flag
   * between every RUN_BATCH_SIZE rule firings. One fresh native run starts the
   * logical run; later batches use continuation so exact-boundary halts and
   * diagnostics match synchronous execution. When the AbortSignal fires, the
   * worker stops submitting chunks and returns a partial RunResult with
   * HaltReason.HaltRequested without setting the native halt latch merely for
   * host cancellation. A zero limit still starts a fresh run and resets the
   * previous logical run's halt state and diagnostics while firing no rules.
   *
   * @param options.limit - Maximum rule firings. Omit for unlimited.
   * @param options.signal - AbortSignal for cancellation.
   */
  async run(options?: { limit?: number; signal?: AbortSignal }): Promise<RunResult> {
    if (this.closed || !this.worker) {
      throw new Error("EngineHandle has been closed");
    }

    const signal = options?.signal;
    const limit = normalizeRunLimit(
      (options as { limit?: unknown } | undefined)?.limit,
      "EngineHandle.run",
    );

    if (signal?.aborted) {
      throw new DOMException("The operation was aborted", "AbortError");
    }

    // Allocate a shared abort flag buffer for cooperative cancellation.
    const sab = new SharedArrayBuffer(ABORT_BUFFER_SIZE * Int32Array.BYTES_PER_ELEMENT);
    const abortFlag = new Int32Array(sab);

    const id = this.nextId++;
    const req: WorkerRequest = {
      id,
      method: "__run_batched",
      args: [limit ?? null, sab],
    };

    const promise = new Promise<RunResult>((resolve, reject) => {
      this.pending.set(id, {
        resolve: (val) => resolve(val as RunResult),
        reject,
      });
    });

    // Wire up cancellation BEFORE posting the message so there's no race.
    let onAbort: (() => void) | undefined;
    if (signal) {
      onAbort = () => {
        Atomics.store(abortFlag, ABORT_FLAG_INDEX, 1);
      };
      signal.addEventListener("abort", onAbort, { once: true });
    }

    this.worker.postMessage(req);

    try {
      return await promise;
    } finally {
      if (signal && onAbort) {
        signal.removeEventListener("abort", onAbort);
      }
    }
  }

  /**
   * Execute a single rule firing.
   * @returns The fired rule, or null if the agenda is empty.
   */
  async step(): Promise<FiredRule | null> {
    return this.call("step", []) as Promise<FiredRule | null>;
  }

  /** Request the engine to halt. Idempotent. */
  async halt(): Promise<void> {
    await this.call("halt", []);
  }

  /** Reset to initial state: clear facts, keep rules, re-assert deffacts. */
  async reset(): Promise<void> {
    await this.call("reset", []);
  }

  /** Remove all rules, facts, templates, and other constructs. */
  async clear(): Promise<void> {
    await this.call("clear", []);
  }

  // ---------------------------------------------------------------------------
  // Introspection
  // ---------------------------------------------------------------------------

  /** Number of user-visible facts. */
  async getFactCount(): Promise<number> {
    return this.call("getFactCount", []) as Promise<number>;
  }

  /** Whether the engine is in a halted state. */
  async getIsHalted(): Promise<boolean> {
    return this.call("getIsHalted", []) as Promise<boolean>;
  }

  /** Number of activations on the agenda. */
  async getAgendaSize(): Promise<number> {
    return this.call("getAgendaSize", []) as Promise<number>;
  }

  /** Name of the current module. */
  async getCurrentModule(): Promise<string> {
    return this.call("getCurrentModule", []) as Promise<string>;
  }

  /** Module at the top of the focus stack, or null if empty. */
  async getFocus(): Promise<string | null> {
    return this.call("getFocus", []) as Promise<string | null>;
  }

  /** Focus stack entries from bottom to top. */
  async getFocusStack(): Promise<string[]> {
    return this.call("getFocusStack", []) as Promise<string[]>;
  }

  /** All registered rules with their salience values. */
  async rules(): Promise<RuleInfo[]> {
    return this.call("rules", []) as Promise<RuleInfo[]>;
  }

  /** Names of all registered templates. */
  async templates(): Promise<string[]> {
    return this.call("templates", []) as Promise<string[]>;
  }

  /** All known module names. */
  async modules(): Promise<string[]> {
    return this.call("modules", []) as Promise<string[]>;
  }

  /**
   * Get a global variable's value.
   * @param name Variable name without the `?*` prefix/suffix.
   * @returns The value, or null if not found/visible in current module context.
   */
  async getGlobal(name: string): Promise<ClipsValue | null> {
    return this.call("getGlobal", [name]) as Promise<ClipsValue | null>;
  }

  // ---------------------------------------------------------------------------
  // I/O
  // ---------------------------------------------------------------------------

  /**
   * Get captured output for a named CLIPS channel (e.g. "t" or "stderr").
   * @returns The output string, or null if no output.
   */
  async getOutput(channel: string): Promise<string | null> {
    return this.call("getOutput", [channel]) as Promise<string | null>;
  }

  /** Clear a specific output channel. */
  async clearOutput(channel: string): Promise<void> {
    await this.call("clearOutput", [channel]);
  }

  /** Push an input line for read/readline functions. */
  async pushInput(line: string): Promise<void> {
    await this.call("pushInput", [line]);
  }

  // ---------------------------------------------------------------------------
  // Serialization
  // ---------------------------------------------------------------------------

  /**
   * Serialize the engine's current state.
   * @param format Serialization format. Default: Bincode.
   * @returns A Buffer containing the snapshot.
   */
  async serialize(format?: Format): Promise<Buffer> {
    const result = await this.call("serialize", [format]);
    // Worker transfers the ArrayBuffer for zero-copy.
    if (result instanceof ArrayBuffer) {
      return Buffer.from(result);
    }
    return result as Buffer;
  }

  // ---------------------------------------------------------------------------
  // Lifecycle
  // ---------------------------------------------------------------------------

  /**
   * Terminate the worker thread and release all resources.
   * In-flight operations will reject with an error.
   * Idempotent — safe to call multiple times.
   */
  async close(): Promise<void> {
    if (this.closed) return;
    this.closed = true;

    const snapshot = [...this.pending.values()];
    this.pending.clear();

    if (this.worker) {
      await this.worker.terminate();
      this.worker = null;
    }

    const closedErr = new Error("EngineHandle closed");
    for (const entry of snapshot) {
      entry.reject(closedErr);
    }
  }

  /**
   * Async dispose for `await using handle = await EngineHandle.create(...)`.
   * Requires TypeScript 5.2+ with `useDefineForClassFields` and Node.js 22+.
   */
  async [Symbol.asyncDispose](): Promise<void> {
    await this.close();
  }
}
