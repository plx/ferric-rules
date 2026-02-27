# 210 Deeply Nested NCC (not-and-not-and)

## Behavioral Divergence
CLIPS supports arbitrary nesting depth of `not(and(...))` NCC groups, including patterns like `not(and(P, not(and(Q, R))))` — a negated conjunction containing an inner negated conjunction. Ferric supports single-level `not(and(...))` (fix 107/108) but fails on multi-level nesting with:
```
compile error: unsupported pattern form 'and': and conditional elements are only supported inside (not (and ...))
```

Examples from the corpus:
```clips
;; Double-nested NCC (tceplace.clp):
(defrule foo1
  (a)
  (not (and (not (and (b) (c)))
            (test (< 3 5))))
  =>)

;; Triple-nested NCC (jnftrght):
(defrule problem-rule-1
   (A)
   (not (and (B) (not (and (C) (D)))))
   (not (E))
   =>)

;; Deeply nested with double negation (jnftrght-20):
(defrule problem-rule-5
   (A ?td2)
   (not (and (not (and (B) (C ?td2)))
             (not (and (D) (not (E ?td2))))))
   =>)
```

These deeply nested structures express complex logical conditions like "for all X where P(X), Q(X) holds" and "it's not the case that (not(B and C) and not(D and not E))".

## Affected Files (~10)
- `clips-official/test_suite/tceplace.clp`
- `telefonica-clips/branches/63x/test_suite/tceplace.clp`
- `telefonica-clips/branches/64x/test_suite/tceplace.clp`
- `telefonica-clips/branches/65x/test_suite/tceplace.clp`
- `generated/test-suite-segments/jnftrght-15.clp`
- `generated/test-suite-segments/jnftrght-16.clp`
- `generated/test-suite-segments/jnftrght-18.clp`
- `generated/test-suite-segments/jnftrght-20.clp`

## Apparent Ferric-Side Root Cause
`crates/ferric-runtime/src/loader.rs` — the NCC compilation path handles `not(and(children...))` by compiling the children into an NCC sub-chain. However, when a child is itself `not(and(...))`, the recursive compilation fails because the inner `and` CE reaches `translate_pattern()` which doesn't handle it recursively in NCC context.

The Rete network in `crates/ferric-core/src/compiler.rs` supports NCC nodes, but the loader does not recursively build nested NCC sub-chains.

## Implementation Plan
1. Recursive NCC compilation.
   - When compiling children of `not(and(...))`, if a child is `not(and(...))`, recursively compile it as a nested NCC sub-chain.
   - The inner NCC becomes a node within the outer NCC sub-chain, with its result (negation) feeding into the outer chain.
   - Caveat: the Rete network's NCC node implementation may need to support nesting — an NCC node within an NCC sub-chain.

2. Flatten double negation where possible.
   - `not(and(not(and(P, Q))))` = `exists(P and Q)` = the conjunction P, Q has at least one match.
   - Where double negation can be detected, simplify to avoid unnecessary NCC nesting.
   - Caveat: simplification is optional; the recursive approach works without it.

3. Handle variable scoping across nesting levels.
   - Variables bound in the outer rule context are visible to all NCC levels.
   - Variables bound within an NCC sub-chain are only visible within that sub-chain and inner levels.
   - Caveat: binding visibility rules must be carefully tracked during recursive compilation.

## Test And Verification
1. Loader unit tests:
```bash
cargo test -p ferric-runtime nested_ncc
```

2. Compatibility smoke checks:
```bash
cargo run -p ferric -- check tests/examples/clips-official/test_suite/tceplace.clp
cargo run -p ferric -- check tests/generated/test-suite-segments/jnftrght-15.clp
cargo run -p ferric -- check tests/generated/test-suite-segments/jnftrght-20.clp
```
Expected: "and conditional elements are only supported inside (not (and ...))" errors at nested levels disappear.
