# Pass 006: User-Defined Function Environment And Execution

## Objective

Implement runtime execution for `deffunction` and `defglobal`, integrated with the shared function-call evaluator.

## Scope

- Function environment storage and lookup for built-ins + user-defined functions.
- Call-frame handling, parameter binding, and return-value flow.
- Runtime global-variable read/write semantics.

## Tasks

1. Implement runtime registries for user-defined functions and globals, including module-aware naming hooks for later passes.
2. Execute deffunction bodies via the same action/expression evaluator used by rule RHS and `test` CE paths.
3. Add call-frame management (parameter binding, wildcard parameter support, recursion limits/error handling).
4. Implement defglobal initialization/update semantics and evaluator access for global variables.
5. Add integration tests covering user-function invocation from rules, nested calls, and global interactions.

## Definition Of Done

- User-defined functions are callable through standard evaluation paths.
- Globals are readable/writable through supported syntax and runtime APIs.
- Call/runtime failures produce stable diagnostics without crashing evaluation loops.

## Verification Commands

- `cargo test -p ferric-runtime execution`
- `cargo test -p ferric-runtime integration_tests`
- `cargo test -p ferric-runtime loader`
- `cargo check --workspace`

## Handoff State

- Phase 3 function-environment plumbing is operational for user-defined calls.

