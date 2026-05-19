---
title: Embedding API
description: How ferric-rules is organized for host applications and independent engine instances.
---

Ferric is designed for embedding. The key design constraint is that each engine instance owns its runtime state and can be hosted independently.

## Runtime Shape

The core host-facing type is `Engine` from `ferric-runtime`, re-exported through the public `ferric` facade crate.

An engine owns:

- compiled rules and templates,
- working memory facts,
- activations and agenda state,
- module and focus-stack state,
- function, global, and generic registries,
- output router buffers.

## Why Independent Engines Matter

CLIPS is battle-tested, but its C runtime model can be awkward in modern applications with multiple isolated contexts. Ferric avoids global runtime state so an application can hold one engine per tenant, document, app feature, simulation, or user session.

## Host Bindings

The repository includes multiple host-facing layers:

| Layer                  | Purpose                                                  |
| ---------------------- | -------------------------------------------------------- |
| `ferric`               | Public Rust facade crate.                                |
| `ferric-runtime`       | Engine, execution environment, value types, and routing. |
| `ferric-ffi`           | C ABI over the runtime.                                  |
| `bindings/go`          | Go binding on top of the FFI.                            |
| `crates/ferric-python` | PyO3 extension module.                                   |
| `packages/ferric`      | TypeScript package work.                                 |

Future embedding targets can reuse the same runtime and FFI boundary.

## Embedding Pattern

```rust
use ferric::runtime::{Engine, RunLimit};

let mut engine = Engine::with_rules(include_str!("rules.clp"))?;

engine.assert_ordered_symbol("user-tier", "free")?;
engine.run(RunLimit::Count(100))?;

for (_id, fact) in engine.facts()? {
    println!("{fact:?}");
}
```

Use a fresh engine when you need a fresh isolated decision context.
