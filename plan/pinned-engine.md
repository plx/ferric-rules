# Rust-Owned Pinned Engine Plan

Date: 2026-05-10
Status: Planning

## Purpose

Ferric's current runtime engine is thread-affine: an engine is bound to the
thread that created it, and every runtime/FFI operation validates that calls
occur on that same thread. This is a safe but inconvenient contract for
bindings whose native concurrency model can serialize access but cannot easily
pin work to one exact OS thread.

This document proposes an additive Rust-owned pinned execution layer. The goal
is to centralize the "dedicated engine thread plus serialized request queue"
pattern that currently exists in bindings such as Go and TypeScript, and to
make the upcoming Swift binding practical without requiring every binding to
reimplement the same thread-management machinery.

## Current State

Runtime:

- `ferric_runtime::Engine` stores a creator `ThreadId` and checks it in most
  public operations.
- `Engine` is intentionally `!Send + !Sync`, currently via `Rc` internals and
  an explicit `PhantomData<*mut ()>` marker.
- `unsafe move_to_current_thread` exists internally but is not exposed through
  C FFI.

FFI:

- `ferric-ffi` exposes raw `FerricEngine *` handles.
- Each `ferric_engine_*` operation checks thread affinity before taking mutable
  access.
- Wrong-thread calls return `FERRIC_ERROR_THREAD_VIOLATION`.
- Diagnostic reads deliberately skip the affinity check.

Bindings:

- Go exposes direct `Engine`, but callers must manage `runtime.LockOSThread`.
- Go's `PinnedEngine`, `Coordinator`, and `Manager` hide affinity by using
  locked worker goroutines and request channels.
- TypeScript exposes a synchronous native `Engine`, plus `EngineHandle` and
  `EnginePool` backed by Node worker threads.
- Python stores engines in a thread-local registry and rejects cross-thread
  calls.
- Swift and C++ are not yet implemented.

## Goals

1. Add Rust-owned pinned execution as an opt-in API without breaking the raw
   `FerricEngine *` contract.
2. Provide a C ABI that can be consumed by Swift, Go, C++, and other bindings.
3. Support one long-lived pinned engine and a coordinator/pool of pinned engine
   threads.
4. Keep engine access serialized on the owning Rust worker thread.
5. Provide deterministic batching semantics.
6. Support Apple autorelease-pool management explicitly and configurably.
7. Keep arbitrary language-level user callbacks off the Rust engine thread.
8. Migrate bindings incrementally.

## Non-Goals

1. Do not make `Engine` itself `Send` in this plan.
2. Do not remove or relax current raw FFI thread-affinity checks.
3. Do not make Rust worker threads into Swift executors.
4. Do not run arbitrary Swift, Go, Python, or JavaScript user closures directly
   on the Rust worker thread.
5. Do not introduce heuristic batching based on timers or queue age.

## High-Level Design

Add a Rust layer with two primary handle types:

- `PinnedEngine`: owns one dedicated Rust worker thread and one `Engine`.
- `EngineCoordinator`: owns N Rust worker threads and lazily creates one engine
  per named spec on each worker.

The public handle is safe to use from any thread. Calls enqueue a request, wake
the worker, wait for completion or cancellation, and return copied/owned
results through the binding-specific API.

Raw engine pointers continue to exist for low-level embedders. The pinned layer
is a higher-level host API.

## Crate Structure

Preferred structure:

```text
crates/
  ferric-pinned/
    src/
      lib.rs
      engine.rs
      coordinator.rs
      queue.rs
      autorelease.rs
      request.rs
      result.rs
```

Then expose the C ABI from `ferric-ffi` by depending on `ferric-pinned`.

Alternative:

- Put the Rust pinned implementation directly in `ferric-ffi`.

The separate crate is preferable because NAPI/PyO3 bindings can use the Rust
API directly if useful, while C/Swift/Go/C++ use the C ABI.

## Configuration

Use one config struct for engine options plus pinned-management options.

Sketch:

