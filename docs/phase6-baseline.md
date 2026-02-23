# Phase 6 Baseline: Starting Contract and Verification Matrix

## Overview

This document records the Phase 6 starting point: what is locked from prior
phases, what remains for Phase 6 delivery, and the full supported CLIPS subset
as implemented.

---

## Locked from Phases 1-5

The following are stable contracts that Phase 6 must not break.

### Engine Semantics

All supported CLIPS constructs are implemented and tested:

- **defrule** -- forward-chaining rules with salience, ordered and template patterns.
- **deftemplate** -- slot-based fact schemas with default values.
- **deffacts** -- initial fact sets asserted on `(reset)`.
- **deffunction** -- user-defined functions with regular and wildcard parameters.
- **defglobal** -- module-scoped global variables with `?*name*` syntax.
- **defmodule** -- module system with `import`/`export` visibility, module-qualified `MODULE::name` syntax.
- **defgeneric / defmethod** -- generic function dispatch with CLIPS specificity ranking and `call-next-method`.
- **forall** -- desugared to `NCC([P, neg(Q)])` at loader level, with vacuous-truth semantics via `initial-fact`.

### Conditional Elements

- Ordered fact patterns (positional matching).
- Template fact patterns (slot-based matching with constraints).
- Variable binding across patterns.
- Constant, variable, and connective (`&`, `|`, `~`) constraints.
- **test CE** -- arbitrary Boolean guard expressions.
- **not CE** -- single-pattern negation via negative nodes with blocker tracking.
- **exists CE** -- existential quantification via support-counting memory.
- **forall CE** -- universal quantification via NCC subnetwork.
- **Negated conjunction** (NCC) -- `(not (and ...))` via NCC subnetwork nodes.

### Conflict Resolution Strategies

Four strategies are implemented and configurable: **Depth**, **Breadth**, **LEX**, **MEA**.

### RHS Actions

| Action | Notes |
|--------|-------|
| `assert` | Ordered and template facts |
| `retract` | By fact address variable |
| `modify` | Template-aware slot overrides |
| `duplicate` | Template-aware duplication |
| `printout` | Channel-based output (literal channel argument only) |
| `halt` | Stops rule execution |
| `focus` | Pushes modules onto the focus stack |
| `list-focus-stack` | Prints the current focus stack |
| `agenda` | Prints current agenda contents |
| `bind` | Variable binding (writes to existing bindings or globals; does not create new variables) |
| `run` | No-op from RHS (documented behavior) |
| `reset` | Deferred -- sets flag checked after action execution |
| `clear` | Deferred -- sets flag checked after action execution |

### Standard Library Functions

**Math:**
`+`, `-`, `*`, `/`, `div`, `mod`, `abs`, `min`, `max`

**Type Conversion:**
`integer`, `float`

**Comparison:**
`=`, `!=` / `<>`, `>`, `<`, `>=`, `<=`, `eq`, `neq`

**Logical:**
`and`, `or`, `not`

**Predicate / Type-checking:**
`integerp`, `floatp`, `numberp`, `symbolp`, `stringp`, `lexemep`, `multifieldp`, `evenp`, `oddp`

**String / Symbol:**
`str-cat`, `sym-cat`, `str-length`, `sub-string`

**Multifield:**
`create$`, `length$`, `nth$`, `member$`, `subsetp`

**I/O:**
`printout`, `format`, `read`, `readline`

**Agenda / Focus:**
`get-focus`, `get-focus-stack`

### FFI Surface (ferric-ffi)

C-ABI functions with opaque `FerricEngine*` handle:

**Lifecycle:**
- `ferric_engine_new` -- create engine with default config
- `ferric_engine_new_with_config` -- create engine with `FerricConfig` (encoding mode, conflict strategy)
- `ferric_engine_free` -- destroy engine

**Loading and Execution:**
- `ferric_engine_load_string` -- parse and compile CLIPS source
- `ferric_engine_reset` -- assert initial-fact and deffacts
- `ferric_engine_run` -- run with rule limit
- `ferric_engine_step` -- fire one rule

**Fact Management:**
- `ferric_engine_assert_string` -- assert fact from string, returns fact ID
- `ferric_engine_retract` -- retract by fact ID
- `ferric_engine_fact_count` -- number of facts in working memory
- `ferric_engine_get_fact_field_count` -- field count for a fact
- `ferric_engine_get_fact_field` -- retrieve a fact field value as `FerricValue`

**Globals:**
- `ferric_engine_get_global` -- read a defglobal value as `FerricValue`

**Output:**
- `ferric_engine_get_output` -- retrieve captured channel output

**Error Handling:**
- `ferric_last_error_global` -- thread-local error (borrowed pointer, valid until next Ferric call)
- `ferric_last_error_global_copy` -- thread-local error (copy-to-buffer)
- `ferric_clear_error_global` -- clear thread-local error
- `ferric_engine_last_error` -- per-engine error (borrowed pointer)
- `ferric_engine_last_error_copy` -- per-engine error (copy-to-buffer)
- `ferric_engine_clear_error` -- clear per-engine error

