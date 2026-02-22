# Pass 008: Waltz And Manners Benchmark Workloads

## Objective

Implement canonical workload benchmarks used by the implementation plan performance targets.

## Scope

- Waltz benchmark workload implementation.
- Manners benchmark workload implementation.
- Fixture/input wiring and output/result validation.

## Tasks

1. Add benchmark implementations for Waltz and Manners workloads under `benches/`.
2. Create or import workload fixtures/data needed for deterministic benchmark runs.
3. Validate each workload benchmark against known baseline execution characteristics.
4. Add benchmark summaries that map measured results to documented targets.
5. Ensure workloads can run in both local profiling mode and CI smoke mode.

## Definition Of Done

- Waltz and Manners benchmarks run reliably and report comparable metrics.
- Workloads are integrated into the benchmark harness and ready for profiling/optimization passes.

## Verification Commands

- `cargo bench --bench waltz -- --noplot`
- `cargo bench --bench manners -- --noplot`

## Handoff State

- Canonical workload metrics are available for optimization prioritization.
