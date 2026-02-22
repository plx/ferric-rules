//! FFI engine APIs — lifecycle, execution, and fact operations.
//!
//! ## Thread Affinity Contract
//!
//! Every `ferric_engine_*` entry point validates that the calling thread
//! matches the thread that created the engine. This check happens BEFORE
//! any mutable borrow or state mutation.
//!
//! - Thread violations return `FERRIC_ERROR_THREAD_VIOLATION` with a descriptive
//!   message in the global error channel.
//!
//! The internal `unsafe fn move_to_current_thread` is deliberately NOT
//! exposed through the C API.

use std::ffi::CStr;
use std::os::raw::c_char;
use std::ptr;

use crate::types::{engine_config_from_ffi, value_to_ferric, FerricConfig, FerricValue};

use crate::error::{
    copy_error_to_buffer, set_engine_error_global, set_global_error, set_load_error_global,
    EngineErrorState, FerricError,
};
use ferric_runtime::engine::EngineError;
use ferric_runtime::{Engine, EngineConfig, RunLimit};

/// Opaque engine handle exposed to C.
///
/// Contains the Rust [`Engine`] plus per-engine error state.
/// C code receives `*mut FerricEngine` as an opaque pointer.
pub struct FerricEngine {
    pub(crate) engine: Engine,
    pub(crate) error_state: EngineErrorState,
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

/// Validate a non-null engine pointer, returning a shared reference.
///
/// Sets global error on null pointer. Used for read-only operations
/// and as the first step of the two-step borrow pattern.
unsafe fn validate_engine_ptr<'a>(
    engine: *const FerricEngine,
) -> Result<&'a FerricEngine, FerricError> {
    if engine.is_null() {
        set_global_error("engine pointer is null".to_string());
        return Err(FerricError::NullPointer);
    }
    Ok(&*engine)
}

/// Validate a non-null engine pointer, returning a mutable reference.
///
/// Sets global error on null pointer. Used after thread-affinity check passes.
unsafe fn validate_engine_ptr_mut<'a>(
    engine: *mut FerricEngine,
) -> Result<&'a mut FerricEngine, FerricError> {
    if engine.is_null() {
        set_global_error("engine pointer is null".to_string());
        return Err(FerricError::NullPointer);
    }
    Ok(&mut *engine)
}

/// Check thread affinity on the engine's inner runtime Engine.
///
/// This is the canonical "step 1" of the two-step borrow pattern:
/// obtain a shared reference, verify the thread, THEN proceed to mutable access.
fn check_thread_affinity(handle: &FerricEngine) -> Result<(), FerricError> {
    match handle.engine.check_thread_affinity() {
        Ok(()) => Ok(()),
        Err(ref err @ EngineError::WrongThread { .. }) => {
            set_global_error(err.to_string());
            Err(FerricError::ThreadViolation)
        }
        Err(ref err) => {
            set_global_error(err.to_string());
            Err(FerricError::InternalError)
        }
    }
}

// ---------------------------------------------------------------------------
// C API: Engine lifecycle
// ---------------------------------------------------------------------------

/// Create a new engine with default configuration.
///
/// Returns a heap-allocated engine handle, or null on failure.
/// The caller owns the returned handle and must free it with
/// `ferric_engine_free`.
///
/// # Safety
///
/// The returned pointer must be freed with `ferric_engine_free`.
/// The engine is bound to the creating thread.
#[no_mangle]
pub unsafe extern "C" fn ferric_engine_new() -> *mut FerricEngine {
    ferric_engine_new_with_config(ptr::null())
}

/// Create a new engine with optional caller-provided configuration.
///
/// If `config` is null, defaults are used.
///
/// # Safety
///
/// - `config` may be null.
/// - Returned pointer must be freed with `ferric_engine_free`.
#[no_mangle]
pub unsafe extern "C" fn ferric_engine_new_with_config(
    config: *const FerricConfig,
) -> *mut FerricEngine {
    let engine_config = if config.is_null() {
        EngineConfig::default()
    } else {
        engine_config_from_ffi(&*config)
    };

    let engine = Engine::new(engine_config);
    let handle = FerricEngine {
        engine,
        error_state: EngineErrorState::new(),
    };
    Box::into_raw(Box::new(handle))
}

