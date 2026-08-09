# TypeScript Binding API for Ferric

> [!WARNING]
> This document is a legacy design draft and is **not** the normative implementation target.
> Use the revised specification suite instead:
> - [TypeScript Binding Architecture (Revised)](/Users/prb/conductor/workspaces/ferric-rules/santo-domingo/docs/typescript-binding-architecture.md)
> - [TypeScript Binding Normative Contract (Revised)](/Users/prb/conductor/workspaces/ferric-rules/santo-domingo/docs/typescript-binding-normative-contract.md)
> - [TypeScript Binding Conformance Matrix](/Users/prb/conductor/workspaces/ferric-rules/santo-domingo/docs/typescript-binding-conformance-matrix.md)
> - [TypeScript Binding Test Specification (Revised)](/Users/prb/conductor/workspaces/ferric-rules/santo-domingo/docs/typescript-binding-test-spec.md)

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

Created via a Rust crate (`ferric-rules-napi`) that depends on
`ferric-rules-core` and `ferric-rules-runtime` directly—there is no FFI
indirection. napi-rs handles the JS ↔ Rust boundary.

### Layer 2: `EngineHandle` / `EnginePool` (pure TypeScript)

Async wrappers that run `Engine` instances inside dedicated `Worker` threads. Communication is via structured-clone `postMessage`. These are shipped as TypeScript alongside the native addon.

This separation means:

- The native addon is simple and stateless beyond the Engine itself.
- Async orchestration, cancellation, and pooling are in TypeScript where they're easy to test, debug, and extend.
- Worker threads each create their own `Engine` on their own OS thread, satisfying thread affinity automatically.

## Crate Structure

