# Phase 006 Plan: Polish (Through End of Phase 6)

## Intent

This plan expands `documents/FerricImplementationPlan.md` into an execution sequence that gets Ferric from the end of Phase 5 to the end of **Phase 6: Polish (Weeks 39-44)**.

## Starting Point

- Phase 5 FFI/CLI work and remediation are complete, with green quality gates and stable exported surface contracts.
- Phase 6 now owns three closing tracks: compatibility-suite hardening, performance/benchmark closure, and release-grade documentation/examples.
- External-surface contracts (FFI/CLI) are now explicitly part of the Phase 6 compatibility scope and must remain stable.
- The repository currently has strong integration fixtures but does not yet have the full `tests/clips_compat`, `benches/`, and `docs/compatibility.md` delivery expected by the implementation plan.

## Phase 6 Targets (from v10.1 plan)

By the end of this phase, the project should satisfy:

1. CLIPS compatibility tests for the supported subset are comprehensive, deterministic, and passing.
2. External-surface compatibility lock suites (FFI/CLI) are in place and passing.
3. FFI contract regressions are prevented for canonical naming, configured construction, thread-affinity behavior, copy-to-buffer semantics, fact-id round trips, and action diagnostics APIs.
4. CLI machine-readable diagnostics contracts are locked and tested for `ferric run --json` and `ferric check --json`.
5. Benchmark harness and canonical workloads (`waltz`, `manners`, plus targeted microbenchmarks) exist and are reproducible.
6. Measured performance is within documented target range or accompanied by explicit tracked deltas and optimization follow-up.
7. Benchmark CI policy is upgraded from early smoke posture to blocking release posture for defined benchmark gates.
8. Compatibility documentation is complete for the required Section 16 coverage topics (supported/unsupported surface, restrictions, string semantics, migration, activation ordering, and external FFI/CLI contracts including machine-readable diagnostics).
9. User-facing examples and migration guidance are complete enough for initial release adoption.

## Pass Breakdown

1. `passes/001-Phase6BaselineAndHarnessAlignment.md`
2. `passes/002-ClipsCompatHarnessScaffoldAndFixtureCuration.md`
3. `passes/003-ClipsCompatCoreExecutionSemanticsSuite.md`
4. `passes/004-ClipsCompatLanguageModuleAndStdlibSemanticsSuite.md`
5. `passes/005-ExternalSurfaceCompatibilityFfiContractLockSuite.md`
6. `passes/006-ExternalSurfaceCompatibilityCliJsonContractSuite.md`
7. `passes/007-BenchmarkHarnessAndMeasurementProtocol.md`
8. `passes/008-WaltzAndMannersBenchmarkWorkloads.md`
9. `passes/009-PerformanceProfilingAndBudgetGapAnalysis.md`
10. `passes/010-TargetedHotPathOptimizationImplementation.md`
11. `passes/011-PerformanceRegressionPolicyAndCiBenchmarkGates.md`
12. `passes/012-CompatibilityDocumentationMigrationAndExamples.md`
13. `passes/013-Phase6IntegrationAndReleaseReadinessValidation.md`

## Cross-Pass Rules

- Each pass must start and end on a clean, testable baseline.
- No pass should leave the branch non-building or with intentionally failing tests.
- Compatibility fixtures must preserve diagnostic meaning and source-location fidelity; adapters must not reinterpret engine semantics.
- External-surface contract behavior introduced in Phase 5 is stable unless changed additively with explicit documentation and tests.
- Performance changes must preserve semantics and existing diagnostics exactly.
- Benchmark comparisons must be reproducible: fixed fixture sets, documented runtime environment assumptions, and consistent command paths.
- Do not weaken Phase 5 safety contracts to chase benchmark numbers (thread affinity, panic policy, ownership/copy contracts remain non-negotiable).
- Documentation updates must be synchronized with behavior and tests in the same pass when contracts are touched.

## Execution Notes

- Passes are intentionally linear and dependency-aware; reorder only for hard prerequisites.
- If a prerequisite is small, absorb it into the active pass; if larger, insert a new pass while preserving dependency order.
- Prefer fixture-driven compatibility work and metric-driven optimization work.
- Treat compatibility and performance as release criteria, not optional polish.

## Phase 6 Definition Of Done

Phase 6 is complete when:

1. CLIPS compatibility suites and external-surface compatibility suites pass in CI.
2. FFI/CLI contracts from Phase 5 are explicitly locked by regression tests (including `--json` diagnostics mode).
3. Benchmark workloads are implemented, repeatable, and used in CI with blocking policy where defined.
4. Performance is validated against documented targets with no semantic regressions.
5. `docs/compatibility.md` fully covers the required Section 16 compatibility topics (regardless of heading taxonomy), and migration/examples documentation is publication-ready.
6. Workspace quality gates are clean (`fmt`, `clippy`, `test`, `check`) with benchmark/compatibility evidence suitable for release sign-off.
