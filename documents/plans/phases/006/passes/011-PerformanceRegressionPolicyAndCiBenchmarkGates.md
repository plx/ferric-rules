# Pass 011: Performance Regression Policy And CI Benchmark Gates

## Objective

Finalize benchmark-based CI policy for Phase 6 so performance regressions are caught automatically before release.

## Scope

- Benchmark smoke and blocking gate policy.
- Threshold definitions and failure criteria.
- CI workflow integration and reporting.

## Tasks

1. Define benchmark regression thresholds for key workloads/metrics (including Waltz/Manners and selected microbenchmarks).
2. Upgrade benchmark CI from early smoke posture to Phase 6 blocking posture where required.
3. Implement CI reporting artifacts for benchmark deltas and threshold evaluation.
4. Add guardrails for flaky environments (retry policy, controlled hardware assumptions, or fallback classification).
5. Document the policy and maintenance procedure for updating thresholds intentionally.

## Definition Of Done

- Benchmark policy is codified and enforced in CI.
- Regressions against defined thresholds fail the gate deterministically.

## Verification Commands

- `./scripts/bench-thresholds.sh`
- `cargo test --workspace`
- CI workflow dry-run/equivalent local checks

## Handoff State

- Performance compliance is continuously enforced, not just manually checked.
