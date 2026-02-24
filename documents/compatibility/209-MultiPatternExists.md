# 209 Multi-Pattern Exists

## Behavioral Divergence
CLIPS allows `(exists ...)` with multiple sub-patterns:
```clips
(exists (SAD G ?ix1 GCH ?ix2 GCH03 ?var2)
        (test (eq ?var2 "B00")))
```

Ferric only supports single-pattern exists and rejects multi-pattern exists with:
```
compile error: unsupported pattern form 'exists': multi-pattern exists is not supported yet (received 2 patterns)
```

The semantics of `(exists P1 P2 ...)` are: "there exists some combination of facts satisfying P1, P2, ... simultaneously." This is equivalent to `(exists (and P1 P2 ...))`.

## Affected Files (4)
- `generated/test-suite-segments/t63x-rulemisc-22.clp`
- `generated/test-suite-segments/t64x-rulemisc-22.clp`
- `generated/test-suite-segments/co-drtest08-15.clp` (overlaps with fix 208)
- `generated/test-suite-segments/t64x-drtest08-15.clp` (overlaps with fix 208)

## Apparent Ferric-Side Root Cause
`crates/ferric-runtime/src/loader.rs` — the `exists` handler checks the number of sub-patterns and explicitly rejects cases with more than one pattern. The existing desugaring `exists(P)` → `not(not(P))` only handles a single pattern.

For multi-pattern exists, the desugaring should be: `exists(P1, P2, ...)` → `not(not(and(P1, P2, ...)))` — i.e., wrap the patterns in a conjunction, then double-negate.

## Implementation Plan
1. Desugar multi-pattern exists to NCC.
   - When `exists` has multiple sub-patterns, wrap them in an implicit `and`: `exists(P1, P2)` → `exists(and(P1, P2))`.
   - Then apply the standard desugaring: `exists(and(P1, P2))` → `not(not(and(P1, P2)))`.
   - The inner `not(and(P1, P2))` is an NCC (already supported via fix 107/108).
   - The outer `not(NCC)` is a negation of the NCC result — effectively double negation.
   - Caveat: double negation of an NCC requires the Rete network to support negating an NCC node, which may need a new node type or reuse of the existing negation mechanism.

2. Handle mixed patterns including test CEs.
   - `(exists (fact-pattern) (test (expr)))` should be desugared to `not(not(and(fact-pattern, test(expr))))`.
   - The test CE within the NCC needs the fix from 202 (test CE inside NCC).
   - Caveat: this fix depends on fix 202 being implemented first.

3. Add tests.
   - `(exists (a ?x) (b ?x))` — fires when there exist matching `a` and `b` facts sharing a value.
   - `(exists (a ?x) (test (> ?x 0)))` — fires when there exists an `a` fact with a positive value.

## Test And Verification
1. Loader unit tests:
```bash
cargo test -p ferric-runtime multi_pattern_exists
```

2. Compatibility smoke checks:
```bash
cargo run -p ferric -- check tests/generated/test-suite-segments/t63x-rulemisc-22.clp
```
Expected: "multi-pattern exists is not supported yet" error disappears; exists with multiple patterns works correctly.
