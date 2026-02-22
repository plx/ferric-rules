# Benchmark Regression Policy

## Overview

Benchmark smoke tests block PRs; full measurement is advisory.

The `bench-smoke` CI job verifies that benchmarks compile and execute on every
push and pull request. It runs in `--test` mode so it adds minimal time to the
pipeline. Full criterion measurement runs are performed on-demand by developers
and are not gating.

## Threshold Definitions

Criterion's built-in regression detection is used for microbenchmarks. Criterion
compares new measurements against the most recent saved baseline and reports
confidence intervals. A change is flagged as a regression when the estimated
difference exceeds the noise threshold (configured at 1% — see
`benches/PROTOCOL.md` for defaults).

Sub-1% changes are noise and should be ignored. See the anti-flake guidance
below before reporting any regression.

## CI Integration

| Job | Trigger | Mode | Blocking? |
|-----|---------|------|-----------|
| `bench-smoke` | Every push / PR | `--test` (compile + execute, no measurement) | Yes |
| Full measurement | On-demand (local or manual workflow dispatch) | Default criterion run | No |

The smoke job ensures benchmarks remain buildable and runnable. It does not
produce performance numbers, so it cannot detect regressions on its own.

For regression detection, developers should run the full suite locally and
compare against a saved baseline (see Updating Baselines below).

## Updating Baselines

When you make an intentional performance change (optimization or known
trade-off):

1. Run the full benchmark suite before your change:
   ```sh
   cargo bench -p ferric --bench engine_bench -- --save-baseline before
   ```
2. Apply your change and run again:
   ```sh
   cargo bench -p ferric --bench engine_bench -- --baseline before
   ```
3. Review the criterion comparison output. If the change is expected, save a
   new baseline:
   ```sh
   cargo bench -p ferric --bench engine_bench -- --save-baseline main
   ```

## Anti-Flake Guidance

- Sub-1% changes are noise — do not report them as regressions.
- Run benchmarks at least twice before reporting a regression.
- Check confidence intervals in `target/criterion/<name>/new/estimates.json`
  rather than relying on a single mean value.
- Do not compare results across different hardware or OS versions.
- Close unnecessary applications and plug in power when benchmarking locally.

See `benches/PROTOCOL.md` for the full measurement protocol and environment
guidance.