```c
typedef enum FerricPinnedAutoreleasePolicy {
    FERRIC_PINNED_AUTORELEASE_NONE = 0,
    FERRIC_PINNED_AUTORELEASE_PER_ITEM = 1,
    FERRIC_PINNED_AUTORELEASE_PER_BATCH = 2,
} FerricPinnedAutoreleasePolicy;

typedef struct FerricPinnedEngineOptions {
    struct FerricConfig engine;
    enum FerricPinnedAutoreleasePolicy autorelease_policy;
    size_t max_batch_size;
    size_t queue_capacity;
    const char *thread_name;
} FerricPinnedEngineOptions;
```

Semantics:

- `autorelease_policy`: controls Rust-managed autorelease pools on Apple
  platforms.
- `max_batch_size == 0`: drain all work currently available when the worker
  becomes idle.
- `max_batch_size > 0`: drain at most that many items per batch.
- `queue_capacity == 0`: use a documented default.
- `thread_name == NULL`: use a documented default.

Do not overload `FerricConfig` with pinned-layer options. `FerricConfig` should
remain the raw engine configuration.

## Apple Autorelease Policy

The pinned layer should expose autorelease-pool behavior analogous to classic
GCD autorelease-frequency options, with Ferric-specific batch semantics.

Policies:

- `NONE`: do not push/pop autorelease pools.
- `PER_ITEM`: wrap each queued item.
- `PER_BATCH`: wrap each drained batch.

Batch definition:

1. The worker blocks while idle until at least one item is available.
2. When woken, it drains all immediately available items, or up to
   `max_batch_size`.
3. It executes that batch in FIFO order.
4. It repeats.

No timers, age thresholds, or dynamic coalescing heuristics are part of the
contract.

Equivalent behavior:

```swift
// per batch
autoreleasepool {
    for item in batch {
        item(engine)
    }
}

// per item
for item in batch {
    autoreleasepool {
        item(engine)
    }
}
```

Rust implementation:

```rust
fn with_autorelease_pool<T>(f: impl FnOnce() -> T) -> T {
    #[cfg(any(
        target_os = "macos",
        target_os = "ios",
        target_os = "tvos",
        target_os = "watchos",
        target_os = "visionos",
    ))]
    {
        apple::with_autorelease_pool(f)
    }

    #[cfg(not(any(
        target_os = "macos",
        target_os = "ios",
        target_os = "tvos",
        target_os = "watchos",
        target_os = "visionos",
    )))]
    {
        f()
    }
}
```

On non-Apple platforms, accept the enum and treat it as a no-op. This keeps
configuration portable and avoids surprising platform-condition failures.

## Callback Policy

The pinned Rust thread may call minimal C ABI callback shims for completion,
cancellation notification, or event delivery. It must not run arbitrary
binding/user closures inline.

Swift-specific guidance:

- The C callback should be a tiny transport shim.
- The shim should wrap callback body work in Swift `autoreleasepool` if it
  touches Swift/Foundation/ObjC.
- The shim should resume a continuation, enqueue onto a Swift actor, or publish
  into an async stream.
- User Swift callbacks should run from Swift concurrency, not from the Rust
  engine thread.

This preserves engine affinity while avoiding accidental dependence on Rust
threads behaving like Swift executors.

## Rust API Sketch

```rust
pub enum AutoreleasePolicy {
    None,
    PerItem,
    PerBatch,
}

pub struct PinnedEngineOptions {
    pub engine_config: EngineConfig,
    pub autorelease_policy: AutoreleasePolicy,
    pub max_batch_size: usize,
    pub queue_capacity: usize,
    pub thread_name: Option<String>,
}

pub struct PinnedEngine {
    // Send + Sync handle. The Engine itself remains on the worker thread.
}

impl PinnedEngine {
    pub fn new(options: PinnedEngineOptions) -> Result<Self, PinnedError>;
    pub fn close(&self) -> Result<(), PinnedError>;

    pub fn load_str(&self, source: &str) -> Result<(), PinnedError>;
    pub fn reset(&self) -> Result<(), PinnedError>;
    pub fn clear(&self) -> Result<(), PinnedError>;
    pub fn run(&self, limit: RunLimit) -> Result<RunResult, PinnedError>;
    pub fn halt(&self) -> Result<(), PinnedError>;
    pub fn serialize(&self, format: SerializationFormat) -> Result<Vec<u8>, PinnedError>;

    pub fn with_engine<R>(
        &self,
        f: impl FnOnce(&mut Engine) -> Result<R, PinnedError> + Send + 'static,
    ) -> Result<R, PinnedError>
    where
        R: Send + 'static;
}
```

