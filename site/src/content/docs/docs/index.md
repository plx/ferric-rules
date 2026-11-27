---
title: Overview
description: Why ferric-rules exists, what is in the repository today, and what to expect from an experimental prototype.
---

`ferric-rules` is an experimental Rust implementation of a forward-chaining production system modeled on [CLIPS 6.30](https://www.clipsrules.net/). A host application loads rules written in CLIPS syntax, asserts facts representing current domain state, runs the Rete matching network until the agenda settles (or hits an explicit firing limit), and inspects the resulting working memory or captured output channels.

## Why build this?

CLIPS has been around since the mid-1980s and remains a remarkably clean way to express forward-chaining logic involving structured facts, negation, existential conditions, salience priorities, and modules. Embedding the reference C implementation into modern applications—especially multi-threaded services, mobile apps, or garbage-collected runtimes—takes care around C build integration, manual lifetime tracking, and environment state.

`ferric-rules` explores how close a memory-safe Rust implementation can get to drop-in CLIPS rule compatibility while behaving like a normal library dependency:

- **Self-contained engines:** Every `Engine` owns all of its rules, working memory, agenda, globals, and I/O routers, with no process-wide singletons.
- **Safe core, explicit FFI boundary:** The parser, Rete core, and runtime crates forbid `unsafe` code; `unsafe` is confined to a panic-contained C ABI (`ferric.h`) and host language bindings.
- **Checked host handles & snapshots:** Host fact and symbol handles carry engine provenance so foreign or stale handles fail cleanly instead of indexing into unrelated Rete slots, and engine state can be serialized into versioned CBOR snapshots.

## Current status and expectations

This is a personal side project and an active prototype rather than a supported product:

- **Source-only builds:** Neither the Rust crates nor the language bindings (TypeScript/Node, Python, Swift, Go) are published to crates.io, npm, or PyPI. Everything builds from a Git dependency or a local checkout.
- **Pre-1.0 churn:** Public APIs and the CBOR snapshot schema (currently version 13) change whenever edge-case semantics or host-ownership guarantees need tightening. Older snapshot schema versions are rejected on restore rather than migrated automatically.
- **Tested against fixtures, unvetted in production:** Behavior is checked continuously against a containerized CLIPS 6.30 binary across roughly 1,270 small corpus programs, sanitizer harnesses, and cross-binding conformance suites, but the engine has not been battle-tested in third-party production workloads.
- **Defined language subset:** The core rules, templates, modules, and procedural constructs work; the COOL object system is out of scope, and `logical` truth-maintenance dependencies are not yet implemented (failing explicitly at load time).

## Where to go next

- **[Getting started](./getting-started/):** Add the crate from Git, run a two-rule forward-chaining example, or experiment interactively in the `ferric` CLI REPL.
- **[Embedding & FFI](./embedding/):** How engine ownership, threading, `FactHandle` provenance, CBOR snapshots, the C ABI, and host bindings fit together.
- **[CLIPS compatibility](./compatibility/):** What works out of the box, what is excluded, where behavior intentionally differs from CLIPS 6.30, and how differential testing is set up.
- **[Architecture & performance](./architecture/):** How the workspace crates divide parsing, Rete indexing, runtime evaluation, and FFI, plus how Criterion benchmarks and scaling checks are run.

For deeper specifications, the repository's [`docs/`](https://github.com/plx/ferric-rules/tree/main/docs) directory holds the multi-chapter [Rust user's guide](https://github.com/plx/ferric-rules/blob/main/docs/users-guide.md) (backed by [runnable workspace examples](https://github.com/plx/ferric-rules/tree/main/examples/users-guide)), the exhaustive [compatibility reference](https://github.com/plx/ferric-rules/blob/main/docs/compatibility.md), the [host handle contract](https://github.com/plx/ferric-rules/blob/main/docs/host-api.md), and the [snapshot specification](https://github.com/plx/ferric-rules/blob/main/docs/snapshots.md).

## Source and license

Source code and issue tracking live at [github.com/plx/ferric-rules](https://github.com/plx/ferric-rules). Dual-licensed under [MIT](https://github.com/plx/ferric-rules/blob/main/LICENSE-MIT) or [Apache-2.0](https://github.com/plx/ferric-rules/blob/main/LICENSE-APACHE).
