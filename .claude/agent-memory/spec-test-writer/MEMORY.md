# ferric-rules spec-test-writer memory

## Project Overview
`ferric-rules` is a Rust CLIPS rules engine. The FFI contract tests live in
`crates/ferric-rules-ffi/src/tests/`, registered as modules in `src/tests.rs`.
Other crates also have unit, integration, and doc tests; use their existing layout.

## Test Patterns (ferric-rules-ffi crate)

### Standard test structure
- Pointer-taking engine/value APIs require `unsafe` calls with valid lifetimes; integer, float, and void value constructors are safe functions.
- Create engine: `ferric_engine_new()`, free: `ferric_engine_free(engine)`
- Call `ferric_engine_reset(engine)` when the test needs deffacts/reset initialization, before asserting facts that must remain live. Reset is not required for every assertion/run and discards existing facts.
- Use `CString::new("...").unwrap()` for C string construction
- Check errors with `assert_eq!(result, FerricError::Ok)`
- Import from `crate::engine::*`, `crate::error::FerricError`, `crate::types::*`

### Buffer-copy pattern
Functions that return strings use: `buf: *mut c_char`, `buf_len: usize`, `out_len: *mut usize`
- Size query: `null buf + buf_len=0 → Ok, *out_len = needed` (including the terminating NUL)
- Undersized: returns `FerricError::BufferTooSmall`, `*out_len` still = needed
- Allocate with `let mut buf = vec![0u8; needed];` and pass `buf.as_mut_ptr().cast()` for the platform's `c_char` pointer.

## Template and Ordered Facts

`ferric_engine_assert_string` accepts a CLIPS form such as
`(assert (person (name Alice)))` and uses the runtime loader. A visible
`deftemplate person` makes this a template fact; without a matching template,
the relation uses ordered-fact syntax. Field expressions are evaluated for both
forms. The output ID is the first asserted fact's opaque host ID.

See `tests/execution.rs::assert_string_evaluates_ordered_and_template_expressions`
for template/ordered matching, arithmetic, globals, and multifield splicing.
`deffacts` plus reset is another supported initialization path, not a workaround
for an ordered-only assertion API. `tests/template_assertion.rs` covers direct
`ferric_engine_assert_template` and slot-by-name access.

## Key API Surface (new FFI expansion functions)

### Fact iteration
- `ferric_engine_fact_ids(engine, out_ids, max_ids, out_count)` — enumerate all fact IDs
- `ferric_engine_find_fact_ids(engine, relation, out_ids, max_ids, out_count)` — by relation

### Fact type/names
- `ferric_engine_get_fact_type(engine, fact_id, out_type)` → `FerricFactType::{Ordered, Template}`
- `ferric_engine_get_fact_relation(engine, fact_id, buf, buf_len, out_len)` — ordered only
- `ferric_engine_get_fact_template_name(engine, fact_id, buf, buf_len, out_len)` — template only
- Both return `InvalidArgument` when called on the wrong fact type

### Template/rule/module introspection
- `ferric_engine_template_count`, `ferric_engine_template_name(engine, index, ...)`
- `ferric_engine_template_slot_count(engine, template_name, out_count)`
- `ferric_engine_template_slot_name(engine, template_name, slot_index, ...)`
- `ferric_engine_rule_count`, `ferric_engine_rule_info(engine, index, buf, buf_len, out_len, out_salience)`
- `ferric_engine_module_count`, `ferric_engine_module_name(engine, index, ...)`
- `ferric_engine_current_module`, `ferric_engine_get_focus`, `ferric_engine_focus_stack_depth/entry`

### Agenda/halt/clear
- `ferric_engine_agenda_count(engine, out_count)`
- `ferric_engine_is_halted(engine, out_halted)` — writes 1 or 0
- `ferric_engine_halt(engine)` — idempotent
- `ferric_engine_push_input(engine, line)` — null line → NullPointer
- `ferric_engine_clear(engine)` — removes all templates, rules, etc.

### Value constructors (types.rs)
- `ferric_value_integer(i64)`, `ferric_value_float(f64)` — no allocation
- `ferric_value_symbol(ptr)`, `ferric_value_string(ptr)` — heap-allocates copy; null → Void
- `ferric_value_void()` — all-zeroed

### Convenience variants
- `ferric_engine_new_with_source(source)` — creates + loads + resets; null → null, bad source → null
- `ferric_engine_new_with_source_config(source, config)` — null config means default config
- `ferric_engine_clear_output(engine, channel)` — clears captured output; null channel → NullPointer
- `ferric_engine_run_ex(engine, limit, out_fired, out_reason)` — writes `FerricHaltReason`

### FerricHaltReason
- `AgendaEmpty = 0`, `LimitReached = 1`, `HaltRequested = 2`, `ActionError = 3`

## Module Registration
New test modules must be added to `crates/ferric-rules-ffi/src/tests.rs`:
```rust
#[cfg(test)]
mod ffi_expansion;
```

## Index of test files
- `tests/execution.rs` — run/step/assert_string/retract/get_output patterns
- `tests/values.rs` — FerricValue conversion, get_fact_field, get_global, fact_count
- `tests/ffi_expansion.rs` — FFI expansion function contracts
