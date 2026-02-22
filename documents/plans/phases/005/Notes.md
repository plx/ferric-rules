# Phase 005 Notes

## Pass 001: Phase 5 Baseline and Harness Alignment

### What was done
- Phase 5 baseline assumptions documented in crate-level doc comments for both ferric-ffi and ferric-cli
- FFI test harness: fixture path helpers, fixture existence checks, constants for fixture names
- CLI test harness: binary invocation via CARGO_BIN_EXE_ferric, exit code + stdout/stderr assertion helpers
- Test fixture .clp files for both FFI and CLI test suites
- CLI integration tests verifying command dispatch, exit codes, and usage messages

### Pre-existing issue fixed
- `debug_assert_consistency` methods in ferric-runtime (engine.rs, modules.rs, functions.rs, test_helpers.rs) were missing `#[cfg(any(test, debug_assertions))]` gates. This was invisible until the `ffi-release` profile (which inherits from `release`, so debug_assertions=false) was added. The ferric-core methods already had these gates, but the ferric-runtime wrappers did not. Fixed by adding the gate consistently.
- Also moved `HashSet` import inside the gated method body to eliminate unused-import warning in release profile.

### Decisions
- Passes 001 and 002 were executed together since the harness code (001) needs the crate scaffolding (002) to exist
- FFI test helpers live in src/tests/test_helpers.rs (unit-test style, inside the crate)
- CLI test helpers live in tests/cli_helpers.rs (integration-test style, external to the crate)
- CLI uses manual arg parsing (no clap dependency) — keeps it simple for now, can upgrade later if needed

### Agent team experiment notes
- Used a team with two parallel agents: ffi-harness and cli-harness
- Both agents completed their tasks successfully in parallel
- Total wall-clock time was less than running them sequentially would have been
- The team overhead (task creation, messaging, shutdown) is non-trivial but worthwhile for truly parallel work

## Pass 002: Workspace Profiles and Crate Scaffolding

### What was done
- ferric-ffi and ferric-cli added to workspace
- FFI profile matrix (ffi-dev, ffi-release) with panic = "abort"
- ferric-ffi: cdylib + staticlib, module structure for error/engine/types/header
- ferric-cli: binary crate with command dispatch skeleton (run/check/repl/version)
- Per-crate lint override: ferric-ffi allows unsafe_code (needed for FFI)

### Remaining notes
- `version` command works (exits 0, prints version)
- `run`, `check`, `repl` are placeholder stubs (exit 1, print "not yet implemented")
- `cbindgen` not yet added as a dependency (comes in Pass 008)
- `rustyline` not yet added (comes in Pass 011)

## Pass 003: FFI Error Model and Unified Return Convention

### What was done
- `FerricError` enum: `#[repr(C)]` with 11 codes covering all runtime error categories
- Thread-local global error storage with CString-based C API for safe pointer returns
- Per-engine `EngineErrorState` for engine-scoped error isolation
- Error mapping functions from `EngineError` and `LoadError` to `FerricError`
- `FerricEngine` opaque handle struct combining runtime Engine + error state
- 19 comprehensive tests covering error codes, channel isolation, C API, and mappings

### Design decisions
- `FerricError::Ok = 0` follows C convention for success
- `InternalError = 99` leaves room for future codes
- Thread-local CString for `ferric_last_error_global` — pointer valid until next error-modifying call
- Kept separate `map_*` and `set_*_global` functions for flexibility (some callers may want to store in per-engine state instead)
- `FerricEngine` struct is defined early (even though lifecycle APIs come in Pass 004) to establish the pattern

### Clippy remediation
- Merged identical match arms (clippy::match_same_arms)
- Added `#[allow(dead_code)]` on scaffolding items not yet consumed by later passes

## Pass 004: FFI Thread Affinity and Engine Lifecycle APIs

### What was done
- Made `Engine::check_thread_affinity` public for FFI access
- Internal helper pattern: `validate_engine_ptr` (shared) → `check_thread_affinity` → `validate_engine_ptr_mut` (mutable)
- Complete lifecycle API: new, free, load_string, reset, last_error, clear_error
- Thread violation test uses usize→raw pointer roundtrip to cross thread boundary

### Design decisions
- No `debug_assert!` in FFI thread check — FFI code should never panic, even in debug builds
- Thread violation always returns `FerricError::ThreadViolation` with descriptive error in global channel
- `ferric_engine_last_error` deliberately skips thread-affinity check (diagnostic operation should always work)
- `ferric_engine_free(null)` returns Ok (C-style null-safe free)
- `ferric_engine_load_string` stores first load error in global channel (Vec<LoadError> → single FerricError)

