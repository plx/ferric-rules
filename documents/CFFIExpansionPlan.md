# C FFI Expansion Plan

## Purpose

Extend the Ferric C FFI surface with 32 new functions covering fact iteration, structured assertion, fact type discrimination, template/rule/module introspection, agenda/halt queries, and improved execution variants. All additions are purely additive — no existing functions are modified.

## Design Principles

All new functions follow the established patterns from the existing FFI:

1. **Error codes.** Every fallible function returns `FerricError`. Output values are written through pointer parameters.
2. **Null safety.** Null engine pointers → `FERRIC_ERROR_NULL_POINTER` with global error message. Null output pointers are checked and rejected.
3. **Thread affinity.** Every `ferric_engine_*` function validates thread affinity before any mutable borrow.
4. **Dual error channels.** Errors are written to both the per-engine error state and the global thread-local error state.
5. **Buffer copy pattern.** String-returning functions use the established `(buf, buf_len, out_len)` triple. Size query: pass `NULL` buf with `buf_len=0`. Returns `FERRIC_ERROR_BUFFER_TOO_SMALL` when buffer is insufficient, writing the required size to `*out_len`.
6. **Array output pattern.** Array-returning functions use `(out_array, max_count, out_count)`. Size query: pass `NULL` array with `max_count=0`. `*out_count` always receives the total count.

---

## New Types

### `FerricFactType`

```c
typedef enum {
    FERRIC_FACT_TYPE_ORDERED  = 0,
    FERRIC_FACT_TYPE_TEMPLATE = 1,
} FerricFactType;
```

Added to `crates/ferric-ffi/src/types.rs` as:

```rust
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FerricFactType {
    Ordered = 0,
    Template = 1,
}
```

### `FerricHaltReason`

```c
typedef enum {
    FERRIC_HALT_REASON_AGENDA_EMPTY    = 0,
    FERRIC_HALT_REASON_LIMIT_REACHED   = 1,
    FERRIC_HALT_REASON_HALT_REQUESTED  = 2,
} FerricHaltReason;
```

Added to `crates/ferric-ffi/src/types.rs` as:

```rust
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FerricHaltReason {
    AgendaEmpty = 0,
    LimitReached = 1,
    HaltRequested = 2,
}

impl From<ferric_runtime::HaltReason> for FerricHaltReason {
    fn from(reason: ferric_runtime::HaltReason) -> Self {
        match reason {
            ferric_runtime::HaltReason::AgendaEmpty => Self::AgendaEmpty,
            ferric_runtime::HaltReason::LimitReached => Self::LimitReached,
            ferric_runtime::HaltReason::HaltRequested => Self::HaltRequested,
        }
    }
}
```

### Reverse Value Conversion: `ferric_to_value`

New function in `crates/ferric-ffi/src/types.rs` for converting C-facing `FerricValue` back to Rust `Value`:

```rust
/// Convert a C-facing `FerricValue` to a Rust `Value`.
///
/// For Symbol and String types, the `string_ptr` is read as a NUL-terminated
/// C string. For Symbol, the string is interned via the engine's symbol table.
///
/// # Safety
///
/// - `fv` must be a valid `FerricValue` with active fields matching `value_type`.
/// - `string_ptr` (for Symbol/String) must be a valid NUL-terminated string.
/// - `multifield_ptr` (for Multifield) must point to `multifield_len` valid `FerricValue`s.
pub(crate) unsafe fn ferric_to_value(
    fv: &FerricValue,
    engine: &mut Engine,
) -> Result<Value, String> {
    match fv.value_type {
        FerricValueType::Void => Ok(Value::Void),
        FerricValueType::Integer => Ok(Value::Integer(fv.integer)),
        FerricValueType::Float => Ok(Value::Float(fv.float)),
        FerricValueType::Symbol => {
            if fv.string_ptr.is_null() {
                return Err("symbol string_ptr is null".to_string());
            }
            let name = CStr::from_ptr(fv.string_ptr).to_str()
                .map_err(|e| format!("symbol is not valid UTF-8: {e}"))?;
            let sym = engine.intern_symbol(name)
                .map_err(|e| e.to_string())?;
            Ok(Value::Symbol(sym))
        }
        FerricValueType::String => {
            if fv.string_ptr.is_null() {
                return Err("string string_ptr is null".to_string());
            }
            let s = CStr::from_ptr(fv.string_ptr).to_str()
                .map_err(|e| format!("string is not valid UTF-8: {e}"))?;
            let fs = engine.create_string(s)
                .map_err(|e| e.to_string())?;
            Ok(Value::String(fs))
        }
        FerricValueType::Multifield => {
            if fv.multifield_len == 0 {
                return Ok(Value::Multifield(vec![]));
            }
            if fv.multifield_ptr.is_null() {
                return Err("multifield_ptr is null with non-zero length".to_string());
            }
            let mut values = Vec::with_capacity(fv.multifield_len);
            for i in 0..fv.multifield_len {
                let elem = &*fv.multifield_ptr.add(i);
                values.push(ferric_to_value(elem, engine)?);
            }
            Ok(Value::Multifield(values))
        }
        FerricValueType::ExternalAddress => {
            Err("ExternalAddress cannot be converted from FFI".to_string())
        }
    }
}
```