/// Free an engine handle.
///
/// Null pointers are safely ignored. After this call, the pointer
/// is invalid and must not be used.
///
/// # Safety
///
/// - `engine` must be a pointer returned by `ferric_engine_new` or null.
/// - The engine must not be in use by another call when freed.
/// - The engine must be freed from the same thread that created it.
#[no_mangle]
pub unsafe extern "C" fn ferric_engine_free(engine: *mut FerricEngine) -> FerricError {
    if engine.is_null() {
        return FerricError::Ok;
    }
    let handle = &*engine;
    if let Err(code) = check_thread_affinity(handle) {
        return code;
    }
    drop(Box::from_raw(engine));
    FerricError::Ok
}

/// Load a CLIPS source string into the engine.
///
/// # Safety
///
/// - `engine` must be a valid engine pointer.
/// - `source` must be a valid NUL-terminated UTF-8 string.
#[no_mangle]
pub unsafe extern "C" fn ferric_engine_load_string(
    engine: *mut FerricEngine,
    source: *const c_char,
) -> FerricError {
    // Step 1: validate engine pointer (shared ref for thread check)
    let handle = match validate_engine_ptr(engine) {
        Ok(h) => h,
        Err(code) => return code,
    };
    if source.is_null() {
        set_global_error("source string pointer is null".to_string());
        return FerricError::NullPointer;
    }

    // Step 2: thread check (shared ref)
    if let Err(code) = check_thread_affinity(handle) {
        return code;
    }

    // Step 3: mutable access (safe after thread check)
    let handle = match validate_engine_ptr_mut(engine) {
        Ok(h) => h,
        Err(code) => return code,
    };

    let c_str = CStr::from_ptr(source);
    let source_str = match c_str.to_str() {
        Ok(s) => s,
        Err(e) => {
            set_global_error(format!("source string is not valid UTF-8: {e}"));
            return FerricError::InvalidArgument;
        }
    };

    match handle.engine.load_str(source_str) {
        Ok(_) => FerricError::Ok,
        Err(errors) => {
            if let Some(first) = errors.first() {
                set_load_error_global(first)
            } else {
                FerricError::InternalError
            }
        }
    }
}

/// Retrieve the last per-engine error message.
///
/// Returns a pointer to a NUL-terminated string, or null if no error
/// is stored. The pointer is valid until the next call on this engine.
///
/// # Safety
///
/// - `engine` must be a valid engine pointer or null.
#[no_mangle]
pub unsafe extern "C" fn ferric_engine_last_error(engine: *const FerricEngine) -> *const c_char {
    // Deliberately skip thread-affinity check: reading the error message
    // is a diagnostic operation that should always succeed.
    let Ok(handle) = validate_engine_ptr(engine) else {
        return ptr::null();
    };

    match handle.error_state.message() {
        Some(msg) => {
            thread_local! {
                static ENGINE_ERR_CSTRING: std::cell::RefCell<Option<std::ffi::CString>> =
                    const { std::cell::RefCell::new(None) };
            }
            let cstring = std::ffi::CString::new(msg).unwrap_or_default();
            ENGINE_ERR_CSTRING.with(|c| {
                *c.borrow_mut() = Some(cstring);
                c.borrow().as_ref().map_or(ptr::null(), |cs| cs.as_ptr())
            })
        }
        None => ptr::null(),
    }
}

/// Copy the per-engine error message into a caller-provided buffer.
///
/// Same contract as `ferric_last_error_global_copy` but reads from the
/// per-engine error channel. Deliberately skips thread-affinity check
/// (diagnostic operation).
///
/// ## Contract
///
/// | Condition | Return | `*out_len` |
/// |-----------|--------|------------|
/// | `engine` is null | `NullPointer` | 0 |
/// | No error stored | `NotFound` | 0 |
/// | `out_len` is null | `InvalidArgument` | (not written) |
/// | `buf` is null AND `buf_len` is 0 (size query) | `Ok` | required size (incl. NUL) |
/// | `buf` non-null, `buf_len` >= needed | `Ok` | bytes written (incl. NUL) |
/// | `buf` non-null, `buf_len` < needed | `BufferTooSmall` | full needed size (incl. NUL) |
///
/// # Safety
///
/// - `engine` must be a valid engine pointer or null (null → `NullPointer`).
/// - `buf` must point to `buf_len` writable bytes, or be null for size query.
/// - `out_len` must be a valid pointer (non-null).
#[no_mangle]
pub unsafe extern "C" fn ferric_engine_last_error_copy(
    engine: *const FerricEngine,
    buf: *mut c_char,
    buf_len: usize,
    out_len: *mut usize,
) -> FerricError {
    if out_len.is_null() {
        return FerricError::InvalidArgument;
    }
    let handle = match validate_engine_ptr(engine) {
        Ok(h) => h,
        Err(code) => {
            *out_len = 0;
            return code;
        }
    };
    copy_error_to_buffer(handle.error_state.message(), buf, buf_len, out_len)
}

