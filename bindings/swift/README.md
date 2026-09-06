# Ferric for Swift

An asynchronous Swift 6 package over Ferric's C ABI. It supports macOS 15 and
iOS 18 or newer, using the standard library's `Synchronization.Mutex` to own the
native state. The local build provides Apple Silicon macOS, arm64 iOS devices,
and Apple Silicon iOS simulators. Intel slices are not currently provided.

## Build and use a local package

Install Xcode with its macOS and iOS SDKs, select it with `xcode-select`, and
install the Rust toolchain declared in the repository. From the repository root:

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

## Ownership and concurrency

`Engine` is `Sendable`. Independent tasks can share it. Every native call,
including reads, conversion, error copying, and destruction, executes on its
serial dispatch queue. Native work does not block the main actor or a Swift
cooperative executor. This uses Ferric's serialized thread-transfer contract;
the queue is not assumed to remain on one OS thread.

`close()` waits behind previously queued work and releases the native engine
exactly once. Concurrent operations either finish with owned results or throw
`EngineError.closed`, depending on queue order. Repeated close succeeds.
Dropping the final Swift reference schedules the same cleanup without blocking
the dropping thread. Prefer explicit close when cleanup completion matters.
An unlimited rule run can delay close indefinitely: use finite `run(limit:)`
batches for potentially unbounded rules. Swift task cancellation does not
interrupt an already queued native operation.

Values, facts, output, snapshot bytes, and error messages are owned Swift data
and remain usable after close. Integers retain all signed 64-bit precision;
symbols and strings are distinct, and nested multifields are supported up to
128 levels. External addresses have no Swift representation and are rejected
explicitly. Embedded NUL text is rejected at C string/value boundaries instead
of being silently truncated. `FactID` belongs to one engine instance: query new
IDs after restore and persist application keys instead of raw fact IDs.

Snapshots use recommended CBOR through the native versioned snapshot API. They
preserve engine state and subsequent rule behavior, subject to the native
snapshot compatibility policy; the wrapper does not erase unsupported values
or attempt its own migration. Corrupt or incompatible input produces an owned
`EngineError.native` diagnostic.

## Validation and shared example

The tests cover independent tasks, overlapping close/use, draining admitted
work, final-reference cleanup, exact typed values, template defaults,
retraction, limited execution, meaningful snapshot continuation, and errors.
Both the tests and external consumer use the canonical
[`examples/embedding/launch-selection.clp`](../../examples/embedding/launch-selection.clp):
three candidates deterministically select `sign-in` at most once for a session,
including after snapshot/resume. Build and smoke scripts reject fixture drift.

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
