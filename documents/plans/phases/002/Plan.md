# Phase 002 Plan: Core Engine (Through End of Phase 2)

## Intent

This plan expands `documents/FerricImplementationPlan.md` into an execution sequence that gets Ferric from the end of Phase 1 to the end of **Phase 2: Core Engine (Weeks 11-20)**.

## Starting Point

- Phase 1 foundation is in place: workspace/CI, value+symbol infrastructure, Stage 1 parser, minimal loader, alpha/beta scaffolding, token store, and initial retraction invariants.
- Rule ingestion still relies on S-expression-level `RuleDef` placeholders; automatic rule compilation from parsed constructs into rete is not complete.
- Negative/NCC/exists nodes, full agenda strategies, and runtime rule firing/action execution are not yet complete.
- `documents/plans/phases/001/PlanAdjustments.md` defines baseline corrections that Phase 2 should assume (type ownership, parser/load contracts, and current retraction cleanup behavior).

## Phase 2 Targets (from v10 plan)

By the end of this phase, the project should satisfy:

1. Stage 2 construct interpretation for `deftemplate`, `defrule`, and `deffacts` (with source-located diagnostics).
2. Rule compilation from interpreted constructs into shared rete network structure.
3. Complete join/binding propagation needed for practical multi-pattern rules.
4. Negative pattern support (`not <single-pattern>`) with blocker tracking and correct retraction behavior.
5. Full agenda conflict strategy support (`Depth`, `Breadth`, `LEX`, `MEA`) with stable total ordering.
6. Engine execution loop support (`run`, `step`, `halt`, `reset`) and Phase 2 RHS action subset (`assert`, `retract`, `modify`, `duplicate`).
7. NCC and exists support for `(not (and ...))` and `(exists ...)`.
8. Compile-time pattern restriction validation (Section 7.7, stable error codes `E0001`-`E0005`) with source spans.
9. Expanded invariant and integration coverage, including real `.clp` fixtures and negative/NCC/exists cleanup checks.

## Pass Breakdown

1. `passes/001-Phase2BaselineAndHarnessAlignment.md`
2. `passes/002-Stage2ConstructAstAndInterpreterScaffold.md`
3. `passes/003-Stage2DeftemplateDefruleAndDeffactsInterpretation.md`
4. `passes/004-RuleCompilationPipelineAndNodeSharing.md`
5. `passes/005-JoinBindingExtractionAndLeftActivationCompletion.md`
6. `passes/006-NegativeNodeSinglePatternAndBlockerTracking.md`
7. `passes/007-AgendaConflictStrategiesAndOrderingContract.md`
8. `passes/008-RunStepHaltAndResetExecutionLoop.md`
9. `passes/009-ActionExecutionAssertRetractModifyAndDuplicate.md`
10. `passes/010-NccAndExistsNodesAndCleanupInvariants.md`
11. `passes/011-PatternValidationAndSourceLocatedCompileErrors.md`
12. `passes/012-Phase2IntegrationAndExitValidation.md`

## Cross-Pass Rules

- Each pass must start from a clean baseline and end in a clean baseline.
- No pass should leave the branch in a non-building or non-testable state.
- Every pass must run targeted tests plus workspace `cargo check` before handoff.
- Retraction cleanup must remain index-driven (no global scans on hot paths).
- Any structure that stores `TokenId` or `ActivationId` must include cleanup hooks/invariant checks in the same pass it is introduced.
- Unsupported pattern constructs must fail compilation in both classic and strict modes (severity may differ, load result must still fail).
- Source spans should be preserved across parser -> interpreter -> validator paths; do not add span-less shortcuts.

## Execution Notes

- Passes are intentionally linear and dependency-aware; do not reorder unless a prior pass reveals a hard prerequisite.
- If a discovered prerequisite is small, include it in the current pass; if not, insert a new pass and keep sequence order.
- Keep Phase 2 scope focused on core engine semantics. Do not pull in Phase 3+ language breadth except where forward-compatible scaffolding is explicitly needed.
- For the required `forall_vacuous_truth_and_retraction_cycle` regression scenario in Section 7.5, add the fixture and test shape in Phase 2 so Phase 3 forall implementation plugs into an existing contract.

## Phase 2 Definition Of Done

Phase 2 is complete when:

1. `.clp` loading supports `deftemplate`, `defrule`, and `deffacts` through Stage 2 interpretation and compilation.
2. Rules compile into rete and execute through `run`/`step` with proper agenda management.
3. Retraction remains correct across beta, negative, NCC, exists, and agenda structures.
4. Negative single-pattern and conjunction-negation (`NCC`) behavior is correct under assert/retract churn.
5. Exists behavior is correct under support-add/support-remove cycles.
6. Pattern restriction violations are rejected at compile time with stable codes and source-located diagnostics.
7. Integration and invariant suites pass, including real `.clp` fixtures and full workspace quality gates.
