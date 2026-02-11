# Phase 001 Plan: Foundation (Through End of Phase 1)

## Intent

This plan expands `documents/FerricImplementationPlan.md` into an execution sequence that gets Ferric from the current state (planning documents only) to the end of **Phase 1: Foundation (Weeks 1-10)**.

## Starting Point

- Detailed architecture and implementation plan exists.
- Codebase is still very early and does not yet implement the engine.
- Primary need: convert design intent into linear, session-sized implementation passes.

## Phase 1 Targets (from v10 plan)

By the end of this phase, the project should satisfy:

1. Project/workspace setup and CI are in place.
2. Value types, symbol interning, and basic fact representation exist.
3. Stage 1 parser (lexer + S-expression parser with spans and recovery) exists.
4. Minimal source loader can parse `.clp` into S-expressions and process top-level `(assert ...)` and `(defrule ...)` forms.
5. Alpha and beta propagation works for simple patterns/rules.
6. Unit tests pass.

Additionally, Phase 1 must introduce the **retraction-invariants suite skeleton** from Section 15.0 so Phase 2 is not blocked.

## Pass Breakdown

1. `passes/001-WorkspaceBootstrapAndCI.md`
2. `passes/002-RuntimeValuesSymbolsAndEncoding.md`
3. `passes/003-FactsBindingsAndEngineSkeleton.md`
4. `passes/004-Stage1LexerAndSExpressionParser.md`
5. `passes/005-MinimalSourceLoaderAssertAndDefrule.md`
6. `passes/006-TokenStoreRetractionIndicesAndInvariantHarness.md`
7. `passes/007-AlphaNetworkAndAlphaMemory.md`
8. `passes/008-BetaNetworkSimpleJoinsAndAgendaPlumbing.md`
9. `passes/009-Phase1IntegrationAndExitValidation.md`

## Cross-Pass Rules

- Each pass must start from a clean baseline and end in a clean baseline.
- No pass should leave `main`/branch in a non-building state.
- Every pass must run at least targeted tests and `cargo check` before handoff.
- Retraction-related index integrity must be validated as soon as those structures are introduced.
- Avoid pulling Phase 2+ concerns into this phase except where needed for forward-compatible scaffolding.

## Execution Notes

- The passes are intentionally linear and dependency-aware.
- If one pass uncovers missing low-level prerequisites, those prerequisites should be added to the current pass only if they are small; otherwise, insert a new pass and keep sequence order.
- Keep public APIs minimal and explicit in Phase 1; avoid premature breadth.

## Phase 1 Definition of Done

Phase 1 is complete when:

1. Basic `.clp` files are parseable into Stage 1 S-expressions.
2. Minimal loader handles top-level `(assert ...)` and `(defrule ...)`.
3. Facts can be asserted/retracted through engine APIs.
4. Simple rule matching through alpha + beta produces activations.
5. Retraction-invariant test harness exists and is wired into tests.
6. Workspace CI checks pass consistently.
