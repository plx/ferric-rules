# ferric-rules Compatibility Roadmap

Ordered list of CLIPS compatibility deficits, by descending priority (files
unblocked × implementation effort). Based on bulk assessment of 5,802 `.clp`
files from 16 sources — see `tests/examples/compat-manifest.json` for full
data and `scripts/compat-{scan,run,report}.py` for the tooling.

Assessment date: 2026-02-23 (ferric @ commit 0adf176).

---

## 1. Bracket Characters in Symbol Names

| | |
|---|---|
| **Files blocked** | 628 |
| **Effort** | Small — lexer-only change |
| **Error** | `parse error: unexpected character: '['` |

### Summary

CLIPS allows `[` and `]` as valid characters within symbol names. The ferric
lexer's `is_symbol_char()` function does not include them, causing a parse
error on any symbol containing brackets. This is by far the single largest
blocker — fixing it unblocks 628 files, overwhelmingly from the CSP-Rules
project which uses brackets as a parameterized naming convention.

### What needs to change

Add `'['` and `']'` to `is_symbol_char()` in
`crates/ferric-parser/src/lexer.rs` (around line 476). The existing function
already allows `{` and `}`, so this is a natural extension.

### CLIPS examples

```clips
;; Rule names with numeric parameters
(defrule activate-template[1]
    (declare (salience ?*activate-template-1-salience*))
    ...
)

;; Global variable names with brackets
(declare (salience ?*partial-OR2-gwhip[10]-salience-1*))

;; Fact references with brackets
(assert (technique ?cont template[3]))
```

### Test files

- `tests/examples/csp-rules-v2.1/SudoRules-V20.1/TEMPLATES/Templates[1].clp`
- `tests/examples/csp-rules-v2.1/SlitherRules-V2.1/XTD-LOOPS/xtd-loops[20].clp`
- `tests/examples/csp-rules-v2.1/CSP-Rules-Generic/CHAIN-RULES-EXOTIC/PARTIAL-OR2-G-WHIPS/Partial-OR2-gWhips[10].clp`

---

## 2. Connective Constraints (`&`, `|`, `~`)

| | |
|---|---|
| **Files blocked** | 70 (direct) + many of the 628 bracket files also use connectives |
| **Effort** | Medium — Stage 2 parser constraint interpreter |
| **Error** | `interpret error: invalid bare connective in pattern (use in constraint expression)` |

### Summary

CLIPS connective constraints allow combining conditions within a single
pattern field using `&` (AND), `|` (OR), and `~` (NOT). The ferric Stage 1
parser correctly tokenizes these as `Atom::Connective`, but Stage 2 does not
yet combine them with adjacent operands into constraint expressions. The code
in `crates/ferric-parser/src/stage2.rs` (around line 1634) simply rejects
bare connectives.

### What needs to change

The Stage 2 `interpret_constraint()` function needs a look-ahead pass that
combines connective tokens with their operands into compound constraint
nodes. For example, the token sequence `?x`, `&`, `~`, `red` should produce
`Constraint::And(Var("x"), Constraint::Not(Literal("red")))`. The
implementation plan already notes this as a planned follow-up
(`documents/plans/phases/002/Notes.md`, line 256).

### CLIPS examples

```clips
;; NOT constraint: match anything except "red"
(color ~red)

;; AND + NOT: bind ?x, require it's not equal to ?zzz
(label ?new-llc&~?zzz)

;; OR constraint: match either value
(type partial-whip|partial-braid)

;; Compound: bind, AND NOT, AND inline test
(label ?new-llc&~?zzz&:(not (member$ ?new-llc $?rlcs)))

;; From zebra.clp: variable AND equality constraint
(avh (a color) (v red) (h ?c1&?n1))
```

### Test files

- `tests/examples/clips-official/examples/zebra.clp` (also needs `field` alias, see item 5)
- `tests/examples/clips-official/examples/wordgame.clp`
- `tests/examples/clips-official/examples/mab.clp`
- `tests/examples/clips-official/test_suite/mfvmatch.clp`
- `tests/examples/csp-rules-v2.1/CSP-Rules-Generic/CHAIN-RULES-COMMON/BRAIDS/Braids[4].clp` (also needs bracket support)

---

## 3. Implicit `(initial-fact)` for Empty Rule Patterns

| | |
|---|---|
| **Files blocked** | 34 |
| **Effort** | Small — compiler adjustment |
| **Error** | `compile error: rule has no patterns` |

### Summary

In CLIPS, a rule with no LHS patterns (or only `(test ...)` CEs) implicitly
matches the `(initial-fact)` fact, which is asserted by `(reset)`. The ferric
compiler in `crates/ferric-core/src/compiler.rs` (line 149) rejects rules
with empty pattern lists via `ensure_non_empty()`. This blocks classic
patterns like unconditional startup rules and test-only rules.

