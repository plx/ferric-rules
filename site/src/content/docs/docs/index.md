---
title: Documentation Overview
description: Orientation for ferric-rules, a mostly CLIPS-compatible rules engine in Rust.
---

`ferric-rules` is a mostly CLIPS-compatible forward-chaining rules engine written in Rust.

It keeps the practical parts of CLIPS that make rule systems useful: `deffacts`, `defrule`, salience, the Rete algorithm, ordered facts, template facts, modules, focus stacks, user functions, generics, globals, and core standard-library behavior.

It drops the parts that make CLIPS hard to embed in modern applications: global runtime state, thread-unsafe singleton assumptions, and C build friction. Each `Engine` instance is independent.

## Project Status

Ferric is early but functional. Core functionality is implemented and apparently working; current work is focused on validation, polish, and performance.

Known scope decisions:

- The COOL object system is intentionally out of scope.
- Some more exotic pattern connectives and non-core I/O utilities are limited.
- Bindings are in progress; Rust, C FFI, Go, Python, and TypeScript-related work exist in the repository.

## Where To Start

- [Getting started](./getting-started/) shows the basic engine lifecycle.
- [CLIPS compatibility](./compatibility/) summarizes the supported language subset.
- [Embedding API](./embedding/) explains how the Rust runtime is organized for host applications.
- [Performance](./performance/) documents benchmark and scaling policy.
- [Internals](./internals/) maps the major crates and engine architecture.

## Repository

Source code lives at [github.com/plx/ferric-rules](https://github.com/plx/ferric-rules).

The crate version in this workspace is `0.1.0`, and the project is dual licensed under `MIT OR Apache-2.0`.
