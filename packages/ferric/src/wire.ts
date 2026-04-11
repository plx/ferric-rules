/**
 * Wire protocol types and serialization helpers for postMessage communication
 * between the main thread and worker threads.
 *
 * Values like FerricSymbol are class instances on their own thread but must
 * be serialized as plain tagged objects to survive structured-clone across
 * the thread boundary. This module handles that transparently.
 */

// ---------------------------------------------------------------------------
// Request / response protocol
// ---------------------------------------------------------------------------

/**
 * A request sent from the main thread to a worker.
 * The `id` is monotonically increasing and used to match responses.
 */
export interface WorkerRequest {
  /** Monotonically increasing request ID. */
  id: number;
  /** Engine method name to invoke, or "__init". */
  method: string;
  /** Structured-clonable arguments for the method. */
  args: unknown[];
}

/**
 * A response sent from a worker to the main thread.
 * Exactly one of `result` or `error` will be present.
 */
export interface WorkerResponse {
  /** Matches the corresponding WorkerRequest.id. */
  id: number;
  /** Return value on success. */
  result?: unknown;
  /** Error details on failure. */
  error?: WorkerErrorPayload;
}

/** Structured error payload carried in WorkerResponse. */
export interface WorkerErrorPayload {
  /** Error class name (e.g. "FerricParseError") for reconstruction. */
  name: string;
  /** Human-readable error message. */
  message: string;
  /** Error code string (e.g. "FERRIC_PARSE_ERROR"). */
  code: string;
}

// ---------------------------------------------------------------------------
// Init message
// ---------------------------------------------------------------------------

/**
 * Initialization payload carried as args[0] in the "__init" request.
 * Sent by the main thread immediately after spawning a worker.
 */
export interface WorkerInit {
  /** Engine construction options. */
  options?: {
    strategy?: number;
    encoding?: number;
    maxCallDepth?: number;
  };
  /** CLIPS source to load and reset after creating the engine. */
  source?: string;
  /**
   * Snapshot to restore from, as an ArrayBuffer (zero-copy transfer).
   * Mutually exclusive with `source`.
   */
  snapshot?: { data: ArrayBuffer; format?: number };
}

/**
 * Initialization payload for pool workers, which manage multiple named engines.
 */
export interface PoolWorkerInit {
  /** Engine specs keyed by name. */
  specs: Array<{
    name: string;
    options?: { strategy?: number; encoding?: number; maxCallDepth?: number };
    source?: string;
  }>;
}

// ---------------------------------------------------------------------------
// Abort-flag layout for cooperative cancellation
// ---------------------------------------------------------------------------

/**
 * Index into the SharedArrayBuffer used for cooperative cancellation.
 * The worker checks Atomics.load(buf, ABORT_FLAG_INDEX) between batches.
 * The main thread calls Atomics.store(buf, ABORT_FLAG_INDEX, 1) to request halt.
 */
export const ABORT_FLAG_INDEX = 0;

/** Size (in Int32 elements) of the shared abort buffer. */
export const ABORT_BUFFER_SIZE = 1;

/**
 * Batch size (rule firings) between cooperative cancellation checks in run().
 * This matches the Go implementation's batch size.
 */
export const RUN_BATCH_SIZE = 100;

// ---------------------------------------------------------------------------
// Tagged FerricSymbol wire form
// ---------------------------------------------------------------------------

/**
 * Wire representation of a FerricSymbol.
 * Used when serializing values across postMessage (structured clone).
 */
export interface WireSymbol {
  __type: "FerricSymbol";
  value: string;
}

/**
 * Type guard: returns true if `val` is a wire-format FerricSymbol.
 */
export function isWireSymbol(val: unknown): val is WireSymbol {
  return (
    typeof val === "object" &&
    val !== null &&
    (val as WireSymbol).__type === "FerricSymbol" &&
    typeof (val as WireSymbol).value === "string"
  );
}

// ---------------------------------------------------------------------------
// Serialization helpers
// ---------------------------------------------------------------------------

/**
 * Convert a value (potentially containing native FerricSymbol class instances)
 * to a structured-clonable wire value for postMessage.
 *
 * Handles:
 * - Native FerricSymbol instances → WireSymbol tagged object
 * - Arrays → recursively converted
 * - Plain objects → recursively converted (e.g. Fact.slots)
 * - Primitives → unchanged
 */
export function toWire(val: unknown): unknown {
  if (val === null || val === undefined) {
    return null;
  }

  if (
    typeof val === "string" ||
    typeof val === "number" ||
    typeof val === "boolean" ||
    typeof val === "bigint"
  ) {
    return val;
  }

  if (Array.isArray(val)) {
    return val.map(toWire);
  }

  if (typeof val === "object") {
    // Detect native FerricSymbol instances by constructor name.
    // This avoids importing the native addon here (which may not exist at
    // type-check time) while still correctly identifying the class.
    const ctorName = (val as object).constructor?.name;
    if (ctorName === "FerricSymbol" && typeof (val as { value?: unknown }).value === "string") {
      return {
        __type: "FerricSymbol",
        value: (val as { value: string }).value,
      } satisfies WireSymbol;
    }

    // Already a wire symbol — pass through unchanged.
    if (isWireSymbol(val)) {
      return val;
    }

    // Plain object (e.g., Fact, slots record) — convert values recursively.
    const result: Record<string, unknown> = {};
    for (const [k, v] of Object.entries(val)) {
      result[k] = toWire(v);
    }
    return result;
  }

  return val;
}

/**
 * Convert a wire-format value back into its canonical form.
 *
 * Reconstructs WireSymbol tagged objects back into native FerricSymbol
 * instances so that callers of EngineHandle/EnginePool see proper
 * FerricSymbol values, not raw tagged objects.
 *
 * The FerricSymbol constructor is passed in to avoid a circular import
 * with native.ts (which may not be loadable at type-check time).
 */
export function fromWire(
  val: unknown,
  FerricSymbolCtor?: new (value: string) => unknown,
): unknown {
  if (val === null || val === undefined) return val;

  if (
    typeof val === "string" ||
    typeof val === "number" ||
    typeof val === "boolean" ||
    typeof val === "bigint"
  ) {
    return val;
  }

  if (Array.isArray(val)) {
    return val.map((v) => fromWire(v, FerricSymbolCtor));
  }

  if (typeof val === "object") {
    // Reconstruct WireSymbol back to native FerricSymbol.
    if (isWireSymbol(val) && FerricSymbolCtor) {
      return new FerricSymbolCtor(val.value);
    }

    // Recursively convert plain objects (e.g. Fact, slots).
    const result: Record<string, unknown> = {};
    for (const [k, v] of Object.entries(val)) {
      result[k] = fromWire(v, FerricSymbolCtor);
    }
    return result;
  }

  return val;
}
