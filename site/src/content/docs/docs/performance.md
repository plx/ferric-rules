---
title: Performance
description: Run the benchmarks and scaling checks.
---

The repository uses Criterion benchmarks to measure execution time and separate
scaling checks to catch changes in asymptotic behavior. This page describes how
to run them; it does not provide a performance comparison with CLIPS.

## Benchmarking policy

From the repository root:

```sh
just bench-join
just bench-waltz
cargo bench -p ferric-rules
```

These commands use the release profile with LTO. For a before-and-after
comparison, use the same machine and profile, record the actual Criterion
median values, and include the environment with the results. Performance claims
must come from `cargo bench` output. Ordinary debug-mode `cargo test` timings
are not comparable; release scaling tests below serve a different purpose.

See the [benchmark guide](https://github.com/plx/ferric-rules/blob/main/benches/README.md)
for the workloads and
[benchmark policy](https://github.com/plx/ferric-rules/blob/main/docs/benchmark-policy.md)
for the project's regression thresholds.

## Scaling checks

```sh
just scaling-check
```

Fourteen [release-mode scaling tests](https://github.com/plx/ferric-rules/blob/main/crates/ferric-rules/tests/scaling_tests.rs)
exercise join propagation, engine execution, retraction, churn, alpha fanout,
exists and NCC support, negative cleanup, focus selection, template multislot
joins, and sequence matching.
Each measures two input sizes, four times apart, and checks the ratio against
bounds for the expected complexity. The bound-sequence join case holds the
number of join keys fixed while increasing sequence length.

These checks can catch regressions such as linear work becoming quadratic.
They do not establish absolute execution times or predict performance for a
particular rule set.
