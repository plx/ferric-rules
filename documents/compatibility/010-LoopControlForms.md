# 010 Loop Control Forms

## Sequence Position
10/10 (depends on control-flow infrastructure from plan 009; final planned item in this batch).

## Behavioral Divergence
After `if` support, CLIPS loop forms remain a major compatibility gap:
- `while ... do ...`
- `loop-for-count ... do ...`
- `foreach` / `progn$`

These forms are common in automation-heavy and multifield-heavy rule bases. Ferric currently lacks loop execution semantics for them.

## Apparent Ferric-Side Root Cause
Current action/evaluator pipelines do not implement loop special forms or loop-scoped binding behavior. Existing expression evaluation handles function calls but not CLIPS loop grammar markers (`do`, iterator bindings, multifield iteration semantics).

## Implementation Plan
1. Reuse/extend the control-flow representation introduced for `if`.
- Add explicit internal forms for each loop construct (or equivalent robust special-form handling with validated argument grammar).
- Keep parsing and execution strategy consistent with the chosen `if` approach.
- Caveat: representation consistency reduces risk, but loop-specific edge behavior can still diverge.

2. Implement `while` semantics.
- Evaluate condition before each iteration.
- Execute body actions in order while condition remains truthy.
- Respect engine stop signals and existing recursion/step safety limits.
- Caveat: `while` support alone will not cover multifield iterator constructs.

3. Implement `loop-for-count` semantics.
- Parse/count bounds and optional loop variable binding.
- Match CLIPS inclusive range behavior and iteration order.
- Ensure loop variable scope does not leak incorrectly.
- Caveat: off-by-one or scoping differences may still appear in larger external examples.

4. Implement multifield iteration forms (`foreach`, `progn$`).
- Add loop binding over multifield values.
- Execute body with per-item variable binding semantics compatible with CLIPS expectations.
- Ensure nested loop interactions with other control forms are deterministic.
- Caveat: multifield-heavy scripts may still reveal missing helper functions or query behavior not covered by this loop work.

5. Add safety and regression coverage.
- Add tests for finite termination, nested loops, and interaction with `if` and side-effecting actions.
- Add explicit guards/tests for runaway loops (halt behavior and optional step caps).
- Caveat: guardrails prevent hangs but do not guarantee semantic equivalence for every legacy script.

## Test And Verification
1. Unit/integration tests:
```bash
cargo test -p ferric-runtime
cargo test -p ferric-parser
```
(add dedicated tests for each loop form and nested combinations)

2. External smoke checks:
```bash
cargo run -p ferric-cli -- run tests/examples/small-clips-examples/elevator.clp
python scripts/compat-run.py --only-pending --source small-clips-examples
```
Expected near-term outcome: remaining `unsupported-control` files should decrease; newly runnable files may still fail on non-control incompatibilities.
