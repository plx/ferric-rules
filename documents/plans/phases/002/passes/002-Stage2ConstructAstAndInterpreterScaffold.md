# Pass 002: Stage 2 Construct AST And Interpreter Scaffold

## Objective

Introduce the typed Stage 2 construct model and interpreter framework that turns Stage 1 S-expressions into structured constructs.

## Scope

- Stage 2 types and interpreter entry points from Section 8.3.
- Error and diagnostic foundations for interpretation failures.
- No deep construct semantics yet (implemented in Pass 003).

## Tasks

1. Add core Stage 2 construct/AST types for Phase 2 scope (`Rule`, `Template`, `Facts`).
2. Implement `interpret_constructs(...)` and top-level dispatch over Stage 1 S-expressions.
3. Add `InterpretError`/error-kind infrastructure with source span and suggestion fields.
4. Provide parser-runtime adapter helpers so loader/runtime can call Stage 2 cleanly.
5. Add unit tests for dispatch, unknown construct handling, and strict/classic stop/continue behavior.

## Definition Of Done

- Stage 2 interpreter skeleton compiles and is test-backed.
- Errors include source locations and deterministic categories.
- Runtime can invoke Stage 2 entry points through a stable adapter.

## Verification Commands

- `cargo test -p ferric-parser`
- `cargo test -p ferric-runtime loader`
- `cargo check --workspace`

## Handoff State

- Typed construct pipeline exists and is ready for real construct interpretation.
