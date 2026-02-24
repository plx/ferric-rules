# 201 If Slot Name Collision

## Behavioral Divergence
CLIPS allows `if` as a valid slot name in deftemplates. The WineDemo benchmark defines:
```clips
(deftemplate rule
  (multislot if)
  (multislot then))
```
and uses it in rules like:
```clips
(defrule throw-away-ands-in-antecedent
  ?f <- (rule (if and $?rest))
  =>
  (modify ?f (if ?rest)))
```

Ferric's parser sees `(if` and interprets it as the beginning of an `if/then/else` control flow construct rather than a slot name, producing:
```
interpret error: missing then keyword in (if ... then ...) at line 56, column 14
```

## Affected Files (14)
- `clips-official/clipsjni/examples/WineDemo/winedemo.clp`
- `telefonica-clips/branches/63x/clipsdotnet/WineFormsExample/wine.clp`
- `telefonica-clips/branches/63x/clipsdotnet/WineWPFExample/wine.clp`
- `telefonica-clips/branches/63x/clipsios/Wine/Wine/wine.clp`
- `telefonica-clips/branches/63x/clipsjni/java-src/net/sf/clipsrules/jni/examples/wine/resources/wine.clp`
- `telefonica-clips/branches/64x/clipsjni/java-src/net/sf/clipsrules/jni/examples/wine/resources/wine.clp`
- `telefonica-clips/branches/64x/windows/MVS_2015/WineFormsExample/wine.clp`
- `telefonica-clips/branches/64x/windows/MVS_2015/WineWPFExample/wine.clp`
- `telefonica-clips/branches/64x/windows/MVS_2017/WineFormsExample/wine.clp`
- `telefonica-clips/branches/64x/windows/MVS_2017/WineWPFExample/wine.clp`
- `telefonica-clips/branches/65x/clipsjni/java-src/net/sf/clipsrules/jni/examples/wine/resources/wine.clp`
- `telefonica-clips/branches/65x/clipsnet/MVS_2017/WineFormsExample/wine.clp`
- `telefonica-clips/branches/65x/clipsnet/MVS_2017/WineWPFExample/wine.clp`
- `telefonica-clips/clipscgi/wine/winecgi.clp`

## Apparent Ferric-Side Root Cause
The Stage 2 parser in `crates/ferric-parser/src/stage2.rs` recognizes `if` as a control-flow keyword unconditionally. When it appears as the first element inside parentheses (in a slot-constraint context within a deftemplate fact pattern or a modify/assert action), the parser enters `if/then/else` mode and expects a `then` keyword, which fails.

CLIPS disambiguates by context: when parsing a fact pattern or template slot access, `if` is treated as a slot name; only in the RHS action context is `if` a control-flow keyword.

## Implementation Plan
1. Context-sensitive parsing of `if` in slot positions.
   - When parsing template slot constraints (inside `(template-name (slot-name value ...))` patterns), treat `if` as a regular symbol, not a control-flow keyword.
   - The key is knowing when the parser is inside a fact pattern vs. a function-call or action context.
   - Caveat: `if` as a slot name in the RHS `(modify ?f (if value))` also needs this treatment — the modify/duplicate action parser must recognize `if` as a slot name when the template has an `if` slot.

2. Approach: check the known template's slot names before interpreting `if` as a keyword.
   - When parsing a modify/duplicate/assert action, look up the bound template's slot list. If `if` is a registered slot name, treat the token as a slot name rather than a keyword.
   - In the LHS pattern context, the same approach applies: if parsing inside a template pattern and `if` matches a slot name, use slot-name interpretation.
   - Caveat: requires the template to be defined before the rule that references it, which is the normal CLIPS load order.

3. Fallback: make `if` a soft keyword.
   - Only interpret `if` as a control-flow keyword when it appears as the first element of a parenthesized expression in a known action context (RHS function call, default value expression, etc.).
   - In all other contexts (pattern matching, slot names, deffacts), treat `if` as a regular symbol.
   - Caveat: may require refactoring the keyword detection to be position-aware.

## Test And Verification
1. Parser unit tests:
```bash
cargo test -p ferric-parser if_slot_name
```

2. Integration tests:
```bash
cargo test -p ferric-runtime if_slot_name
```

3. Compatibility smoke checks:
```bash
cargo run -p ferric -- check tests/examples/clips-official/clipsjni/examples/WineDemo/winedemo.clp
cargo run -p ferric -- run tests/examples/clips-official/clipsjni/examples/WineDemo/winedemo.clp
```
Expected: `winedemo.clp` loads and runs without errors. The rule fires when template facts with `if`/`then` slots are asserted.
