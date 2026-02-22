# Phase 5 FFI Pass Notes

## Pass 003: FFI Error Model
- `ferric-ffi` crate uses `crate-type = ["cdylib", "staticlib"]`; tests run via the lib test harness
- Scaffold functions/fields that aren't yet called from non-test code need `#[allow(dead_code)]` to pass `-D warnings`
- `match_same_arms`: merge `EngineError::FactNotFound | EngineError::ModuleNotFound` → `NotFound`, etc.
- `with_global_error` must be `pub(crate)` — tests in submodules need access
- `ferric_last_error_global` lifetime pattern: store `CString` in a separate `thread_local!` inside the fn so the raw pointer outlives the function call
- Test count: 22 ferric-ffi after pass 003

## Pass 004: FFI Engine Lifecycle APIs and Thread Affinity
- `Engine::check_thread_affinity` was `pub(crate)`; changed to `pub` so ferric-ffi can call it
- `EngineConfig` is re-exported as `ferric_runtime::EngineConfig` (not from `ferric_runtime::engine`)
- `EngineError` is in `ferric_runtime::engine::EngineError`
- Two-step borrow pattern: validate ptr (shared ref) → thread check → validate ptr (mut ref)
- `debug_assert!` in FFI thread check causes test failures in debug mode (tests spawn threads to test violation); remove it and return `ThreadViolation` uniformly
- Thread-violation cross-thread test: `Engine` is `!Send`; transmit as `usize` (raw pointer addr), then reconstruct with `unsafe` in spawned thread
- Clippy `doc_markdown`: item names like `debug_assert` and `NullPointer` in doc comments need backticks
- Clippy `manual_let_else`: `let x = match { Ok(h) => h, Err(_) => return ... }` → `let Ok(x) = ... else { return ... };`
- Test count: 37 ferric-ffi after pass 004

## Pass 005: FFI Execution and Fact Mutation APIs
- **CRITICAL**: CLIPS channel for stdout is `"t"`, NOT `"stdout"`. `get_output("stdout")` returns None after `printout t`.
- `RunLimit::Count(limit as usize)` where `limit: i64` (checked >= 0) needs `#[allow(clippy::cast_sign_loss, clippy::cast_possible_truncation)]`
- `FactId` FFI conversion: `slotmap::KeyData::from_ffi(u64)` → `ferric_core::FactId::from(key_data)`; add `slotmap` to `[dependencies]` in `ferric-ffi/Cargo.toml`
- `ferric_engine_assert_string` uses `load_str` to handle full top-level `(assert ...)` forms; `out_fact_id` set to 0 (individual IDs not yet exposed)
- Test count: 55 ferric-ffi after pass 005

## Pass 006: Copy-To-Buffer Error APIs
- `copy_error_to_buffer` is a `pub(crate) unsafe fn` in `error.rs`; shared by both global and per-engine copy APIs
- NUL terminator written as `*buf.add(n) = 0` (the `c_char` cast is `i8` on most platforms, but the byte is 0 either way; writing via `buf.cast::<u8>()` and `.add(n) = 0` works)
- Per-engine error state (`FerricEngine::error_state`) is `pub(crate)`, so tests inside `src/tests/` can set it directly via `(*engine).error_state.set(msg)` — no helper needed
- Size query contract: `buf=null, buf_len=0` → `Ok` with `*out_len=needed`; `buf=null, buf_len>0` → `InvalidArgument`
- `ferric_engine_last_error_copy` deliberately skips thread-affinity check (diagnostic operation, same as `ferric_engine_last_error`)
- When engine pointer is null, write `*out_len = 0` before returning `NullPointer` (consistent with NotFound behavior)
- Test count: 71 ferric-ffi after pass 006

