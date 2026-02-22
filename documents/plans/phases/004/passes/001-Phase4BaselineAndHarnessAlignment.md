# Pass 001: Phase 4 Baseline And Harness Alignment

## Objective

Establish an explicit Phase 4 baseline and test harness so compatibility and stdlib-expansion passes can proceed without setup churn.

## Scope

- Reconcile documented Phase 3 carryovers now owned by Phase 4.
- Prepare fixture/test scaffolding for module-qualified resolution, generic dispatch parity, and stdlib breadth.
- No new end-user semantics in this pass.

## Tasks

1. Document current Phase 4 starting contracts in crate-level docs/comments (what is complete, what is deferred).
2. Add/extend integration-test helpers for callable/global visibility scenarios, module-qualified names, and dispatch-order assertions.
3. Create fixture skeletons for module-qualified lookup, `call-next-method`, and each stdlib function family.
4. Reserve consistency-check extension points for any new registries/state stores introduced in later passes.
5. Confirm all current Phase 3 suites remain green before semantic changes begin.

## Definition Of Done

- Phase 4 baseline assumptions are explicit and consistent across docs/tests.
- Harness scaffolding exists for all planned Phase 4 feature areas.
- No regressions relative to the Phase 3 baseline.

## Verification Commands

- `cargo test -p ferric-runtime`
- `cargo test -p ferric-parser stage2`
- `cargo check --workspace`

## Handoff State

- Phase 4 implementation passes can proceed linearly with minimal setup risk.
