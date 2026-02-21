# Phase 4 Remediation Report

## Scope

This report evaluates Phase 4 implementation consistency against:

- `documents/FerricImplementationPlan.md`
- `documents/plans/phases/004/Plan.md`
- `documents/plans/phases/004/Notes.md`

"Consistent" here means either:

1. implemented as planned, or
2. intentionally diverged with a documented and non-problematic rationale.

## Executive Result

Phase 4 is **not yet in a fully consistent state**. Core features landed, but several material gaps remain in module/global semantics.

## Findings

| ID | Severity | Status | Summary |
|---|---|---|---|
| R4-01 | Critical | Open | Callable/global registries are keyed by unqualified name only; module namespaces are not modeled. |
| R4-02 | Critical | Open | `bind` does not enforce defglobal visibility/write rules, violating planned cross-module read/write contract. |
| R4-03 | High | Open | Module-qualified defglobal syntax is not parseable from source (`?*MODULE::name*`), so qualified global resolution is effectively unreachable. |
| R4-04 | Medium | Open | `read`/`readline` lose input context in deffunction/defmethod call frames (nested calls return `EOF`). |
| R4-05 | Medium | Open | Phase notes contain a few behavior mismatches versus code (reset run-loop semantics, coverage claims), reducing traceability. |

---

## Detailed Findings And Required Changes

### R4-01: Unqualified-Key Registries Break Module Namespace Semantics

**Evidence**

- `FunctionEnv` is `HashMap<String, UserFunction>` (`crates/ferric-runtime/src/functions.rs:32`).
- `GlobalStore` is `HashMap<String, Value>` (`crates/ferric-runtime/src/functions.rs:81`).
- `GenericRegistry` is `HashMap<String, GenericFunction>` (`crates/ferric-runtime/src/functions.rs:189`).
- Loader associates ownership using local-name keys (`crates/ferric-runtime/src/loader.rs:276`, `crates/ferric-runtime/src/loader.rs:291`, `crates/ferric-runtime/src/loader.rs:331`).
- Qualified dispatch still does `get(local_name)` against global registries (`crates/ferric-runtime/src/evaluator.rs:399`, `crates/ferric-runtime/src/evaluator.rs:432`).

**Observed behavior**

- Same-name `deffunction` across two modules overwrites by last registration.
- Same-name `defglobal`/`defgeneric` across modules are rejected as duplicates.
- `A::f`/`B::f` cannot both resolve correctly if both exist.

**Why this is inconsistent**

Phase 4’s module-qualified and visibility goals implicitly require module-scoped namespaces. Current behavior only works when names are globally unique.

**Remediation**

1. Re-key callable/global registries and ownership maps by `(ModuleId, String)`.
2. Add resolver helpers:
   - qualified: exact `(module, name)` lookup
   - unqualified: resolve from caller module using import/export visibility
3. Update conflict policy to be module-aware:
   - `deffunction` vs `defgeneric` conflict should be enforced per module namespace.
4. Add coverage for same local-name definitions in different modules.

---

### R4-02: `bind` Bypasses Global Visibility And Write Rules

**Evidence**

- `dispatch_bind` writes directly via `ctx.globals.set(&name, value)` with no visibility/ownership checks (`crates/ferric-runtime/src/evaluator.rs:1491`, `crates/ferric-runtime/src/evaluator.rs:1502`).

**Observed behavior**

- A rule in module `MAIN` can mutate a hidden global owned by `CONFIG` without import/export visibility.

**Why this is inconsistent**

`documents/FerricImplementationPlan.md` Phase 4 deliverables explicitly require cross-module `defglobal` **reads/writes** visibility enforcement.

**Remediation**

1. Route bind target resolution through the same visibility/ownership logic used by global reads.
2. Enforce unknown/not-visible diagnostics on writes.
3. Decide and document whether bind may create undeclared globals; if not, reject undeclared targets.
4. Add integration tests for:
   - visible cross-module write
   - not-visible write
   - unknown global write

---

### R4-03: Qualified Defglobal Syntax Is Not Reachable From Source

**Evidence**

- Global-var lexer accepts only `is_symbol_char` chars while scanning `?*...*` (`crates/ferric-parser/src/lexer.rs:288`, `crates/ferric-parser/src/lexer.rs:300`).
- `is_symbol_char` excludes `:` (`crates/ferric-parser/src/lexer.rs:464`).
- Evaluator has `resolve_qualified_global()` path (`crates/ferric-runtime/src/evaluator.rs:468`), but parser cannot emit qualified global variable names that reach it.

**Observed behavior**

- `?*CONFIG::g*` and `CONFIG::?*g*` produce parse/interpret errors.

**Why this is inconsistent**

Pass 004 claims module-qualified global lookup support; in practice, source syntax cannot exercise that path.

**Remediation**

1. Extend lexer/parser global-variable handling to preserve module-qualified names.
2. Normalize one canonical syntax form and document it.
3. Add parser tests + integration tests for qualified global success/failure cases.

---

### R4-04: Input Context Is Dropped In Nested Callable Frames

**Evidence**

- Deffunction body context sets `input_buffer: None` (`crates/ferric-runtime/src/evaluator.rs:633`).
- Defgeneric method body context sets `input_buffer: None` (`crates/ferric-runtime/src/evaluator.rs:914`).
- `call-next-method` context sets `input_buffer: None` (`crates/ferric-runtime/src/evaluator.rs:1020`).

**Observed behavior**

- `(read)` inside a deffunction returns `EOF` even when engine input is queued.

**Why this is inconsistent**

Phase 4 emphasizes function behavior parity across expression execution paths. Current behavior depends on call depth/context.

**Remediation**

1. Propagate input buffer through nested EvalContext construction.
2. If not supported intentionally, document this explicitly and gate it as deferred work.
3. Add tests for `read`/`readline` in deffunction and defmethod bodies.

---

### R4-05: Notes/Behavior Traceability Mismatches

**Evidence**

- Notes say `(reset)` from RHS "continues the run loop" (`documents/plans/phases/004/Notes.md:202`), but `run()` returns immediately after reset (`crates/ferric-runtime/src/engine.rs:533`).
- Notes and progress claim broad fixture/coverage completion; qualified-global coverage is absent.

**Why this matters**

This creates audit friction and can mislead follow-on phase planning.

**Remediation**

1. Correct Phase 4 notes to match actual behavior.
2. Add explicit "known limitations" list for any deferred semantics.
3. Align exit evidence with real test inventory.

---

## Recommended Execution Order

1. **R4-01 and R4-02 together** (shared resolver/namespace work).
2. **R4-03** (qualified-global syntax pipeline).
3. **R4-04** (I/O context propagation).
4. **R4-05** (documentation reconciliation, then re-run exit checklist).

## Exit Gate For Remediation Completion

Remediation is complete when all are true:

1. Same local-name constructs can coexist across modules and resolve correctly via qualified/unqualified paths.
2. Global reads and writes both enforce import/export visibility.
3. Qualified global references parse and execute with source-located diagnostics.
4. `read`/`readline` behavior is consistent (or explicitly documented as intentionally restricted).
5. Phase 4 notes, progress, and test evidence match actual runtime behavior.