The Rust closure API is useful inside Rust but should not be exposed directly
through C ABI. C callers need typed operations or an explicit callback-command
interface with strict lifetime rules.

## C ABI Sketch

Opaque handles:

```c
typedef struct FerricPinnedEngine FerricPinnedEngine;
typedef struct FerricPinnedCoordinator FerricPinnedCoordinator;
```

Lifecycle:

```c
FerricPinnedEngine *
ferric_pinned_engine_new(const FerricPinnedEngineOptions *options);

enum FerricError
ferric_pinned_engine_close(FerricPinnedEngine *engine);

enum FerricError
ferric_pinned_engine_free(FerricPinnedEngine *engine);
```

Typed operations:

```c
enum FerricError
ferric_pinned_engine_load_string(FerricPinnedEngine *engine, const char *source);

enum FerricError
ferric_pinned_engine_reset(FerricPinnedEngine *engine);

enum FerricError
ferric_pinned_engine_run(
    FerricPinnedEngine *engine,
    int64_t limit,
    struct FerricRunResult *out_result
);
```

Async completion API for Swift and other event-loop runtimes:

```c
typedef void (*FerricPinnedCompletionFn)(
    void *context,
    enum FerricError code,
    uint64_t request_id
);

enum FerricError
ferric_pinned_engine_run_async(
    FerricPinnedEngine *engine,
    int64_t limit,
    uint64_t request_id,
    void *context,
    FerricPinnedCompletionFn completion
);
```

The async API should store results in a request-result table or return result
payloads through an explicit C-owned result object. Avoid passing borrowed
engine references or borrowed Rust memory across callback boundaries.

## Error Model

Add pinned-layer errors without reusing `ThreadViolation`:

- `FERRIC_ERROR_CLOSED`: handle already closed.
- `FERRIC_ERROR_CANCELED`: request canceled before completion.
- `FERRIC_ERROR_QUEUE_FULL`: bounded queue rejected new work.
- `FERRIC_ERROR_DISPATCH_FAILED`: worker stopped unexpectedly.

Existing runtime, parse, compile, encoding, serialization, and not-found errors
should preserve the existing mapping.

Per-request errors should be copied into per-pinned-engine error state. The raw
thread-local global error channel remains available for construction failures.

## Queue And Shutdown Semantics

Queue behavior:

- FIFO within one pinned engine.
- One request executes at a time.
- Batched draining affects autorelease-pool lifetime only, not observable order.
- Accepted requests complete with their real result unless cancellation succeeds
  before execution starts.

Shutdown behavior:

- `close` stops accepting new requests.
- Already accepted requests are drained by default.
- Add a future option for immediate shutdown only if there is a concrete user
  need.
- `free` calls `close` if needed, then releases the handle.
- `close` and `free` are idempotent at the binding level; raw C `free` still
  invalidates the pointer after success.

Cancellation:

- Support pre-dispatch cancellation.
- Support cooperative in-flight cancellation for `run`, using a Rust-owned
  cancellation token checked between rule-firing batches.
- Do not claim hard preemption.

## Coordinator Design

The coordinator mirrors Go `Coordinator` and TypeScript `EnginePool`.

Concepts:

- `EngineSpec`: name plus engine config plus optional source/snapshot.
- `CoordinatorOptions`: worker count, queue capacity, autorelease policy,
  batch size, dispatch policy.
- Each worker lazily creates one engine per spec.
- Stateless evaluation does `reset -> assert facts -> run -> collect facts`.
- Stateful dispatch is possible but should be carefully documented.

Initial dispatch policy:

- Round robin.

Future dispatch policies:

- Hash by key.
- Least queued.
- Caller-provided route hint.

## Swift Binding Migration

Use the pinned C ABI from day one.

Suggested shape:

```swift
public actor FerricEngine {
    private let handle: OpaquePointer

    public func load(_ source: String) async throws
    public func reset() async throws
    public func run(limit: Int? = nil) async throws -> RunResult
}
```

