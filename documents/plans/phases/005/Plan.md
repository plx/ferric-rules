# Phase 005 Plan: FFI & CLI (Through End of Phase 5)

## Intent

This plan expands `documents/FerricImplementationPlan.md` into an execution sequence that gets Ferric from the end of Phase 4 to the end of **Phase 5: FFI & CLI (Weeks 33-38)**.

## Starting Point

- Phase 4 standard-library and module/generic compatibility closures are complete, including source-located diagnostics for visibility, module-qualified names, ambiguity, and dispatch/conflict failures.
- Workspace currently contains the Rust-facing crates (`ferric`, `ferric-core`, `ferric-parser`, `ferric-runtime`) but does not yet ship `ferric-ffi` and `ferric-cli` crates.
- Runtime semantics and diagnostics now form a stable contract that external surfaces must expose without reinterpretation.
- Phase 5 introduces cross-language ABI concerns (thread affinity, panic policy, ownership, and C buffer conventions) that require explicit contract tests.

## Phase 5 Targets (from v10 plan)

By the end of this phase, the project should satisfy:

1. C programs can create/embed and drive Ferric through a documented C API.
2. Unified FFI error handling is complete across thread-local and per-engine channels.
3. Copy-to-buffer error APIs satisfy the exact size/query/truncation contract.
4. Runtime thread-affinity checks are enforced on every `ferric_engine_*` entry point before any mutable borrow or mutation.
5. FFI panic policy is enforced through `ffi-dev`/`ffi-release` profiles (`panic = "abort"`) with validation coverage.
6. Generated C header includes prominent thread-safety warning block and ownership/lifetime documentation.
7. CLI commands (`run`, `check`, `repl`, `version`) meet documented exit-code and diagnostic behavior.
8. REPL supports balanced-paren multiline input, required commands, and source-located diagnostics.
9. Phase 4 diagnostic contracts (module visibility/ambiguity, module-qualified name failures, generic dispatch/conflict diagnostics) are surfaced unchanged through FFI and CLI.

## Pass Breakdown

1. `passes/001-Phase5BaselineAndHarnessAlignment.md`
2. `passes/002-WorkspaceProfilesAndCrateScaffolding.md`
3. `passes/003-FfiErrorModelAndUnifiedReturnConvention.md`
4. `passes/004-FfiThreadAffinityAndEngineLifecycleApis.md`
5. `passes/005-FfiCoreRunStepAssertAndRetractApis.md`
6. `passes/006-FfiCopyToBufferErrorApisAndEdgeCases.md`
7. `passes/007-FfiExtendedApiValueAndQuerySurface.md`
8. `passes/008-CHeaderGenerationThreadSafetyBannerAndOwnershipDocs.md`
9. `passes/009-FfiArtifactBuildMatrixAndPanicPolicyVerification.md`
10. `passes/010-CliRunCheckVersionCommandsAndDiagnostics.md`
11. `passes/011-ReplInteractiveLoopAndCommandSurface.md`
12. `passes/012-Phase5IntegrationAndExitValidation.md`

## Cross-Pass Rules

- Each pass must start and end on a clean, testable baseline.
- No pass should leave the branch non-building or with intentionally failing tests.
- Every `ferric_engine_*` entry point must perform thread-affinity validation before mutable borrows and before state mutation.
- C-facing diagnostics must preserve runtime source context; do not collapse, rewrite, or reclassify Phase 4 diagnostic meanings at FFI/CLI boundaries.
- FFI APIs that can fail must use `FerricError` return codes with out-parameters for payloads.
- Error-message lifetimes and ownership semantics must be explicit and test-backed for all pointer-returning functions.
- Copy-to-buffer APIs must implement "no error" precedence (`FERRIC_ERROR_NOT_FOUND`) before buffer argument validation.
- The Rust-only `unsafe fn move_to_current_thread` capability remains non-exposed in C APIs.
- FFI header generation, exported symbols, and ownership docs must remain synchronized in the same pass when signatures change.
- Phase 5 should not introduce Phase 6 compatibility/performance expansion beyond what is needed to make FFI/CLI contracts reliable.

## Execution Notes

- Passes are intentionally linear and dependency-aware; reorder only for hard prerequisites.
- If a prerequisite is small, absorb it in the active pass; if larger, insert a new pass without breaking dependency order.
- Prefer fixture-driven verification for diagnostic parity and ABI contracts (thread violation, copy-to-buffer edges, CLI exit/status behavior).
- Keep external surfaces thin wrappers over runtime contracts; avoid duplicated semantic logic in adapters.

## Phase 5 Definition Of Done

Phase 5 is complete when:

1. `ferric-ffi` exports a usable C API with lifecycle, execution, fact operations, and documented ownership.
2. Global/per-engine error retrieval and copy-to-buffer variants pass exhaustive edge-case contract tests.
3. Thread-affinity misuse is caught deterministically with the documented debug/release behavior.
4. FFI profile matrix (`ffi-dev`, `ffi-release`) enforces abort-on-panic for shipped artifacts.
5. Generated `ferric.h` contains the required thread-safety warning and ownership documentation sections.
6. `ferric-cli` supports `run`, `check`, `repl`, and `version` with documented exit behavior.
7. REPL is interactive and functional with multiline continuation, required commands, and diagnostics.
8. Full integration/quality gates are clean and external surfaces preserve Phase 4 diagnostic contracts without loss of source context.
