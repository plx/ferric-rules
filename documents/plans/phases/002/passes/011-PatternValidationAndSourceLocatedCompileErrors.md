# Pass 011: Pattern Validation And Source-Located Compile Errors

## Objective

Enforce all documented pattern restrictions at compile time with stable error codes and source-located diagnostics.

## Scope

- Pattern restriction validation contract from Section 7.7.
- Error codes `E0001`-`E0005`, validation stages, and suggestions.
- Compiler integration so validation happens before node construction.

## Tasks

1. Implement `PatternValidator`, `PatternViolation`, and `PatternValidationError` types with stable machine-readable codes.
2. Enforce nesting-depth, unsupported-combination, and forall restriction checks per Sections 7.5-7.7.
3. Ensure every emitted validation error carries source span when parser spans are available.
4. Integrate validation into compile path before rete node creation; ensure invalid rules never partially compile.
5. Add tests for each error code and diagnostics shape; add the `forall_vacuous_truth_and_retraction_cycle` regression fixture/test contract for Phase 3 forall work.

## Definition Of Done

- Unsupported pattern constructs fail at compile time in both classic and strict modes.
- Validation errors are stable, source-located, and test-backed.
- No invalid rule reaches rete construction.

## Verification Commands

- `cargo test -p ferric-core rete`
- `cargo test -p ferric-runtime loader`
- `cargo check --workspace`

## Handoff State

- Pattern validation policy is implemented and ready for long-term compatibility tooling.
