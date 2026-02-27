# 006 Compile-Time Function Validation

## Sequence Position
6/10 (medium; independent of parser features, but best done before broad control-flow rollout to tighten diagnostics).

## Behavioral Divergence
CLIPS rejects unknown function calls at load time with diagnostics like:
`[EXPRNPSR3] Missing function declaration for <name>.`

Ferric previously accepted many of these at load time and only emitted runtime action diagnostics, which diverged from CLIPS load behavior.

## Apparent Ferric-Side Root Cause
In `crates/ferric-runtime/src/loader.rs`, `compile_rule_construct()` pre-translates actions via:
`from_action_expr(...).ok()`.

This drops translation errors and allows unresolved call sites to pass load. There is also no post-load pass validating that function names are resolvable against built-ins, `deffunction`, and `defgeneric` declarations in scope.

## Implementation Summary
1. Rule RHS callable validation now runs at compile/load time in `loader.rs`.
- Unknown unqualified call names now fail load with:
  - `[EXPRNPSR3] Missing function declaration for <name> ...`
- Validation walks nested rule action/control-flow forms and handles structured
  action arguments (`assert` fact patterns, `modify`/`duplicate` slot pairs).

2. Rule action pre-translation no longer swallows errors.
- `compile_single_rule()` now maps `from_action_expr` failures into explicit
  `LoadError::Compile` diagnostics instead of silently storing `None`.

3. Builtin callable list was centralized for validation reuse.
- Added `crate::evaluator::is_builtin_callable()` and used it from loader validation.

## Scope Notes
- This pass is intentionally focused on unknown **unqualified** callable names in
  rule RHS/action expressions.
- Module-qualified callable resolution keeps existing runtime behavior/diagnostics
  (visibility/unknown-module checks are unchanged).

## Test And Verification
1. Runtime/loader tests:
```bash
cargo test -p ferric-runtime unknown_function_in_rhs_fails_at_load_time
cargo test -p ferric-runtime phase4_integration_tests
```
(load-time unknown-function validation remains green with phase 4 coverage)

2. CLI smoke checks:
```bash
cargo run -p ferric-cli -- check tests/examples/clips-official/examples/sudoku/puzzles/grid2x2-p1.clp
cargo run -p ferric-cli -- check tests/examples/clips-executive/cx_plugins/protobuf_plugin/clips/protobuf.clp
```
Expected near-term outcome: ferric should fail at load time where CLIPS reports missing function declarations; files may still need multi-file loading context.
