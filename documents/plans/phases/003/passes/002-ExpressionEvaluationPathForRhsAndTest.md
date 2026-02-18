# Pass 002: Expression Evaluation Path For RHS And Test

## Objective

Introduce a shared expression-evaluation pipeline so RHS actions and `test` conditional elements execute through one deterministic function-call path.

## Scope

- Runtime expression evaluation core (literals, variables, globals, nested calls).
- Unified function dispatch entry used by actions and `test` CE checks.
- Source-located diagnostics for expression evaluation failures.

## Tasks

1. Define/normalize the runtime expression model consumed by evaluator paths from Stage 2 constructs.
2. Implement evaluator support for literals, bound variables, global references, and nested function invocations.
3. Route RHS action argument evaluation and `test` CE evaluation through the same evaluator entry point.
4. Add structured evaluation errors (arity/type/unknown-function/runtime) with source spans.
5. Add unit and integration tests for nested calls, failed calls, and deterministic evaluation order.

## Definition Of Done

- RHS and `test` expressions both execute via the same evaluator implementation.
- Evaluation failures return stable, source-located diagnostics.
- Tests cover success and failure paths, including nested call chains.

## Verification Commands

- `cargo test -p ferric-runtime execution`
- `cargo test -p ferric-runtime loader`
- `cargo check --workspace`

## Handoff State

- Runtime has a single expression-call pipeline ready for carryover actions and user-defined functions.

