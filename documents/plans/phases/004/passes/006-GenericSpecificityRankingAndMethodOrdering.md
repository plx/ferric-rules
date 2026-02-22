# Pass 006: Generic Specificity Ranking And Method Ordering

## Objective

Implement CLIPS-style method specificity ranking so generic dispatch ordering no longer depends on registration order.

## Scope

- Method ordering policy for explicit/auto indices and specificity.
- Deterministic ranking/tie-break behavior.
- Dispatch candidate ordering only; `call-next-method` chaining is next pass.

## Tasks

1. Implement a specificity comparator consistent with the documented CLIPS compatibility target.
2. Update method registration/auto-index assignment to use specificity-aware ordering.
3. Preserve deterministic tie-break behavior for equal specificity cases.
4. Add diagnostics/validation for invalid or ambiguous ordering states that violate the contract.
5. Add evaluator and integration tests covering type-restriction lattices, wildcard methods, and equal-specificity ties.

## Definition Of Done

- Generic method ordering follows specificity rules instead of registration order.
- Ordering remains deterministic under equivalent-specificity scenarios.
- Dispatch picks the expected most-specific method in tested cases.

## Verification Commands

- `cargo test -p ferric-runtime evaluator`
- `cargo test -p ferric-runtime functions`
- `cargo check --workspace`

## Handoff State

- Generic dispatch ordering is compatibility-oriented and ready for chaining semantics.
