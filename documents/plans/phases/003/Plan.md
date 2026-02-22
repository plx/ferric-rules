# Phase 003 Plan: Language Completion (Through End of Phase 3)

## Intent

This plan expands `documents/FerricImplementationPlan.md` into an execution sequence that gets Ferric from the end of Phase 2 to the end of **Phase 3: Language Completion (Weeks 21-26)**.

## Starting Point

- Phase 2 core engine work and documented remediations are complete (`documents/plans/phases/002/Progress.txt`).
- Stage 2 interpretation is operational for `defrule`, `deftemplate`, and `deffacts`; remaining top-level constructs are still deferred.
- Runtime action execution exists, but Phase 2 carries known narrowing: `modify`/`duplicate` are not fully template-aware and `printout` is still placeholder behavior.
- Pattern validation, NCC, and exists baselines are in place; the `forall` vacuous-truth regression fixture shape is scaffolded and must be fully enabled in this phase.
- The parser -> runtime translation -> core compilation layering is now a required architectural contract and should remain intact.

## Phase 3 Targets (from v10 plan)

By the end of this phase, the project should satisfy:

1. Runtime carryover closure: template-aware `modify`/`duplicate`.
2. Non-placeholder `printout` behavior through runtime function plumbing.
3. A single function-call evaluation path used by RHS actions and `test` expressions.
4. `deffunction` parsing/loading/runtime execution.
5. `defglobal` parsing/loading/runtime semantics.
6. `defmodule` import/export semantics and focus integration.
7. `defgeneric`/`defmethod` parsing and runtime dispatch.
8. Limited `forall` implementation on top of existing NCC/exists semantics, including vacuous truth and retraction-cycle behavior.
9. Stable, source-located diagnostics for unsupported/invalid forms (no silent degradation).

## Pass Breakdown

1. `passes/001-Phase3BaselineAndHarnessAlignment.md`
2. `passes/002-ExpressionEvaluationPathForRhsAndTest.md`
3. `passes/003-TemplateAwareModifyAndDuplicateSemantics.md`
4. `passes/004-PrintoutRuntimeAndRouterIntegration.md`
5. `passes/005-Stage2DeffunctionAndDefglobalInterpretation.md`
6. `passes/006-UserDefinedFunctionEnvironmentAndExecution.md`
7. `passes/007-Stage2DefmoduleDefgenericDefmethodInterpretation.md`
8. `passes/008-DefmoduleImportExportAndFocusSemantics.md`
9. `passes/009-DefgenericDefmethodDispatchRuntime.md`
10. `passes/010-ForallLimitedSemanticsAndRegressionContract.md`
11. `passes/011-Phase3IntegrationAndExitValidation.md`

## Cross-Pass Rules

- Each pass must start and end on a clean, testable baseline.
- Unsupported constructs must fail with explicit diagnostics; never silently drop behavior.
- Preserve source spans from parser through runtime translation and compile/validation errors.
- Keep parser and core decoupled; runtime owns translation into parser-agnostic compile/runtime models.
- Any new structure storing `FactId`, `TokenId`, or `ActivationId` must include cleanup/invariant coverage in the same pass.
- Use one evaluator path for function-call expressions across RHS actions, `test` CE evaluation, and user-defined function invocation.
- Keep module/function/global visibility rules deterministic and test-backed.

## Execution Notes

- Passes are intentionally linear. Reorder only if a hard prerequisite is discovered.
- If a prerequisite is small, absorb it into the active pass; otherwise insert a new pass without breaking dependency order.
- Keep Phase 3 scope constrained to language/runtime completion. Do not pull in broad Phase 4 stdlib expansion except the minimal plumbing needed by Phase 3 behaviors.
- Maintain fixture-driven progress: each new construct should gain at least one `.clp` integration fixture in the pass where it becomes executable.

## Phase 3 Definition Of Done

Phase 3 is complete when:

1. `modify`/`duplicate` are template-aware and preserve rete/retraction invariants.
2. `printout` is implemented and testable (no placeholder behavior on supported channels).
3. Function-call expression evaluation is shared across RHS, `test`, and callable runtime surfaces.
4. `deffunction` and `defglobal` are loadable and executable with source-located diagnostics.
5. `defmodule` import/export and focus behavior are implemented with deterministic resolution.
6. `defgeneric`/`defmethod` load and dispatch correctly, including error behavior for non-applicable/ambiguous methods.
7. Limited `forall` semantics are implemented and the vacuous-truth retraction-cycle regression contract passes.
8. Integration fixtures and workspace quality gates pass cleanly (`fmt`, `clippy`, `test`, `check`).