/// Clear the per-engine error state.
///
/// # Safety
///
/// - `engine` must be a valid engine pointer or null (null returns `NullPointer`).
#[no_mangle]
pub unsafe extern "C" fn ferric_engine_clear_error(engine: *mut FerricEngine) -> FerricError {
    let handle = match validate_engine_ptr(engine) {
        Ok(h) => h,
        Err(code) => return code,
    };
    if let Err(code) = check_thread_affinity(handle) {
        return code;
    }
    let handle = match validate_engine_ptr_mut(engine) {
        Ok(h) => h,
        Err(code) => return code,
    };
    handle.error_state.clear();
    FerricError::Ok
}

/// Reset the engine to its initial state.
///
/// # Safety
///
/// - `engine` must be a valid engine pointer.
#[no_mangle]
pub unsafe extern "C" fn ferric_engine_reset(engine: *mut FerricEngine) -> FerricError {
    let handle = match validate_engine_ptr(engine) {
        Ok(h) => h,
        Err(code) => return code,
    };
    if let Err(code) = check_thread_affinity(handle) {
        return code;
    }
    let handle = match validate_engine_ptr_mut(engine) {
        Ok(h) => h,
        Err(code) => return code,
    };

    match handle.engine.reset() {
        Ok(()) => FerricError::Ok,
        Err(ref err) => set_engine_error_global(err),
    }
}

// ---------------------------------------------------------------------------
// C API: Execution and fact mutation
// ---------------------------------------------------------------------------

/// Run the engine, executing rules until the agenda is empty, the limit is
/// reached, or a halt action fires.
///
/// - `limit`: Maximum rule firings. Pass `-1` for unlimited.
/// - `out_fired`: If non-null, receives the number of rules fired.
///
/// Returns `FerricError::Ok` on success.
///
/// # Safety
///
/// - `engine` must be a valid engine pointer.
/// - `out_fired` may be null (output is simply not written).
#[no_mangle]
pub unsafe extern "C" fn ferric_engine_run(
    engine: *mut FerricEngine,
    limit: i64,
    out_fired: *mut u64,
) -> FerricError {
    let handle = match validate_engine_ptr(engine) {
        Ok(h) => h,
        Err(code) => return code,
    };
    if let Err(code) = check_thread_affinity(handle) {
        return code;
    }
    let handle = match validate_engine_ptr_mut(engine) {
        Ok(h) => h,
        Err(code) => return code,
    };

    let run_limit = if limit < 0 {
        RunLimit::Unlimited
    } else {
        #[allow(clippy::cast_sign_loss, clippy::cast_possible_truncation)]
        RunLimit::Count(limit as usize)
    };

    match handle.engine.run(run_limit) {
        Ok(result) => {
            if !out_fired.is_null() {
                *out_fired = result.rules_fired as u64;
            }
            FerricError::Ok
        }
        Err(ref err) => set_engine_error_global(err),
    }
}

/// Execute a single rule firing step.
///
/// - `out_status`: If non-null, receives: `1` = rule fired, `0` = agenda empty,
///   `-1` = halted.
///
/// Returns `FerricError::Ok` on success.
///
/// # Safety
///
/// - `engine` must be a valid engine pointer.
/// - `out_status` may be null.
#[no_mangle]
pub unsafe extern "C" fn ferric_engine_step(
    engine: *mut FerricEngine,
    out_status: *mut i32,
) -> FerricError {
    let handle = match validate_engine_ptr(engine) {
        Ok(h) => h,
        Err(code) => return code,
    };
    if let Err(code) = check_thread_affinity(handle) {
        return code;
    }
    let handle = match validate_engine_ptr_mut(engine) {
        Ok(h) => h,
        Err(code) => return code,
    };

    match handle.engine.step() {
        Ok(Some(_fired)) => {
            if !out_status.is_null() {
                *out_status = 1;
            }
            FerricError::Ok
        }
        Ok(None) => {
            if !out_status.is_null() {
                if handle.engine.is_halted() {
                    *out_status = -1;
                } else {
                    *out_status = 0;
                }
            }
            FerricError::Ok
        }
        Err(ref err) => set_engine_error_global(err),
    }
}

