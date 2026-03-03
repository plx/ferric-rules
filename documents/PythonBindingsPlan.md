# Python Bindings Plan — `ferric-python`

## Purpose

Provide native Python bindings for the Ferric rules engine using PyO3 and maturin, enabling Python developers to embed and interact with the engine without CLIPS source-level indirection for common operations.

## Crate Structure

New workspace member at `crates/ferric-python/`:

```
crates/ferric-python/
├── Cargo.toml
├── pyproject.toml
├── build.rs
├── src/
│   ├── lib.rs          # #[pymodule] definition
│   ├── engine.rs       # #[pyclass(unsendable)] Engine wrapper
│   ├── value.rs        # Rust Value <-> Python native type conversion
│   ├── fact.rs         # Fact, OrderedFact, TemplateFact Python classes
│   ├── config.rs       # Strategy, Encoding PyO3 enums
│   ├── result.rs       # RunResult, HaltReason, FiredRule
│   └── error.rs        # Exception hierarchy (FerricError, etc.)
└── tests/
    ├── conftest.py
    ├── test_engine.py
    ├── test_facts.py
    ├── test_execution.py
    ├── test_values.py
    ├── test_config.py
    ├── test_errors.py
    ├── test_introspection.py
    └── test_io.py
```

### `Cargo.toml`

```toml
[package]
name = "ferric-python"
version.workspace = true
edition.workspace = true
rust-version.workspace = true
license.workspace = true

[lib]
name = "ferric"
crate-type = ["cdylib"]

[dependencies]
ferric-core = { workspace = true }
ferric-runtime = { workspace = true }
pyo3 = { version = "0.22", features = ["extension-module"] }

[build-dependencies]
pyo3-build-config = "0.22"
```

### `pyproject.toml`

```toml
[build-system]
requires = ["maturin>=1.0,<2.0"]
build-backend = "maturin"

[project]
name = "ferric"
requires-python = ">=3.9"
classifiers = [
    "Programming Language :: Rust",
    "Programming Language :: Python :: Implementation :: CPython",
]

[tool.maturin]
features = ["pyo3/extension-module"]
```

### `build.rs`

```rust
fn main() {
    pyo3_build_config::add_extension_module_link_args();
}
```

### Workspace Changes

Add to root `Cargo.toml`:

```toml
[workspace]
members = [
    # ... existing members ...
    "crates/ferric-python",
]

[workspace.dependencies]
pyo3 = { version = "0.22", features = ["extension-module"] }
pyo3-build-config = "0.22"
```

---

## Python API Surface

### Module Root

```python
import ferric

engine = ferric.Engine()
```

The `#[pymodule]` function registers all classes and enums into a single `ferric` module.

### Engine Lifecycle

```python
# Default configuration (UTF-8, Depth strategy)
engine = ferric.Engine()

# Explicit configuration
engine = ferric.Engine(strategy=Strategy.DEPTH, encoding=Encoding.UTF8)

# Convenience: load source and reset in one call
engine = ferric.Engine.from_source("(defrule ...)")
engine = ferric.Engine.from_source("(defrule ...)", strategy=Strategy.LEX)

# Context manager (calls engine.clear() on exit)
with ferric.Engine.from_source(rules) as engine:
    result = engine.run()
```

**Rust mapping:**

| Python | Rust |
|---|---|
| `Engine()` | `Engine::new(EngineConfig::default())` |
| `Engine(strategy=..., encoding=...)` | `Engine::new(EngineConfig { ... })` |
| `Engine.from_source(src)` | `Engine::with_rules(src)` |
| `Engine.from_source(src, strategy=...)` | `Engine::with_rules_config(src, config)` |
| `__enter__` / `__exit__` | Return self / call `Engine::clear()` |

### Loading

```python
engine.load("(defrule ...)")           # CLIPS source string
engine.load_file("rules.clp")         # Load from file path
```

**Rust mapping:**

| Python | Rust |
|---|---|
| `engine.load(src)` | `Engine::load_str(src)` |
| `engine.load_file(path)` | `Engine::load_file(path)` |

### Fact Operations

```python
# Assert via CLIPS syntax
fid = engine.assert_string('(color red)')

# Assert structured (no CLIPS parsing)
fid = engine.assert_fact("color", "red")
fid = engine.assert_fact("point", 3, 4.5)

# Retract
engine.retract(fid)

# Query
fact = engine.get_fact(fid)        # Fact object or None
facts = engine.facts()             # list[Fact]
facts = engine.find_facts("color") # list[Fact] by relation
```

