---
title: CLIPS Compatibility
description: Supported CLIPS language areas, known differences, and current exclusions.
---

Ferric targets semantic compatibility with the CLIPS Basic Programming Guide for the supported subset. “Supported” means implemented, not proven equivalent for every rule set in that area. Exact compatibility claims are limited to the reviewed differential policy cases and qualified by the known gaps below.

## Supported Core Areas

| Area                                                        | Support                   |
| ----------------------------------------------------------- | ------------------------- |
| Ordered facts                                               | Supported                 |
| Template facts                                              | Supported                 |
| `initial-fact` on reset                                     | Supported                 |
| `defrule`                                                   | Supported                 |
| Salience                                                    | Supported                 |
| `test`, `not`, `exists`, `forall`, NCC                      | Supported                 |
| Constraint connectives `~`, `\|`, `&`                       | Supported                 |
| Modules and focus stack                                     | Supported with known gaps |
| `deffunction`, `defgeneric`, `defmethod`                    | Supported                 |
| Globals                                                     | Supported                 |
| Core math, string, multifield, predicate, and I/O functions | Supported subset          |

## Direct Output

`printout` writes top-level STRING contents without quotes. Multifields use
parentheses, one space between fields, and quotes around STRING fields:
`(printout t (create$ "a" "two words") crlf)` writes `("a" "two words")` and
a newline. Empty multifields print `()` and empty STRING fields print `""`.
The same rendering applies inside deffunctions and methods and to Ferric's
RHS `println`, which adds a newline.

Printed STRING fields retain literal embedded quotes, backslashes, control
characters, and UTF8 bytes. `implode$` instead uses the escaped field mode
described below. SYMBOL fields remain
literal, including `crlf`, `tab`, `vtab`, and `ff`. Those four symbols expand
to LF, TAB, VT, and FF only as top-level output operands.

FLOAT output uses up to 15 significant decimal digits, preserving `-0.0`.
Rounded decimal exponents from -4 through 14 use fixed notation; other values
use scientific notation such as `1e-05` and `1e+15`. Integral fixed-form
FLOATs include `.0`; nonfinite spellings are `nan.0`, `inf.0`, and `-inf.0`.
INTEGER spelling remains exact. These rules do not change `str-cat`,
`sym-cat`, `format`, or `save-facts` formatting.

Typed INSTANCE-NAME and FACT-ADDRESS print forms remain representation gaps;
INTEGERs are printed as integers and host ExternalAddress values retain an
opaque placeholder. This output contract does not cover arbitrary invalid
UTF8 strings or general source round-tripping.

## Multifield Text

`create$` evaluates VOID-producing operands for their effects but omits those
scalar results from the multifield. Empty STRINGs remain fields.

`implode$` accepts exactly one MULTIFIELD and returns a STRING containing its
fields separated by one space, without outer parentheses. An empty multifield
returns an empty STRING; an empty STRING field contributes `""`. Each STRING
field is quoted, with embedded quotes and backslashes escaped by a backslash.
Literal control characters and UTF8 bytes remain unchanged, and SYMBOLs keep
their raw spelling, including `crlf`, `tab`, `vtab`, and `ff`.

INTEGER spelling stays exact. FLOATs share direct output's 15-significant-digit
format, including `-0.0`, exponents, and nonfinite spellings. The operand is
evaluated once after the argument-count check; a scalar result produces a type
error. Rendering leaves input values and the separate `str-cat`, `sym-cat`,
`format`, and `save-facts` formatters unchanged.

Quoted STRING-field round-tripping through `explode$` still depends on
[#339](https://github.com/plx/ferric-rules/issues/339). Arbitrary generated
SYMBOL spellings have no general source round-trip guarantee. The typed-value
and invalid-UTF8 boundaries described for direct output apply here too;
INTEGERs are never reinterpreted as fact addresses.

## Conflict Resolution

Depth and breadth use activation creation order and match the pinned reference cases. The retained LEX and MEA host options are experimental Ferric strategies with the ordering gaps below.

| Strategy | Description                               |
| -------- | ----------------------------------------- |
| Depth    | Most recent activation fires first.       |
| Breadth  | Oldest activation fires first.            |
| LEX      | Lexicographic recency comparison.         |
| MEA      | First-pattern recency, then LEX tiebreak. |

Not implemented: Simplicity, Complexity, Random.

## Known Differential Gaps

The blocking pinned-CLIPS policy retains these differences as exact known deviations rather than reporting them as equivalent. Any unexplained or changed divergence fails the gate.

| Area        | Known gap                                             | Policy cases                                                                             | Tracking                                               |
| ----------- | ----------------------------------------------------- | ---------------------------------------------------------------------------------------- | ------------------------------------------------------ |
| LEX and MEA | Selected recency vectors and the MEA tiebreak differ. | `FR-RETE-009` LEX recency-vector ordering; `FR-RETE-009-MEA` MEA recency-vector ordering | [#155](https://github.com/plx/ferric-rules/issues/155) |

The reviewed gate covers 57 scenarios: 55 equivalences and the two known LEX/MEA differences. All 35 scenarios added beyond the 22-case baseline match pinned CLIPS 6.30. Other corpus fixtures are not compatibility claims until they have a structured oracle and reviewed policy entry.

## Known Exclusions

- COOL object system is intentionally out of scope.
- Truth maintenance through the `logical` conditional element is intentionally out of scope.
- CLIPS-valid complex negated constraints are explicitly rejected pending [#300](https://github.com/plx/ferric-rules/issues/300).
- Some I/O utilities are limited while rule execution remains the core focus.

## Validation Posture

Compatibility coverage uses hand-written fixtures, real-world CLIPS corpus work, generated harnesses, authenticated engine observations, and an exact pinned-CLIPS policy. Pull requests and `main` require the blocking compatibility gate; retained artifacts bind the candidate and reference digests. The repository also includes scaling checks that exercise asymptotic behavior for core operations.
