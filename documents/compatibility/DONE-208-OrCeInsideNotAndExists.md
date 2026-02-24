# 208 Or CE Inside Not And Exists

## Behavioral Divergence
CLIPS allows `(or ...)` CEs nested inside `(not (and ...))` and `(exists ...)`. Ferric rejects these with:
```
compile error: unsupported pattern form 'or': or CE reached translate_pattern unexpectedly (should be expanded via rule duplication)
```

Examples from the corpus:
```clips
;; Or CE inside not(and(...)):
(defrule fd-1
   (a)
   (not (and (b)
             (or (c) (d))))
   =>)

;; Or CE inside exists:
(defrule fd-2
   (a)
   (exists (b)
           (or (and (c)) (d)))
   =>)

;; Complex nested or CE inside exists:
(defrule problem-rule-3
   (A)
   (exists (or (and (B) (C))
               (and (D) (E))
               (and (F) (G))))
   (not (U))
   =>)
```

## Affected Files (4)
- `generated/test-suite-segments/co-drtest08-15.clp`
- `generated/test-suite-segments/t64x-drtest08-15.clp`
- `generated/test-suite-segments/jnftrght-17.clp`

## Apparent Ferric-Side Root Cause
`crates/ferric-runtime/src/loader.rs` — the or CE expansion (fix 102) only works at the top level of a rule's LHS. When an `or` CE appears nested inside `not(and(...))` or `exists(...)`, the rule-duplication strategy needs to apply recursively within those sub-contexts, but the current implementation does not recurse into NCC or existential sub-chains.

For `not(and(P1, or(Q1, Q2)))`:
- CLIPS semantics: "not (P1 and (Q1 or Q2))" = "not ((P1 and Q1) or (P1 and Q2))"
- Implementation: duplicate the NCC group — one copy with Q1, another with Q2 — then negate the disjunction, or equivalently: `not(and(P1, Q1))` AND `not(and(P1, Q2))`.

For `exists(or(P1, P2))`:
- CLIPS semantics: "exists (P1 or P2)" = "exists P1 or exists P2"
- Implementation: the rule can be duplicated at the top level, or the exists can be expanded.

## Implementation Plan
1. Recursive or-CE expansion in NCC contexts.
   - Before compiling an NCC group `not(and(children...))`, scan `children` for any `or` CE.
   - If found, distribute the NCC over the or branches: `not(and(A, or(B, C)))` becomes `not(and(A, B))` AND `not(and(A, C))`.
   - Note: this is semantically correct because `not(P or Q)` = `not(P) and not(Q)`.
   - Caveat: deep nesting (or inside and inside not inside or) may require recursive application.

2. Or-CE expansion in exists contexts.
   - `exists(or(P1, P2))` = `exists(P1) or exists(P2)`, which means the *rule* must be duplicated.
   - Alternatively, `exists(or(P1, P2))` = `not(not(or(P1, P2)))` = `not(and(not(P1), not(P2)))`.
   - The second form avoids rule duplication by expressing the exists-or as an NCC.
   - Caveat: the NCC transformation changes the Rete topology; verify equivalence.

3. Add tests.
   - `(not (and (b) (or (c) (d))))` — fires when there is no (b, c) pair AND no (b, d) pair.
   - `(exists (or (and (B) (C)) (and (D) (E))))` — fires when either (B, C) or (D, E) exists.

## Test And Verification
1. Loader unit tests:
```bash
cargo test -p ferric-runtime or_ce_in_ncc
```

2. Compatibility smoke checks:
```bash
cargo run -p ferric -- check tests/generated/test-suite-segments/co-drtest08-15.clp
cargo run -p ferric -- check tests/generated/test-suite-segments/jnftrght-17.clp
```
Expected: "or CE reached translate_pattern unexpectedly" errors in nested contexts disappear.
