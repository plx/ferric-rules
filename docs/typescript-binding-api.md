# TypeScript Binding API for Ferric

## Purpose

Define a TypeScript-native API for ferric-rules that:

1. Feels natural in Node.js and TypeScript (Promises, `AbortSignal`, `using`, iterators).
2. Preserves Ferric's thread-affine engine contract.
3. Provides both a synchronous low-level API and an async worker-backed API for non-blocking use.
4. Implements via [napi-rs](https://napi.rs), linking directly to Rust — no C FFI hop.

## Thread Affinity in Node.js

The Ferric engine is thread-affine: it must be used only on the OS thread that created it. Node.js has a single main thread for JavaScript execution, which makes simple usage straightforward — a synchronous `Engine` created on the main thread naturally satisfies the contract.

The challenge arises when:

1. **Long-running `run()` blocks the event loop.** A complex ruleset may fire thousands of rules synchronously.
2. **Worker threads** (`node:worker_threads`) each have their own V8 isolate and OS thread.

The design addresses both via a two-layer architecture.

## Architecture

### Layer 1: `Engine` (native, napi-rs)

A synchronous class exported from the native addon. All methods execute on the calling thread. This is the only layer that touches Rust code.

Created via a new Rust crate (`ferric-napi`) that depends on the `ferric` facade crate directly — no FFI indirection. napi-rs handles the JS ↔ Rust boundary.

### Layer 2: `EngineHandle` / `EnginePool` (pure TypeScript)

Async wrappers that run `Engine` instances inside dedicated `Worker` threads. Communication is via structured-clone `postMessage`. These are shipped as TypeScript alongside the native addon.

This separation means:

- The native addon is simple and stateless beyond the Engine itself.
- Async orchestration, cancellation, and pooling are in TypeScript where they're easy to test, debug, and extend.
- Worker threads each create their own `Engine` on their own OS thread, satisfying thread affinity automatically.

## Crate Structure

```
crates/ferric-napi/
├── Cargo.toml          # depends on ferric, napi, napi-derive
├── src/
│   ├── lib.rs          # #[napi] module registration
│   ├── engine.rs       # Engine class
│   ├── fact.rs         # Fact, FactType
│   ├── value.rs        # FerricSymbol, value conversion
│   ├── result.rs       # RunResult, HaltReason, FiredRule, RuleInfo
│   ├── config.rs       # Strategy, Encoding, Format enums
│   └── error.rs        # Error types
└── npm/                # platform-specific npm packages (napi-rs convention)
```

The published npm package (`@ferric-rules/node` or `ferric`) includes:

- Platform-specific native binaries (via napi-rs's `@ferric-rules/node-{platform}-{arch}` packages).
- TypeScript source and declarations for `EngineHandle`, `EnginePool`, wire types.
- A re-export barrel that exposes both the native `Engine` and the async wrappers.

## Public API

### Value Types

```typescript
/**
 * CLIPS symbol — a distinct value type from string.
 * Symbols are unquoted identifiers (e.g., TRUE, FALSE, foo).
 */
export class FerricSymbol {
  constructor(value: string);
  readonly value: string;
  toString(): string;
  /** Symbols with the same value are equal. */
  valueOf(): string;
}

/**
 * Union of all value types that can appear in CLIPS facts and expressions.
 *
 * Conversion rules (JS → CLIPS):
 *   FerricSymbol    → CLIPS symbol
 *   string          → CLIPS string (quoted)
 *   number          → CLIPS integer (if Number.isInteger) or float
 *   boolean         → CLIPS symbol TRUE / FALSE
 *   bigint          → CLIPS integer (for values outside safe-integer range)
 *   ClipsValue[]    → CLIPS multifield
 *   null/undefined  → CLIPS void
 *
 * Conversion rules (CLIPS → JS):
 *   CLIPS symbol    → FerricSymbol
 *   CLIPS string    → string
 *   CLIPS integer   → number (if within safe-integer range) or bigint
 *   CLIPS float     → number
 *   CLIPS multifield → ClipsValue[]
 *   CLIPS void      → null
 */
export type ClipsValue =
  | FerricSymbol
  | string
  | number
  | bigint
  | boolean
  | ClipsValue[]
  | null;
```

Note: unlike the Python binding, plain `string` maps to a CLIPS *string* (quoted), not a symbol. This matches Go's behavior and avoids a common footgun — CLIPS symbols should be explicitly constructed via `new FerricSymbol("foo")`. Booleans are a convenience that maps to `TRUE`/`FALSE` symbols.

### Enums

```typescript
export enum Strategy {
  Depth = 0,
  Breadth = 1,
  Lex = 2,
  Mea = 3,
}

export enum Encoding {
  Ascii = 0,
  Utf8 = 1,
  AsciiSymbolsUtf8Strings = 2,
}

export enum HaltReason {
  AgendaEmpty = 0,
  LimitReached = 1,
  HaltRequested = 2,
}

export enum FactType {
  Ordered = 0,
  Template = 1,
}

export enum Format {
  Bincode = 0,
  Json = 1,
  Cbor = 2,
  MessagePack = 3,
  Postcard = 4,
}
```

### Result Types

```typescript
export interface RunResult {
  readonly rulesFired: number;
  readonly haltReason: HaltReason;
}

export interface FiredRule {
  readonly ruleName: string;
}

export interface RuleInfo {
  readonly name: string;
  readonly salience: number;
}

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
```

### Configuration

```typescript
export interface EngineOptions {
  /** Conflict resolution strategy. Default: Depth. */
  strategy?: Strategy;
  /** String encoding mode. Default: Utf8. */
  encoding?: Encoding;
  /** Maximum function call depth. Default: 64. */
  maxCallDepth?: number;
}
```

### Error Hierarchy

```typescript
export class FerricError extends Error {
  readonly code: string;
}

export class FerricParseError extends FerricError {}
export class FerricCompileError extends FerricError {}
export class FerricRuntimeError extends FerricError {}
export class FerricFactNotFoundError extends FerricError {}
export class FerricTemplateNotFoundError extends FerricError {}
export class FerricSlotNotFoundError extends FerricError {}
export class FerricModuleNotFoundError extends FerricError {}
export class FerricEncodingError extends FerricError {}
export class FerricSerializationError extends FerricError {}
```

### Engine (synchronous, native)

The synchronous `Engine` is the core building block. All methods are synchronous and execute on the calling thread. It is suitable for scripts, CLI tools, short-lived evaluations, and as the backing implementation inside worker threads.

```typescript
export class Engine {
  /**
   * Create a new engine.
   * @throws {FerricError} if engine creation fails.
   */
  constructor(options?: EngineOptions);

  /**
   * Create an engine with CLIPS source pre-loaded and reset.
   * Equivalent to: new Engine(options) → load(source) → reset().
   */
  static fromSource(source: string, options?: EngineOptions): Engine;

  /**
   * Restore an engine from a serialized snapshot.
   * Skips parsing and compilation for fast instantiation.
   */
  static fromSnapshot(data: Buffer, format?: Format): Engine;

  /**
   * Restore an engine from a snapshot file.
   */
  static fromSnapshotFile(path: string, format?: Format): Engine;

  // --- Loading ---

  /** Parse and compile CLIPS source into the engine. */
  load(source: string): void;

  /** Parse and compile CLIPS source from a file. */
  loadFile(path: string): void;

  // --- Fact Operations ---

  /**
   * Assert one or more facts from a CLIPS source string.
   * @returns Array of fact IDs for the asserted facts.
   * @example engine.assertString("(color red) (color blue)")
   */
  assertString(source: string): number[];

  /**
   * Assert an ordered fact.
   * @returns The fact ID.
   * @example engine.assertFact("color", new FerricSymbol("red"))
   */
  assertFact(relation: string, ...fields: ClipsValue[]): number;

  /**
   * Assert a template fact with named slots.
   * @returns The fact ID.
   * @example engine.assertTemplate("person", { name: "Alice", age: 30 })
   */
  assertTemplate(
    templateName: string,
    slots: Record<string, ClipsValue>,
  ): number;

  /** Retract a fact by ID. */
  retract(factId: number): void;

  /** Get a snapshot of a single fact, or null if not found. */
  getFact(factId: number): Fact | null;

  /** Get snapshots of all user-visible facts. */
  facts(): Fact[];

  /** Get snapshots of facts matching a relation name. */
  findFacts(relation: string): Fact[];

  /** Get a template fact's slot value by name. */
  getFactSlot(factId: number, slotName: string): ClipsValue;

  // --- Execution ---

  /**
   * Run the engine to completion or until the limit is reached.
   * @param limit Maximum rule firings. Omit or pass undefined for unlimited.
   * @returns Result with number of rules fired and halt reason.
   */
  run(limit?: number): RunResult;

  /**
   * Execute a single rule firing.
   * @returns The fired rule, or null if the agenda is empty.
   */
  step(): FiredRule | null;

  /** Request the engine to halt. Idempotent. */
  halt(): void;

  /** Reset to initial state: clear facts, keep rules, re-assert deffacts. */
  reset(): void;

  /** Remove all rules, facts, templates, and other constructs. */
  clear(): void;

  // --- Introspection ---

  /** Number of user-visible facts. */
  get factCount(): number;

  /** Whether the engine is in a halted state. */
  get isHalted(): boolean;

  /** Number of activations on the agenda. */
  get agendaSize(): number;

  /** Name of the current module. */
  get currentModule(): string;

  /** Module at the top of the focus stack, or null if empty. */
  get focus(): string | null;

  /** Focus stack entries from bottom to top. */
  get focusStack(): string[];

  /** All registered rules with their salience values. */
  rules(): RuleInfo[];

  /** Names of all registered templates. */
  templates(): string[];

  /** All known module names. */
  modules(): string[];

  /**
   * Get a global variable's value.
   * @param name Variable name without the ?* prefix/suffix.
   * @returns The value, or null if not found/visible in current module context.
   */
  getGlobal(name: string): ClipsValue | null;

  // --- Focus Stack ---

  /** Replace the entire focus stack with a single module. */
  setFocus(moduleName: string): void;

  /** Push a module onto the focus stack. */
  pushFocus(moduleName: string): void;

  // --- I/O ---

  /**
   * Get captured output for a named channel (for example, "t" or "stderr").
   * @returns The output string, or null if no output.
   */
  getOutput(channel: string): string | null;

  /** Clear a specific output channel. */
  clearOutput(channel: string): void;

  /** Push an input line for read/readline functions. */
  pushInput(line: string): void;

  // --- Diagnostics ---

  /** Non-fatal action error messages from recent execution. */
  get diagnostics(): string[];

  /** Clear stored action diagnostics. */
  clearDiagnostics(): void;

  // --- Serialization ---

  /**
   * Serialize the engine's current state.
   * @param format Serialization format. Default: Bincode.
   */
  serialize(format?: Format): Buffer;

  /**
   * Save a serialized snapshot to a file.
   */
  saveSnapshot(path: string, format?: Format): void;

  // --- Lifecycle ---

  /**
   * Explicitly release the engine's resources.
   * After calling, all other methods will throw.
   * Idempotent — safe to call multiple times.
   */
  close(): void;

  /**
   * Support for TC39 Explicit Resource Management.
   * Allows: `using engine = new Engine()`
   * Requires TypeScript 5.2+ / Node.js 22+.
   */
  [Symbol.dispose](): void;
}
```

### EngineHandle (async, worker-backed)

`EngineHandle` wraps a synchronous `Engine` running on a dedicated Worker thread. All methods return Promises. The handle is safe to use from the main thread (or any thread) without blocking.

This is the recommended API for servers and applications where blocking the event loop is unacceptable.

```typescript
export interface EngineHandleOptions extends EngineOptions {
  /** CLIPS source to load at creation (load + reset). */
  source?: string;
  /** Snapshot to restore from (mutually exclusive with source). */
  snapshot?: { data: Buffer; format?: Format };
}

export class EngineHandle {
  /**
   * Create an EngineHandle backed by a dedicated Worker thread.
   * The Engine is created on the worker thread, satisfying thread affinity.
   */
  static create(options?: EngineHandleOptions): Promise<EngineHandle>;

  // --- Loading ---
  load(source: string): Promise<void>;
  loadFile(path: string): Promise<void>;

  // --- Fact Operations ---
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

  // --- Execution ---

  /**
   * Run the engine. Supports cancellation via AbortSignal.
   *
   * Cancellation is cooperative: the worker runs in batches of 100 rule
   * firings, checking for abort between batches. An aborted run returns
   * a partial RunResult with HaltReason.HaltRequested.
   *
   * @param options.limit - Maximum rule firings (omit for unlimited).
   * @param options.signal - AbortSignal for cancellation.
   */
  run(options?: {
    limit?: number;
    signal?: AbortSignal;
  }): Promise<RunResult>;

  step(): Promise<FiredRule | null>;
  halt(): Promise<void>;
  reset(): Promise<void>;
  clear(): Promise<void>;

  // --- Introspection ---
  getFactCount(): Promise<number>;
  getIsHalted(): Promise<boolean>;
  getAgendaSize(): Promise<number>;
  getCurrentModule(): Promise<string>;
  getFocus(): Promise<string | null>;
  getFocusStack(): Promise<string[]>;
  rules(): Promise<RuleInfo[]>;
  templates(): Promise<string[]>;
  modules(): Promise<string[]>;
  /** Resolves to null when the global is not found/visible. */
  getGlobal(name: string): Promise<ClipsValue | null>;

  // --- I/O ---
  /** Raw engine channels (for example "t", "stderr"). */
  getOutput(channel: string): Promise<string | null>;
  clearOutput(channel: string): Promise<void>;
  pushInput(line: string): Promise<void>;

  // --- Serialization ---
  serialize(format?: Format): Promise<Buffer>;

  // --- Lifecycle ---

  /**
   * Terminate the worker thread and release all resources.
   * In-flight operations will reject with an error.
   */
  close(): Promise<void>;

  /** Async dispose for `await using handle = ...` */
  [Symbol.asyncDispose](): Promise<void>;
}
```

### EnginePool (concurrent evaluation)

`EnginePool` manages multiple Worker threads for concurrent, stateless evaluation. It is the TypeScript equivalent of Go's `Coordinator` + `Manager` pattern.

Each worker lazily creates engines from named specs. Requests are dispatched round-robin across workers.

```typescript
export interface EngineSpec {
  name: string;
  options?: EngineOptions;
  /** CLIPS source to load at creation. */
  source?: string;
}

export interface EvaluateRequest {
  /** Facts to assert after reset. */
  facts?: Array<
    | { kind: "ordered"; relation: string; fields: ClipsValue[] }
    | {
        kind: "template";
        templateName: string;
        slots: Record<string, ClipsValue>;
      }
  >;
  /** Maximum rule firings. 0 or omit for unlimited. */
  limit?: number;
}

export interface EvaluateResult {
  readonly runResult: RunResult;
  readonly facts: readonly Fact[];
  /**
   * Captured output mapped to user-friendly keys:
   * "stdout" -> CLIPS "t" channel, "stderr" -> CLIPS "stderr" channel.
   */
  readonly output: Readonly<Record<string, string>>;
}

export class EnginePool {
  /**
   * Create a pool with the given engine specs and thread count.
   * @param specs Named engine configurations.
   * @param options.threads Number of worker threads. Default: 1.
   */
  static create(
    specs: EngineSpec[],
    options?: { threads?: number },
  ): Promise<EnginePool>;

  /**
   * Dispatch a function to run on a pooled engine.
   * The callback receives a proxy object for the named engine.
   * The proxy must not be retained beyond the callback's return.
   *
   * @param specName Engine spec to use.
   * @param fn Callback receiving an EngineHandle-like proxy.
   * @param options.signal AbortSignal for cancellation.
   *
   * Note: `T` must be structured-clonable because results cross the
   * worker-thread boundary.
   */
  do<T>(
    specName: string,
    fn: (engine: EngineProxy) => Promise<T>,
    options?: { signal?: AbortSignal },
  ): Promise<T>;

  /**
   * Stateless one-shot evaluation: reset → assert → run → return facts.
   * This is the primary entry point for concurrent rule evaluation.
   *
   * @param specName Engine spec to use.
   * @param request Facts and parameters for the evaluation.
   * @param options.signal AbortSignal for cancellation.
   */
  evaluate(
    specName: string,
    request: EvaluateRequest,
    options?: { signal?: AbortSignal },
  ): Promise<EvaluateResult>;

  /** Shut down all workers. Blocks until in-flight requests complete. */
  close(): Promise<void>;

  [Symbol.asyncDispose](): Promise<void>;
}

/**
 * Proxy object passed to EnginePool.do() callbacks.
 * Has the same shape as EngineHandle but operations are
 * dispatched to a specific worker's engine.
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
```

## Value Conversion Details

### JS → CLIPS

| JS type | CLIPS type | Notes |
|---------|-----------|-------|
| `FerricSymbol` | Symbol | Explicit marker type |
| `string` | String | Quoted CLIPS string |
| `number` (integer) | Integer | `Number.isInteger(n)` check |
| `number` (float) | Float | |
| `bigint` | Integer | For values outside `Number.MAX_SAFE_INTEGER` |
| `boolean` | Symbol | `true` → `TRUE`, `false` → `FALSE` |
| `Array` | Multifield | Recursive conversion |
| `null` / `undefined` | Void | |

### CLIPS → JS

| CLIPS type | JS type | Notes |
|-----------|---------|-------|
| Symbol | `FerricSymbol` | Always wrapped |
| String | `string` | Plain JS string |
| Integer | `number` or `bigint` | `bigint` only if abs value > `2^53 - 1` |
| Float | `number` | |
| Multifield | `ClipsValue[]` | Recursive |
| Void | `null` | |
| ExternalAddress | `null` | Not representable in JS |

### Integer Representation

CLIPS integers are `i64`. JavaScript `number` is a 64-bit IEEE 754 float with 53 bits of integer precision. The binding:

- Returns `number` for integers in `[-(2^53-1), 2^53-1]`.
- Returns `bigint` for integers outside that range.
- Accepts both `number` and `bigint` for assertion.

This avoids silent precision loss while keeping the common case (small integers) ergonomic.

## Worker Communication Protocol

`EngineHandle` and `EnginePool` communicate with their Worker threads via `postMessage` using a simple request/response protocol:

```typescript
// Main → Worker
interface WorkerRequest {
  id: number;             // monotonic request ID
  method: string;         // engine method name
  args: unknown[];        // structured-clonable arguments
}

// Worker → Main
interface WorkerResponse {
  id: number;             // matches request ID
  result?: unknown;       // return value (if success)
  error?: {               // error info (if failure)
    code: string;
    message: string;
    name: string;         // error class name for reconstruction
  };
}
```

Values like `FerricSymbol` and `Fact` are serialized as plain objects for `postMessage` and reconstructed on the receiving side. `Buffer` arguments (snapshots) use `ArrayBuffer` transfer for zero-copy.

The worker script:

```typescript
// Internal worker entry point (not part of public API)
import { parentPort } from "node:worker_threads";
import { Engine } from "./native.js";

let engine: Engine | null = null;

parentPort!.on("message", (req: WorkerRequest) => {
  // ... dispatch req.method to engine, post response
});
```

## Cancellation Semantics

### EngineHandle.run()

- **Before dispatch**: If the signal is already aborted, the Promise rejects immediately with `AbortError`.
- **During execution**: The worker runs batches of 100 rule firings. Between batches, it checks a shared `SharedArrayBuffer` flag set by the main thread when the signal fires. If set, the worker calls `engine.halt()` and returns a partial result.
- **After completion**: Signal changes are ignored.

### EnginePool.evaluate() / EnginePool.do()

- **Before dispatch**: Reject immediately if aborted.
- **Waiting for worker**: If aborted while queued, the request is dequeued (if possible) and rejected.
- **During execution**: Same batched-halt mechanism as `EngineHandle`.

This matches Go's `context.Context` cancellation pattern adapted for JS idioms.

## Usage Examples

### Quick Script (synchronous)

```typescript
import { Engine } from "ferric";

const engine = new Engine();
engine.load(`
  (deftemplate person (slot name) (slot age))
  (defrule greet
    (person (name ?n) (age ?a))
    =>
    (printout t "Hello " ?n ", age " ?a crlf))
`);
engine.reset();
engine.assertTemplate("person", { name: "Alice", age: 30 });

const result = engine.run();
console.log(`Fired ${result.rulesFired} rules`);
console.log(engine.getOutput("t")); // "Hello Alice, age 30\n"

engine.close();
```

### With Explicit Resource Management

```typescript
import { Engine } from "ferric";

{
  using engine = Engine.fromSource(`
    (defrule hello (initial-fact) => (printout t "Hello!" crlf))
  `);
  engine.run();
  console.log(engine.getOutput("t"));
} // engine.close() called automatically
```

### Non-blocking Server

```typescript
import { EngineHandle } from "ferric";

const handle = await EngineHandle.create({
  source: `
    (deftemplate order (slot id) (slot total))
    (defrule big-order
      (order (id ?id) (total ?t&:(> ?t 1000)))
      =>
      (printout t "Large order: " ?id crlf))
  `,
});

// In a request handler:
async function handleRequest(orderId: string, total: number) {
  await handle.reset();
  await handle.assertTemplate("order", {
    id: orderId,
    total,
  });

  const controller = new AbortController();
  setTimeout(() => controller.abort(), 5000); // 5s timeout

  const result = await handle.run({ signal: controller.signal });
  const output = await handle.getOutput("t");
  return { rulesFired: result.rulesFired, output };
}

// On shutdown:
await handle.close();
```

### Concurrent Evaluation Pool

```typescript
import fs from "node:fs";
import { EnginePool, FerricSymbol } from "ferric";

const pool = await EnginePool.create(
  [
    {
      name: "fraud-detector",
      source: fs.readFileSync("rules/fraud.clp", "utf-8"),
    },
    {
      name: "pricing",
      source: fs.readFileSync("rules/pricing.clp", "utf-8"),
    },
  ],
  { threads: 4 },
);

// Stateless evaluation — each call resets, asserts, runs, returns.
const result = await pool.evaluate("fraud-detector", {
  facts: [
    {
      kind: "template",
      templateName: "transaction",
      slots: { amount: 9999, country: new FerricSymbol("NG") },
    },
  ],
});

console.log(result.runResult.rulesFired);
console.log(result.facts);
console.log(result.output);

await pool.close();
```

### EnginePool.do() for Stateful Operations

```typescript
// When you need more control than evaluate() provides:
const score = await pool.do("pricing", async (engine) => {
  await engine.reset();
  await engine.assertTemplate("customer", {
    tier: new FerricSymbol("gold"),
    years: 5,
  });
  await engine.assertTemplate("item", {
    sku: "WIDGET-42",
    basePrice: 29.99,
  });
  await engine.run();
  const facts = await engine.findFacts("final-price");
  return facts[0]?.fields[0] as number;
});
```

## Implementation Notes

### napi-rs Specifics

- Use `#[napi(object)]` for plain data types (`RunResult`, `RuleInfo`, etc.) — these become plain JS objects.
- Use `#[napi]` on the `Engine` struct for the class binding.
- `Buffer` in napi-rs maps to Node.js `Buffer` (zero-copy when possible).
- Enums: use `#[napi]` on Rust enums with explicit discriminants. Expose regular TypeScript `enum` declarations in the public package (avoid `const enum` in library-facing API for toolchain compatibility).
- Error mapping: napi-rs's `napi::Error` supports custom `status` codes. Implement `From<EngineError>` for `napi::Error` with the appropriate error class.
- `Symbol.dispose` / `Symbol.asyncDispose`: implement via `#[napi(ts_return_type = "void")]` methods named `[Symbol.dispose]` — or more practically, add `close()` in Rust and wire `Symbol.dispose` in the TypeScript wrapper.

### Engine Ownership in napi-rs

The napi-rs `Engine` class wraps a Rust `Option<ferric::Engine>`:

```rust
#[napi]
pub struct Engine {
    inner: Option<ferric::Engine>,
}
```

- `close()` takes the engine out of the `Option`, dropping it.
- All methods check `self.inner.is_some()` and throw if closed.
- `Drop` for the napi-rs struct drops the inner engine if still present (handles GC without explicit close).
- No thread-affinity enforcement needed in the napi-rs layer: the Rust `Engine` is used directly (no FFI thread check), and JS naturally calls methods on the thread that created the object.

### Worker Thread Bootstrap

The worker thread script needs access to the native addon. napi-rs addons work in Worker threads — Node.js loads a separate instance of the addon per thread. The worker entry point:

1. Receives an `init` message with engine options.
2. Creates a synchronous `Engine` (which creates the Rust engine on the worker's OS thread).
3. Enters a request loop, dispatching method calls and posting responses.
4. On `close` message, drops the engine and exits.

### Serialization Across Workers

Values passed via `postMessage` must be structured-clonable. The binding provides transparent serialization for:

- `FerricSymbol` → `{ __type: "FerricSymbol", value: string }` (tagged for reconstruction).
- `Fact` → plain object (already structured-clonable).
- `Buffer` (snapshots) → transferred as `ArrayBuffer` (zero-copy).

This is handled in the TypeScript layer, not in Rust.

### Batch Size for Cooperative Cancellation

The `run()` implementation in workers uses a batch size of 100 rule firings (matching Go). Between batches, the worker:

1. Checks a `SharedArrayBuffer` abort flag.
2. If set, calls `engine.halt()` and returns a partial result.

The main thread sets the flag when `AbortSignal` fires. This gives cancellation latency of at most ~100 rule firings, which is typically sub-millisecond.

### Package Layout

```
packages/ferric/
├── package.json
├── src/
│   ├── index.ts              # barrel re-export
│   ├── native.ts             # re-export from native addon
│   ├── engine-handle.ts      # EngineHandle (async wrapper)
│   ├── engine-pool.ts        # EnginePool (concurrent wrapper)
│   ├── worker.ts             # worker thread entry point
│   ├── types.ts              # shared TypeScript types
│   └── wire.ts               # wire types for postMessage
├── native/                   # napi-rs generated bindings
│   ├── index.js
│   └── index.d.ts
└── npm/                      # platform packages
    ├── darwin-arm64/
    ├── darwin-x64/
    ├── linux-x64-gnu/
    └── win32-x64-msvc/
```

## Comparison with Other Bindings

| Aspect | Python | Go | TypeScript |
|--------|--------|----|------------|
| Thread safety | TLS registry | LockOSThread / Coordinator | Worker threads |
| Sync API | All methods sync | All methods sync | `Engine` (sync) |
| Async API | N/A | `context.Context` on Run | `EngineHandle` (Promise + AbortSignal) |
| Concurrency | N/A | Coordinator + Manager | `EnginePool` |
| Cancellation | N/A | `context.Context` | `AbortSignal` |
| Resource cleanup | `close()` + context manager | `Close()` (io.Closer) | `close()` + `Symbol.dispose` |
| Value distinction | `Symbol` class / `ClipsString` class | `Symbol` type alias | `FerricSymbol` class |
| String default | str → Symbol | string → String | string → String |
| Integer overflow | Python int is arbitrary | int64 native | `number` / `bigint` adaptive |
| FFI layer | PyO3 (Rust direct) | CGo → C FFI | napi-rs (Rust direct) |

## Non-Goals

1. Browser/Wasm support (napi-rs is Node.js only; Wasm would be a separate binding).
2. Streaming or event-based rule firing callbacks (can be added later via napi-rs `ThreadsafeFunction`).
3. Exposing the Rete network internals or providing custom node types.
4. Supporting Deno or Bun out of the box (likely works but not a test target initially).

## Future Extensions

- **Event callbacks**: Use napi-rs `ThreadsafeFunction` to invoke a JS callback on each rule firing, enabling streaming observation of engine execution.
- **Snapshot transfer**: Allow `EnginePool` to pre-serialize a snapshot and distribute it to workers for fast warm-start.
- **Custom functions**: Register JS functions callable from CLIPS RHS actions. Requires `ThreadsafeFunction` for the worker-backed APIs.
