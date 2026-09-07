# Migrating from CLIPS to Ferric

This guide covers practical steps for migrating existing CLIPS applications
to Ferric. For detailed feature compatibility, see [compatibility.md](compatibility.md).

---

## Pre-1.0 seed and reset changes

Loading `deffacts` now registers a named definition without asserting its facts.
Call `reset()` after loading startup rules and seeds, or use Rust
`Engine::with_rules`, which already loads and resets. A later load leaves
existing application facts unchanged until the next reset. `LoadResult` no
longer reports deffacts seeds as newly asserted facts. Explicit `assert` and
`load-facts` continue to add facts immediately.

A definition is identified by module and local name. Successful replacement
moves it to the end of that module's definition order; reset visits modules
in creation order, then their definitions in order. `undeffacts` removes
definitions without retracting current facts; its `*` selector applies only to
the current module. An invalid individual definition
leaves its previous definition intact. This atomic replacement is deliberately
stronger than CLIPS 6.30, which removes the old same-name definition before
reporting some replacement errors.

The internal `(initial-fact)` remains matchable by rules but is hidden from
host fact lookup/enumeration and protected against retract, modify, and
duplicate. Replacing `MAIN::initial-fact` with a user `deffacts` is rejected.
These restrictions differ from CLIPS; keep application bootstrap state in
ordinary named facts. Empty and leading-negative rule conditions use the
independent RETE root token. Reset establishes that root and the internal
initial fact before asserting application seeds.

Core-internal consumers of `AlphaMemory::lookup_by_slot` now receive an
iterator in insertion order instead of a borrowed hash set. Collect that
iterator when a materialized collection is needed. Public engine fact APIs
retain their existing result types.

## Step 1: Check Feature Coverage

Review your CLIPS codebase for features that Ferric does not support:

**Not supported (will not compile):**
- COOL object system (`defclass`, `definstances`, `defmessage-handler`,
  `send`, `make-instance`)
- Certainty factors
- Conflict strategies: `simplicity`, `complexity`, `random`

**Partially supported:**
- Pattern nesting: single-level `not`, `exists`, `forall`, and NCC are
  supported. Triple-nested negation, `(exists (not ...))`, and nested
  `(forall ...)` are not.

If your rules use only `defrule`, `deftemplate`, `deffacts`, `deffunction`,
`defglobal`, `defmodule`, `defgeneric`, and `defmethod` with standard
library functions, you are likely in the supported subset.

## Step 2: Validate with ferric check

Use the CLI to validate your files without executing:

```bash
ferric check rules.clp
```

Or with machine-readable output for CI integration:

```bash
ferric check --json rules.clp 2> errors.json
```

Fix any reported parse or compilation errors before proceeding.

## Step 3: Review Rule Patterns and Actions

### Replace nested negation

```clp
;; CLIPS (unsupported triple nesting)
(not (not (not (condition))))

;; Ferric: use an intermediate fact
(defrule detect-condition
    (condition) => (assert (condition-present)))
(defrule no-condition
    (not (condition-present)) => ...)
```

### Use if/then/else for RHS actions

Conditional rule actions use the ordinary CLIPS `if`/`then`/`else` form:

```clp
;; Supported in Ferric
(defrule classify
    (value ?x)
    =>
    (if (> ?x 10) then (printout t "big") else (printout t "small")))
```

To select matching rules with the condition instead, use a `test` CE:

```clp
(defrule classify-big
    (value ?x) (test (> ?x 10)) => (printout t "big" crlf))
(defrule classify-small
    (value ?x) (test (<= ?x 10)) => (printout t "small" crlf))
```

### Replace (exists (not ...))

```clp
;; CLIPS (unsupported nesting)
(exists (not (done ?x)))

;; Ferric: use a helper rule
(defrule find-undone
    (item ?x) (not (done ?x))
    => (assert (has-undone-item)))

(defrule process-undone
    (has-undone-item) => ...)
```

## Step 4: Review format Usage

In Ferric, `format` returns a string and does not write to a router
directly. Adjust calls accordingly:

```clp
;; CLIPS
(format t "value=%d" 42)

;; Ferric
(printout t (format nil "value=%d" 42) crlf)
```

## Step 5: Understand Comparison Semantics

Ferric implements two distinct equality operators:

- `=` performs **numeric** equality. It coerces types: `(= 1 1.0)` is TRUE.
- `eq` performs **value** equality. It is type-sensitive: `(eq 1 1.0)` is FALSE.

