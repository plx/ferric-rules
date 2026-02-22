# Phase 5 Remediation Report

## Scope
Consistency remediation against:
- `documents/FerricImplementationPlan.md`
- `documents/plans/phases/005/Plan.md`
- `documents/plans/phases/005/Notes.md`

## Outcome
Phase 5 is now materially closer to a consistent state. Multiple contract gaps were remediated in code and tests.

One consistency gap remains open: FFI does not yet expose runtime action diagnostics (warnings/errors captured during `run`/`step`) through a dedicated C API.

## Resolved Findings

| ID | Status | Remediation Completed |
|---|---|---|
| R5-01 | Closed | Added missing lifecycle API `ferric_engine_new_with_config` with C-facing config types (`FerricConfig`, `FerricStringEncoding`, `FerricConflictStrategy`). Wired through header generation and tests. |
| R5-02 | Closed | Enforced thread-affinity check in `ferric_engine_clear_error` before mutable borrow/mutation, matching the thread-safety contract. Added cross-thread regression test proving no mutation on violation. |
| R5-03 | Closed | `ferric_engine_assert_string` now returns a real fact ID (when available) via `out_fact_id` instead of always returning `0`. Added assert/retract round-trip regression test. |
| R5-04 | Closed | Added machine-friendly diagnostics mode for CLI (`ferric run --json`, `ferric check --json`) with integration coverage for parseable error output. |
| R5-05 | Closed | Strengthened parity tests for Phase 4 conflict/visibility behavior where currently surfaceable (FFI load-time construct conflict diagnostics; CLI runtime visibility warning diagnostics). |

## Remaining Findings

| ID | Severity | Finding | Required Remediation |
|---|---|---|---|
| R5-06 | High | FFI still lacks a stable API to retrieve runtime action diagnostics produced during execution (for example Phase 4 visibility/ambiguity/generic dispatch warnings). This blocks full compliance with the "diagnostics surfaced through FFI unchanged" goal for non-fatal runtime diagnostics. | Add explicit action-diagnostic retrieval APIs (count/get/copy/clear or equivalent), then add parity tests for module visibility, module/global ambiguity, module-qualified failures, and generic dispatch/conflict runtime diagnostics. |

## Validation
Executed and passing:
- `cargo fmt --all`
- `cargo clippy -p ferric-ffi -p ferric-cli --all-targets -- -D warnings`
- `cargo test -p ferric-ffi --tests`
- `cargo test -p ferric-cli --test cli_integration`
- `cargo check --workspace --all-targets`

## Key Files Updated In This Remediation
- `crates/ferric-ffi/src/types.rs`
- `crates/ferric-ffi/src/engine.rs`
- `crates/ferric-ffi/cbindgen.toml`
- `crates/ferric-ffi/ferric.h`
- `crates/ferric-ffi/src/tests/lifecycle.rs`
- `crates/ferric-ffi/src/tests/execution.rs`
- `crates/ferric-ffi/src/tests/diagnostic_parity.rs`
- `crates/ferric-ffi/src/tests/header.rs`
- `crates/ferric-cli/src/commands/run.rs`
- `crates/ferric-cli/src/commands/check.rs`
- `crates/ferric-cli/tests/cli_integration.rs`
