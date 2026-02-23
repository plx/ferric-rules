# Benchmark Measurement Protocol

## Execution Commands

### Full benchmark run
```sh
cargo bench -p ferric
```

### Smoke test (CI/quick validation)
```sh
cargo bench -p ferric -- --test
```

### No-plot run (CI-friendly, suppresses HTML report generation)
```sh
cargo bench -p ferric --bench engine_bench -- --noplot
cargo bench -p ferric --bench waltz_bench -- --noplot
cargo bench -p ferric --bench manners_bench -- --noplot
```

## Criterion Configuration

Criterion defaults are used unless overridden per-benchmark:

- **Warmup:** 3 seconds
- **Measurement time:** 5 seconds
- **Sample size:** 100 iterations
- **Noise threshold:** 0.01 (1%)

These provide good signal for microbenchmarks while keeping total run time under
2 minutes for the full suite.

## Environment Guidance

### Local development

1. Close unnecessary applications to reduce background CPU contention.
2. Plug in power (laptops may throttle on battery).
3. Avoid running on initial build — let the system settle after compilation.
4. Run the benchmark suite 2-3 times; trust the median, not any single run.

### CI environments

1. Pin to a specific runner type if available (avoid shared/burstable runners).
2. Use `--noplot` to skip HTML report generation (faster, no gnuplot dependency).
3. Use `--test` mode for smoke checks (verifies benchmarks compile and execute,
   does not measure performance).
4. For regression detection, compare baseline JSON files rather than raw timing
   (Criterion supports `--save-baseline` and `--baseline` flags).

## Anti-Flake Recommendations

- **Do not** interpret sub-1% changes as meaningful — they are likely noise.
- **Do** run benchmarks at least twice before reporting regressions.
- **Do** check `target/criterion/<name>/new/estimates.json` for confidence
  intervals rather than relying solely on mean values.
- **Do not** mix benchmark runs across different hardware or OS versions.
- **Do** document the hardware/environment when publishing baseline numbers.

## Baseline Capture

To save a named baseline for comparison:

```sh
cargo bench -p ferric --bench engine_bench -- --save-baseline phase6-start
```

To compare against a saved baseline:

```sh
cargo bench -p ferric --bench engine_bench -- --baseline phase6-start
```

## Output Artifacts

| Artifact | Location |
|----------|----------|
| HTML reports | `target/criterion/report/index.html` |
| Per-benchmark data | `target/criterion/<name>/` |
| Baseline JSON | `target/criterion/<name>/new/estimates.json` |

## CI Integration

The CI pipeline includes a `bench-smoke` job that runs `--test` mode on every PR.
For full regression detection, see `docs/benchmark-policy.md`.
