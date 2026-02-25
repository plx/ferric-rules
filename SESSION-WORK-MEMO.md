# Session Work Memo — Batch 3 Compatibility Fixes (200-series)

## Task
Work through 16 compatibility fixes (201-216) sequentially. For each:
1. Start from clean git status
2. Attempt fix
3. If successful: rename to `DONE-$name`, commit
4. If blocked: revert changes, rename to `BLOCKED-$name`, add blocker note, commit just that
5. "Fails for a different reason" counts as SUCCESS (we fixed the known issue)

## Starting State
- Branch: fix/compatibility
- Tests: 1388 passing
- Clean git status

## Fix Queue (in order)
| # | File | Complexity | Summary |
|---|------|-----------|---------|
| 201 | IfSlotNameCollision | Medium | `if` as slot name conflicts with if/then/else keyword |
| 202 | TestCeInsideNccAndExists | Medium-High | test CEs inside not(and), exists, forall |
| 203 | OrConstraintPredicateOnly | Medium | Predicate-only or-constraints `:(p1)\|:(p2)` |
| 204 | DuplicateAction | Medium | Missing `duplicate` RHS action |
| 205 | EmptyMultislotInDeffacts | Simple | `(bar)` = empty multislot, `(default (create$))` |
| 206 | SingleQuoteAndBackslashInLexer | Simple | `'` and `\` as valid symbol chars |
| 207 | ModuleQualifiedDefglobal | Simple | `(defglobal MAIN ?*x* = 1)` |
| 208 | OrCeInsideNotAndExists | High | or CE nested inside not(and)/exists |
| 209 | MultiPatternExists | Medium | `(exists P1 P2)` with multiple patterns |
| 210 | DeeplyNestedNcc | High | not(and(not(and(...)))) recursive NCC |
| 211 | PredicateConstraintInNegation | Medium-High | Complex constraint chains, forward var refs |
| 212 | ErrorRecoveryForMalformedConstructs | High | Error recovery, continue after bad constructs |
| 213 | BroadenClipsErrorRegex | Simple | Script fix: broaden error regex |
| 214 | MissingCompileTimeValidation | Low priority | Compile-time warnings (CLIPS-style) |
| 215 | HarnessProtocolFixes | Simple | Script fix: add reset/run to harness |
| 216 | RuntimeOutputDivergences | Medium | Multiple small runtime gaps |

## Key Rule
A fix that resolves the SPECIFIC known incompatibility is a SUCCESS even if the file
then fails for a DIFFERENT reason. We must granularly check: did we fix the issue
described in the compatibility doc?