---

## New Functions (32 total)

### 1. Fact Iteration (2 functions)

#### `ferric_engine_fact_ids`

Copy all user-visible fact IDs to a caller-provided array.

```c
FerricError ferric_engine_fact_ids(
    const FerricEngine *engine,
    uint64_t           *out_ids,     // NULL for size query
    size_t              max_ids,     // capacity of out_ids array
    size_t             *out_count    // receives total fact count
);
```

**Wraps:** `Engine::facts()` → collect `FactId`s, convert to `u64` via `KeyData::as_ffi()`.

**Behavior:**
- `out_ids == NULL && max_ids == 0`: Size query. Writes total count to `*out_count`, returns `Ok`.
- `out_ids != NULL`: Copies up to `max_ids` IDs. `*out_count` always receives total count. Returns `Ok` (partial copy is allowed).
- `out_count == NULL`: Returns `NullPointer`.

#### `ferric_engine_find_fact_ids`

Find fact IDs by relation name.

```c
FerricError ferric_engine_find_fact_ids(
    const FerricEngine *engine,
    const char         *relation,    // NUL-terminated relation name
    uint64_t           *out_ids,     // NULL for size query
    size_t              max_ids,
    size_t             *out_count
);
```

**Wraps:** `Engine::find_facts(relation)` → collect `FactId`s.

**Behavior:** Same size-query pattern as `ferric_engine_fact_ids`. Returns `NullPointer` if `relation` is null.

---

### 2. Fact Type & Names (3 functions)

#### `ferric_engine_get_fact_type`

Discriminate ordered vs. template fact.

```c
FerricError ferric_engine_get_fact_type(
    const FerricEngine *engine,
    uint64_t            fact_id,
    FerricFactType     *out_type
);
```

**Wraps:** `Engine::get_fact(fid)` → discriminate `Fact::Ordered` vs `Fact::Template`.

**Behavior:** Returns `NotFound` if fact does not exist. Returns `NullPointer` if `out_type` is null.

#### `ferric_engine_get_fact_relation`

Get the relation name for an ordered fact.

```c
FerricError ferric_engine_get_fact_relation(
    const FerricEngine *engine,
    uint64_t            fact_id,
    char               *buf,
    size_t              buf_len,
    size_t             *out_len
);
```

**Wraps:** `Engine::get_fact(fid)` → `OrderedFact::relation` → resolve symbol to string.

**Behavior:** Standard buffer copy pattern. Returns `NotFound` if fact does not exist. Returns `InvalidArgument` if the fact is a template fact (not ordered).

#### `ferric_engine_get_fact_template_name`

Get the template name for a template fact.

```c
FerricError ferric_engine_get_fact_template_name(
    const FerricEngine *engine,
    uint64_t            fact_id,
    char               *buf,
    size_t              buf_len,
    size_t             *out_len
);
```

**Wraps:** `Engine::get_fact(fid)` → `TemplateFact::template_id` → `Engine::template_name_by_id()`.

**Behavior:** Standard buffer copy pattern. Returns `NotFound` if fact does not exist. Returns `InvalidArgument` if the fact is an ordered fact (not template).

