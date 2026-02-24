# 202 Test CE Inside NCC and Exists

## Behavioral Divergence
CLIPS allows `(test ...)` CEs inside `(not (and ...))` NCC groups, inside `(exists ...)`, and as sub-patterns of `(forall ...)`. Ferric rejects these with:
```
compile error: unsupported pattern form 'test': test CE reached translate_pattern unexpectedly (should be handled earlier)
```

Examples from the corpus:
```clips
;; test CE inside not(and(...)):
(defrule should-fire-4a
  (not (and (test (< 5 3))))
  =>)

;; test CE inside exists(...):
(defrule should-fire-5a
  (exists (test (< 3 5)))
  =>)

;; test CE inside forall:
(defrule should-fire-6b
  (forall (initial-fact) (test (> 5 3)))
  =>)

;; test CE inside not(and(...)) with variables:
(defrule crash
  (p ?X)
  (not (test (eq ?X 1)))
  (p ?Y)
  (not (and (test (neq ?Y 20)) (test (neq ?Y 30))))
  =>)
```

## Affected Files (~8)
- `clips-official/test_suite/pataddtn.clp`
- `telefonica-clips/branches/63x/test_suite/pataddtn.clp`
- `telefonica-clips/branches/64x/test_suite/pataddtn.clp`
- `telefonica-clips/branches/65x/test_suite/pataddtn.clp`
- `generated/test-suite-segments/co-drtest08-47.clp`
- `generated/test-suite-segments/t64x-drtest08-47.clp`

## Apparent Ferric-Side Root Cause
`crates/ferric-runtime/src/loader.rs` — the `translate_conditions()` function extracts `test` CEs at the top level and converts them to join-node test expressions. However, when `test` CEs appear nested inside `not(and(...))`, `exists(...)`, or `forall(...)`, they are passed to `translate_pattern()` which does not handle them.

The NCC compilation path in the loader processes the inner patterns of `not(and(...))` by calling `translate_pattern()` on each child, but `Pattern::Test` is not a fact pattern — it should be extracted as a join test within the NCC sub-chain.

Similarly, `exists(...)` and `forall(...)` call into pattern translation for their sub-patterns without first extracting test CEs.

## Implementation Plan
1. Extract `test` CEs within NCC sub-chains.
   - When compiling `not(and(P1, P2, ...))`, scan the inner patterns for `Pattern::Test` entries.
   - Convert each test CE to a join-level test expression attached to the appropriate node in the NCC sub-chain.
   - For standalone `(not (test (expr)))`, compile as a negated join test: the rule fires when the test expression is false.
   - Caveat: variable scoping — test expressions inside NCCs may reference variables bound in the outer rule context or within the NCC itself.

2. Handle `test` CEs inside `exists(...)`.
   - `(exists (test (< 3 5)))` is degenerate: it means "the test is true." Since `exists` is desugared to `not(not(...))`, the test should be compiled as a join test within the inner negation.
   - `(exists (fact-pattern) (test (expr)))` means "there exists a fact matching the pattern such that the test holds."
   - Caveat: `exists` desugaring to double negation interacts with test CE placement.

3. Handle `test` CEs inside `forall(...)`.
   - `forall(P, Q)` is desugared to `not(and(P, not(Q)))`. If Q is a `test` CE, it becomes a test within the inner negation of the NCC.
   - Caveat: must ensure the forall desugaring preserves test CEs through the transformation.

4. Add tests.
   - `(not (and (test (< 5 3))))` — fires (since 5 < 3 is false, the NCC has no matches, so the negation succeeds).
   - `(exists (test (< 3 5)))` — fires (3 < 5 is true).
   - `(not (test (eq ?X 1)))` — fires when `?X` is not 1.
   - `(forall (initial-fact) (test (> 5 3)))` — fires.

## Test And Verification
1. Loader unit tests:
```bash
cargo test -p ferric-runtime test_ce_in_ncc
```

2. Compatibility smoke checks:
```bash
cargo run -p ferric -- check tests/examples/clips-official/test_suite/pataddtn.clp
cargo run -p ferric -- check tests/generated/test-suite-segments/co-drtest08-47.clp
```
Expected: "unsupported pattern form 'test'" errors inside NCC/exists/forall contexts disappear.
