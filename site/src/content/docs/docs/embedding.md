---
title: Embedding API
description: Engine lifecycle, thread ownership, output, and language bindings.
---

Use `Engine` from `ferric_rules::runtime`. The `ferric-rules` facade crate
re-exports the runtime, parser, and core types; most Rust applications need only
this dependency.

## Ownership and threads

Each engine owns its rules, facts, agenda, modules, and output buffers. Engines
share no runtime state. An `Engine` is `Send + Sync`: ownership can move between
threads, and shared references may be read concurrently. Mutation requires
exclusive access; use a mutex when multiple threads operate on one engine.
Separate workers can also each own an engine.

## Load and reuse

`Engine::with_rules(source)` compiles CLIPS source and resets the engine. For
configuration, use `Engine::with_rules_config(source, config)` with an
`EngineConfig`.

The host `reset()` method clears facts, activations, channel buffers, and action
diagnostics; it restores globals and the focus stack, then reasserts initial
facts. Compiled rules and templates remain available. The host `clear()` method
also removes rules and other registered constructs. Both preserve pending
deliveries in an enabled output-event journal.

The [getting-started example](../getting-started/) shows the load, assert, run,
and read sequence. Reuse an engine with `reset()` when successive inputs use the
same rules. Source `(reset)` and `(clear)` have additional rules for preserving
active execution and output; see the
[lifecycle contract](https://github.com/plx/ferric-rules/blob/main/docs/migration.md#expression-effects-and-source-lifecycle).

## Run results and errors

`run(RunLimit::Count(n))` permits at most `n` rule firings. Its `RunResult` contains
`rules_fired` and `halt_reason`:

| Halt reason     | Meaning                                                    |
| --------------- | ---------------------------------------------------------- |
| `AgendaEmpty`   | No activation is runnable on the current focus stack.      |
| `LimitReached`  | The firing limit was reached.                              |
| `HaltRequested` | Execution was stopped by a halt request.                   |
| `ActionError`   | An action failed; the rest of that activation was skipped. |

A successful Rust `Result` from `run` does not imply that every action succeeded.
For `ActionError`, inspect `action_diagnostics()` before calling `run`, `step`,
or `reset` again: these calls clear the diagnostics. Earlier actions keep their
effects, and later activations remain available for another run.

## Read results

Use `find_facts` to select ordered facts by relation, or `facts()` to iterate user
facts, including template facts. Host fact handles belong to one engine and
must not be reused after retraction, reset, or clear. Keep durable application
identifiers in fact fields. CLIPS `FACT-ADDRESS` values are a distinct runtime
type; they are not integer host handles.

Use `get_output("t")` to read a captured channel. Output is buffered in the engine;
it is not automatically written to the host's stdout. For ordered delivery across
channels, call `enable_output_events()` before evaluation and consume
`drain_output_events()`. Draining consumes the events and clears the captured
channel buffers.
The event journal is a transient host observer and is not stored in snapshots.

## Other languages

TypeScript, Python, and Swift are primary embedding interfaces alongside Rust.
Go and the C ABI retain maintenance support. Follow each binding's source-build
instructions; these links do not imply that packages have been published.

| Interface            | Source and documentation                                                                                                                                                                                    |
| -------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| TypeScript / Node.js | [`packages/ferric`](https://github.com/plx/ferric-rules/tree/main/packages/ferric), [API reference](https://github.com/plx/ferric-rules/blob/main/docs/typescript-binding-api.md)                           |
| Python               | [`ferric-rules-python`](https://github.com/plx/ferric-rules/tree/main/crates/ferric-rules-python), [source-build guide](https://github.com/plx/ferric-rules/blob/main/crates/ferric-rules-python/README.md) |
| Swift                | [Swift package and build guide](https://github.com/plx/ferric-rules/blob/main/bindings/swift/README.md)                                                                                                     |
| C ABI                | [`ferric-rules-ffi`](https://github.com/plx/ferric-rules/tree/main/crates/ferric-rules-ffi), [host contract](https://github.com/plx/ferric-rules/blob/main/docs/host-api.md)                                |
| Go                   | [`bindings/go`](https://github.com/plx/ferric-rules/tree/main/bindings/go)                                                                                                                                  |

The Swift 6 package supports macOS 15 and iOS 18 through a local XCFramework
build. Engine operations run on a serial queue; task cancellation cooperates
between rule-firing chunks, `halt()` requests a stop synchronously, and closing
an engine stops active work before releasing it. Its public values are owned
and `Sendable`.

For configuration and versioned CBOR snapshots, see the
[Rust user guide](https://github.com/plx/ferric-rules/blob/main/docs/users-guide.md)
and [snapshot contract](https://github.com/plx/ferric-rules/blob/main/docs/snapshots.md).
