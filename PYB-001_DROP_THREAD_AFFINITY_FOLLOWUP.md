# PYB-001 Follow-Up: Cross-Thread Drop Leak in Python Engine Wrapper

## Context

The PYB-001 remediation replaced PyO3 `unsendable` panics with explicit thread checks and `FerricRuntimeError` for cross-thread calls.

That behavior goal is mostly achieved, but the current implementation introduces a new high-severity lifecycle problem in destructor behavior.

## Current Problem

In [`crates/ferric-python/src/engine.rs`](/Users/prb/conductor/workspaces/ferric-rules/stockholm/crates/ferric-python/src/engine.rs):

- `PyEngine` now stores `engine: ManuallyDrop<Engine>` ([line 46](/Users/prb/conductor/workspaces/ferric-rules/stockholm/crates/ferric-python/src/engine.rs:46))
- `unsafe impl Send` and `unsafe impl Sync` are added ([lines 66-67](/Users/prb/conductor/workspaces/ferric-rules/stockholm/crates/ferric-python/src/engine.rs:66))
- `Drop` has thread-conditional behavior ([lines 69-88](/Users/prb/conductor/workspaces/ferric-rules/stockholm/crates/ferric-python/src/engine.rs:69)):
  - same thread: drop inner `Engine`
  - foreign thread: intentionally leak `Engine` and print to stderr

This means foreign-thread finalization does not panic, but leaks engine resources (facts, rete graph memory, interned symbols, etc.).

## Why This Is High Severity

1. Resource management regression
- The engine can hold substantial state; repeated foreign-thread drops can leak unbounded memory.

2. Unsafe contract fragility
- `unsafe impl Send/Sync` is justified by “every entry point checks thread,” but future methods can violate this by omission.
- The design shifts safety burden to human discipline across all future edits.

3. Runtime side effects in destructor
- `eprintln!` in `Drop` emits noise in normal application logs and test output.

4. Test suite currently encodes leak behavior as acceptable
- [`tests/test_threading.py`](/Users/prb/conductor/workspaces/ferric-rules/stockholm/crates/ferric-python/tests/test_threading.py) has `TestCrossThreadDrop.test_drop_on_foreign_thread_no_panic`, which treats “leak but no crash” as passing behavior.

## Reproduction

```python
import ferric, threading, gc

eng = ferric.Engine()
for i in range(10000):
    eng.assert_fact("x", i)

def drop_on_other_thread(e):
    del e

t = threading.Thread(target=drop_on_other_thread, args=(eng,))
del eng
t.start()
t.join()
gc.collect()
```

Observed today:
- no panic
- stderr message: engine leaked due to wrong-thread drop
- inner `Engine` not dropped

## Target Behavior

1. Cross-thread method/property access still raises `ferric.FerricRuntimeError`.
2. Cross-thread object finalization does **not** leak engine resources.
3. No panic-derived `PanicException` for public API usage paths.
4. No stderr writes from `Drop`.

## Recommended Fix Direction

Use a design that avoids `unsafe Send/Sync` + `ManuallyDrop` leak fallback.

Preferred characteristics:
- No custom unsafe auto-trait impls for `PyEngine`
- Deterministic ownership and cleanup semantics
- Thread-affinity enforcement at call boundary, not via panic

Possible implementation paths (Claude should choose one and justify):

1. Re-architecture around explicit close/owner indirection
- Move actual engine ownership into a creator-thread-owned registry/handle model.
- `PyEngine` instances carry a lightweight handle; `drop` releases handle metadata safely.
- Actual engine destruction occurs on creator thread via explicit close/managed teardown.

2. Safe rollback if above is too large
- Revert to `#[pyclass(unsendable)]` semantics temporarily (panic path), remove unsafe/leak behavior, and document follow-up work to get catchable errors without unsafe drop leaks.
- This is less ergonomic but safer than silent resource leaks.

## Acceptance Criteria

1. No intentional leak path remains in `Drop`.
2. No `unsafe impl Send`/`unsafe impl Sync` on `PyEngine` unless backed by a stronger proven invariant than per-method checks.
3. No `eprintln!` in destructor paths.
4. Cross-thread call tests still pass (`FerricRuntimeError` expected) or are explicitly revised if fallback strategy is chosen.
5. Add/adjust tests to validate destructor behavior:
- A non-leaking teardown test (or equivalent instrumentation-backed assertion)
- No unraisable drop warnings in pytest output for expected usage
6. `crates/ferric-python` test suite passes under `maturin develop` flow.

## Suggested Test/Instrumentation Additions

Because leak assertions are hard from Python black-box tests, add Rust-side instrumentation under `cfg(test)` or a test-only feature:

- `ENGINE_INSTANCE_COUNT` atomic increment on create, decrement on true drop
- expose test helper (only in test builds) to assert count returns to baseline

This gives strong confidence that cross-thread scenarios do not silently leak.

## Scope Notes

- Breaking API changes from PYB-002/PYB-003 are acceptable for now per project direction.
- This follow-up is specifically about eliminating leak-prone teardown semantics introduced by PYB-001 remediation.

