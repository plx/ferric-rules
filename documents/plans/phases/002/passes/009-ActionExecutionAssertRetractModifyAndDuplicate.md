# Pass 009: Action Execution (`assert`, `retract`, `modify`, `duplicate`)

## Objective

Implement the Phase 2 RHS action subset and wire it into rule firing so rules can mutate working memory meaningfully.

## Scope

- RHS action evaluation for core fact operations.
- Minimal expression evaluation required by Phase 2 action paths.
- Interaction with rete assertion/retraction flows.

## Tasks

1. Implement action dispatch for `assert`, `retract`, `modify`, and `duplicate`.
2. Add minimal RHS expression evaluation support for literals, bound variables, and required fact references.
3. Implement `modify`/`duplicate` via safe retract+assert semantics while preserving rule-engine invariants.
4. Ensure action-driven retractions route through cascade cleanup and agenda delta handling.
5. Add tests for end-to-end rule firing that asserts, retracts, modifies, and duplicates facts.

## Definition Of Done

- Phase 2-required fact actions execute during rule firing.
- Action paths preserve rete/token/agenda consistency.
- Integration tests cover action-triggered assert/retract/modify cycles.

## Verification Commands

- `cargo test -p ferric-runtime integration_tests`
- `cargo test -p ferric-core rete`
- `cargo check --workspace`

## Handoff State

- Rule firing now has practical state-changing behavior required for core engine use.
