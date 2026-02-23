# Ferric Compatibility with CLIPS

This document details Ferric's compatibility with CLIPS (C Language Integrated
Production System). Each section covers a major CLIPS language area and
documents supported features, behavioral differences, and any restrictions.

Ferric targets semantic compatibility with the CLIPS Basic Programming Guide
for the supported subset. Rules written for CLIPS within this subset should
execute identically in Ferric without modification.

---

## 16.1 Facts

Ferric supports both ordered (positional) and template (slot-based) facts with
the same value types and working-memory semantics as CLIPS.

### Value Types

| Type | Description |
|------|-------------|
| `INTEGER` | 64-bit signed integer |
| `FLOAT` | 64-bit IEEE 754 double |
| `SYMBOL` | Interned identifier (e.g., `red`, `TRUE`) |
| `STRING` | Quoted string (e.g., `"hello"`) |
| `MULTIFIELD` | Ordered sequence of values |

### Ordered Facts

Ordered facts are positional sequences of values:

```clp
(assert (color red))
(assert (data 10 20 30))
```

### Template Facts

Template facts use named slots defined by `deftemplate`:

```clp
(deftemplate person (slot name) (slot age (default 0)))
(assert (person (name Alice) (age 30)))
```

### Fact Identity

Each asserted fact receives a unique fact index (monotonically increasing
integer). Fact addresses can be captured via pattern-binding variables
(`?f <- (pattern)`) and used in `retract`, `modify`, and `duplicate` actions.

### initial-fact

On `(reset)`, the engine asserts `(initial-fact)` before processing any
`deffacts` groups. This matches CLIPS behavior and is required for
standalone negation and `forall` patterns to activate correctly.

### Behavioral Notes

- Duplicate fact detection: asserting an identical fact to one already in
  working memory is silently ignored (no new fact created).
- Refraction: a rule fires at most once per unique token (set of matching
  facts). Retracting and re-asserting the same content creates a new fact
  identity, allowing re-firing.

---

## 16.2 Rules

Ferric implements `defrule` with the same syntax and semantics as CLIPS,
including all commonly used conditional elements and RHS actions.

### defrule Syntax

```clp
(defrule rule-name
    "optional comment"
    (declare (salience <integer>))
    ;; LHS patterns
    (pattern-1)
    ?var <- (pattern-2)
    (test (> ?x 10))
    =>
    ;; RHS actions
    (printout t "fired" crlf))
```

### Conditional Elements

| CE | Syntax | Notes |
|----|--------|-------|
| Ordered pattern | `(fact-name ?x ?y)` | Positional field matching |
| Template pattern | `(template (slot-name ?v))` | Slot-based matching |
| test | `(test (expr))` | Boolean guard expression |
| not | `(not (pattern))` | Single-pattern negation |
| exists | `(exists (pattern))` | Fires once when any match exists |
| forall | `(forall (P) (Q))` | Universal quantification |
| NCC | `(not (and (P) (Q)))` | Negated conjunction |

### Variable Binding

- Variables bind on first occurrence and must be consistent across all
  patterns in the rule.
- Fact-address variables (`?f <- (pattern)`) capture the fact identity for
  use in `retract`/`modify`/`duplicate`.

### Constraint Connectives

| Connective | Meaning | Example |
|------------|---------|---------|
| `~` | Negation | `(color ~red)` |
| `\|` | Disjunction | `(color red\|blue)` |
| `&` | Conjunction | `(value ?x&~0)` |

### Conflict Resolution Strategies

Four strategies are implemented and configurable:

| Strategy | Description |
|----------|-------------|
| **Depth** | Most recent activation fires first (default) |
| **Breadth** | Oldest activation fires first |
| **LEX** | Lexicographic recency comparison |
| **MEA** | First-pattern recency, then LEX tiebreak |

Not implemented: `Simplicity`, `Complexity`, `Random`.

### Salience

Rules may declare an integer salience. Higher salience fires first within the
chosen conflict resolution strategy:

```clp
(defrule high-priority
    (declare (salience 100))
    (go) => (printout t "high" crlf))

(defrule low-priority
    (declare (salience 10))
    (go) => (printout t "low" crlf))
```

### Activation Ordering Contract

- Ferric guarantees a total ordering of activations at runtime within a
  single `run` call.
- Cross-run replay-identical ordering is **not** guaranteed.
- Semantic compatibility expectations should focus on final working-memory
  outcomes for order-insensitive rule sets.
