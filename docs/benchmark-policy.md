# Benchmark Regression Policy

## Overview

Benchmark validation is now enforced with two blocking CI layers:

1. `bench-smoke` verifies benchmark targets compile and execute (`--test` mode).
2. `bench-thresholds` runs Criterion measurements and enforces absolute
   pass/fail thresholds for key workloads and selected microbenchmarks.

The threshold gate is implemented by `scripts/bench-thresholds.sh`.

## Threshold Definitions

Thresholds are evaluated against Criterion `median.point_estimate` (ns) from
`target/criterion/**/new/estimates.json`.

| Benchmark | Threshold (ns) | Rationale |
|---|---:|---|
| `waltz_100_junctions` | `10_000_000_000` | Section 14 target: Waltz <= 10s |
| `manners_64_guests` | `5_000_000_000` | Section 14 target: Manners 64 <= 5s |
| `engine_create` | `5_000_000` | Micro guardrail for lifecycle path |
| `load_and_run_simple` | `50_000_000` | Micro guardrail for end-to-end simple pipeline |
| `reset_run_retract_3` | `50_000_000` | Micro guardrail for retraction-sensitive path |
| `compile_template_rule` | `50_000_000` | Micro guardrail for parse/load hot path |

These thresholds are intentionally conservative to minimize CI flake while still
failing catastrophic regressions deterministically.

## CI Integration

| Job | Trigger | Mode | Blocking? |
|-----|---------|------|-----------|
| `bench-smoke` | Every push / PR | `cargo bench -p ferric -- --test` | Yes |
| `bench-thresholds` | Every push / PR | `./scripts/bench-thresholds.sh` | Yes |

`bench-thresholds` publishes:
- `target/bench-threshold-report.json`
- `target/bench-threshold-report.md`
- selected Criterion `new/estimates.json` artifacts

## Local Validation

Run the same threshold gate locally:

```sh
./scripts/bench-thresholds.sh
```

For deeper analysis of intentional performance work, use baseline comparisons:

```sh
cargo bench -p ferric --bench engine_bench -- --save-baseline before
cargo bench -p ferric --bench engine_bench -- --baseline before
```

## Anti-Flake Guidance

- Treat sub-1% differences as noise unless repeated across runs.
- Run full local measurements at least twice before concluding a regression.
- Compare results only on like-for-like hardware/OS.
- Use the generated threshold report artifacts to review outliers before
  adjusting thresholds.

See `benches/PROTOCOL.md` for the execution protocol and environment guidance.