### Agent team notes
- Single agent (ffi-lifecycle) worked well for this focused pass
- Agent initially produced a thread test that tried to `move` Engine across threads (Engine is !Send)
- Agent self-corrected during its run, removing the debug_assert and fixing the test

## Pass 005: FFI Core run/step/assert/retract APIs

### What was done
- Execution APIs: `ferric_engine_run` (with limit), `ferric_engine_step` (with status), `ferric_engine_get_output`
- Fact APIs: `ferric_engine_assert_string` (via load_str), `ferric_engine_retract` (via FactId↔u64)
- 18 comprehensive tests covering all API paths

### Design decisions
- `ferric_engine_assert_string` delegates to `load_str` which handles full CLIPS assert forms — this means users pass `(assert (foo 1))` not just `(foo 1)`. This matches CLIPS's own batch-processing semantics.
- `FactId` is converted to/from `u64` using slotmap's `KeyData::as_ffi()/from_ffi()` — this is the stable FFI-safe representation
- Output channel is `"t"` for `(printout t ...)`, matching CLIPS convention — NOT `"stdout"`
- `ferric_engine_step` status codes: 1=fired, 0=empty, -1=halted — simple C-friendly convention

## Pass 006: FFI Copy-To-Buffer Error APIs and Edge Cases

### What was done
- Shared `copy_error_to_buffer` helper in error.rs implements the full branch matrix
- `ferric_last_error_global_copy`: C API for copying global error to caller buffer
- `ferric_engine_last_error_copy`: C API for copying per-engine error to caller buffer
- 16 exhaustive tests covering every documented contract branch

