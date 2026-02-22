# Pass 003: CLIPS Compatibility Core Execution Semantics Suite

## Objective

Implement compatibility coverage for core engine semantics and rete/retraction behavior expected by the supported CLIPS subset.

## Scope

- Rule activation/firing basics, salience ordering, and agenda behavior.
- Assert/retract/modify/duplicate cycles and retraction cleanup invariants.
- Negation, `exists`, NCC, and `forall` vacuous-truth cycle compatibility.

## Tasks

1. Add compatibility fixtures and assertions for core rule execution and salience/conflict behaviors.
2. Add compatibility fixtures for retraction-sensitive flows (including chained retractions and re-satisfaction).
3. Extend coverage for negation family semantics (`not`, NCC, `exists`) and `forall` vacuous-truth/retraction cycle behavior.
4. Ensure compatibility assertions include both final working-memory outcomes and relevant diagnostics where behavior is intentionally bounded.
5. Add regression tags/organization so future bug fixes append rather than replace coverage.

## Definition Of Done

- Core execution semantics are covered by compatibility fixtures.
- Retraction-sensitive and negation-family behaviors are compatibility-tested.
- Suites run deterministically and pass.

## Verification Commands

- `cargo test --test clips_compat_core`
- `cargo test --workspace`

## Handoff State

- Core compatibility baseline is established for higher-level language/stdlib compatibility work.
