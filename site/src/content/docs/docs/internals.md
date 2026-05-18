---
title: Internals
description: High-level map of the ferric-rules workspace and engine architecture.
---

Ferric is a Rust workspace with a small set of core crates and several host-facing layers.

## Core Crates

| Crate | Responsibility |
| --- | --- |
| `ferric-core` | Rete network, pattern matching, agenda, facts, values, and low-level engine data structures. |
| `ferric-parser` | Lexer, S-expression parser, and CLIPS construct AST. |
| `ferric-runtime` | Engine, loader, execution loop, evaluator, modules, functions, routers, and serialization. |
| `ferric` | Public facade crate that re-exports core, parser, and runtime surfaces. |

## Runtime Pipeline

1. Parse CLIPS source into stage-two AST structures.
2. Register templates, modules, functions, globals, and generics.
3. Compile rule patterns into the Rete network.
4. Assert facts into working memory.
5. Propagate changes through alpha and beta nodes.
6. Schedule activations on the agenda.
7. Execute RHS actions and update working memory or output channels.

## Validation Areas

The test suite covers:

- CLIPS compatibility fixtures,
- real-world example corpus work,
- parser and runtime behavior,
- FFI lifecycle and diagnostics,
- language bindings,
- scaling behavior,
- user-guide example synchronization.

## Scope

The internals are organized for correctness first, then performance. Shared APIs should preserve deterministic behavior and independent engine ownership.
