# 101 Logical Conditional Element

## Sequence Position
1/9 (highest impact — unblocks ~2,600 files; parser + loader work; no upstream dependencies).

## Behavioral Divergence
CLIPS supports the `(logical ...)` conditional element for truth maintenance. Patterns wrapped with `(logical ...)` provide logical support to facts asserted in the rule's RHS; when the supporting facts are retracted, the dependent facts are automatically retracted too.

Ferric does not recognize `logical` as a CE keyword. The parser falls through to ordered-pattern interpretation, treating `logical` as a relation name. Inner sub-patterns then become "slot constraints" within that fake ordered pattern. List-form constraints inside a slot trigger `"complex constraint expressions not yet supported"`, which is the dominant error in the compatibility corpus (~78% of all ferric-error files).

Example from the corpus:
```clips
(defrule activate-bivalue-chain[10]
   (declare (salience ?*activate-bivalue-chain[10]-salience*))
   (logical
      (play)
      (context (name ?cont))
      (not (deactivate ?cont bivalue-chain)))
   =>
   (assert (deactivate ?cont bivalue-chain)))
```

## Apparent Ferric-Side Root Cause
`crates/ferric-parser/src/stage2.rs` — `interpret_conditional_pattern()` recognizes `and`, `not`, `test`, `exists`, `forall` as CE keywords but not `logical`. The token `logical` is treated as a symbol, and the list form `(logical (play) ...)` enters the ordered-pattern path. The inner `(play)` and `(context ...)` sub-expressions are misinterpreted as list-form slot constraints, hitting the "complex constraint" rejection.

Downstream, even if parsed correctly, `crates/ferric-runtime/src/loader.rs` has no `Pattern::Logical` translation path.

## Implementation Plan
1. Add `Pattern::Logical` variant to the Stage 2 AST.
- Add `Logical(Vec<Pattern>, Span)` to the `Pattern` enum in `crates/ferric-parser/src/stage2.rs`.
- In `interpret_conditional_pattern()`, add a `"logical"` match arm that recursively interprets each sub-expression as a pattern and wraps them in `Pattern::Logical`.
- Allow zero or more sub-patterns (CLIPS permits `(logical)` with no children as a degenerate case, but at least one is typical).
- Caveat: parsing alone does not provide truth maintenance semantics.

2. Add transparent loader pass-through for `Pattern::Logical`.
- In `crates/ferric-runtime/src/loader.rs`, `translate_conditions()` or `translate_pattern()`, handle `Pattern::Logical` by flattening its children into the current condition list — effectively stripping the `logical` wrapper.
- This preserves the pattern-matching behavior without implementing truth maintenance.
- Caveat: facts that should be auto-retracted under truth maintenance will persist. This is a semantic gap but allows files to load and run with correct forward-chaining behavior.

3. Add parser tests for the new `logical` CE.
- Positive test: multi-pattern logical CE parses to `Pattern::Logical` with correct children.
- Positive test: `(logical (not (pattern)))` nests correctly.
- Negative test: empty `(logical)` produces appropriate error or degenerate form.
- Caveat: parser tests do not verify runtime truth maintenance.

4. Add loader-level smoke test.
- Load a rule with `(logical (pattern1) (pattern2))` and verify it compiles and fires.
- Verify that the rule behaves identically to the unwrapped version (modulo truth maintenance).
- Caveat: truth maintenance gap remains; this tests load + forward-chaining only.

## Test And Verification
1. Parser unit tests:
```bash
cargo test -p ferric-parser interpret_logical
```

2. Loader/runtime tests:
```bash
cargo test -p ferric-runtime logical
```

3. Compatibility smoke checks:
```bash
cargo run -p ferric-cli -- check tests/examples/clips-official/test_suite/lgclexe.clp
cargo run -p ferric-cli -- check tests/examples/clips-official/clipsjni/examples/AutoDemo/autodemo.clp
cargo run -p ferric-cli -- check 'tests/examples/csp-rules-v2.1/CSP-Rules-Generic/CHAIN-RULES-COMMON/BIVALUE-CHAINS/Bivalue-Chains[10].clp'
```
Expected near-term outcome: the `"complex constraint expressions"` error from `(logical ...)` disappears; files may still fail for separate reasons (e.g., `?var <-` assignment, `or` CE, `switch`).