- For order-sensitive side effects, encode precedence explicitly via salience,
  `focus`, or phase facts.

### RHS Actions

| Action | Notes |
|--------|-------|
| `assert` | Assert ordered or template facts |
| `retract` | Retract by fact-address variable |
| `modify` | Modify template fact slots in place |
| `duplicate` | Create a copy of a template fact with slot overrides |
| `printout` | Write to a named channel (`t` for stdout) |
| `halt` | Stop the run loop immediately |
| `focus` | Push one or more modules onto the focus stack |
| `bind` | Bind a variable or update a global |
| `list-focus-stack` | Print the current focus stack |
| `agenda` | Print the current agenda |
| `run` | No-op when called from RHS (documented behavior) |
| `reset` | Deferred: sets a flag checked after action execution |
| `clear` | Deferred: sets a flag checked after action execution |

**Example -- modify and retract:**

```clp
(deftemplate person (slot name) (slot age (default 0)))

(defrule birthday
    ?ctrl <- (do-birthday)
    ?p <- (person (name ?n) (age ?a))
    =>
    (retract ?ctrl)
    (modify ?p (age (+ ?a 1)))
    (printout t ?n " is now " (+ ?a 1) crlf))
```

---

## 16.3 Deftemplates

Ferric supports `deftemplate` with the same syntax as CLIPS.

```clp
(deftemplate person
    (slot name)
    (slot age (default 0))
    (multislot hobbies))
```

### Slots

- **slot**: Single-valued field. May specify a `(default <value>)`.
- **multislot**: Multi-valued field. Defaults to an empty multifield if no
  default is specified.

### Behavioral Notes

- Templates must be defined before use in patterns or assertions.
- Template names are module-scoped and follow import/export visibility rules.
- Asserting a template fact with missing slots uses declared defaults.
- Template facts can be matched with partial slot patterns (unmentioned
  slots match anything).

---

## 16.4 Deffacts

`deffacts` groups define facts that are asserted automatically during
`(reset)`.

```clp
(deffacts startup
    (color red)
    (color blue)
    (person (name Alice) (age 30)))
```

### Semantics

- All `deffacts` groups are processed during `(reset)`, after `(initial-fact)`
  is asserted.
- Multiple `deffacts` groups may exist; all are processed.
- `deffacts` groups are module-scoped. Use `MODULE::name` syntax to define
  deffacts in a specific module context.
- On each `(reset)`, existing user facts are retracted and deffacts are
  reasserted.

---

## 16.5 Defrules

This section covers the full `defrule` syntax reference. See Section 16.2 for
high-level rule semantics.

### LHS Conditional Element Coverage

All of the following are supported:

- **Ordered patterns**: `(fact-name ?x ?y)`
- **Template patterns**: `(template (slot ?v))`
- **Variable binding**: `?f <- (pattern)`
- **test CE**: `(test (> ?x 10))`
- **not CE**: `(not (pattern))`
- **exists CE**: `(exists (pattern))`
- **forall CE**: `(forall (P) (Q))`
- **Negated conjunction**: `(not (and (P) (Q)))`
- **Constraint connectives**: `&`, `|`, `~`

### Pattern Nesting Restrictions

Ferric supports single-level negation, exists, forall, and NCC. The following
nestings are **not** supported:

| Unsupported Pattern | Rationale |
|---------------------|-----------|
| Triple-nested negation | Rete subnetwork complexity; rarely needed in practice |
| `(exists (not ...))` | Equivalent refactorings exist using separate rules |
| Nested `(forall ...)` | Decompose into multiple rules with phase facts |

**Refactoring example** -- replace `(exists (not (done ?x)))` with:

```clp
(defrule has-undone
    (item ?x)
    (not (done ?x))
    =>
    (assert (has-undone-item)))
```

### forall Semantics

`forall` is desugared to `NCC([P, neg(Q)])` at loader level. This means
"for every fact matching P, there also exists a matching Q."

Vacuous truth: when no facts match P, the forall condition holds:

```clp
;; Fires because no (task ?x) facts exist
(defrule all-tasks-done
    (ready)
    (forall (task ?x) (done ?x))
    =>
    (printout t "all done" crlf))
```

### Module Scoping

- Rules are scoped to the module in which they are defined.
- Only rules in the current focus-stack module are eligible to fire.
- Module-qualified syntax: `(defrule MODULE::rule-name ...)`
- Focus stack controls module execution order via `(focus MODULE)` action.

**Example:**

