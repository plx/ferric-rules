# Phase 6 Compatibility Test Notes

## Compat test harness location
- Fixture files: `tests/clips_compat/fixtures/<domain>/<name>.clp`
- Test functions: `crates/ferric/tests/clips_compat.rs`
- Harness API: `run_clips_compat`, `run_clips_compat_full`, `run_clips_compat_file`

## Key constraints discovered writing compat fixtures

### `(test)` is a reserved pattern keyword
`test` is a CLIPS test conditional element keyword. Using `(test)` as a fact name in a
pattern fails with "missing expression after 'test'". Use names like `(run-it)`, `(compute)`,
`(run-compare)`, etc.

### `=` cannot be a function call in expressions
The lexer emits `Token::Equals` for bare `=` (not followed by symbol char). So `(= 42 42)`
in a function call position fails with "expected function name (symbol)". Use `eq` for
symbolic equality, `<>` for numeric inequality.

### `bind` only handles globals (`?*name*`)
`dispatch_bind` in evaluator.rs only handles `RuntimeExpr::GlobalVar`. Using
`(bind ?local-var ...)` in a rule RHS fails silently (TypeError → ActionError collected,
but execution continues). Subsequent references to `?local-var` are unbound → empty output.
Always use inline expressions for multifield/complex operations, or use globals.

### `format` is evaluator-only (no router write)
`builtin_format` returns a String value but does NOT write to the output router.
Use `(printout t (format nil "..." ...) crlf)` — pass `nil` as channel to format,
then pass the result string to `printout`.

### `?*MODULE::name*` qualified global syntax
Works for both read (`?*CONFIG::threshold*`) and write (`(bind ?*CONFIG::name* value)`).
Requires the global to be exported by the owning module and imported by the calling module.

### Visibility diagnostics
Action-level visibility errors go to `engine.action_diagnostics()` (not printed to output).
Load-level unknown constructs produce `LoadError::UnsupportedForm { name, line, column }`.

### float formatting
`(float 42)` → prints as `42.0`; `(/ 100 4)` → prints as `25.0` (division always returns float).
`(integer 3)` → prints as `3` (no decimal).

## Connective constraint parsing (stage2.rs)

- Added `interpret_constraint_sequence()` with OR > AND > NOT precedence
- Added `parse_or_expr`, `parse_and_expr`, `parse_unary_expr` helpers
- Added `Constraint::span()` method
- `Colon` and `Equals` connectives produce `Constraint::Wildcard` placeholder (item 005 not yet done)
- **CRITICAL**: CLI fixture `tests/fixtures/cli/check_valid.clp` uses `?a&:(> ?a 18)` — was silently ignoring predicate. Now parsed with wildcard placeholder.
- `interpret_pattern_slot_constraint` now uses `interpret_constraint_sequence(&slot_list[1..])` instead of just `slot_list[1]`
- 4 new tests: `interpret_ordered_connective_and`, `interpret_ordered_connective_or`, `interpret_template_connective_constraint`, `interpret_negation_constraint`

## Test count trajectory (phase 6)
- Baseline: 1216
- After pass 001 (harness scaffold): 1220
- After pass 002 (module/generic/stdlib fixtures): 1243
- After pass 003 (core/negation deep): 1254
- After pass 004 (lang/module/stdlib deep): 1265
- After pass 005 (this pass — deeper lang/module/stdlib): ~1276 (58 compat tests total)