### What needs to change

When a rule has no patterns, automatically insert a pattern matching the
`(initial-fact)` ordered fact. The `(reset)` command must also assert
`(initial-fact)` into working memory (check if it already does). The
waltz benchmark files — a classic CLIPS benchmark — are blocked by this.

### CLIPS examples

```clips
;; Unconditional startup rule — fires once after (reset)
(defrule hello-world
=>
  (println "hello world")
)

;; Startup rule that asserts initial data
(defrule startup
   =>
   (assert (stage (value duplicate))
           (line (p1 50003) (p2 60003))))

;; Test-only rule — also needs implicit (initial-fact)
(defrule should-fire-2a
  (declare (salience 2))
  (test (> 5 3))
  =>)
```

### Test files

- `tests/examples/clips-executive/cx_tutorial_agents/clips/hello_world.clp` — simplest possible case
- `tests/examples/clips-official/examples/waltz/waltz12.clp` — classic benchmark
- `tests/examples/clips-official/test_suite/pataddtn.clp` — CLIPS official test suite for this behavior

---

## 4. `(and ...)` Conditional Element

| | |
|---|---|
| **Files blocked** | 8 (direct) + others that combine `and` with `not` |
| **Effort** | Medium — compiler pattern handling |
| **Error** | `compile error: unsupported pattern form 'and'` |

### Summary

CLIPS allows `(and ...)` as a conditional element to group patterns, most
commonly inside `(not (and ...))` to express "it is not the case that both A
and B are true." The ferric loader in `crates/ferric-runtime/src/loader.rs`
(line 1086) explicitly rejects `Pattern::And`. Standalone `(and ...)` at top
level is equivalent to listing patterns directly, but `(not (and ...))` is
semantically distinct and cannot be decomposed.

### What needs to change

Support `(and ...)` in two contexts:
1. **Top-level**: Flatten into the rule's pattern list (trivial).
2. **Inside `(not ...)`**: Compile as a conjunctive negation in the Rete
   network — this requires joining the sub-patterns before the NOT node.

### CLIPS examples

```clips
;; Negated conjunction: fire when it's NOT the case that both b-1 and c-1 exist
(defrule rule-1-1 "+j+j+j+j+j+j"
  (declare (salience 20))
  (a-1)
  (not (and (b-1) (c-1)))
  (d-1)
  =>)

;; Standalone and (equivalent to listing patterns directly)
(defrule should-fire-3a
  (declare (salience 4))
  (and (test (> 5 3)))
  =>)

;; And with initial-fact
(defrule should-fire-3b
  (declare (salience 5))
  (and (initial-fact)
       (test (> 5 3)))
  =>)
```

### Test files

- `tests/examples/clips-official/test_suite/joinshre.clp` — shared join tests
- `tests/examples/clips-official/test_suite/pataddtn.clp` — pattern addition tests
- `tests/examples/clips-official/test_suite/tceplace.clp`

---

## 5. `(field ...)` as Alias for `(slot ...)`

| | |
|---|---|
| **Files blocked** | 11 |
| **Effort** | Tiny — one-line parser change |
| **Error** | `interpret error: expected 'slot' or 'multislot'` |

### Summary

CLIPS 6.0 used `(field ...)` as the keyword for defining template slots.
Later versions renamed it to `(slot ...)` but maintained backward
compatibility. The ferric parser in `crates/ferric-parser/src/stage2.rs`
(around line 1739) only recognizes `slot` and `multislot`. Files from the
CLIPS 6.0 era (including the classic `zebra.clp` puzzle) use `field`.

### What needs to change

Add `"field"` as a match arm alongside `"slot"` in
`interpret_slot_definition()` mapping to `SlotType::Single`. Optionally also
add `"multifield"` mapping to `SlotType::Multi` for completeness.

### CLIPS examples

```clips
;; CLIPS 6.0 style — uses (field ...) instead of (slot ...)
(deftemplate avh (field a) (field v) (field h))

;; Modern equivalent:
(deftemplate avh (slot a) (slot v) (slot h))
```

### Test files

- `tests/examples/clips-official/examples/zebra.clp` — the classic "Who owns the Zebra?" puzzle
- `tests/examples/clips-official/test_suite/zebra.clp`
- `tests/examples/clips-official/test_suite/fctpcstr.clp`

---

## 6. Complex Constraint Expressions (Nested Lists in Constraints)

| | |
|---|---|
| **Files blocked** | 5 |
| **Effort** | Medium — extends constraint interpreter |
| **Error** | `interpret error: complex constraint expressions not yet supported` |

