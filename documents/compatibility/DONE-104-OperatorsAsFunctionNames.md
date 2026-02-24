# 104 Operators As Function Names In Actions

## Sequence Position
4/9 (small parser fix; ~19 files combined; no upstream dependencies).

## Behavioral Divergence
CLIPS treats several operator-like tokens as valid function names in action/expression contexts:
- `=` (numeric equality), `<>` (numeric inequality), `<`, `>`, `<=`, `>=`
- `and`, `or`, `not` (logical functions)

Ferric's lexer tokenizes `=` as `Token::Equals` (used for constraint syntax) and treats `&`, `|`, `~` as connective operators. When these appear in function-call position in RHS actions or deffunction bodies, the parser rejects them because `interpret_function_call()` expects the first list element to be a `Symbol`.

Two distinct errors result:
1. **"expected function name (symbol)"** (~8 files): triggered by `(= expr expr)` in action context.
2. **"connectives not allowed in actions"** (~11 files): triggered by `(and expr expr)` or `(or expr expr)` as logical function calls in action context.

Examples from the corpus:
```clips
;; "expected function name" — waltz.clp line 110
(if (= ?delta-x 0) then ...)

;; "connectives not allowed in actions" — tf2_tracked_pose.clp
(do-for-fact ((?robot robot))
   (and (eq ?robot:number value)
        (eq ?robot:team-color value))
   ...)
```

## Apparent Ferric-Side Root Cause
`crates/ferric-parser/src/stage2.rs`:
- `interpret_function_call()` (around line 1972) calls `as_symbol()` on the first element. `Token::Equals` and `Atom::Connective(...)` do not convert to symbols, so the call fails.
- `interpret_action_expr()` (around line 2428) explicitly rejects `Atom::Connective` tokens.

The lexer in `crates/ferric-parser/src/lexer.rs` tokenizes `=` specially (not as a symbol), and `&`/`|`/`~` as connective operators.

## Implementation Plan
1. Extend `interpret_function_call()` to accept operator tokens in function-call position.
- When the first element of a list is `Token::Equals`, treat it as the function name `"="`.
- When the first element is a connective keyword (`and`, `or`, `not`), treat it as a function name.
- Note: `and`/`or`/`not` may already be tokenized as symbols in some contexts — verify the exact token type produced by the lexer for these words.
- Caveat: this only affects function-call position (first element of a list in action context); constraint connectives in pattern context remain unchanged.

2. Ensure evaluator built-ins handle these function names.
- Verify that `=` maps to numeric equality (same as `eq` for numbers, or a dedicated numeric `=` function).
- Verify that `and`, `or`, `not` map to logical conjunction/disjunction/negation in the evaluator.
- These may already be built-in; the issue is purely that parsing rejects them before they reach evaluation.
- Caveat: CLIPS `=` is numeric equality (returns TRUE/FALSE), distinct from `eq` (which also handles strings/symbols).

3. Add parser tests.
- `(= ?x 0)` in action context parses as function call to `=`.
- `(and (eq ?a 1) (eq ?b 2))` in action context parses as function call to `and`.
- `(not (> ?x 5))` in action context parses as function call to `not`.
- Caveat: these tests verify parsing only; evaluation tests should exist elsewhere.

## Test And Verification
1. Parser unit tests:
```bash
cargo test -p ferric-parser interpret_equals_as_function
cargo test -p ferric-parser interpret_connective_as_function
```

2. Compatibility smoke checks:
```bash
cargo run -p ferric-cli -- check tests/examples/clips-official/examples/waltz/waltz.clp
cargo run -p ferric-cli -- check tests/examples/clips-official/examples/sudoku/output-frills.clp
cargo run -p ferric-cli -- check tests/examples/clips-executive/cx_tutorial_agents/clips/tf2_tracked_pose.clp
```
Expected near-term outcome: the "expected function name" and "connectives not allowed in actions" errors disappear from these files; files may still fail for separate reasons (e.g., `or` constraints, `logical` CE).
