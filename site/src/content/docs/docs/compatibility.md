---
title: CLIPS compatibility
description: Supported CLIPS language areas, explicit differences, and validation evidence.
---

Ferric implements a defined subset of the CLIPS rule language. Check the
[full compatibility contract](https://github.com/plx/ferric-rules/blob/main/docs/compatibility.md)
before migrating an existing rule set; an implemented feature is not a promise
of equivalence for every program.

## Validation and evidence

The [granular corpus](https://github.com/plx/ferric-rules/blob/main/tests/clips_compat/corpus/README.md)
declares 1,095 programs: 1,075 conformance cases and 20 active characterizations
of differences. The conformance count includes 205 cases that reproduce an
expected CLIPS load or execution error. Eligible programs also run through
incremental rule loading and JSON/CBOR snapshot replays.

These are coverage counts. Verification is reported separately for the actual
revision, selected cases, Ferric run, and pinned CLIPS 6.30 reference run. Missing,
filtered, stale, or failed evidence does not establish a fully verified result.
The [assessment reports](https://github.com/plx/ferric-rules/blob/main/docs/compatibility-assessment.md)
distinguish fixed gaps from added coverage, changed scenarios or goldens, and
removed cases. Unexecuted legacy files remain inventory, not compatibility
results.

A separate reviewed semantic policy requires equivalent observations for all
57 scenarios, including the repaired LEX/MEA cases. Generated harnesses and
binding tests provide additional coverage. These checks are evidence for their
cases, not proof of complete language equivalence.

## Implemented features

| Area             | Implemented features                                                                                                                                                         |
| ---------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Facts            | Ordered and template facts, slot constraints and defaults, `deffacts`, and `initial-fact` on reset.                                                                          |
| Rules            | `defrule`, definition-time salience, auto-focus, `test`, `not`, `exists`, `forall`, negated conjunction, and field constraints `~`, `\|`, `&`, within the documented limits. |
| Functions        | `deffunction`, `defgeneric`, `defmethod`, globals, and expression-position fact effects.                                                                                     |
| Modules          | Modules, construct visibility, and focus stacks.                                                                                                                             |
| Standard library | A subset of CLIPS math, string, multifield, predicate, query, and I/O functions.                                                                                             |

`format` writes its completed string to its channel and returns the string;
`nil` suppresses routing. Functions and methods can assert, retract, modify,
and duplicate facts. Earlier effects remain visible if a later expression
fails. Engine mutation is rejected during rule matching.

CLIPS fact addresses use a distinct `FACT-ADDRESS` value, printed as `<Fact-N>`
or `<Dummy Fact>`. They are not integers. Host fact handles have a separate
engine-ownership contract; see [Embedding API](../embedding/).

## Conflict resolution

Depth, breadth, LEX, and MEA are supported. Salience takes precedence in every
strategy. `(get-strategy)` reads the strategy; `(set-strategy breadth)` changes
it and reorders pending activations.

| Strategy | Ordering within equal salience                                                    |
| -------- | --------------------------------------------------------------------------------- |
| Depth    | Most recent activation first.                                                     |
| Breadth  | Oldest activation first.                                                          |
| LEX      | Fact recencies sorted newest first, then rule specificity, then older activation. |
| MEA      | First outer condition's recency, then the full LEX comparison.                    |

Absent conditions contribute recency entries older than every fact. The remaining
activation-order differences concern specific network-sharing and late-install
cases, not the former LEX/MEA comparison defects. Use salience or focus when an
application requires explicit precedence.

## Explicit compatibility differences

The corpus retains exact observations for these boundaries:

- Four UTF-8 and malformed-`format` differences are accepted permanent behavior.
  Ferric preserves valid UTF-8 with U+FFFD where CLIPS exposes invalid bytes,
  and rejects malformed directives instead of delegating them to libc. See
  the [UTF-8 and format decision](https://github.com/plx/ferric-rules/blob/main/docs/compatibility.md#accepted-utf-8-and-format-divergences).
- Four equal-salience cases concern separately compiled negative/NCC joins or
  independent multi-pattern `exists` support ordering.
- Two late-install auto-focus cases retain different NCC history or predicate
  subnetwork sharing.
- Ten CLIPS-valid programs exercise explicit conditional-element nesting or
  operand limits and are rejected with located load errors.

Direct complex predicate and return-value constraints inside negated fact
patterns also have an explicit supported-subset boundary. Variable comparisons
and supported integer-offset forms work; arbitrary expressions such as
`(not (data ?x&:(> (* ?x ?x) 10)))` are rejected at load. Such expressions are
valid CLIPS. Ferric does not defer them to rule firing. See
[the constraint boundary](https://github.com/plx/ferric-rules/blob/main/docs/compatibility.md#predicate-and-return-value-constraints)
and the full compatibility contract for supported match-time forms.

## Exclusions and bounds

The COOL object system and `logical` truth maintenance are out of scope.
Simplicity, complexity, and random conflict strategies are not implemented.

Source patterns allow up to four combined `not`/`exists`/`forall` levels;
triple and four-deep negation work. Compiled condition budgets still apply.
Single-operand `exists` around a negated fact, nested `forall`, and `forall`
beneath `not` or `exists` remain unsupported. `forall` requires one fact
condition and one fact or test-only requirement. Pure-test wrappers have their
own supported normalization; these are not blanket bans on nested expressions.
See the [source and compiled limits](https://github.com/plx/ferric-rules/blob/main/docs/compatibility.md#source-and-compiled-network-limits).
