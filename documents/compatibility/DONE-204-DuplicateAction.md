# 204 Duplicate Action

## Behavioral Divergence
CLIPS supports `(duplicate ...)` as an RHS action that creates a copy of an existing fact with specified slot modifications. It works like `(modify ...)` but creates a new fact rather than modifying the existing one. Ferric does not recognize `duplicate` as an action keyword, producing parse errors:
```
interpret error: invalid connectives not allowed in actions
```

Example from the corpus (`dilemma1.clp`):
```clips
(defrule move-alone
  ?node <- (status (search-depth ?num) (farmer-location ?fs))
  (opposite-of ?fs ?ns)
  =>
  (duplicate ?node (search-depth =(+ 1 ?num))
                   (parent ?node)
                   (farmer-location ?ns)
                   (last-move alone)))
```

The `duplicate` action also uses the `=(expr)` return-value constraint form in slot values, which evaluates the expression and uses its result as the slot value.

## Affected Files (6)
- `clips-official/examples/dilemma1.clp`
- `clips-official/test_suite/dilemma1.clp`
- `telefonica-clips/branches/63x/examples/dilemma1.clp`
- `telefonica-clips/branches/63x/test_suite/dilemma1.clp`
- `telefonica-clips/branches/65x/test_suite/dilemma1.clp`
- `fawkes-robotics/src/plugins/clips-navgraph/clips/navgraph.clp`

## Apparent Ferric-Side Root Cause
`crates/ferric-parser/src/stage2.rs` — action parsing does not recognize `duplicate` as an action keyword. The token `duplicate` is treated as a function call, and the slot-modification syntax `(slot-name value)` is misinterpreted.

`crates/ferric-runtime/src/engine.rs` (or `execute_actions`) — even if parsed, there is no `Action::Duplicate` variant or execution path.

The `=(expr)` return-value constraint form in slot values (e.g., `=(+ 1 ?num)`) may also need parser support in the action context.

## Implementation Plan
1. Add `Action::Duplicate` to the AST.
   - In `crates/ferric-parser/src/stage2.rs`, recognize `duplicate` as an action keyword with the same syntax as `modify`: `(duplicate <fact-var> (slot value) ...)`.
   - Parse it identically to `modify` but produce an `Action::Duplicate` node.
   - Caveat: `duplicate` uses the same slot-modification syntax as `modify`, so the parser can reuse that logic.

2. Implement `Action::Duplicate` execution.
   - In the action executor, `duplicate` should:
     a. Look up the fact referenced by the fact variable.
     b. Create a copy of all slot values.
     c. Apply the specified slot modifications to the copy.
     d. Assert the new (modified copy) fact.
     e. Leave the original fact unchanged.
   - Caveat: unlike `modify`, `duplicate` does NOT retract the original fact.

3. Support `=(expr)` return-value constraint in slot values.
   - The `=(+ 1 ?num)` form evaluates the expression and uses the result as the slot value.
   - If not already supported in modify slot values, add expression evaluation for `=()` forms.
   - Caveat: this may already work if the modify parser handles expressions in slot values.

## Test And Verification
1. Parser unit tests:
```bash
cargo test -p ferric-parser parse_duplicate
```

2. Runtime tests:
```bash
cargo test -p ferric-runtime duplicate_action
```

3. Compatibility smoke checks:
```bash
cargo run -p ferric -- check tests/examples/clips-official/examples/dilemma1.clp
cargo run -p ferric -- run tests/examples/clips-official/examples/dilemma1.clp
```
Expected: `dilemma1.clp` loads and runs; the `duplicate` action creates new facts while preserving originals.
