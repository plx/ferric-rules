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
declares 1,269 programs: 1,245 conformance cases and 24 active characterizations
of differences. The conformance count includes 232 cases that reproduce an
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

- Four UTF-8 and malformed-`format` differences are accepted permanent behavior,
  decided in [#394](https://github.com/plx/ferric-rules/issues/394). Ferric
  keeps valid UTF-8, holding U+FFFD where CLIPS emits invalid bytes, and reports
  a format error for malformed directives such as `%5-3d`, which CLIPS passes to
  libc. Ferric also refuses source and fact files that are not valid UTF-8. See
  the [UTF-8 and format decision](https://github.com/plx/ferric-rules/blob/main/docs/compatibility.md#accepted-utf-8-and-format-divergences).
- Five equal-salience cases concern separately compiled identical negative/NCC
  joins, independent multi-pattern `exists` support ordering, or a transient
  nested NCC refire on a shared subnetwork entry.
- Four auto-focus cases retain a different focus history: three rules installed
  after their facts, and one test CE cancelled by NCC completion.
- Nine CLIPS-valid programs exercise the explicit conditional-element nesting or
  operand limits from [#405](https://github.com/plx/ferric-rules/issues/405)
  and are rejected with located load errors.
- Two CLIPS-valid programs hold the located load error for the negated-constraint
  boundary below.

CLIPS-valid general predicate and return-value expressions in directly negated
patterns are explicitly rejected at load, the boundary decided in
[#300](https://github.com/plx/ferric-rules/issues/300). This covers ordered
fields, template slots, each `|` alternative, and the `forall` requirement.
Comparisons of the field variable against a literal or bound variable, with
integer offsets, and `str-compare` are lowered and work (their mixed-type and
overflow differences are tracked in [#499](https://github.com/plx/ferric-rules/issues/499));
an arbitrary expression such as `(not (data ?x&:(> (* ?x ?x) 10)))` is rejected. An explicit
`(not (and (P) (test ...)))` evaluates such a check at match time. A
single-pattern `exists` has a related load boundary, tracked in
[#446](https://github.com/plx/ferric-rules/issues/446). See
[the constraint boundary](https://github.com/plx/ferric-rules/blob/main/docs/compatibility.md#predicate-and-return-value-constraints)
for the supported subset and workarounds.

## Exclusions and bounds

The COOL object system and `logical` truth maintenance are out of scope.
Simplicity, complexity, and random conflict strategies are not implemented.

Source patterns allow up to four combined `not`/`exists`/`forall` levels;
triple and four-deep negation work. Compiled condition budgets still apply.
Nested `forall` and `forall` beneath `not` or `exists` remain unsupported. `forall` requires one fact
condition and one fact or test-only requirement. Pure-test wrappers have their
own supported normalization; these are not blanket bans on nested expressions.
See the [source and compiled limits](https://github.com/plx/ferric-rules/blob/main/docs/compatibility.md#source-and-compiled-network-limits)
and [pattern nesting restrictions](https://github.com/plx/ferric-rules/blob/main/docs/compatibility.md#pattern-nesting-restrictions).
