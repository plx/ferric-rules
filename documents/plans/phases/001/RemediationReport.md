# Phase 001 Remediation Report

## Scope

This report audits Phase 001 implementation consistency against:

- `documents/FerricImplementationPlan.md`
- `documents/plans/phases/001/Plan.md`
- `documents/plans/phases/001/passes/*.md`
- `documents/plans/phases/001/Notes.md`

Consistency definition used: a divergence is acceptable if it is documented and non-problematic for subsequent phases; otherwise it requires remediation.

## Executive Result

Phase 001 is now in a **consistent state** for handoff to Phase 2, with targeted remediations applied in this session for the material gaps that were either undocumented or directly conflicting with Phase 1 contracts.

## Remediations Applied In This Session

### 1) Retraction cleanup now uses `Token.owner_node` for direct beta-memory removal

- **Plan references:** §5.5, §6.6.1, §14.2 (O(1) token cleanup requirement)
- **Previous state:** `ReteNetwork::retract_fact` scanned all beta memories for each removed token.
- **Remediation:** `ReteNetwork::retract_fact` now uses `token.owner_node -> memory` lookup and removes directly.
- **Files changed:**
  - `crates/ferric-core/src/rete.rs`
- **Outcome:** behavior now matches the planned owner-node cleanup contract and eliminates the documented O(memories × removed_tokens) Phase 1 divergence.

### 2) Retraction invariant coverage expanded to include agenda/beta cross-structure integrity

- **Plan references:** §15.0 (retraction invariants + `debug_assert_consistency` expectations)
- **Previous state:** invariant tests primarily exercised token/alpha consistency; agenda and beta cross-check coverage was partial.
- **Remediation:**
  - Added `Agenda::debug_assert_consistency()`.
  - Added `ReteNetwork::debug_assert_consistency()` that checks token/alpha/beta/agenda plus beta-memory token liveness against `TokenStore`.
  - Updated retraction-invariant tests to invoke rete-level consistency checks after assert/retract operations.
- **Files changed:**
  - `crates/ferric-core/src/agenda.rs`
  - `crates/ferric-core/src/rete.rs`
- **Outcome:** Phase 1 invariant harness is now materially aligned with the planned cross-structure consistency intent.

### 3) Engine thread-transfer and generic assert hooks aligned with Phase 1/Pre-implementation contract intent

- **Plan references:** §2.1 (thread-affinity + transfer hook), Pass 003 engine API intent
- **Previous state:** Engine had thread affinity checks but no `move_to_current_thread` hook; only `assert_ordered` existed.
- **Remediation:**
  - Added `unsafe fn move_to_current_thread(&mut self)`.
  - Added `Engine::assert(Fact)` in addition to `assert_ordered`.
  - Added tests for thread handoff and structured fact assertion.
- **Files changed:**
  - `crates/ferric-runtime/src/engine.rs`
- **Outcome:** Engine surface is now closer to the planned contract and better positioned for Phase 2 rule compilation and runtime integration.

## Documented Divergences Considered Consistent (No Immediate Code Remediation Required)

### A) Parser API shape (`ParseResult` vs `Result<Vec<SExpr>, Vec<ParseError>>`)

- **Status:** documented in `Notes.md` (Pass 004).
- **Assessment:** acceptable; richer partial-results API is coherent with recovery strategy and does not block Phase 2.

### B) Loader scope expansion (`deffacts`) and aggregate error return

- **Status:** documented in `Notes.md` (Pass 005).
- **Assessment:** acceptable; additive behavior and non-blocking for subsequent phases.

### C) Type ownership shift from `ferric-runtime` to `ferric-core`

- **Status:** documented in `Notes.md` (Pass 003).
- **Assessment:** acceptable and necessary to avoid crate-dependency cycles.

### D) Rule ingestion remains S-expression-level (`RuleDef`) with manual rete wiring in integration tests

- **Status:** documented in `Notes.md` (Pass 005/009).
- **Assessment:** acceptable for Phase 1, but must be explicitly reflected in master plan wording so expectations for Phase 2 start point are accurate.

## Verification

All quality gates pass after remediation:

- `cargo fmt --all`
- `cargo clippy --workspace --all-targets -- -D warnings`
- `cargo test --workspace`

