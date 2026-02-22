# Pass 012: Agenda And Focus Query Function Surface

## Objective

Complete agenda/focus callable query/control parity (`run`, `halt`, `focus`, `get-focus`, `get-focus-stack`, `list-focus-stack`, `agenda`) without violating agenda invariants.

## Scope

- Function-call exposure for agenda/focus operations.
- Query output shape and diagnostics.
- Consistency with existing engine focus/agenda semantics.

## Tasks

1. Implement callable surfaces for agenda/focus operations required by Section 10.2.
2. Ensure control operations route through existing engine APIs and preserve agenda/focus contracts.
3. Define stable return/value formatting for query functions (`get-focus-stack`, `list-focus-stack`, `agenda`).
4. Add diagnostics for invalid module names, invalid call contexts, and unsupported invocation patterns.
5. Add integration tests covering interaction of agenda/focus functions with rule firing and module visibility.

## Definition Of Done

- Documented agenda/focus callable functions are implemented.
- Query/control behavior is deterministic and invariant-safe.
- Integration tests validate behavior under multi-module execution scenarios.

## Verification Commands

- `cargo test -p ferric-runtime engine`
- `cargo test -p ferric-runtime phase4_integration_tests`
- `cargo check --workspace`

## Handoff State

- Agenda/focus function surface is complete for Phase 4 exit.
