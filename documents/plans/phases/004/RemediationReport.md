# Phase 4 Remediation Report

## Scope
Consistency remediation against:
- `documents/FerricImplementationPlan.md`
- `documents/plans/phases/004/Plan.md`
- `documents/plans/phases/004/Notes.md`

## Outcome
Phase 4 is now in a consistent state for the previously identified gaps.

## Resolved Findings

| ID | Status | Remediation Completed |
|---|---|---|
| R4-01 | Closed | Re-keyed `deffunction`, `defglobal`, and `defgeneric` registries to `(ModuleId, local-name)` and updated loader/evaluator resolution to module-aware behavior. Same local names now coexist across modules. |
| R4-02 | Closed | `bind` now enforces defglobal visibility and ownership semantics, and rejects undeclared global targets. |
| R4-03 | Closed | Lexer now supports qualified global tokenization in canonical form `?*MODULE::name*`; Stage 2 preserves this through to runtime resolution. |
| R4-04 | Closed | Nested callable frames (`deffunction`, `defmethod`, `call-next-method`) now preserve input-buffer access for `read`/`readline`. |
| R4-05 | Closed | Phase 4 notes were reconciled to runtime behavior (notably reset/clear run semantics and qualified-global syntax wording), with a remediation addendum added. |

## Key Implementation Changes
- Runtime namespace model:
  - `crates/ferric-runtime/src/functions.rs`
  - `crates/ferric-runtime/src/loader.rs`
  - `crates/ferric-runtime/src/evaluator.rs`
  - `crates/ferric-runtime/src/engine.rs`
  - `crates/ferric-runtime/src/actions.rs`
- Qualified global parsing:
  - `crates/ferric-parser/src/lexer.rs`
  - `crates/ferric-parser/src/stage2.rs`
- Regression coverage additions:
  - `crates/ferric-runtime/src/phase4_integration_tests.rs`
- Runtime safety guard update (default recursion depth):
  - `crates/ferric-runtime/src/config.rs`
- Documentation reconciliation:
  - `documents/plans/phases/004/Notes.md`

## Validation
Executed and passing:
- `cargo test -p ferric-parser`
- `cargo test -p ferric-runtime`

Notable new/updated coverage includes:
- qualified global success/failure (`?*MODULE::name*`)
- bind undeclared/not-visible failures
- same-name callable/global coexistence across modules
- unqualified ambiguity diagnostics
- `read`/`readline` behavior in nested callable contexts (existing cross-feature test now passes with propagated input buffer)

## Residual Risk
No known Phase 4 consistency blockers remain from the remediation findings.