/// Assert a fact from a CLIPS source string (e.g., `"(assert (color red))"`).
///
/// The source is parsed as a top-level CLIPS form and evaluated. If
/// `out_fact_id` is non-null and an assert occurred, it receives the first
/// asserted fact's opaque ID. If no fact was asserted, `0` is written.
///
/// # Safety
///
/// - `engine` must be a valid engine pointer.
/// - `source` must be a valid NUL-terminated UTF-8 string.
/// - `out_fact_id` may be null.
#[no_mangle]
pub unsafe extern "C" fn ferric_engine_assert_string(
    engine: *mut FerricEngine,
    source: *const c_char,
    out_fact_id: *mut u64,
) -> FerricError {
    use slotmap::Key as _;

    let handle = match validate_engine_ptr(engine) {
        Ok(h) => h,
        Err(code) => return code,
    };
    if source.is_null() {
        set_global_error("source string pointer is null".to_string());
        return FerricError::NullPointer;
    }
    if let Err(code) = check_thread_affinity(handle) {
        return code;
    }
    let handle = match validate_engine_ptr_mut(engine) {
        Ok(h) => h,
        Err(code) => return code,
    };

    let c_str = CStr::from_ptr(source);
    let source_str = match c_str.to_str() {
        Ok(s) => s,
        Err(e) => {
            set_global_error(format!("source string is not valid UTF-8: {e}"));
            return FerricError::InvalidArgument;
        }
    };

    match handle.engine.load_str(source_str) {
        Ok(load_result) => {
            if !out_fact_id.is_null() {
                *out_fact_id = load_result
                    .asserted_facts
                    .first()
                    .map_or(0, |fid| fid.data().as_ffi());
            }
            FerricError::Ok
        }
        Err(errors) => {
            if let Some(first) = errors.first() {
                set_load_error_global(first)
            } else {
                FerricError::InternalError
            }
        }
    }
}

/// Retract a fact by its opaque fact ID obtained from a previous assert.
///
/// # Safety
///
/// - `engine` must be a valid engine pointer.
/// - `fact_id` must be a valid fact ID obtained from a previous assert.
#[no_mangle]
pub unsafe extern "C" fn ferric_engine_retract(
    engine: *mut FerricEngine,
    fact_id: u64,
) -> FerricError {
    let handle = match validate_engine_ptr(engine) {
        Ok(h) => h,
        Err(code) => return code,
    };
    if let Err(code) = check_thread_affinity(handle) {
        return code;
    }
    let handle = match validate_engine_ptr_mut(engine) {
        Ok(h) => h,
        Err(code) => return code,
    };

    let key_data = slotmap::KeyData::from_ffi(fact_id);
    let fid = ferric_core::FactId::from(key_data);

    match handle.engine.retract(fid) {
        Ok(()) => FerricError::Ok,
        Err(ref err) => set_engine_error_global(err),
    }
}

/// Get the engine's captured output for a named channel (e.g., `"stdout"`).
///
/// Returns a pointer to a NUL-terminated string, or null if the channel has
/// no output, the engine pointer is null, or the channel pointer is null.
/// The returned pointer is valid until the next call that writes to that channel.
///
/// # Safety
///
/// - `engine` must be a valid engine pointer or null.
/// - `channel` must be a valid NUL-terminated UTF-8 string or null.
#[no_mangle]
pub unsafe extern "C" fn ferric_engine_get_output(
    engine: *const FerricEngine,
    channel: *const c_char,
) -> *const c_char {
    let Ok(handle) = validate_engine_ptr(engine) else {
        return ptr::null();
    };
    if channel.is_null() {
        return ptr::null();
    }

    let c_str = CStr::from_ptr(channel);
    let Ok(channel_str) = c_str.to_str() else {
        return ptr::null();
    };

    match handle.engine.get_output(channel_str) {
        Some(output) if !output.is_empty() => {
            thread_local! {
                static OUTPUT_CSTRING: std::cell::RefCell<Option<std::ffi::CString>> =
                    const { std::cell::RefCell::new(None) };
            }
            let cstring = std::ffi::CString::new(output).unwrap_or_default();
            OUTPUT_CSTRING.with(|c| {
                *c.borrow_mut() = Some(cstring);
                c.borrow().as_ref().map_or(ptr::null(), |cs| cs.as_ptr())
            })
        }
        _ => ptr::null(),
    }
}

