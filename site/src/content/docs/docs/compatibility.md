---
title: CLIPS compatibility
description: Supported CLIPS language areas, known differences, and current exclusions.
---

Ferric implements much of the CLIPS rule language, but is not a complete
replacement. An implemented feature may still differ from CLIPS for particular
rule sets. Known differences are listed below.

## Implemented features

| Area             | Implemented features                                                                                                    |
| ---------------- | ----------------------------------------------------------------------------------------------------------------------- |
| Facts            | Ordered and template facts, `deffacts`, and `initial-fact` on reset.                                                    |
| Rules            | `defrule`, salience, `test`, `not`, `exists`, `forall`, negated conjunction, and constraint connectives `~`, `\|`, `&`. |
| Functions        | `deffunction`, `defgeneric`, `defmethod`, and globals.                                                                  |
| Modules          | Modules and focus stacks.                                                                                               |
| Standard library | A subset of CLIPS math, string, multifield, predicate, and I/O functions.                                               |

## Conflict resolution

Depth (the default) and Breadth use activation creation order and match the
pinned reference cases. LEX and MEA are experimental Ferric strategies with the
ordering differences below. Use salience or focus to express required precedence.

| Strategy | Description                               |
| -------- | ----------------------------------------- |
| Depth    | Most recent activation fires first.       |
| Breadth  | Oldest activation fires first.            |
| LEX      | Lexicographic recency comparison.         |
| MEA      | First-pattern recency, then LEX tiebreak. |

## Known Differential Gaps

The comparison tests currently record these differences from the pinned CLIPS
reference. The linked issue describes both cases.

| Area        | Known difference                                      | Policy cases                                                                             | Tracking                                               |
| ----------- | ----------------------------------------------------- | ---------------------------------------------------------------------------------------- | ------------------------------------------------------ |
| LEX and MEA | Some recency comparisons and the MEA tiebreak differ. | `FR-RETE-009` LEX recency-vector ordering; `FR-RETE-009-MEA` MEA recency-vector ordering | [#155](https://github.com/plx/ferric-rules/issues/155) |

## Known exclusions

- The COOL object system and `logical` truth maintenance are out of scope.
- Simplicity, Complexity, and Random conflict strategies are not implemented.
- Triple-nested negation, `(exists (not ...))`, and nested `forall` are not supported.
- Complex negated constraints that CLIPS accepts are rejected pending [#300](https://github.com/plx/ferric-rules/issues/300).

Other differences affect function bodies and I/O. For example, `format` returns
a string without writing to a router, and `deffunction` bodies cannot mutate
facts. See the [full compatibility reference](https://github.com/plx/ferric-rules/blob/main/docs/compatibility.md)
for restrictions by language feature.

## Validation

The differential suite runs the same cases against Ferric and a pinned CLIPS
build. It distinguishes matching results from known differences; a new or
changed difference fails the check.

The reviewed policy covers 57 scenarios: 55 match pinned CLIPS 6.30, and two
retain the LEX/MEA differences above. This is evidence for those cases, not
proof of compatibility for the whole language. The
[assessment documentation](https://github.com/plx/ferric-rules/blob/main/docs/compatibility-assessment.md)
describes the cases, expected results, and retained evidence.