### Summary

Some CLIPS patterns use nested s-expressions within constraint positions
(e.g., function calls or computed constraints). The ferric Stage 2
`interpret_constraint()` in `crates/ferric-parser/src/stage2.rs` (line 1584)
rejects any list-form expression in a constraint position. Fixing connective
constraints (item 2) will likely cover most of these, as the "complex
constraint" error is often a cascade from connective parsing failure.

### What needs to change

Allow list expressions in constraint positions to be interpreted as function
calls or predicate constraints. This partially overlaps with connective
constraint support.

### CLIPS examples

```clips
;; Inline predicate constraint — function call as constraint
(candidate (context ?cont) (status cand) (label ?cand))

;; Computed constraint (less common)
(slot-value =(compute-expected ?x))
```

### Test files

- `tests/examples/csp-rules-v2.1/CSP-Rules-Generic/GENERAL/is-cspvar-for-cand.clp`
- `tests/examples/csp-rules-v2.1/CSP-Rules-Generic/CHAIN-RULES-EXOTIC/SYMMETRIFY-ORk/symmetrify-OR2-relations.clp`
- `tests/examples/csp-rules-v2.1/SlitherRules-V2.1/SPECIFIC/W1-Equiv/detect-diagonal-2s.clp`

---

## 7. `do-for-fact` / `do-for-all-facts` Action Macros

| | |
|---|---|
| **Files blocked** | 8 |
| **Effort** | Medium — parser + runtime |
| **Error** | `interpret error: expected function name (symbol)` |

### Summary

CLIPS provides query macros like `(do-for-fact ...)`,
`(do-for-all-facts ...)`, and `(delayed-do-for-all-facts ...)` that iterate
over facts matching a pattern. These have a distinctive syntax where the
first argument is a variable-binding list `((?var template))`, not a function
name. The ferric parser's `interpret_function_call()` in
`crates/ferric-parser/src/stage2.rs` (line 1663) expects the first element
after `(` to be a symbol, so it rejects the binding-list syntax.

### What needs to change

Add special-case parsing for `do-for-fact`, `do-for-all-facts`,
`delayed-do-for-all-facts`, `any-factp`, `find-fact`, and
`find-all-facts`. These need their own AST node type that captures the
variable binding, the query condition, and the action body.

### CLIPS examples

```clips
;; Iterate over all matching facts and perform an action
(do-for-fact ((?c counter)) TRUE
  (modify ?c (iteration (+ 1 ?c:iteration))))

;; Query with delayed cleanup
(delayed-do-for-all-facts ((?cv confval))
  (ppfact ?cv blue))
```

### Test files

- `tests/examples/clips-executive/cx_tutorial_agents/clips/tf2_tracked_pose.clp`
- `tests/examples/clips-executive/cx_bringup/clips/plugin_examples/config.clp`
- `tests/examples/fawkes-robotics/src/plugins/clips-executive/clips/wm-config.clp`
- `tests/examples/rcll-refbox/src/games/rcll/setup.clp`

---

## 8. Control Flow: `if`/`then`/`else`

| | |
|---|---|
| **Files blocked** | ~4,370 (pre-classified, not yet run through ferric) |
| **Effort** | Medium-Large — parser + runtime + evaluator |
| **Pre-classification reason** | `unsupported-control` |

### Summary

This is the largest single category of incompatibility. The `if/then/else`
construct appears in 4,370 of the 5,802 files (75%). It is used pervasively
in rule RHS actions for conditional logic. The ferric parser grammar already
lists `if` as a valid action name, but the runtime action executor in
`crates/ferric-runtime/src/actions.rs` does not handle it — unknown actions
fall through to expression evaluation, which fails.

### What needs to change

1. **Parser**: Add an `If` variant to `ActionExpr` with condition, then-branch,
   and optional else-branch.
2. **Runtime**: Add `if` handling to the action executor. The evaluator already
   has `is_truthy()` support (in `evaluator.rs`).
3. **Syntax**: CLIPS `if` uses `then` and `else` as delimiters:
   `(if <condition> then <actions> else <actions>)`

### CLIPS examples

```clips
;; Basic if/then/else in rule action
(defrule check-value
   (data (value ?v))
   =>
   (if (> ?v 100)
      then (printout t "High: " ?v crlf)
      else (printout t "Low: " ?v crlf)))

;; Nested if
(if (eq ?type "A")
   then (assert (category A))
   else (if (eq ?type "B")
            then (assert (category B))
            else (assert (category unknown))))

;; if without else
(if ?*print-main-levels*
   then (printout t "entering level T1"))
```

### Test files