## Pass 007: Extended API Value and Query Surface
- `FerricValue` is a `#[repr(C)]` struct with all fields (integer, float, string_ptr, multifield_ptr, etc.); use `FerricValue::void()` as default base via `..FerricValue::void()`
- Multifield conversion: collect into `Vec<FerricValue>`, then `into_boxed_slice()` → `Box::into_raw()` → `cast::<FerricValue>()` (NOT `as *mut FerricValue` — clippy `ptr_as_ptr` error)
- `free_value_resources`: must handle recursive multifield freeing; match all enum arms exhaustively (clippy requires matching `Void | Integer | Float | ExternalAddress => {}`)
- `ferric_engine_fact_count` uses a shared ref (not mutable) — `facts()` does its own thread check internally
- `ferric_engine_get_fact_field` and `ferric_engine_get_fact_field_count` also work with shared refs
- `ferric_engine_get_global` needs `check_thread_affinity` (thread-sensitive operation)
- **FactId to u64 in tests**: `fact_id.data().as_ffi()` (use `slotmap::Key as _` import for the `.data()` method)
- **Clippy**: `match option { Some(x) => ..., None => ... }` with non-trivial branches → use `if let Some(x) = option { ... } else { ... }` (single_match_else lint)
- **Clippy**: Field names in doc comments like `string_ptr`, `multifield_ptr` need backtick wrapping
- Test count: 93 ferric-ffi after pass 007 (22 new tests in `tests/values.rs`)

## Pass 008: C Header Generation, Thread-Safety Banner, and Ownership Docs

### cbindgen Setup Pattern
- Add `cbindgen = "0.29"` to `[workspace.dependencies]` in root `Cargo.toml`
- Add `[build-dependencies] cbindgen = { workspace = true }` and `[dev-dependencies] cbindgen = { workspace = true }` to `ferric-ffi/Cargo.toml`
- Create `ferric-ffi/cbindgen.toml` with `language="C"`, `include_guard="FERRIC_H"`, `style="both"`, enum prefix settings
- Create `ferric-ffi/build.rs` that calls `cbindgen::Builder::new().with_crate(...).with_config(...).with_header(PREAMBLE).generate().write_to_file(...)

### Key cbindgen Configuration
- `FerricEngine` is NOT in `[export].include` — cbindgen auto-detects it as an opaque type (not `#[repr(C)]`) and emits `typedef struct FerricEngine FerricEngine;`
- Only `FerricError`, `FerricValueType`, `FerricValue` in `[export].include`
- `[enum] rename_variants = "ScreamingSnakeCase"` + `prefix_with_name = true` → produces `FERRIC_ERROR_OK`, `FERRIC_VALUE_TYPE_VOID` etc.
- `[parse] parse_deps = false` — no need to parse workspace dependencies
- `documentation_style = "c99"` → Rust `///` doc comments become `//` comments in C header

