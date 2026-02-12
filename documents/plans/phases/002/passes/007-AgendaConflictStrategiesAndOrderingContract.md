# Pass 007: Agenda Conflict Strategies And Ordering Contract

## Objective

Complete agenda ordering behavior for all Phase 2 conflict strategies while preserving total-order and cleanup guarantees.

## Scope

- Strategy support: `Depth`, `Breadth`, `LEX`, `MEA` (Section 6.6.1).
- `AgendaKey` strategy-specific ordering fields and monotonic sequence tiebreak.
- Runtime wiring from engine config into agenda behavior.

## Tasks

1. Implement strategy-specific key generation in `Agenda::build_key` for all four strategies.
2. Add recency vector handling for LEX/MEA with fixed per-rule length invariants.
3. Ensure `add`, `pop`, and `remove_activations_for_token` maintain all agenda indices under each strategy.
4. Wire runtime strategy configuration into agenda initialization and engine construction paths.
5. Add deterministic tests for ordering behavior, sequence tiebreak correctness, and token-linked activation cleanup.

## Definition Of Done

- Agenda supports all Phase 2 conflict strategies.
- Ordering remains total and stable within a run.
- Retraction cleanup remains O(k log n) via token reverse index.

## Verification Commands

- `cargo test -p ferric-core agenda`
- `cargo test -p ferric-runtime integration_tests`
- `cargo check --workspace`

## Handoff State

- Conflict-resolution behavior is complete for Phase 2 runtime execution.
