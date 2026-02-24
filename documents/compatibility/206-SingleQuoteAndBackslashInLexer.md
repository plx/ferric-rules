# 206 Single-Quote And Backslash In Lexer

## Behavioral Divergence
CLIPS treats single-quote characters (`'`) and backslash characters (`\`) as valid constituents of symbols and strings. Ferric's lexer rejects them:
```
parse error: 3:16: unexpected character: '\''
parse error: 25:35: unexpected character: '\\'
```

Examples from the corpus:
```clips
;; Single-quote in symbol (drtest03-04):
(but we didn't have time !)

;; Single-quote in rule name (river.clp):
(defrule can't-move-together ...)

;; Backslash as line continuation in CLIPS strings:
(do-for-all-facts ((?action rl-action)) \
   TRUE ...)
```

In CLIPS, `didn't` is a valid symbol — the apostrophe is an ordinary symbol character. Similarly, `can't-move-together` is a valid rule name. Backslash at end of line is used for line continuation in some contexts.

## Affected Files (6)
- `generated/test-suite-segments/co-drtest03-04.clp`
- `generated/test-suite-segments/t64x-drtest03-04.clp`
- `telefonica-clips/branches/63x/examples/benchmarks/river.clp`
- `telefonica-clips/branches/64x/examples/benchmarks/river.clp`
- `clips-executive/extensions/reinforment_learning/cx_rl_clips/clips/cx_rl_clips/get-action-list-srv.clp`
- `clips-executive/extensions/reinforment_learning/cx_rl_clips/clips/cx_rl_clips/get-predefined-observables-srv.clp`

## Apparent Ferric-Side Root Cause
`crates/ferric-parser/src/lexer.rs` — the lexer's character classification does not include `'` (single-quote, U+0027) as a valid symbol character. In CLIPS, the set of valid symbol characters includes everything except whitespace, parentheses, `"`, `;`, `&`, `|`, `~`, and `<` `>` in certain contexts. The single-quote is a legal symbol character.

For backslash: the lexer may not handle `\` as a line continuation or as a valid character in certain contexts. Inside strings, CLIPS uses `\` for escape sequences (`\n`, `\t`, `\"`, etc.). Outside strings, `\` appearing at end of line is a line continuation in batch mode.

## Implementation Plan
1. Add single-quote to the set of valid symbol characters.
   - In the lexer's symbol-scanning logic, include `'` as a character that can appear within a symbol.
   - CLIPS symbol delimiters are: whitespace, `(`, `)`, `"`, `;`, `&`, `|`, `~`, `<`, `>`, `?`, `$`. Everything else (including `'`, `!`, `@`, `#`, `%`, `^`, `*`, `+`, `-`, `.`, `/`, `=`, `{`, `}`, etc.) is valid within a symbol.
   - Caveat: must not break string parsing or comment handling.

2. Handle backslash in string contexts.
   - Inside double-quoted strings, `\` introduces an escape sequence. Ensure the lexer properly handles `\"`, `\\`, `\n`, `\t`, and passes through other `\X` as literal characters (CLIPS behavior).
   - Caveat: CLIPS may treat unrecognized `\X` differently than standard C escape handling.

3. Handle backslash line continuation (lower priority).
   - In batch-mode contexts, `\` at end of line continues the current expression on the next line.
   - This is primarily relevant for REPL/batch mode and may not be needed for `ferric run` (file loading).
   - Caveat: line continuation changes line-number tracking for error reporting.

## Test And Verification
1. Lexer unit tests:
```bash
cargo test -p ferric-parser single_quote_symbol
cargo test -p ferric-parser backslash_in_string
```

2. Compatibility smoke checks:
```bash
cargo run -p ferric -- check tests/generated/test-suite-segments/co-drtest03-04.clp
cargo run -p ferric -- check tests/examples/telefonica-clips/branches/63x/examples/benchmarks/river.clp
```
Expected: "unexpected character" errors for `'` and `\` disappear; files parse successfully.
