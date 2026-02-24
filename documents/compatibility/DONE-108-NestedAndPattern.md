# 108 Nested And Pattern In Non-Top-Level Position

## Sequence Position
8/9 (medium loader work; ~2 files directly; closely related to 107 but distinct code path).

## Behavioral Divergence
CLIPS allows `(and ...)` CEs in positions other than the top level of a rule's LHS. Ferric currently flattens top-level `(and ...)` into the condition list (fix 007 from the first batch), but rejects `(and ...)` when it appears nested inside other CEs like `(not ...)` or `(or ...)` with:
```
unsupported pattern form `and`: and conditional elements are only supported at the top level
```

Example from the corpus (`joinshre.clp`):
```clips
(defrule example
   (a ?x)
   (not (and (b ?x) (c ?x)))
   =>
   (printout t "No matching b+c for " ?x crlf))
```

This case (`not(and(...))`) overlaps with plan 107, but `(and ...)` can also appear inside `(or ...)`, `(exists ...)`, and other composite CEs.

## Apparent Ferric-Side Root Cause
`crates/ferric-runtime/src/loader.rs` — `translate_pattern()` has a `Pattern::And` handler that only works at the top level (flattening into the parent condition list). When `Pattern::And` appears nested inside another CE, it falls through to the unsupported-form error.

## Implementation Plan
1. Support `Pattern::And` inside `Pattern::Not` (subsumes plan 107).
- When translating `Pattern::Not(Pattern::And(children))`, compile the children as an NCC group.
- This is the same implementation described in plan 107; this plan covers the general case.
- Caveat: plans 107 and 108 overlap; implementing either should resolve both.

2. Support `Pattern::And` inside `Pattern::Or`.
- When translating `Pattern::Or(branches)` where a branch is `Pattern::And(children)`, flatten the `and` within that branch of the duplicated rule.
- This is straightforward if plan 102 (or CE) is implemented first via rule duplication.
- Caveat: depends on plan 102 implementation strategy.

3. Support `Pattern::And` inside `Pattern::Exists` and `Pattern::Forall`.
- `(exists (and P1 P2))` = "there exists some combination satisfying P1 and P2" — compile as an existential over a conjunction.
- `(forall (and P1 P2) Q)` — compile the premise conjunction followed by the consequence.
- Caveat: these may already work if the existing handlers recursively process patterns.

4. Add tests for each nesting combination.
- `(not (and (a) (b)))` — NCC.
- `(or (and (a) (b)) (c))` — disjunction with conjunction branch.
- `(exists (and (a ?x) (b ?x)))` — existential conjunction.
- Caveat: comprehensive nesting testing is combinatorial; focus on patterns found in the corpus.

## Test And Verification
1. Loader unit tests:
```bash
cargo test -p ferric-runtime translate_nested_and
```

2. Compatibility smoke checks:
```bash
cargo run -p ferric-cli -- check tests/examples/clips-official/test_suite/joinshre.clp
cargo run -p ferric-cli -- check tests/examples/clips-official/test_suite/tceplace.clp
```
Expected near-term outcome: "unsupported pattern form `and`" errors in nested position disappear; files may still fail for separate reasons.
