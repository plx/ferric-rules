BLOCKED: # 216 Runtime Output Divergences (globltst, drtest03, dfrulcmd)

## Scope And Intent
This document describes the remaining runtime-feature compatibility gaps that cause output divergence (or load-time rejection) in `globltst`, `drtest03-*`, and `dfrulcmd-*` families.

## CLIPS Behavior (Reference)
CLIPS supports all of the following in the affected scenarios:

1. Deffunction/method bodies can call standard output and expression functions.
- `printout` can be invoked from user-defined function bodies.

2. Multifield utility builtins are available in expression contexts.
- Functions like `subseq$` are callable inside `bind` and other RHS expressions.

3. Runtime rule introspection/mutation commands are available from rule actions.
- `rules`, `undefrule`, `ppdefrule`, and related commands are callable and affect runtime state.

4. Dynamic loading commands can be invoked from rule actions.
- `(load "...")` can be used as a command in knowledge-base workflows.

## Incompatible Ferric Behavior
Current observed behavior (2026-02-25):

1. `globltst.clp` diverges with action warnings.
- `deffunction printem` fails internally because `printout` is not available in the evaluator callable set used by function bodies.
- This leads to partial output and test ending with errors.

2. `co-drtest03-03.clp` fails at load.
- `[EXPRNPSR3] Missing function declaration for subseq$ ...`

3. `co-dfrulcmd-03.clp` fails at load.
- `[EXPRNPSR3] Missing function declaration for rules ...`

4. Related command surface remains missing.
- `undefrule`, `load`, `ppdefrule` command semantics are not fully implemented for RHS execution contexts.

## Root Cause For Ferric Divergence
This is a runtime callable-surface split problem plus missing mutable rule-management features.

1. Split dispatch surfaces (evaluator vs action executor).
- `printout` exists as an action command path in `actions.rs`.
- Deffunction bodies run through evaluator call dispatch (`evaluator.rs`) where `printout` is absent.

2. Missing builtin implementations.
- `subseq$` and some related compatibility functions are not present in evaluator builtin dispatch.

3. Strict load-time callable validation now surfaces missing runtime commands earlier.
- After item 006, unresolved callables fail at load with `[EXPRNPSR3]` rather than deferred runtime warnings.
- This is desirable in general, but it exposes unimplemented command surface directly.

4. Rule graph mutation/introspection APIs are incomplete.
- `rules` may be implementable as read-only reporting.
- `undefrule` requires safe removal/update of compiled rule structures and agenda interactions.

5. Runtime `load` command requires execution-time loader entry and policy controls.
- Needs path handling, recursion safeguards, and deterministic semantics while engine is running.

## High-Level Sketch Of Required Changes

1. Unify callable capabilities by context.
- Define callable categories: pure expression builtins, output side-effect builtins, engine-mutation commands.
- Make compatibility explicit instead of implicit by code location.

2. Fill expression builtin gaps first.
- Implement missing non-mutating builtins (`subseq$` and any required companions).

3. Bridge output side effects into evaluator context safely.
- Add output sink capability to evaluator contexts so deffunction/method bodies can use `printout` behavior.

4. Add rule introspection/mutation commands in controlled phases.
- Start read-only (`rules`), then mutation (`undefrule`) with strong invariants.

5. Add runtime `load` only after reentrancy and state-safety strategy is defined.

## Tentative Implementation Plan (Session-Sized Passes)

### Pass 1: Baseline Repro And Guardrail Tests
Goal: lock current divergence signatures before changes.

Changes:
- Add focused runtime/CLI tests for:
  - `globltst` deffunction `printout` path,
  - `subseq$` call in `drtest03-03`-like fixture,
  - `rules`/`undefrule` command expectations in reduced fixture.

Validation:
- `cargo test -p ferric-runtime` targeted tests.
- CLI smoke checks for representative fixtures.

Expected end-of-pass state:
- No behavior changes, reliable regression harness in place.

### Pass 2: Implement `subseq$` (Expression Builtin)
Goal: remove hard load-time failure for missing multifield helper.

Changes:
- Add evaluator builtin implementation for `subseq$` with CLIPS indexing semantics.
- Add arity/type checks consistent with current builtin style.
- Register callable in builtin callable list used by load-time validation.

Validation:
- Unit tests for normal, boundary, and out-of-range cases.
- Re-run `co-drtest03-03` representative check.

Expected end-of-pass state:
- `subseq$` no longer blocks load.

