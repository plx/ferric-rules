# 009 If Then Else Control Flow

## Sequence Position
9/10 (large impact and larger implementation surface; prerequisite for loop-form parity in plan 010).

## Behavioral Divergence
`if/then/else` appears in most example files and is core CLIPS RHS control flow. Ferric currently parses these forms as generic function calls, but execution does not implement CLIPS branch semantics.

As a result, `if` calls fall through action/evaluator paths as unknown behavior rather than selecting branch actions.

## Apparent Ferric-Side Root Cause
- Stage 2 stores RHS forms as plain `FunctionCall` / `ActionExpr` without control-flow-aware structure.
- `execute_single_action()` in `crates/ferric-runtime/src/actions.rs` has no `if` branch.
- Evaluator built-in dispatch in `crates/ferric-runtime/src/evaluator.rs` also lacks CLIPS `if` special-form semantics.

So parser tokenization is present, but execution semantics are missing.

## Implementation Plan
1. Define canonical internal representation for `if`.
- Choose one of:
  - explicit `ActionExpr::If { condition, then_actions, else_actions }`, or
  - special-form `if` handling with strict marker parsing (`then`/`else`) at runtime.
- Apply the same representation consistently across rule actions and deffunction/defmethod bodies.
- Caveat: representation choice may still require follow-up refactors as additional control forms land.

2. Implement parser normalization for CLIPS `if` grammar.
- Enforce `then` delimiter and optional `else` delimiter.
- Support nested `if` bodies and multi-action branches.
- Emit targeted diagnostics for malformed `if` forms.
- Caveat: parse correctness does not guarantee full runtime semantic parity.

3. Implement runtime branch execution semantics.
- Evaluate condition using existing truthiness rules (`is_truthy`).
- Execute only the selected branch in order.
- Preserve side-effect behavior (`assert`, `retract`, `modify`, `printout`, `halt`, etc.).
- Ensure nested `if` and early-stop signals (`reset`/`clear`/`halt`) propagate correctly.
- Caveat: complex branch bodies may still expose unsupported calls unrelated to `if` itself.

4. Add focused regression coverage and corpus smoke checks.
- Add unit tests for:
  - `if` with else,
  - `if` without else,
  - nested `if`,
  - branch-local side effects.
- Re-run compatibility scanning/runs to observe classification movement from `unsupported-control`.
- Caveat: many files include additional unsupported constructs, so `if` support alone will not make all newly testable files fully pass.

## Test And Verification
1. Core tests:
```bash
cargo test -p ferric-runtime
cargo test -p ferric-parser
```
(add dedicated control-flow tests and run by new test names)

2. Compatibility workflow:
```bash
python scripts/compat-scan.py
python scripts/compat-run.py --all
```

3. File-level smoke checks:
```bash
cargo run -p ferric-cli -- run tests/examples/clips-official/examples/sudoku/sudoku.clp
cargo run -p ferric-cli -- run tests/examples/small-clips-examples/elevator.clp
```
Expected near-term outcome: files blocked only by `if` become runnable/checkable; many files will still fail on loop/query or other remaining gaps.