After implementing, re-run the scanner to reclassify:
```
python scripts/compat-scan.py && python scripts/compat-run.py --all
```

Files that use `if` but no other unsupported features will become testable.
As a starting point:
- `tests/examples/csp-rules-v2.1/SudoRules-V20.1/TEMPLATES/Templates[1].clp` (also needs bracket + connective support)
- `tests/examples/clips-official/examples/sudoku/sudoku.clp`
- `tests/examples/small-clips-examples/elevator.clp`

---

## 9. Control Flow: `while`, `loop-for-count`, `foreach`/`progn$`

| | |
|---|---|
| **Files blocked** | Subset of the 3,774 `unsupported-control` files |
| **Effort** | Medium — builds on `if` infrastructure |
| **Pre-classification reason** | `unsupported-control` |

### Summary

After `if`, the remaining control flow constructs are loop forms.
`loop-for-count` is an indexed loop, `while` is a conditional loop, and
`foreach`/`progn$` iterate over multifield values. These are less pervasive
than `if` but still appear in hundreds of files.

### What needs to change

Each loop form needs an AST variant, parser support, and runtime execution
with proper variable scoping and termination semantics.

### CLIPS examples

```clips
;; Indexed loop
(loop-for-count (?i 1 10) do
   (printout t ?i crlf))

;; While loop
(while (> ?count 0) do
   (printout t ?count crlf)
   (bind ?count (- ?count 1)))

;; Foreach / progn$ — iterate multifield
(progn$ (?item $?list)
   (printout t ?item crlf))
```

### Test files

- `tests/examples/small-clips-examples/elevator.clp` (uses `while`)
- Re-run `python scripts/compat-run.py --all` after implementing

---

## 10. Compile-Time Function Validation

| | |
|---|---|
| **Files affected** | 142 (classified `clips-load-error`) |
| **Effort** | Medium — loader validation pass |
| **Behavioral difference** | Ferric silently accepts; CLIPS rejects with `[EXPRNPSR3]` |

### Summary

CLIPS validates at load time that every function call in a rule action
references a known function (built-in, deffunction, or defgeneric). Ferric
currently defers all function validation to runtime — calls to undefined
functions in `crates/ferric-runtime/src/loader.rs` (line 902) are silently
swallowed with `.ok()`. This means ferric accepts files that CLIPS rejects.

This is not a blocker (these files can't run correctly in either engine
standalone) but it is a behavioral divergence from CLIPS. For drop-in
compatibility, ferric should emit the same error.

### What needs to change

After loading all constructs from a file, perform a validation pass over
rule actions checking that every function reference resolves to a known
built-in, deffunction, or defgeneric. Emit a diagnostic matching CLIPS's
`[EXPRNPSR3] Missing function declaration for <name>`.

### CLIPS behavior

```
CLIPS> (defrule test (foo) => (nonexistent-fn))
[EXPRNPSR3] Missing function declaration for nonexistent-fn.
```

### Test files

- `tests/examples/clips-official/examples/sudoku/puzzles/grid2x2-p1.clp` — references `row` (defined in separate `sudoku.clp`)
- `tests/examples/clips-executive/cx_bringup/clips/plugin_examples/executive.clp` — references `now` (plugin function)
- `tests/examples/clips-executive/cx_plugins/protobuf_plugin/clips/protobuf.clp` — references `pb-destroy` (plugin function)

---

## Summary Table

| # | Issue | Files | Effort | Ferric Source |
|---|-------|------:|--------|---------------|
| 1 | Bracket `[`/`]` in symbols | 628 | Small | `ferric-parser/src/lexer.rs:476` |
| 2 | Connective constraints | 70+ | Medium | `ferric-parser/src/stage2.rs:1634` |
| 3 | Implicit `(initial-fact)` | 34 | Small | `ferric-core/src/compiler.rs:149` |
| 4 | `(and ...)` conditional element | 8 | Medium | `ferric-runtime/src/loader.rs:1086` |
| 5 | `(field ...)` slot alias | 11 | Tiny | `ferric-parser/src/stage2.rs:1739` |
| 6 | Complex constraint expressions | 5 | Medium | `ferric-parser/src/stage2.rs:1584` |
| 7 | `do-for-fact` query macros | 8 | Medium | `ferric-parser/src/stage2.rs:1663` |
| 8 | `if`/`then`/`else` | ~4,370 | Med-Large | `ferric-runtime/src/actions.rs` |
| 9 | `while`/`loop-for-count`/`foreach` | subset | Medium | `ferric-runtime/src/actions.rs` |
| 10 | Compile-time function validation | 142 | Medium | `ferric-runtime/src/loader.rs:902` |
