---
title: CLIPS compatibility
description: Supported CLIPS 6.30 features, out-of-scope constructs, intentional differences, and differential testing.
---

`ferric-rules` aims for close semantic compatibility with the core rule and procedural language of CLIPS 6.30 (roughly the surface of the _CLIPS Basic Programming Guide_), so that straightforward rule sets run as expected without silent surprises. Because it is an independent implementation with strict UTF-8 and memory-safety invariants, compatibility covers a defined subset rather than every corner of CLIPS.

For the exhaustive construct-by-construct specification, see [`docs/compatibility.md`](https://github.com/plx/ferric-rules/blob/main/docs/compatibility.md) in the repository.

## How compatibility is tested

Day-to-day verification relies on differential testing against a pinned CLIPS 6.30 reference container:

- **Granular golden corpus ([`tests/clips_compat/corpus/`](https://github.com/plx/ferric-rules/blob/main/tests/clips_compat/corpus/README.md)):** Roughly 1,270 small `.clp` programs paired with exact CLIPS 6.30 output goldens. Around 1,245 conforming programs (including ~230 expected load or runtime error cases) must match CLIPS 6.30 output byte-for-byte on normal execution, and again after a CBOR and JSON snapshot round-trip.
- **Pinned gap characterizations:** Roughly 24 corpus cases exercise known behavioral differences or stricter load-time boundaries. Each records `ferric-rules`'s exact current output or located load error so behavior cannot drift in either direction without an explicit review.
- **Multi-step scenario checks:** A separate suite of 57 multi-step scenarios compares working-memory and execution observations directly against the CLIPS 6.30 container on every pull request.

Passing these suites gives good regression coverage for the paths they exercise, though any existing CLIPS rule set being ported should still be checked against the exclusions and boundaries below.

## Supported language surface

| Area                     | Implemented features                                                                                                                                                                                                                                                                        |
| ------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| **Facts**                | Ordered positional facts and `deftemplate` slot/multislot facts; static and dynamic defaults; type, allowed-values, range, and cardinality constraints; `deffacts`; protected `(initial-fact)` on `(reset)`; configurable fact-duplication policy.                                          |
| **Rules & patterns**     | `defrule` with ordered and template patterns, `?f <-` fact-address bindings, single/multifield wildcards and captures (`?`, `?x`, `$?`, `$?xs`), field connectives (`~`, `&`, `\|`), `(test ...)`, `(not ...)`, `(exists ...)`, `(forall ...)`, and `(not (and ...))` negated conjunctions. |
| **Agenda & strategies**  | Definition-time integer and expression `salience` (`-10000` to `10000`), `auto-focus`, and the `depth` (default), `breadth`, `lex`, and `mea` conflict-resolution strategies.                                                                                                               |
| **Procedural & queries** | `deffunction`, `defgeneric` / `defmethod` dispatch, `defglobal`, `if`/`then`/`else`, `switch`, `while`, `loop-for-count`, `progn$` / `foreach`, `break`, and fact-query forms (`any-factp`, `find-fact`, `find-all-facts`, `do-for-fact`, `do-for-all-facts`, `delayed-do-for-all-facts`).  |
| **Modules & stdlib**     | `defmodule` with `import`/`export` visibility, explicit `focus` and focus stacks, plus core CLIPS math, string, multifield, predicate, construct-inspection, and channel I/O functions.                                                                                                     |

## Excluded and unimplemented features

These CLIPS features are not supported and fail explicitly with load or runtime errors rather than being ignored:

- **COOL (CLIPS Object-Oriented Language):** Classes (`defclass`), instances (`definstances`, `make-instance`), and message-handlers (`defmessage-handler`) are out of scope. Bracketed `[name]` literals exist only as scalar `INSTANCE-NAME` values.
- **Logical dependencies (`logical` CE):** Truth maintenance is not supported; rules containing `(logical ...)` are rejected at load time.
- **Dynamic salience & minor strategies:** `when-activated` and `every-cycle` salience evaluation (`set-salience-evaluation`, `refresh-agenda`) and the `simplicity`, `complexity`, and `random` conflict strategies are unsupported.

## Intentional and known behavioral differences

1. **UTF-8 text and checked `format` directives:**
   `ferric-rules` uses UTF-8 strings throughout its parser, runtime, C ABI, and bindings. Source and fact files must be valid UTF-8; byte operations that emit raw non-UTF-8 bytes in CLIPS 6.30 produce `U+FFFD` instead. Similarly, malformed `format` directives such as `%5-3d` (which CLIPS passes through to the platform's `libc` `printf`) fail with an explicit format error.
2. **Load-time bounds on complex constraints inside direct negations:**
   Directly negated patterns (`(not (data ...))` and the second operand of `forall`) lower field predicates (`:`) and return-value constraints (`=`) into indexed join checks. Comparisons against literals or earlier variables (including integer `+`/`-` offsets and binary `str-compare`) work directly, while arbitrary non-linear expressions such as `(not (data ?x&:(> (* ?x ?x) 10)))` are rejected at load time. Write those explicitly as a negated conjunction with a `test` condition:
   ```lisp
   (not (and (data ?x)
             (test (> (* ?x ?x) 10))))
   ```
   Single-pattern `(exists ...)` has a similar load boundary; adding a `(test ...)` condition inside `exists` works around it.
3. **Quantifier nesting limits:**
   Patterns support up to four nested levels of `not` / `exists` / `forall` (including triple and quadruple negation), within compiled condition budgets. Nesting `forall` inside another `forall`, `not`, or `exists` is rejected at load time.
4. **Equal-salience tie-breaking and transient `auto-focus` edge cases:**
   CLIPS shares identical negative and negated-conjunction joins across rules; `ferric-rules` currently shares positive beta joins across rules and compiles negative/NCC subnetworks per rule. When multiple activations share the same salience (and, under `lex`/`mea`, the same recency vector and specificity), tie-breaking order can differ from CLIPS 6.30, as can focus-stack history in a handful of late-installed `auto-focus` + NCC combinations. Rule sets that depend on relative firing order should express precedence with `salience`, `focus`, or phase facts.