```clp
(defmodule A)
(defmodule B)

(defrule MAIN::start
    (go) => (focus A) (printout t "MAIN" crlf))

(defrule A::do-a
    (initial-fact) => (focus B) (printout t "A" crlf))

(defrule B::do-b
    (initial-fact) => (printout t "B" crlf))
```

### Pattern Restriction Diagnostics

Unsupported constructs produce source-located compile errors. Ferric does not
silently ignore invalid patterns.

---

## 16.6 Defglobals

Ferric supports `defglobal` with the `?*name*` naming convention.

```clp
(defglobal ?*count* = 0)
(defglobal ?*label* = "default")
```

### Module Scoping

- Globals are scoped to the module in which they are defined.
- Cross-module access requires `import`/`export` declarations.
- Module-qualified references use `?*MODULE::name*` syntax:

```clp
(defmodule CONFIG (export defglobal ?ALL))
(defglobal ?*base-value* = 10)

(defmodule MAIN (import CONFIG defglobal ?ALL))

(defrule MAIN::update
    (run-it)
    =>
    (bind ?*CONFIG::base-value* (* ?*CONFIG::base-value* 3))
    (printout t "value: " ?*CONFIG::base-value* crlf))
```

### Mutation via bind

- `(bind ?*name* <value>)` updates an existing global variable.
- `bind` does **not** create new variables -- the global must already exist.
- Globals are accessible from rule RHS actions and function bodies.

### Reset Behavior

On `(reset)`, globals are restored to their declared initial values.

---

## 16.7 Deffunctions

Ferric supports user-defined functions via `deffunction`.

```clp
(deffunction double (?x) (* ?x 2))
(deffunction greet (?name)
    (str-cat "Hello, " ?name "!"))
```

### Parameters

- **Regular parameters**: `?x`, `?y`
- **Wildcard parameter**: `$?rest` (collects remaining arguments as a
  multifield; must be the last parameter)

### Evaluation

Function bodies are expression sequences. The value of the last expression is
the return value. Functions are pure expressions -- they cannot execute
side-effect actions like `assert`, `retract`, or `printout` directly. Use
functions to compute values and perform side effects in the calling rule's RHS.

### Module Scoping

- Functions are registered in their defining module.
- Cross-module calls require `import`/`export`:

```clp
(defmodule UTILS (export deffunction ?ALL))
(deffunction square (?x) (* ?x ?x))

(defmodule MAIN (import UTILS deffunction ?ALL))
(defrule MAIN::compute
    (compute) => (printout t "result: " (square 5) crlf))
```

### Recursive Calls

Recursive calls are supported. A configurable maximum call depth prevents
stack overflow.

### Conflict with defgeneric

Defining a `deffunction` and `defgeneric` with the same name in the same
module is a compile error with a diagnostic message.

---

## 16.8 Generic Functions and Methods

Ferric supports generic function dispatch via `defgeneric` and `defmethod`.

### Syntax

```clp
(defgeneric describe)
(defmethod describe ((?x INTEGER)) (str-cat "integer: " ?x))
(defmethod describe ((?x STRING)) (str-cat "string: " ?x))
(defmethod describe ((?x NUMBER)) (str-cat "number: " ?x))
```

### Method Specificity

Methods are ranked by type specificity. More specific types win:
`INTEGER` > `NUMBER`, `FLOAT` > `NUMBER`, etc. When multiple methods could
match, the most specific applicable method is selected.

```clp
(defgeneric classify)
(defmethod classify ((?x NUMBER)) (str-cat "number"))
(defmethod classify ((?x INTEGER)) (str-cat "integer"))
;; (classify 5) => "integer" (INTEGER is more specific than NUMBER)
```

### call-next-method

Within a method body, `(call-next-method)` invokes the next less-specific
applicable method in the dispatch chain:

```clp
(defgeneric annotate)
(defmethod annotate ((?x NUMBER)) (str-cat "num(" ?x ")"))
(defmethod annotate ((?x INTEGER)) (str-cat "int+" (call-next-method)))
;; (annotate 7) => "int+num(7)"
```

### Wildcard Parameters

Methods support wildcard parameters for variable-arity dispatch.

### Auto-indexing

Method indices are auto-assigned when not explicitly provided. Explicit
indices are also supported.

### Module Scoping

Generic functions are module-scoped and follow the same import/export
visibility rules as deffunctions.

### Interaction with deffunction

A `defgeneric` and `deffunction` with the same name in the same module
is a compile error with a diagnostic message.

### Method Bodies