**Rust mapping:**

| Python | Rust |
|---|---|
| `engine.assert_string(src)` | `Engine::load_str(src)` with `(assert ...)` wrapper, or direct source parsing |
| `engine.assert_fact(rel, *args)` | `Engine::assert_ordered(rel, values)` with `python_to_value()` conversion |
| `engine.retract(fid)` | `Engine::retract(FactId)` |
| `engine.get_fact(fid)` | `Engine::get_fact(FactId)` → snapshot to `Fact` pyclass |
| `engine.facts()` | `Engine::facts()` → collect snapshots |
| `engine.find_facts(rel)` | `Engine::find_facts(rel)` → collect snapshots |

`assert_string` parses a bare fact assertion string by wrapping it in `(assert ...)` and loading via `Engine::load_str`, then extracting the resulting fact ID. Alternatively, it can use the engine's assertion path directly if the string is a bare ordered fact like `(color red)`.

### Execution

```python
result = engine.run()              # RunResult(rules_fired=N, halt_reason=...)
result = engine.run(limit=100)     # Run at most 100 rule firings
fired = engine.step()              # FiredRule or None
engine.halt()
engine.reset()
engine.clear()
```

**Rust mapping:**

| Python | Rust |
|---|---|
| `engine.run()` | `Engine::run(RunLimit::Unlimited)` |
| `engine.run(limit=N)` | `Engine::run(RunLimit::Count(N))` |
| `engine.step()` | `Engine::step()` |
| `engine.halt()` | `Engine::halt()` |
| `engine.reset()` | `Engine::reset()` |
| `engine.clear()` | `Engine::clear()` |

### Properties

```python
engine.fact_count       # int — number of user-visible facts
engine.is_halted        # bool
engine.agenda_size      # int — number of pending activations
engine.current_module   # str — name of the current module
engine.focus            # str | None — top of focus stack
engine.focus_stack      # list[str] — all modules in focus stack
engine.diagnostics      # list[str] — non-fatal action diagnostics
```

**Rust mapping:**

| Python | Rust |
|---|---|
| `fact_count` | `Engine::facts()?.count()` |
| `is_halted` | `Engine::is_halted()` |
| `agenda_size` | `Engine::agenda_len()` |
| `current_module` | `Engine::current_module()` |
| `focus` | `Engine::get_focus()` |
| `focus_stack` | `Engine::get_focus_stack()` |
| `diagnostics` | `Engine::action_diagnostics()` → map to strings |

### Introspection

```python
engine.rules()          # list[tuple[str, int]]  — (name, salience)
engine.templates()      # list[str]              — template names
engine.get_global("x")  # Python native value or None
```

**Rust mapping:**

| Python | Rust |
|---|---|
| `engine.rules()` | `Engine::rules()` |
| `engine.templates()` | `Engine::templates()` |
| `engine.get_global(name)` | `Engine::get_global(name)` → `value_to_python()` |

### I/O

```python
engine.get_output("stdout")      # str | None
engine.clear_output("stdout")
engine.push_input("some line")
```

**Rust mapping:**

| Python | Rust |
|---|---|
| `engine.get_output(ch)` | `Engine::get_output(ch)` |
| `engine.clear_output(ch)` | `Engine::clear_output_channel(ch)` |
| `engine.push_input(line)` | `Engine::push_input(line)` |

### Python Protocols

```python
repr(engine)      # "Engine(facts=3, rules=2, halted=False)"
len(engine)       # fact_count
fid in engine     # __contains__ by fact ID
```

`__repr__` uses `fact_count`, `rules().len()`, and `is_halted`. `__len__` returns `fact_count`. `__contains__` calls `get_fact(fid)` and returns `True` if the result is `Some`.

---

## Value Conversion

### `value_to_python(py, val, engine)` — Rust → Python

| Rust `Value` | Python type | Notes |
|---|---|---|
| `Integer(i64)` | `int` | Direct conversion |
| `Float(f64)` | `float` | Direct conversion |
| `Symbol(sym)` | `str` | Resolve via `engine.resolve_symbol(sym)` |
| `String(s)` | `str` | `FerricString::as_str()` |
| `Multifield(vals)` | `list` | Recursive conversion of each element |
| `Void` | `None` | Maps to Python `None` |
| `ExternalAddress` | `None` | Not supported in v1 |

