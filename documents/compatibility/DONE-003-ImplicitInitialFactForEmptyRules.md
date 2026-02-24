# 003 Implicit Initial Fact For Empty Rules

## Sequence Position
3/10 (small-to-medium loader/runtime alignment; independent of later parser features).

## Behavioral Divergence
CLIPS allows rules with no LHS patterns (and rules containing only `test` CEs). These rules implicitly match `(initial-fact)` after `(reset)`.

Ferric currently fails such rules with `compile error: rule has no patterns`.

## Apparent Ferric-Side Root Cause
`ReteCompiler::compile_conditions()` in `crates/ferric-core/src/compiler.rs` rejects empty condition lists via `ensure_non_empty()`.

The loader path in `crates/ferric-runtime/src/loader.rs` (`translate_rule_construct()`) already synthesizes `(initial-fact)` for NCC-leading rules, but does not do so when the translated condition list is empty.

`Engine::reset()` in `crates/ferric-runtime/src/engine.rs` reasserts `(initial-fact)` only when previously enabled, but currently reasserts deffacts before `(initial-fact)`, which can diverge from CLIPS activation ordering.

## Implementation Plan
1. Inject synthetic `(initial-fact)` when translated condition list is empty.
- In `translate_rule_construct()`, after collecting `conditions` and `test_conditions`, insert a `CompilableCondition::Pattern` for ordered relation `initial-fact` when `conditions.is_empty()`.
- Reuse the same symbol interning path used by existing NCC bootstrap logic.
- Caveat: this only addresses the known empty-LHS gate; additional unsupported constructs in the same file can still prevent successful runs.

2. Keep compiler invariants intact.
- Do not remove `ensure_non_empty()` in core compiler; preserve the invariant that runtime/loader prepares valid condition lists.
- Add loader-level tests proving empty/test-only rules now compile through `compile_conditions()`.
- Caveat: compile-path success still does not ensure runtime parity for all empty-pattern edge cases.

3. Align reset assertion ordering with CLIPS.
- In `Engine::reset()`, reassert `(initial-fact)` before reasserting registered deffacts.
- Preserve `initial_fact_id` tracking so `facts()` still hides the synthetic fact.
- Caveat: ordering alignment reduces divergence but may reveal additional agenda-order differences not covered by this fix.

4. Add focused execution regression tests.
- Empty-LHS rule fires once after reset.
- Test-only rule `(test (> 5 3))` fires once after reset.
- Rule does not repeatedly fire without new facts (normal agenda behavior).
- Caveat: these targeted tests do not prove broad CLIPS suite parity.

## Test And Verification
1. Core/runtime tests:
```bash
cargo test -p ferric-runtime load_rule_with_test_pattern_compiles_successfully
cargo test -p ferric-core test_compile_empty_rule_error
```
(Add new loader/engine tests for empty-pattern and test-only rules.)

2. Example smoke checks:
```bash
cargo run -p ferric-cli -- run tests/examples/clips-executive/cx_tutorial_agents/clips/hello_world.clp
cargo run -p ferric-cli -- check tests/examples/clips-official/test_suite/pataddtn.clp
```
Expected near-term outcome: empty-rule compile failure disappears; files may still fail for separate unsupported features.
