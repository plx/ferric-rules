# 001 Field Alias For Slot Definitions

## Sequence Position
1/10 (very small parser-only compatibility gap; no upstream dependencies).

## Behavioral Divergence
Older CLIPS sources use `(field ...)` in `deftemplate` definitions where modern CLIPS also accepts `(slot ...)`.

Ferric currently rejects this syntax with an interpret-stage error (`expected 'slot' or 'multislot'`), so files that are otherwise valid fail before rule compilation starts.

## Apparent Ferric-Side Root Cause
`crates/ferric-parser/src/stage2.rs` only maps `slot` and `multislot` in `interpret_slot_definition()`. The parser test suite also explicitly encodes `(field ...)` as invalid (`interpret_error_invalid_slot_keyword`).

## Implementation Plan
1. Extend slot keyword mapping in `interpret_slot_definition()`.
- Add `"field" => (SlotType::Single, 1)`.
- Optionally add `"multifield" => (SlotType::Multi, 1)` for older CLIPS variants.
- Caveat: this step only resolves the slot-keyword parse failure; files can still fail later for unrelated unsupported constructs.

2. Update parser diagnostics to reflect accepted aliases.
- Adjust any hard-coded diagnostic text that currently says only `slot`/`multislot`.
- Keep the message explicit so real typos still produce actionable errors.
- Caveat: improving diagnostics does not guarantee behavioral parity for all legacy templates.

3. Convert and expand parser tests.
- Replace the current negative test around `(field ...)` with positive coverage.
- Add a regression test that `(deftemplate avh (field a) (field v) (field h))` produces three single slots.
- If `multifield` is accepted, add a dedicated positive test for it.
- Caveat: passing parser tests still does not prove runtime parity for every template option combination.

4. Add one loader-level smoke test.
- Ensure a minimal rule loading a `field`-based template reaches compile/run.
- Caveat: this smoke test validates loading path only; large legacy programs may still fail for separate reasons.

## Test And Verification
1. Parser unit tests:
```bash
cargo test -p ferric-parser interpret_error_invalid_slot_keyword
cargo test -p ferric-parser interpret_deftemplate_multiple_slots
```

2. Compatibility smoke checks:
```bash
cargo run -p ferric-cli -- check tests/examples/clips-official/examples/zebra.clp
cargo run -p ferric-cli -- check tests/examples/clips-official/test_suite/fctpcstr.clp
```
Expected near-term outcome: the specific `field` keyword error disappears; other incompatibilities (for example connective constraints) may still fail these files.
