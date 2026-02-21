# Phase 4 Plan Adjustments (For `documents/FerricImplementationPlan.md`)

## Purpose

This document proposes updates to the master implementation plan so it accurately reflects Phase 4 outcomes and the required follow-on work.

## Summary

Phase 4 delivered most targeted surface area, but module/global resolution semantics are not fully complete. The plan should be updated to explicitly capture the unresolved items as required remediation before Phase 5 interface hardening.

## Proposed Master-Plan Changes

### A1. Tighten Phase 4 Exit Criteria Language Around Module Namespaces

**Where to update**

- `documents/FerricImplementationPlan.md` §15 "Phase 4: Standard Library"

**Current wording risk**

- Exit criteria imply module-qualified/cross-module callable/global behavior is complete.

**Proposed adjustment**

Add explicit criterion:

- "Callable/global registries must be module-scoped (not name-global), so same local names may coexist across modules and resolve correctly with `MODULE::name` and import/export rules."

**Why**

Current implementation keys registries by unqualified name, which breaks module namespace expectations and qualified resolution completeness.

---

### A2. Add Explicit Defglobal Write-Semantics Requirement

**Where to update**

- `documents/FerricImplementationPlan.md` §15 Phase 4 deliverables/exit criteria

**Proposed adjustment**

Make write behavior explicit:

- "`bind` to `defglobal` must enforce the same visibility/ownership rules as global reads, including source-located diagnostics for unknown/not-visible targets."

**Why**

The plan already says reads/writes, but implementation criteria and tests focused on reads. This needs explicit gating text.

---

### A3. Clarify Qualified Defglobal Syntax In Language/Parser Section

**Where to update**

- `documents/FerricImplementationPlan.md` §8 parser/language section

**Proposed adjustment**

Add canonical syntax and requirement:

- Canonical qualified global reference syntax (for example `?*MODULE::name*`, if this is the chosen form).
- Parser must preserve this form through Stage 2 to runtime resolver paths.

**Why**

Without explicit syntax requirements, qualified-global support can appear "implemented" while being unreachable from source text.

---

### A4. Add A "Phase 4 Remediation Gate" Before Phase 5

**Where to update**

- `documents/FerricImplementationPlan.md` §15 between Phase 4 and Phase 5

**Proposed adjustment**

Insert a short remediation gate listing:

1. module-scoped callable/global namespace corrections,
2. qualified-global syntax + diagnostics completion,
3. bind visibility/write enforcement,
4. regression coverage for same-name cross-module constructs and qualified global reads/writes.

**Why**

Phase 5 (FFI/CLI) depends on stable diagnostic and resolution semantics. Freezing external surfaces before this remediation would harden incorrect behavior.

---

### A5. Behavior Clarifications For Documentation Accuracy

**Where to update**

- `documents/FerricImplementationPlan.md` §9 API/runtime behavior notes
- optionally §10.2 notes column where applicable

**Proposed adjustments**

1. Clarify actual `(reset)`-from-RHS behavior in `run()` (current implementation returns after reset rather than continuing same run invocation).
2. Clarify `format` channel behavior (current implementation evaluates channel arg but does not route output; returns string).
3. Clarify whether `read`/`readline` are intended to work in nested callable contexts (deffunction/defmethod bodies). If yes, make it a requirement; if no, state the restriction.

**Why**

These are currently ambiguous or mismatched between notes and code; explicit plan text reduces drift in later phases.

---

## Implications For Subsequent Phases

1. **Phase 5 FFI/CLI**
   - Should not finalize diagnostic mapping/contracts for module/global resolution until remediation gate passes.
2. **Compatibility Documentation (Phase 6)**
   - Must document final, corrected module namespace and qualified-global semantics, not interim behavior.
3. **Testing Strategy**
   - Add dedicated regression fixture group for module namespace collisions and qualified global resolution.

## Suggested Plan Note To Add (Short Form)

"Phase 4 function-surface completion is accepted conditionally, pending module/global namespace remediation (qualified defglobal syntax, module-scoped callable/global registries, and bind write-visibility enforcement). Phase 5 interface stabilization begins only after this gate is closed."