```
crates/ferric-rules-napi/
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
  ActionError = 3,
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

`ActionError` means the failing activation was consumed, its remaining RHS
actions were skipped, and later activations remain queued for a subsequent
`run()`. Read `engine.diagnostics` before starting that next run.

Every public `run()` invocation that reaches native execution starts a fresh
logical run. Starting fresh clears any pending halt request and action
diagnostics while leaving working memory and the agenda intact. Worker-backed
APIs may split that one logical run into private continuation chunks for
cancellation polling; those chunks do not start new logical runs.

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

/** Canonical, lossless 64-bit generational fact identifier. */
export type FactId = bigint;

/**
 * Accepted fact-ID input. Legacy numbers must be non-negative safe integers;
 * use FactId for new code and all IDs returned by Ferric.
 */
export type FactIdInput = FactId | number;

export interface Fact {
  readonly id: FactId;
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

/** Host-side EnginePool admission failure; never crosses the Worker wire. */
export class EnginePoolQueueFullError extends FerricError {
  readonly capacity: number;
  readonly queued: number;
  readonly slotIndex: number;

  constructor(capacity: number, queued: number, slotIndex: number);
}
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
  assertString(source: string): FactId[];

  /**
   * Assert an ordered fact.
   * @returns The fact ID.
   * @example engine.assertFact("color", new FerricSymbol("red"))
   */
  assertFact(relation: string, ...fields: ClipsValue[]): FactId;

  /**
   * Assert a template fact with named slots.
   * @returns The fact ID.
   * @example engine.assertTemplate("person", { name: "Alice", age: 30 })
   */
  assertTemplate(
    templateName: string,
    slots: Record<string, ClipsValue>,
  ): FactId;

  /** Retract a fact by ID. */
  retract(factId: FactIdInput): void;

  /** Get a snapshot of a single fact, or null if not found. */
  getFact(factId: FactIdInput): Fact | null;

  /** Get snapshots of all user-visible facts. */
  facts(): Fact[];

  /** Get snapshots of facts matching a relation name. */
  findFacts(relation: string): Fact[];

  /** Get a template fact's slot value by name. */
  getFactSlot(factId: FactIdInput, slotName: string): ClipsValue;

  // --- Execution ---

  /**
   * Run the engine to completion or until the limit is reached.
   * Every call starts a fresh logical run, clearing any previous halt request
   * and action diagnostics. A limit of 0 still starts that fresh run, but fires
   * no rules and returns LimitReached.
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

Worker ownership begins only after the Worker constructor returns and transfers
to the caller only when initialization succeeds. If any intervening setup or
initialization step fails, `create()` removes its initialization bookkeeping
and Worker listeners, invokes and awaits `terminate()` exactly once, and only
then rejects. The initialization error remains the rejection with object
identity, class, and message intact; a simultaneous termination failure is
attached as its `cause` rather than replacing it when cleanup can define or
redefine an own writable/configurable cause property on the primary `Error`.
For an error that rejects that descriptor update, or a non-`Error` thrown value,
attachment is best-effort and exact primary identity takes precedence.
Pre-Worker validation and a synchronous Worker-constructor throw own no Worker,
while a successful create retains the normal listeners and transfers the live
Worker to the returned handle.

This failed-create rule includes cleanup after an initialization
`postMessage` throw. Ordinary handle sends use the request-local rollback rule
in the Worker Communication Protocol below. The completion barrier shared by
concurrent public `close()` calls remains FR-NODE-010.

After a returned handle registers an ordinary request, a synchronous
`postMessage` failure rejects that request's Promise with the exact thrown
value only after removing its pending entry and any request-owned abort
listener. It does not close or terminate the handle, detach its shared Worker
listeners, reuse the failed request ID, or prevent a later valid request.

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
   * A failure after Worker construction rejects only after exactly-once Worker
   * teardown; the primary initialization error is preserved.
   */
  static create(options?: EngineHandleOptions): Promise<EngineHandle>;

  // --- Loading ---
  load(source: string): Promise<void>;
  loadFile(path: string): Promise<void>;

  // --- Fact Operations ---
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

  // --- Execution ---

  /**
   * Run the engine. Supports cancellation via AbortSignal.
   *
   * The worker starts one fresh logical run and uses private continuation
   * chunks after the first batch. Without host cancellation, the result,
   * halted state, agenda, and diagnostics match an equivalent synchronous run,
   * including when a rule halts on an exact batch boundary.
   *
   * Cancellation is cooperative: the worker checks an out-of-band abort flag
   * between batches and stops submitting continuation chunks. For API
   * compatibility, an aborted run resolves with a partial RunResult whose
   * haltReason is HaltRequested; host cancellation does not call native halt()
   * or otherwise set the engine's halt latch.
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

Each worker lazily creates engines from named specs. Requests are dispatched round-robin across workers while the pool is healthy.
Work assigned to one worker slot is admitted FIFO. A `do()` callback receives
an exclusive lease over its selected slot before the callback begins, so no
unrelated task can use that worker until the pool processes the callback's
normal settlement and its accepted proxy calls drain. Cancellation closes
future proxy admission promptly but does not preempt that callback or shorten
its lease.

Pool construction defaults to one Worker when `threads` is omitted or
`undefined`. An explicit count must be a JavaScript safe integer in the
inclusive range `1..64`; Ferric does not coerce, clamp, or provide an override
for larger values. Invalid counts throw `RangeError` synchronously from
`EnginePool.create()` before it returns a Promise or constructs any Worker.

Each selected worker slot also has one shared finite waiting budget. The
default `queueCapacity` is `1024` entries per slot; the budget covers queued
root evaluations, queued `do()` lease admissions, and accepted lease-private
proxy calls together. Work that can dispatch immediately and the admitted
callback/lease itself do not consume a queue entry. A full selected slot rejects
immediately rather than waiting, probing another slot, or replaying work.

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

export interface EnginePoolOptions {
  /** Number of worker threads. Default: 1; range: 1..64. */
  threads?: number;
  /** Maximum waiting entries on each worker slot. Default: 1024; range: >= 0. */
  queueCapacity?: number;
}

export interface EnginePoolSlotMetrics {
  readonly slotIndex: number;
  readonly queued: number;
  readonly inFlight: number;
  /** Queue-full admission rejections on this slot since pool creation. */
  readonly rejected: number;
}

export interface EnginePoolMetrics {
  /** Configured waiting capacity of each slot, not a pool-wide capacity. */
  readonly queueCapacity: number;
  /** Sum of queued entries across all slots. */
  readonly queued: number;
  /** Sum of dispatched requests across all slots. */
  readonly inFlight: number;
  /** Sum of queue-full admission rejections since pool creation. */
  readonly rejected: number;
  readonly slots: readonly EnginePoolSlotMetrics[];
}

export class EnginePool {
  /**
   * Create a pool with the given engine specs and pool options.
   * @param specs Named engine configurations.
   * @param options Pool construction and per-slot queue limits.
   * @throws RangeError synchronously if threads or queueCapacity is invalid.
   */
  static create(
    specs: EngineSpec[],
    options?: EnginePoolOptions,
  ): Promise<EnginePool>;

  /**
   * Dispatch a function to run on a pooled engine.
   * The callback receives a proxy object for the named engine and exclusively
   * leases the selected worker slot for its whole asynchronous lifetime.
   * Proxy calls execute serially in invocation order. Aborting before callback
   * settlement rejects `do()` promptly and makes later proxy calls reject with
   * `AbortError`; calls already accepted remain eligible to drain. The proxy
   * must not be retained after `do()` delivers the callback's value or error.
   *
   * @param specName Engine spec to use.
   * @param fn Callback receiving an EngineHandle-like proxy.
   * @param options.signal AbortSignal for cancellation.
   */
  do<T>(
    specName: string,
    fn: (engine: EngineProxy) => Promise<T>,
    options?: { signal?: AbortSignal },
  ): Promise<T>;

  /**
   * Stateless one-shot evaluation: reset → assert → run → return facts.
   * This is the primary entry point for concurrent rule evaluation.
   * Its run phase uses one fresh logical run followed by private continuation
   * chunks, with the same absent-cancellation semantics as Engine.run().
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

  /**
   * Return a fresh, detached point-in-time scheduling snapshot.
   * This method is synchronous and remains available during callbacks,
   * terminal failure, shutdown, and after close.
   */
  metrics(): EnginePoolMetrics;

  /**
   * Shut down all workers. Blocks until in-flight requests and callbacks that
   * already acquired a worker-slot lease complete.
   */
  close(): Promise<void>;

  [Symbol.asyncDispose](): Promise<void>;
}

/**
 * Proxy object passed to EnginePool.do() callbacks.
 * Has the same shape as EngineHandle but operations are
 * dispatched to a specific worker's engine. Calls are serialized in invocation
 * order. New calls reject deterministically after cancellation or callback
 * settlement without reaching the Worker.
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
   * Start a fresh logical run, using private continuation chunks only for
   * cancellation polling after the first batch.
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
```

