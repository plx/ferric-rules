# Ferric for Swift

An asynchronous Swift 6 package over Ferric's C ABI. It supports macOS 15 and
iOS 18 or newer, using the standard library's `Synchronization.Mutex` to own the
native state. The local build provides Apple Silicon macOS, arm64 iOS devices,
and Apple Silicon iOS simulators. Intel slices are not currently provided.

## Build and use a local package

Install Xcode with its macOS and iOS SDKs and the Rust toolchain declared in the
repository. Select Xcode with `xcode-select`, or set `DEVELOPER_DIR` in the shell
running these commands (for example, `export DEVELOPER_DIR=/Applications/Xcode.app/Contents/Developer`).
From the repository root:

```sh
scripts/build-swift.sh
swift test --package-path bindings/swift -Xswiftc -strict-concurrency=complete
scripts/swift-consumer-smoke.sh
```

The script builds the three native static libraries with `serde` and the
optimized `ffi-release` profile, then generates
`bindings/swift/Artifacts/CFerric.xcframework`. It installs the three Rust target
components when needed. `scripts/build-swift.sh --macos-only` is a faster local
option when iOS is unnecessary. The build retains symbols to avoid an observed
Xcode 27 rejection of stripped Rust proc-macro libraries; optimization and LTO
are unchanged.

Add `bindings/swift` as a local package in Xcode, or use
`.package(path: "/path/to/ferric-rules/bindings/swift")` and depend on the `Ferric`
product in a Swift package. After building, the entire `bindings/swift` directory
can be copied elsewhere: it contains the Swift source, headers, and native
artifact needed by consumers, with license texts and third-party notices in
`Artifacts`. The external smoke test does exactly this and
runs without paths back into the repository. Generated artifacts are ignored by
Git; no registry or signing account is required.

```swift
import Ferric

let engine = try await Engine.create()
try await engine.load("""
    (defrule choose (candidate ?name) => (assert (selected ?name)))
    """)
let candidate = try await engine.assertFact("candidate", fields: [.symbol("welcome")])
let result = try await engine.run(limit: 100)
let facts = try await engine.facts()
try await engine.retract(candidate)
let saved = try await engine.snapshot()
try await engine.close()

let restored = try await Engine.restore(saved)
try await restored.close()
```

`assertTemplate(_:slots:)` asserts declared templates, applying native defaults
for omitted slots. `reset()` reinstalls named deffacts. `output(channel:)`
returns a copied string, or `nil` when the channel has no output. Loads are
incremental according to the engine's supported CLIPS contract. A run that
stops with `.actionError` includes owned messages in `RunResult.diagnostics`;
inspect them when handling a failed rule action.

Create a configured engine with
`Engine.create(config: EngineConfig(stringEncoding: .utf8, strategy: .breadth))`.
The default configuration uses UTF-8, depth ordering, and a requested callable
depth of 64; native evaluation caps callable depth at 32. A depth of zero
disables user-defined calls. Encoding options are `.ascii`, `.utf8`, and
`.asciiSymbolsUTF8Strings`; strategies are `.depth`, `.breadth`, `.lex`, and `.mea`.

The wrapper also exposes these native operations:

| API | Result or behavior |
| --- | --- |
| `step()` | `.fired(diagnostics:)`, `.agendaEmpty`, or `.halted`; action errors accompany a fired step |
| `clear()` | Remove constructs and facts, invalidating old fact IDs |
| `isHalted`, `agendaCount` | Async throwing properties for the native halt flag and pending activation count |
| `global(_:)` | An owned `Value`, or `nil` for the ABI's missing/ambiguous lookup result |
| `pushInput(_:)` | Queue a complete line for `read` or `readline` |
| `clearOutput(channel:)` | Clear one output channel, defaulting to `t` |
| `findFacts(relation:)` | Owned ordered facts for a relation; use `facts()` to include templates |
| `slotValue(_:of:)` | An owned template slot value; foreign, stale, or invalid IDs/slots throw |
| `currentModule`, `focus` | Async throwing properties; focus is `nil` when its stack is empty |
| `focusStack()` | Module names from bottom to top |
| `rules()`, `templates()`, `modules()` | Owned native metadata; rule listings include compiled disjunction branches |

## Ownership and concurrency

`Engine` is `Sendable`. Independent tasks can share it. Every native call,
including reads, conversion, error copying, and destruction, executes on its
serial dispatch queue. Native work does not block the main actor or a Swift
cooperative executor. This uses Ferric's serialized thread-transfer contract;
the queue is not assumed to remain on one OS thread.