Method bodies are pure expressions (same as deffunction bodies). They cannot
execute side-effect actions directly. Use the calling rule's RHS for
`printout`, `assert`, etc.

---

## 16.9 Modules

Ferric supports the CLIPS module system with `defmodule`, import/export
visibility, and focus-stack-driven execution.

### defmodule Syntax

```clp
(defmodule SENSORS (export deftemplate reading))
(defmodule MAIN (import SENSORS deftemplate reading))
```

### Export/Import

- `(export <construct-type> ?ALL)` -- export all constructs of a type
- `(export <construct-type> <name>)` -- export a specific construct
- `(import <module> <construct-type> ?ALL)` -- import all exports of a type
- `(import <module> <construct-type> <name>)` -- import a specific construct

Supported construct types for import/export: `deftemplate`, `deffunction`,
`defglobal`, `defgeneric`.

### Module-Qualified Names

Constructs can be referenced with `MODULE::name` syntax:

```clp
(deffacts MAIN::startup (go))
(defrule MAIN::start (go) => (printout t "started" crlf))
(bind ?*CONFIG::base-value* 42)
```

### Focus Stack

- `MAIN` is the default focus module after `(reset)`.
- `(focus MODULE)` pushes a module onto the focus stack.
- Only rules in the current focus-stack module are eligible to fire.
- When a module's agenda is empty, it is popped and the next module resumes.

### Facts Are Global

Facts exist in a single global working memory. Module scoping affects only
which rules are eligible to fire, not which facts are visible.

---

## 16.10 Standard Library

Ferric implements the following standard library functions. All behave
identically to their CLIPS counterparts for the supported argument types.

### Math Functions

| Function | Description | Example |
|----------|-------------|---------|
| `+` | Addition | `(+ 1 2)` => `3` |
| `-` | Subtraction | `(- 10 3)` => `7` |
| `*` | Multiplication | `(* 4 5)` => `20` |
| `/` | Division | `(/ 10 3)` => `3.333...` |
| `div` | Integer division | `(div 10 3)` => `3` |
| `mod` | Modulo | `(mod 10 3)` => `1` |
| `abs` | Absolute value | `(abs -5)` => `5` |
| `min` | Minimum | `(min 3 7)` => `3` |
| `max` | Maximum | `(max 3 7)` => `7` |

### Type Conversion

| Function | Description |
|----------|-------------|
| `integer` | Convert to integer (truncates floats) |
| `float` | Convert to float |

### Comparison Functions

| Function | Description |
|----------|-------------|
| `=` | Numeric equality |
| `!=` / `<>` | Numeric inequality |
| `>`, `<`, `>=`, `<=` | Numeric ordering |
| `eq` | Value equality (type-sensitive) |
| `neq` | Value inequality |

### Logical Functions

| Function | Description |
|----------|-------------|
| `and` | Logical AND |
| `or` | Logical OR |
| `not` | Logical NOT |

### Predicate / Type-Checking Functions

| Function | Returns TRUE when |
|----------|-------------------|
| `integerp` | Argument is an INTEGER |
| `floatp` | Argument is a FLOAT |
| `numberp` | Argument is INTEGER or FLOAT |
| `symbolp` | Argument is a SYMBOL |
| `stringp` | Argument is a STRING |
| `lexemep` | Argument is a SYMBOL or STRING |
| `multifieldp` | Argument is a MULTIFIELD |
| `evenp` | Argument is an even integer |
| `oddp` | Argument is an odd integer |

### String / Symbol Functions

| Function | Description | Example |
|----------|-------------|---------|
| `str-cat` | Concatenate to string | `(str-cat "a" "b")` => `"ab"` |
| `sym-cat` | Concatenate to symbol | `(sym-cat a b)` => `ab` |
| `str-length` | String length in bytes | `(str-length "hello")` => `5` |
| `sub-string` | Extract substring (1-indexed) | `(sub-string 1 3 "hello")` => `"hel"` |

### Multifield Functions

| Function | Description | Example |
|----------|-------------|---------|
| `create$` | Create a multifield | `(create$ a b c)` |
| `length$` | Multifield length | `(length$ (create$ a b c))` => `3` |
| `nth$` | Get nth element (1-indexed) | `(nth$ 2 (create$ a b c))` => `b` |
| `member$` | Find element position | `(member$ b (create$ a b c))` => `2` |
| `subsetp` | Subset test | `(subsetp (create$ a) (create$ a b))` => `TRUE` |

### I/O Functions

