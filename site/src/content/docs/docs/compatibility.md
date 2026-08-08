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

Ferric implements these configurable strategies. Recreated activations and selected multi-pattern ties still have the issue-linked ordering gaps below.

| Strategy | Description                               |
| -------- | ----------------------------------------- |
| Depth    | Most recent activation fires first.       |
| Breadth  | Oldest activation fires first.            |
| LEX      | Lexicographic recency comparison.         |
| MEA      | First-pattern recency, then LEX tiebreak. |

Not implemented: Simplicity, Complexity, Random.

## Known Differential Gaps

The blocking pinned-CLIPS policy retains these differences as exact known deviations rather than reporting them as equivalent. Any unexplained or changed divergence fails the gate.

| Area                      | Known gap                                                                     | Policy cases                                                                                   | Tracking                                               |
| ------------------------- | ----------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------- | ------------------------------------------------------ |
| Depth and breadth         | Recreated-activation chronology can reverse order-sensitive effects.          | `FR-RETE-008` depth activation chronology; `FR-RETE-008-BREADTH` breadth activation chronology | [#154](https://github.com/plx/ferric-rules/issues/154) |
| LEX and MEA               | Selected recency vectors and the MEA tiebreak differ.                         | `FR-RETE-009` LEX recency-vector ordering; `FR-RETE-009-MEA` MEA recency-vector ordering       | [#155](https://github.com/plx/ferric-rules/issues/155) |
| Reset ordering            | `deffacts` and `initial-fact` activations are inserted in the opposite order. | `FR-RETE-010` reset bootstrap ordering                                                         | [#156](https://github.com/plx/ferric-rules/issues/156) |
| Rule replacement          | A superseded same-name rule can remain live.                                  | `FR-RETE-011` same-name rule replacement                                                       | [#157](https://github.com/plx/ferric-rules/issues/157) |
| Template redefinition     | A rejected live-template redefinition can corrupt later load state.           | `FR-RETE-012` in-use template redefinition                                                     | [#158](https://github.com/plx/ferric-rules/issues/158) |
| Module imports            | An invalid import can be accepted and leak a qualified fact.                  | `FR-RETE-015` module export visibility                                                         | [#160](https://github.com/plx/ferric-rules/issues/160) |
| Immediate focus reporting | `list-focus-stack` can omit a newly focused module.                           | `FR-RETE-016` immediate focus changes                                                          | [#192](https://github.com/plx/ferric-rules/issues/192) |
| Drained focus stack       | Ferric can retain `MAIN` after CLIPS reports an empty stack.                  | `FR-RETE-017` focus-stack draining                                                             | [#193](https://github.com/plx/ferric-rules/issues/193) |
| Late `deffacts` loading   | Facts can be asserted without another reset.                                  | `FR-RETE-018` deffacts reset lifecycle                                                         | [#161](https://github.com/plx/ferric-rules/issues/161) |

The reviewed gate covers 22 scenarios for 20 production-audit IDs plus one generated-harness control. Other corpus fixtures are not compatibility claims until they have a structured oracle and reviewed policy entry.

## Known Exclusions

- COOL object system is intentionally out of scope.
- Truth maintenance through the `logical` conditional element is intentionally out of scope.
- Some exotic pattern connectives remain outside the current subset.
- Some I/O utilities are limited while rule execution remains the core focus.

## Validation Posture

Compatibility coverage uses hand-written fixtures, real-world CLIPS corpus work, generated harnesses, authenticated engine observations, and an exact pinned-CLIPS policy. Pull requests and `main` require the blocking compatibility gate; retained artifacts bind the candidate and reference digests. The repository also includes scaling checks that exercise asymptotic behavior for core operations.