#### `EnginePool.do()` lease and proxy lifetime

- The lease covers the entire selected worker slot, not only the named engine.
  A task for a different spec cannot execute on that worker during the callback.
- Admission is FIFO within each slot. Different slots remain concurrent, so the
  pool does not promise global start or completion order.
- The callback begins only after acquiring its lease. External awaits and a
  callback that makes no proxy calls still retain it.
- Proxy methods are serialized in invocation order, including calls started in
  parallel.
- Normal callback settlement is observed when the pool's registered reaction
  to the returned Promise begins. Promise reactions that the callback itself
  registered before returning that Promise run first under JavaScript's FIFO
  reaction ordering and remain inside the lease; proxy calls they invoke are
  accepted and drain in order.
- At the pool-observed settlement boundary, the proxy becomes invalid before
  `do()` delivers the callback's value or error. Calls already accepted drain
  before the lease releases. Every call after that boundary rejects with one
  deterministic lifetime error without reaching the worker.
- A proxy request is accepted when its final active-lease, slot-state, and
  signal gate passes immediately before request-ID allocation. Proxy methods
  apply the same gate before method validation and the send path rechecks it
  after preprocessing, closing an abort-during-validation race. Calls accepted
  before abort remain accepted even if they are waiting in the lease-private
  FIFO; they drain in order, keep their own response/send/terminal outcomes,
  and may mutate engine state after cancellation. Abort does not dequeue them
  or roll them back.
- If the `do()` signal aborts before the pool observes callback settlement, the
  outer Promise promptly rejects with a `DOMException` whose name is
  `AbortError` and message is `The operation was aborted`. Every proxy method
  invoked afterward returns that rejection before method validation, ID
  allocation, accounting, listener registration, queue insertion, or
  `postMessage`.
