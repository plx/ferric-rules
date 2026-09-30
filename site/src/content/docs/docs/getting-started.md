---
title: Getting started
description: Run a small Rust program with one rule and one fact.
---

This example loads a rule, asserts a temperature reading, and checks the output.

## Install

In a Rust binary project, add the crate:

```sh
cargo add ferric-rules
```

The Cargo package is `ferric-rules`; Rust imports use `ferric_rules`.
The minimum supported Rust version is 1.75.

## Minimal rule set

Save this as `src/rules.clp`:

```text
(defrule high-temperature
  (temperature ?t)
  (test (> ?t 75))
  =>
  (printout t "High temperature" crlf))
```

`(temperature ?t)` matches an ordered fact with one field. The `test` condition
checks its value. Everything after `=>` runs when the rule fires.

## Rust host code

Save this as `src/main.rs`:

```rust
use ferric_rules::runtime::{Engine, HaltReason, RunLimit};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut engine = Engine::with_rules(include_str!("rules.clp"))?;
    engine.assert_ordered("temperature", 80_i64)?;

    let result = engine.run(RunLimit::Count(100))?;
    assert_eq!(result.rules_fired, 1);
    assert_eq!(result.halt_reason, HaltReason::AgendaEmpty);
    assert_eq!(engine.get_output("t"), Some("High temperature\n"));
    Ok(())
}
```

Run it with `cargo run`. The program exits without printing anything:
`printout` writes to an engine buffer, which the host reads with `get_output`.
Change `80_i64` to `70_i64` and the rule no longer matches, so the assertions fail.

## Engine lifecycle

1. `Engine::with_rules` parses and compiles the source, then resets the engine
   to assert `initial-fact` and any `deffacts`.
2. Assert the facts for the current input.
3. Call `run` and check `halt_reason`. Reaching a firing limit is different from
   completing all pending work.
4. Read the resulting facts or output. For application decisions, use
   `find_facts` or `facts()` to read typed values.
5. Call `reset()` to reuse the compiled rules with new input. It clears the
   current facts and output and reasserts the initial facts.

See [Embedding API](../embedding/) for thread ownership and error handling, or the
[worked examples](https://github.com/plx/ferric-rules/tree/main/examples/users-guide)
for templates, priorities, modules, and snapshots.