---

### 3. Structured Assertion (1 function)

#### `ferric_engine_assert_ordered`

Assert an ordered fact from structured values, bypassing CLIPS source parsing.

```c
FerricError ferric_engine_assert_ordered(
    FerricEngine       *engine,
    const char         *relation,       // NUL-terminated relation name
    const FerricValue  *fields,         // array of field values
    size_t              field_count,     // number of fields
    uint64_t           *out_fact_id     // receives the new fact ID
);
```

**Wraps:** `Engine::assert_ordered(relation, values)` after converting each `FerricValue` via `ferric_to_value()`.

**Behavior:**
- `relation == NULL`: Returns `NullPointer`.
- `fields == NULL && field_count > 0`: Returns `NullPointer`.
- `fields == NULL && field_count == 0`: Asserts a zero-field fact (e.g., `(initial-fact)`-style).
- `out_fact_id == NULL`: Fact is asserted but ID is not returned.
- Conversion failure in any field value: Returns `InvalidArgument` with description.

---

### 4. Value Construction Helpers (5 functions)

Pure helpers that create `FerricValue` structs. No engine pointer needed.

#### `ferric_value_integer`

```c
FerricValue ferric_value_integer(int64_t value);
```

Returns a `FerricValue` with `value_type = Integer`, `integer = value`.

#### `ferric_value_float`

```c
FerricValue ferric_value_float(double value);
```

Returns a `FerricValue` with `value_type = Float`, `float = value`.

#### `ferric_value_symbol`

```c
FerricValue ferric_value_symbol(const char *name);
```

Returns a `FerricValue` with `value_type = Symbol`, `string_ptr` = heap-copy of `name`. The caller owns the `string_ptr` and must free with `ferric_value_free`. Returns a void value if `name` is null.

#### `ferric_value_string`

```c
FerricValue ferric_value_string(const char *s);
```

Returns a `FerricValue` with `value_type = String`, `string_ptr` = heap-copy of `s`. Same ownership rules as `ferric_value_symbol`. Returns a void value if `s` is null.

#### `ferric_value_void`

```c
FerricValue ferric_value_void(void);
```

Returns a `FerricValue` with `value_type = Void` and all fields zeroed/null.

---

### 5. Template Introspection (4 functions)

#### `ferric_engine_template_count`

```c
FerricError ferric_engine_template_count(
    const FerricEngine *engine,
    size_t             *out_count
);
```

**Wraps:** `Engine::templates().len()`.

#### `ferric_engine_template_name`

```c
FerricError ferric_engine_template_name(
    const FerricEngine *engine,
    size_t              index,
    char               *buf,
    size_t              buf_len,
    size_t             *out_len
);
```

**Wraps:** `Engine::templates()[index]` — name of the template at zero-based index.

**Behavior:** Standard buffer copy pattern. Returns `InvalidArgument` if `index >= template_count`.

#### `ferric_engine_template_slot_count`

```c
FerricError ferric_engine_template_slot_count(
    const FerricEngine *engine,
    const char         *template_name,
    size_t             *out_count
);
```

**Wraps:** NEW `Engine::template_slot_names(name)` → `.len()`.

**Behavior:** Returns `NotFound` if template name is not registered. Returns `NullPointer` if `template_name` or `out_count` is null.

#### `ferric_engine_template_slot_name`

```c
FerricError ferric_engine_template_slot_name(
    const FerricEngine *engine,
    const char         *template_name,
    size_t              slot_index,
    char               *buf,
    size_t              buf_len,
    size_t             *out_len
);
```

**Wraps:** NEW `Engine::template_slot_names(name)[slot_index]`.

**Behavior:** Standard buffer copy pattern. Returns `NotFound` if template not found. Returns `InvalidArgument` if `slot_index >= slot_count`.

---

### 6. Rule Introspection (2 functions)

#### `ferric_engine_rule_count`

```c
FerricError ferric_engine_rule_count(
    const FerricEngine *engine,
    size_t             *out_count
);
```

**Wraps:** `Engine::rules().len()`.

#### `ferric_engine_rule_info`