- Cancellation does not interrupt arbitrary callback JavaScript or release its
  worker-slot lease. Unrelated work remains excluded until the callback really
  settles and every accepted call drains; that path releases the lease exactly
  once. The prompt outer rejection does not wait for this barrier.
- Callback settlement observed before abort wins even while accepted calls are
  still draining. Its outer listener is removed, the callback outcome remains
  fixed, and a later abort cannot replace the normal lifetime error.
- A synchronous `postMessage` failure rejects only that proxy operation. It
  does not release or invalidate the lease, and already-accepted later owner
  calls continue through the lease-private FIFO after the failed send is
  rolled back.
- The callback return value stays on the main thread and does not need to be
  structured-clonable. Only proxy arguments and results cross the worker
  boundary.
- A lease supplies isolation, not rollback. Facts, rules, output, and other
  engine state changed by the callback remain changed if it rejects.
- Calling `do()`, `evaluate()`, or `close()` on the same pool from inside its
  active callback rejects rather than waiting on the callback's own lease.
  Calling another pool is supported.
- `close()` waits for a callback that already acquired its lease, including an
  idle await between proxy calls. A callback still waiting to acquire a lease
  is rejected as not-yet-admitted work.

While the callback remains active, an already-failed slot's retained terminal
error takes precedence over `AbortError`; after callback settlement, the
lifetime error takes precedence over either. An accepted request that has
started `postMessage` keeps the synchronous-send/first-settlement rules below,
independently of the outer cancellation outcome.

#### Bounded queue backpressure and metrics

- `queueCapacity` is a per-slot waiting limit. It defaults to `1024` only when
  omitted or `undefined`; an explicit value must be a nonnegative safe integer.
  Zero disables waiting while still allowing work that can dispatch or acquire
  a lease immediately. JavaScript `-0` is normalized to `0`.
- `EnginePool.create()` validates `threads` first and then `queueCapacity`.
  Invalid capacity throws
  `RangeError("EnginePool.create: 'queueCapacity' must be a non-negative safe integer")`
  synchronously, before spec inspection, Promise creation, Worker construction,
  or initialization bookkeeping.
- One selected slot's shared budget counts its root FIFO entries (`evaluate()`
  requests and waiting `do()` leases) plus its active lease's private FIFO
  entries. Its dispatched request and active callback/lease do not count. The
  pool-wide maximum waiting count is therefore `threads * queueCapacity`.
- If work must wait and the selected slot already retains `queueCapacity`
  entries, its Promise rejects with `EnginePoolQueueFullError`. The error has
  name `EnginePoolQueueFullError`, code `FERRIC_POOL_QUEUE_FULL`, exact message
  `EnginePool queue is full`, and readonly `capacity`, `queued`, and
  `slotIndex` fields describing the selected slot at rejection.
- Overflow is reject-only. Ferric does not wait, time out, scan another slot,
  retry, replay, or rewind the completed round-robin selection. The rejected
  item never enters either FIFO and consumes no request ID, lease,
  pending/lease-call unit, queue entry, or Worker post. An already-full slot
  rejects before installing any request listener, and no overflow retains one.
- Existing guards and validation retain precedence. Capacity is tested only
  after the applicable reentry, lifetime, closed, terminal, abort, argument,
  and preprocessing gates. `evaluate()` and proxy `run()` then install their
  cooperative-cancellation listener before request-ID allocation and Worker
  send. Because `signal.addEventListener` is replaceable JavaScript, that hook
  may synchronously admit other work; Ferric rechecks lifecycle, abort, current
  scheduling state, and capacity afterward. If the hook filled the slot, that
  nested work linearizes first and the outer call rejects with
  `EnginePoolQueueFullError`; its transient cooperative listener is removed and
  it still owns no ID, accounting, queue entry, or Worker post. Public and proxy
  overload failures are rejected Promises rather than synchronous public
  throws.
