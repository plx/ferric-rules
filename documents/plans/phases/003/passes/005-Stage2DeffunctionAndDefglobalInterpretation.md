# Pass 005: Stage 2 `deffunction` And `defglobal` Interpretation

## Objective

Extend Stage 2 interpretation so `deffunction` and `defglobal` load as typed constructs with source-located diagnostics.

## Scope

- Parser Stage 2 construct variants and typed data models.
- Validation of arity/parameter/global assignment shape.
- Loader ingestion plumbing for newly interpreted constructs.

## Tasks

1. Extend `Construct` and Stage 2 AST types for `deffunction` and `defglobal`.
2. Parse deffunction signatures/bodies (including wildcard parameter form) into typed representations.
3. Parse defglobal assignments into typed global-definition representations with spans.
4. Add interpretation diagnostics for malformed signatures, invalid global names, and duplicate definitions.
5. Wire interpreted constructs into runtime loading paths as stored definitions (execution deferred to Pass 006).

## Definition Of Done

- `deffunction` and `defglobal` parse into typed constructs with spans.
- Invalid forms fail interpretation with source-located errors.
- Loader can ingest and retain these constructs without silent dropping.

## Verification Commands

- `cargo test -p ferric-parser stage2`
- `cargo test -p ferric-runtime loader`
- `cargo check --workspace`

## Handoff State

- Stage 2 and loader pipelines are ready for user-defined function/global runtime semantics.

