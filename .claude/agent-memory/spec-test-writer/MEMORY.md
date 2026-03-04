# ferric-rules spec-test-writer memory

## Project Overview
`ferric-rules` is a Rust CLIPS rules engine. Tests live in `crates/ferric-ffi/src/tests/`.
All tests are inside the crate as `#[cfg(test)] mod` modules registered in `tests.rs`.

## Test Patterns (ferric-ffi crate)

### Standard test structure
- All FFI tests use `unsafe { }` blocks
- Create engine: `ferric_engine_new()`, free: `ferric_engine_free(engine)`
- Always call `ferric_engine_reset(engine)` before asserting facts or running
- Use `CString::new("...").unwrap()` for C string construction
- Check errors with `assert_eq!(result, FerricError::Ok)`
- Import from `crate::engine::*`, `crate::error::FerricError`, `crate::types::*`

### Buffer-copy pattern
Functions that return strings use: `buf: *mut c_char`, `buf_len: usize`, `out_len: *mut usize`
- Size query: `null buf + buf_len=0 → Ok, *out_len = needed`
- Undersized: returns `FerricError::BufferTooSmall`, `*out_len` still = needed
- Allocate with: `let mut buf = vec![0i8; N]; buf.as_mut_ptr()`

## CRITICAL: Template Facts vs Ordered Facts

`ferric_engine_assert_string` always creates **ordered facts** — it calls the
simplified `process_assert_fact` path which does `assert_ordered` unconditionally,
regardless of whether a deftemplate exists for that name.

To create true **template facts** in tests, use `deffacts` + `reset()`:
```rust
let source = CString::new(
    "(deftemplate person (slot name))\
     (deffacts init (person (name Alice)))",
).unwrap();
ferric_engine_load_string(engine, source.as_ptr());
ferric_engine_reset(engine); // triggers deffacts → template fact is now in WM
```

Then enumerate fact IDs with `ferric_engine_fact_ids` to obtain the fact ID.

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
- `AgendaEmpty = 0`, `LimitReached = 1`, `HaltRequested = 2`

## Module Registration
New test modules must be added to `crates/ferric-ffi/src/tests.rs`:
```rust
#[cfg(test)]
mod ffi_expansion;
```

## Index of test files
- `tests/execution.rs` — run/step/assert_string/retract/get_output patterns
- `tests/values.rs` — FerricValue conversion, get_fact_field, get_global, fact_count
- `tests/ffi_expansion.rs` — all new FFI expansion functions (63 tests)
