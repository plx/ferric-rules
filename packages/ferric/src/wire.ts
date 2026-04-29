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
// Error name/code extraction from napi-rs error messages
// ---------------------------------------------------------------------------

/**
 * Known Ferric error class names and their stable error codes.
 * Used to extract the error class from napi-rs error messages which
 * embed the class name as a prefix: "FerricXxxError: actual message".
 */
export const FERRIC_ERROR_CODES: Readonly<Record<string, string>> = {
  FerricError: "FERRIC_ERROR",
  FerricParseError: "FERRIC_PARSE_ERROR",
  FerricCompileError: "FERRIC_COMPILE_ERROR",
  FerricRuntimeError: "FERRIC_RUNTIME_ERROR",
  FerricFactNotFoundError: "FERRIC_FACT_NOT_FOUND",
  FerricTemplateNotFoundError: "FERRIC_TEMPLATE_NOT_FOUND",
  FerricSlotNotFoundError: "FERRIC_SLOT_NOT_FOUND",
  FerricModuleNotFoundError: "FERRIC_MODULE_NOT_FOUND",
  FerricEncodingError: "FERRIC_ENCODING_ERROR",
  FerricSerializationError: "FERRIC_SERIALIZATION_ERROR",
};

/**
 * Extract the Ferric error class name and clean message from a napi-rs
 * error message. The Rust error.rs module prefixes messages with the
 * class name, e.g. "FerricParseError: parse error: ...".
 */
export function extractFerricError(
  errorName: string,
  errorMessage: string,
  errorCode?: string,
): { name: string; message: string; code: string } {
  // Try to extract the class name from the message prefix.
  const match = errorMessage.match(/^(Ferric\w+Error):\s*/);
  if (match) {
    const name = match[1];
    const cleanMessage = errorMessage.slice(match[0].length);
    return {
      name,
      message: cleanMessage,
      code: FERRIC_ERROR_CODES[name] ?? "FERRIC_ERROR",
    };
  }

  // No Ferric prefix — use the original error name.
  return {
    name: errorName,
    message: errorMessage,
    code: errorCode ?? "FERRIC_ERROR",
  };
}

// ---------------------------------------------------------------------------
// Tagged FerricSymbol wire form
// ---------------------------------------------------------------------------

/**
 * Wire representation of a FerricSymbol.
 * Used when serializing values across postMessage (structured clone).
 *
 * NOTE: This `{ __type: "FerricSymbol", value: string }` shape is the
 * postMessage wire form used only between the main thread and worker
 * threads. It is DISTINCT from the native-call marshal form
 * `{ __ferric_symbol: true, value: string }` defined by
 * `crates/ferric-napi/index.js::marshalValue` and consumed by the Rust
 * side in `crates/ferric-napi/src/value.rs`. The two formats never mix:
 * workers always convert wire symbols back into native FerricSymbol
 * instances (see `fromWireToNative`) before invoking the engine.
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
 * Recursively reconstruct wire-format FerricSymbol tagged objects back into
 * live class instances using the supplied constructor.
 *
 * Used inside worker threads to rehydrate the native napi `FerricSymbol`
 * class before passing values into the engine (the engine only accepts the
 * real class, not the wire tagged object).
 *
 * Compared with {@link fromWire}, this helper always rehydrates symbols and
 * does not short-circuit on non-plain objects — callers guarantee the value
 * came through structured clone, so all objects are plain.
 */
export function fromWireToNative(
  val: unknown,
  FerricSymbolCtor: new (value: string) => unknown,
): unknown {
  if (val === null || val === undefined) return val;
  if (typeof val !== "object") return val;

  if (isWireSymbol(val)) {
    return new FerricSymbolCtor(val.value);
  }

  if (Array.isArray(val)) {
    return val.map((v) => fromWireToNative(v, FerricSymbolCtor));
  }

  const result: Record<string, unknown> = {};
  for (const [k, v] of Object.entries(val)) {
    result[k] = fromWireToNative(v, FerricSymbolCtor);
  }
  return result;
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

    // Skip non-plain objects like Buffer, ArrayBuffer, etc.
    // These should pass through unchanged.
    const proto = Object.getPrototypeOf(val);
    if (proto !== Object.prototype && proto !== null) {
      return val;
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
