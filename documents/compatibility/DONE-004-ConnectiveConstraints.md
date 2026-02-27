# 004 Connective Constraints

## Sequence Position
4/10 (medium parser/compiler work; prerequisite for complex constraint expressions in plan 005).

## Behavioral Divergence
CLIPS supports inline field/slot constraint connectives (`&`, `|`, `~`) such as:
- `?x&~red`
- `partial-whip|partial-braid`
- `?new-llc&~?zzz`

Ferric currently tokenizes connectives but rejects them during Stage 2 interpretation with `invalid bare connective in pattern`.

## Apparent Ferric-Side Root Cause
`crates/ferric-parser/src/stage2.rs` interprets each pattern field as a single atom-level constraint (`interpret_constraint()`), not as an expression over adjacent connective tokens. As a result, connective tokens are treated as standalone invalid atoms.

Downstream, `crates/ferric-runtime/src/loader.rs` only partially supports compiled connective semantics (for example, `Constraint::Or` is explicitly rejected, and negated non-literal constraints are restricted), so parser-only fixes are insufficient.

## Implementation Plan
1. Add constraint-expression parsing over token sequences.
- Replace per-atom parsing in ordered/template field paths with a sequence parser that consumes operands and connective operators.
- Implement precedence `~` > `&` > `|` and deterministic associativity.
- Preserve existing simple-constraint behavior for non-connective inputs.
- Caveat: parsing improvements alone may still surface compile-time unsupported-constraint errors.

2. Harden error reporting for malformed connective syntax.
- Emit targeted errors for dangling operators (`?x&`), leading infix operators, and invalid operand forms.
- Include source span at the connective site.
- Caveat: better diagnostics do not imply added semantic support.

3. Extend translation pipeline for parsed connective AST.
- `Constraint::And`: continue recursive lowering, including mixed literal/variable terms.
- `Constraint::Not(Variable(...))`: map to beta-side inequality tests (reusing `JoinTestType::NotEqual`).
- `Constraint::Or`: choose and implement one lowering strategy (explicit OR support or safe desugaring), not a parser-only placeholder.
- Caveat: each lowered form may still expose additional engine constraints in larger programs.

4. Add targeted parser + loader tests before external corpus checks.
- Parser tests for `?x&~?y`, `a|b`, `~red`, and mixed connective chains.
- Loader tests for translated inequality/disjunction behavior.
- Caveat: unit coverage may pass while large CLIPS sources still hit separate unsupported features.

## Test And Verification
1. Parser/runtime unit tests:
```bash
cargo test -p ferric-parser interpret_ordered_pattern_with_variables
cargo test -p ferric-runtime translate_or_constraint_returns_compile_error
```
(replace/update expectations as connective support lands)

2. External smoke checks:
```bash
cargo run -p ferric-cli -- check tests/examples/clips-official/examples/wordgame.clp
cargo run -p ferric-cli -- check tests/examples/clips-official/test_suite/mfvmatch.clp
cargo run -p ferric-cli -- check 'tests/examples/csp-rules-v2.1/CSP-Rules-Generic/CHAIN-RULES-MEMORY/BRAIDS/Braids[4].clp'
```
Expected near-term outcome: connective-related parse failures are replaced by later-stage outcomes; full program success is not guaranteed yet.
