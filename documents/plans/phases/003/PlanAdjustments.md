# Phase 003 Plan Adjustments

## Purpose

This document lists updates needed in `/Users/prb/github/ferric-rules/documents/FerricImplementationPlan.md` so the master plan accurately reflects Phase 3 outcomes and deferrals.

## Required Master-Plan Adjustments

### A1. Clarify `forall` sign-off criteria in Section 7.5

Add explicit statement that Phase 3 sign-off requires the **full** 6-step `forall_vacuous_truth_and_retraction_cycle` regression flow (including unsatisfied/re-satisfied transitions after assert/retract), not only vacuous-truth-at-empty-state checks.

Also document current implementation detail: hidden `(initial-fact)` support is used to enable standalone negation/forall activation behavior.

### A2. Reconcile focus API contract with implemented runtime

The master plan currently specifies `set_focus` and agenda query `get-focus`. Phase 3 runtime currently behaves as a focus-stack system (`push_focus` + current focus semantics).

Choose one and reflect it in the plan:

1. Keep original contract: require remediation to add `set_focus` and `get-focus`, or
2. Update API spec to stack-first semantics and move query helpers (`get-focus-stack`, etc.) into explicit Phase 4 work.

### A3. Make module-resolution scope explicit for Phase 3 vs Phase 4

Update Phase 3 defmodule language to state what is actually complete now:

- Focus-driven module execution semantics are implemented.
- Template visibility checks are implemented.
- Cross-module visibility/resolution for functions/globals and module-qualified names (`MODULE::name`) are deferred.

Then add explicit Phase 4 entry tasks for those deferred pieces.

### A4. Document generic dispatch semantics delivered in Phase 3

Add a note under generic/method semantics:

- Method selection is deterministic by method index order.
- Auto-indexing is registration-order-based (not full CLIPS specificity ranking yet).
- Full CLIPS-style specificity ranking and `call-next-method` remain deferred.

### A5. Clarify name-collision policy (`deffunction` vs `defgeneric`)

Current behavior allows precedence/shadowing rather than a hard definition-time conflict.

Master plan should explicitly define target policy:
- either enforce CLIPS-like conflict errors, or
- codify precedence semantics and compatibility implications.

### A6. Clarify `printout` channel contract

Plan/docs should specify one consistent rule:
- channel must be literal symbol only, or
- channel may be expression-evaluated.

Current docs and runtime behavior are not aligned.

### A7. Add explicit duplicate-definition diagnostics requirement

Pass-level docs require duplicate diagnostics, but this should also be visible in master plan validation requirements for construct interpretation/loading:
- duplicate globals
- duplicate modules
- duplicate generics
- duplicate methods / method indices

### A8. Extend consistency-check expectations for new registries

Update consistency/invariant language to include Phase 3 state holders:
- module registry/focus stack
- function registry
- global store
- generic registry

This avoids a mismatch between rete-only invariant checks and broader runtime state added in Phase 3.

## Suggested Placement in Master Plan

1. Section 7.5 (`forall`): apply A1.
2. Engine API / module sections: apply A2 and A3.
3. Function/generic sections: apply A4, A5, A7.
4. I/O section (`printout`): apply A6.
5. Invariants/debug-consistency sections: apply A8.

## Impact on Subsequent Phases

If the above adjustments are accepted, Phase 4 should explicitly include:

1. module-qualified name resolution and cross-module function/global visibility,
2. finalized generic specificity/`call-next-method` behavior,
3. finalized focus API/query surface,
4. duplicate-definition diagnostics completion (if not remediated in Phase 3 cleanup),
5. harmonized `printout` channel semantics and compatibility notes.
