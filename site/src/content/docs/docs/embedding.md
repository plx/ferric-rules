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

After a run, `reset()` clears facts, activations, output, and action diagnostics;
it restores globals and the focus stack, then reasserts initial facts. Compiled
rules and templates remain available. `clear()` also removes rules and other
registered constructs.

The [getting-started example](../getting-started/) shows the load, assert, run,
and read sequence. Reuse an engine with `reset()` when successive inputs use the
same rules.

## Run results and errors

`run(RunLimit::Count(n))` permits at most `n` rule firings. Its `RunResult` contains
`rules_fired` and `halt_reason`:

| Halt reason     | Meaning                                                    |
| --------------- | ---------------------------------------------------------- |
| `AgendaEmpty`   | No activations remain.                                     |
| `LimitReached`  | The firing limit was reached.                              |
| `HaltRequested` | Execution was stopped by a halt request.                   |
| `ActionError`   | An action failed; the rest of that activation was skipped. |

A successful Rust `Result` from `run` does not imply that every action succeeded.
For `ActionError`, inspect `action_diagnostics()` before calling `run`, `step`,
or `reset` again: these calls clear the diagnostics. Earlier actions keep their
effects, and later activations remain available for another run.

## Read results

Use `find_facts` to select facts by relation, or `facts()` to iterate working
memory. Use `get_output("t")` to read the captured `printout` channel. Output is
buffered in the engine; it is not automatically written to the host's stdout.

## Other languages

These interfaces are also in the repository. Follow each binding's documentation
for its API, build instructions, and lifecycle rules.

| Interface            | Source and documentation                                                                                                                                                                                                        |
| -------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| C ABI                | [`ferric-rules-ffi`](https://github.com/plx/ferric-rules/tree/main/crates/ferric-rules-ffi), [C contract](https://github.com/plx/ferric-rules/blob/main/docs/compatibility.md#1613-external-interface-contracts-ffi--embedding) |
| Go                   | [`bindings/go`](https://github.com/plx/ferric-rules/tree/main/bindings/go)                                                                                                                                                      |
| Python               | [`ferric-rules-python`](https://github.com/plx/ferric-rules/tree/main/crates/ferric-rules-python)                                                                                                                               |
| TypeScript / Node.js | [`packages/ferric`](https://github.com/plx/ferric-rules/tree/main/packages/ferric), [API reference](https://github.com/plx/ferric-rules/blob/main/docs/typescript-binding-api.md)                                               |

For configuration and snapshots, see the
[Rust user guide](https://github.com/plx/ferric-rules/blob/main/docs/users-guide.md).
