# Pass 006: Token Store, Retraction Indices, And Invariant Harness

## Objective

Add token storage and reverse indices required for efficient retraction, plus the required Phase 1 retraction-invariants test harness skeleton.

## Scope

- Token model and storage from Section 5.5/5.5.1.
- Retraction-root selection and cascade removal support.
- Initial invariant tests from Section 15.0.

## Tasks

1. Implement `TokenId`, `Token`, and `TokenStore`.
2. Implement and validate:
   - `fact_to_tokens` index,
   - `parent_to_children` index,
   - de-dup behavior for repeated facts in token fact lists.
3. Implement `remove`, `remove_cascade`, and `retraction_roots`.
4. Add debug assertions for no-duplicate reverse-index insertion and `remove_cascade` precondition.
5. Introduce retraction-invariants harness skeleton with tests for implemented structures:
   - no stale token references in indices after retraction,
   - subtree-only cascade behavior,
   - dedup correctness on repeated `FactId`.
6. Add `debug_assert_consistency()` skeleton for implemented structures and call it from invariant tests.

## Definition Of Done

- Token storage and cascade mechanics are operational and tested.
- Invariant harness exists and is wired so future passes extend rather than reinvent it.

## Verification Commands

- `cargo test -p ferric-core`
- `cargo test --workspace`
- `cargo check --workspace`

## Handoff State

- Retraction-first core indices are in place and validated for current scope.
