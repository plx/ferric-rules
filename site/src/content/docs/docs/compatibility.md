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

## Known Differential Gaps

The blocking pinned-CLIPS policy retains these differences as exact known deviations rather than reporting them as equivalent. Any unexplained or changed divergence fails the gate.

| Area        | Known gap                                             | Policy cases                                                                             | Tracking                                               |
| ----------- | ----------------------------------------------------- | ---------------------------------------------------------------------------------------- | ------------------------------------------------------ |
| LEX and MEA | Selected recency vectors and the MEA tiebreak differ. | `FR-RETE-009` LEX recency-vector ordering; `FR-RETE-009-MEA` MEA recency-vector ordering | [#155](https://github.com/plx/ferric-rules/issues/155) |

The reviewed gate covers 57 scenarios: 55 equivalences and the two known LEX/MEA differences. All 35 scenarios added beyond the 22-case baseline match pinned CLIPS 6.30. Other corpus fixtures are not compatibility claims until they have a structured oracle and reviewed policy entry.

## Known Exclusions

- COOL object system is intentionally out of scope.
- Truth maintenance through the `logical` conditional element is intentionally out of scope.
- Runtime constraints on ordered sequence fields (`$?`) and scalar fields within multislots require sequence matching support. Fixed-width ordered and scalar template constraints inside `not` are evaluated at match time, including nonlinear expressions tracked in [#300](https://github.com/plx/ferric-rules/issues/300).
- Some I/O utilities are limited while rule execution remains the core focus.

## Validation Posture

Compatibility coverage uses hand-written fixtures, real-world CLIPS corpus work, generated harnesses, authenticated engine observations, and an exact pinned-CLIPS policy. Pull requests and `main` require the blocking compatibility gate; retained artifacts bind the candidate and reference digests. The repository also includes scaling checks that exercise asymptotic behavior for core operations.
