# Pass 007: Alpha Network And Alpha Memory

## Objective

Implement alpha-side discrimination and memory/index maintenance so asserted facts can flow into rete matching paths.

## Scope

- Alpha node identity/sharing concepts from Section 6.2/6.3.
- Alpha memory behavior from Section 6.4.

## Tasks

1. Implement alpha network structures:
   - `NodeId`,
   - entry/constant-test node variants,
   - `ConstantTest` and `ConstantTestType`.
2. Implement `AlphaMemory` with:
   - base fact membership,
   - optional slot indices by `AtomKey`,
   - eager prune-on-empty behavior for index maps.
3. Implement index request/backfill logic for already-asserted facts.
4. Implement assertion-time alpha propagation for supported simple patterns.
5. Implement retraction-time alpha memory removal updates.
6. Add tests for:
   - entry discrimination,
   - constant-test pass/fail behavior,
   - index lookup/backfill correctness.

## Definition Of Done

- Facts correctly enter and leave alpha memories.
- Alpha indices stay internally consistent as facts churn.

## Verification Commands

- `cargo test -p ferric-core alpha`
- `cargo test --workspace`
- `cargo check --workspace`

## Handoff State

- Alpha side is ready for beta join propagation in Pass 008.
