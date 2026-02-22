# Pass 003: Template-Aware `modify` And `duplicate` Semantics

## Objective

Complete Phase 2 carryover for `modify` and `duplicate` so template facts receive correct slot-aware updates under rule firing.

## Scope

- Template fact slot mutation semantics for `modify`/`duplicate`.
- Ordered fact compatibility and explicit error behavior for invalid slot updates.
- Retraction/agenda/token cleanup correctness during action-driven mutation.

## Tasks

1. Implement template slot-resolution for `modify` and `duplicate` action arguments.
2. Ensure `modify` follows safe retract+assert semantics while preserving invariants and expected activation churn.
3. Ensure `duplicate` creates a new fact with merged slot overrides and leaves original fact intact.
4. Add validation/diagnostics for unknown slots, duplicate slot updates, and invalid value/type combinations.
5. Add end-to-end tests for template-aware modify/duplicate across assert/retract cycles.

## Definition Of Done

- `modify`/`duplicate` are template-aware and operational in rule execution paths.
- Action-driven fact mutation preserves cleanup and agenda consistency.
- Invalid slot-update cases fail with clear diagnostics.

## Verification Commands

- `cargo test -p ferric-runtime actions`
- `cargo test -p ferric-runtime integration_tests`
- `cargo test -p ferric-core rete`
- `cargo check --workspace`

## Handoff State

- Carryover fact-action semantics are complete for template facts.

