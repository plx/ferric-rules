/**
 * Shared TypeScript types for the Ferric rules engine bindings.
 *
 * These mirror the types exposed by the native napi-rs addon and are
 * used across the synchronous Engine, EngineHandle, and EnginePool APIs.
 */

// ---------------------------------------------------------------------------
// Value types
// ---------------------------------------------------------------------------

/**
 * Wire-form representation of a FerricSymbol, used when native class
 * instances cannot cross postMessage boundaries (structured-clone).
 *
 * The native FerricSymbol class is used for direct Engine calls.
 * Over postMessage, symbols appear as this tagged object.
 */
export interface WireSymbolObject {
  __type: "FerricSymbol";
  value: string;
}

/**
 * Structural interface matching native FerricSymbol instances.
 *
 * This allows `ClipsValue` to accept FerricSymbol objects in type-checked
 * code without importing the native module (which may not be available
 * during pure type-checking).
 */
export interface FerricSymbolInstance {
  readonly value: string;
  toString(): string;
  valueOf(): string;
}

/**
 * Union of all value types that can appear in CLIPS facts and expressions.
 *
 * Conversion rules (JS → CLIPS):
 *   FerricSymbol / WireSymbolObject  → CLIPS symbol
 *   string                           → CLIPS string (quoted)
 *   number (integer)                 → CLIPS integer (`Number.isInteger` check)
 *   number (float)                   → CLIPS float
 *   bigint                           → CLIPS integer (for values outside safe-integer range)
 *   boolean                          → CLIPS symbol TRUE / FALSE
 *   ClipsValue[]                     → CLIPS multifield
 *   null                             → CLIPS void
 *
 * Conversion rules (CLIPS → JS):
 *   CLIPS symbol    → FerricSymbol (native) or WireSymbolObject (across postMessage)
 *   CLIPS string    → string
 *   CLIPS integer   → number (if within safe-integer range) or bigint
 *   CLIPS float     → number
 *   CLIPS multifield → ClipsValue[]
 *   CLIPS void      → null
 */
export type ClipsValue =
  | FerricSymbolInstance
  | WireSymbolObject
  | string
  | number
  | bigint
  | boolean
  | ClipsValue[]
  | null;

// ---------------------------------------------------------------------------
// Enums
// ---------------------------------------------------------------------------

/** Conflict resolution strategy for the agenda. */
export enum Strategy {
  Depth = 0,
  Breadth = 1,
  Lex = 2,
  Mea = 3,
}

/** String encoding mode for the engine. */
export enum Encoding {
  Ascii = 0,
  Utf8 = 1,
  AsciiSymbolsUtf8Strings = 2,
}

/** Reason the engine's run loop terminated. */
export enum HaltReason {
  AgendaEmpty = 0,
  LimitReached = 1,
  HaltRequested = 2,
}

/** Discriminates ordered vs. template facts. */
export enum FactType {
  Ordered = 0,
  Template = 1,
}

/** Serialization format for engine snapshots. */
export enum Format {
  Bincode = 0,
  Json = 1,
  Cbor = 2,
  MessagePack = 3,
  Postcard = 4,
}

// ---------------------------------------------------------------------------
// Result types
// ---------------------------------------------------------------------------

/** Result returned from Engine.run(). */
export interface RunResult {
  readonly rulesFired: number;
  readonly haltReason: HaltReason;
}

/** Identifies a rule that was fired during Engine.step(). */
export interface FiredRule {
  readonly ruleName: string;
}

/** Name and salience of a registered rule. */
export interface RuleInfo {
  readonly name: string;
  readonly salience: number;
}

/** Snapshot of a single fact. */
export interface Fact {
  readonly id: number;
  readonly type: FactType;
  /** Relation name (ordered facts only). */
  readonly relation?: string;
  /** Template name (template facts only). */
  readonly templateName?: string;
  /** Positional field values. */
  readonly fields: readonly ClipsValue[];
  /** Named slot values (template facts only). */
  readonly slots?: Readonly<Record<string, ClipsValue>>;
}

// ---------------------------------------------------------------------------
// Configuration
// ---------------------------------------------------------------------------

/** Options for creating an Engine or EngineHandle. */
export interface EngineOptions {
  /** Conflict resolution strategy. Default: Depth. */
  strategy?: Strategy;
  /** String encoding mode. Default: Utf8. */
  encoding?: Encoding;
  /** Maximum function call depth. Default: 64. */
  maxCallDepth?: number;
}

/** Options for EngineHandle.create(). */
export interface EngineHandleOptions extends EngineOptions {
  /** CLIPS source to load at creation (load + reset). */
  source?: string;
  /** Snapshot to restore from (mutually exclusive with source). */
  snapshot?: { data: Buffer; format?: Format };
}

// ---------------------------------------------------------------------------
// Pool types
// ---------------------------------------------------------------------------

/** Named engine configuration for use in an EnginePool. */
export interface EngineSpec {
  name: string;
  options?: EngineOptions;
  /** CLIPS source to load at creation. */
  source?: string;
}

/** Request for EnginePool.evaluate(). */
export interface EvaluateRequest {
  /** Facts to assert after reset. */
  facts?: Array<
    | { kind: "ordered"; relation: string; fields: ClipsValue[] }
    | { kind: "template"; templateName: string; slots: Record<string, ClipsValue> }
  >;
  /** Maximum rule firings. 0 or omit for unlimited. */
  limit?: number;
}

