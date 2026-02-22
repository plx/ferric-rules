# Pass 004: `printout` Runtime And Router Integration

## Objective

Replace placeholder `printout` behavior with a testable runtime implementation aligned with Phase 3 scope.

## Scope

- Runtime `printout` function behavior (channel handling and argument formatting).
- Router/output abstraction to keep integration tests deterministic.
- Evaluator integration for computed `printout` arguments.

## Tasks

1. Implement `printout` runtime function and register it in the callable function environment.
2. Introduce a minimal output-router abstraction so tests can capture output without process-global hacks.
3. Enforce argument and destination validation with source-located errors.
4. Wire `printout` through the shared expression evaluator path.
5. Add unit/integration tests for output formatting, destinations, and error cases.

## Definition Of Done

- `printout` is no longer placeholder behavior on supported channels.
- Output behavior is testable and deterministic.
- Diagnostics for invalid `printout` usage are clear and source-located.

## Verification Commands

- `cargo test -p ferric-runtime execution`
- `cargo test -p ferric-runtime integration_tests`
- `cargo check --workspace`

## Handoff State

- Runtime I/O plumbing required for Phase 3 is in place.

