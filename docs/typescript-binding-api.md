# TypeScript Binding API for Ferric

> [!NOTE]
> This is the public API reference for `@ferric-rules/node`.
> The [Normative Contract](typescript-binding-normative-contract.md) governs behavior
> and takes precedence if the documents conflict. See also the
> [Architecture](typescript-binding-architecture.md),
> [Conformance Matrix](typescript-binding-conformance-matrix.md), and
> [Test Specification](typescript-binding-test-spec.md).

## Purpose

The package provides a TypeScript API for ferric-rules that:

1. Uses Node.js and TypeScript conventions: Promises, `AbortSignal`, and explicit resource management.
2. Keeps long-running engine work off the Node.js event loop. (The Rust engine is `Send + Sync`; workers exist for event-loop responsiveness, not thread affinity.)
3. Provides both a synchronous low-level API and an async worker-backed API for non-blocking use.
4. Links directly to Rust through napi-rs.

## Execution in Node.js

The Rust engine supports ownership transfer and shared reads. A native Node
object belongs to its V8 isolate, so `Engine` remains a synchronous API in that
isolate. Every native call has checked reentrancy protection: a JavaScript
getter cannot close or access an engine during its admitted operation.

Long `load`, `run`, serialization, and file operations block the calling event
loop. Use `EngineHandle` for their asynchronous worker-backed counterparts;
serialize there and use `node:fs/promises.writeFile` to save without blocking the
main thread. `EnginePool` retains independent workers for parallel evaluations
and exclusive leases. No dedicated-thread assumption is needed for the Rust
engine itself.

Node 22 is the supported minimum. See the [normative contract](typescript-binding-normative-contract.md)
for the pre-1.0 numeric, disposal, import, and snapshot migrations.

## Architecture

### Layer 1: `Engine` (native, napi-rs)

A synchronous class exported from the native addon. All methods execute on the calling thread. This is the only layer that touches Rust code.

Created via a Rust crate (`ferric-rules-napi`) that depends on
`ferric-rules-core` and `ferric-rules-runtime` directly—there is no FFI
indirection. napi-rs handles the JS ↔ Rust boundary.

### Layer 2: `EngineHandle` / `EnginePool` (pure TypeScript)

Async wrappers that run `Engine` instances inside dedicated `Worker` threads. Communication is via structured-clone `postMessage`. The package ships compiled JavaScript and TypeScript declarations alongside its native loader.

This separation means:

- The native addon owns engine state and value conversion.
- Async orchestration, cancellation, and pooling are in TypeScript where they're easy to test, debug, and extend.
- Each worker thread owns its own `Engine`, so long-running engine work never blocks the main event loop.

## Public API

### Value Types

`FerricSymbol` and `FerricInstanceName` are concrete constructor exports. Their
instance and constructor types are exported under the `Native*` names below.
A symbol's spelling is distinct from a quoted string; an instance name represents
CLIPS `[widget]`, with `value` containing `widget` without brackets.

```typescript
export interface NativeFerricSymbolConstructor {
  new (value: string): NativeFerricSymbol;
}
export interface NativeFerricSymbol {
  readonly value: string;
  toString(): string;
  valueOf(): string;
}
export interface NativeFerricInstanceNameConstructor {
  new (value: string): NativeFerricInstanceName;
}
export interface NativeFerricInstanceName {
  readonly value: string;
  toString(): string;
  valueOf(): string;
}
export declare const FerricSymbol: NativeFerricSymbolConstructor;
export declare const FerricInstanceName: NativeFerricInstanceNameConstructor;

export interface WireSymbolObject {
  __type: "FerricSymbol";
  value: string;
}

// Structural helpers used by ClipsValue; these three names are not root exports.
interface FerricSymbolInstance {
  readonly value: string;
  toString(): string;
  valueOf(): string;
}
interface FerricInstanceNameInstance {
  readonly value: string;
  toString(): string;
  valueOf(): string;
}
interface WireInstanceNameObject {
  __type: "FerricInstanceName";
  value: string;
}

export type ClipsValue =
  | FerricSymbolInstance
  | WireSymbolObject
  | FerricInstanceNameInstance
  | WireInstanceNameObject
  | string
  | number
  | bigint
  | boolean
  | ClipsValue[]
  | null;
```