```c
FerricError ferric_engine_rule_info(
    const FerricEngine *engine,
    size_t              index,
    char               *buf,         // receives the rule name
    size_t              buf_len,
    size_t             *out_len,
    int32_t            *out_salience // receives salience value
);
```

**Wraps:** `Engine::rules()[index]` — returns `(&str, i32)` tuple.

**Behavior:** Standard buffer copy pattern for the name. `*out_salience` written if non-null. Returns `InvalidArgument` if `index >= rule_count`.

---

### 7. Module Operations (6 functions)

#### `ferric_engine_current_module`

```c
FerricError ferric_engine_current_module(
    const FerricEngine *engine,
    char               *buf,
    size_t              buf_len,
    size_t             *out_len
);
```

**Wraps:** `Engine::current_module()`.

**Behavior:** Standard buffer copy pattern. Always succeeds (MAIN module always exists).

#### `ferric_engine_get_focus`

```c
FerricError ferric_engine_get_focus(
    const FerricEngine *engine,
    char               *buf,
    size_t              buf_len,
    size_t             *out_len
);
```

**Wraps:** `Engine::get_focus()`.

**Behavior:** Standard buffer copy pattern. Returns `NotFound` if focus stack is empty (writes `*out_len = 0`).

#### `ferric_engine_focus_stack_depth`

```c
FerricError ferric_engine_focus_stack_depth(
    const FerricEngine *engine,
    size_t             *out_depth
);
```

**Wraps:** `Engine::get_focus_stack().len()`.

#### `ferric_engine_focus_stack_entry`

```c
FerricError ferric_engine_focus_stack_entry(
    const FerricEngine *engine,
    size_t              index,
    char               *buf,
    size_t              buf_len,
    size_t             *out_len
);
```

**Wraps:** `Engine::get_focus_stack()[index]`.

**Behavior:** Standard buffer copy pattern. Returns `InvalidArgument` if `index >= stack_depth`. Index 0 = bottom of stack, last index = top (current focus).

#### `ferric_engine_module_count`

```c
FerricError ferric_engine_module_count(
    const FerricEngine *engine,
    size_t             *out_count
);
```

**Wraps:** NEW `Engine::modules().len()`.

#### `ferric_engine_module_name`

```c
FerricError ferric_engine_module_name(
    const FerricEngine *engine,
    size_t              index,
    char               *buf,
    size_t              buf_len,
    size_t             *out_len
);
```

**Wraps:** NEW `Engine::modules()[index]`.

**Behavior:** Standard buffer copy pattern. Returns `InvalidArgument` if `index >= module_count`.

---

### 8. Agenda, Halt, Input, Clear (5 functions)

#### `ferric_engine_agenda_count`

```c
FerricError ferric_engine_agenda_count(
    const FerricEngine *engine,
    size_t             *out_count
);
```

**Wraps:** `Engine::agenda_len()`.

#### `ferric_engine_is_halted`

```c
FerricError ferric_engine_is_halted(
    const FerricEngine *engine,
    int32_t            *out_halted   // 1 = halted, 0 = not halted
);
```

**Wraps:** `Engine::is_halted()`.

#### `ferric_engine_halt`

```c
FerricError ferric_engine_halt(
    FerricEngine *engine
);
```

**Wraps:** `Engine::halt()`.

**Behavior:** Always succeeds. Idempotent — calling halt on an already-halted engine is a no-op.

#### `ferric_engine_push_input`

```c
FerricError ferric_engine_push_input(
    FerricEngine *engine,
    const char   *line
);
```

**Wraps:** `Engine::push_input(line)`.

**Behavior:** Returns `NullPointer` if `line` is null.

#### `ferric_engine_clear`

```c
FerricError ferric_engine_clear(
    FerricEngine *engine
);
```

**Wraps:** `Engine::clear()`.

**Behavior:** Resets the engine to a blank slate (removes all facts, rules, templates, globals, functions, generics, and modules except MAIN). Always succeeds.

---

### 9. Convenience & Improved Variants (4 functions)

#### `ferric_engine_new_with_source`

Create an engine from CLIPS source with default configuration.

```c
FerricEngine *ferric_engine_new_with_source(
    const char *source
);
```

