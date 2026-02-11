# Pass 008: Beta Network, Simple Joins, And Agenda Plumbing

## Objective

Implement minimal beta-join propagation and activation tracking sufficient for Phase 1 simple-rule execution paths.

## Scope

- Beta network subset from Sections 6.5 and 6.8 (root/join/terminal path only).
- Basic agenda structures and activation lifecycle from Section 6.6.1 (Phase 1 subset).

## Tasks

1. Implement beta memory and node structures for simple joins.
2. Implement join tests over left token bindings and right fact slot values.
3. Implement token creation/ownership tracking on successful joins.
4. Implement minimal agenda:
   - activation insertion,
   - ordering key scaffolding,
   - pop/removal by token linkage for retraction cleanup.
5. Wire right-activation propagation from alpha memories into beta joins and terminal activation creation.
6. Implement retraction cleanup path for implemented beta/agenda structures using token reverse indices.
7. Add tests:
   - simple two-pattern join activation,
   - activation removal when supporting fact retracts,
   - no stale activation references in agenda indices after cleanup.

## Definition Of Done

- Simple pattern rules can propagate through alpha->beta->agenda.
- Retraction removes dependent activations in implemented scope.

## Verification Commands

- `cargo test -p ferric-core beta`
- `cargo test --workspace`
- `cargo check --workspace`

## Handoff State

- End-to-end minimal matching path exists and is test-backed.
