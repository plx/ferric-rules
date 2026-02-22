# Pass 001: Phase 5 Baseline And Harness Alignment

## Objective

Establish an explicit Phase 5 baseline and validation harness so FFI/CLI delivery can proceed without setup churn.

## Scope

- Reconcile Phase 4 carryover contracts now exposed externally.
- Prepare test scaffolding for C ABI contracts and CLI diagnostics.
- No new external feature behavior in this pass.

## Tasks

1. Document Phase 5 baseline assumptions in crate docs/comments (Phase 4 diagnostic parity, thread-affinity constraints, ownership expectations).
2. Add shared test helpers for FFI contract testing (error-code assertions, pointer/null handling, message retrieval checks).
3. Add shared test helpers for CLI command execution (exit code capture, stderr/stdout capture, fixture invocation).
4. Create fixture skeletons for FFI thread-violation, copy-to-buffer truncation, and CLI run/check/repl coverage.
5. Confirm current Phase 4 suites remain green before introducing FFI/CLI semantics.

## Definition Of Done

- Phase 5 baseline assumptions are explicit and consistent.
- Harness scaffolding exists for all planned FFI/CLI areas.
- No regressions relative to the Phase 4 baseline.

## Verification Commands

- `cargo test -p ferric-runtime phase4_integration_tests`
- `cargo test --workspace`
- `cargo check --workspace`

## Handoff State

- FFI/CLI implementation passes can proceed linearly with minimal setup risk.