### `python_to_value(obj, engine)` — Python → Rust

| Python type | Rust `Value` | Notes |
|---|---|---|
| `int` | `Integer(i64)` | Overflow → `FerricError` |
| `float` | `Float(f64)` | Direct conversion |
| `str` | `Symbol(sym)` | Intern via `engine.intern_symbol(s)` |
| `bool` | `Symbol` | `True` → `TRUE`, `False` → `FALSE` |
| `list` | `Multifield` | Recursive conversion |
| `tuple` | `Multifield` | Same as list |
| `None` | `Void` | |

Implementation notes:
- `python_to_value` must borrow the engine mutably for `intern_symbol` (symbol creation).
- Round-trip fidelity: `int → Integer → int`, `float → Float → float`. Symbol round-trip: `str → Symbol → str` (resolved text).
- Strings in CLIPS source syntax (`"hello"`) are `Value::String`; bare words are `Value::Symbol`. The Python API uses `str` for both — `python_to_value` defaults to Symbol since that's the common case for structured assertion.

---

## Fact Representation

`Fact` is a `#[pyclass]` holding a **snapshot** (value copy, not engine reference). This avoids lifetime issues and ensures facts remain valid after engine mutations.

### Fields

```python
fact.id             # int — fact ID (from FactId, converted to u64)
fact.fact_type      # FactType.ORDERED or FactType.TEMPLATE
fact.relation       # str | None — ordered fact relation name
fact.template_name  # str | None — template fact template name
fact.fields         # list — ordered: field values; template: slot values
fact.slots          # dict[str, object] | None — template only
```

### FactType Enum

```python
class FactType:
    ORDERED = 0
    TEMPLATE = 1
```

Implemented as a `#[pyclass]` with `#[classattr]` constants.

### Snapshot Construction

When creating a `Fact` snapshot from Rust:

```rust
fn fact_to_python(py: Python, fact_id: FactId, fact: &ferric_core::Fact, engine: &Engine) -> Fact {
    match fact {
        ferric_core::Fact::Ordered(ordered) => Fact {
            id: fact_id.data().as_ffi(),
            fact_type: FactType::ORDERED,
            relation: Some(engine.resolve_symbol(ordered.relation)
                .unwrap_or("<unknown>").to_string()),
            template_name: None,
            fields: ordered.fields.iter()
                .map(|v| value_to_python(py, v, engine))
                .collect(),
            slots: None,
        },
        ferric_core::Fact::Template(template) => {
            let tmpl_name = engine.template_name_by_id(template.template_id)
                .unwrap_or("<unknown>").to_string();
            let slot_names = engine.template_slot_names(&tmpl_name);
            Fact {
                id: fact_id.data().as_ffi(),
                fact_type: FactType::TEMPLATE,
                relation: None,
                template_name: Some(tmpl_name),
                fields: template.slot_values.iter()
                    .map(|v| value_to_python(py, v, engine))
                    .collect(),
                slots: slot_names.map(|names| {
                    names.iter().zip(template.slot_values.iter())
                        .map(|(name, val)| (name.to_string(), value_to_python(py, val, engine)))
                        .collect()
                }),
            }
        }
    }
}
```

### Protocols

- `__repr__`: `"Fact(id=3, type=ORDERED, relation='color', fields=['red'])"` or `"Fact(id=5, type=TEMPLATE, template='person', slots={'name': 'Alice'})"`.
- `__eq__`: By fact ID (two `Fact` snapshots with the same `id` are equal).
- `__hash__`: Hash of fact ID.

---

## Configuration Enums

### Strategy

```python
class Strategy:
    DEPTH = 0
    BREADTH = 1
    LEX = 2
    MEA = 3
```

Maps to `ConflictResolutionStrategy` in `ferric_core`.

### Encoding

```python
class Encoding:
    ASCII = 0
    UTF8 = 1
    ASCII_SYMBOLS_UTF8_STRINGS = 2
```

Maps to `StringEncoding` in `ferric_core`.

Both implemented as `#[pyclass]` with `#[classattr]` constants.

---

## Exception Hierarchy

```
FerricError (base)
├── FerricParseError
├── FerricCompileError
├── FerricRuntimeError
├── FerricFactNotFoundError
├── FerricModuleNotFoundError
└── FerricEncodingError
```

