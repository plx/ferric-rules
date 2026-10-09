---
title: Overview
description: How the engine works and where to start.
---

`ferric-rules` is a mostly CLIPS-compatible forward-chaining rules engine written
in Rust. The host application loads rules, asserts facts, runs the engine, and
reads the results. Rules can assert or change facts that trigger other rules.
Each `Engine` instance owns its state.

## Project status

This is a working prototype. Core functionality is implemented; validation,
polish, and performance work continue. Depth, breadth, LEX, and MEA conflict
resolution are supported. Compatibility remains a defined subset; known
differences are characterized in the corpus or tracked as open issues. The COOL
object system and `logical` truth maintenance are out of scope.

Check [CLIPS compatibility](./compatibility/) before using an existing rule set.
Rust, TypeScript, Python, and Swift are the primary embedding interfaces;
Go and the C ABI retain maintenance support. The crates and binding packages
are built from source; follow the linked instructions for each language.

## Guides and reference

- [Getting started](./getting-started/): a complete Rust program with one rule
  and one fact.
- [Embedding API](./embedding/): engine lifecycle, thread ownership, results,
  and bindings.
- [CLIPS compatibility](./compatibility/): implemented features and explicit
  differences.
- [Performance](./performance/): benchmark commands and scaling checks.
- [Internals](./internals/): the main crates and execution pipeline.

The repository's [user guide](https://github.com/plx/ferric-rules/blob/main/docs/users-guide.md)
and [runnable examples](https://github.com/plx/ferric-rules/tree/main/examples/users-guide)
cover templates, salience, modules, functions, configuration, and snapshots.

## Source and license

[Source code and issue tracker](https://github.com/plx/ferric-rules).
Licensed under [MIT](https://github.com/plx/ferric-rules/blob/main/LICENSE-MIT) or
[Apache-2.0](https://github.com/plx/ferric-rules/blob/main/LICENSE-APACHE).
