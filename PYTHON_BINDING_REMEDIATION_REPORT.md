# Python Binding Remediation Report

Date: 2026-03-27

Scope audited:
- `crates/ferric-python/src/*.rs`
- `crates/ferric-python/tests/*.py`
- Runtime API parity against `crates/ferric-runtime/src/engine.rs`
- Process/CI coverage for Python bindings

Validation run during audit:
- `cd crates/ferric-python && uv run pytest tests -q` -> `137 passed`
- Additional targeted runtime probes were executed for edge cases called out below.

## Coverage Snapshot

Current tests are strong on happy-path lifecycle, execution, facts, template facts, serialization, and basic error hierarchy. The suite is light or absent on several high-risk areas:
- Thread-affinity behavior (cross-thread use)
- Type fidelity edge cases (symbol vs string semantics)
- API contract edge cases (`assert_string` cardinality semantics)
- Identity semantics across engine instances (fact equality/hashing)
- API surface gaps (`load_file` path-like input, focus mutators, modules, diagnostics clearing)
- Negative conversion/error-path breadth (unsupported Python types, error aggregation shape)

## Remediation Issues

## PYB-001: Cross-thread access surfaces as `PanicException` (Rust panic path)

- Priority: P0
- Size: L
- Depends on: None

### What and why
`PyEngine` is `#[pyclass(unsendable)]`. Accessing the same engine from a different Python thread currently triggers a Rust panic inside PyO3 and raises `pyo3_runtime.PanicException` (a `BaseException`, not an `Exception`), with panic output emitted to stderr.

This is a correctness/safety issue because thread-affinity misuse should produce a regular ferric runtime error, not a panic-derived exception path.

### Sketch fix
- Introduce a non-panicking thread-affinity gate at the Python boundary.
- Ensure cross-thread misuse raises `FerricRuntimeError` with a stable message.
- Avoid exposing raw PyO3 panic semantics to library consumers.

### Acceptance criteria
1. Cross-thread method/property access raises `ferric.FerricRuntimeError`, not `PanicException`.
2. No Rust panic output is emitted for this case.
3. A dedicated test covers cross-thread read and mutating calls.

## PYB-002: Structured assertions cannot represent CLIPS `String` values

- Priority: P0
- Size: L
- Depends on: None

### What and why
`python_to_value` maps Python `str` to `Value::Symbol` unconditionally. This makes structured APIs (`assert_fact`, `assert_template`) unable to express CLIPS string literals distinctly from symbols.

Observed behavior: a template asserted with `name="Alice"` only matches `(name Alice)` rules, not `(name "Alice")` rules.

This is a semantic correctness gap and prevents full CLIPS value fidelity from Python.

### Sketch fix
- Introduce explicit Python-side value types (for example `Symbol`, `String`) or equivalent constructors/helpers.
- Define and document default mapping rules and migration behavior.
- Ensure conversions preserve symbol/string distinction in both directions.

### Acceptance criteria
1. Python API can intentionally create both CLIPS symbols and CLIPS strings via structured assertion APIs.
2. Rule matching distinguishes them correctly in tests.
3. Value round-trip tests verify symbol vs string fidelity.

## PYB-003: `assert_string` silently accepts multi-fact asserts but returns one ID

- Priority: P1
- Size: S
- Depends on: None

### What and why
`assert_string` wraps input as `(assert {source})`, then returns only the first `asserted_facts` entry. Inputs like `"(a) (b)"` assert two facts but return one id, making the API contract ambiguous and lossy.

### Sketch fix
- Make contract explicit and enforced:
  - Option A: require exactly one fact and error otherwise.
  - Option B: return `list[int]` for all asserted IDs (possibly via a new method to preserve compatibility).

### Acceptance criteria
1. Multi-fact input no longer silently drops IDs.
2. API docs specify cardinality behavior.
3. Tests cover single-fact, multi-fact, and malformed input.

## PYB-004: `Fact.__eq__` / `__hash__` conflate facts from different engines

- Priority: P1
- Size: M
- Depends on: None

### What and why
`Fact` equality/hash use only `fact.id`. Because IDs are engine-local, facts from different engines can compare equal and collide in hash sets/maps.

Observed behavior: first asserted fact in two fresh engines has same ID and compares equal.

### Sketch fix
- Include engine identity in fact snapshot identity semantics (for example hidden `engine_instance_id` + `fact_id` tuple).
- Update `__eq__` and `__hash__` accordingly.

### Acceptance criteria
1. Same fact ID from different engines is not equal.
2. Hash behavior matches equality.
3. Existing same-engine equality semantics remain intact.

## PYB-005: PyO3 classes/enums are registered under `builtins`, not `ferric`