### Design decisions
- Shared helper avoids code duplication between global and per-engine variants
- `out_len` null check returns `InvalidArgument` (not `NullPointer` — it's a logic error, not a missing object)
- `*out_len = 0` on NullPointer for engine variant — keeps contract consistent: `*out_len` always written when `out_len` is non-null
- Per-engine tests set error state directly via raw pointer (`(*engine).error_state.set(...)`) since no current load path writes to per-engine channel
- `ferric_engine_last_error_copy` skips thread-affinity check (diagnostic operation, matches `ferric_engine_last_error`)

## Pass 007: FFI Extended API Value and Query Surface

### What was done
- Added `Engine::resolve_symbol(sym) -> Option<&str>` to ferric-runtime public API
- `FerricValueType` + `FerricValue` C-facing types in types.rs
- `value_to_ferric` internal conversion: heap-allocates CStrings for Symbol/String, arrays for Multifield
- Resource management: `ferric_string_free`, `ferric_value_free` (recursive), `ferric_value_array_free`
- Query APIs: `ferric_engine_fact_count`, `ferric_engine_get_fact_field_count`, `ferric_engine_get_fact_field`
- Global access: `ferric_engine_get_global` with module visibility resolution
- 22 tests covering conversion, ownership, queries, and round-trip cycles

### Design decisions
- `FerricValue` uses flat struct (not C union) — simpler for C consumers, active fields determined by `value_type`
- Symbol values get their text heap-allocated via `engine.resolve_symbol()` → `CString::into_raw()`
- Multifield arrays are heap-allocated via `Box::into_raw(boxed_slice)` — recursive free in `free_value_resources`
- `ExternalAddress.pointer` is NOT owned by `FerricValue` — caller-managed lifetime
- Fact queries use shared references only (`validate_engine_ptr` + engine methods that self-check thread affinity)
- `get_global` explicitly checks thread affinity since `Engine::get_global` doesn't
- `resolve_symbol` skips thread check (pure read of immutable interned data, consistent with `is_halted`, `get_output`)
- Fixed clippy `float_cmp` in void test: use `abs() < EPSILON` instead of `assert_eq!` for floats

### Remaining notes
- `ferric_engine_assert_string` still returns `out_fact_id = 0` — a real assert-and-return-ID API could be added later
- No fact enumeration/iteration API yet (can be added when needed)
- No template fact name resolution (would need `template_defs` accessor on Engine)

## Pass 008: C Header Generation, Thread-Safety Banner, and Ownership Docs

### What was done
- cbindgen 0.29 integrated via workspace deps, build.rs, and cbindgen.toml
- Generated `ferric.h` with all 20 FFI functions, 3 repr(C) types, opaque `FerricEngine`
- Thread-safety and ownership preamble documenting all pointer lifetime contracts
- 12 smoke tests verify header contains expected symbols and documentation

### Design decisions
- Preamble lives in build.rs (can't import from crate being built)
- Header generated to source tree (`$CARGO_MANIFEST_DIR/ferric.h`) for git commit
- Drift detection via CI `git diff --exit-code` after build + smoke tests in code
- `FerricEngine` is opaque (not in `[export].include`) — cbindgen emits forward declaration
- cbindgen renames `float` field to `float_` to avoid C keyword collision
- ScreamingSnakeCase enum variants: `FERRIC_ERROR_OK`, `FERRIC_VALUE_TYPE_VOID`, etc.

## Pass 009: FFI Artifact Build Matrix and Panic Policy Verification

### What was done
- Build matrix tests verifying ffi-dev and ffi-release profiles produce correct artifacts
- Platform-conditional artifact name constants (macOS/Linux/Windows)
- `default_test_profile_uses_unwind` test proving catch_unwind works in test builds
- Build instructions added to crate-level docs

### Design decisions
- Build matrix tests are `#[ignore]` (slow subprocess builds) — run with `--ignored` in CI
- `#[ignore]` requires reason string due to project's clippy::ignore_without_reason lint
- Unwind test is non-ignored — lightweight proof that dev/test profiles don't abort

## Pass 010: CLI run/check/version Commands and Diagnostics

### What was done
- Full `ferric run` implementation: load → reset → run → print output
- Full `ferric check` implementation: load only, validate parse/compile
- 8 new CLI integration tests covering success, failure, and edge cases

### Bug fix: module-qualified rule name association
- Rules with qualified names (e.g., `MAIN::start`) inside `(defmodule REPORT ...)` sections were being associated with REPORT (the current module) instead of MAIN (the declared module)
- Fixed in loader.rs by calling `parse_qualified_name` during construct collection to extract the module prefix
- The `parse_qualified_name` import was already present but unused — this was clearly the intended use case

### Design decisions
- Output goes to stdout (channel "t"), diagnostics to stderr
- File-not-found is exit 1 (not exit 2 — it's a runtime error, not a usage error)
- `check` is silent on success (no "ok" message) — matches linting tool convention
- Action diagnostics printed as warnings to stderr after run

## Pass 011: REPL Interactive Loop and Command Surface

### What was done
- Full REPL with rustyline 17 (line editing, history)
- Multiline input: accumulates lines until parens balanced (string/comment-aware)
- REPL commands: (reset), (run [N]), (facts), (agenda), (clear), (exit)/(quit), (load "file")
- General CLIPS forms evaluated via load_str, output printed after each form
- 10 unit tests + 5 integration tests (piped stdin via subprocess)

### Design decisions
- rustyline v17 (latest stable) — API is `DefaultEditor`, `ReadlineError::Eof/Interrupted`
- `(reset)` is silent on success (matches CLIPS behavior)
- `(load "path")` uses simple quote stripping (not full CLIPS string parsing)
- Template facts displayed as `(template-fact v1 v2 ...)` — slot names not available (would need engine.resolve_template_slots)
- Added slotmap as direct dependency for `Key::data().as_ffi()` on FactId
- Ctrl-D and Ctrl-C both exit cleanly with code 0

## Pass 012: Phase 5 Integration and Exit Validation

### What was done
- 6 FFI diagnostic parity tests covering full C embedding flows
- 2 CLI diagnostic parity tests verifying error output format
- All quality gates verified clean

### Phase 5 overall observations
- Phase added 145 tests (1055 → 1200)
- 2 new crates: ferric-ffi (cdylib+staticlib), ferric-cli (binary)
- 20 extern "C" functions in the FFI surface
- 3 repr(C) types: FerricError, FerricValueType, FerricValue
- Generated ferric.h header with cbindgen
- One latent bug found and fixed: module-qualified rule name association in loader.rs
- One pre-existing gap filled: Engine::resolve_symbol public method added

### Agent team experiment observations (across all passes)
- Passes 001-002: Used parallel agents for FFI+CLI harness — effective for truly independent work
- Passes 003-012: Single focused agents proved most effective for sequential FFI/CLI work
- Agent overhead (task creation, messaging, shutdown) is non-trivial for small passes
- Detailed specs lead to high-quality first-pass results; vague specs lead to back-and-forth
- Agents don't always format code perfectly — `cargo fmt --all` is essential after agent work
- Agents occasionally trigger clippy issues (float_cmp, doc_markdown) — quick manual fixups needed