**Wraps:** `Engine::with_rules(source)`.

**Behavior:** Returns `NULL` on parse/compile error (sets global error message). Returns valid engine handle on success.

#### `ferric_engine_new_with_source_config`

Create an engine from CLIPS source with explicit configuration.

```c
FerricEngine *ferric_engine_new_with_source_config(
    const char         *source,
    const FerricConfig *config   // NULL → defaults
);
```

**Wraps:** `Engine::with_rules_config(source, config)`.

**Behavior:** Same null/error handling as `ferric_engine_new_with_source`. `config == NULL` → default configuration.

#### `ferric_engine_clear_output`

Clear a specific output channel.

```c
FerricError ferric_engine_clear_output(
    FerricEngine *engine,
    const char   *channel
);
```

**Wraps:** `Engine::clear_output_channel(channel)`.

**Behavior:** Returns `NullPointer` if `channel` is null. Always succeeds otherwise (clearing a non-existent channel is a no-op).

#### `ferric_engine_run_ex`

Extended run with halt reason output.

```c
FerricError ferric_engine_run_ex(
    FerricEngine     *engine,
    int64_t           limit,
    uint64_t         *out_fired,    // may be NULL
    FerricHaltReason *out_reason    // may be NULL
);
```

**Wraps:** `Engine::run(limit)` → `RunResult { rules_fired, halt_reason }`.

**Behavior:** Same limit semantics as existing `ferric_engine_run` (negative = unlimited). Additionally writes `halt_reason` to `*out_reason` if non-null.

---

## Shared Prerequisites (ferric-runtime changes)

Both the Python bindings and this FFI expansion require new public methods on `Engine` and `ModuleRegistry`. These are identical to those listed in the Python Bindings Plan:

### `Engine::template_slot_names`

```rust
/// Return the slot names for a named template, or `None` if the template
/// does not exist.
pub fn template_slot_names(&self, name: &str) -> Option<Vec<&str>> {
    let tid = self.template_ids.get(name)?;
    let def = self.template_defs.get(*tid)?;
    Some(def.slot_names.iter().map(String::as_str).collect())
}
```

Required by: `ferric_engine_template_slot_count`, `ferric_engine_template_slot_name`.

### `Engine::template_name_by_id`

```rust
/// Return the template name for a `TemplateId`, or `None` if the ID
/// is not registered.
pub fn template_name_by_id(&self, tid: TemplateId) -> Option<&str> {
    self.template_defs.get(tid).map(|def| def.name.as_str())
}
```

Required by: `ferric_engine_get_fact_template_name`.

### `Engine::modules`

```rust
/// Return the names of all registered modules.
pub fn modules(&self) -> Vec<&str> {
    self.module_registry.module_names()
}
```

Required by: `ferric_engine_module_count`, `ferric_engine_module_name`.

### `ModuleRegistry::module_names`

```rust
/// Return the names of all registered modules.
pub fn module_names(&self) -> Vec<&str> {
    self.modules.values().map(|m| m.name.as_str()).collect()
}
```

Required by: `Engine::modules()`.

---

## Existing FFI Functions (Unchanged)

The following functions exist today and are **not modified** by this plan:

| Function | Purpose |
|---|---|
| `ferric_engine_new` | Create engine (default config) |
| `ferric_engine_new_with_config` | Create engine (explicit config) |
| `ferric_engine_free` | Free engine handle |
| `ferric_engine_load_string` | Load CLIPS source string |
| `ferric_engine_last_error` | Per-engine error (borrowed pointer) |
| `ferric_engine_last_error_copy` | Per-engine error (buffer copy) |
| `ferric_engine_clear_error` | Clear per-engine error |
| `ferric_engine_reset` | Reset engine to initial state |
| `ferric_engine_run` | Run execution loop |
| `ferric_engine_step` | Single-step execution |
| `ferric_engine_assert_string` | Assert fact from CLIPS source |
| `ferric_engine_retract` | Retract fact by ID |
| `ferric_engine_get_output` | Get output channel (borrowed pointer) |
| `ferric_engine_action_diagnostic_count` | Diagnostic count |
| `ferric_engine_action_diagnostic_copy` | Copy diagnostic to buffer |
| `ferric_engine_clear_action_diagnostics` | Clear diagnostics |
| `ferric_engine_fact_count` | User-visible fact count |
| `ferric_engine_get_fact_field_count` | Field count for a fact |
| `ferric_engine_get_fact_field` | Get single field value |
| `ferric_engine_get_global` | Get global variable value |
| `ferric_last_error_global` | Global error (borrowed pointer) |
| `ferric_last_error_global_copy` | Global error (buffer copy) |
| `ferric_clear_error_global` | Clear global error |
| `ferric_string_free` | Free heap string |
| `ferric_value_free` | Free FerricValue resources |
| `ferric_value_array_free` | Free FerricValue array |

