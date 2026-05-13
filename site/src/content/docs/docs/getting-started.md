---
title: Getting Started
description: Load CLIPS rules into ferric-rules, assert facts, run the engine, and inspect results.
---

Ferric is built around an `Engine`. Load CLIPS source once, assert facts that describe the current situation, run the engine, and read the resulting facts or captured output.

## Install

```sh
cargo add ferric
```

The public facade crate is `ferric`; the project and repository are named `ferric-rules`.

## Minimal Rule Set

```text
(defrule show-paywall
  (user-tier free)
  (accessed-premium-feature)
  =>
  (assert (show paywall))
  (printout t "ACTION: paywall" crlf))
```

## Rust Host Code

```rust
use ferric::runtime::{Engine, RunLimit};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut engine = Engine::with_rules(r#"
      (defrule show-paywall
        (user-tier free)
        (accessed-premium-feature)
        =>
        (assert (show paywall))
        (printout t "ACTION: paywall" crlf))
    "#)?;

    engine.assert_ordered_symbol("user-tier", "free")?;
    engine.assert_ordered_symbol("accessed-premium-feature", "yes")?;

    let result = engine.run(RunLimit::Count(100))?;
    assert_eq!(result.rules_fired, 1);

    Ok(())
}
```

## Engine Lifecycle

1. Create an engine with rules loaded from a string or file.
2. Reset when you want `initial-fact` and `deffacts` groups asserted.
3. Assert runtime facts from the host application.
4. Run with a limit.
5. Read facts, output channels, and run results.

Use independent `Engine` instances for independent application contexts.
