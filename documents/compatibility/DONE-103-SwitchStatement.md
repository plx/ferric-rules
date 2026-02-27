# 103 Switch Statement

## Sequence Position
3/9 (high impact — 1,023 files statically flagged; medium parser + evaluator + action executor work; independent of CE-level fixes).

## Behavioral Divergence
CLIPS supports the `(switch ...)` control flow construct:
```clips
(switch ?variable
   (case value1 then action1)
   (case value2 then action2)
   (default action-default))
```

Ferric does not parse `switch` as a special form. The scanner (`scripts/compat-scan.py`) flags files containing `switch` as `unsupported-control`, and 1,023 files in the corpus are blocked by this alone. Files that reach the parser fail with cascading errors as `case` and `default` sub-forms are misinterpreted.

Example from the corpus:
```clips
(deffunction ordinal (?n)
   (bind ?q (mod (mod ?n 100) 10))
   (switch ?q
      (case 0 then (bind ?res (sym-cat ?n th)))
      (case 1 then (bind ?res (sym-cat ?n st)))
      (case 2 then (bind ?res (sym-cat ?n nd)))
      (case 3 then (bind ?res (sym-cat ?n rd)))
      (default (bind ?res (sym-cat ?n th))))
   ?res)
```

## Apparent Ferric-Side Root Cause
`crates/ferric-parser/src/stage2.rs` has no special-form handling for `switch` in `interpret_action()` or `interpret_action_expr()`. The keyword falls through to the generic function-call path, where `(case ...)` and `(default ...)` sub-forms fail to parse as arguments.

The evaluator (`crates/ferric-runtime/src/evaluator.rs`) and action executor (`crates/ferric-runtime/src/actions.rs`) have no `Switch` variant in their respective enums.

## Implementation Plan
1. Add `ActionExpr::Switch` variant to the Stage 2 AST.
- Define the variant: `Switch { expr: Box<ActionExpr>, cases: Vec<(ActionExpr, Vec<ActionExpr>)>, default: Option<Vec<ActionExpr>>, span: Span }`.
- Each `case` has a test value and a body (one or more actions after `then`).
- The `default` clause has a body but no test value.
- Caveat: CLIPS allows `case` test values to be symbols, numbers, or strings — the comparison is done with equality.

2. Add `interpret_switch_expr()` parser function.
- Detect `"switch"` in `interpret_action()` (alongside `"if"`, `"while"`, etc.).
- Parse the discriminant expression (first argument after `switch`).
- Iterate remaining sub-lists, matching `(case <value> then <actions>...)` and `(default <actions>...)`.
- The `then` keyword is required in `case` clauses per CLIPS spec.
- Emit clear errors for malformed case clauses (missing `then`, duplicate `default`).
- Caveat: some legacy CLIPS code may omit `then` — check corpus for variation.

3. Add `RuntimeExpr::Switch` to the evaluator.
- In `crates/ferric-runtime/src/evaluator.rs`, add a `Switch` variant with the same structure.
- Implement `from_action_expr()` translation for `ActionExpr::Switch`.
- Implement `eval()` execution: evaluate the discriminant, then iterate cases comparing with equality (using same semantics as `eq`), executing the first matching case body. Fall through to default if no case matches.
- Caveat: CLIPS `switch` does NOT fall through between cases (unlike C); each case is mutually exclusive.

4. Add `switch` handling to the action executor.
- In `crates/ferric-runtime/src/actions.rs`, `execute_single_action()`, add a `"switch"` handler that delegates to the evaluator or implements inline case-matching with action execution.
- Caveat: `switch` bodies may contain side-effecting actions (assert, retract, printout), requiring the full action execution context.

5. Update the compat scanner.
- Remove `"switch"` from `UNSUPPORTED_CONTROL` in `scripts/compat-scan.py`.
- Caveat: some files with `switch` will still fail for other reasons.

6. Add parser and runtime tests.
- Parser: `(switch ?x (case 1 then (a)) (case 2 then (b)) (default (c)))` produces correct AST.
- Runtime: switch selects correct case; default fires when no case matches; empty switch returns nil.
- Caveat: edge cases with complex case values and nested switches.

## Test And Verification
1. Parser unit tests:
```bash
cargo test -p ferric-parser interpret_switch
```

2. Runtime unit tests:
```bash
cargo test -p ferric-runtime switch
```

3. Compatibility smoke checks:
```bash
cargo run -p ferric-cli -- check tests/examples/csp-rules-v2.1/CSP-Rules-Generic/UTIL/utils.clp
cargo run -p ferric-cli -- check tests/examples/rcll-refbox/src/games/rcll/production.clp
```
Expected near-term outcome: `switch`-related parse failures disappear; files may still fail for separate reasons (e.g., `logical` CE, multi-file dependencies).
