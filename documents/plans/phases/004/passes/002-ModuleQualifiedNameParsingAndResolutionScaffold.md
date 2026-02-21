# Pass 002: Module-Qualified Name Parsing And Resolution Scaffold

## Objective

Introduce parser/runtime scaffolding for module-qualified references (`MODULE::name`) so later passes can implement behavior and diagnostics without ad-hoc parsing.

## Scope

- Stage 2 interpretation and runtime expression/model updates for qualified identifiers.
- Shared resolution utilities for qualified vs unqualified references.
- Structural groundwork only; full visibility enforcement follows in later passes.

## Tasks

1. Extend parsing/interpretation paths to preserve module-qualified callable/global references with source spans.
2. Add runtime-level name-resolution helpers that split and validate `MODULE::name` forms deterministically.
3. Define diagnostics for malformed/invalid qualified-name syntax (no silent fallback to unqualified).
4. Thread qualified-name representation through evaluator/loader translation paths needed by later passes.
5. Add parser and unit tests for qualified, unqualified, and malformed forms.

## Definition Of Done

- Qualified-name syntax is preserved from parse through runtime translation.
- A single shared resolver path exists for qualified identifier handling.
- Invalid qualified-name syntax yields explicit, source-located diagnostics.

## Verification Commands

- `cargo test -p ferric-parser stage2`
- `cargo test -p ferric-runtime loader`
- `cargo check --workspace`

## Handoff State

- Later module-resolution passes can focus on visibility and behavior, not parsing scaffolding.
