---
title: Getting started
description: Add ferric-rules from Git, walk through a small forward-chaining program, or try the interactive REPL.
---

Because `ferric-rules` is pre-1.0 and not published to crates.io yet, Rust projects depend on the public `ferric-rules` facade crate directly from GitHub.

## Add the Git dependency

In an existing Cargo binary project:

```sh
cargo add --git https://github.com/plx/ferric-rules ferric-rules
```

Or add it directly in `Cargo.toml`:

```toml
[dependencies]
ferric-rules = { git = "https://github.com/plx/ferric-rules" }
```

Commit your project's `Cargo.lock` (or specify `rev = "..."` in `Cargo.toml`) if you want builds pinned to a specific revision while the repository evolves. Cargo package names use hyphens (`ferric-rules`), while Rust imports use underscores (`ferric_rules`).

## Write a small rule set

Save this as `src/rules.clp`:

```lisp
(defrule flag-faulty-sensor
  (declare (salience 10))
  (sensor-error ?id)
  =>
  (assert (suppressed ?id)))

(defrule high-temp-alert
  (reading ?id ?temp)
  (test (> ?temp 75))
  (not (suppressed ?id))
  =>
  (assert (alert ?id high-temperature))
  (printout t "Alert on " ?id ": " ?temp "C" crlf))
```

Two rules interact here through working memory:

1. `flag-faulty-sensor` declares higher salience (`10` vs. the default `0`), so if `(sensor-error ?id)` is present, it fires first and derives `(suppressed ?id)`.
2. `high-temp-alert` matches any `(reading ?id ?temp)` above `75` only when no matching `(suppressed ?id)` fact exists. When it fires, it both asserts a structured `(alert ...)` fact and writes a line to the standard CLIPS output channel `t`.

## Drive the engine from Rust

Save this as `src/main.rs` and run `cargo run`:

```rust
use ferric_rules::runtime::{Engine, HaltReason, RunLimit};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut engine = Engine::with_rules(include_str!("rules.clp"))?;

    // Pass 1: healthy sensor reports 82C -> high-temp-alert fires.
    let kiln_1 = engine.symbol_value("kiln-1")?;
    engine.assert_ordered("reading", vec![kiln_1, 82_i64.into()])?;

    let result = engine.run(RunLimit::Count(100))?;
    assert_eq!(result.rules_fired, 1);
    assert_eq!(result.halt_reason, HaltReason::AgendaEmpty);
    assert_eq!(engine.find_facts("alert")?.len(), 1);
    assert_eq!(engine.get_output("t"), Some("Alert on kiln-1: 82C\n"));

    // Pass 2: reuse the compiled network with a faulty sensor -> suppressed.
    engine.reset()?;
    engine.assert_ordered_symbol("sensor-error", "kiln-2")?;
    let kiln_2 = engine.symbol_value("kiln-2")?;
    engine.assert_ordered("reading", vec![kiln_2, 90_i64.into()])?;

    let result = engine.run(RunLimit::Count(100))?;
    assert_eq!(result.rules_fired, 1);
    assert!(engine.find_facts("alert")?.is_empty());
    assert_eq!(engine.get_output("t"), None);

    Ok(())
}
```

Note that `(printout t ...)` does not write directly to the host process's stdout: `ferric-rules` buffers output per named channel inside the engine so embedded callers can inspect or route it without global I/O side effects.

## Typical engine lifecycle

1. **Compile once:** `Engine::with_rules` parses the CLIPS source, compiles constructs into the Rete network, and performs an initial `(reset)` so `(initial-fact)` and any `deffacts` are ready.
2. **Assert input facts:** Use `assert_ordered`, `assert_ordered_symbol`, or `assert_template_slots` to describe the current request, session, or domain snapshot.
3. **Run with a budget:** `engine.run(RunLimit::Count(n))` bounds how many rules may fire in one call; inspect `result.halt_reason` to distinguish a drained agenda (`AgendaEmpty`) from hitting the step cap (`LimitReached`) or a mid-rule error (`ActionError`).
4. **Read outputs:** Query derived facts in working memory (`find_facts`, `facts`, `get_fact`) or read buffered router output (`get_output`).
5. **Reset and reuse:** Calling `engine.reset()` clears transient facts, activations, and channel buffers while keeping the compiled Rete network intact for the next run.

## Trying rules from the CLI

If you want to experiment with `.clp` files directly without writing host code first, clone the repository and run the `ferric` CLI crate:

```sh
cargo run -p ferric-rules-cli -- repl
cargo run -p ferric-rules-cli -- run examples/embedding/launch-selection.clp
cargo run -p ferric-rules-cli -- check examples/embedding/launch-selection.clp
```

## Toolchain and MSRV note

The Rust workspace crates declare a minimum supported Rust version (MSRV) of **1.75** (the Python binding crate requires **1.83** for PyO3). On current stable Rust toolchains, `cargo build` works out of the box. If you specifically need to resolve a fresh lockfile for an older compiler near 1.75, set `rust-version` in your `Cargo.toml` and generate the lockfile with Cargo's MSRV-aware resolver (`cargo generate-lockfile --config 'resolver.incompatible-rust-versions="fallback"'`).

Next, see [Embedding & FFI](../embedding/) for thread ownership, host handles, snapshots, and non-Rust bindings, or browse the [worked examples](https://github.com/plx/ferric-rules/tree/main/examples/users-guide) in the repository.
