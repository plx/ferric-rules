# Phase 004 Plan: Standard Library (Through End of Phase 4)

## Intent

This plan expands `documents/FerricImplementationPlan.md` into an execution sequence that gets Ferric from the end of Phase 3 to the end of **Phase 4: Standard Library (Weeks 27-32)**.

## Starting Point

- Phase 3 language-completion work is complete, including `deffunction`, `defglobal`, `defmodule`, `defgeneric`, `defmethod`, `forall`, template-aware `modify`/`duplicate`, and `printout` runtime behavior.
- Phase 3 remediation explicitly deferred three compatibility closures to Phase 4: cross-module `deffunction`/`defglobal` visibility enforcement, module-qualified `MODULE::name` resolution, and generic dispatch parity (`call-next-method` + CLIPS-style specificity ranking).
- The evaluator already provides a core builtin baseline (math/comparison/boolean/type subset), but the full documented Phase 4 standard-library surface is not yet complete.
- Focus-stack runtime APIs (`set_focus`, `get_focus`, `get_focus_stack`) exist, but Phase 4 must complete callable query/control parity for agenda/focus/environment and validate behavior through integration fixtures.
- Parser -> runtime translation -> core compilation layering remains the architectural contract and should not be collapsed while extending function breadth.

## Phase 4 Targets (from v10 plan)

By the end of this phase, the project should satisfy:

1. Module visibility is enforced for cross-module `deffunction` calls and `defglobal` reads/writes.
2. Module-qualified `MODULE::name` resolution paths are implemented with source-located diagnostics.
3. Same-name `deffunction`/`defgeneric` definitions fail at definition time with explicit conflict diagnostics.
4. Generic dispatch matches documented CLIPS-style specificity behavior and supports `call-next-method`.
5. Documented predicate/math/string/symbol/multifield function sets are implemented.
6. I/O/environment/agenda function surfaces are implemented (including `format`, `read`, `readline`, focus query functions, and agenda visibility queries).
7. `printout` behavior is validated across full supported formatting/routing scenarios.
8. Function behavior is covered through both direct-call and RHS/test expression execution paths.
9. Standard CLIPS examples for the supported subset execute successfully.

## Pass Breakdown

1. `passes/001-Phase4BaselineAndHarnessAlignment.md`
2. `passes/002-ModuleQualifiedNameParsingAndResolutionScaffold.md`
3. `passes/003-CrossModuleDeffunctionAndDefglobalVisibilityEnforcement.md`
4. `passes/004-ModuleQualifiedCallableAndGlobalLookupDiagnostics.md`
5. `passes/005-DeffunctionDefgenericConflictDiagnostics.md`
6. `passes/006-GenericSpecificityRankingAndMethodOrdering.md`
7. `passes/007-CallNextMethodDispatchChainSemantics.md`
8. `passes/008-PredicateMathAndTypeSurfaceParity.md`
9. `passes/009-StringAndSymbolFunctionSurface.md`
10. `passes/010-MultifieldFunctionSurfaceAndEdgeCases.md`
11. `passes/011-IoAndEnvironmentFunctionSurface.md`
12. `passes/012-AgendaAndFocusQueryFunctionSurface.md`
13. `passes/013-Phase4IntegrationAndExitValidation.md`

## Cross-Pass Rules

- Each pass must start and end on a clean, testable baseline.
- No pass should leave the branch non-building or with intentionally failing tests.
- Preserve source spans through parser/runtime/evaluator paths for all new diagnostics.
- Enforce module visibility consistently across all callable/global lookup paths (rule RHS, `test` CE evaluation, user function bodies, generic methods, and global initialization paths where applicable).
- Keep agenda invariants intact: new callable agenda/focus/environment functions must route through existing engine contracts, not bypass rete/agenda bookkeeping.
- `call-next-method` must be deterministic, stack-safe, and recursion-limit aware.
- Restrict builtin expansion to the documented Section 10.2 surface unless the master plan is updated in the same pass.
- Every new function must ship with evaluator-level tests and at least one integration-level fixture or scenario.
- If a pass introduces or mutates stateful runtime stores (e.g., input/output buffers, callable registries, module maps), extend consistency-check coverage in the same pass.

## Execution Notes

- Passes are intentionally linear and dependency-aware; reorder only for hard prerequisites.
- If a prerequisite is small, absorb it into the active pass; if it is larger, insert a new pass without violating dependency order.
- Prefer fixture-driven development for compatibility-sensitive behavior (`MODULE::name` lookups, generic dispatch ordering, `call-next-method`, and stdlib function semantics).
- Keep Phase 4 scope tight to language-compatibility closure + documented stdlib breadth; do not pull FFI/CLI-specific work forward from Phase 5.

## Phase 4 Definition Of Done

Phase 4 is complete when:

1. Module-qualified and cross-module callable/global resolution paths honor import/export visibility with source-located diagnostics.
2. Same-name `deffunction`/`defgeneric` definitions fail with explicit conflict diagnostics.
3. Generic dispatch follows documented specificity and `call-next-method` behavior.
4. All Section 10.2 documented builtin functions are implemented and test-backed.
5. `printout` and new I/O functions (`format`, `read`, `readline`) have deterministic, regression-tested behavior.
6. Agenda/focus/environment callable surfaces operate without breaking agenda/retraction invariants.
7. Integration fixtures and workspace quality gates are clean (`fmt`, `clippy`, `test`, `check`).
8. Representative standard CLIPS examples for the supported subset run successfully.
