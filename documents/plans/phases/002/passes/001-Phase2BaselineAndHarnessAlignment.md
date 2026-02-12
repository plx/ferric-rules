# Pass 001: Phase 2 Baseline And Harness Alignment

## Objective

Establish a clean and explicit Phase 2 starting baseline so subsequent passes can focus on implementation work instead of re-litigating Phase 1 assumptions.

## Scope

- Baseline alignment from `documents/plans/phases/001/PlanAdjustments.md`.
- Test harness extension for Phase 2 features (compiler/runtime/invariants).
- No new language features in this pass.

## Tasks

1. Reconcile Phase 1 baseline assumptions in code comments/docs (Stage 1 parse shape, loader return contracts, module ownership notes).
2. Add shared integration-test helpers for the Phase 2 path: parse -> interpret -> compile -> run.
3. Extend consistency-check scaffolding so negative/NCC/exists/agenda invariants can be added incrementally without rewiring tests later.
4. Add empty/skeleton test modules for upcoming Phase 2 areas (Stage 2 interpreter, compiler, negative/exists/NCC, strategy ordering).
5. Ensure existing Phase 1 tests still pass unchanged.

## Definition Of Done

- Baseline assumptions are explicit and consistent.
- Harness scaffolding exists for all planned Phase 2 areas.
- No behavior regressions in Phase 1 test suites.

## Verification Commands

- `cargo test -p ferric-runtime integration_tests`
- `cargo test --workspace`
- `cargo check --workspace`

## Handoff State

- Phase 2 work can proceed pass-by-pass without baseline ambiguity.
