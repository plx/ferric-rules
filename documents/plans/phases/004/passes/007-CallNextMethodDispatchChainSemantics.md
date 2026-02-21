# Pass 007: `call-next-method` Dispatch Chain Semantics

## Objective

Add `call-next-method` support so generic methods can explicitly continue along the ordered applicable-method chain.

## Scope

- Runtime dispatch context for active generic call chains.
- `call-next-method` evaluator/runtime behavior and diagnostics.
- Recursion-limit and stack-safety integration.

## Tasks

1. Introduce dispatch-chain context that tracks applicable methods and current method position.
2. Implement `call-next-method` according to the documented Phase 4 contract.
3. Emit explicit diagnostics for invalid usage (outside generic dispatch, no next method, invalid arguments).
4. Ensure chain progression remains deterministic with specificity ordering from Pass 006.
5. Add tests for multi-method chaining, termination behavior, and recursion/stack-limit interaction.

## Definition Of Done

- `call-next-method` is executable and deterministic for supported generic-method flows.
- Out-of-context/no-next-method usage returns explicit diagnostics.
- Dispatch chain behavior is fully regression-tested.

## Verification Commands

- `cargo test -p ferric-runtime evaluator`
- `cargo test -p ferric-runtime phase4_integration_tests`
- `cargo check --workspace`

## Handoff State

- Generic dispatch compatibility closure is functionally complete.
