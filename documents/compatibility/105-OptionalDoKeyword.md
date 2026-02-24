# 105 Optional Do Keyword In While And Loop-For-Count

## Sequence Position
5/9 (small parser fix; ~4+ files directly, more once other blockers are resolved; no upstream dependencies).

## Behavioral Divergence
CLIPS `while` and `loop-for-count` historically allow the `do` keyword to be omitted:
```clips
;; Standard form (with 'do'):
(while (<= ?i ?n) do
   (bind ?i (+ ?i 1)))

;; Legacy/common form (without 'do'):
(while (<= ?i ?n)
   (bind ?i (+ ?i 1)))
```

Ferric requires the `do` keyword and rejects files that omit it with `"missing do keyword in (while ... do ...)"`. The same issue applies to `loop-for-count`.

Example from the corpus (`utils.clp` lines 57-65):
```clips
(deffunction create-list-of-0s (?n)
    (bind ?list (create$))
    (bind ?i 1)
    (while (<= ?i ?n)
        (bind ?list (create$ ?list 0))
        (bind ?i (+ ?i 1))
    )
    ?list)
```

## Apparent Ferric-Side Root Cause
`crates/ferric-parser/src/stage2.rs` — `interpret_while_expr()` explicitly checks for the `do` keyword after the condition expression and emits an error if it is absent. The same pattern exists in `interpret_loop_for_count_expr()`.

## Implementation Plan
1. Make the `do` keyword optional in `interpret_while_expr()`.
- After parsing the condition, check if the next token is the symbol `do`. If so, consume it and continue. If not, proceed directly to body parsing.
- The condition expression is always parenthesized (a single list form), so the boundary between condition and body is unambiguous even without `do`.
- Caveat: edge cases where the first body action happens to be the symbol `do` used as a variable or function name — CLIPS resolves this by treating `do` as a keyword if it follows the condition.

2. Make the `do` keyword optional in `interpret_loop_for_count_expr()`.
- Same approach as `while`: after parsing the range specification, optionally consume `do`.
- Caveat: same edge case considerations.

3. Add parser tests.
- `(while (cond) do (action))` still works (backward compatible).
- `(while (cond) (action))` now works (no `do`).
- `(loop-for-count (?i 1 10) (action))` now works (no `do`).
- Caveat: parser tests only; runtime behavior of `while`/`loop-for-count` is unchanged.

## Test And Verification
1. Parser unit tests:
```bash
cargo test -p ferric-parser interpret_while
cargo test -p ferric-parser interpret_loop_for_count
```

2. Compatibility smoke checks:
```bash
cargo run -p ferric-cli -- check tests/examples/csp-rules-v2.1/CSP-Rules-Generic/UTIL/utils.clp
cargo run -p ferric-cli -- check tests/examples/clips-executive/extensions/pddl/cx_pddl_bringup/clips/cx_pddl_bringup/raw_agent.clp
```
Expected near-term outcome: "missing do keyword" errors disappear; files may still fail for separate reasons (e.g., `switch`, `logical` CE).
