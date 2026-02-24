# 109 Multi-Variable In Template Slot Constraints

## Sequence Position
9/9 (small-medium loader work; ~2 files directly; independent of other fixes).

## Behavioral Divergence
CLIPS allows multi-field variables (`$?var`) in template slot constraints, particularly in multislot positions:
```clips
(deftemplate example (multislot items))
(defrule match-items
   (example (items $?before ?target $?after))
   =>
   (printout t "Found " ?target crlf))
```

Ferric rejects multi-field variables in template slot constraints with:
```
unsupported constraint form `multi-variable`: multi-field variable `$?x` is not supported in template slot constraints
```

Example from the corpus (`mfvmatch.clp`):
```clips
(deftemplate data (multislot values))
(defrule match
   (data (values $?x ?y $?z))
   =>
   ...)
```

Also seen in `globltst.clp` with `$?y` in a template context.

## Apparent Ferric-Side Root Cause
`crates/ferric-runtime/src/loader.rs` — `translate_constraint()` has a `Constraint::MultiVariable` arm that returns an `unsupported_constraint` error. The parser correctly produces `Constraint::MultiVariable` nodes for `$?var` in slot positions, but the loader does not know how to compile them into Rete alpha/beta tests.

Multi-field variables in template slots require the Rete network to handle variable-length matching within a slot's value list, which is more complex than single-field slot constraints.

## Implementation Plan
1. Determine scope of multi-field variable support needed.
- CLIPS multislot values are stored as ordered lists. `$?var` matches zero or more elements.
- In template slot constraints, `$?var` can appear as: the entire slot value (`(slot $?x)`), or as part of a decomposition (`(slot $?before ?mid $?after)`).
- The simpler case (entire slot) can be handled by binding the variable to the full slot value.
- The decomposition case requires pattern-matching against the slot's value list, potentially with backtracking.
- Caveat: full decomposition matching is substantially more complex than single-field matching.

2. Implement the simple case: `$?var` as entire multislot value.
- When a multislot constraint is a single `$?var`, bind the variable to the slot's complete multifield value.
- This is the most common use case and sufficient for many corpus files.
- Caveat: does not handle decomposition patterns.

3. Implement decomposition matching (if needed by corpus).
- For `(slot $?a ?x $?b)`, generate a loop over possible split points in the slot value list.
- Each split point produces a potential binding for `$?a`, `?x`, and `$?b`.
- Integrate with the Rete network's token propagation to try each split.
- Caveat: this adds combinatorial complexity to pattern matching; may need backtracking or generate-and-test approach.

4. Add loader and runtime tests.
- Simple case: `(data (values $?all))` compiles and binds `$?all` to the full value list.
- Decomposition: `(data (values $?a ?x $?b))` correctly splits the value list.
- Caveat: decomposition tests depend on implementation depth.

## Test And Verification
1. Loader unit tests:
```bash
cargo test -p ferric-runtime translate_multivariable_constraint
```

2. Runtime integration tests:
```bash
cargo test -p ferric-runtime multivariable_slot
```

3. Compatibility smoke checks:
```bash
cargo run -p ferric-cli -- check tests/examples/clips-official/test_suite/mfvmatch.clp
cargo run -p ferric-cli -- check tests/examples/clips-official/test_suite/globltst.clp
```
Expected near-term outcome: "unsupported constraint form `multi-variable`" errors disappear for the simple case; decomposition support may be deferred.
