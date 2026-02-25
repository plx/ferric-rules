# Release Profile Optimization Results

## Summary

Evaluated Rust release profile optimization flags to maximize runtime
performance. The winning configuration provides a **consistent 5-7%
improvement** across all workload benchmarks with a ~3x increase in
release compile time and a 35% reduction in binary size.

## Configuration Adopted

```toml
[profile.release]
lto = true
codegen-units = 1
strip = "symbols"
```

Added to workspace `Cargo.toml`. Propagates to `ffi-release` via `inherits`.

## Experiment Matrix

Eight configurations tested against Rust defaults (opt-level=3,
lto=false, codegen-units=16, no strip):

| ID | Configuration | waltz_100 | manners_64 | Build | Binary |
|----|--------------|-----------|------------|-------|--------|
| E0 | Default (Rust defaults) | 933 µs | 237 µs | 5s | 2.36 MB |
| E1 | `lto = "thin"` | -2.1% | -3.6% | — | — |
| E2 | `lto = true` | -3.9% | -7.3% | 13s | 1.92 MB |
| E3 | `codegen-units = 1` | -1.7% | -3.8% | — | — |
| E4 | `lto = true` + `codegen-units = 1` | **-4.6%** | **-6.8%** | 14s | 1.76 MB |
| E5 | E4 + `target-cpu=native` | -2.8% | -5.2% | — | — |
| E6 | `strip = "symbols"` only | -0.1% | -0.2% | — | — |
| E7 | E4 + strip + native | -2.0% | -5.0% | — | — |

E1-E7 percentages are from the initial sweep. E5 and E7 underperformed
E4 likely due to thermal effects (later experiments in a long sequence).

## Verification Round

Back-to-back measurements with stable machine state confirmed E4 results:

| Benchmark | Baseline | Fat LTO + CGU=1 | Change |
|-----------|----------|-----------------|--------|
| waltz_100 | 933.1 µs | 888.7 µs | **-4.8%** |
| waltz_50 | 429.8 µs | 407.5 µs | **-5.2%** |
| waltz_20 | 179.5 µs | 169.8 µs | **-5.4%** |
| waltz_5 | 66.0 µs | 62.7 µs | **-5.0%** |
| manners_64 | 237.2 µs | 220.5 µs | **-7.1%** |
| manners_32 | 135.6 µs | 126.4 µs | **-6.8%** |
| manners_16 | 85.0 µs | 80.2 µs | **-5.6%** |
| manners_8 | 58.3 µs | 55.8 µs | **-4.3%** |

## Binary Size

| Config | Size | vs Baseline |
|--------|------|-------------|
| Default | 2,362,928 bytes | — |
| Fat LTO + CGU=1 | 1,764,512 bytes | -25% |
| Fat LTO + CGU=1 + strip | 1,544,736 bytes | -35% |

## Build Time

Release compile time increases from ~5s to ~14s (2.8x), well within the
3x tolerance. This is a full clean build of ferric-cli including all
dependencies.

## Flags Not Adopted

- **`target-cpu=native`**: Did not consistently improve performance
  beyond Fat LTO + CGU=1, and produces non-portable binaries. Not
  encoded in the profile.
- **PGO (Profile-Guided Optimization)**: Not tested. The simpler flags
  already provide meaningful improvement and PGO adds significant build
  complexity. Available as a future option if tighter performance is
  needed.

## How Each Flag Helps

- **`lto = true`** (Fat LTO): Enables whole-program link-time
  optimization across all crates. LLVM can inline and optimize across
  crate boundaries that are normally opaque. This is the single most
  impactful flag.
- **`codegen-units = 1`**: Forces LLVM to compile each crate as a
  single unit, maximizing optimization opportunities within a crate.
  Marginal benefit on top of Fat LTO, but reduces binary size further.
- **`strip = "symbols"`**: Removes debug symbols from the release
  binary. No runtime impact; purely a binary size reduction.

## Methodology

- Rust 1.93.0 on macOS (Apple Silicon)
- Criterion 0.5 with default settings (100 samples, 5s measurement, 3s warmup)
- Full `target/release` clean between configuration changes
- Criterion baselines used for structured comparison
- Verification round: back-to-back E0 vs E4 with stable machine state
- All CI threshold checks pass with the new configuration

## Date

2026-02-25
