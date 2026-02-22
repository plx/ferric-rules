# Phase 5 Plan Adjustments

## Purpose
Adjust `documents/FerricImplementationPlan.md` so it matches the implemented Phase 5 surface and contracts.

## Required Updates To Master Plan

## 1) FFI Lifecycle Surface: Add Configured Constructor
Update Section 11 lifecycle APIs to include:
- `ferric_engine_new_with_config(const FerricConfig* config)`
- C-facing config types:
  - `FerricStringEncoding`
  - `FerricConflictStrategy`
  - `FerricConfig { string_encoding, strategy, max_call_depth }`

Reason: this API and type surface are now implemented and exported in `ferric.h`.

## 2) Thread-Affinity Contract: Clarify Diagnostic Exceptions
In Section 11.2 and header-thread-safety text, explicitly keep the exception list for diagnostic access:
- `ferric_engine_last_error`
- `ferric_engine_last_error_copy`

Reason: these entry points intentionally skip affinity checks; mutating entry points remain affinity-checked before mutation.

## 3) Core FFI Names: Align Plan Examples To `ferric_engine_*`
Update API examples/prototypes that still use unprefixed names (`ferric_run`, `ferric_step`, `ferric_assert_string`, `ferric_retract`) to the implemented exported names:
- `ferric_engine_run`
- `ferric_engine_step`
- `ferric_engine_assert_string`
- `ferric_engine_retract`

Reason: Phase 5 exports and tests are standardized on the `ferric_engine_*` naming family.

## 4) Assert Fact-ID Contract
Update the assert API notes to reflect current behavior:
- `ferric_engine_assert_string(..., out_fact_id)` now returns a usable fact ID when an assert is produced.

Reason: implementation now satisfies practical assert/retract round-trip expectations.

## 5) CLI Machine-Friendly Diagnostics
In Section 12 (CLI goals/commands), add explicit mention of the implemented machine-friendly mode:
- `ferric run --json <file>`
- `ferric check --json <file>`

Reason: parseable diagnostics are now implemented and tested.

## 6) Copy-To-Buffer Semantics: Reconcile Documented Contract Text
Section 11.4.1 should be reconciled with the current exported behavior:
- `out_len` is currently required (null returns `FERRIC_ERROR_INVALID_ARGUMENT`).
- For copy/truncation paths, `out_len` currently reports required size including NUL.

Reason: current implementation and tests use this contract; the plan text currently describes a different `out_len` model.

## 7) Thread-Violation Debug/Release Behavior Text
Section 11.2 currently describes debug assertion-abort vs release error-code behavior. Implementation consistently returns `FERRIC_ERROR_THREAD_VIOLATION` for violations (no debug assert path in FFI functions).

Reason: this was an intentional FFI safety choice and should be reflected in the master plan.

## 8) FFI Runtime Action-Diagnostic Retrieval Surface
Add an explicit FFI diagnostic-retrieval subsection for non-fatal runtime action diagnostics produced by execution:
- `ferric_engine_action_diagnostic_count`
- `ferric_engine_action_diagnostic_copy`
- `ferric_engine_clear_action_diagnostics`

Reason: this is now implemented and is required to satisfy the Phase 5 diagnostic-parity contract for runtime warnings/errors surfaced during `run`/`step`.

## Implications For Subsequent Phases
1. Wrapper work should target the actual exported `ferric_engine_*` API names and the new `FerricConfig` constructor path.
2. If optional-`out_len` semantics are desired for wrappers later, introduce additive v2 copy APIs rather than silently changing existing behavior.
3. Wrapper documentation in Phase 6 should include guidance for consuming action diagnostics through the new count/copy/clear API family.
