---
title: CLIPS Compatibility
description: Supported CLIPS language areas, known differences, and current exclusions.
---

Ferric targets semantic compatibility with the CLIPS Basic Programming Guide for the supported subset. Rule sets inside that subset should execute without modification.

## Supported Core Areas

| Area                                                        | Support          |
| ----------------------------------------------------------- | ---------------- |
| Ordered facts                                               | Supported        |
| Template facts                                              | Supported        |
| `initial-fact` on reset                                     | Supported        |
| `defrule`                                                   | Supported        |
| Salience                                                    | Supported        |
| `test`, `not`, `exists`, `forall`, NCC                      | Supported        |
| Constraint connectives `~`, `\|`, `&`                       | Supported        |
| Modules and focus stack                                     | Supported        |
| `deffunction`, `defgeneric`, `defmethod`                    | Supported        |
| Globals                                                     | Supported        |
| Core math, string, multifield, predicate, and I/O functions | Supported subset |

## Conflict Resolution

Ferric implements these configurable strategies:

| Strategy | Description                               |
| -------- | ----------------------------------------- |
| Depth    | Most recent activation fires first.       |
| Breadth  | Oldest activation fires first.            |
| LEX      | Lexicographic recency comparison.         |
| MEA      | First-pattern recency, then LEX tiebreak. |

Not implemented: Simplicity, Complexity, Random.

## Known Exclusions

- COOL object system is intentionally out of scope.
- Logical dependencies are planned but not currently present.
- Some exotic pattern connectives remain outside the current subset.
- Some I/O utilities are limited while rule execution remains the core focus.

## Validation Posture

Compatibility coverage is tested through hand-written fixtures, real-world CLIPS corpus work, and dedicated compatibility harnesses. The repository also includes scaling checks that exercise asymptotic behavior for core operations.