| Function | Description |
|----------|-------------|
| `printout` | Write to a named channel |
| `format` | Printf-style formatting (returns string; does not write to router) |
| `read` | Read a single value from input |
| `readline` | Read a line from input |

**format note:** In Ferric, `format` is an evaluator-only function that
returns a formatted string. It does not write directly to a router. Use
`(printout t (format nil "n=%d" 42) crlf)` to produce output.

### Agenda / Focus Functions

| Function | Description |
|----------|-------------|
| `get-focus` | Return the current focus module name |
| `get-focus-stack` | Return the focus stack as a multifield |

---

## 16.11 Unsupported Features

The following features are explicitly out of scope.

| Feature | Status | Notes |
|---------|--------|-------|
| COOL object system | Not planned | Classes, instances, message-passing |
| Certainty factors | Not planned | Probabilistic/fuzzy reasoning |
| Distributed evaluation | Not planned | Networked rule engines |
| `Simplicity` strategy | Deferred | Until fully specified |
| `Complexity` strategy | Deferred | Until fully specified |
| `Random` strategy | Deferred | Until fully specified |
| Replay-identical ordering | Not guaranteed | Total order within a run, but not reproducible across runs |
| `if`/`then`/`else` expressions | Not yet implemented | Use conditional rule patterns instead |
| Triple-nested negation | Not supported | Decompose into multiple rules |
| `(exists (not ...))` | Not supported | Use separate rules |
| Nested `(forall ...)` | Not supported | Decompose with phase facts |

---

## 16.12 String and Symbol Comparison Semantics

### Byte-Equality Comparison

Ferric uses **byte-equality comparison** for strings and symbols. Two values
are equal if and only if their byte sequences are identical.

- No Unicode normalization is performed. NFC and NFD representations of the
  same character are treated as distinct values.
- No collation or locale-aware ordering.
- No case-insensitive comparison built in.

### sub-string Indexing

`sub-string` uses **byte indices** (1-indexed), not Unicode codepoint indices.
For ASCII content, byte and codepoint indices are identical.

### Compatibility with CLIPS

For ASCII content, Ferric's comparison and indexing behavior is identical to
CLIPS. Differences arise only with non-ASCII content, where CLIPS behavior
varies by platform and build configuration.

### Guidance for Unicode Users

If your application requires normalization-aware comparison, normalize strings
to a canonical form (e.g., NFC) before asserting them as facts. This ensures
consistent matching regardless of input source.

---

## 16.13 External Interface Contracts (FFI + Embedding)

Ferric provides a C-compatible FFI layer (`ferric-ffi`) for embedding into
C, C++, Swift, Kotlin (NDK), and other languages with C FFI support.

### Engine Lifecycle

```c
#include "ferric.h"

// Create engine with defaults
FerricEngine* engine = ferric_engine_new();

// Or with configuration
FerricConfig cfg = {
    .string_encoding = FERRIC_STRING_ENCODING_UTF8,
    .strategy = FERRIC_CONFLICT_STRATEGY_DEPTH,
    .max_call_depth = 256
};
FerricEngine* engine = ferric_engine_new_with_config(&cfg);

// Load, reset, run
ferric_engine_load_string(engine, "(defrule r (go) => (printout t \"hello\" crlf))");
ferric_engine_reset(engine);

uint64_t fired;
ferric_engine_run(engine, -1, &fired);

// Read output
const char* output = ferric_engine_get_output(engine, "stdout");

// Clean up
ferric_engine_free(engine);
```

### Thread Affinity

Engine instances are bound to their creating thread (`!Send + !Sync`):

- Every `ferric_engine_*` function validates thread affinity before mutation.
- Wrong-thread calls return `FERRIC_ERROR_THREAD_VIOLATION` with no state
  modified.
- **Exceptions**: `ferric_engine_last_error` and `ferric_engine_last_error_copy`
  skip thread checks (diagnostic access should always work).

Global error functions (`ferric_last_error_global`, etc.) use thread-local
storage and are safe from any thread.

### Error Handling

Two error channels exist:

1. **Per-engine errors**: `ferric_engine_last_error()` /
   `ferric_engine_last_error_copy()` / `ferric_engine_clear_error()`
2. **Global (thread-local) errors**: `ferric_last_error_global()` /
   `ferric_last_error_global_copy()` / `ferric_clear_error_global()`

Use the global channel for pre-engine failures (e.g., creation errors).
Use the per-engine channel for all other operations.

