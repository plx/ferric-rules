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
comparison, use the same machine and profile, record the Criterion median
values, and include the environment with the results. Timings from `cargo test`
or `cargo test --bench` use an unoptimized profile and are not comparable.

See the [benchmark guide](https://github.com/plx/ferric-rules/blob/main/benches/README.md)
for the workloads and
[benchmark policy](https://github.com/plx/ferric-rules/blob/main/docs/benchmark-policy.md)
for the project's regression thresholds.

## Scaling checks

```sh
just scaling-check
```

Seven integration tests measure join propagation, engine execution, retraction
cascades, churn, alpha fanout, exists support assertion, and independent negative
cleanup at two input sizes, four times apart. They check the ratio of elapsed
times against bounds for the expected complexity.

These checks can catch regressions such as linear work becoming quadratic.
They do not establish absolute execution times or predict performance for a
particular rule set.