Plain strings become CLIPS strings. Use `new FerricSymbol("foo")` for a symbol
and `new FerricInstanceName("widget")` for an instance name. Booleans become the
symbols `TRUE` and `FALSE`. Compare wrapper spellings with `.value`; separate
wrapper objects do not become equal under JavaScript `===`.

`null` is in `ClipsValue` to represent returned CLIPS Void. Both `null` and
`undefined` are rejected as fact inputs, including inside nested multifields.
The union is therefore broader than the accepted fact-input domain. Arbitrary
objects that merely satisfy a structural interface are not a replacement for
the constructors or canonical tagged wire objects. The package also exports
`WireSymbol` and `WireInstanceName` with those tagged transport shapes; the
worker-backed APIs handle their conversion automatically.

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
  Json = 1, // debugging and inspection
  Cbor = 2, // recommended default
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

/** Opaque, engine-scoped unsigned 64-bit fact handle. */
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
  /** Requested call depth. Default: 64; effective runtime ceiling: 32. */
  maxCallDepth?: number;
}
```

`maxCallDepth` accepts integers in `0..=4294967295`; zero disallows user-function
calls. Strategy, encoding, and format selectors must be exact enum members even
where the native declarations below use `number`.

### Error Hierarchy

```typescript
export class FerricError extends Error {
  readonly code: string;
  constructor(message: string, code: string);
}

export class FerricParseError extends FerricError { constructor(message: string); }
export class FerricCompileError extends FerricError { constructor(message: string); }
export class FerricRuntimeError extends FerricError { constructor(message: string); }
export class FerricFactNotFoundError extends FerricError { constructor(message: string); }
export class FerricTemplateNotFoundError extends FerricError { constructor(message: string); }
export class FerricSlotNotFoundError extends FerricError { constructor(message: string); }
export class FerricModuleNotFoundError extends FerricError { constructor(message: string); }
export class FerricEncodingError extends FerricError { constructor(message: string); }
export class FerricSerializationError extends FerricError { constructor(message: string); }
export class FerricIOError extends FerricError { constructor(message: string); }

/** Host-side EnginePool admission failure; never crosses the Worker wire. */
export class EnginePoolQueueFullError extends FerricError {
  readonly capacity: number;
  readonly queued: number;
  readonly slotIndex: number;

  constructor(capacity: number, queued: number, slotIndex: number);
}
```

`FerricIOError` reports filesystem failures. Native and worker errors preserve
the Ferric class and its `code`. `ERROR_REGISTRY` is exported as
`Readonly<Record<string, (message: string) => FerricError>>`; its entries are
factories, not constructors. The host-only `EnginePoolQueueFullError` is not in
that registry.

### Engine (synchronous, native)

The synchronous `Engine` is the core building block. All methods are synchronous and execute on the calling thread. It is suitable for scripts, CLI tools, short-lived evaluations, and as the backing implementation inside worker threads.

`Engine` is a constructor value typed as `NativeEngineConstructor`; annotate an
instance as `NativeEngine` (or `InstanceType<typeof Engine>`). The following are
the current public declarations. Native field inputs and some return values use
`unknown`, while selectors and the native halt reason use `number`; runtime
validation still enforces the value and enum contracts described here.

```typescript
export interface NativeEngineConstructor {
  new (options?: { strategy?: number; encoding?: number; maxCallDepth?: number }): NativeEngine;
  fromSource(source: string, options?: { strategy?: number; encoding?: number; maxCallDepth?: number }): NativeEngine;
  fromSnapshot(data: Buffer, format?: number): NativeEngine;
  fromSnapshotFile(path: string, format?: number): NativeEngine;
}