Implementation:

- Construct `FerricPinnedEngine` from Swift options.
- Use async C API plus continuations for long-running calls.
- Callback shim resumes continuations or enqueues actor messages.
- Configure autorelease policy from Swift:
  - `.none`
  - `.perItem`
  - `.perBatch(maxBatchSize:)`
- Default to `.perItem` for safety on Apple platforms.
- Consider `.perBatch` as the performance-oriented default only after
  measuring real Swift/Foundation-heavy workloads.

Do not expose raw `FerricEngine *` in the primary Swift API. A low-level escape
hatch can be added later if needed.

## Go Binding Migration

Goal: replace Go-owned locked goroutines with Rust-owned pinned handles while
preserving the public Go API.

Phase 1:

- Add cgo wrappers for `FerricPinnedEngine`.
- Implement an internal `pinnedBackend` interface:

```go
type pinnedBackend interface {
    Close() error
    Do(context.Context, func(*Engine) error) error
    // or typed methods if the closure model is not retained
}
```

- Keep the current Go implementation as `goPinnedBackend`.
- Add a Rust-backed implementation as `rustPinnedBackend`.

Phase 2:

- Port `PinnedEngine` methods to call typed pinned FFI operations.
- Preserve `PinnedEngine` public semantics:
  - safe from any goroutine
  - FIFO serialization
  - close drains accepted work
  - context cancellation before dispatch

Phase 3:

- Port `Coordinator` and `Manager`.
- Map Go `EngineSpec` to Rust `EngineSpec`.
- Move observability either:
  - around Go calls at the Manager level, or
  - into explicit pinned/coordinator callbacks from Rust.

Phase 4:

- Delete or deprecate Go `runtime.LockOSThread` worker implementation after
  stress tests prove equivalent behavior.

Compatibility:

- Direct Go `Engine` can remain as the raw FFI wrapper for low-level use.
- `PinnedEngine`, `Coordinator`, and `Manager` should become Rust-backed by
  default.

Tests to preserve:

- Concurrent access tests.
- Close-race stress tests.
- Cancellation tests.
- Serialization round trips.
- Thread-violation tests for direct `Engine`.

## TypeScript/Node Migration

The existing TypeScript worker model already works and also solves event-loop
blocking. Migration is optional and should be later than Swift/Go.

Possible paths:

1. Keep current JS worker architecture.
   - No change needed.
   - Native `Engine` remains direct.
   - `EngineHandle` and `EnginePool` remain TypeScript-owned worker wrappers.

2. Add NAPI async APIs backed by Rust pinned handles.
   - `EngineHandle` no longer needs a JS worker for affinity.
   - Long-running calls must use NAPI async work or thread-safe functions so
     they do not block the event loop.
   - Error and symbol wire conversion can simplify, but NAPI async result
     conversion must be carefully tested.

Recommendation:

- Do not migrate TypeScript in the first pinned-engine release.
- Revisit after Swift and Go validate the Rust pinned layer.

## Python Migration

Current Python behavior is conservative: cross-thread access raises a runtime
error through a TLS registry.

Options:

1. Keep current `Engine` unchanged.
2. Add `PinnedEngine` as a new class.
3. Later consider making `Engine` a thin wrapper over `PinnedEngine`.

Recommendation:

- Add a new `PinnedEngine` class only if Python users ask for cross-thread or
  async-friendly use.
- Keep existing `Engine` semantics and tests stable initially.

## C++ Binding Plan

Use the pinned C ABI as the default high-level C++ API.

Sketch:

```cpp
class PinnedEngine {
public:
    explicit PinnedEngine(PinnedEngineOptions options);
    ~PinnedEngine();

    void load(std::string_view source);
    RunResult run(std::optional<std::size_t> limit = std::nullopt);
    void reset();

private:
    FerricPinnedEngine* handle_;
};
```

Also expose a low-level raw `Engine` wrapper for embedders who want exact
control and can honor thread affinity themselves.

## Testing Plan

Rust unit tests:

