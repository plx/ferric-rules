# Pass 009: Performance Profiling And Budget Gap Analysis

## Objective

Profile current performance and produce a prioritized gap analysis against Section 14 targets and hot-path budgets.

## Scope

- Profiling of agenda, token, alpha lookup, and FFI boundary hotspots.
- Gap analysis versus performance targets.
- Optimization backlog prioritized by impact and implementation risk.

## Tasks

1. Run benchmark suite and capture baseline numbers for throughput/latency metrics.
2. Profile hot paths (`agenda insert/pop/remove`, token cascade, alpha lookup, execution loop) using appropriate tooling.
3. Compare measurements to documented targets (including Waltz/Manners targets) and identify largest deltas.
4. Produce a ranked optimization plan with expected gain, risk level, and validation strategy.
5. Lock a short list of Phase 6 optimization candidates for implementation in the next pass.

## Definition Of Done

- Performance baseline and budget gaps are explicitly quantified.
- Optimization priorities are justified by data rather than intuition.

## Verification Commands

- `cargo bench -p ferric --bench engine_bench -- --noplot`
- `cargo bench -p ferric --bench waltz_bench -- --noplot`
- `cargo bench -p ferric --bench manners_bench -- --noplot`
- `cargo test --workspace`

## Handoff State

- Optimization work can proceed with clear, evidence-backed priorities.