/** Result from EnginePool.evaluate(). */
export interface EvaluateResult {
  readonly runResult: RunResult;
  readonly facts: readonly Fact[];
  /**
   * Captured output mapped to user-friendly keys.
   * "stdout" maps to the CLIPS "t" channel.
   * "stderr" maps to the CLIPS "stderr" channel.
   */
  readonly output: Readonly<Record<string, string>>;
}

// ---------------------------------------------------------------------------
// Error hierarchy
// ---------------------------------------------------------------------------

/** Base class for all Ferric engine errors. */
export class FerricError extends Error {
  readonly code: string;

  constructor(message: string, code: string) {
    super(message);
    this.name = "FerricError";
    this.code = code;
    // Maintain proper prototype chain when transpiling to ES5
    Object.setPrototypeOf(this, new.target.prototype);
  }
}

/** A CLIPS source string failed to parse. */
export class FerricParseError extends FerricError {
  constructor(message: string) {
    super(message, "FERRIC_PARSE_ERROR");
    this.name = "FerricParseError";
    Object.setPrototypeOf(this, new.target.prototype);
  }
}

/** A parsed construct failed to compile into the Rete network. */
export class FerricCompileError extends FerricError {
  constructor(message: string) {
    super(message, "FERRIC_COMPILE_ERROR");
    this.name = "FerricCompileError";
    Object.setPrototypeOf(this, new.target.prototype);
  }
}

/** An error occurred during rule execution. */
export class FerricRuntimeError extends FerricError {
  constructor(message: string) {
    super(message, "FERRIC_RUNTIME_ERROR");
    this.name = "FerricRuntimeError";
    Object.setPrototypeOf(this, new.target.prototype);
  }
}

/** The requested fact ID does not exist. */
export class FerricFactNotFoundError extends FerricError {
  constructor(message: string) {
    super(message, "FERRIC_FACT_NOT_FOUND");
    this.name = "FerricFactNotFoundError";
    Object.setPrototypeOf(this, new.target.prototype);
  }
}

/** The requested template name is not registered. */
export class FerricTemplateNotFoundError extends FerricError {
  constructor(message: string) {
    super(message, "FERRIC_TEMPLATE_NOT_FOUND");
    this.name = "FerricTemplateNotFoundError";
    Object.setPrototypeOf(this, new.target.prototype);
  }
}

/** The requested slot name does not exist on the template. */
export class FerricSlotNotFoundError extends FerricError {
  constructor(message: string) {
    super(message, "FERRIC_SLOT_NOT_FOUND");
    this.name = "FerricSlotNotFoundError";
    Object.setPrototypeOf(this, new.target.prototype);
  }
}

/** The requested module name is not registered. */
export class FerricModuleNotFoundError extends FerricError {
  constructor(message: string) {
    super(message, "FERRIC_MODULE_NOT_FOUND");
    this.name = "FerricModuleNotFoundError";
    Object.setPrototypeOf(this, new.target.prototype);
  }
}

/** A string encoding constraint was violated. */
export class FerricEncodingError extends FerricError {
  constructor(message: string) {
    super(message, "FERRIC_ENCODING_ERROR");
    this.name = "FerricEncodingError";
    Object.setPrototypeOf(this, new.target.prototype);
  }
}

/** Serialization or deserialization of an engine snapshot failed. */
export class FerricSerializationError extends FerricError {
  constructor(message: string) {
    super(message, "FERRIC_SERIALIZATION_ERROR");
    this.name = "FerricSerializationError";
    Object.setPrototypeOf(this, new.target.prototype);
  }
}

/**
 * Factory for a FerricError subclass from a worker error payload's message.
 *
 * Most entries are the subclass constructors themselves (which take a single
 * message argument), but the base `FerricError` needs a factory because its
 * constructor also takes a code parameter. Consumers should treat every
 * entry uniformly as a factory and invoke it as a plain call — not with
 * `new`.
 */
type FerricErrorFactory = (message: string) => FerricError;

/**
 * Map from error name strings (as returned by the worker) to factories
 * that produce the correct FerricError subclass from a WorkerResponse.error
 * payload.
 */
export const ERROR_REGISTRY: Readonly<Record<string, FerricErrorFactory>> = {
  FerricError: (msg) => new FerricError(msg, "FERRIC_ERROR"),
  FerricParseError: (msg) => new FerricParseError(msg),
  FerricCompileError: (msg) => new FerricCompileError(msg),
  FerricRuntimeError: (msg) => new FerricRuntimeError(msg),
  FerricFactNotFoundError: (msg) => new FerricFactNotFoundError(msg),
  FerricTemplateNotFoundError: (msg) => new FerricTemplateNotFoundError(msg),
  FerricSlotNotFoundError: (msg) => new FerricSlotNotFoundError(msg),
  FerricModuleNotFoundError: (msg) => new FerricModuleNotFoundError(msg),
  FerricEncodingError: (msg) => new FerricEncodingError(msg),
  FerricSerializationError: (msg) => new FerricSerializationError(msg),
};

/**
 * Convert a native napi-rs error into the correct FerricError subclass.
 *
 * napi-rs errors embed the class name as a prefix in the message:
 * "FerricParseError: parse error: ...". This function extracts the
 * class name, constructs the correct subclass, and returns it.
 */
export function convertNativeError(err: unknown): Error {
  if (!(err instanceof Error)) return new Error(String(err));

  const match = err.message.match(/^(Ferric\w+Error):\s*/);
  if (match) {
    const name = match[1];
    const cleanMessage = err.message.slice(match[0].length);
    const make = ERROR_REGISTRY[name];
    if (make) {
      return make(cleanMessage);
    }
  }

  return err;
}
