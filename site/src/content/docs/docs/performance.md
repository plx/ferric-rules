---
title: Performance
description: Benchmark and scaling policy for ferric-rules.
---

Ferric performance claims should come from release-mode Criterion benchmarks, not debug-mode test runs.

## Benchmarking Policy

Performance numbers in commit messages, documentation, and PR descriptions should come from `cargo bench` output. Debug-mode timings from `cargo test`, `cargo test --bench`, or ad hoc local runs are not representative.

Recommended commands:

```sh
just bench-join
just bench-waltz
cargo bench -p ferric
```

When claiming an improvement:

- run benchmarks before and after the change,
- use the same machine and profile,
- quote Criterion median values,
- note the machine or environment when relevant.

## Scaling Checks

The repository includes ignored integration tests that assert asymptotic behavior for core operations:

- join propagation,
- engine run,
- retraction cascade,
- churn lifecycle,
- alpha fanout.

Run them with:

```sh
just scaling-check
```

These checks catch complexity-class regressions without depending on fragile absolute timing thresholds.

## Design Implication

The documentation site should avoid publishing benchmark numbers until they are sourced from the release-profile benchmark suite.
