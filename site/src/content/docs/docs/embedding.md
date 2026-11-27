---
title: Embedding & FFI
description: Per-engine state isolation, checked host handles, CBOR snapshots, the C ABI, and language bindings.
---

`ferric-rules` is structured around embedding: rules run inside an application-owned `Engine` value with explicit execution budgets, buffered I/O, and checked handles across FFI.

## State isolation and threading

Each `Engine` owns its construct definitions, Rete memories, agenda, globals, and output buffers. Engines share no mutable globals, so an application can keep independent rule sets or per-session engines in the same process without coordination.

In Rust, `Engine` implements `Send + Sync`:

- Ownership of an `Engine` can move freely across threads (for example, into a worker pool or async task).
- Shared `&Engine` references support concurrent read-only inspection.
- Mutating operations (`assert_*`, `retract`, `run`, `reset`, `load_str`) take `&mut self` and require exclusive access.

Over the C ABI, a `FerricEngine*` handle follows a serialized-call contract: one handle may move between OS threads when idle, while concurrent calls on the same handle must be synchronized by the caller. Higher-level bindings build idiomatic concurrency wrappers on top of that rule (such as worker-thread handles and pools in Node.js, and a serial-queue actor/async wrapper in Swift).

## Host values and checked fact handles

Inside the Rete network, symbols and facts live in compact arena tables. Exposing raw arena indices across an embedding or FFI boundary makes it too easy to pass a retracted handle or a symbol interned in Engine A into Engine B.

To prevent silent cross-engine bugs, the host API separates external handles from internal Rete keys:

- **`SymbolHandle` and `HostValue`:** Primitive integers (`i64`), floats (`f64`), and UTF-8 strings convert directly into `HostValue`. Symbols are interned through `engine.symbol_value("name")?` and carry their owning engine's identity; passing a foreign symbol (even nested inside a multifield) returns an error instead of resolving the wrong entry.
- **`FactHandle`:** Asserting a fact from host code returns a process-unique 64-bit `FactHandle`. The engine validates ownership and liveness on lookup or retraction, rejecting stale handles after retraction, `reset()`, `clear()`, or snapshot restore.
- **Rule-level `FACT-ADDRESS` vs. host IDs:** Inside CLIPS rules, `?f <- (pattern)` binds a `FACT-ADDRESS` value (printed as `<Fact-N>`). Those addresses belong to rule evaluation and are intentionally not interchangeable with host `FactHandle` integers. When your application needs durable correlation across resets or snapshots, store your own application ID in a fact field or template slot.

See the repository's [host API notes](https://github.com/plx/ferric-rules/blob/main/docs/host-api.md) for full details.

## Run limits, errors, and buffered I/O

`engine.run(RunLimit::Count(n))` caps execution at `n` rule firings (`RunLimit::Unlimited` runs until the agenda empties). It returns a `RunResult` with `rules_fired` and a `HaltReason`:

| `HaltReason`    | Meaning                                                                                             |
| --------------- | --------------------------------------------------------------------------------------------------- |
| `AgendaEmpty`   | No runnable activations remain on the current module focus stack.                                   |
| `LimitReached`  | Execution paused after reaching the requested firing limit; pending activations remain queued.      |
| `HaltRequested` | A rule action or host call requested `(halt)`.                                                      |
| `ActionError`   | An expression on a rule's right-hand side failed; the remainder of that rule's actions was skipped. |

Matching CLIPS behavior, a runtime error inside one rule's actions (such as dividing by zero) records diagnostics and halts the run as `Ok(RunResult { halt_reason: HaltReason::ActionError, .. })` rather than discarding earlier side effects. Inspect `engine.action_diagnostics()` before the next `run`, `step`, or `reset` call (which clear the buffer).

Output from `printout` and `format` accumulates in named per-engine string buffers (`engine.get_output("t")`). Callers that need interleaved chronological ordering across multiple channels can opt into `enable_output_events()` and consume `drain_output_events()`.

## Engine snapshots

Enabling the Cargo `serde` feature (`ferric-rules = { ..., features = ["serde"] }`) adds snapshot serialization via versioned CBOR (`SerializationFormat::Cbor`), along with JSON for debugging and inspection.

A snapshot captures the full state needed to resume execution identically—compiled constructs, facts, internal Rete tokens, blocker chronology for `not`/`exists`/negated conjunctions, refraction history, module focus, and pending agenda activations—without executing code or installing callbacks during decode. Because Rete state is tightly coupled to compiler invariants, snapshots use an explicit schema version (currently `13`) with checksum and structural validation on restore; older schema versions are rejected rather than auto-migrated. See [`docs/snapshots.md`](https://github.com/plx/ferric-rules/blob/main/docs/snapshots.md).

## C ABI and language bindings

None of these packages are published to external registries yet; each builds from a local checkout of the repository. A shared language-neutral conformance suite (`just bindings-conformance`) exercises the same rule, fact, error, and snapshot scenarios across the language adapters.

| Target                | Location & documentation                                                                                                                                                      | Current scope                                                                                                                                                            |
| --------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| **Rust**              | [`crates/ferric-rules`](https://github.com/plx/ferric-rules/tree/main/crates/ferric-rules), [User's guide](https://github.com/plx/ferric-rules/blob/main/docs/users-guide.md) | Native facade crate exposing runtime, parser, and core APIs.                                                                                                             |
| **C ABI**             | [`crates/ferric-rules-ffi`](https://github.com/plx/ferric-rules/tree/main/crates/ferric-rules-ffi) (`ferric.h`)                                                               | Static/dynamic C library. Every `extern "C"` entry point is wrapped in proc-macro panic containment and tested under C, AddressSanitizer, and ThreadSanitizer harnesses. |
| **TypeScript / Node** | [`packages/ferric`](https://github.com/plx/ferric-rules/tree/main/packages/ferric), [API docs](https://github.com/plx/ferric-rules/blob/main/docs/typescript-binding-api.md)  | `napi-rs` native addon with synchronous `Engine`, worker-thread `EngineHandle`, and `EnginePool`.                                                                        |
| **Python**            | [`crates/ferric-rules-python`](https://github.com/plx/ferric-rules/tree/main/crates/ferric-rules-python)                                                                      | PyO3 extension targeting CPython's `abi3` stable ABI (Python 3.9+), built locally with `maturin`.                                                                        |
| **Swift**             | [`bindings/swift`](https://github.com/plx/ferric-rules/blob/main/bindings/swift/README.md)                                                                                    | Local Swift 6 package over an XCFramework build (macOS 15 / iOS 18) with `Sendable` value types, serial-queue isolation, and cooperative task cancellation.              |
| **Go**                | [`bindings/go`](https://github.com/plx/ferric-rules/tree/main/bindings/go)                                                                                                    | `cgo` binding over the C ABI; kept building and passing conformance tests in maintenance mode.                                                                           |
