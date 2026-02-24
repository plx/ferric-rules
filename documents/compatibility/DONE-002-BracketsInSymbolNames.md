# 002 Brackets In Symbol Names

## Sequence Position
2/10 (small lexer change; depends only on baseline parser behavior).

## Behavioral Divergence
CLIPS permits `[` and `]` inside symbol names. Ferric lexing currently treats these characters as invalid, producing `unexpected character: '['`.

This blocks a large set of CSP-Rules files that encode parameterized names like `Templates[1]`, `gWhips[10]`, and bracketed salience globals.

## Apparent Ferric-Side Root Cause
`crates/ferric-parser/src/lexer.rs` uses `is_symbol_char()` to define valid symbol characters. The allow-list includes `{` and `}` but omits `[` and `]`.

`lex_symbol()`, `lex_variable()`, and `lex_multivar()` all rely on this helper, so the omission affects symbols and variable-like identifiers consistently.

## Implementation Plan
1. Extend lexical symbol character set.
- In `is_symbol_char()`, add `[` and `]`.
- Keep all other tokenization rules unchanged.
- Caveat: this only enables tokenization; files may still fail in later parser/runtime stages.

2. Add direct lexer tests for bracket-bearing identifiers.
- Symbol token test: `Templates[1]` should lex as `Token::Symbol("Templates[1]")`.
- Variable test: `?x[2]` should remain a single variable token (if valid in CLIPS usage).
- Global var test: `?*partial-OR2-gwhip[10]-salience-1*` should lex as one global variable token.
- Caveat: token-level success does not guarantee that all grammar contexts accept those identifiers.

3. Add parser integration regression.
- Create/extend a small `defrule` parse test where rule name or relation contains brackets.
- Confirm Stage 2 interpretation completes with no parse errors.
- Caveat: parse success may still reveal unsupported features during compilation.

4. Run representative external smoke checks.
- Use bracket-heavy files from CSP-Rules to ensure the lexer no longer fails early.
- Caveat: these files often also require connective/control-flow support, so full success is not expected yet.

## Test And Verification
1. Lexer/parser unit tests:
```bash
cargo test -p ferric-parser lex_connectives
cargo test -p ferric-parser interpret_rule_construct
```
(add bracket-specific tests and run them directly)

2. Compatibility smoke checks:
```bash
cargo run -p ferric-cli -- check 'tests/examples/csp-rules-v2.1/SudoRules-V20.1/TEMPLATES/Templates[1].clp'
cargo run -p ferric-cli -- check 'tests/examples/csp-rules-v2.1/CSP-Rules-Generic/CHAIN-RULES-EXOTIC/PARTIAL-OR2-G-WHIPS/Partial-OR2-gWhips[10].clp'
```
Expected near-term outcome: the bracket-character parse error is eliminated, possibly exposing the next incompatibility category.
