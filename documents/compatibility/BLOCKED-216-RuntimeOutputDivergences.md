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
Current observed behavior (2026-02-27):

1. `globltst.clp` now runs through to completion (`Test Completed - No Errors`).
- The prior blockers in this slice were addressed:
  - evaluator-side `printout` support for deffunction/method bodies,
  - deffacts global value resolution,
  - RHS local `bind` rebinding support,
  - ordered multifield splicing/capture corrections used by this fixture.

2. `co-drtest03-03.clp` now runs without the previous `bind` warning.
- `subseq$` callability and RHS local `bind` rebinding now both work in this representative slice.
- Remaining `drtest03-*` differences are now outside the original `bind` blocker (for example stricter template-slot validation in `co-drtest03-08.clp` for unknown slot `three`).

3. `co-dfrulcmd-03.clp` is partially aligned.
- `rules` is callable from RHS and prints loaded rule names.
- `undefrule` now mutates runtime rule state from RHS:
  - removes targeted rule metadata (`*`, unqualified, qualified selectors),
  - clears queued activations for removed rules,
  - prevents removed rules from creating new activations.
- Runtime `load` now mutates the live engine state from RHS:
  - invokes the normal file loader during rule execution,
  - newly loaded rules are visible to subsequent `(rules)` calls in the same RHS,
  - load failures surface as non-fatal action diagnostics.

4. Related command surface remains partially missing.
- `ppdefrule` now prints stored rule definitions from RHS (`name`, `*`, qualified/module selectors).
- `load` in evaluator-only expression contexts (outside RHS action dispatch) still uses placeholder return semantics.

## Root Cause For Ferric Divergence
This is a runtime callable-surface split problem plus missing mutable rule-management features.

1. Runtime callable surface is split by execution context.
- RHS action dispatch now supports mutable `load`, but evaluator-only paths still keep compatibility placeholders for mutation commands.

2. Runtime `load` during active execution still needs policy hardening.
- Current behavior re-enters the loader directly; recursion-depth limits and stricter cycle guards are future hardening work.

3. Strict callable validation still surfaces the missing mutation semantics.
- This is desirable for early diagnostics, but it means command names can parse/load while still diverging behaviorally until mutation semantics are implemented.

## High-Level Sketch Of Required Changes

1. Unify callable capabilities by context.
- Define callable categories: pure expression builtins, output side-effect builtins, engine-mutation commands.
- Make compatibility explicit instead of implicit by code location.

2. Fill expression builtin gaps first.
- Implement missing non-mutating builtins (`subseq$` and any required companions).

3. Bridge output side effects into evaluator context safely.
- Add output sink capability to evaluator contexts so deffunction/method bodies can use `printout` behavior.

4. Add rule introspection/mutation commands in controlled phases.
- Read-only (`rules`), mutation (`undefrule`), and rule pretty-print (`ppdefrule`) are now in place.

5. Runtime `load` is now wired for RHS actions.
- Remaining work is hardening/policy, not basic command availability.

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

### Pass 5: `undefrule` Command (Mutation) [Completed]
Goal: support runtime rule removal safely.

Implemented:
- Add RHS `undefrule` action handling for `*`, unqualified names, qualified names, and `MODULE::` module selectors.
- Add safe runtime mutation updates across:
  - compiled rule metadata,
  - rule-module bookkeeping,
  - agenda activations (purged immediately),
  - future activation creation (disabled-rule guard in rete terminal propagation).

Validation:
- `cargo test -p ferric-runtime`:
  - `undefrule_star_removes_rules_and_cancels_pending_activations`
  - `undefrule_by_name_removes_targeted_rule_before_it_fires`
- `cargo test -p ferric-core`:
  - `agenda::tests::remove_activations_for_rule_removes_all_matching_entries`
  - `rete::tests::disable_rule_removes_existing_activations_and_blocks_new_ones`

Expected end-of-pass state:
- Core `dfrulcmd` mutation behavior available.

### Pass 6: Runtime `load` Command [Completed]
Goal: support `load` from RHS with predictable safety constraints.

Implemented:
- Add RHS `load` action handling that:
  - evaluates a string/symbol path argument,
  - re-enters `Engine::load_file` during action execution,
  - restores the caller module context after load,
  - reports load failures through action diagnostics.
- Route action execution through a mutable engine context so runtime loader mutation and subsequent RHS commands (`rules`) observe the same live state.

Validation:
- `cargo test -p ferric-runtime`:
  - `runtime_load_mutates_rule_set_and_rules_output`
  - `runtime_load_missing_file_surfaces_action_diagnostic`
- CLI smoke with `Temp/foo.tmp` fixture:
  - `co-dfrulcmd-03.clp`, `t64x-dfrulcmd-03.clp`, `t65x-dfrulcmd-03.clp`
  - post-`load` `(rules)` now prints `foo1 foo2 foo3`.

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

1. Real CLIPS workflows are substantially less blocked.
- Users depending on RHS `rules`/`undefrule`/`ppdefrule`/`load` admin idioms can now execute the core pattern directly.

2. Runtime dynamic load workflows still have hardening gaps.
- Basic mutation works, but recursion/cycle policy is not yet as explicit as desired.

3. Evaluator-only mutation call sites remain intentionally conservative.
- Calls to mutation commands through pure expression-eval paths still return compatibility placeholders.

4. Migration confidence is still reduced.
- Compatibility failures cluster around runtime operability, which users feel immediately in end-to-end runs.

Plausible blocked scenarios and workarounds:
- Scenario: a user uses CLIPS runtime admin rules that self-manage rule sets via `rules`/`undefrule`.
  - Likely outcome today: works for direct RHS command usage, including runtime `load`.
  - Remaining downside: edge-case policy differences around recursive/nested loads may still diverge.

- Scenario: a user depends on dynamic runtime `(load "...")` orchestration inside RHS admin rules.
  - Likely outcome today: live mutations apply and subsequent RHS introspection sees loaded rules.
  - Remaining downside: missing-file and invalid-load cases surface as Ferric action diagnostics rather than CLIPS-identical text.