This matches CLIPS semantics, but is a common source of bugs when migrating.
Use `=` for numeric comparisons and `eq` when you need exact type+value matching
(e.g., comparing symbols or strings).

## Step 6: Move Fact Mutation Out of Functions

In Ferric, `deffunction` and `defmethod` bodies are evaluator expressions, not
full RHS action lists. They can call expression functions such as `str-cat`,
`format`, and `printout`, but fact mutation and agenda/focus control belong in
the calling rule's RHS:

```clp
;; CLIPS (fact mutation inside deffunction)
(deffunction record-and-double (?x)
    (assert (saw ?x))
    (* ?x 2))

;; Ferric: split into expression + RHS action
(deffunction double (?x) (* ?x 2))

(defrule compute
    (value ?x)
    =>
    (assert (saw ?x))
    (printout t (double ?x) crlf))
```

Also note: `(run)` called from a rule's RHS is a documented no-op in Ferric.
Use `(reset)` and `(clear)` from RHS with care -- they are deferred and take
effect after the current action sequence completes.

## Step 7: Review String Handling

Ferric uses byte-equality comparison with no Unicode normalization:

- ASCII content: behavior identical to CLIPS.
- Non-ASCII content: ensure inputs are normalized to a consistent form
  (e.g., NFC) before asserting.
- `sub-string` uses byte indices. For ASCII, this is identical to CLIPS
  character indices.

## Step 8: Test Incrementally

1. Start with `ferric check` to validate syntax.
2. Run with `ferric run` and compare output to CLIPS.
3. Compare working-memory state, firing counts and observable output. Supported
   depth/breadth ordering follows activation creation chronology; LEX/MEA remain
   experimental and have documented CLIPS differences.
4. Use `(declare (salience ...))` and `(focus ...)` to enforce ordering
   where side-effect order matters.

## Step 9: Embed via FFI (Optional)

If your application embeds CLIPS via its C API, Ferric provides a similar
C FFI surface. Key differences:

- Raw engines may move between OS threads; the host must serialize runtime
  calls and protect borrowed-pointer use and destruction. Per-engine error
  copies are separately synchronized. Both free entry points now have the same
  lifetime contract. The legacy thread-violation error discriminant is retained.
- Error handling uses return codes plus synchronized error channels. A failure
  involving a validated raw-engine handle updates both its per-engine snapshot
  and the calling thread's global fallback; pre-handle failures update only
  global state.
- Prefer `ferric_engine_get_output_copy` for captured output. The legacy
  `ferric_engine_get_output` pointer is an engine-owned, per-channel snapshot:
  another engine cannot invalidate it, but a later borrowed read for the same
  engine/channel can replace it; output clear/reset operations and engine
  destruction invalidate the relevant snapshot.
- Include `ferric.h` and link against the Ferric shared library.

See [compatibility.md](compatibility.md) Section 16.13 for the full FFI
contract.

Python `Engine` ordinary operations now serialize across threads. Code that
previously expected a `wrong thread` exception should use the normal result
or operation error; a closed engine reports `engine has been closed` on every
thread. `halt()` still cancels only an active run, and `close()` remains an
idempotent destruction barrier. Same-engine reentry during Python conversion
is explicitly rejected. This pre-1.0 change removes the creator-thread
requirement without adding an asynchronous operation queue.

Go `Engine` now serializes its complete native operation and diagnostic-copy
window internally, so callers may use it from different goroutines without a
lifetime `runtime.LockOSThread`. Constructors pin temporarily for thread-local
error retrieval. Raw `Halt` queues behind an active operation; use a cancelable
run context or `PinnedEngine.Halt` for active-run cancellation. Existing worker
queues and pool ownership rules remain in force, including not retaining an
engine borrowed inside a manager callback.

---

## Common Gotchas

| Gotcha | Detail |
|--------|--------|
| `=` vs `eq` | `=` is numeric (coerces types); `eq` is value+type sensitive |
| `format` writes nowhere | `format` returns a string; use `(printout t (format nil ...) crlf)` |
| `sub-string` byte indices | Byte-based, not codepoint-based; identical for ASCII |
| Function bodies are evaluator expressions | Put fact mutation and agenda/focus control in rule RHS code |
| `run` from RHS is a no-op | `(run)` inside a rule action does nothing |
| `reset`/`clear` are deferred | Flag is set and checked after the current action sequence completes |
| LHS guards belong in patterns/tests | Use RHS `if/then/else` for action control; use `(test ...)` CEs for match-time guards |
| Activation order | Depth/breadth follow activation creation chronology; LEX/MEA are experimental |

---

## Quick Reference: CLIPS to Ferric

