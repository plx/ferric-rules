# Pass 008: Predicate, Math, And Type Surface Parity

## Objective

Close remaining predicate/math/type builtin gaps from the documented minimum function set and stabilize their diagnostics/contracts.

## Scope

- Predicate and numeric builtin parity against Section 10.2.
- Missing type predicates (including `multifieldp`) and aliases.
- Arity/type/error behavior hardening.

## Tasks

1. Audit current evaluator builtins against Section 10.2 predicate/math/type requirements.
2. Implement missing functions/aliases and close behavioral mismatches.
3. Align numeric/type diagnostics with existing source-located error formatting.
4. Add direct evaluator tests for normal and error paths (arity/type/division edge cases).
5. Add integration scenarios where these functions execute through RHS/test expression paths.

## Definition Of Done

- Documented predicate/math/type builtins are implemented and test-backed.
- Error behavior is explicit, stable, and source-located where spans exist.
- Function behavior matches evaluator and integration execution paths.

## Verification Commands

- `cargo test -p ferric-runtime evaluator`
- `cargo test -p ferric-runtime phase4_integration_tests`
- `cargo check --workspace`

## Handoff State

- Core numeric/predicate/type builtin surface is complete and stable.
