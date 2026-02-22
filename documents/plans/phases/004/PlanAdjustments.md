# Phase 4 Plan Adjustments

## Purpose
Adjust `documents/FerricImplementationPlan.md` so it matches actual Phase 4 behavior after remediation.

## Recommended Updates To Master Plan

## 1) Phase 4 Exit Criteria: Namespace Semantics
Add explicit acceptance criteria in the Phase 4 section:
- Callable/global registries are module-scoped (`(ModuleId, local-name)`), not globally keyed by unqualified name.
- Same local names are allowed across modules.
- Unqualified lookup is caller-module-first, then visible imports; multiple visible matches produce explicit ambiguity diagnostics.

Reason: this is now implemented and is required for consistent `MODULE::name` semantics.

## 2) Canonical Qualified Global Syntax
In parser/language sections, explicitly document canonical qualified global syntax as:
- `?*MODULE::name*`

Reason: this is the implemented source form that now lexes/parses and resolves end-to-end.

## 3) Defglobal Write Semantics (`bind`)
In Phase 4/runtime semantics, make write rules explicit:
- `bind` uses the same visibility enforcement as global reads.
- `bind` does not create undeclared globals; unknown targets produce unbound-global diagnostics.

Reason: this was a key remediation change and should be normative in the plan.

## 4) Reset/Clear Runtime Semantics
In runtime behavior notes, correct/reset wording to:
- RHS `(reset)` and `(clear)` are deferred until action completion, then the current `run()` invocation returns after applying the operation.

Reason: this matches engine behavior and Phase 4 notes after reconciliation.

## 5) Nested Callable I/O Behavior
In standard-library/runtime notes, state that:
- `read`/`readline` are available from nested deffunction/defmethod/call-next-method frames (input buffer is propagated).

Reason: this was previously inconsistent and is now fixed.

## 6) Recursion Guard Default
In runtime configuration section, update default recursion-depth documentation to:
- `max_call_depth = 64` (default constructors)

Reason: implemented to ensure recursion-limit diagnostics occur before stack overflow under default test-thread stacks.

## Implications For Subsequent Phases
1. Phase 5 FFI/CLI can now freeze diagnostics for module/visibility/ambiguity behavior from corrected semantics.
2. Phase 6 compatibility docs should document qualified globals as `?*MODULE::name*` and bind non-creation semantics.
3. Regression suites for module namespace collisions and qualified-global paths should remain mandatory for future refactors.