- Priority: P1
- Size: S
- Depends on: None

### What and why
Core Python types (`Engine`, `Fact`, enums, result types) currently report `__module__ == "builtins"`. This is non-idiomatic and hurts introspection/documentation quality.

### Sketch fix
- Add `module = "ferric"` on all `#[pyclass]` declarations.
- Verify exception classes and regular classes have consistent module attribution.

### Acceptance criteria
1. All exported ferric classes/enums report `__module__ == "ferric"`.
2. Regression tests cover module attribution for representative types.

## PYB-006: Python API omits important runtime capabilities

- Priority: P1
- Size: M
- Depends on: None

### What and why
`ferric_runtime::Engine` exposes capabilities that Python currently lacks, including:
- Focus mutation (`set_focus`, `push_focus`)
- Module enumeration (`modules`)
- Diagnostics reset (`clear_action_diagnostics`)
- Direct template slot accessor (`get_fact_slot_by_name`)

Python currently exposes only part of this surface (mostly read-only focus state), limiting practical parity and forcing workarounds.

### Sketch fix
- Add targeted missing methods with Pythonic naming and exception mapping.
- Keep behavior aligned with existing runtime semantics.

### Acceptance criteria
1. Added methods expose the missing runtime operations above.
2. Methods are documented and covered by tests (success + error paths).
3. Error types map to ferric exception hierarchy consistently.

## PYB-007: `load_file` rejects `pathlib.Path` (inconsistent with snapshot APIs)

- Priority: P1
- Size: S
- Depends on: None

### What and why
`load_file` currently takes `&str`, so `pathlib.Path` inputs raise `TypeError`. `save_snapshot` / `from_snapshot_file` already accept path-like values via `PathBuf`, so behavior is inconsistent and non-idiomatic for Python.

### Sketch fix
- Change `load_file` signature to `PathBuf` like snapshot file methods.
- Preserve existing `str` usage compatibility.

### Acceptance criteria
1. `engine.load_file(pathlib.Path(...))` works.
2. Existing string path behavior is unchanged.
3. Tests cover both `str` and `Path` inputs.

## PYB-008: Conversion code uses `expect(...)`, leaving panic paths in bindings

- Priority: P1
- Size: M
- Depends on: None

### What and why
`value_to_python` currently uses multiple `expect(...)` calls. Allocation/conversion failures should surface as Python exceptions, not panic-derived failures.

### Sketch fix
- Refactor `value_to_python` to return `PyResult<PyObject>`.
- Propagate errors through all callers (`fact_to_python`, `get_global`, etc.).
- Remove panic-based conversion assumptions from Python boundary code.

### Acceptance criteria
1. No `expect(...)` remains in conversion paths crossing the Python boundary.
2. Conversion failures propagate as Python exceptions.
3. Existing behavior is preserved for normal inputs.

## PYB-009: Error translation drops multi-error context

- Priority: P1
- Size: M
- Depends on: None

### What and why
`load_errors_to_pyerr` returns immediately on first parse/compile error, which can hide additional loader diagnostics from the same call. This reduces debuggability for users loading larger sources with multiple problems.

### Sketch fix
- Preserve aggregated diagnostics while still classifying exception type.
- Consider attaching structured detail (for example an `.errors` list on exception instances).

### Acceptance criteria
1. Multi-error load failures expose all relevant diagnostics.
2. Parse/compile classification remains accurate.
3. Tests cover multi-error payload behavior.

## PYB-010: Test/process gaps on tricky Python-binding edge cases

- Priority: P1
- Size: M
- Depends on: PYB-001, PYB-002, PYB-003, PYB-004, PYB-005, PYB-006, PYB-007, PYB-008, PYB-009

### What and why
Current test suite is broad but still misses several risky behaviors:
- Cross-thread access behavior
- Symbol/string semantic fidelity
- `assert_string` cardinality edge cases
- Cross-engine fact identity semantics
- `load_file` path-like support
- Focus/module mutation APIs (once added)
- Rich multi-error mapping behavior

Also, local preflight (`just preflight-pr`) does not currently include `crates/ferric-python` pytest execution.

### Sketch fix
- Expand tests to cover the edge cases above.
- Add a local binding-test recipe and include it in preflight-pr.
- Keep CI and local preflight expectations aligned.

### Acceptance criteria
1. New tests exist for all edge cases listed above.
2. A single local command path runs ferric-python tests as part of PR preflight.
3. CI and local preflight coverage for Python bindings are consistent.

## Suggested Implementation Order

1. PYB-001
2. PYB-002
3. PYB-003
4. PYB-004
5. PYB-005
6. PYB-007
7. PYB-008
8. PYB-009
9. PYB-006
10. PYB-010

