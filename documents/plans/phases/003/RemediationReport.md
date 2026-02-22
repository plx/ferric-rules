# Phase 003 Remediation Report

## Purpose

This report identifies Phase 3 implementation inconsistencies against:

- `/Users/prb/github/ferric-rules/documents/FerricImplementationPlan.md`
- `/Users/prb/github/ferric-rules/documents/plans/phases/003/Plan.md`
- `/Users/prb/github/ferric-rules/documents/plans/phases/003/passes/*.md`

“Consistent” means either implemented as planned, or diverged with explicit documentation and acceptable downstream impact.

## Consistency Findings

| ID | Severity | Finding | Documented? | Downstream Risk | Primary Evidence |
|---|---|---|---|---|---|
| R1 | High | `forall_vacuous_truth_and_retraction_cycle` regression contract is not fully implemented (missing assert/retract transition coverage). | Partially (overstated as complete) | High | `/Users/prb/github/ferric-rules/documents/FerricImplementationPlan.md` (Section 7.5 contract), `/Users/prb/github/ferric-rules/crates/ferric-runtime/src/phase2_integration_tests.rs`, `/Users/prb/github/ferric-rules/crates/ferric-runtime/src/phase3_integration_tests.rs`, `/Users/prb/github/ferric-rules/tests/fixtures/forall_vacuous_truth.clp` |
| R2 | Medium | RHS `focus` silently drops unknown modules; pass plan requires explicit invalid-module diagnostics. | No | Medium | `/Users/prb/github/ferric-rules/crates/ferric-runtime/src/actions.rs`, `/Users/prb/github/ferric-rules/crates/ferric-runtime/src/engine.rs`, `/Users/prb/github/ferric-rules/documents/plans/phases/003/passes/008-DefmoduleImportExportAndFocusSemantics.md` |
| R3 | Medium | Duplicate-definition diagnostics promised in pass plans are missing (`defglobal`, `defmodule`, `defgeneric`, `defmethod`). | No | Medium | `/Users/prb/github/ferric-rules/crates/ferric-parser/src/stage2.rs`, `/Users/prb/github/ferric-rules/crates/ferric-runtime/src/loader.rs`, pass 005/007 docs |
| R4 | Medium | Unbound variable/global evaluator errors do not carry source spans (`span: None`), conflicting with source-located diagnostics target. | No | Medium | `/Users/prb/github/ferric-rules/crates/ferric-runtime/src/evaluator.rs`, pass 002 doc |
| R5 | Medium | Module visibility enforcement is template-only; pass 008 scope included deterministic cross-module resolution for rules/templates/functions/globals. | Yes (as TODO) | Medium | `/Users/prb/github/ferric-rules/crates/ferric-runtime/src/loader.rs`, `/Users/prb/github/ferric-rules/documents/plans/phases/003/Notes.md` |
| R6 | Medium | Public focus API drift vs implementation plan (`set_focus`/`get-focus` contract vs current stack-style API). | No | Medium | `/Users/prb/github/ferric-rules/documents/FerricImplementationPlan.md`, `/Users/prb/github/ferric-rules/crates/ferric-runtime/src/engine.rs` |
| R7 | Low | Pass 001 documentation still states unsupported-form baseline for constructs that are now loadable. | No | Low | `/Users/prb/github/ferric-rules/documents/plans/phases/003/Notes.md`, `/Users/prb/github/ferric-rules/documents/plans/phases/003/Progress.txt`, `/Users/prb/github/ferric-rules/crates/ferric-runtime/src/phase3_integration_tests.rs` |
| R8 | Low | `printout` channel argument behavior in runtime diverges from notes (notes describe literal-only channel semantics). | No | Low | `/Users/prb/github/ferric-rules/crates/ferric-runtime/src/actions.rs`, `/Users/prb/github/ferric-rules/documents/plans/phases/003/Notes.md` |
| R9 | Low | Invariant harness extension points for new Phase 3 registries were planned but not completed. | Yes | Low-Med | `/Users/prb/github/ferric-rules/documents/plans/phases/003/passes/001-Phase3BaselineAndHarnessAlignment.md`, `/Users/prb/github/ferric-rules/documents/plans/phases/003/Notes.md` |

## Required Remediation Work

1. Close R1 first: implement full 6-step `forall` vacuous-truth/retraction-cycle regression contract exactly as specified in Section 7.5.
2. Fix R2: unknown modules in RHS `focus` must produce explicit runtime/action diagnostics (no silent ignore).
3. Fix R3: add duplicate-definition validation with source-located errors for globals/modules/generics/methods (including duplicate explicit method indices).
4. Fix R4: preserve spans for variable/global references through evaluator translation and include them in `UnboundVariable`/`UnboundGlobal`.
5. Resolve R5 explicitly: either implement function/global module visibility now, or formally defer in plan docs with Phase 4 entry tasks.
6. Resolve R6 explicitly: either implement `set_focus`/focus query parity, or revise master API contract to current stack-based semantics.
7. Correct R7 docs so pass summaries match current tests.
8. Resolve R8 by choosing and enforcing one `printout` channel contract (literal-only vs expression-evaluated) in both code and docs.
9. Complete R9 by extending debug consistency checks to module/function/global registries and focus stack integrity.

## Execution Order

1. R1
2. R2, R3, R4
3. R5, R6
4. R7, R8, R9

## Consistency Exit Condition

Phase 3 is “consistent” when all High/Medium items above are either implemented or explicitly deferred in both:

- `/Users/prb/github/ferric-rules/documents/plans/phases/003/Notes.md`
- `/Users/prb/github/ferric-rules/documents/FerricImplementationPlan.md`

with downstream phase impacts clearly stated.
