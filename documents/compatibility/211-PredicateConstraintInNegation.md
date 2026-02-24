# 211 Predicate Constraint In Negation And Complex Patterns

## Behavioral Divergence
CLIPS allows predicate constraints (`:()`) and return-value constraints (`=()`) within negated and complex constraint forms. Ferric's pattern validator rejects several valid CLIPS constraint patterns:

1. **Predicate inside or-constraint with function calls:**
```clips
;; zebra.clp — complex constraint chain:
(avh (a pet) (v fox)
     (h ?p3&~?p2&~?p1&:(or (= ?s2 (- ?p3 1)) (= ?s2 (+ ?p3 1)))))
```
The `:(or ...)` is a predicate constraint using `or` as a boolean function (not the `|` or-constraint), combined with variable negation constraints.

2. **Forward variable reference in constraint:**
```clips
;; misclns1 — forward reference:
?f <- (x ?y&?x)    ;; ?x referenced before binding
```

3. **Type-constraint warnings treated as errors:**
```clips
;; fctpcstr.clp — CLIPS warns but compiles:
(defrule error-1
  (foo (x ?x) (y ?x))    ;; CLIPS warns about type mismatch, still compiles
  =>)
```

Ferric produces various errors: `pattern validation failed`, `unsupported constraint form`, or compile errors for these patterns.

## Affected Files (~17)
- `clips-official/examples/zebra.clp`
- `clips-official/test_suite/fctpcstr.clp`
- `clips-official/test_suite/zebra.clp`
- `telefonica-clips/branches/63x/examples/zebra.clp`
- `telefonica-clips/branches/63x/test_suite/fctpcstr.clp`
- `telefonica-clips/branches/63x/test_suite/zebra.clp`
- `telefonica-clips/branches/64x/test_suite/fctpcstr.clp`
- `telefonica-clips/branches/65x/test_suite/fctpcstr.clp`
- `telefonica-clips/branches/65x/test_suite/zebra.clp`
- `generated/test-suite-segments/co-misclns1-00.clp`
- `generated/test-suite-segments/co-misclns1-11.clp`
- `generated/test-suite-segments/t63x-misclns1-00.clp`
- `generated/test-suite-segments/t63x-misclns1-11.clp`
- `generated/test-suite-segments/co-rulemisc-14.clp`
- `generated/test-suite-segments/t63x-rulemisc-14.clp`
- `generated/test-suite-segments/t64x-rulemisc-14.clp`

## Apparent Ferric-Side Root Cause
Multiple locations:

1. `crates/ferric-runtime/src/loader.rs` — the constraint compiler rejects predicate constraints that use `or` as a function name within `:(...)`. The `or` symbol is treated as a connective operator rather than a boolean function.

2. `crates/ferric-runtime/src/loader.rs` — the constraint validator rejects forward variable references (`?y&?x` where `?x` is not yet bound in a prior pattern).

3. `crates/ferric-runtime/src/loader.rs` — type-constraint validation is strict (hard error) where CLIPS issues only a warning.

## Implementation Plan
1. Allow `or` as a function name in predicate constraints.
   - `:(or (= ?s2 X) (= ?s2 Y))` — the `or` here is the boolean function `or`, not the `|` connective constraint.
   - The predicate constraint evaluator should treat `or` as a callable function in this context.
   - Caveat: must distinguish between `|` (constraint connective) and `or` (boolean function).

2. Relax forward variable reference validation.
   - In constraint chains like `?y&?x`, if `?x` is bound later in the same pattern or in a prior pattern, accept it and generate a join test.
   - CLIPS handles this by deferring the binding test to the join level.
   - Caveat: truly unbound variables (never bound anywhere) should still be flagged.

3. Downgrade type-constraint violations to warnings.
   - When CLIPS would issue `[RULECSTR2]` or `[CSTRNPSR1]` warnings, ferric should warn but continue compilation.
   - Caveat: some type violations may indicate genuine errors; warnings allow the user to decide.

## Test And Verification
1. Loader unit tests:
```bash
cargo test -p ferric-runtime predicate_constraint_complex
```

2. Compatibility smoke checks:
```bash
cargo run -p ferric -- check tests/examples/clips-official/examples/zebra.clp
cargo run -p ferric -- run tests/examples/clips-official/examples/zebra.clp
cargo run -p ferric -- check tests/examples/clips-official/test_suite/fctpcstr.clp
```
Expected: `zebra.clp` loads and runs correctly. Type-constraint warnings are emitted but do not prevent compilation.
