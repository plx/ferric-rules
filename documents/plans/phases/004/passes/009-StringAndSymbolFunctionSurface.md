# Pass 009: String And Symbol Function Surface

## Objective

Implement the documented string/symbol builtin set with encoding-aware behavior and compatibility diagnostics.

## Scope

- `str-cat`, `str-length`, `sub-string`, `sym-cat`.
- String/symbol conversion and encoding-mode behavior.
- Boundary/error handling for substring and mixed-argument calls.

## Tasks

1. Implement `str-cat`, `str-length`, `sub-string`, and `sym-cat` in evaluator dispatch.
2. Align behavior with Ferric string-encoding constraints from the master plan.
3. Define and enforce index/boundary rules for `sub-string` with clear diagnostics.
4. Add unit tests for ASCII and UTF-8 configurations, mixed symbol/string arguments, and edge bounds.
5. Add integration fixtures using these functions in rules and user-defined functions.

## Definition Of Done

- All documented string/symbol builtins are implemented.
- Encoding and boundary behavior is deterministic and test-backed.
- Integration tests confirm usage through runtime expression paths.

## Verification Commands

- `cargo test -p ferric-runtime evaluator`
- `cargo test -p ferric-runtime phase4_integration_tests`
- `cargo check --workspace`

## Handoff State

- String/symbol builtin surface is complete for Phase 4 scope.
