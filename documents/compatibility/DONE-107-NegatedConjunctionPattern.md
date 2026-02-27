# 107 Negated Conjunction Pattern

## Sequence Position
7/9 (medium Rete compiler work; ~2 files directly; independent of other fixes but benefits from broader CE support).

## Behavioral Divergence
CLIPS supports `(not (and P1 P2 ...))` — a negated conjunction meaning "there is no combination of facts simultaneously satisfying P1, P2, etc." This is semantically distinct from `(not P1) (not P2)` (which means "no fact satisfies P1 AND no fact satisfies P2").

Ferric rejects this with `"unsupported pattern form not/and: nested negation in and"`.

Example from the corpus (`sudoku.clp` lines 133-135):
```clips
(not (and (size-value (value ?v2&:(< ?v2 ?v)))
          (not (possible (row ?r) (column ?c) (value ?v2)))))
```

This means: "There is no value v2 < v such that v2 is NOT a possible value at position (r,c)." In other words: "All smaller values are still possible at this position."

## Apparent Ferric-Side Root Cause
`crates/ferric-runtime/src/loader.rs` — the `translate_pattern()` function handles `Pattern::Not(inner)` but when `inner` is `Pattern::And(...)`, it falls into an unsupported-form error rather than compiling the nested conjunction.

The Rete compiler in `crates/ferric-core/src/compiler.rs` supports simple negation (NCC with a single pattern) and `forall` (which desugars to NCC), but does not support multi-pattern NCC directly through the `not(and(...))` syntax path.

## Implementation Plan
1. Recognize `(not (and P1 P2 ...))` as a valid NCC (Negated Conjunctive Condition).
- In `crates/ferric-runtime/src/loader.rs`, when `Pattern::Not` wraps a `Pattern::And`, extract the inner patterns and compile them as an NCC group.
- This is the same Rete structure used for `forall` desugaring: a beta-network sub-chain whose aggregate result is negated.
- Caveat: the existing NCC implementation may need to support arbitrary nesting depth.

2. Translate inner `and` patterns to Rete NCC nodes.
- Each pattern within the `and` becomes a node in the NCC sub-chain.
- Variable bindings from the outer context are available within the NCC patterns.
- The NCC fires (contributes to rule activation) when NO complete set of matches exists for the inner conjunction.
- Caveat: variable scoping between the outer rule and the NCC sub-chain needs careful handling.

3. Handle the doubly-nested case `(not (and P1 (not P2)))`.
- This is common in the corpus (sudoku.clp uses it): "not exists P1 where not P2" = "for all P1, P2 holds" (effectively a forall).
- The existing `forall` desugaring already handles this pattern; verify that the NCC approach produces equivalent Rete topology.
- Caveat: if the code path diverges from `forall`, ensure equivalence through testing.

4. Add loader and runtime tests.
- Loader: `(not (and (a ?x) (b ?x)))` compiles successfully.
- Runtime: the pattern fires when no pair of (a, b) facts share a value.
- Runtime: `(not (and (a ?x) (not (b ?x))))` fires when every `a` fact has a matching `b` fact.
- Caveat: NCC tests require careful setup of fact populations.

## Test And Verification
1. Loader unit tests:
```bash
cargo test -p ferric-runtime translate_negated_conjunction
```

2. Runtime integration tests:
```bash
cargo test -p ferric-runtime negated_conjunction
```

3. Compatibility smoke checks:
```bash
cargo run -p ferric-cli -- check tests/examples/clips-official/clipsjni/examples/SudokuDemo/sudoku.clp
cargo run -p ferric-cli -- check tests/examples/clips-official/test_suite/sudoku.clp
```
Expected near-term outcome: "unsupported pattern form `not/and`" errors disappear; files may still fail for separate reasons (e.g., `or` constraints, unknown templates).