/** Shape of a native Engine instance. */
export interface NativeEngine {
  load(source: string): void;
  loadFile(path: string): void;
  assertString(source: string): FactId[];
  assertFact(relation: string, ...fields: unknown[]): FactId;
  assertTemplate(templateName: string, slots: Record<string, unknown>): FactId;
  retract(factId: FactIdInput): void;
  getFact(factId: FactIdInput): Fact | null;
  facts(): Fact[];
  findFacts(relation: string): Fact[];
  getFactSlot(factId: FactIdInput, slotName: string): unknown;
  run(limit?: number): { rulesFired: number; haltReason: number };
  step(): { ruleName: string } | null;
  halt(): void;
  reset(): void;
  clear(): void;
  readonly factCount: number;
  readonly isHalted: boolean;
  readonly agendaSize: number;
  readonly currentModule: string;
  readonly focus: string | null;
  readonly focusStack: string[];
  rules(): Array<{ name: string; salience: number }>;
  templates(): string[];
  modules(): string[];
  getGlobal(name: string): unknown | null;
  setFocus(moduleName: string): void;
  pushFocus(moduleName: string): void;
  getOutput(channel: string): string | null;
  clearOutput(channel: string): void;
  pushInput(line: string): void;
  readonly diagnostics: string[];
  clearDiagnostics(): void;
  serialize(format?: number): Buffer;
  saveSnapshot(path: string, format?: number): void;
  close(): void;
  [Symbol.dispose](): void;
}
export declare const Engine: NativeEngineConstructor;
```

`fromSource` performs construction, `load`, then `reset`. Snapshot factories
restore the saved state; omitted snapshot formats use CBOR. `assertString`
accepts one or more source facts, and all assertion methods return opaque fact
handles. `getGlobal` takes the name without the `?*` / `*` delimiters and returns
`null` if the global is not found or visible in the current module. `findFacts`
selects ordered facts by relation; use `facts()` and filter `templateName` for
template facts. This applies to the async wrappers as well.

`run()` starts a fresh logical run, clearing prior halt requests and action
diagnostics. Omitting `limit` runs without a firing limit; zero fires no rules
and returns `LimitReached`. `step()` returns the fired rule or `null` when no rule
fires. `reset()` reinitializes working memory from deffacts while retaining
constructs; `clear()` removes constructs and facts. The `focusStack` array is
ordered from bottom to top. `setFocus` replaces that stack; `pushFocus` pushes a
module, except that pushing the module already at the top leaves the stack
unchanged (a module deeper in the stack may be pushed again).

Output uses raw channel names such as `"t"` and `"stderr"`. Read `diagnostics`
after an action error before starting another run. `close()` and
`[Symbol.dispose]()` release the native engine; close is idempotent, and later
operational calls fail. TypeScript 5.2 or newer supports `using` declarations.

### EngineHandle (async, worker-backed)

`EngineHandle` wraps a synchronous `Engine` running on a dedicated Worker thread. Its operations return Promises and offload native work from the owning JavaScript thread. The handle itself is not transferable to another Worker.

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
concurrent public `close()` calls now waits for the same complete cleanup.

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
  private constructor(); // Obtain instances through create().
  /**
   * Create an EngineHandle backed by a dedicated Worker thread.
   * The Engine is created and owned by the worker thread.
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

`EngineHandle` exposes only the methods listed above: it has no `getFactSlot`,
focus mutation, diagnostics, or `saveSnapshot` method. To save asynchronously,
await `serialize()` and pass its Buffer to `node:fs/promises.writeFile`. Individual
calls are serialized by the worker; a sequence of calls is not a transaction or
an exclusive request session. Use a pool `do()` lease for that isolation.

### EnginePool (concurrent evaluation)

`EnginePool` manages multiple Worker threads for concurrent, stateless evaluation.

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
  private constructor(); // Obtain instances through create().
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
   * Shut down all workers. Resolves after in-flight requests and callbacks that
   * already acquired a worker-slot lease complete.
   */
  close(): Promise<void>;

  [Symbol.asyncDispose](): Promise<void>;
}

/**
 * Proxy object passed to EnginePool.do() callbacks.
 * Exposes the subset below, dispatched to a specific worker's engine. Calls are serialized in invocation
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
  hostile removal remains best-effort. Successful root dispatch also removes its
  queue listener before sending.
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
another Worker. An exit after `close()` deliberately starts Worker termination
is expected. Concurrent `close()` calls share the cleanup completion barrier.

## Value Conversion Details

### JS → CLIPS

| JS type | CLIPS type | Notes |
|---------|-----------|-------|
| `FerricSymbol` or its canonical wire object | Symbol | Explicit symbol spelling |
| `FerricInstanceName` or its canonical wire object | Instance name | `new FerricInstanceName("widget")` is `[widget]` |
| `string` | String | Quoted CLIPS string |
| `number` (safe integer) | Integer | `Number.isSafeInteger(n)`; unsafe integers rejected |
| `number` (float) | Float | |
| `bigint` | Integer | Must fit signed 64-bit range |
| `boolean` | Symbol | `true` → `TRUE`, `false` → `FALSE` |
| `Array` | Multifield | Recursive conversion |
| `null` / `undefined` | Rejected as fact input | Also rejected inside nested multifields |

Fact inputs allow at most 32 multifield levels and one million values per assertion.

### CLIPS → JS

| CLIPS type | JS type | Notes |
|-----------|---------|-------|
| Symbol | `FerricSymbol` | Always wrapped |
| Instance name | `FerricInstanceName` | Always wrapped; `value` omits the brackets |
| String | `string` | Plain JS string |
| Integer | `number` or `bigint` | `bigint` only if abs value > `2^53 - 1` |
| Float | `number` | |
| Multifield | `ClipsValue[]` | Recursive |
| Void | `null` | |
| FactAddress / ExternalAddress | Rejected | Explicit unsupported-value error, including inside multifields |

### Integer Representation

CLIPS integers are `i64`. JavaScript `number` is a 64-bit IEEE 754 float with 53 bits of integer precision. The binding:

- Returns `number` for integers in `[-(2^53-1), 2^53-1]`.
- Returns `bigint` for integers outside that range.
- Accepts both `number` and `bigint` for assertion.

This avoids silent precision loss while keeping the common case (small integers) ergonomic.

### Fact Identifier Representation and Migration

Fact identifiers are not CLIPS integer values. They are opaque unsigned 64-bit,
engine-scoped handles, so Ferric exposes every returned ID as the
canonical `FactId = bigint` representation even when its current value would
fit in a JavaScript safe integer. This applies to `assertString`, `assertFact`,
`assertTemplate`, and the `id` property returned by `getFact`, `facts`, and
`findFacts`.

These handles belong to the engine that returned them. Reset, clear, or restore
requires fresh handles queried from durable application fields. They are not
CLIPS fact-address values and cannot be substituted for those values in facts.

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
- Pass a handle returned by the same engine. Do not invent or reconstruct handles
  from CLIPS fact indices. Existing safe numeric inputs remain accepted during migration.
- Never convert a returned ID with `Number(id)`, because doing so can discard
  its identity.
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
export interface WorkerRequest {
  id: number;             // monotonic request ID
  method: string;         // engine method name
  args: unknown[];        // structured-clonable arguments
}

// Worker → Main
export interface WorkerResponse {
  id: number;             // matches request ID
  result?: unknown;       // return value (if success)
  error?: WorkerErrorPayload; // error info (if failure)
}
```

