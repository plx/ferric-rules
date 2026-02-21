# Pass 001: Phase 3 Baseline And Harness Alignment

## Objective

Establish an explicit Phase 3 baseline and test harness so language-completion passes can focus on feature work instead of setup churn.

## Scope

- Align assumptions from completed Phase 2 remediations.
- Add fixture/test scaffolding for Phase 3 constructs and expression evaluation.
- No new end-user language semantics in this pass.

## Tasks

1. Reconcile crate-level docs/comments for Phase 3 starting contracts (`modify`/`duplicate` narrowing, `printout` placeholder, deferred constructs).
2. Extend integration test helpers for expression-evaluator, function-environment, module, generic/method, and forall scenarios.
3. Add or refresh fixture skeletons for `deffunction`, `defglobal`, `defmodule`, `defgeneric`, `defmethod`, and `forall` execution paths.
4. Ensure invariants harness extension points exist for any new indices/memories expected in later Phase 3 passes.
5. Confirm all existing Phase 2 suites remain green without semantic regressions.

## Definition Of Done

- Phase 3 starting assumptions are explicit and consistent across docs/tests.
- Harness scaffolding exists for each planned Phase 3 feature area.
- No behavior regressions from Phase 2 baseline.

## Verification Commands

- `cargo test -p ferric-runtime phase2_integration_tests`
- `cargo test --workspace`
- `cargo check --workspace`

## Handoff State

- Baseline ambiguity is removed and Phase 3 feature passes can proceed linearly.