- After final admission, a signaled root request or waiting lease structurally
  enters its FIFO before its replaceable dequeue-cancellation listener is
  registered. Reentrant work therefore observes that reserved capacity. If
  registration throws while the entry is still queued, Ferric removes it,
  rejects with the exact thrown value, releases a waiting lease once, and
  continues the FIFO. If hook reentry already dispatched, admitted, faulted,
  or close-rejected the entry, that earlier outcome wins and the now-stale
  listener is detached; the hook cannot roll it back or settle it twice. This
  reconciliation retries a synchronous-abort detachment if a replaceable
  removal hook throws, without replacing the owned outcome; persistently
  hostile removal remains best-effort. It does not implement FR-NODE-006's
  general successful-root-dispatch listener cleanup.
- A queue unit is reclaimed when its entry is removed or dequeued. Abort frees
  a queued root request or not-yet-admitted lease; it does not dequeue a proxy
  call already accepted under the callback-cancellation contract. Dispatch
  frees the unit before `postMessage`, so synchronous send rollback must not
  free it twice. Worker terminal cleanup frees both queue levels. Close frees
  the root FIFO while an admitted callback's accepted owner FIFO retains its
  existing drain contract. Response completion changes only `inFlight`, since
  its queue unit was reclaimed at dispatch.
- `metrics()` is synchronous, side-effect-free, and independent of admission
  and same-pool callback reentrancy. Each call returns fresh detached objects:
  `queueCapacity` is the configured per-slot limit; `queued`, `inFlight`, and
  `rejected` are pool-wide sums; and stable `slotIndex` entries report the same
  three counters per slot. `rejected` counts queue-full admissions only, not
  abort, close, terminal, response, or send failures. Readonly typing prevents
  supported mutation; no live internal queue reference is exposed.

#### Worker terminal failure policy

Each pool slot has an explicit lifecycle. The first Worker `error` or
unexpected `exit` observed before close begins terminating that Worker marks
its slot failed and establishes one pool-wide terminal failure. An `error`
retains the exact emitted object; an unexpected exit creates one stable error
for its exit code. Later terminal signals cannot replace that primary failure
or settle work a second time.

The failed slot rejects every request, root queue entry, lease admission, and
lease-private proxy operation assigned to it. It clears its counters, removes
owned abort and Worker listeners, and wakes close bookkeeping. The pool does
not replay that work or respawn the Worker because replacement would silently
discard the mutable engines hosted by the failed slot.

Once the failure is observed, new `evaluate()` and `do()` calls reject with the
same primary error before round-robin selection or request bookkeeping. Work
already accepted on another healthy slot remains eligible to finish through
that slot's FIFO. An already-admitted healthy `do()` lease may continue its
owner proxy operations until the callback settles unless its own signal has
already closed future proxy admission. Recovery is explicit: close the failed
pool and create a new one.

A failed-slot callback's pending and queued proxy operations reject, and later
proxy sends fail fast. The pool cannot forcibly settle arbitrary JavaScript in
that callback, such as an unrelated Promise awaited while the worker is idle;
the existing admitted-callback release barrier remains in force until the
callback settles. `close()` therefore still observes the documented callback
lifetime while no pool-generated request or close waiter remains stranded.

An ordinary response error or synchronous request-side `postMessage` failure
from a live Worker rejects only its matching request and does not poison the
pool. A failed send restores the selected slot's in-flight capacity and
continues its root or lease-private FIFO without replaying the request on
another Worker. An exit caused after `close()` deliberately starts Worker
termination is also expected, not a fault. FR-NODE-009 owns the `do` outer
listener and proxy-`run` listener described here. General root-queue listener
cleanup after a successful queued dispatch is FR-NODE-006, and the Promise
shared by concurrent close callers is FR-NODE-010. FR-NODE-011 owns only the
bounded admission and metrics contract above.

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

### Fact Identifier Representation and Migration

Fact identifiers are not CLIPS integer values. They are unsigned 64-bit,
generational engine handles, so Ferric exposes every returned ID as the
canonical `FactId = bigint` representation even when its current value would
fit in a JavaScript safe integer. This applies to `assertString`, `assertFact`,
`assertTemplate`, and the `id` property returned by `getFact`, `facts`, and
`findFacts`.

ID-accepting APIs use `FactIdInput = FactId | number` as a deliberate migration
bridge. A `number` is accepted only when it is finite, integral, non-negative,
and no greater than `Number.MAX_SAFE_INTEGER`; unsafe numeric inputs are
rejected with an argument error that directs callers to use `bigint`. A
`bigint` must fit the unsigned 64-bit range.

