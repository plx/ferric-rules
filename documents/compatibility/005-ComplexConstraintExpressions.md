# 005 Complex Constraint Expressions

## Sequence Position
5/10 (medium, depends on connective parsing from plan 004).

## Behavioral Divergence
CLIPS allows richer constraint forms in pattern fields, including computed and predicate-style forms (for example `=<expr>` and `:<expr>` inside constraint chains).

Ferric currently rejects list-form constraints with `complex constraint expressions not yet supported`.

## Apparent Ferric-Side Root Cause
`interpret_constraint()` in `crates/ferric-parser/src/stage2.rs` immediately rejects list-shaped `SExpr` values, so Stage 2 cannot represent predicate/computed constraints at all.

Even after parsing, loader translation only supports a narrow subset of constraint lowering in `translate_constraint()` (`crates/ferric-runtime/src/loader.rs`), so expression-bearing constraints need explicit lowering behavior.

## Implementation Plan
1. Extend Stage 2 constraint model to represent computed/predicate constraints.
- Add explicit variants for expression-backed constraints (for example predicate constraint and computed-equality constraint).
- Parse `:<expr>` and `=<expr>` forms from connective sequences and preserve source spans.
- Caveat: adding AST variants alone may still leave translation/runtime unsupported.

2. Implement lowering strategy in loader translation.
- For constraints not representable as static alpha/beta tests, lower to generated runtime test expressions bound to field/slot variables.
- Thread generated tests into existing `test_conditions` execution path so evaluation occurs at rule-firing time.
- Caveat: generated test lowering may still miss some CLIPS edge semantics (especially around side effects and evaluation order).

3. Define explicit unsupported boundary and diagnostics for out-of-scope forms.
- Where exact CLIPS semantics are still pending, fail with clear `unsupported constraint form` diagnostics (with spans) instead of generic parser errors.
- Caveat: improved failure mode still means some files remain incompatible until remaining forms are implemented.

4. Add regression tests focused on expression constraints.
- Parser tests for both computed equality and predicate constraints in ordered/template patterns.
- Loader/runtime tests that these constraints gate rule firing correctly.
- Caveat: targeted regressions do not guarantee complete compatibility with all CSP-Rules macros.

## Test And Verification
1. Unit tests:
```bash
cargo test -p ferric-parser
cargo test -p ferric-runtime
```
(Add focused tests for newly introduced expression-backed constraint variants.)

2. External smoke checks:
```bash
cargo run -p ferric-cli -- check tests/examples/csp-rules-v2.1/CSP-Rules-Generic/GENERAL/is-cspvar-for-cand.clp
cargo run -p ferric-cli -- check tests/examples/csp-rules-v2.1/CSP-Rules-Generic/CHAIN-RULES-EXOTIC/SYMMETRIFY-ORk/symmetrify-OR2-relations.clp
```
Expected near-term outcome: the specific `complex constraint expressions` parser error is eliminated; additional unsupported constructs may still fail these files.