- Worker creates engine on worker thread.
- All operations execute on worker thread.
- FIFO request order.
- `PER_ITEM` wraps each item.
- `PER_BATCH` wraps exactly one drained batch.
- `NONE` does not invoke autorelease hooks.
- `max_batch_size` limits batch size deterministically.
- Close drains accepted requests.
- Closed handle rejects new requests.
- Panic policy matches FFI expectations.

FFI tests:

- Null pointer handling.
- Construction with default options.
- Construction with explicit engine config.
- Construction with each autorelease policy.
- Load/reset/run/facts smoke test.
- Async completion callback smoke test.
- Callback does not receive borrowed engine memory.
- Result/error retrieval after async completion.

Go tests:

- Existing `PinnedEngine` and `Coordinator` tests unchanged.
- Race/stress tests with `-race`.
- Validate no use of `runtime.LockOSThread` is required by public APIs.

Swift tests:

- Actor API smoke tests.
- Continuation completion on success and error.
- Cancellation before dispatch and during long run.
- Autorelease policy tests with Foundation objects in callback shims.
- Repeated long-running workloads under Instruments or leak checks.

TypeScript tests, if migrated:

- Existing conformance suite unchanged.
- Event-loop non-blocking tests.
- Worker-removal regressions if Rust pinned async replaces JS workers.

## Benchmarking Plan

Do not quote performance numbers from debug builds or unit tests.

Measure:

- Raw `Engine` direct call baseline.
- Current Go `PinnedEngine`.
- Rust-backed Go `PinnedEngine`.
- Current TS `EngineHandle`.
- Rust-backed NAPI async handle, if implemented.
- Swift actor over Rust pinned handle.

Use `cargo bench` for Rust engine-level measurements. Binding-level dispatch
latency can use binding-specific benchmarks, but PR descriptions must clearly
separate those numbers from engine benchmark claims.

Important metrics:

- Single operation enqueue/dequeue overhead.
- Batched throughput.
- `run` throughput with and without cancellation checks.
- Memory growth under each autorelease policy on Apple platforms.
- Close/drain latency under load.

## Rollout Plan

Phase 0: Design lock

- Finalize option structs and enum numeric values.
- Decide crate location.
- Decide initial async result ownership model.

Phase 1: Rust pinned engine

- Implement `ferric-pinned::PinnedEngine`.
- Implement queue, batching, shutdown, cancellation token.
- Add Apple autorelease abstraction with test hooks.

Phase 2: C ABI

- Add `FerricPinnedEngine` and option structs.
- Add typed sync operations.
- Add async completion API.
- Add generated header coverage and contract tests.

Phase 3: Swift binding

- Build initial Swift actor API on the pinned C ABI.
- Use `.perItem` default autorelease policy.
- Keep callbacks transport-only.

Phase 4: Go migration

- Add Rust-backed pinned backend.
- Run both implementations behind a temporary build tag or internal feature
  switch.
- Make Rust-backed pinned backend the default after stress/race parity.

Phase 5: Coordinator migration

- Move Go `Coordinator`/`Manager` to Rust coordinator handles.
- Add C++ high-level wrapper.

Phase 6: Optional binding cleanup

- Evaluate Python `PinnedEngine`.
- Evaluate TypeScript NAPI async pinned APIs.

## Open Questions

1. Should the pinned layer live in a new crate or directly in `ferric-ffi`?
2. Should async C API results be retrieved by request ID, passed as owned result
   handles, or copied into caller-provided buffers?
3. Should `close` always drain, or should immediate shutdown be configurable in
   v1?
4. What should the default Apple autorelease policy be for Swift: `PER_ITEM` for
   maximum safety or `PER_BATCH` for lower overhead?
5. Does the coordinator need routing hints in v1, or is round robin enough?
6. Should Go migration preserve `Manager.Do(func(*Engine) error)`, or should it
   move to typed operations only?

## Recommended Initial Decisions

1. Create a new `ferric-pinned` crate.
2. Expose pinned handles through `ferric-ffi`.
3. Keep raw `FerricEngine *` unchanged.
4. Use deterministic batch draining with configurable `max_batch_size`.
5. Default Apple autorelease policy to `PER_ITEM` in Swift-facing helpers.
6. Use Rust pinned engine for Swift first.
7. Migrate Go `PinnedEngine` second.
8. Leave TypeScript and Python unchanged for the initial release.
