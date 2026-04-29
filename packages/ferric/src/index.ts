/**
 * @ferric-rules/node — TypeScript bindings for the Ferric rules engine.
 *
 * ## Layers
 *
 * - **Engine** (native, synchronous): Direct wrapper around the Rust engine.
 *   All methods are synchronous and execute on the calling thread.
 *   Import from this package; the native addon is loaded automatically.
 *
 * - **EngineHandle** (async, worker-backed): Wraps one Engine on a dedicated
 *   Worker thread. All methods return Promises. Suitable for servers.
 *
 * - **EnginePool** (concurrent): Multiple Worker threads for parallel evaluation.
 *   The primary entry point is `pool.evaluate(specName, request)`.
 */

// ---------------------------------------------------------------------------
// Core value types and enums
// ---------------------------------------------------------------------------

export type {
  ClipsValue,
  WireSymbolObject,
  RunResult,
  FiredRule,
  RuleInfo,
  Fact,
  EngineOptions,
  EngineHandleOptions,
  EngineSpec,
  EvaluateRequest,
  EvaluateResult,
} from "./types";

export {
  Strategy,
  Encoding,
  HaltReason,
  FactType,
  Format,
  FerricError,
  FerricParseError,
  FerricCompileError,
  FerricRuntimeError,
  FerricFactNotFoundError,
  FerricTemplateNotFoundError,
  FerricSlotNotFoundError,
  FerricModuleNotFoundError,
  FerricEncodingError,
  FerricSerializationError,
  ERROR_REGISTRY,
} from "./types";

// ---------------------------------------------------------------------------
// Native engine and symbol
// ---------------------------------------------------------------------------

export {
  Engine,
  FerricSymbol,
} from "./native";

export type {
  NativeEngine,
  NativeEngineConstructor,
  NativeFerricSymbol,
  NativeFerricSymbolConstructor,
} from "./native";

// ---------------------------------------------------------------------------
// Async wrappers
// ---------------------------------------------------------------------------

export { EngineHandle } from "./engine-handle";

export { EnginePool } from "./engine-pool";

export type { EngineProxy } from "./engine-pool";

// ---------------------------------------------------------------------------
// Wire utilities (advanced use)
// ---------------------------------------------------------------------------

export {
  isWireSymbol,
  toWire,
  fromWire,
  ABORT_FLAG_INDEX,
  ABORT_BUFFER_SIZE,
  RUN_BATCH_SIZE,
} from "./wire";

export type {
  WireSymbol,
  WorkerRequest,
  WorkerResponse,
  WorkerErrorPayload,
  WorkerInit,
  PoolWorkerInit,
} from "./wire";