Symbols and instance names use canonical tagged wire objects, and returned
values are reconstructed as native wrappers. Fact IDs remain `bigint` through
structured clone. Snapshot transport copies the Buffer's byte range into an
`ArrayBuffer` and transfers that buffer; it does not detach the caller's Buffer.

The package exports the request and response types above, plus these transport
types and helpers. Applications normally use `EngineHandle` or `EnginePool`
directly. `ABORT_BUFFER_SIZE` counts Int32 elements, not bytes.

```typescript
export interface WorkerErrorPayload {
  name: string;
  message: string;
  code: string;
}
export interface WorkerInit {
  options?: { strategy?: number; encoding?: number; maxCallDepth?: number };
  source?: string;
  snapshot?: { data: ArrayBuffer; format?: number };
}
export interface PoolWorkerInit {
  specs: Array<{
    name: string;
    options?: { strategy?: number; encoding?: number; maxCallDepth?: number };
    source?: string;
  }>;
}
export interface WireSymbol { __type: "FerricSymbol"; value: string }
export interface WireInstanceName { __type: "FerricInstanceName"; value: string }
export function isWireSymbol(value: unknown): value is WireSymbol;
export function isWireInstanceName(value: unknown): value is WireInstanceName;
export function toWire(value: unknown): unknown;
export function fromWire(
  value: unknown,
  FerricSymbolCtor?: new (value: string) => unknown,
  FerricInstanceNameCtor?: new (value: string) => unknown,
): unknown;
export const ABORT_FLAG_INDEX = 0;
export const ABORT_BUFFER_SIZE = 1; // Int32 elements
export const RUN_BATCH_SIZE = 100; // Rule firings per cancellation check
```

