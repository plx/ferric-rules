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

Depth and breadth use activation creation order. LEX and MEA compare fact recencies and specificity before breaking remaining ties in favor of older activations. Salience takes precedence for all four strategies.

| Strategy | Description                                                 |
| -------- | ----------------------------------------------------------- |
| Depth    | Most recent activation fires first.                         |
| Breadth  | Oldest activation fires first.                              |
| LEX      | Sorted fact recencies, specificity, then older activations. |
| MEA      | First-pattern recency, then the LEX comparison.             |

Choose a strategy through the host configuration API. Source `set-strategy` and `get-strategy` commands are unsupported. Simplicity, Complexity, and Random are not implemented.

## Reviewed Compatibility Evidence

The blocking pinned-CLIPS policy requires equivalent observations in all 57 reviewed scenarios, including the LEX/MEA cases repaired in [#412](https://github.com/plx/ferric-rules/issues/412). Any unexplained divergence fails the gate. The granular corpus separately characterizes known differences, including activation chronology at negative/NCC sharing and late-install boundaries; see the [full compatibility contract](https://github.com/plx/ferric-rules/blob/main/docs/compatibility.md). Other fixtures are not compatibility claims until they have a structured oracle and reviewed policy entry.

## Known Exclusions

- COOL object system is intentionally out of scope.
- Truth maintenance through the `logical` conditional element is intentionally out of scope.
- CLIPS-valid complex negated constraints are explicitly rejected pending [#300](https://github.com/plx/ferric-rules/issues/300).
- Some I/O utilities are limited while rule execution remains the core focus.

## Validation Posture

Compatibility coverage uses hand-written fixtures, real-world CLIPS corpus work, generated harnesses, authenticated engine observations, and an exact pinned-CLIPS policy. Pull requests and `main` require the blocking compatibility gate; retained artifacts bind the candidate and reference digests. The repository also includes scaling checks that exercise asymptotic behavior for core operations.
