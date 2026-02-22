# Phase 002 Remediation Report

## Scope

This report audits Phase 002 implementation consistency against:

- `documents/FerricImplementationPlan.md`
- `documents/plans/phases/002/Plan.md`
- `documents/plans/phases/002/passes/*.md`
- `documents/plans/phases/002/Notes.md`

Consistency definition used: a divergence is acceptable only when it is both documented and non-problematic for subsequent phases. If a divergence is undocumented, or documented-but-problematic, remediation is required.

## Executive Result

Phase 002 is **not yet in a consistent state** with the Phase 2 planning contract.

Most implemented areas are solid and well-tested (Stage 2 interpretation, positive joins, negative single-pattern behavior, agenda strategies, run/step/halt/reset loop, and single-pattern exists). The remaining inconsistencies are concentrated in NCC completeness and unsupported-pattern handling, with a few architectural contract mismatches.

## Required Remediations

### 1) Complete NCC semantics for `(not (and ...))` before Phase 2 closure

- **Plan references:** `FerricImplementationPlan.md` Sections 7.3 and 15 (Phase 2 exit), `documents/plans/phases/002/Plan.md` Targets 7 and DoD item 4, Pass 010/012 task lists.
- **Current state:** NCC memory and node scaffolding exists, but end-to-end NCC semantics are not implemented.
  - `crates/ferric-core/src/rete.rs` keeps `ncc_partner_receive_result(...)` as a stub.
  - Loader/parser path does not compile or execute conjunction-negation behavior.
  - Runtime integration tests explicitly leave NCC as a planned area (`crates/ferric-runtime/src/phase2_integration_tests.rs`).
  - No NCC `.clp` fixture exists in the Phase 2 fixture set.
- **Notes coverage:** Documented as a TODO in `documents/plans/phases/002/Notes.md`.
- **Why this is still inconsistent:** Phase artifacts (`Progress.txt`, pass completion language) mark Phase 2 complete despite an unmet required Phase 2 capability.
- **Remediation:**
  1. Implement parser + compiler support for NCC shape and topology.
  2. Implement partner result accounting and unblock/reblock behavior in rete runtime.
  3. Add NCC fixture + integration tests under assert/retract churn.
  4. Re-run Phase 2 exit gates only after NCC behavior is green.

### 2) Eliminate silent rule downgrades for unsupported patterns/constraints

- **Plan references:** `FerricImplementationPlan.md` Section 2.3 and Section 7.7, plus Phase 2 cross-pass rule in `documents/plans/phases/002/Plan.md`.
- **Current state:** Unsupported forms can be silently dropped during translation/compilation:
  - `translate_pattern(...)` returns `Ok(None)` for `Pattern::Test`, `Pattern::Template`, and multi-pattern `Pattern::Exists`.
  - `translate_rule_construct(...)` skips `None` patterns and still compiles the remaining rule.
  - `translate_constraint(...)` ignores unsupported branches (`Or`, multi-variable, non-literal negation).
- **Notes coverage:** Partially documented (Pass 004/010 notes), but behavior still violates the plan's fail-compile policy for unsupported constructs.
- **Why this is inconsistent:** Semantics can change without an error, violating the explicit "unsupported constructs must fail compilation" contract.
- **Remediation:**
  1. Convert unsupported translation branches into explicit compile/validation errors.
  2. Reserve/assign stable validation codes for these additional unsupported forms.
  3. Add regression tests proving unsupported forms fail in load/compile paths.

### 3) Fix `not` arity handling so multi-element negation is not silently misread

- **Plan references:** Sections 7.2, 7.3, 7.7; Pass 006/010 parser-compiler assumptions.
- **Current state:** In Stage 2 parsing, `not` consumes only the first child and ignores additional children (`crates/ferric-parser/src/stage2.rs`).
- **Notes coverage:** Not documented.
- **Why this is inconsistent:** Author intent can be misinterpreted without diagnostics, and this conflicts with the intended NCC pathway for conjunction negation.
- **Remediation:**
  1. Enforce strict arity/shape validation for `not`.
  2. Emit source-located errors for invalid/multi-element `not` forms until NCC syntax is explicitly compiled.
  3. Add parser + loader tests for these forms.

### 4) Align validation ownership with compiler contract (or formally revise the contract)

- **Plan references:** `FerricImplementationPlan.md` Section 6.7 (`ReteCompiler` owns validator) and Section 7.7 (validation before node construction).
- **Current state:** Validation is performed in runtime loader (`crates/ferric-runtime/src/loader.rs`) before translation; core compiler does not enforce validator policy directly.
- **Notes coverage:** Documented indirectly (Pass 011 notes), but not reconciled against Section 6.7 design.
- **Why this is inconsistent:** Direct users of `ferric-core` compiler can bypass planned validation guarantees.
- **Remediation:**
  1. Move/enforce validation in `ferric-core::ReteCompiler::compile_rule`, or
  2. Introduce a single validated compile entry point in core and require runtime to use it.

### 5) Resolve join-sharing contract drift

- **Plan references:** `FerricImplementationPlan.md` Sections 6.2 and 6.7; Pass 004 scope/DoD.
- **Current state:** Alpha-path sharing is implemented; canonical join-node sharing (`JoinNodeKey` cache) is not.
- **Notes coverage:** Not explicitly documented as a deviation.
- **Why this matters:** This is not a semantic correctness blocker, but it is a direct mismatch with planned compiler structure and performance assumptions.
- **Remediation decision required:**
  1. Implement join-node cache now (preferred for strict plan consistency), or
  2. Explicitly de-scope join sharing from Phase 2 and move it to later performance work with updated plan text.

### 6) Correct phase-status artifacts after remediation

- **Plan references:** Phase 2 DoD and Pass 012 handoff requirements.
- **Current state:** `documents/plans/phases/002/Progress.txt` and final notes language state "complete" while required behavior remains open.
- **Remediation:**
  1. Keep status as incomplete until items 1-3 are fixed.
  2. After remediation, re-run and record all Phase 2 quality gates and fixture evidence.

## Recommended Remediation Sequence

1. Implement fail-fast unsupported-form handling (Item 2) and `not` arity fix (Item 3).
2. Complete NCC semantics and NCC integration fixtures/tests (Item 1).
3. Resolve validator ownership and join-sharing decision (Items 4-5).
4. Update progress/notes and rerun full exit gates (Item 6).

## Verification Gates After Remediation

- `cargo fmt --all --check`
- `cargo clippy --workspace --all-targets -- -D warnings`
- `cargo test --workspace`
- `cargo check --workspace --all-targets`
- Plus explicit NCC integration fixture and unsupported-form failure regression coverage.