All prefixed with `Ferric` to avoid shadowing Python builtins (`RuntimeError`, `ModuleNotFoundError`).

### Mapping from Rust Errors

```rust
impl From<EngineError> for PyErr {
    fn from(err: EngineError) -> Self {
        match err {
            EngineError::WrongThread { .. } => FerricRuntimeError::new_err(err.to_string()),
            EngineError::FactNotFound(_) => FerricFactNotFoundError::new_err(err.to_string()),
            EngineError::Encoding(_) => FerricEncodingError::new_err(err.to_string()),
            _ => FerricError::new_err(err.to_string()),
        }
    }
}

impl From<LoadError> for PyErr {
    fn from(err: LoadError) -> Self {
        match err {
            LoadError::Parse(_) => FerricParseError::new_err(err.to_string()),
            LoadError::Compile(_) => FerricCompileError::new_err(err.to_string()),
            _ => FerricError::new_err(err.to_string()),
        }
    }
}

impl From<InitError> for PyErr {
    fn from(err: InitError) -> Self {
        // InitError wraps LoadError
        FerricError::new_err(err.to_string())
    }
}
```

---

## Thread Safety

### Design

- `Engine` wrapper is `#[pyclass(unsendable)]` — PyO3 rejects cross-thread access at runtime.
- The GIL serializes all calls → no concurrent `Rc` access within the Engine.
- Engine's Rust-level thread-affinity check passes because all calls come from the same OS thread that created the engine.

### Free-threaded Python (3.13+)

`unsendable` correctly rejects cross-thread access. No extra work needed — the PyO3 `unsendable` annotation handles this case.

### Interior Wrapper

```rust
#[pyclass(unsendable)]
pub struct PyEngine {
    engine: Engine,
}
```

The `PyEngine` struct holds an owned `Engine`. All `#[pymethods]` take `&self` or `&mut self` through PyO3's borrow mechanism, which serializes access via the GIL.

---

## Result Types

### RunResult

```python
class RunResult:
    rules_fired: int        # Number of rules fired
    halt_reason: HaltReason # Why execution stopped
```

### HaltReason

```python
class HaltReason:
    AGENDA_EMPTY = 0
    LIMIT_REACHED = 1
    HALT_REQUESTED = 2
```

### FiredRule

```python
class FiredRule:
    rule_name: str   # Name of the fired rule
```

`FiredRule` is returned by `engine.step()`. The `rule_name` is resolved from `RuleId` via `Engine::rule_name()`.

---

## `#[pymodule]` Definition

```rust
#[pymodule]
fn ferric(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_class::<PyEngine>()?;
    m.add_class::<Fact>()?;
    m.add_class::<FactType>()?;
    m.add_class::<Strategy>()?;
    m.add_class::<Encoding>()?;
    m.add_class::<RunResult>()?;
    m.add_class::<HaltReason>()?;
    m.add_class::<FiredRule>()?;

    // Register exception hierarchy
    m.add("FerricError", m.py().get_type::<FerricError>())?;
    m.add("FerricParseError", m.py().get_type::<FerricParseError>())?;
    m.add("FerricCompileError", m.py().get_type::<FerricCompileError>())?;
    m.add("FerricRuntimeError", m.py().get_type::<FerricRuntimeError>())?;
    m.add("FerricFactNotFoundError", m.py().get_type::<FerricFactNotFoundError>())?;
    m.add("FerricModuleNotFoundError", m.py().get_type::<FerricModuleNotFoundError>())?;
    m.add("FerricEncodingError", m.py().get_type::<FerricEncodingError>())?;

    Ok(())
}
```

---

## Shared Prerequisites (ferric-runtime changes)

The following new **public methods** must be added to `Engine` in `crates/ferric-runtime/src/engine.rs` before the Python bindings can be fully implemented. These are small, safe additions that do not break any existing API.

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

Required by: `Fact.slots` (template fact slot-name-to-value mapping).

### `Engine::template_name_by_id`

```rust
/// Return the template name for a `TemplateId`, or `None` if the ID
/// is not registered.
pub fn template_name_by_id(&self, tid: TemplateId) -> Option<&str> {
    self.template_defs.get(tid).map(|def| def.name.as_str())
}
```

Required by: `fact_to_python()` (resolving template name from `TemplateFact::template_id`).

### `Engine::modules`