### Pass 3: Deffunction/Method-Side `printout` Support
Goal: allow output from deffunction/method bodies.

Changes:
- Extend evaluator context with output capability (for example an output sink/router handle).
- Implement evaluator-level `printout` dispatch path compatible with existing formatting behavior.
- Thread output capability through function/generic dispatch contexts.

Validation:
- Unit tests for `printout` inside deffunction and method bodies.
- Re-run `globltst` representative.

Expected end-of-pass state:
- `printem`-style deffunctions can emit output correctly.

### Pass 4: `rules` Command (Read-Only Introspection)
Goal: support non-mutating rule listing command used by compatibility fixtures.

Changes:
- Add `rules` action command implementation in RHS execution path.
- Add command to load-time action callable allowlist.
- Define deterministic output format (as close to CLIPS as practical).

Validation:
- Integration tests for rule listing in simple and multi-module cases.

Expected end-of-pass state:
- `rules` no longer blocks load; basic listing works.

### Pass 5: `undefrule` Command (Mutation)
Goal: support runtime rule removal safely.

Changes:
- Add engine API to remove rules by name and wildcard (`*`) with safe updates to:
  - compiled rule metadata,
  - rule-module bookkeeping,
  - agenda/activation references.
- Add command path in action executor and callable validation.

Validation:
- Tests for remove-one, remove-all, remove-nonexistent, and post-removal run behavior.

Expected end-of-pass state:
- Core `dfrulcmd` mutation behavior available.

### Pass 6: Runtime `load` Command
Goal: support `load` from RHS with predictable safety constraints.

Changes:
- Add runtime load action command invoking loader with policy controls:
  - path resolution rules,
  - recursion/reentrancy protections,
  - bounded error reporting semantics.

Validation:
- Integration tests for successful nested load and controlled failure cases.

Expected end-of-pass state:
- `load` command available for compatibility workflows.

### Pass 7: Compatibility Sweep And Stabilization
Goal: close the loop against affected fixture set.

Changes:
- Re-run `globltst`, `drtest03-*`, `dfrulcmd-*` families.
- Triage residual formatting/ordering differences.
- Document any intentionally deferred edge behavior.

Validation:
- Compatibility report delta and targeted fixture checks.

Expected end-of-pass state:
- Gap either closed or reduced with explicit residuals.

## Collateral Compatibility Damage Risks

1. Engine-state integrity risk from `undefrule`/`load`.
- Runtime mutation while running can invalidate assumptions in agenda/rete bookkeeping if not handled atomically.

2. Reentrancy and recursion hazards.
- `load` during rule execution can create nested compile/run interactions that are hard to reason about.

3. Output-path coupling risk.
- Introducing evaluator-side `printout` may duplicate or conflict with existing action-side formatting/output routing.

4. Behavioral drift in command formatting.
- Even when commands exist, output text/order differences can create new compatibility mismatches.

5. Performance risk.
- Additional callable/context plumbing and runtime mutation guards can add overhead on hot paths.

## Cost Of Doing Nothing

1. Real CLIPS workflows remain blocked.
- Users depending on `rules`/`undefrule`/`load` command idioms cannot run those scripts directly.

2. Deffunction output patterns remain broken.
- Existing CLIPS knowledge bases that encapsulate output logic in deffunctions produce incorrect or partial output.

3. Multifield-heavy scripts remain non-portable.
- Missing `subseq$` blocks some test suites and user rule bases at load time.

4. Migration confidence is reduced.
- Compatibility failures cluster around runtime operability, which users feel immediately in end-to-end runs.

Plausible blocked scenarios and workarounds:
- Scenario: a user uses CLIPS runtime admin rules that self-manage rule sets via `rules`/`undefrule`.
  - Likely outcome today: load-time failure.
  - Workaround: external orchestration code mutating engine state via host API.
  - Practical downside: significant rewrite, loss of in-language self-management.

- Scenario: a user has reusable deffunctions for reporting/logging using `printout`.
  - Likely outcome today: runtime warnings and missing output.
  - Workaround: move output calls back into each rule RHS.
  - Practical downside: duplicated logic, poorer maintainability.

- Scenario: a rule base uses `subseq$` to manipulate multifields in RHS expressions.
  - Likely outcome today: load-time failure.
  - Workaround: rewrite with slower/manual `nth$` loops where possible.
  - Practical downside: complex rewrites and uncertain semantic equivalence.
