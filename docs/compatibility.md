# Ferric Compatibility with CLIPS

This document details Ferric's compatibility with CLIPS (C Language Integrated
Production System). Each section covers a major CLIPS language area and
documents supported features, behavioral differences, and any restrictions.

Ferric targets semantic compatibility with the CLIPS Basic Programming Guide
for the supported subset. "Supported" means that the language area is
implemented, not that every rule set in that area has been proven equivalent.
Exact CLIPS compatibility claims are limited to the reviewed differential
policy cases and are qualified by the known gaps below.

## Known Differential Gaps

The blocking pinned-CLIPS lane currently retains the following known
differences as exact, issue-linked deviations. They are not accepted as
equivalent: the gate fails if their observed fields or semantic fingerprints
change, and it rejects every unexplained divergence.

| Area | Current difference from pinned CLIPS | Policy cases | Tracking |
|------|--------------------------------------|--------------|----------|
| LEX and MEA agenda order | Recency vectors and the MEA tiebreak differ for selected multi-pattern activations. | `FR-RETE-009` LEX recency-vector ordering; `FR-RETE-009-MEA` MEA recency-vector ordering | [#155](https://github.com/plx/ferric-rules/issues/155) |

The reviewed differential policy covers 57 scenarios: the existing 22 cases
and 35 distinct rehabilitation scenarios, plus a generated-harness control.
55 cases are equivalent; the two LEX/MEA cases retain exact known divergences. It does not turn undeclared corpus
fixtures into compatibility claims; those remain pending or incompatible
until they receive a structured oracle and reviewed policy entry. See
[Compatibility assessment oracles](compatibility-assessment.md) for the exact
evidence boundary.

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

RHS assertions resolve declared templates in the rule's module, evaluate named
slots, fill defaults, and propagate template matches. Multislots splice supplied
multifield values; both `?items` and `$?items` read the same bound value. Invalid
slot names, repeated slots and statically invalid cardinality reject the rule
before installation. A dynamic single-slot cardinality error stops that RHS
without asserting a partial fact. Void expression results are omitted from
multislots while their output effects remain observable.

Pre-1.0 migration: template metadata now records slot cardinality. Legacy raw
engine snapshots are not a stable interchange contract across this change;
retain application facts/rule source for rebuilding. The rehabilitation's
versioned persistence work will define the supported snapshot envelope.

Complex non-linear predicate or return-value constraints inside negated ordered
patterns are CLIPS-valid but explicitly rejected during load. PR #254 removed an
incorrect firing-time fallback; it did not complete that optional language
feature. The remaining gap is tracked in [#300](https://github.com/plx/ferric-rules/issues/300).

### Fact Identity

Each successfully asserted fact receives a unique fact index. Fact addresses
can be captured via pattern-binding variables (`?f <- (pattern)`) and used in
`retract`, `modify`, and `duplicate` actions.

### Fact Duplication

Like CLIPS, Ferric disables fact duplication by default. With duplication
disabled, an assertion is rejected if an equivalent active fact exists; it
creates no new fact ID, RETE token, activation, or existential support.

Structural equality is defined as follows:

- Ordered facts compare their relation and every positional field.
- Template facts compare their template identity and every slot value in the
  template's canonical slot order.
- Nested multifields compare recursively. Floats compare by IEEE-754 bit
  representation, including distinct signed-zero and NaN representations.
- Fact IDs and assertion timestamps are never part of structural equality.

`(get-fact-duplication)` reports the current policy.
`(set-fact-duplication TRUE|FALSE)` changes it immediately and returns the
previous setting. Existing duplicate facts are not collapsed when duplication
is disabled. The setting survives `reset`, `clear`, and engine snapshot
round-trips.

### initial-fact

On `(reset)`, Ferric currently reasserts registered `deffacts` before
`(initial-fact)`. The bootstrap fact enables standalone negation and `forall`
patterns, but this ordering differs from pinned CLIPS and can reverse activation
order; see [#156](https://github.com/plx/ferric-rules/issues/156).

### Behavioral Notes

- Duplicate fact detection: asserting an identical fact to one already in
  working memory is rejected by default (no new fact created).
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

Depth and breadth are the supported CLIPS ordering strategies. The host API
also retains two experimental Ferric orderings for existing consumers:

| Strategy | Description |
|----------|-------------|
| **Depth** | Most recent activation fires first (default) |
| **Breadth** | Oldest activation fires first |
| **LEX** (experimental) | Ferric's pattern-order recency comparison; not CLIPS LEX |
| **MEA** (experimental) | Ferric's first-pattern recency, then its LEX tiebreak; not CLIPS MEA |

CLIPS LEX/MEA specificity and sorted-recency semantics are deferred (#155).
Use depth/breadth for portable rules. `Simplicity`, `Complexity`, and `Random`
are not implemented. CLIPS `set-strategy`/`get-strategy` source commands are
unsupported and produce missing-function diagnostics; configure a declared
strategy through the host API. Bindings reject unknown enum/name values.

### Salience

Rules may declare one static integer salience in -10000 through 10000.
Higher salience fires first within the
chosen conflict resolution strategy:

```clp
(defrule high-priority
    (declare (salience 100))
    (go) => (printout t "high" crlf))

(defrule low-priority
    (declare (salience 10))
    (go) => (printout t "low" crlf))
```

Dynamic salience expressions, salience-evaluation modes, `refresh-agenda`, and
`auto-focus` declarations are unsupported. Invalid/unsupported declarations
reject the construct; they never become salience zero. `refresh-agenda` now
reports an error instead of returning a successful no-op. These are deliberate
pre-1.0 corrections to previously silent behavior.

### Fact-query expressions

Use the host fact inspection API for queries. RHS `do-for-*` actions retain
their existing fact iteration, but CLIPS query expressions (`any-factp`,
`find-fact`, `find-all-facts`) in expressions or callable bodies are unsupported.
They now report a load or execution error rather than inventing FALSE/empty
results. Query-bound `?fact:slot` expressions remain unsupported; ordinary
rule LHS fact-address slot access remains available. This limitation does not
restrict normal joins or host-side typed fact inspection.

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
| `if`/`then`/`else` | Conditional action execution |
| `while` | Conditional loop with `do`; shares the configured per-activation action-loop budget |
| `loop-for-count` | Indexed loop with optional variable binding; shares the configured per-activation action-loop budget |
| `progn$` / `foreach` | Multifield iteration with element and index binding |
| `switch`/`case`/`default` | Multi-branch dispatch |
| `do-for-fact` | Iterate first matching fact |
| `do-for-all-facts` | Iterate all matching facts |
| `delayed-do-for-all-facts` | Deferred all-facts iteration |
| `any-factp` | Boolean fact existence check |
| `find-fact` | Find first matching fact |
| `find-all-facts` | Find all matching facts |

### RHS evaluation errors

Ferric matches CLIPS when evaluating an RHS action fails:

- actions before the failure keep their effects;
- the first failure is retained in `action_diagnostics()` with its original
  evaluator category and cause;
- later actions in that activation do not run;
- the failing activation is consumed and the current `run()` returns
  `HaltReason::ActionError`; and
- lower-priority activations remain on the agenda. A subsequent `run()` clears
  the previous diagnostic and continues that work. `reset()` instead rebuilds
  the original agenda, so the failing activation can be encountered again.

An action error does not set the engine's persistent halt flag. `step()` still
returns the processed activation and exposes the diagnostic without consuming
the next activation.

A diagnostic is not always fatal. Unavailable `sort` comparator names or
incompatible builtin/deffunction arity produce a diagnostic and return `FALSE`
without stopping subsequent actions. Check the run outcome rather than treating every entry in
`action_diagnostics()` as an action failure. A fatal sort error can also return
a partial value to its enclosing expression before the action stops; it does
not imply that an enclosing assignment was rolled back. See
[Predicate sorting](#predicate-sorting) for these distinctions.

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

- Templates must be defined before use in patterns or assertions. A new
  explicit template is rejected while existing facts, seed definitions, or
  constructs depend on an ordered relation with the same local name. Earlier
  ordered uses in the same load are protected as well. Ordered relations have
  global identities in Ferric, so this guard also applies across modules and
  to module-qualified spellings; separate already-explicit template identities
  remain module-scoped. The internal `initial-fact` identity cannot be shadowed.
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

- All `deffacts` groups are processed during `(reset)`. Ferric currently
  processes them before asserting `(initial-fact)`; pinned CLIPS uses the
  opposite bootstrap order, as tracked in
  [#156](https://github.com/plx/ferric-rules/issues/156).
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

### Source and compiled network limits

Source loading rejects input above 16 MiB before parsing. Rule normalization
and disjunction expansion use checked, conservative work estimates: at most
256 alternatives, 16,384 expanded pattern/constraint nodes, and 8 MiB of
expanded source per rule. Both normalization passes also share a per-load
budget of 1,048,576 estimated nodes and 32 MiB of expanded source. The estimate
may reject an unusually redundant OR expression that could be optimized to
less work; Ferric does not perform that optimization implicitly.

Each compiled rule allows at most 64 condition nodes, counting predicates and
nested NCC wrappers/children, and each alpha path allows at most 64 constant
tests. These bounds keep recursive propagation practical without adding a
resumable execution subsystem. Boundary regressions exercise combined alpha
and beta depth, assertion, run, reset, and retraction on a 512 KiB native stack.
Over-limit constructs fail before installation; previously installed rules and
facts remain usable. Loading multiple constructs remains incremental, so a
later failure does not roll back earlier successful constructs.

These are implementation limits for the current pre-1.0 engine, not CLIPS
language limits or a guarantee that arbitrary large fact populations fit a
host's memory. The recommended snapshot envelope applies its own input and
restored-graph validation limits.

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

### Logical support

Every `logical` CE is rejected during rule loading, including nested and
disjunctive positions. Ferric does not track the support that CLIPS uses to
retract derived facts automatically. A rejected replacement preserves the
previous installed rule. Use ordinary stated facts and explicit retraction
when the application owns that lifecycle.

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

Each named global is installed after its initializer succeeds. Earlier names in
one `defglobal` group remain available to later initializers. If an initializer
fails, its name and later names in that group are not installed; earlier globals
and following top-level constructs retain their incremental load behavior.

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
the return value. `(return)` and `(return <expression>)` immediately unwind the
current deffunction or generic-method call; an inner callable's return does not
unwind its caller. A top-level return is an evaluation error. On a rule RHS,
`return` follows CLIPS behavior and stops only the remaining actions in that
activation.

Bodies are evaluator expressions, not full RHS action lists: expression
functions such as `str-cat`, `format`, and `printout` are available, but fact
mutation and agenda/focus control belong in the calling rule's RHS.

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

Recursive calls are supported with a requested maximum call depth, capped at
32 effective callable frames in every build profile. Active expression evaluation
is bounded at 64 frames across nested bodies and calls; expression translation
and cloning accept trees up to 16 levels. Limit violations report an execution
or load diagnostic before continuing recursive evaluation. Callable bodies
are checked before registration; a failed function redefinition or implicit
generic registration preserves the previous registry state.

Embedding note: release recursion regressions run on 512 KiB native stacks;
unoptimized development regressions use 2 MiB. These are supported test
baselines, not a promise for arbitrarily small host stacks. Larger requested
call limits remain readable and persistable but cannot raise the enforced
ceiling. This pre-1.0 change replaces unsafe high-depth behavior with explicit
execution errors.

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

Method bodies use the same evaluator expression model as deffunction bodies.
Use the calling rule's RHS for fact mutation and agenda/focus control.

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

### Template declaration spellings across modules

CLIPS 6.30 accepts the same unqualified template declaration name in different
modules. Ferric currently requires distinct public declaration spellings: after
`(defmodule A) (deftemplate item ...)`, declare a second identity as
`(defmodule B) (deftemplate B::item ...)`. Unqualified references inside module B
still resolve its local template. A second conflicting unqualified declaration
is rejected before installing metadata; previously it could overwrite the
public name index and make snapshots invalid. This is an explicit module support
limitation, not a claim of CLIPS equivalence. Existing qualified identities and
same-module unused-template replacement retain their behavior.

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
| `**` | Power | `(** 2 10)` => `1024.0` |
| `sqrt` | Square root | `(sqrt 16)` => `4.0` |
| `round` | Round to nearest integer; half ties choose the lower integer, INTEGER inputs stay exact | `(round 2.5)` => `2`; `(round -2.5)` => `-3` |
| `ceiling` | Round up to integer | `(ceiling 3.1)` => `4` |
| `floor` | Round down to integer | `(floor 3.9)` => `3` |
| `pi` | Pi constant | `(pi)` => `3.14159...` |
| `exp` | e^x | `(exp 1)` => `2.718...` |
| `log` | Natural logarithm | `(log 2.718)` => `~1.0` |
| `log10` | Base-10 logarithm | `(log10 100)` => `2.0` |
| `sin`, `cos`, `tan` | Trigonometric | `(sin 0)` => `0.0` |
| `asin`, `acos`, `atan` | Inverse trigonometric | `(acos 1)` => `0.0` |
| `atan2` | Two-argument arctangent | `(atan2 1 1)` => `0.785...` |
| `sinh`, `cosh`, `tanh` | Hyperbolic | `(cosh 0)` => `1.0` |
| `asinh`, `acosh`, `atanh` | Inverse hyperbolic | `(asinh 0)` => `0.0` |
| `deg-rad`, `rad-deg` | Angle conversion | `(deg-rad 180)` => `3.14159...` |
| `deg-grad`, `grad-deg` | Degree/gradian conversion | `(deg-grad 90)` => `100.0` |

`min` and `max` return the selected operand with its original INTEGER or FLOAT
type. Numeric ties retain the first selected operand, including the sign of a
floating-point zero. Integer pairs compare exactly; mixed INTEGER/FLOAT pairs
compare after floating-point conversion. Each comparison uses the current
selected operand's type, even if an earlier discarded operand was a FLOAT.

For FLOAT arguments, Ferric uses `ceil(x - 0.5)` to reproduce the observed
CLIPS floating-point boundary behavior. For example,
`(round -0.49999999999999994)` returns `-1`. INTEGER arguments remain unchanged
without conversion through floating point.

### Type Conversion

| Function | Description |
|----------|-------------|
| `integer` | Convert to integer (truncates floats) |
| `float` | Convert to float |

### Comparison Functions

| Function | Description |
|----------|-------------|
| `=` | First numeric operand equals every subsequent operand |
| `!=` / `<>` | First numeric operand differs from every subsequent operand |
| `>`, `<`, `>=`, `<=` | Each adjacent numeric pair satisfies the ordering |
| `eq` | Value equality (type-sensitive) |
| `neq` | Value inequality |

Numeric comparisons accept two or more operands and evaluate them from left to
right, stopping at the first failed comparison. For example, `(< 1 2 3)` and
`(<> 1 2 2)` return TRUE; `(< 2 1 (later-call))` returns FALSE without evaluating
`later-call`. A reached nonnumeric operand produces a type error.

INTEGER pairs compare exactly. Mixed INTEGER/FLOAT pairs use floating-point
conversion, and FLOAT equality uses exact numeric equality without an epsilon
tolerance. For example, `(= 0.0 1e-20)` returns FALSE and `(= -0.0 0.0)` returns TRUE.

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
| `str-index` | Find substring position (1-indexed), FALSE if not found | `(str-index "lo" "hello")` => `4` |
| `upcase` | Convert to uppercase (preserves type) | `(upcase "hello")` => `"HELLO"` |
| `lowcase` | Convert to lowercase (preserves type) | `(lowcase "HELLO")` => `"hello"` |
| `str-compare` | Lexicographic comparison (-1, 0, or 1) | `(str-compare "a" "b")` => `-1` |
| `string-to-field` | Read the first CLIPS field from STRING, SYMBOL, or INSTANCE-NAME bytes | `(string-to-field "42 trailing")` => `42` |
| `explode$`, `str-explode` | Scan STRING bytes into typed CLIPS fields | `(explode$ "a \"two words\" 3")` => `(a "two words" 3)` |
| `funcall` | Call function by name at runtime | `(funcall + 1 2)` => `3` |

`string-to-field` ignores the input after its first token and preserves INTEGER,
FLOAT, STRING, SYMBOL, and INSTANCE-NAME identity. Empty or comment-only input
returns the symbol `EOF`; variable and punctuation tokens return their literal
print forms as strings. Quoted fields preserve bytes and CLIPS escape behavior.
Like a CLIPS string source, the input ends at its first NUL byte.

Integer overflow saturates with a scanner warning. An unterminated quoted field
returns its partial string with a notice, including a literal `0xff` byte when
the final escape reaches EOF. These notices remain observable through action
diagnostics and the `wwarning` or `werror` output channel while evaluation
continues. Wrong argument count or type halts evaluation; an unknown scanner
token instead returns the string `*** ERROR ***` without a diagnostic.

`explode$` (also named `str-explode`) scans every field and returns a
MULTIFIELD, preserving quoted strings, INTEGER/FLOAT distinctions, symbols,
and instance names. Empty input returns an empty multifield. Variable and
punctuation tokens become strings of their print forms; an unknown token
becomes the string `<<<unprintable character>>>` and scanning continues.
It shares the byte, escape, NUL, and nonfatal notice behavior described above,
retaining earlier fields when a later quoted string is incomplete.

Both aliases require exactly one STRING argument, evaluated once. Wrong
argument count, wrong type, or an operand error yields an empty multifield
and halts evaluation. SYMBOL and INSTANCE-NAME input values are not accepted
by these aliases.

### Multifield Functions

| Function | Description | Example |
|----------|-------------|---------|
| `create$` | Create a multifield | `(create$ a b c)` |
| `implode$` | Convert multifield fields to a STRING | `(implode$ (create$ a 3))` => `"a 3"` |
| `length$` | Multifield length | `(length$ (create$ a b c))` => `3` |
| `nth$` | Get nth element (1-indexed), `nil` if absent | `(nth$ 2 (create$ a b c))` => `b` |
| `member$` | Find element position | `(member$ b (create$ a b c))` => `2` |
| `subsetp` | Subset test | `(subsetp (create$ a) (create$ a b))` => `TRUE` |
| `insert$` | Insert values at position | `(insert$ (create$ a c) 2 b)` => `(a b c)` |
| `delete$` | Remove range (1-indexed, inclusive) | `(delete$ (create$ a b c) 2 2)` => `(a c)` |
| `replace$` | Replace range with values | `(replace$ (create$ a b c) 2 2 x)` => `(a x c)` |
| `first$` | First element as multifield | `(first$ (create$ a b c))` => `(a)` |
| `rest$` | All but first as multifield | `(rest$ (create$ a b c))` => `(b c)` |
| `sort` | Stable predicate sort of scalar and multifield arguments | `(sort < (create$ 3 1 2))` => `(3 2 1)` |

`create$` evaluates VOID-producing operands for their effects but omits those
scalar results from the multifield. Empty STRINGs remain fields.

`implode$` accepts exactly one MULTIFIELD, evaluates that operand once, and
returns a STRING. Fields are separated by one space without outer parentheses.
An empty multifield returns an empty STRING; an empty STRING field contributes
`""`. STRING fields have surrounding quotes, and embedded quotes and
backslashes receive a preceding backslash. Literal control characters and
raw bytes remain unchanged. SYMBOL spellings, including `crlf`, `tab`,
`vtab`, and `ff`, remain literal names. INSTANCE-NAME values use bracketed
raw name bytes. Scalar operands produce a type error; the argument-count
check precedes operand evaluation.

INTEGERs retain their exact decimal spelling. FLOATs use direct output's
15-significant-digit representation, including `-0.0`, scientific notation,
and its nonfinite spellings. These field rules apply to the supplied slice or
capture and leave the input values unchanged. The separate `str-cat`,
`sym-cat`, `format`, and `save-facts` formatters retain their existing behavior.

`explode$` and `str-explode` can read the quoted STRING fields back into a
MULTIFIELD. The reviewed round-trip cases preserve empty, numeric-looking,
and bracket-looking STRINGs, escaped quotes and backslashes, scannable SYMBOLs
and INSTANCE-NAMEs, exact INTEGERs, and selected FLOATs such as `1.25` and
`-0.0`. Coverage includes actual field types and bytes, normal and late rule
installation, and all five snapshot formats. For example,
`(explode$ (implode$ (create$ a "two words" 3)))` returns `(a "two words" 3)`.

These cases do not establish a general source serialization contract.
Arbitrary SYMBOL spellings may scan as another type or multiple fields;
INSTANCE-NAME payloads also need a valid scanner spelling. FLOAT formatting
can round values beyond 15 significant digits, so arbitrary f64 bits need
not survive. Length-bearing host values retain NUL and invalid UTF8 bytes
in the imploded result, but scanning stops at the first NUL byte. Typed
FACT-ADDRESS print forms and opaque host addresses retain the direct-output
boundaries below; INTEGERs are never reinterpreted as addresses.

#### Predicate sorting

`(sort <predicate> <value>...)` accepts an unqualified comparator `SYMBOL`
followed by zero or more values. Scalar values and the fields of multifield
arguments form one sequence in argument order. The result is always a
multifield on the normal sorting path, including empty and singleton inputs:

```clp
(sort > 3 (create$ 1 4) 2)  ; (1 2 3 4)
(sort < (create$ 3 1 2))    ; (3 2 1)
(sort >)                   ; ()
```

The comparator is called with the current left and right fields. Only the
actual symbol `FALSE` keeps the left field first; every other return value
selects the right field first. Consequently, `>` sorts numbers in ascending
order and `<` sorts them in descending order. A user-defined predicate has
the same direction as the equivalent builtin. `0`, `0.0`, `nil`, an empty
string or multifield, and a Void predicate result all count as true for sort;
this rule is specific to sorting.

Sorting is stable when the predicate returns `FALSE` for equal keys. It uses
merge traversal: split an odd-sized sequence with the larger half on the left,
sort the left half and then the right half, compare each pair of current heads
once, and append the unconsumed remainder. Comparator effects therefore have
a defined order; sort does not make a second, reversed-argument call to decide
whether two fields are equal. The selected fields retain their runtime types
and values, including equal numeric values with different INTEGER/FLOAT types.

The comparator expression is evaluated once, before the data expressions.
Name resolution and builtin/deffunction arity checks also precede data
execution. Supported builtins, visible deffunctions and generics can be used;
unqualified names follow the caller's module visibility. Explicit qualified
comparator names such as `M::compare` are rejected. Generic method applicability
is checked against the actual pair only when comparison is needed. Thus empty
and singleton data do not invoke the comparator, although its name must still
resolve. Data expressions run once, from left to right, before comparisons.

**Diagnostics and control.** An unavailable comparator name or an incompatible
builtin/deffunction arity returns actual `FALSE`, skips data expressions and
records a nonfatal `action_diagnostics()` entry. Later actions and activations
can still run. A non-SYMBOL name, failed source expression, or predicate error
is fatal instead. Effects already performed remain observable, and the run
reports `HaltReason::ActionError`.

A fatal error and its returned value are distinct. Once sorting has begun,
CLIPS can finish merging with the values returned by failed or skipped calls;
the resulting value may be stored by an enclosing assignment or assertion
before later rule actions stop. Multifield construction can substitute an empty
multifield while an evaluation error remains set. Error values depend on the called function: a failed user
function returns `FALSE`, while numeric builtins may retain a numeric default
or partial result. Reached builtin expressions may still execute; later user
function bodies do not. Enclosing builtins also differ in whether they accept
a value while an evaluation error remains set, so a partial result is not
proof of successful evaluation. Using `return` as a comparator is separate:
it can end the current rule after sort supplies its result, while other
activations remain eligible to run.

**Compatibility boundary.** These semantics apply to Ferric's supported
runtime value representations and callable implementations. Instance-name
values and byte lexemes are supported by the value layer; unrelated builtin
and qualified-declaration limitations remain. The pinned
CLIPS 6.30 process faults when Void is used as a sort data field; that case has
no supported returned-value contract and is not a required process fault in
Ferric. A Void *predicate result* is distinct and is supported as described
above. Callable-local binding and special-form invocation also retain their
existing limits: the CLIPS-valid `(sort bind c b a)` callback is currently
unsupported because it requires local binding through comparator dispatch.
This is separate from malformed source `bind` targets, which must retain their
ordinary variable form. Comparator arity metadata alone does not establish
full compatibility for every builtin or special form.

`nth$` and its `nth` alias preserve the selected field's type and return the
lowercase symbol `nil` for zero, negative, or excessive positions, including an
empty multifield. After validating the numeric index, they evaluate and validate
the multifield even when the position is absent. Runtime FLOAT indices truncate
toward zero. CLIPS separately rejects literal FLOAT indices during source
validation; Ferric's runtime conversion does not implement that static check.

### Fact Introspection Functions

| Function | Description | Example |
|----------|-------------|---------|
| `fact-existp` | Check if fact index is live | `(fact-existp 1)` => `TRUE` |
| `fact-index` | Extract integer index from fact address | `(fact-index 1)` => `1` |
| `fact-relation` | Get relation name as symbol | `(fact-relation 1)` => `person` |
| `fact-slot-value` | Get named slot value | `(fact-slot-value 1 name)` => `"Alice"` |
| `fact-slot-names` | Get slot names as multifield | `(fact-slot-names 1)` => `(name age)` |

### I/O Functions

| Function | Description |
|----------|-------------|
| `printout` | Write to a named channel |
| `format` | Printf-style formatting (returns string; does not write to router) |
| `read` | Scan the first field of a queued input line |
| `readline` | Return the next queued input line unchanged |
| `load-facts` | Load facts from a `.fct` file into working memory |
| `save-facts` | Save all facts to a `.fct` file |

`printout` writes a top-level STRING without surrounding quotes. A MULTIFIELD
uses parentheses and one space between fields, with STRING fields surrounded
by quotes: `(printout t (create$ "a" "two words") crlf)` writes
`("a" "two words")` followed by a newline. An empty multifield writes `()`;
an empty STRING field writes `""`. These rules apply to RHS output and output
from deffunctions and methods. Ferric's RHS `println` uses the same rendering
and appends a newline.

Quotes and backslashes inside printed STRING fields remain literal; printing
does not escape them. Literal control characters and raw bytes also remain
unchanged. `implode$` uses the escaped STRING-field mode described above;
direct printing retains raw embedded quotes and backslashes. SYMBOL fields
retain their spelling. Only top-level SYMBOL operands `crlf`, `tab`, `vtab`, and `ff`
expand to LF, TAB, VT, and FF; the same symbols inside a multifield remain
literal names.

Direct output renders FLOATs with up to 15 significant decimal digits, using
fixed notation for rounded decimal exponents from -4 through 14 and scientific
notation otherwise. Integral fixed-form FLOATs include `.0`; for example,
`1.0`, `-0.0`, `1e-05`, and `1e+15`. Nonfinite spellings are `nan.0`, `inf.0`,
and `-inf.0`. INTEGERs retain exact decimal spelling, including values above
2^53. This output formatter leaves `str-cat`, `sym-cat`, `format`, and
`save-facts` formatting unchanged.

STRING and SYMBOL payloads retain every byte, including NUL and invalid UTF8.
INSTANCE-NAME values print as bracketed raw name bytes, both at the top level
and inside multifields. General source round-tripping is a separate contract.
Typed FACT-ADDRESS print forms remain a representation gap; ordinary INTEGERs
are never interpreted as addresses while printing. Host ExternalAddress
values retain Ferric's opaque placeholder.

#### Queued input

`read` and `readline` share the lines supplied through `Engine::push_input`.
Each accepts zero or one input name, evaluated once when present; `t`, `T`,
and `stdin` select the same queue. `read` skips blank or comment-only lines,
returns the first token of the selected line, and discards its remaining
text. `readline` returns the next complete line unchanged, including an
empty line. The host supplies already-framed lines; `push_input` does not
split or normalize CR/LF sequences.

`read` preserves INTEGER, FLOAT, STRING, SYMBOL, and INSTANCE-NAME identity.
For example, input `"two words"` returns the STRING `two words`. Variable
and punctuation tokens return STRING print forms. An exhausted queue
returns the SYMBOL `EOF`; an unknown scanner token instead returns the
STRING `*** READ ERROR ***` without a diagnostic. Integer overflow and
incomplete quotes return their clamped or partial values with nonfatal
scanner notices, including through the `werror` channel. Quoted strings
retain CLIPS escapes and exact bytes, including an invalid UTF-8 `0xff`
when a trailing escape reaches EOF. Returned bytes follow the configured
encoding policy.

An unknown or invalid input name returns `*** READ ERROR ***` as a STRING,
records a diagnostic, and halts following actions without consuming input.
After resolving a valid input name, an already halted evaluation also
returns that STRING without consuming a queued line or adding a diagnostic.
This is Ferric's framed-input policy: CLIPS can consume one byte before
checking halt, which this line queue cannot represent. An evaluation error
without a halt does not by itself prevent a read.

Reset preserves unread input; clear discards it. Snapshots preserve the
remaining queue with the existing schema. Named-file input and `open` are
unsupported; the queued-line behavior does not imply a persistent named
stream or a raw stdin API.

#### Formatting

`format` returns a STRING and does not write to the requested router. Use
`(printout t (format nil "n=%d" 42) crlf)` to produce output.

The supported data conversions are lowercase `d`, `o`, `x`, `u`, `f`, `e`,
`g`, `s`, and `c`. Canonical directives accept leading `-` and `0` flags,
a decimal minimum width, and optional `.precision`. Width never truncates
content. `-` selects left alignment with trailing spaces and overrides `0`;
otherwise numeric zero padding follows any minus sign. Repeated leading
flags are accepted.

| Conversion       | Operand and rendering                                                                                                                                                                          |
| ---------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `%d`             | INTEGER or FLOAT, rendered as signed decimal.                                                                                                                                                  |
| `%o`, `%x`, `%u` | INTEGER or FLOAT, rendered as octal, lowercase hexadecimal, or unsigned decimal using the converted 64-bit integer's bit pattern.                                                              |
| `%f`             | INTEGER or FLOAT, fixed notation; precision counts fractional digits and defaults to 6.                                                                                                        |
| `%e`             | INTEGER or FLOAT, scientific notation; precision counts fractional digits and defaults to 6. The lowercase exponent has an explicit sign and at least two digits.                              |
| `%g`             | INTEGER or FLOAT, significant digits; precision defaults to 6, with explicit zero treated as 1. Rounded exponent selects fixed or scientific notation; trailing fractional zeroes are removed. |
| `%s`             | STRING, SYMBOL, or INSTANCE-NAME; names render without brackets. Precision limits bytes.                                                                                                       |
| `%c`             | INTEGER, STRING, or SYMBOL; renders the integer's low byte or the lexeme's first byte. FLOAT and INSTANCE-NAME are not accepted. Precision is ignored.                                         |

For integer conversions, finite in-range FLOAT values truncate toward zero;
INTEGER values retain their exact digits. Integer precision is a minimum
digit count and disables width zero padding. Zero precision with zero emits
no digits. Floating conversions preserve negative zero. `%g` uses fixed
notation when the rounded exponent is at least -4 and less than the
significant precision; it does not automatically append `.0`. Infinity and
NaN render as lowercase `inf` and `nan`, with spaces for width padding even
when `0` is present. `%s` and `%c` also use spaces for width padding.

Immediately following `%`, the no-data controls `n`, `r`, `t`, `v`, and `%`
emit newline, carriage return, tab, vertical tab, and a literal percent sign,
respectively. They consume no operand. An incomplete fragment such as `%12`
is literal text. A conversion can follow at most 73 modifier bytes; 74
modifier bytes finish a literal fragment instead.

Control strings stop at their first NUL byte. `%s` also stops at its
operand's first NUL. Width and string precision count raw bytes, so
precision and `%c` may return a partial UTF-8 sequence without replacing it.
An empty lexeme or an integer with a zero low byte gives `%c` a NUL: only
padding before that NUL survives that conversion, while later format
fragments still append. The returned bytes remain subject to the engine's
configured string encoding policy.

`format` requires a router argument and a STRING control argument. It checks
minimum arity before evaluating arguments, then evaluates router and control
in order. It validates the complete control prefix and exact data operand
count before evaluating any data. Each consumed data expression then runs
once, in order, and is checked before the next expression. Failure returns
an empty STRING, discards partial formatting, and retains prior operand
side effects. Invalid flags, argument counts, and control/numeric/`%s` type
errors record a diagnostic and halt subsequent rule actions. A `%c` type
error records a nonfatal diagnostic, returns an empty STRING, and skips
remaining format operands; following actions can run unless an earlier
error already requested a halt. `%c` preserves an inherited error's state;
it does not use the numeric/`%s` gate that stops on an existing evaluation
error.

**Ferric policies.** Each call has a private 16 MiB (16,777,216 byte) ceiling
for both the control prefix before NUL and the aggregate result. Oversized
requests fail without constructing the oversized result. FLOAT-to-integer
conversion follows Rust's saturating conversion: NaN becomes zero, values
beyond the signed 64-bit range and infinities become the corresponding
endpoint. These resource and conversion rules are engine policies, not
claims about undefined behavior in CLIPS's C formatting implementation.

The CLIPS format scanner also admits misplaced dot/minus modifiers that are
not canonical directives. Ferric still evaluates and checks their operand,
then emits a deterministic fragment: normalize the valid prefix, preserve
the remaining modifier bytes, and retain CLIPS's inserted `ll` before an
integer conversion. For example, `%..d` becomes `%.0.lld`. This algorithm
matches five pinned reference echoes; its application to other malformed
fragments is a portable Ferric policy, not universal C library compatibility.
Other printf extensions, including `+`, `#`, dynamic `*` widths, positional
arguments, length modifiers, and uppercase conversions, are unsupported.

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
| Truth maintenance (`logical` CE) | Explicitly rejected | Logical support is outside the current supported subset; no performance claim is implied |
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

Ferric provides a C-compatible FFI layer (`ferric-rules-ffi`) for embedding into
C, C++, Swift, Kotlin (NDK), and other languages with C FFI support.

### Engine Lifecycle

```c
#include "ferric.h"
#include <stdlib.h>

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

// Read output into caller-owned storage
size_t output_len = 0;
if (ferric_engine_get_output_copy(engine, "t", NULL, 0, &output_len) ==
    FERRIC_ERROR_OK) {
    char* output = malloc(output_len);
    if (output != NULL &&
        ferric_engine_get_output_copy(engine, "t", output, output_len,
                                      &output_len) == FERRIC_ERROR_OK) {
        // use output
    }
    free(output);
}

// Clean up
ferric_engine_free(engine);
```

### Logical Run Continuation

`ferric_engine_run_ex` always begins a *fresh logical run*: it clears any
pending halt request and the accumulated action diagnostics, but leaves working
memory, the agenda, globals, and captured output untouched. Hosts that split
one long run into bounded chunks — to poll for cancellation between them —
must not use repeated `ferric_engine_run_ex` calls for that, because each call
discards the halt flag and diagnostics that the previous chunk produced. A rule
set that halts on its 100th activation then fires 101 rules through a
100-activation chunk loop.

`ferric_engine_continue_run_ex` resumes the current logical run instead:

```c
uint64_t chunk_fired = 0, total_fired = 0;
FerricHaltReason reason;

if (ferric_engine_run_ex(engine, CHUNK, &chunk_fired, &reason) != FERRIC_ERROR_OK)
    return -1;
total_fired += chunk_fired;

while (reason == FERRIC_HALT_REASON_LIMIT_REACHED) {
    if (host_canceled())   // cancellation is the host's outcome, not the engine's
        break;
    if (ferric_engine_continue_run_ex(engine, CHUNK, &chunk_fired, &reason) !=
        FERRIC_ERROR_OK)
        return -1;
    total_fired += chunk_fired;
}
```

Contract:

- Continuation is legal only after `ferric_engine_run_ex` or a previous
  continuation returned `FERRIC_HALT_REASON_LIMIT_REACHED`. Any other call
  returns `FERRIC_ERROR_INVALID_ARGUMENT`, records a per-engine message, and
  leaves `*out_fired` and `*out_reason` unmodified.
- `FERRIC_HALT_REASON_AGENDA_EMPTY`, `FERRIC_HALT_REASON_HALT_REQUESTED`, and
  `FERRIC_HALT_REASON_ACTION_ERROR` are terminal and end continuation
  eligibility.
- `*out_fired` is that chunk's count, not a running total. Hosts accumulate it.
- Read-only queries and `ferric_engine_clear_error` may be interleaved between
  chunks. Any other call that reaches engine state ends the logical run,
  whether or not it then succeeds — a rejected `ferric_engine_retract` counts.
- A call rejected *before* it reaches engine state changes no runtime or
  continuation state, though it still publishes its documented error on the
  channels described under Error Handling. A null handle or an overlapping or
  reentrant call leaves the logical run intact for a later serialized
  continuation.
- Absent host cancellation, a chunked run and an equivalent one-shot run report
  the same total fired count, halt reason, agenda state, and action
  diagnostics.
- Host cancellation is not `FERRIC_HALT_REASON_HALT_REQUESTED`. The ABI has no
  canceled state: a canceling host stops submitting chunks, reports its own
  outcome, and starts any later logical run with `ferric_engine_run_ex`. The
  agenda is left intact.
- Cancellation does **not** guarantee an un-halted engine. The halt flag
  reflects whatever the chunks that did run executed. If a chunk landed exactly
  on an activation that called `(halt)`, that chunk still reports
  `FERRIC_HALT_REASON_LIMIT_REACHED` — the pending halt only surfaces as
  `HALT_REQUESTED` on the next chunk that can run — so a host canceling at that
  boundary leaves a halted engine. Query `ferric_engine_is_halted` if the
  distinction matters; `ferric_engine_run_ex` clears the flag either way when it
  starts the next logical run.
- Continuation eligibility is per-handle and is not serialized. A handle
  produced by `ferric_engine_deserialize_*` always begins a fresh logical run.

### Thread transfer and serialization

Rust `Engine` is structurally `Send + Sync`: ownership can transfer, shared
reads may run concurrently, and mutation requires exclusive access. A raw C
handle may move between OS threads for use and destruction, but the C host
must serialize all
runtime calls, including reads, and protect the allocation's lifetime:

- An atomic admission guard rejects overlapping runtime calls and same-engine
  reentry from a host callback with `FERRIC_ERROR_INTERNAL_ERROR`.
- `ferric_engine_last_error_copy` is separately synchronized and copies one
  coherent snapshot per call, including during callbacks or other calls.
- `ferric_engine_last_error` may be called from any thread. The host must
  protect use of its borrowed pointer against another borrowed error read or
  destruction. Prefer the copy API when those windows could overlap.
- Borrowed output strings must also be copied or consumed before another call
  can invalidate them; transfer does not extend their documented lifetime.
- Successful destruction must not overlap any access, including diagnostics
  and use of borrowed pointers. Admission is not a handle registry and cannot
  make a stale pointer safe. `ferric_engine_free_unchecked` remains an ABI
  compatibility alias with the same lifetime obligations as ordinary free.
- `FERRIC_ERROR_THREAD_VIOLATION` retains its numeric ABI value for compatibility;
  ordinary raw-handle calls no longer emit it based on the creating thread.

Global error functions use thread-local storage. Retrieve or copy a global
error on the same OS thread as the failing call, before another call can
replace it. A transferred handle carries its per-engine error snapshot, not
the previous thread's global error. Bindings should copy output and errors
before releasing the host-side protection that makes them valid.

### Error Handling

Two error channels exist:

1. **Per-engine errors**: `ferric_engine_last_error()` /
   `ferric_engine_last_error_copy()` / `ferric_engine_clear_error()`
2. **Global (thread-local) errors**: `ferric_last_error_global()` /
   `ferric_last_error_global_copy()` / `ferric_clear_error_global()`

Failures involving a validated raw-engine handle publish the same current
message to both that engine's snapshot and the calling thread's global
fallback. Failures before handle validation (for example, creation or
null-handle errors) update only the global channel. Engine snapshots are
independent: an operation on one engine does not overwrite another engine's
message.

Bindings should prefer the per-engine channel for engine operations and use the
global channel as a fallback or for pre-engine failures.

### Embedded-NUL String Policy

Ferric strings, symbols, and instance names can preserve arbitrary bytes.
Legacy C text entry points still use NUL-terminated UTF-8 strings; explicit raw
constructors and appended transport tags carry byte spans without changing the
`FerricValue` layout:

- A legacy `const char *` input ends at its first NUL by definition; bytes
  after it are not part of the C string. Bindings starting from a
  pointer-plus-length string must reject embedded NUL before calling any such
  entry point.
- `ferric_value_symbol_bytes` and `ferric_value_string_bytes` accept an
  explicit UTF-8 byte span and return `FERRIC_ERROR_INVALID_ARGUMENT` if it
  contains embedded NUL. Their output remains Void on failure.
- `ferric_value_string_raw`, `ferric_value_symbol_raw`, and
  `ferric_value_instance_name` copy arbitrary bytes. Their transport tags are
  `STRING_BYTES` (7), `SYMBOL_BYTES` (8), and `INSTANCE_NAME` (9).
- Fact-field, global, and named-slot queries retain the legacy String/Symbol
  tags for valid UTF-8 without NUL. Other strings and symbols use their byte
  tags, recursively inside multifields. Instance names always use tag 9;
  their payload excludes brackets. Byte tags use `string_ptr` together with
  `multifield_len` as the byte count. Free them with `ferric_value_free`,
  never `ferric_string_free`.
- `ferric_engine_get_output` returns NULL and records
  `FERRIC_ERROR_INVALID_ARGUMENT` when captured output contains embedded NUL
  or invalid UTF-8. Use `ferric_engine_get_output_copy` for exact access.
- Length-reporting copy APIs preserve every source byte, including embedded
  NUL. Their reported length includes one additional trailing terminator, so
  callers must use `out_len` rather than `strlen`.
- Snapshot serialization/deserialization APIs are byte-oriented and preserve
  their serialized bytes exactly, including byte lexemes and instance names.
  Restored values use the same lossless byte-span egress.

The Go binding rejects embedded NUL before every legacy `C.CString`
conversion. Rejections return `ErrInvalidArgument` through error-bearing APIs;
their diagnostic identifies the offending argument and the byte offset of the
first NUL. `Engine.GetOutputE`, `Engine.ClearOutputE`, and
`Engine.PushInputE` (and their `PinnedEngine` counterparts) expose these
errors for I/O arguments. The older convenience methods delegate to those
error-aware methods and intentionally discard the error for compatibility;
an invalid channel never aliases its prefix, and invalid input is not queued.

Snapshot payloads remain byte-oriented and may contain NUL. Snapshot file
paths are handled by Go's filesystem APIs rather than `C.CString`; invalid
paths therefore retain the platform's `os.PathError` behavior.

### Copy-to-Buffer Contract

The `*_copy` functions follow a uniform contract:

| Condition | Return Code | `*out_len` |
|-----------|-------------|------------|
| No source value (for example, no error or no channel output) | `FERRIC_ERROR_NOT_FOUND` | 0 |
| `out_len` is NULL | `FERRIC_ERROR_INVALID_ARGUMENT` | (not written) |
| `buf` is NULL, `buf_len` is 0 (size query) | `FERRIC_ERROR_OK` | Required size (incl. NUL) |
| `buf` non-null, `buf_len` >= needed | `FERRIC_ERROR_OK` | Bytes written (incl. NUL) |
| `buf` non-null, `buf_len` < needed | `FERRIC_ERROR_BUFFER_TOO_SMALL` | Full needed size (incl. NUL) |

On truncation, the buffer receives `buf_len - 1` bytes followed by a NUL
terminator and the function returns `FERRIC_ERROR_BUFFER_TOO_SMALL`; truncation
is never reported as success. `ferric_engine_get_output_copy` follows this
contract and is the preferred output accessor for caller-owned storage. The
reported length is authoritative even when the copied payload contains an
embedded NUL.

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

Rule-action evaluation failures are collected as action diagnostics, distinct
from API/ABI failures. They do not invalidate the engine, but they stop the
current activation and `run()` as described above.

This list also includes nonfatal `sort` comparator-name and arity diagnostics.
Such a diagnostic accompanies a `FALSE` return and allows later actions to
continue. Inspect the run's halt reason to distinguish a warning from a fatal
action failure; a nonempty diagnostic list alone does not make that distinction.

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
| `ferric_value_multifield_copy` | Deep-copy a borrowed `FerricValue` array and its nested tree into one Ferric-owned multifield; external-address payload pointers remain caller-owned |
| `ferric_value_free` | Free a `FerricValue` and its owned resources (recursive); returns `FERRIC_ERROR_INVALID_ARGUMENT` if any `value_type` tag is unknown (that value's payload is left untouched; known siblings and owned arrays are still freed) |
| `ferric_value_array_free` | Free an array of `FerricValue`s; same unknown-tag contract as `ferric_value_free` |

Structured values passed to assertion APIs and
`ferric_value_multifield_copy` are borrowed for the call: Ferric never retains
or frees their strings or arrays. The copy constructor returns an independent
Ferric-owned tree that must be released with `ferric_value_free`; only
Ferric-owned trees may be passed to Ferric value cleanup APIs. External-address
payload pointers are shallow and caller-owned in this legacy copy helper.
Engine assertion and value-conversion APIs reject `ExternalAddress`; copying
this field does not create a transferable Rust host token. Multifield-copy inputs
must be acyclic and no deeper than 128 nested multifield levels.

Other borrowed pointers must **not** be freed by the caller.
`ferric_engine_last_error` remains valid until the next borrowed last-error
read on that engine or engine destruction; error writers and the copy API do
not invalidate it. `ferric_engine_get_output` returns a per-engine,
per-channel snapshot that remains valid until a later borrowed read for the
same engine and channel replaces it; that channel is cleared; the engine is
reset or cleared; or until engine destruction. Reads on other engines do not
invalidate it, and later output writes are not reflected in an existing
snapshot. Use `ferric_engine_get_output_copy` when the caller should own the
bytes.

### Panic Policy

Every public C function is generated as a small `extern "C"` wrapper around a
non-extern Rust implementation. The `ffi-dev` and `ffi-release` profiles retain
unwind support, and each wrapper catches ordinary Rust panics before they can
reach the C ABI.

A contained panic records a stable message naming the export, without
formatting or downcasting the panic payload. The calling thread's global error
channel is always updated; a supplied live raw or pinned engine also receives
the same per-engine message. Ownership-consuming free functions update only
the global channel because a panic can make the handle's remaining lifetime
indeterminate.

Return sentinels are fixed by category:

| Return category | Panic sentinel |
|-----------------|----------------|
| `FerricError` | `FERRIC_ERROR_INTERNAL_ERROR` |
| Any pointer | NULL |
| `FerricValue` | Void |
| `bool` | false |
| Integer/count | 0 |
| `void` | Return after recording the diagnostic |

An async submission-wrapper panic is a synchronous rejection: it returns
`FERRIC_ERROR_INTERNAL_ERROR` and does not invoke the completion callback.
After a pinned async submission returns `FERRIC_ERROR_OK`, an ordinary Rust
panic while executing that accepted request is instead a terminal asynchronous
result: the registry entry is removed, the callback fires exactly once with
`FERRIC_ERROR_INTERNAL_ERROR`, and later work continues on the same worker.
Registry cleanup happens before callback invocation, so the completed
`request_id` is reusable at that point.
The terminal diagnostic is carried by the result handle rather than written to
the global or per-engine last-error channel. Although the worker remains
available, the panic may have left logical engine state partially updated;
consumers that require a known state should reset or recreate the engine before
relying on later results.
Containment does not cover non-unwinding termination such as allocator
abort/OOM or an explicit process abort. Foreign callbacks must still return
normally and obey their own no-unwind contract.

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

### Template type declarations

Primitive template slot unions (`SYMBOL`, `STRING`, `INTEGER`, `FLOAT`,
`NUMBER`, `LEXEME`, `EXTERNAL-ADDRESS`) are retained and checked. Default values,
seed facts, and literal rule assertions are validated before their construct is
installed; runtime values and host template assertions are always validated.
Unlike CLIPS 6.30 with its default dynamic checking disabled, Ferric rejects
runtime values that violate a declared slot type. See
[the migration notes](migration.md#primitive-template-slot-types) for default
priority, external token handling, and explicit unsupported optional attributes.