Existing callers should migrate as follows:

- Treat the output change from `number` to `bigint` as a source-level breaking
  change; code and declarations that annotate returned IDs as `number` must be
  updated.
- Treat returned IDs as `bigint` and update type assertions from `number` to
  `FactId`.
- Use bigint literals such as `123n` for new ID inputs. Existing safe numeric
  inputs remain accepted during migration.
- Never convert a returned ID with `Number(id)`, because doing so can discard
  its generation bits.
- `bigint` is supported by Node's structured-clone algorithm, so IDs pass
  unchanged through `EngineHandle` and `EnginePool`. For JSON, encode an ID as
  decimal text with `id.toString()` and reconstruct it with `BigInt(text)`;
  `JSON.stringify` does not serialize `bigint` by default.

This fact-ID rule is intentionally separate from the adaptive
`number`/`bigint` representation used for CLIPS integer field values and from
run limits and fired counts.

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

### Synchronous request-send failures

Main-to-Worker request submission is transactional from pending registration
through `postMessage` acceptance. This applies to ordinary `EngineHandle`
methods and `run()`, pool initialization, immediate and queued `evaluate()`
dispatch, and immediate and queued `EngineProxy` dispatch. `EngineHandle`
initialization uses the stronger failed-create ownership rule described above.

If `postMessage` throws synchronously while the exact registered request entry
is still pending, Ferric rolls back that request before its Promise rejection
can be observed:

- remove the exact pending entry and decrement its pool in-flight count once;
- remove only abort listeners owned by that failed request;
- reject with the exact thrown value, without reconstruction or replacement;
- wake pool close bookkeeping and continue the applicable root or
  lease-private FIFO until another request is accepted or that FIFO is empty;
  and
- keep the returned handle, Worker slot, pool terminal state, and active lease
  otherwise unchanged.

The request is neither replayed nor moved to another Worker. Its monotonically
allocated request ID and any completed round-robin selection remain consumed.
A later valid request therefore uses normal forward progress rather than
reusing transport history.

The rollback is conditional on ownership of the same `(id, pending entry)`.
A deterministic Worker seam can synchronously emit a response, `error`, or
`exit` before throwing from `postMessage`; if that event already settled and
removed the entry, it wins and the catch path must not settle, decrement, or
drain it again. Conversely, when send rollback wins first, a later terminal
Worker event cannot replace that request's send error, although it still
governs remaining and future pool work under the terminal-failure policy.

Existing pre-dispatch gates retain precedence. For a callback proxy those are,
in order: inactive/released lifetime, failed slot, non-running/closed slot,
aborted active lease, and then method validation. Other closed/terminal checks,
argument validation, and a dequeuable pre-abort likewise fail before request
submission and do not call `postMessage`. Once a send is attempted, its
synchronous failure is the request outcome; a later abort cannot replace it.
Pool initialization failure rejects the creation Promise with that exact value
and the existing failed-create transaction terminates every unpublished Worker
it constructed.

This rule covers main-to-Worker sends that own host request bookkeeping.
Worker-to-main response sends create no such registration and remain governed
by the response protocol and Worker error/exit lifecycle. It also does not add
generic cleanup after a *successful* queued root dispatch (FR-NODE-006), alter
the callback cancellation boundary above, or make concurrent close calls share
a Promise (FR-NODE-010). For FR-NODE-011, a queued unit is already reclaimed
when the entry is removed for dispatch; send rollback must not reclaim it a
second time and continues the same FIFO as described above.

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
- **During execution**: The worker starts one fresh native run, then uses private continuation chunks of at most 100 rule firings. Between chunks it checks a shared `SharedArrayBuffer` flag set by the main thread. If set, it stops submitting chunks and returns the partial count with `HaltReason.HaltRequested`; it does not call native `halt()` or set the engine halt latch.
- **Without cancellation**: Chunked execution is observationally equivalent to synchronous `Engine.run()` in total fired count, halt reason, halted state, agenda, and diagnostics. A halt produced on an exact chunk boundary is observed before another activation can fire.
- **Caller limit**: Exhausting the public limit has the same precedence as synchronous execution. In particular, if the limit-th activation also requests a halt, the result is `LimitReached` while the engine's halted state remains observable.
- **Zero limit**: `run({ limit: 0 })` still starts a fresh native run. It fires no rules and returns `LimitReached`, while clearing the previous logical run's halt request and diagnostics.
- **After completion**: Signal changes are ignored.

