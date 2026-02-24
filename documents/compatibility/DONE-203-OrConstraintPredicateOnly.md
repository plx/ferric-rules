# 203 Or-Constraint With Predicate-Only Alternatives

## Behavioral Divergence
CLIPS allows or-constraints (`|`) composed entirely of predicate constraints (`:(...)`), with no literal or variable alternative required. Ferric's or-constraint compiler (implemented in fix 106) requires at least one literal or variable alternative, rejecting predicate-only or-constraints with:
```
compile error: unsupported constraint form 'or': or constraints require at least one literal or variable alternative
```

Also related: negation of multi-field variables (`~$?x`) and multifield-variable or-constraints (`$?y|$?x`) are rejected.

Examples from the corpus (`mfvmatch.clp`):
```clips
;; Predicate-only or-constraint:
(factoid a $?x&:(< (length$ ?x) 3)|:(> (length$ ?x) 3))

;; Negated multifield variable:
(factoid c $?x&:(= (length$ ?x) 1) $?y&~$?x $?z $?w&$?y|$?x)
```

## Affected Files (4)
- `clips-official/test_suite/mfvmatch.clp`
- `telefonica-clips/branches/63x/test_suite/mfvmatch.clp`
- `telefonica-clips/branches/64x/test_suite/mfvmatch.clp`
- `telefonica-clips/branches/65x/test_suite/mfvmatch.clp`

## Apparent Ferric-Side Root Cause
`crates/ferric-runtime/src/loader.rs` — the `translate_or_constraint()` function (added in fix 106) lowered or-constraints by collecting literal/variable alternatives into `AlphaTest::OneOf` and treating predicate alternatives as additional beta-level tests. But it validates that at least one alternative is a literal or variable, rejecting pure-predicate or-constraints.

Additionally, `translate_constraint()` does not support `Constraint::Not(Constraint::MultiVariable(...))` (`~$?x`), and multifield-variable alternatives in or-constraints (`$?y|$?x`) are not handled.

## Implementation Plan
1. Allow predicate-only or-constraints.
   - When all alternatives in an or-constraint are predicate constraints (`:(expr1)|:(expr2)`), compile them as a disjunctive join test: the constraint passes if any predicate is true.
   - Generate `TestExpr::Or(vec![pred1, pred2, ...])` at the join level.
   - Caveat: this is a purely join-level constraint with no alpha-level filtering.

2. Support negated multifield variables (`~$?x`).
   - `$?y&~$?x` means "the multifield bound to `$?y` must not equal the multifield bound to `$?x`."
   - Compile as a join-level inequality test between the two multifield bindings.
   - Caveat: requires multifield equality comparison at the join level.

3. Support multifield-variable alternatives in or-constraints (`$?y|$?x`).
   - `$?w&$?y|$?x` means "the multifield bound to `$?w` must equal either `$?y` or `$?x`."
   - Compile as `TestExpr::Or(vec![eq($?w, $?y), eq($?w, $?x)])` at the join level.
   - Caveat: depends on multifield variable binding and comparison infrastructure.

## Test And Verification
1. Unit tests:
```bash
cargo test -p ferric-runtime predicate_only_or_constraint
```

2. Compatibility smoke checks:
```bash
cargo run -p ferric -- check tests/examples/clips-official/test_suite/mfvmatch.clp
```
Expected: the "or constraints require at least one literal or variable alternative" error disappears; `mfvmatch.clp` may still fail for other multifield-related reasons.
