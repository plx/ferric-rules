# 205 Empty Multislot In Deffacts

## Behavioral Divergence
CLIPS allows empty multislot values in deffacts assertions. When a template has a multislot, providing the slot name with no values assigns an empty multifield:
```clips
(deftemplate foo (multislot bar) (multislot yak))
(deffacts init
   (foo (bar) (yak)))   ;; both bar and yak get empty multifield values
```

Ferric rejects this with:
```
interpret error: missing value for slot in fact at line 3, column 9
```

Additionally, the `(default (create$))` syntax for specifying an empty multifield as a slot default value is rejected:
```clips
(deftemplate state (multislot items (default (create$))))
```
Ferric produces: `interpret error: expected default value at line N, column M`

## Affected Files (~7)
- `generated/test-suite-segments/co-drtest09-64.clp`
- `generated/test-suite-segments/t63x-drtest09-64.clp`
- `generated/test-suite-segments/t64x-drtest09-64.clp`
- `generated/test-suite-segments/t64x-drtest10-15.clp`
- `generated/test-suite-segments/t65x-drtest10-15.clp`
- `clips-executive/extensions/reinforment_learning/cx_rl_clips/clips/cx_rl_clips/deftemplates.clp`
- `fawkes-robotics/src/plugins/clips/clips/blackboard.clp`

## Apparent Ferric-Side Root Cause
Two related issues:

1. **Empty multislot in deffacts:** `crates/ferric-runtime/src/loader.rs` or the deffacts interpreter — when parsing a fact assertion like `(foo (bar))`, the slot parser expects at least one value after the slot name. For multislots, an empty parenthesized slot name `(bar)` is valid and means "empty multifield."

2. **`(default (create$))` in deftemplate:** `crates/ferric-parser/src/stage2.rs` — the default-value parser for template slot facets does not recognize function calls like `(create$)` as valid default expressions. It expects a literal value or the special tokens `?DERIVE`, `?NONE`.

## Implementation Plan
1. Allow empty multislot values in fact assertions.
   - In the fact parser/interpreter, when a slot name is followed by `)` with no intervening values, and the slot is a multislot, assign an empty multifield value `()`.
   - For single-field slots, an empty `(slot-name)` should remain an error (single-field slots require exactly one value).
   - Caveat: need to distinguish multislot vs. single-field slot to decide whether empty is valid.

2. Support `(default (create$))` and `(default (create$ values...))` in deftemplate facets.
   - The `(default ...)` facet should accept function-call expressions, not just literals.
   - `(create$)` evaluates to an empty multifield — this is the standard way to specify "default empty" for a multislot.
   - `(create$ 0 0)` evaluates to a multifield containing `0 0`.
   - Caveat: full expression evaluation at template-definition time may be complex; a simpler approach is to recognize `(create$)` specially as "empty multifield default."

3. Add tests.
   - `(deftemplate foo (multislot bar)) (deffacts d (foo (bar)))` — loads without error; bar gets empty multifield.
   - `(deftemplate foo (multislot bar (default (create$))))` — loads without error; default is empty multifield.
   - `(deftemplate foo (slot x)) (deffacts d (foo (x)))` — should still error (single-field slot with no value).

## Test And Verification
1. Parser/loader unit tests:
```bash
cargo test -p ferric-runtime empty_multislot
```

2. Compatibility smoke checks:
```bash
cargo run -p ferric -- check tests/generated/test-suite-segments/co-drtest09-64.clp
cargo run -p ferric -- check tests/examples/fawkes-robotics/src/plugins/clips/clips/blackboard.clp
```
Expected: "missing value for slot" and "expected default value" errors disappear for multislot contexts.