---

## Implementation Sequencing

### Phase A: Prerequisites

1. Add `Engine::template_slot_names`, `Engine::template_name_by_id`, `Engine::modules` to `crates/ferric-runtime/src/engine.rs`.
2. Add `ModuleRegistry::module_names` to `crates/ferric-runtime/src/modules.rs`.
3. Unit tests for all new methods.

### Phase B: New Types & Value Conversion

4. Add `FerricFactType`, `FerricHaltReason` to `crates/ferric-ffi/src/types.rs`.
5. Add `ferric_to_value()` reverse conversion to `crates/ferric-ffi/src/types.rs`.
6. Add 5 value construction helpers (`ferric_value_integer`, etc.) to `crates/ferric-ffi/src/types.rs`.

### Phase C: New Engine Functions

Implement in `crates/ferric-ffi/src/engine.rs` in dependency order:

7. Fact iteration: `ferric_engine_fact_ids`, `ferric_engine_find_fact_ids`.
8. Fact type & names: `ferric_engine_get_fact_type`, `ferric_engine_get_fact_relation`, `ferric_engine_get_fact_template_name`.
9. Structured assertion: `ferric_engine_assert_ordered`.
10. Template introspection: `ferric_engine_template_count`, `ferric_engine_template_name`, `ferric_engine_template_slot_count`, `ferric_engine_template_slot_name`.
11. Rule introspection: `ferric_engine_rule_count`, `ferric_engine_rule_info`.
12. Module operations: `ferric_engine_current_module`, `ferric_engine_get_focus`, `ferric_engine_focus_stack_depth`, `ferric_engine_focus_stack_entry`, `ferric_engine_module_count`, `ferric_engine_module_name`.
13. Agenda/halt/input/clear: `ferric_engine_agenda_count`, `ferric_engine_is_halted`, `ferric_engine_halt`, `ferric_engine_push_input`, `ferric_engine_clear`.
14. Convenience: `ferric_engine_new_with_source`, `ferric_engine_new_with_source_config`, `ferric_engine_clear_output`, `ferric_engine_run_ex`.

### Phase D: Header & Tests

15. Verify `cbindgen` generates correct C header entries for all new functions and types.
16. Add FFI integration tests in `crates/ferric-ffi/src/tests.rs` covering:
    - Each new function's happy path.
    - Null pointer rejection for all pointer parameters.
    - Buffer-too-small / size-query patterns.
    - Thread violation detection on new functions.
    - Value construction and `ferric_to_value` round-trip.

---

## Testing Approach

### Unit Tests (Rust)

Each new function gets at least:

1. **Happy path:** Create engine, load rules, exercise the function, verify output.
2. **Null pointer:** Pass null engine, null output pointers → correct error code.
3. **Not found:** Query non-existent facts, templates, modules → `NotFound`.
4. **Buffer pattern:** Size query (null buf, 0 len), exact-fit buffer, undersized buffer.

### Integration Tests

End-to-end scenarios:

1. **Fact lifecycle:** Assert ordered (structured), iterate fact IDs, discriminate type, read relation/template name, retract, verify ID disappears from iteration.
2. **Template introspection:** Load `(deftemplate person (slot name) (slot age))`, verify template count, name, slot count, slot names.
3. **Rule introspection:** Load rules, verify rule count and info (name, salience).
4. **Module introspection:** Load multi-module rules, verify module count and names, focus stack operations.
5. **Extended run:** Verify `ferric_engine_run_ex` returns correct halt reasons for each scenario (agenda empty, limit reached, halt requested).
6. **Value construction → assert:** Build `FerricValue` array with value helpers, call `ferric_engine_assert_ordered`, verify fields via `ferric_engine_get_fact_field`.