| CLIPS Feature | Ferric Status |
|---------------|---------------|
| `defrule` | Supported |
| `deftemplate` | Supported |
| `deffacts` | Supported |
| `deffunction` | Supported (evaluator expressions; no fact mutation/control actions) |
| `defglobal` | Supported |
| `defmodule` | Supported |
| `defgeneric` / `defmethod` | Supported (evaluator expressions; no fact mutation/control actions) |
| `assert` / `retract` / `modify` / `duplicate` | Supported |
| `printout` / `format` / `read` / `readline` | Supported (format is expression-only) |
| `not` / `exists` / `forall` / `test` | Supported (single-level nesting) |
| Salience | Supported |
| Focus stack | Supported |
| Depth / Breadth | Supported |
| LEX / MEA | Experimental; documented CLIPS ordering differences |
| `defclass` / COOL | Not supported |
| `if` / `then` / `else` | Supported for conditional RHS actions |
| Certainty factors | Not supported |

## Primitive template slot types

Ferric now retains `(type ...)` declarations for `SYMBOL`, `STRING`, `INTEGER`,
`FLOAT`, `NUMBER`, `LEXEME`, and `EXTERNAL-ADDRESS`. Type lists form a union;
`NUMBER` means integer or float, and `LEXEME` means symbol or string. Omitted
constraints and `(type ?VARIABLE)` permit any supported value kind. Each field
of a constrained multislot must satisfy its declared union.

Defaults follow CLIPS' primitive preference: symbol `nil`, empty string,
integer `0`, then float `0.0`, independent of the type list's spelling order.
Multislot defaults are empty unless specified. Literal multifield defaults,
including literal `create$` forms, retain every field. External-address slots
require `(default ?NONE)`; Ferric does not manufacture host identity tokens.

Invalid literal assertions reject a rule before installation or replacement.
Defaults and named deffacts are checked before registration. Runtime assertions,
`modify`, `duplicate`, and host template assertions also validate types before
changing facts. This runtime checking is intentionally stricter than CLIPS
6.30's default `FALSE` dynamic-constraint setting: applications must supply
values matching their declarations. A failed `modify` leaves the original fact
intact; a failed RHS action produces an action diagnostic and stops that RHS.

Previously ignored `range`, `allowed-*`, `cardinality`, `default-dynamic`, and
other optional slot attributes now produce an explicit unsupported error.
Arbitrary computed defaults are also unsupported; use literal defaults or
`?DERIVE`, and calculate dynamic values before assertion. `FACT-ADDRESS` and
instance type declarations are rejected because the supported value model has
no corresponding tagged value. These restrictions do not add CLIPS class
constraints, general static type inference, or dynamic constraint toggles.

## September 2026 embedding API changes

- Rust assertions accept engine-scoped host values and opaque `FactHandle`s.
  Use `engine.symbol_value`, `HostValue::multifield`, named template slots, and
  `()` for empty fields. Raw core symbols cannot be used as portable input.
  Re-query fact handles after reset or restore; persist application IDs in facts.
  See [host-api.md](host-api.md).
- Snapshots use a bounded version-one envelope; CBOR is recommended and is the
  default for CLI, TypeScript, Python and Swift consumers. Legacy unversioned
  snapshots are rejected explicitly. Export durable application data through
  the producing version before upgrading; see [snapshots.md](snapshots.md).
- Python plain `str` now means a CLIPS string. Use `ferric.Symbol` for symbols.
  Typed strings and symbols compare distinctly from each other and plain strings.
  Python `None`, Node `null`, and Swift `.void` cannot be stored in facts.
  Use an explicit application sentinel instead. Unsupported external values
  produce errors rather than null conversions. Owned Python fact values remain
  readable after closing their source engine and can be asserted into another
  engine, which interns symbol text into its own symbol table.
- Node requires version 22 or newer. Integers and fact IDs use `bigint` when they
  exceed safe-number precision; run counts accept exact safe integers. Closing a
  worker, handle or pool waits for supported native work and cleanup to finish.
- Requested callable depth remains configurable and persists, while actual
  evaluation is capped at 32 calls and 64 expression frames. Excessive recursive
  work returns an action diagnostic; deeply nested definitions are rejected.
- Go source imports use `github.com/plx/ferric-rules/bindings/go`. Raw operations
  serialize across goroutines, while worker APIs retain offload and cancellation.
  Swift's local package uses Swift 6, macOS 15 or iOS 18, with asynchronous native
  work and owned results; see [its build instructions](../bindings/swift/README.md).