```rust
/// Return the names of all registered modules.
pub fn modules(&self) -> Vec<&str> {
    self.module_registry.module_names()
}
```

Required by: future introspection API (not strictly needed for v1 Python bindings, but part of the shared prerequisite set).

### `ModuleRegistry::module_names`

New method on `ModuleRegistry` in `crates/ferric-runtime/src/modules.rs`:

```rust
/// Return the names of all registered modules.
pub fn module_names(&self) -> Vec<&str> {
    self.modules.values().map(|m| m.name.as_str()).collect()
}
```

Required by: `Engine::modules()`.

---

## Testing

### Test Files

```
crates/ferric-python/tests/
    conftest.py           # shared fixtures (engine instances, common rules)
    test_engine.py        # lifecycle: create, from_source, context manager, reset, clear
    test_facts.py         # assert_string, assert_fact, retract, get_fact, facts(), find_facts()
    test_execution.py     # run(), step(), halt(), run(limit=N)
    test_values.py        # conversion round-trips: int, float, str, list, None
    test_config.py        # Strategy/Encoding enums, keyword args
    test_errors.py        # exception hierarchy, error messages
    test_introspection.py # rules(), templates(), get_global()
    test_io.py            # get_output(), clear_output(), push_input()
```

### Running Tests

```bash
cd crates/ferric-python
maturin develop
pytest tests/ -v
```

### Key Test Scenarios

1. **Engine lifecycle:** Create default engine, create with config, `from_source` happy path and parse error, context manager calls `clear()` on exit (including on exception).
2. **Fact operations:** Assert ordered fact via CLIPS string, assert structured fact with type conversion, retract by ID, retract nonexistent ID raises `FerricFactNotFoundError`, `get_fact` returns `None` for missing, `facts()` excludes initial-fact, `find_facts` filters by relation.
3. **Execution:** `run()` fires all rules, `run(limit=1)` fires exactly one, `step()` returns `FiredRule` or `None`, `halt()` stops execution, `reset()` clears facts and re-asserts deffacts.
4. **Value round-trips:** `int → assert_fact → get_fact → int`, `float → assert_fact → get_fact → float`, `str → Symbol → str`, nested `list` → Multifield → `list`.
5. **Error hierarchy:** `isinstance(FerricParseError(), FerricError)` is `True`, parse error from invalid source, compile error from invalid rule, runtime error from wrong thread.
6. **Protocols:** `repr()` includes fact count and rule count, `len()` equals `fact_count`, `in` operator checks fact existence.

---

## Distribution

### Build

maturin builds platform-specific wheels:

```bash
maturin build --release
```

### PyPI

Package name: `ferric`. Install: `pip install ferric`.

### CI Matrix

Use `PyO3/maturin-action` for cross-platform wheel building:

| Platform | Targets |
|---|---|
| Linux | manylinux (x86_64, aarch64) |
| macOS | arm64, x86_64 (universal2) |
| Windows | x86_64 |

### Python Version Support

Python 3.9+ (matching PyO3 0.22 support matrix).

---

## Implementation Sequencing

1. **Shared prerequisites:** Add `template_slot_names`, `template_name_by_id`, `modules`, `module_names` to ferric-runtime.
2. **Crate scaffold:** Create `crates/ferric-python/` with `Cargo.toml`, `pyproject.toml`, `build.rs`, empty `src/lib.rs`.
3. **Value conversion:** Implement `value.rs` with `value_to_python` and `python_to_value`.
4. **Error hierarchy:** Implement `error.rs` with exception classes and `From` impls.
5. **Config enums:** Implement `config.rs` with `Strategy`, `Encoding`.
6. **Result types:** Implement `result.rs` with `RunResult`, `HaltReason`, `FiredRule`.
7. **Fact representation:** Implement `fact.rs` with `Fact`, `FactType`.
8. **Engine wrapper:** Implement `engine.rs` with `PyEngine` and all `#[pymethods]`.
9. **Module registration:** Implement `lib.rs` with `#[pymodule]`.
10. **Tests:** Write pytest suite covering all API surface.

---

## Non-Goals (v1)

- `ExternalAddress` support (maps to `None`).
- Template fact assertion via Python dict (requires template-aware assertion path).
- Async/await support (engine is synchronous and thread-affine).
- Submodule structure (flat `import ferric` is sufficient for v1).
- `__iter__` on engine for fact iteration (use `engine.facts()` instead).