/// Get the number of action diagnostics captured during recent execution.
///
/// Diagnostics are collected by `run`/`step` when non-fatal action errors occur
/// (for example module visibility failures surfaced as warnings).
///
/// # Safety
///
/// - `engine` must be a valid engine pointer.
/// - `out_count` must be a valid pointer.
#[no_mangle]
pub unsafe extern "C" fn ferric_engine_action_diagnostic_count(
    engine: *const FerricEngine,
    out_count: *mut usize,
) -> FerricError {
    let handle = match validate_engine_ptr(engine) {
        Ok(h) => h,
        Err(code) => return code,
    };
    if out_count.is_null() {
        set_global_error("out_count pointer is null".to_string());
        return FerricError::NullPointer;
    }
    if let Err(code) = check_thread_affinity(handle) {
        return code;
    }

    *out_count = handle.engine.action_diagnostics().len();
    FerricError::Ok
}

/// Copy one action diagnostic message into a caller-provided buffer.
///
/// Message selection is by zero-based index into the current action-diagnostic list.
/// The copy contract matches `ferric_last_error_global_copy`.
///
/// # Safety
///
/// - `engine` must be a valid engine pointer.
/// - `buf` must point to `buf_len` writable bytes, or be null for size query.
/// - `out_len` must be a valid pointer (non-null).
#[no_mangle]
pub unsafe extern "C" fn ferric_engine_action_diagnostic_copy(
    engine: *const FerricEngine,
    index: usize,
    buf: *mut c_char,
    buf_len: usize,
    out_len: *mut usize,
) -> FerricError {
    if out_len.is_null() {
        return FerricError::InvalidArgument;
    }

    let handle = match validate_engine_ptr(engine) {
        Ok(h) => h,
        Err(code) => {
            *out_len = 0;
            return code;
        }
    };
    if let Err(code) = check_thread_affinity(handle) {
        *out_len = 0;
        return code;
    }

    let message = handle
        .engine
        .action_diagnostics()
        .get(index)
        .map(ToString::to_string);
    copy_error_to_buffer(message.as_deref(), buf, buf_len, out_len)
}

/// Clear all stored action diagnostics.
///
/// # Safety
///
/// - `engine` must be a valid engine pointer or null (null returns `NullPointer`).
#[no_mangle]
pub unsafe extern "C" fn ferric_engine_clear_action_diagnostics(
    engine: *mut FerricEngine,
) -> FerricError {
    let handle = match validate_engine_ptr(engine) {
        Ok(h) => h,
        Err(code) => return code,
    };
    if let Err(code) = check_thread_affinity(handle) {
        return code;
    }
    let handle = match validate_engine_ptr_mut(engine) {
        Ok(h) => h,
        Err(code) => return code,
    };
    handle.engine.clear_action_diagnostics();
    FerricError::Ok
}

// ---------------------------------------------------------------------------
// C API: Fact and value queries
// ---------------------------------------------------------------------------

/// Get the count of user-visible facts in working memory.
///
/// The synthetic `(initial-fact)` is excluded from the count.
///
/// # Safety
///
/// - `engine` must be a valid engine pointer.
/// - `out_count` must be a valid pointer.
#[no_mangle]
pub unsafe extern "C" fn ferric_engine_fact_count(
    engine: *const FerricEngine,
    out_count: *mut usize,
) -> FerricError {
    let handle = match validate_engine_ptr(engine) {
        Ok(h) => h,
        Err(code) => return code,
    };
    if out_count.is_null() {
        set_global_error("out_count pointer is null".to_string());
        return FerricError::NullPointer;
    }
    // facts() does its own thread check
    match handle.engine.facts() {
        Ok(iter) => {
            *out_count = iter.count();
            FerricError::Ok
        }
        Err(ref err) => set_engine_error_global(err),
    }
}