### EnginePool.evaluate() / EnginePool.do()

- **Before dispatch**: After the existing reentry, closed, and terminal guards,
  an otherwise-admissible call rejects immediately if already aborted.
- **Waiting for worker**: A queued `evaluate()` root request or a `do()` lease
  admission that has not begun its callback is removed and rejected if abort
  wins. This does not apply to a proxy request already accepted into the
  callback's lease-private FIFO; accepted owner work remains eligible to drain.
- **During execution**: The run phase follows the same fresh-run, continuation,
  exact-boundary, and out-of-band cancellation contract as `EngineHandle`.
- **Callback admission**: For `do()`, abort observed before callback settlement
  promptly rejects the outer Promise and makes every later active-proxy method
  return `AbortError` before validation or request bookkeeping. A request whose
  final gate passed before abort remains accepted, even while lease-queued, and
  keeps its normal outcome and possible state effects.
- **Callback lease**: Rejecting the outer `do()` Promise does not release a
  worker slot still owned by a running callback. Unrelated work remains queued
  until the pool-observed callback settlement boundary and accepted-call drain,
  which release the lease exactly once.
- **Retained proxy**: While the aborted callback is still active, its healthy
  proxy rejects with `AbortError`. After callback settlement it rejects with
  the ordinary lifetime error. An active failed-slot error takes precedence;
  callback settlement observed before abort makes a later abort irrelevant.

The partial `HaltRequested` result is the existing JavaScript API projection of
host cancellation; it does not imply that the native engine halt latch was set.
A later `run()` always starts fresh and clears the documented execution state.
An accepted proxy `run()` receives the same out-of-band abort flag and normally
resolves its partial result even though the independently returned outer
`do()` Promise rejects with `AbortError`.

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

The napi-rs `Engine` class wraps a Rust `Option<ferric_rules::Engine>`:

```rust
#[napi]
pub struct Engine {
    inner: Option<ferric_rules::Engine>,
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
- `Fact` → plain object whose `bigint` ID is preserved by structured clone.
- `Buffer` (snapshots) → transferred as `ArrayBuffer` (zero-copy).

An uncloneable main-to-Worker request rejects under the synchronous send rule
above without retaining request bookkeeping or disabling the Worker.

This is handled in the TypeScript layer, not in Rust.

### Batch Size for Cooperative Cancellation

The `run()` implementation in workers uses a batch size of 100 rule firings
(matching Go). The first batch starts a fresh native logical run; later batches
continue it without clearing its halt request or diagnostics. After a chunk
returns `LimitReached`, the worker applies this order:

1. If the caller's total limit is exhausted, return `LimitReached`.
2. If the `SharedArrayBuffer` abort flag is set, stop and return the existing
   partial `HaltRequested` API result without calling native `halt()`.
3. If the engine halt latch is set, return `HaltRequested`.
4. Otherwise submit a continuation chunk.

Native terminal reasons are returned immediately. This ordering preserves
synchronous behavior when an explicit limit, host abort, or rule-side halt
coincides with a batch boundary. The main thread sets the abort flag when the
`AbortSignal` fires, giving cancellation latency of at most one batch.

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
| Thread safety | Creator-thread-affine ordinary operations; selected native work releases the GIL | LockOSThread / Coordinator | Worker threads |
| Sync API | All methods sync | All methods sync | `Engine` (sync) |
| Async API | N/A | `context.Context` on Run | `EngineHandle` (Promise + AbortSignal) |
| Concurrency | No cross-thread queue | Coordinator + Manager | `EnginePool` |
| Cancellation | Active-run halt / close | `context.Context` | `AbortSignal` |
| Resource cleanup | Any-thread `close()` / context exit + final-reference drop | `Close()` (io.Closer) | `close()` + `Symbol.dispose` |
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
