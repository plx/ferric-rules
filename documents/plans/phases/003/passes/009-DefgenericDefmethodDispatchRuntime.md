# Pass 009: `defgeneric`/`defmethod` Dispatch Runtime

## Objective

Implement runtime generic-function dispatch so `defgeneric` and `defmethod` become executable through the standard function-call path.

## Scope

- Generic function/method registries and dispatch selection.
- Method applicability, precedence ordering, and ambiguity handling.
- Integration with module visibility and expression evaluation.

## Tasks

1. Implement runtime data structures for generic definitions and method sets.
2. Define deterministic method applicability/selection rules for supported Phase 3 method constraints.
3. Integrate generic dispatch into the shared function evaluator and runtime call API.
4. Emit diagnostics for missing generics, no-applicable-method, and ambiguous dispatch outcomes.
5. Add tests for method selection, module interaction, and failure-path diagnostics.

## Definition Of Done

- Generic calls dispatch to the expected method deterministically.
- Error cases are explicit and source-located where source spans exist.
- Dispatch integrates cleanly with existing function and module environments.

## Verification Commands

- `cargo test -p ferric-runtime execution`
- `cargo test -p ferric-runtime integration_tests`
- `cargo check --workspace`

## Handoff State

- Remaining callable construct set (`defgeneric`/`defmethod`) is runtime-operational.

