# Pass 005: External Surface Compatibility FFI Contract Lock Suite

## Objective

Lock Phase 5 FFI contracts with dedicated compatibility regression tests to prevent accidental ABI/behavior drift.

## Scope

- Canonical exported naming and lifecycle/configured constructor path.
- Thread-affinity and diagnostic-exception behavior.
- Copy-to-buffer semantics, fact-id round trip, and runtime action diagnostics APIs.

## Tasks

1. Add contract tests verifying wrapper-facing canonical names and configured construction (`ferric_engine_new_with_config`, config enums/struct behavior).
2. Add regression tests for thread-affinity contract: pre-mutation checks, `FERRIC_ERROR_THREAD_VIOLATION`, and diagnostic-read exceptions (`ferric_engine_last_error`, `ferric_engine_last_error_copy`).
3. Add exhaustive compatibility checks for copy-to-buffer semantics (`out_len` required, no-error precedence, query/truncation/length rules).
4. Add assert/retract round-trip tests verifying usable fact IDs from `ferric_engine_assert_string` when assertions are produced.
5. Add lock tests for runtime action-diagnostic API lifecycle (`count`/`copy`/`clear`) across run/step flows.

## Definition Of Done

- FFI compatibility lock suite covers all Phase 5 contract-carryover items.
- Behavior is explicitly regression-protected and green.

## Verification Commands

- `cargo test -p ferric-ffi --tests`
- `cargo test -p ferric-ffi action_diagnostics`

## Handoff State

- FFI surface is contract-locked for Phase 6 and future refactors.
