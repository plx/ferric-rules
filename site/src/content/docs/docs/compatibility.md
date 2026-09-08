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

## Conflict Resolution

Depth and breadth use activation creation order and match the pinned reference cases. The retained LEX and MEA host options are experimental Ferric strategies with the ordering gaps below.

| Strategy | Description                               |
| -------- | ----------------------------------------- |
| Depth    | Most recent activation fires first.       |
| Breadth  | Oldest activation fires first.            |
| LEX      | Lexicographic recency comparison.         |
| MEA      | First-pattern recency, then LEX tiebreak. |

Not implemented: Simplicity, Complexity, Random.

## Predicate Sorting

`sort` invokes its comparator and accepts scalar or multifield arguments:
`(sort > 3 (create$ 1 2))` returns `(1 2 3)`, while `<` gives descending order.
Only the actual symbol `FALSE` keeps the left field before the right field;
other results, including zero and a Void predicate result, request exchange.
Stable merge traversal preserves equal-key input order when the predicate
returns `FALSE` for ties, and makes comparator calls in a defined order. Data expressions run once before comparisons, and
empty or singleton inputs do not invoke the comparator.

The comparator must be an unqualified symbol naming a supported visible
builtin, deffunction or generic. Missing names and incompatible builtin or
deffunction arity return `FALSE`, skip data and record a nonfatal diagnostic.
Fatal expression or predicate errors stop subsequent rule actions, but may
still carry a partial value into an enclosing assignment. Diagnostic presence
alone does not distinguish these outcomes; inspect the run's halt reason.

The value layer supports `INSTANCE-NAME` and byte lexemes, including strings
that are not valid UTF-8. This does not require reproducing the pinned CLIPS
process fault for Void used as a data field. The CLIPS-valid
`(sort bind c b a)` callback remains an unsupported local-binding/special-form
invocation; comparator metadata does not imply parity for every builtin.
Malformed source bind targets are a separate parsed-variable restriction.

## Formatting

`format` returns a STRING without writing to a router. Use
`(printout t (format nil "n=%d" 42) crlf)` to print its result.

Canonical lowercase `d`, `o`, `x`, `u`, `f`, `e`, `g`, `s`, and `c`
conversions accept leading `-`/`0` flags, a minimum decimal width, and
optional `.precision`. Left alignment overrides zero padding. Integer
precision counts digits and disables width zero padding; zero precision
with zero emits no digits. `%f` and `%e` default to six fractional digits;
`%e` includes a signed exponent with at least two digits. `%g` defaults to
six significant digits, treats explicit zero precision as one, chooses its
notation from the rounded exponent, and removes trailing fractional zeroes.
Negative zero is preserved. Infinity and NaN use lowercase spellings and
space padding. `%n`, `%r`, `%t`, `%v`, and `%%` emit newline, carriage return,
tab, vertical tab, and percent without consuming data.

Numeric conversions accept INTEGER or FLOAT. `%s` accepts STRING, SYMBOL,
and INSTANCE-NAME, with names rendered without brackets. `%c` accepts
INTEGER, STRING, or SYMBOL; it takes the low integer byte or first lexeme
byte and ignores precision. Width and string precision count raw bytes,
including partial UTF-8 sequences. Control strings and `%s` operands stop
at their first NUL. A NUL from `%c` keeps only padding before it; later
fragments still append. Returned bytes follow the configured encoding policy.

The complete control prefix and exact operand count are validated before
any data expression runs. Data then evaluates once, sequentially, with each
conversion checked before the next. Errors return an empty STRING and
retain prior side effects. Invalid flags/counts and control, numeric, or
`%s` type errors halt following actions. A `%c` type error instead records a
nonfatal diagnostic and skips the remaining format operands; it does not
clear an earlier halt.

Ferric limits each call's control prefix and aggregate output to 16 MiB
(16,777,216 bytes). FLOAT-to-integer conversion saturates at signed 64-bit
endpoints, with NaN becoming zero. These are explicit engine policies.
Scanner-admitted malformed fragments use a deterministic normalized-prefix
and preserved-tail echo, including CLIPS's inserted `ll` for integer
conversions. The algorithm matches five pinned echoes; other applications
are Ferric policy, not universal C library parity. Other printf extensions,
such as dynamic widths, positional arguments, length modifiers, and
uppercase conversions, are unsupported. The return-only router policy also
remains a documented difference.

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