`toWire` recursively converts name wrappers to tagged objects. `fromWire`
reconstructs the corresponding wrappers when given their constructors; without
a constructor it retains the tagged representation.

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
Worker-to-main response sends create no such registration and follow the Worker
error/exit lifecycle. A queued unit is reclaimed when removed for dispatch;
send rollback must not reclaim it a second time. Successful root dispatch
removes its queue listener before sending, and concurrent close callers share
the cleanup completion barrier.

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
import { Engine } from "@ferric-rules/node";

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
import { Engine } from "@ferric-rules/node";

{
  using engine = Engine.fromSource(`
    (defrule hello (initial-fact) => (printout t "Hello!" crlf))
  `);
  engine.run();
  console.log(engine.getOutput("t"));
} // engine.close() called automatically
```

### Non-blocking Evaluation

```typescript
import { EngineHandle } from "@ferric-rules/node";

const handle = await EngineHandle.create({
  source: `
    (deftemplate order (slot id) (slot total))
    (defrule big-order
      (order (id ?id) (total ?t&:(> ?t 1000)))
      =>
      (printout t "Large order: " ?id crlf))
  `,
});

// Run this sequence without concurrent callers sharing the handle.
// For concurrent server requests, use EnginePool.evaluate() or do().
async function handleRequest(orderId: string, total: number) {
  await handle.reset();
  await handle.assertTemplate("order", {
    id: orderId,
    total,
  });

  const controller = new AbortController();
  const timeout = setTimeout(() => controller.abort(), 5000);
  try {
    const result = await handle.run({ signal: controller.signal });
    const output = await handle.getOutput("t");
    return { rulesFired: result.rulesFired, output };
  } finally {
    clearTimeout(timeout);
  }
}

// On shutdown:
await handle.close();
```

### Concurrent Evaluation Pool

```typescript
import fs from "node:fs";
import { EnginePool, FerricSymbol } from "@ferric-rules/node";

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
import { EnginePool, FerricSymbol } from "@ferric-rules/node";

const pool = await EnginePool.create([{
  name: "pricing",
  source: `
    (deftemplate customer (slot tier) (slot years))
    (deftemplate item (slot sku) (slot basePrice))
    (defrule price
      (customer (tier gold) (years ?years))
      (item (basePrice ?base))
      => (assert (final-price (- ?base ?years))))
  `,
}]);

try {
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
    return facts[0]?.fields[0];
  });
  console.log(score);
} finally {
  await pool.close();
}
```

For native ownership, worker implementation, and package loading, see the
[Architecture](typescript-binding-architecture.md). The package targets Node.js;
browser/Wasm, Deno/Bun compatibility, user-defined JavaScript RHS callbacks, and
rule-firing event streams are not public API features.