### Property-Based Tests

Use `proptest` for:

- Arbitrary `FerricValue` → `ferric_to_value` → `value_to_ferric` round-trip (for Integer, Float, Symbol types).
- Arbitrary relation names with arbitrary field counts → assert and retrieve.

---

## Summary: New Functions by Category

| # | Category | Function | Wraps |
|---|---|---|---|
| 1 | Fact Iteration | `ferric_engine_fact_ids` | `Engine::facts()` |
| 2 | Fact Iteration | `ferric_engine_find_fact_ids` | `Engine::find_facts()` |
| 3 | Fact Type | `ferric_engine_get_fact_type` | `Fact::Ordered` vs `Fact::Template` |
| 4 | Fact Type | `ferric_engine_get_fact_relation` | `OrderedFact::relation` |
| 5 | Fact Type | `ferric_engine_get_fact_template_name` | `Engine::template_name_by_id()` |
| 6 | Assertion | `ferric_engine_assert_ordered` | `Engine::assert_ordered()` |
| 7 | Value | `ferric_value_integer` | Pure constructor |
| 8 | Value | `ferric_value_float` | Pure constructor |
| 9 | Value | `ferric_value_symbol` | Pure constructor |
| 10 | Value | `ferric_value_string` | Pure constructor |
| 11 | Value | `ferric_value_void` | Pure constructor |
| 12 | Template | `ferric_engine_template_count` | `Engine::templates().len()` |
| 13 | Template | `ferric_engine_template_name` | `Engine::templates()[i]` |
| 14 | Template | `ferric_engine_template_slot_count` | `Engine::template_slot_names()` |
| 15 | Template | `ferric_engine_template_slot_name` | `Engine::template_slot_names()[i]` |
| 16 | Rule | `ferric_engine_rule_count` | `Engine::rules().len()` |
| 17 | Rule | `ferric_engine_rule_info` | `Engine::rules()[i]` |
| 18 | Module | `ferric_engine_current_module` | `Engine::current_module()` |
| 19 | Module | `ferric_engine_get_focus` | `Engine::get_focus()` |
| 20 | Module | `ferric_engine_focus_stack_depth` | `Engine::get_focus_stack().len()` |
| 21 | Module | `ferric_engine_focus_stack_entry` | `Engine::get_focus_stack()[i]` |
| 22 | Module | `ferric_engine_module_count` | `Engine::modules()` |
| 23 | Module | `ferric_engine_module_name` | `Engine::modules()[i]` |
| 24 | Agenda | `ferric_engine_agenda_count` | `Engine::agenda_len()` |
| 25 | Halt | `ferric_engine_is_halted` | `Engine::is_halted()` |
| 26 | Halt | `ferric_engine_halt` | `Engine::halt()` |
| 27 | Input | `ferric_engine_push_input` | `Engine::push_input()` |
| 28 | Clear | `ferric_engine_clear` | `Engine::clear()` |
| 29 | Convenience | `ferric_engine_new_with_source` | `Engine::with_rules()` |
| 30 | Convenience | `ferric_engine_new_with_source_config` | `Engine::with_rules_config()` |
| 31 | Convenience | `ferric_engine_clear_output` | `Engine::clear_output_channel()` |
| 32 | Convenience | `ferric_engine_run_ex` | `Engine::run()` + `HaltReason` |

---

## Critical Files

| File | Changes |
|---|---|
| `crates/ferric-runtime/src/engine.rs` | Add 3 new pub methods |
| `crates/ferric-runtime/src/modules.rs` | Add `module_names()` |
| `crates/ferric-ffi/src/types.rs` | Add `FerricFactType`, `FerricHaltReason`, value constructors, `ferric_to_value` |
| `crates/ferric-ffi/src/engine.rs` | Add 27 new `extern "C"` functions |
| `crates/ferric-ffi/src/tests.rs` | Add tests for all new functions |
