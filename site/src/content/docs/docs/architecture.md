---
title: Architecture & performance
description: Workspace crate layout, Rete indexing and safety boundaries, Criterion benchmarks, and scaling checks.
---

`ferric-rules` splits parsing, Rete network matching, runtime evaluation, and FFI into separate workspace crates so that core data structures stay isolated from C ABI concerns and host bindings.

## Workspace layout

| Crate                                       | Role                                                                                                                                                                                                                  |
| ------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `ferric-rules-parser`                       | Three-stage front end: lexer, S-expression reader with source spans, and typed construct ASTs (`defrule`, `deftemplate`, `deffacts`, `deffunction`, `defgeneric`, `defmethod`, `defglobal`, `defmodule`).             |
| `ferric-rules-core`                         | Rete network internals: interned symbols and values, working-memory arenas, alpha and beta networks, negative/NCC/`exists` support tracking, rule compiler, and agenda strategies (`depth`, `breadth`, `lex`, `mea`). |
| `ferric-rules-runtime`                      | `Engine` lifecycle, module visibility and focus stacks, expression evaluator and CLIPS builtin library, I/O routers, engine-checked host handles, and optional CBOR/JSON snapshot serialization.                      |
| `ferric-rules`                              | Public Rust facade re-exporting `parser`, `core`, and `runtime`, plus the compatibility corpus runner, Criterion benchmark suites, and release scaling tests.                                                         |
| `ferric-rules-cli`                          | The `ferric` command-line tool (`run`, `check`, `repl`, `snapshot`, `version`).                                                                                                                                       |
| `ferric-rules-ffi`                          | C ABI (`libferric_rules_ffi` and `cbindgen`-verified `ferric.h`), backed by `ferric-rules-ffi-macros` for panic containment across `extern "C"` functions.                                                            |
| `ferric-rules-napi` / `ferric-rules-python` | Native binding crates for Node.js/TypeScript (`napi-rs`) and Python (`PyO3`).                                                                                                                                         |

All pure-Rust engine crates enforce `#![deny(unsafe_code)]` via workspace lints. `unsafe` is permitted only at the C ABI and native binding boundaries, where `ffi-dev` and `ffi-release` Cargo profiles retain unwinding so exported wrappers catch Rust panics before they can cross into C, Swift, or Go frames.

## From CLIPS source to Rete execution

1. **Parse and validate:** Source text is lexed and parsed into typed constructs. Variable bindings, slot names, static cardinality constraints, quantifier nesting bounds, and negated field-constraint subsets are checked before a rule touches the live network, so rejected constructs leave existing engine state intact.
2. **Compile into alpha and beta nodes:** Single-fact tests (relation/template identity, literal slot checks, intra-fact constraints) become **alpha nodes** feeding alpha memories. Cross-pattern bindings compile into **beta join nodes**, sharing positive join prefixes across rules where possible. Negations (`not`), existential patterns (`exists`), universal quantifiers (`forall`, lowered through negated conjunctions), and arbitrary `(not (and ...))` subnetworks compile into dedicated nodes that track blocker and support counts.
3. **Propagate facts incrementally:** Asserting or retracting a fact updates only the affected alpha memories and downstream beta/negative tokens rather than rescanning rules. Complete rule matches enter the **agenda**, ordered by module focus, salience, and the active conflict-resolution strategy, with refraction preventing the same token from re-firing.
4. **Execute right-hand sides:** `Engine::run` pops the top activation from the focused module's agenda and evaluates its RHS actions. Assertions, retractions, and `modify`/`duplicate` calls immediately propagate through the Rete network, potentially scheduling or cancelling further activations before the next cycle.

## Data representation and Rete indexing

Rete engines spend most of their time probing join memories, maintaining blocker lists during fact churn, and cloning small token prefixes. Several implementation choices keep those paths predictable:

- **Arena storage and inline small collections:** Working-memory facts, tokens, and network nodes live in generational `slotmap` arenas (`rustc-hash` `FxHashMap` indices for fast internal lookups) with `smallvec` inline storage for short field lists and token memberships, cutting heap traffic on small facts.
- **Hash-indexed joins beyond simple positive equality:** Beta joins index equality keys so incoming tokens probe matching partners directly instead of scanning full memories. The same equality-index machinery extends to `exists` support propagation, simple negative blocker lookups, and retraction cleanup limited to the memories a fact actually inhabits.
- **Shared immutable definitions:** Template metadata and compiled construct definitions sit behind `Arc`, keeping `Engine` `Send + Sync` while avoiding deep copies during RHS assertions and host fact reads.

## Benchmarks and asymptotic scaling checks

This site intentionally avoids publishing headline "X× faster than CLIPS" numbers—rule-engine timings depend heavily on join selectivity, retraction churn, and host boundary traffic. Instead, the repository tracks performance with two complementary suites:

### Criterion benchmark suites

Release-profile benchmarks (`lto = true`, `codegen-units = 1`) live under [`crates/ferric-rules/benches/`](https://github.com/plx/ferric-rules/tree/main/crates/ferric-rules/benches) and smaller crate-level benches, covering classic production-system benchmarks (**Waltz** line labeling and **Manners** seating) alongside targeted micro- and component-level benchmarks for beta joins, negation, `exists`, `forall`, retraction cascades, fact churn, alpha fanout, conflict strategies, modules, fact queries, rule compilation, and CBOR serialization:

```sh
just bench-waltz
just bench-join
cargo bench -p ferric-rules
```

Workload descriptions and measurement protocols are documented in [`benches/README.md`](https://github.com/plx/ferric-rules/blob/main/benches/README.md) and [`docs/benchmark-policy.md`](https://github.com/plx/ferric-rules/blob/main/docs/benchmark-policy.md).

### Complexity-class scaling tests

Absolute timings vary across machines and CI runners, which makes them noisy guards against algorithmic regressions. To catch accidental complexity blowups (such as an `O(N)` retraction or negative-join path silently degrading to `O(N²)`), [`crates/ferric-rules/tests/scaling_tests.rs`](https://github.com/plx/ferric-rules/blob/main/crates/ferric-rules/tests/scaling_tests.rs) runs fourteen release-mode scaling checks:

```sh
just scaling-check
```

Each test runs a workload at two input sizes 4× apart and verifies that the elapsed-time ratio stays within bounds for its expected complexity class—covering join propagation, engine runs, retraction cascades, churn lifecycles, alpha fanout, `exists` support assertion, indexed NCC completion, independent negative cleanup, dormant focus selection, template multislot joins, and sequence-pattern matching.
