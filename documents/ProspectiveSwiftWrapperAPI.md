# Prospective Swift Wrapper API for Ferric FFI

## Purpose

Define a Swift-native API that:

1. Feels natural on Apple platforms (`async`/`await`, actor-facing API).
2. Preserves Ferric's thread-affine FFI contract.
3. Avoids mandatory copy-out for all values (keep efficient FFI usage).

This design assumes the Ferric C API remains thread-affine and is not changed to a fully thread-hopping/copy-only model.

## Core Decision

Use a dedicated host thread per `FerricEngine` instance.

All `FerricEngine*` calls run only on that host thread. Swift callers use an actor facade and never touch raw pointers directly.

This gives:

1. Apple-friendly API ergonomics.
2. Strict conformance to Ferric's "same creating thread" contract.
3. No need to weaken Ferric runtime checks.

## Layered Architecture

### Layer 1: `FerricThreadHost` (internal)

Internal transport that owns:

1. Dedicated worker thread.
2. Private work queue.
3. `FerricEngine*` lifecycle (create, use, free) on that same thread.

Responsibilities:

1. Start worker thread.
2. Initialize engine via `ferric_engine_new` (or `_with_config`) on worker thread.
3. Execute submitted operations serially.
4. Shut down cleanly and call `ferric_engine_free` on worker thread.

### Layer 2: `FerricEngine` (public actor)

Actor that exposes the Swift API and delegates all work to `FerricThreadHost`.

Responsibilities:

1. Validate Swift-side arguments.
2. Convert Swift types to FFI inputs.
3. Map `FerricError` to Swift `Error`.
4. Provide async methods and cancellation semantics.

## Public Swift API (Proposed)

```swift
public actor FerricEngine {
    public struct Configuration: Sendable {
        public var strictMode: Bool
        public var strategy: ConflictStrategy
        public init(strictMode: Bool = false, strategy: ConflictStrategy = .depth)
    }

    public enum ConflictStrategy: Sendable {
        case depth
        case breadth
        case lex
        case mea
    }

    public init(configuration: Configuration = .init()) async throws
    deinit

    public func load(_ source: String) async throws
    public func assertFact(_ fact: String) async throws -> FactID
    public func retract(_ factID: FactID) async throws

    public func run(limit: Int64 = -1) async throws -> RunResult
    public func step() async throws -> StepResult

    public func clearErrors() async
    public func shutdown() async
}

public struct FactID: Sendable, Hashable {
    public let rawValue: UInt64
}

public struct RunResult: Sendable {
    public let rulesFired: Int64
}

public enum StepResult: Sendable {
    case fired
    case agendaEmpty
    case halted
}
```

Notes:

1. `shutdown()` is idempotent and recommended for explicit teardown.
2. `deinit` should still perform best-effort cleanup if caller does not call `shutdown()`.
3. API can grow as new FFI endpoints are added.

## Error Model

```swift
public enum FerricEngineError: Error, Sendable {
    case invalidArgument(message: String)
    case parse(message: String)
    case runtime(message: String)
    case notFound(message: String?)
    case outOfMemory(message: String)
    case encoding(message: String)
    case compilation(message: String)
    case threadViolation(message: String)
    case bufferTooSmall(message: String)
    case engineClosed
    case internalBridgeError(message: String)
}
```

Mapping rules:

1. Non-`OK` `FerricError` maps to `FerricEngineError`.
2. Read detail text with copy APIs (`ferric_engine_last_error_copy`, `ferric_last_error_global_copy`).
3. Prefer engine-local error message when engine pointer is available.
4. Fall back to global error message when engine is unavailable or engine message is empty.

## Threading and Lifetime Contract

Swift users get this guarantee:

1. No concurrent access to the same engine instance.
2. No cross-thread FFI handle usage.
3. No public API exposing raw borrowed pointers that could outlive engine operations.

Bridge implementors must enforce:

1. Engine pointer is created and freed on host thread.
2. Every engine operation closure executes on host thread.
3. No operation closure stores engine-owned borrowed memory outside closure scope.

## Cancellation Semantics

Default policy:

1. Cancellation before host execution: request is dropped and throws `CancellationError`.
2. Cancellation after operation has started: operation runs to completion (FFI call is synchronous).
3. For long runs, expose cooperative cancellation by using repeated `step()` or bounded `run(limit:)`.

Future extension:

1. If Ferric FFI exposes `halt` API, wrapper can issue halt from queued control operations.

## Internal Host Transport (Recommended Shape)

Implementation options:

1. Swift-only host thread + lock/condition-variable queue.
2. Private C++ helper queue/thread primitive, called from Swift.

Given prior embedding experience, option 2 is reasonable and keeps queue/thread mechanics off the Swift actor path.

Minimal host contract:

```swift
protocol FerricThreadHostProtocol: Sendable {
    func start(configuration: FerricEngine.Configuration) async throws
    func submit<T: Sendable>(_ op: @Sendable @escaping (UnsafeMutablePointer<FerricEngineOpaque>) throws -> T) async throws -> T
    func stop() async
}
```

`submit` always:

1. Enqueues work.
2. Executes on host thread with engine pointer.
3. Returns result/error to caller continuation.

## Example Usage

```swift
let engine = try await FerricEngine(
    configuration: .init(strictMode: true, strategy: .depth)
)

try await engine.load("""
    (deftemplate person (slot name))
    (defrule hello (person (name ?n)) => (printout t "Hello " ?n crlf))
""")

let id = try await engine.assertFact(#"(person (name "Alice"))"#)
let run = try await engine.run(limit: -1)
print("Fired:", run.rulesFired)

try await engine.retract(id)
await engine.shutdown()
```

## Performance Notes

1. One host thread per engine is usually acceptable for app-embedded rule engines.
2. Queue hop overhead is small relative to non-trivial rule evaluation.
3. Keep high-frequency patterns batched when practical (for example, assert N facts in one host operation once such FFI exists).

## Test Plan for the Wrapper

1. Lifecycle: create, use, shutdown, double-shutdown.
2. Thread-affinity compliance: assert all FFI calls happen on host thread (debug instrumentation).
3. Error mapping: each `FerricError` code maps correctly and includes message text.
4. Cancellation behavior: queued cancellation vs in-flight operations.
5. Stress: high-volume queued operations and repeated create/destroy cycles.
6. Deinit safety: no leaks or double-free on abandoned engine objects.

## Non-Goals

1. Making Ferric FFI fully thread-hopping safe without affinity.
2. Exposing raw `FerricEngine*` to application code.
3. Guaranteeing forceful interruption of an already-started synchronous FFI call.

## Migration Path if Ferric Later Relaxes Affinity

If Ferric later supports serialized cross-thread calls safely:

1. Keep public actor API unchanged.
2. Replace `FerricThreadHost` internals with direct call path or pooled executor.
3. Retain host abstraction to avoid source-breaking changes.

