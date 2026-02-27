# 106 Or Constraint Compilation

## Sequence Position
6/9 (medium loader/compiler work; ~4+ files directly; benefits from 104 being done first; partially independent of CE-level fixes).

## Behavioral Divergence
CLIPS supports `|` (or) connective constraints within pattern fields and template slots:
```clips
;; Ordered pattern with OR constraint:
(phase initial-output | final-output)

;; Template slot with OR constraint:
(edge (p1 ?p1) (p2 ?p2) (joined true | false))
```

Ferric's parser correctly parses `|` constraints into `Constraint::Or` nodes, but the loader explicitly rejects them at compile time:
```
compile error: unsupported constraint form `or`: or constraints are not supported yet
```

Example from the corpus (`waltz.clp` line 393):
```clips
(edge (p1 ?ep1) (p2 ?ep2) (joined false)
      (label B | - ))
```

Example from `output-frills.clp` line 41:
```clips
(phase initial-output | final-output)
```

## Apparent Ferric-Side Root Cause
`crates/ferric-runtime/src/loader.rs` — `translate_constraint()` has an explicit `Constraint::Or` arm that returns an `unsupported_constraint` error (around line 1351). The parser correctly produces `Constraint::Or` nodes, but the loader does not know how to lower them into Rete alpha-network tests.

## Implementation Plan
1. Implement `Constraint::Or` lowering as multiple alpha tests.
- For literal-only or-constraints like `value1 | value2 | value3`, generate an alpha-level disjunctive test: the field must equal any one of the listed values.
- This can be implemented as a new `AlphaTest::OneOf(Vec<Value>)` variant or by expanding into `AlphaTest::Equal(v1) || AlphaTest::Equal(v2) || ...` at the Rete level.
- Caveat: the Rete alpha node API may need extension to support disjunctive tests.

2. Handle variable-containing or-constraints.
- For `?x | ?y` or mixed `value | ?x`, the semantics are: the field matches if it equals any of the alternatives.
- Variable alternatives require beta-network binding tests (join tests), similar to how `&` constraints with variables work.
- Implement as: try each alternative in order; the constraint passes if any alternative succeeds.
- Caveat: or-constraints with variables that also need binding (`?x | literal`) have complex semantics — does `?x` get bound if the literal alternative matched? In CLIPS, the first alternative that matches determines bindings.

3. Consider rule-duplication fallback for complex cases.
- If alpha-level disjunction is too complex, an alternative is to duplicate the pattern (or rule) for each `or` branch, similar to the `or` CE approach in plan 102.
- This is simpler to implement but increases rule/pattern count.
- Caveat: rule duplication may interact with salience and conflict resolution differently.

4. Add loader and runtime tests.
- Loader: `(phase initial-output | final-output)` compiles successfully.
- Runtime: a rule with `(value red | blue)` fires for both `red` and `blue` facts but not `green`.
- Runtime: or-constraint with variables `(?x | 5)` binds `?x` when the non-literal matches.
- Caveat: comprehensive constraint combination testing is open-ended.

## Test And Verification
1. Loader unit tests:
```bash
cargo test -p ferric-runtime translate_or_constraint
```

2. Runtime integration tests:
```bash
cargo test -p ferric-runtime or_constraint
```

3. Compatibility smoke checks:
```bash
cargo run -p ferric-cli -- check tests/examples/clips-official/examples/waltz/waltz.clp
cargo run -p ferric-cli -- check tests/examples/clips-official/examples/sudoku/output-frills.clp
cargo run -p ferric-cli -- check tests/examples/clips-official/examples/sudoku/output-none.clp
```
Expected near-term outcome: "unsupported constraint form `or`" errors disappear; files may still fail for separate reasons (e.g., `=` as function name, `logical` CE).
