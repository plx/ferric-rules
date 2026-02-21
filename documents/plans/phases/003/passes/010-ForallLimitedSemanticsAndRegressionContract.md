# Pass 010: Limited `forall` Semantics And Regression Contract

## Objective

Implement the limited Phase 3 `forall` subset on top of existing NCC/exists infrastructure, including the required vacuous-truth retraction-cycle behavior.

## Scope

- Compiler/runtime handling of `(forall <condition> <then>)` with documented restrictions.
- Validation enforcement for unsupported `forall` shapes and nesting.
- End-to-end execution tests for vacuous truth and assert/retract transitions.

## Tasks

1. Extend translation/compile paths so supported `forall` forms become executable rete semantics.
2. Enforce Section 7.5 restrictions (single fact pattern condition and then-clause, scoped variables, no nested forall) with stable validation behavior.
3. Implement runtime blocking/unblocking behavior so empty-condition sets satisfy `forall` (vacuous truth).
4. Enable and complete the `forall_vacuous_truth_and_retraction_cycle` regression fixture/test contract.
5. Add additional integration tests for supported and unsupported forall forms, including retraction churn.

## Definition Of Done

- Supported limited `forall` rules execute correctly.
- Vacuous-truth and re-satisfaction/retraction cycle behavior is test-backed.
- Unsupported forall patterns fail with explicit, source-located diagnostics.

## Verification Commands

- `cargo test -p ferric-core validation`
- `cargo test -p ferric-runtime integration_tests`
- `cargo test --workspace forall`
- `cargo check --workspace`

## Handoff State

- Phase 3 universal-quantification subset is complete and compatible with existing negation/existential semantics.