**Action Diagnostics:**
- `ferric_engine_action_diagnostic_count` -- number of action diagnostics after run/step
- `ferric_engine_action_diagnostic_copy` -- copy diagnostic message to buffer
- `ferric_engine_clear_action_diagnostics` -- clear accumulated diagnostics

**Value/Memory Management:**
- `ferric_string_free` -- free a Ferric-allocated C string
- `ferric_value_free` -- free a single `FerricValue`
- `ferric_value_array_free` -- free an array of `FerricValue`

**Thread Affinity Contract:**
- Engine instances are thread-affine (`!Send + !Sync`).
- Every `ferric_engine_*` entry point checks thread affinity before mutation.
- Mismatch returns `FERRIC_ERROR_THREAD_VIOLATION` with no state modified.
- Diagnostic read exceptions: `ferric_engine_last_error` and `ferric_engine_last_error_copy`.

**FFI Panic Policy:**
- Shipped FFI artifacts use `ffi-dev` / `ffi-release` profiles with `panic = "abort"`.
- No Rust unwind crosses the FFI boundary.

**Copy-to-Buffer Semantics (stable contract):**
- `out_len` is required.
- `buf = NULL` + `buf_len = 0` is the size-query path (returns `FERRIC_OK` with required size).
- Too-small buffers truncate to `buf_len - 1` bytes + NUL, return `FERRIC_ERROR_BUFFER_TOO_SMALL`.
- No error present: returns `FERRIC_ERROR_NOT_FOUND` before inspecting `buf`/`buf_len`.

### CLI Surface (ferric-cli)

- `ferric run [--json] <file>` -- load, reset, run, print output
- `ferric check [--json] <file>` -- load and validate without executing
- `ferric repl` -- interactive REPL
- `ferric version` / `--version` / `-V` -- print version information

**Exit Codes:**
- 0: Success
- 1: Runtime/load error
- 2: Usage error

**`--json` mode** emits structured JSON diagnostics on stderr:
```json
{"command":"run","level":"error","kind":"load_error","message":"..."}
```
Fields: `command`, `level` (`error` | `warning`), `kind`, `message`.

### Engine Configuration

- `EngineConfig` with encoding mode (`Ascii`, `Utf8`, `AsciiSymbolsUtf8Strings`).
- Conflict resolution strategy (`Depth`, `Breadth`, `Lex`, `Mea`).
- Classic vs. Strict error modes (unsupported constructs always fail compilation in both modes).

---

## What Remains for Phase 6

### Compatibility Testing (Passes 002-006)

- CLIPS compatibility test suites in `tests/clips_compat/` covering core execution semantics.
- Language, module, and stdlib semantic compatibility coverage.
- External-surface contract lock suites for FFI (canonical `ferric_engine_*` naming, configured construction, thread-affinity, copy-to-buffer, fact-ID round trips, action diagnostics).
- CLI `--json` diagnostics contract regression tests.

### Benchmarking (Passes 007-011)

- Benchmark harness with `criterion` in `benches/`.
- Canonical workloads: Waltz line-labeling, Manners seating.
- Targeted microbenchmarks for hot paths.
- Performance profiling and budget gap analysis.
- CI benchmark gates progressing from advisory to blocking.

### Documentation (Pass 012)

- `docs/compatibility.md` filled out for Sections 16.1-16.8.
- String comparison semantics documentation (byte-equality, no normalization).
- Pattern nesting restriction rationale.
- FFI embedding and wrapper guidance.
- Machine-readable CLI diagnostics contract examples.
- User-facing examples and migration guidance.

### Integration and Release Readiness (Pass 013)

- Final release readiness validation.
- All quality gates clean with benchmark/compatibility evidence.

---

## Current Test Baseline

Total tests passing: **1223** across the workspace (1220 unit/integration + 3 doc-tests).

| Crate | Tests |
|-------|-------|
| ferric (facade) | 1 |
| ferric-cli | 4 |
| ferric-core | 10 |
| ferric-ffi | 27 |
| ferric-parser | 273 |
| ferric-runtime (lib) | 127 (+4 ignored) |
| ferric-runtime (integration) | 176 |
| ferric (integration) | 602 |
| Doc-tests | 3 |

---

## Verification Command Matrix

| Command | Purpose | Gate Level |
|---------|---------|------------|
| `cargo test --workspace` | Full test suite | Blocking |
| `cargo check --workspace --all-targets` | Compilation check | Blocking |
| `cargo fmt --all -- --check` | Formatting | Blocking |
| `cargo clippy --workspace --all-targets -D warnings` | Lint | Blocking |
| `cargo test -p ferric --test clips_compat` | Compatibility suite | Blocking (Phase 6) |
| `cargo bench -p ferric` | Benchmark suite | Advisory -> Blocking |

---

## Unsupported Features (Explicitly Out of Scope)

These are documented in the implementation plan Appendix A and are not planned:

- COOL object system
- Certainty factors / probabilistic reasoning
- Distributed or networked rule evaluation
- Replay-identical deterministic scheduling across runs/platforms
- Conflict strategies `Simplicity`, `Complexity`, `Random`
- Triple-nested negation, `exists(not ...)`, nested `forall`
- `if`/`then`/`else` expression form (not yet implemented)
