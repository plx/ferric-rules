# Migrating from CLIPS to Ferric

This guide covers practical steps for migrating existing CLIPS applications
to Ferric. For detailed feature compatibility, see [compatibility.md](compatibility.md).

---

## Step 1: Check Feature Coverage

Review your CLIPS codebase for features that Ferric does not support:

**Not supported (will not compile):**
- COOL object system (`defclass`, `definstances`, `defmessage-handler`,
  `send`, `make-instance`)
- Certainty factors
- `if`/`then`/`else` expression form (use conditional patterns instead)
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

## Step 3: Adjust Unsupported Patterns

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

### Replace if/then/else

```clp
;; CLIPS
(defrule classify
    (value ?x)
    =>
    (if (> ?x 10) then (printout t "big") else (printout t "small")))

;; Ferric: use separate rules with test CE
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

## Step 6: Move Side Effects Out of Functions

In Ferric, `deffunction` and `defmethod` bodies are **pure expressions**. They
cannot execute side-effect actions like `assert`, `retract`, or `printout`
directly. If your CLIPS code uses side effects inside functions, move them to
the calling rule's RHS:

```clp
;; CLIPS (side effect inside deffunction)
(deffunction log-and-double (?x)
    (printout t "doubling " ?x crlf)
    (* ?x 2))

;; Ferric: split into expression + RHS action
(deffunction double (?x) (* ?x 2))

(defrule compute
    (value ?x)
    =>
    (printout t "doubling " ?x crlf)
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
3. Focus on final working-memory state rather than firing order -- Ferric
   guarantees a total order within a run but not replay-identical order
   across runs.
4. Use `(declare (salience ...))` and `(focus ...)` to enforce ordering
   where side-effect order matters.

## Step 9: Embed via FFI (Optional)

If your application embeds CLIPS via its C API, Ferric provides a similar
C FFI surface. Key differences:

- Engine handles are thread-affine (must be used on the creating thread).
- Error handling uses return codes + error channels (per-engine and global).
- Include `ferric.h` and link against the Ferric shared library.

See [compatibility.md](compatibility.md) Section 16.13 for the full FFI
contract.

---

## Common Gotchas

| Gotcha | Detail |
|--------|--------|
| `=` vs `eq` | `=` is numeric (coerces types); `eq` is value+type sensitive |
| `format` writes nowhere | `format` returns a string; use `(printout t (format nil ...) crlf)` |
| `sub-string` byte indices | Byte-based, not codepoint-based; identical for ASCII |
| Pure function bodies | `deffunction`/`defmethod` bodies cannot call `assert`, `retract`, `printout` |
| `run` from RHS is a no-op | `(run)` inside a rule action does nothing |
| `reset`/`clear` are deferred | Flag is set and checked after the current action sequence completes |
| No `if`/`then`/`else` | Use separate rules with `(test ...)` CEs instead |
| Activation order | Total order within a run, but not reproducible across runs |

---

## Quick Reference: CLIPS to Ferric

| CLIPS Feature | Ferric Status |
|---------------|---------------|
| `defrule` | Supported |
| `deftemplate` | Supported |
| `deffacts` | Supported |
| `deffunction` | Supported (pure expressions only) |
| `defglobal` | Supported |
| `defmodule` | Supported |
| `defgeneric` / `defmethod` | Supported (pure expressions only) |
| `assert` / `retract` / `modify` / `duplicate` | Supported |
| `printout` / `format` / `read` / `readline` | Supported (format is expression-only) |
| `not` / `exists` / `forall` / `test` | Supported (single-level nesting) |
| Salience | Supported |
| Focus stack | Supported |
| Depth / Breadth / LEX / MEA | Supported |
| `defclass` / COOL | Not supported |
| `if` / `then` / `else` | Not yet implemented |
| Certainty factors | Not supported |
