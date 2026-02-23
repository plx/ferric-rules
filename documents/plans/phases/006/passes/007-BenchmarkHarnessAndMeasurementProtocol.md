# Pass 007: Benchmark Harness And Measurement Protocol

## Objective

Create a reproducible benchmark harness and measurement protocol suitable for Phase 6 performance closure.

## Scope

- Benchmark project structure and command ergonomics.
- Measurement protocol (warmup, sample count, environment assumptions).
- Baseline reporting format for trend comparison.

## Tasks

1. Create benchmark harness scaffolding under `benches/` for micro and scenario benchmarks.
2. Define benchmark execution protocol (invocation commands, warmup/measurement settings, CPU/environment notes).
3. Implement baseline output capture format (structured summary artifacts suitable for CI comparison).
4. Add a benchmark smoke command path for CI and local validation.
5. Document anti-flake guidance for benchmark execution environments.

## Definition Of Done

- Benchmark harness is runnable and produces repeatable baseline outputs.
- Measurement protocol is documented and actionable for future optimization passes.

## Verification Commands

- `cargo bench -p ferric --bench engine_bench -- --noplot`
- `cargo check --workspace --all-targets`

## Handoff State

- Workload benchmarks and profiling can proceed on stable measurement foundations.