### Copy-to-Buffer Contract

The `*_copy` functions follow a uniform contract:

| Condition | Return Code | `*out_len` |
|-----------|-------------|------------|
| No error stored | `FERRIC_ERROR_NOT_FOUND` | 0 |
| `out_len` is NULL | `FERRIC_ERROR_INVALID_ARGUMENT` | (not written) |
| `buf` is NULL, `buf_len` is 0 (size query) | `FERRIC_ERROR_OK` | Required size (incl. NUL) |
| `buf` non-null, `buf_len` >= needed | `FERRIC_ERROR_OK` | Bytes written (incl. NUL) |
| `buf` non-null, `buf_len` < needed | `FERRIC_ERROR_BUFFER_TOO_SMALL` | Full needed size (incl. NUL) |

On truncation, the buffer receives `buf_len - 1` bytes followed by a NUL
terminator.

### Fact Lifecycle

```c
// Assert and get fact ID
uint64_t fact_id;
ferric_engine_assert_string(engine, "(color red)", &fact_id);

// Retract by ID
ferric_engine_retract(engine, fact_id);

// Query facts
size_t count;
ferric_engine_fact_count(engine, &count);

size_t field_count;
ferric_engine_get_fact_field_count(engine, fact_id, &field_count);

FerricValue val;
ferric_engine_get_fact_field(engine, fact_id, 0, &val);
// ... use val ...
ferric_value_free(&val);
```

### Action Diagnostics

Non-fatal warnings from rule execution (e.g., module visibility issues) are
collected as action diagnostics, distinct from fatal errors:

```c
size_t diag_count;
ferric_engine_action_diagnostic_count(engine, &diag_count);

for (size_t i = 0; i < diag_count; i++) {
    size_t needed;
    // Size query
    ferric_engine_action_diagnostic_copy(engine, i, NULL, 0, &needed);
    char* buf = malloc(needed);
    ferric_engine_action_diagnostic_copy(engine, i, buf, needed, &needed);
    printf("warning: %s\n", buf);
    free(buf);
}
ferric_engine_clear_action_diagnostics(engine);
```

### Value and Memory Management

| Function | Purpose |
|----------|---------|
| `ferric_string_free` | Free a Ferric-allocated C string |
| `ferric_value_free` | Free a `FerricValue` and its owned resources (recursive) |
| `ferric_value_array_free` | Free an array of `FerricValue`s |

Borrowed pointers (from `ferric_engine_last_error`, `ferric_engine_get_output`)
must **not** be freed by the caller and are valid only until the next FFI call
that may modify that channel.

### Panic Policy

FFI builds use `panic = "abort"` profiles. No Rust panic unwind crosses the
FFI boundary.

---

## 16.14 Machine-Readable CLI Diagnostics

The `ferric` CLI supports `--json` mode for structured diagnostics on stderr.

### Commands

```
ferric run [--json] <file>    # load, reset, run, print output
ferric check [--json] <file>  # load and validate without executing
```

### JSON Diagnostic Format

Each diagnostic is a single JSON object on one line of stderr:

```json
{"command":"run","level":"error","kind":"load_error","message":"Unexpected token at line 3"}
```

### Field Descriptions

| Field | Type | Values |
|-------|------|--------|
| `command` | string | `"run"` or `"check"` |
| `level` | string | `"error"` or `"warning"` |
| `kind` | string | Diagnostic category (see below) |
| `message` | string | Human-readable diagnostic text |

### Diagnostic Kinds

| Kind | Emitted by | Description |
|------|-----------|-------------|
| `io_error` | run, check | File not found or I/O failure |
| `load_error` | run, check | Parse or compilation error |
| `runtime_error` | run | Execution failure |
| `action_warning` | run | Non-fatal action diagnostic |

### Evolution Contract

- New fields may be added to diagnostic objects in future versions.
- Existing documented fields will not be removed or repurposed.
- Parsers should ignore unknown fields for forward compatibility.

### Exit Codes

| Code | Meaning |
|------|---------|
| 0 | Success |
| 1 | Runtime or load error |
| 2 | Usage error (missing argument) |

### Example: CI Integration

```bash
# Check syntax and capture errors as JSON
ferric check --json rules.clp 2> errors.json
if [ $? -ne 0 ]; then
    cat errors.json | jq '.message'
fi

# Run and capture warnings
ferric run --json rules.clp 2> diagnostics.json
```

Standard output (stdout) contains the rule engine's normal output. All
diagnostics are emitted to stderr.