/// Get the number of fields in a fact.
///
/// For ordered facts, returns the number of field values.
/// For template facts, returns the number of slots.
///
/// # Safety
///
/// - `engine` must be a valid engine pointer.
/// - `out_count` must be a valid pointer.
#[no_mangle]
pub unsafe extern "C" fn ferric_engine_get_fact_field_count(
    engine: *const FerricEngine,
    fact_id: u64,
    out_count: *mut usize,
) -> FerricError {
    let handle = match validate_engine_ptr(engine) {
        Ok(h) => h,
        Err(code) => return code,
    };
    if out_count.is_null() {
        set_global_error("out_count pointer is null".to_string());
        return FerricError::NullPointer;
    }

    let key_data = slotmap::KeyData::from_ffi(fact_id);
    let fid = ferric_core::FactId::from(key_data);

    match handle.engine.get_fact(fid) {
        Ok(Some(fact)) => {
            use ferric_core::fact::Fact;
            *out_count = match fact {
                Fact::Ordered(o) => o.fields.len(),
                Fact::Template(t) => t.slots.len(),
            };
            FerricError::Ok
        }
        Ok(None) => {
            set_global_error(format!("fact not found: {fact_id}"));
            FerricError::NotFound
        }
        Err(ref err) => set_engine_error_global(err),
    }
}

/// Get a single field from a fact as a `FerricValue`.
///
/// For ordered facts, `index` is the field position (0-based).
/// For template facts, `index` is the slot position (0-based).
///
/// The returned `FerricValue` is written to `*out_value`. The caller owns
/// any heap-allocated resources (`string_ptr`, `multifield_ptr`) and must free
/// them with `ferric_value_free` or the type-specific free functions.
///
/// # Safety
///
/// - `engine` must be a valid engine pointer.
/// - `out_value` must be a valid pointer to a `FerricValue`.
#[no_mangle]
pub unsafe extern "C" fn ferric_engine_get_fact_field(
    engine: *const FerricEngine,
    fact_id: u64,
    index: usize,
    out_value: *mut FerricValue,
) -> FerricError {
    let handle = match validate_engine_ptr(engine) {
        Ok(h) => h,
        Err(code) => return code,
    };
    if out_value.is_null() {
        set_global_error("out_value pointer is null".to_string());
        return FerricError::NullPointer;
    }

    let key_data = slotmap::KeyData::from_ffi(fact_id);
    let fid = ferric_core::FactId::from(key_data);

    match handle.engine.get_fact(fid) {
        Ok(Some(fact)) => {
            use ferric_core::fact::Fact;
            let field_value = match fact {
                Fact::Ordered(o) => o.fields.get(index),
                Fact::Template(t) => t.slots.get(index),
            };
            if let Some(val) = field_value {
                *out_value = value_to_ferric(val, &handle.engine);
                FerricError::Ok
            } else {
                set_global_error(format!("field index {index} out of bounds"));
                FerricError::InvalidArgument
            }
        }
        Ok(None) => {
            set_global_error(format!("fact not found: {fact_id}"));
            FerricError::NotFound
        }
        Err(ref err) => set_engine_error_global(err),
    }
}

/// Get a global variable's value.
///
/// The name should NOT include the `?*` prefix/suffix — pass just the base name
/// (e.g., `"x"` for `?*x*`).
///
/// Module/global visibility resolution follows the runtime's standard rules.
/// Ambiguity and not-found conditions produce runtime-authored diagnostics.
///
/// # Safety
///
/// - `engine` must be a valid engine pointer.
/// - `name` must be a valid NUL-terminated UTF-8 string.
/// - `out_value` must be a valid pointer to a `FerricValue`.
#[no_mangle]
pub unsafe extern "C" fn ferric_engine_get_global(
    engine: *const FerricEngine,
    name: *const c_char,
    out_value: *mut FerricValue,
) -> FerricError {
    let handle = match validate_engine_ptr(engine) {
        Ok(h) => h,
        Err(code) => return code,
    };
    if name.is_null() {
        set_global_error("name pointer is null".to_string());
        return FerricError::NullPointer;
    }
    if out_value.is_null() {
        set_global_error("out_value pointer is null".to_string());
        return FerricError::NullPointer;
    }
    if let Err(code) = check_thread_affinity(handle) {
        return code;
    }

    let c_str = CStr::from_ptr(name);
    let name_str = match c_str.to_str() {
        Ok(s) => s,
        Err(e) => {
            set_global_error(format!("name is not valid UTF-8: {e}"));
            return FerricError::InvalidArgument;
        }
    };

    if let Some(val) = handle.engine.get_global(name_str) {
        *out_value = value_to_ferric(val, &handle.engine);
        FerricError::Ok
    } else {
        set_global_error(format!("global variable not found: {name_str}"));
        FerricError::NotFound
    }
}