`run()` cooperatively checks Swift task cancellation and `engine.halt()` between
bounded native chunks. Both stop the logical run and return its completed rule
count with `.haltRequested`. `halt()` is synchronous and does not wait for the
engine queue. It affects only a run that has started executing on the queue and
does nothing when no run is executing, including while a run is still queued
(for example, immediately after starting it in a new `Task`). To stop a specific
run, including one started moments ago, cancel its task: a cancellation belongs
to one run, is recorded even while that run waits, and canceling an older or
queued task cannot stop a different run.

```swift
let running = Task { try await engine.run() }
// Later, from the task that owns this handle (works even before the run starts):
running.cancel()
let stopped = try await running.value
try await engine.close()
```

The wrapper keeps the entire logical run on one queue operation. Other engine
calls cannot interleave between chunks. A source `(halt)` at an internal chunk
boundary is preserved; there is no need to implement a manual batching loop.
Each public `run` is still a fresh run. An explicit caller limit takes precedence
on its exact final firing, while native terminal results such as `.actionError`
or `.agendaEmpty` take precedence over late cancellation. `run(limit: 0)` retains
the native zero-limit behavior, including clearing earlier halt/diagnostics.
A canceled positive/unlimited run starts fresh and can return zero progress.
Cancellation is cooperative: it cannot interrupt a rule activation while its
actions are executing. The wrapper's `.haltRequested` cancellation result does not
set the native halt flag; `isHalted` reports the native flag.

`close()` marks the engine as closing, requests cancellation of its active run,
then waits for queued cleanup and releases the native engine exactly once.
Runs that have not gained admission throw `EngineError.closed` once closing
begins, including runs already waiting in the queue. Other previously queued
operations finish according to queue order. Repeated close succeeds, and
canceling the task awaiting close does not cancel cleanup. Dropping the final
Swift reference schedules cleanup without blocking the dropping thread. Prefer
explicit close when cleanup completion matters.

Values, facts, output, snapshot bytes, and error messages are owned Swift data
and remain usable after close. Integers retain all signed 64-bit precision;
symbols, strings and instance names (`.instanceName("widget")` for `[widget]`)
are distinct, and nested multifields are supported up to
32 levels and one million aggregate values per assertion. Fact input rejects
`.void`, including nested instances, before allocating C values; it represents
an absent result rather than durable fact data. Use an application symbol such
as `.symbol("nil")` for a stored sentinel. External addresses have no Swift
representation and are rejected explicitly. Rule-created fact addresses (`?f`,
including `<Dummy Fact>` slot defaults and addresses inside multifields) are
rejected through the C ABI as well, so `facts()` throws while any fact holds
one. Use `FactID`s and application keys instead. Embedded NUL text is rejected at C string/value boundaries instead
of being silently truncated. `FactID` retains the full unsigned 64-bit native ID and belongs to one engine
instance. Reset and restore invalidate old IDs; query new IDs and persist
application keys instead of raw fact IDs. Copied symbol/string values are owned
text and can be asserted into a different engine after their source closes.

Snapshots use recommended CBOR through the native versioned snapshot API. They
preserve engine state and subsequent rule behavior, subject to the native
snapshot compatibility policy; the wrapper does not erase unsupported values
or attempt its own migration. Corrupt or incompatible input produces an owned
`EngineError.native` diagnostic.

## Validation and shared example

The tests cover independent tasks, task cancellation and host halt, source halt
at chunk boundaries, queued-run isolation, overlapping close/use, draining
admitted work, final-reference cleanup, exact typed values, template defaults,
retraction, limited execution, meaningful snapshot continuation, and errors.
Both the tests and external consumer use the canonical
[`examples/embedding/launch-selection.clp`](../../examples/embedding/launch-selection.clp):
three candidates deterministically select `sign-in` at most once for a session,
including after pending and completed snapshot/resume. Build and smoke scripts
reject fixture drift.

The iOS wrapper can also be checked without signing or launching an app:

```sh
swift build --package-path bindings/swift --triple arm64-apple-ios18.0 \
  --sdk "$(xcrun --sdk iphoneos --show-sdk-path)" \
  -Xswiftc -strict-concurrency=complete
swift build --package-path bindings/swift --triple arm64-apple-ios18.0-simulator \
  --sdk "$(xcrun --sdk iphonesimulator --show-sdk-path)" \
  -Xswiftc -strict-concurrency=complete
```

The Swift CI job checks all three native slices, both iOS wrapper builds, macOS
tests, and the external macOS consumer. It does not claim device execution.