### cbindgen generates to source tree, not OUT_DIR
- `build.rs` writes `ferric.h` to `$CARGO_MANIFEST_DIR/ferric.h` (committed to version control)
- CI drift detection: `git diff --exit-code crates/ferric-ffi/ferric.h` after build
- Header smoke tests verify content: read with `std::fs::read_to_string(format!("{}/ferric.h", env!("CARGO_MANIFEST_DIR")))`
- The HEADER_PREAMBLE constant lives in `build.rs` (build scripts can't import from the crate they compile)

### Generated Header Details
- `float` field of `FerricValue` appears as `float_` in C (cbindgen adds underscore to avoid C keyword collision)
- `usize` → `uintptr_t` in the header
- `*mut c_char` → `char*`, `*const c_char` → `const char*`
- Test count: 105 ferric-ffi after pass 008 (12 new header smoke tests)

## Pass 011: CLI REPL Interactive Loop

### Files Modified
- `Cargo.toml` — added `rustyline = "17"` to `[workspace.dependencies]`
- `crates/ferric-cli/Cargo.toml` — added `rustyline = { workspace = true }`, `slotmap = { workspace = true }`
- `crates/ferric-cli/src/commands/repl.rs` — full REPL implementation
- `crates/ferric-cli/tests/cli_integration.rs` — 5 new REPL integration tests

### rustyline 17.x API
- `DefaultEditor` is a `pub type` alias: `Editor<(), DefaultHistory>`
- `ReadlineError::Eof` and `ReadlineError::Interrupted` are the clean-exit variants
- `editor.readline(prompt)` returns `Ok(String)` for non-tty piped stdin until EOF

### slotmap Dependency for FactId Display
- `FactId.data().as_ffi()` requires `slotmap::Key` trait in scope
- ferric-core and ferric-runtime don't re-export `slotmap`; must add `slotmap = { workspace = true }` directly to ferric-cli

### Fact Display Pattern
```rust
use slotmap::Key as _;
let id_num = id.data().as_ffi();
```

### ferric-core Fact Re-exports
- `ferric_core::Fact`, `ferric_core::Fact::Ordered`, `ferric_core::Fact::Template` — use directly
- `ferric_runtime` does NOT re-export `Fact`, `OrderedFact`, `TemplateFact`, or `FactId`

### Clippy `explicit_iter_loop`
- `for field in o.fields.iter()` → `for field in &o.fields` (SmallVec supports this)

### Multiline REPL Pattern
- Accumulate lines in a `String buffer`
- Call `parens_balanced(&buffer)` — `continue` if false, process if true
- `parens_balanced` tracks `in_comment` (resets on `\n`) and `in_string` (respects `\"`)

### Test Count
- ferric-cli: 10 unit tests + 21 integration tests after pass 011

## Pass 010: CLI `run`, `check`, and `version` commands

### Files Modified
- `crates/ferric-cli/src/commands/run.rs` — full implementation
- `crates/ferric-cli/src/commands/check.rs` — full implementation
- `crates/ferric-cli/tests/cli_integration.rs` — 8 new integration tests added
- `crates/ferric-runtime/src/loader.rs` — bug fix for module-qualified rule name ownership

### RunResult Field Name
The task spec said `RunResult.halt_requested: bool`, but actual field is `halt_reason: HaltReason`. Always verify at `crates/ferric-runtime/src/execution.rs`.

### CRITICAL: Module-Qualified Rule Name Bug Fix
**Bug**: `(defrule MAIN::start ...)` after `(defmodule REPORT ...)` was tagged as belonging to REPORT (not MAIN). After `reset()`, focus stack is `[MAIN]`, so these rules never fired.

**Root Cause**: `loader.rs` used `self.module_registry.current_module()` as owning module for all rules, ignoring `MODULE::` prefixes in rule names.

**Fix**: In the `Construct::Rule` arm of the collect loop, parse the rule name with `parse_qualified_name()`. If it has a module qualifier, use `module_registry.get_by_name(mod_name)` to resolve the actual owning module.

### CLI API Pattern
```rust
let mut engine = Engine::new(EngineConfig::default());
engine.load_file(file_path)?;         // Err(Vec<LoadError>)
engine.reset()?;                       // Err(EngineError)
let _result = engine.run(RunLimit::Unlimited)?; // Err(EngineError)
if let Some(out) = engine.get_output("t") { print!("{out}"); }
for diag in engine.action_diagnostics() { eprintln!("{diag}"); }
```

### Exit Code Contract
- 0: success (halt-requested is normal CLIPS termination)
- 1: file not found, load error, runtime error
- 2: usage error (missing argument)

### Test Count
- ferric-cli: 16 integration tests after pass 010
- workspace total: 1176 tests after pass 010

## Pass 009: Artifact Build Matrix and Panic Policy Verification

- **Clippy `ignore_without_reason`**: `#[ignore]` on tests must include a reason string: `#[ignore = "..."]`. Plain `#[ignore]` is a hard error under `-D warnings`.
- Build matrix tests invoke `cargo build -p ferric-ffi --profile <profile>` as subprocesses via `std::process::Command`. Use `#[ignore = "invokes cargo build as a subprocess; slow, intended for CI"]` on these.
- `workspace_root()` helper: `Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap().parent().unwrap()` (ferric-ffi is at `crates/ferric-ffi`, so two `.parent()` calls reach the workspace root).
- Artifact paths on macOS: `target/ffi-dev/libferric_ffi.dylib`, `target/ffi-dev/libferric_ffi.a`, same under `ffi-release/`.
- Unwind semantics verification test (non-ignored): use `std::panic::catch_unwind` to confirm the test profile does NOT have `panic = "abort"`.
- Test count: 110 ferric-ffi after pass 009 (1 new non-ignored + 4 new ignored tests)
