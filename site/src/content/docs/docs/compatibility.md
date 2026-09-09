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
characters, and raw bytes. `implode$` uses the escaped STRING-field mode
described below. SYMBOL fields remain literal, including `crlf`, `tab`,
`vtab`, and `ff`. Those four symbols expand
to LF, TAB, VT, and FF only as top-level output operands.

FLOAT output uses up to 15 significant decimal digits, preserving `-0.0`.
Rounded decimal exponents from -4 through 14 use fixed notation; other values
use scientific notation such as `1e-05` and `1e+15`. Integral fixed-form
FLOATs include `.0`; nonfinite spellings are `nan.0`, `inf.0`, and `-inf.0`.
INTEGER spelling remains exact. These rules do not change `str-cat`,
`sym-cat`, `format`, or `save-facts` formatting.

STRING and SYMBOL payloads retain NUL and invalid UTF8 bytes. INSTANCE-NAME
values print as bracketed raw name bytes, at the top level and inside
multifields. Typed FACT-ADDRESS print forms remain a representation gap;
INTEGERs are printed as integers and host ExternalAddress values retain an
opaque placeholder. General source round-tripping is a separate contract.

## Multifield Text

`create$` evaluates VOID-producing operands for their effects but omits those
scalar results from the multifield. Empty STRINGs remain fields.

`implode$` accepts exactly one MULTIFIELD and returns a STRING containing its
fields separated by one space, without outer parentheses. An empty multifield
returns an empty STRING; an empty STRING field contributes `""`. Each STRING
field is quoted, with embedded quotes and backslashes escaped by a backslash.
Literal control characters and raw bytes remain unchanged. SYMBOLs keep their
literal spelling, including `crlf`, `tab`, `vtab`, and `ff`; INSTANCE-NAMEs
use bracketed raw name bytes.

INTEGER spelling stays exact. FLOATs share direct output's 15-significant-digit
format, including `-0.0`, exponents, and nonfinite spellings. The operand is
evaluated once after the argument-count check; a scalar result produces a type
error. Rendering leaves input values and the separate `str-cat`, `sym-cat`,
`format`, and `save-facts` formatters unchanged.

`explode$` and `str-explode` scan STRING bytes into typed fields, including
quoted STRINGs and INSTANCE-NAMEs. Quoted-field round-trip coverage now
composes the scanner with `implode$`: it checks empty, numeric-looking and
bracket-looking STRINGs, quotes and backslashes, scannable SYMBOLs and names,
exact INTEGERs, and selected FLOATs such as `1.25` and `-0.0`. Normal and late
rule installation and all five snapshot formats are covered. For example,
`(explode$ (implode$ (create$ a "two words" 3)))` returns `(a "two words" 3)`.

General source serialization remains a separate contract: arbitrary SYMBOL
or INSTANCE-NAME spellings may not scan back to the same value, and the
15-significant-digit FLOAT representation need not retain arbitrary f64 bits.
Length-bearing host values preserve NUL and invalid UTF8 bytes in the imploded
result, while the scanner stops at the first NUL. The typed FACT-ADDRESS and
opaque host-address limits described for direct output also apply here;
INTEGERs are never reinterpreted as addresses.

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

## Queued input

`read` and `readline` share lines supplied through `Engine::push_input`.
Their optional input name is evaluated once; `t`, `T`, and `stdin` select
that queue. `read` skips blank or comment-only lines, scans the first token
of the selected line, and discards the rest. `readline` returns the next
line unchanged, even if empty. Hosts supply already-framed lines: the API
does not split or normalize CR/LF sequences.

Quoted input `"two words"` returns an actual STRING `two words`. Other fields
retain INTEGER, FLOAT, SYMBOL, or INSTANCE-NAME identity; variable and
punctuation tokens return STRING print forms. Exhausted input yields the
SYMBOL `EOF`. An unknown scanner token yields the STRING
`*** READ ERROR ***` without a diagnostic. Overflow and incomplete quotes
retain clamped or partial values with nonfatal notices; an error-channel
notice does not halt execution. Quoted fields retain CLIPS escapes and
exact bytes, including invalid UTF-8 from a trailing escape at EOF, subject
to the configured encoding policy.

Unknown or invalid input names return the read-error STRING, record a
diagnostic and halt without consuming input. After resolving a valid name, an already
halted evaluation returns the same STRING without a new diagnostic or
consuming a line. This is Ferric policy: CLIPS's raw stream can consume a
byte before checking halt. An error flag alone does not block a read.

Reset preserves unread lines, clear discards them, and snapshots preserve
the remaining queue. Named-file input and `open` remain unsupported;
queued input does not imply a persistent named stream or raw stdin API.

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
