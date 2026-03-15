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

use std::cell::RefCell;
use std::collections::HashMap;
use std::ffi::{CStr, CString};
use std::os::raw::c_char;
use std::ptr;

use crate::types::{
    engine_config_from_ffi, ferric_to_value, value_to_ferric, FerricConfig, FerricFactType,
    FerricHaltReason, FerricValue,
};

use crate::error::{
    copy_error_to_buffer, map_engine_error, map_load_error, set_engine_error_global,
    set_global_error, EngineErrorState, FerricError,
};
use ferric_runtime::engine::EngineError;
use ferric_runtime::loader::LoadError;
use ferric_runtime::{Engine, EngineConfig, InitError, RunLimit};

/// Opaque engine handle exposed to C.
///
/// Contains the Rust [`Engine`] plus per-engine error state.
/// C code receives `*mut FerricEngine` as an opaque pointer.
pub struct FerricEngine {
    pub(crate) engine: Engine,
    pub(crate) error_state: EngineErrorState,
    pub(crate) error_cstring: RefCell<Option<CString>>,
}

thread_local! {
    static OUTPUT_CSTRINGS: RefCell<HashMap<String, CachedOutputCString>> =
        RefCell::new(HashMap::new());
}

struct CachedOutputCString {
    snapshot: String,
    cstring: CString,
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

unsafe fn borrow_engine_mut<'a>(
    engine: *mut FerricEngine,
) -> Result<&'a mut FerricEngine, FerricError> {
    let handle = validate_engine_ptr(engine)?;
    check_thread_affinity(handle)?;
    validate_engine_ptr_mut(engine)
}

unsafe fn borrow_engine_checked<'a>(
    engine: *const FerricEngine,
) -> Result<&'a FerricEngine, FerricError> {
    let handle = validate_engine_ptr(engine)?;
    check_thread_affinity(handle)?;
    Ok(handle)
}

unsafe fn c_str_to_str<'a>(ptr: *const c_char, label: &str) -> Result<&'a str, FerricError> {
    if ptr.is_null() {
        set_global_error(format!("{label} pointer is null"));
        return Err(FerricError::NullPointer);
    }
    let c_str = CStr::from_ptr(ptr);
    c_str.to_str().map_err(|e| {
        set_global_error(format!("{label} is not valid UTF-8: {e}"));
        FerricError::InvalidArgument
    })
}

fn set_engine_error_message(
    handle: &mut FerricEngine,
    code: FerricError,
    message: String,
) -> FerricError {
    handle.error_state.set(message.clone());
    set_global_error(message);
    code
}

fn set_engine_runtime_error(handle: &mut FerricEngine, err: &EngineError) -> FerricError {
    set_engine_error_message(handle, map_engine_error(err), err.to_string())
}

fn set_engine_load_error(handle: &mut FerricEngine, err: &LoadError) -> FerricError {
    set_engine_error_message(handle, map_load_error(err), err.to_string())
}

/// Copy a string to a caller-provided buffer using the standard buffer copy pattern.
///
/// - Size query: `buf` is null AND `buf_len` is 0 → writes needed size to `*out_len`, returns `Ok`.
/// - Full copy: `buf` is non-null, `buf_len` >= needed → copies string + NUL, returns `Ok`.
/// - Undersized: `buf` is non-null, `buf_len` < needed → truncated copy + NUL, returns `BufferTooSmall`.
///
/// # Safety
///
/// - `out_len` must be a valid, non-null pointer.
/// - If `buf` is non-null, it must point to at least `buf_len` writable bytes.
unsafe fn copy_str_to_buffer(
    s: &str,
    buf: *mut c_char,
    buf_len: usize,
    out_len: *mut usize,
) -> FerricError {
    let needed = s.len() + 1; // string bytes + NUL

    if buf.is_null() {
        *out_len = needed;
        return if buf_len == 0 {
            FerricError::Ok
        } else {
            set_global_error("non-zero buf_len with null buf".to_string());
            FerricError::InvalidArgument
        };
    }

    if buf_len == 0 {
        *out_len = needed;
        return FerricError::BufferTooSmall;
    }

    if buf_len >= needed {
        std::ptr::copy_nonoverlapping(s.as_ptr(), buf.cast::<u8>(), s.len());
        *buf.add(s.len()) = 0;
        *out_len = needed;
        FerricError::Ok
    } else {
        let copy_len = buf_len - 1;
        std::ptr::copy_nonoverlapping(s.as_ptr(), buf.cast::<u8>(), copy_len);
        *buf.add(copy_len) = 0;
        *out_len = needed;
        FerricError::BufferTooSmall
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
        match engine_config_from_ffi(&*config) {
            Ok(cfg) => cfg,
            Err(message) => {
                set_global_error(message);
                return ptr::null_mut();
            }
        }
    };

    let engine = Engine::new(engine_config);
    let handle = FerricEngine {
        engine,
        error_state: EngineErrorState::new(),
        error_cstring: RefCell::new(None),
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
    if let Err(code) = validate_engine_ptr(engine) {
        return code;
    }
    let source_str = match c_str_to_str(source, "source string") {
        Ok(s) => s,
        Err(code) => return code,
    };
    let handle = match borrow_engine_mut(engine) {
        Ok(h) => h,
        Err(code) => return code,
    };

    match handle.engine.load_str(source_str) {
        Ok(_) => FerricError::Ok,
        Err(errors) => {
            if let Some(first) = errors.first() {
                set_engine_load_error(handle, first)
            } else {
                set_engine_error_message(
                    handle,
                    FerricError::InternalError,
                    "internal error: load failed without diagnostics".to_string(),
                )
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
            let cstring = CString::new(msg).unwrap_or_default();
            let mut slot = handle.error_cstring.borrow_mut();
            *slot = Some(cstring);
            slot.as_ref().map_or(ptr::null(), |cs| cs.as_ptr())
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
    let handle = match borrow_engine_mut(engine) {
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
    let handle = match borrow_engine_mut(engine) {
        Ok(h) => h,
        Err(code) => return code,
    };

    match handle.engine.reset() {
        Ok(()) => FerricError::Ok,
        Err(ref err) => set_engine_runtime_error(handle, err),
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
    let handle = match borrow_engine_mut(engine) {
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
        Err(ref err) => set_engine_runtime_error(handle, err),
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
    let handle = match borrow_engine_mut(engine) {
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
        Err(ref err) => set_engine_runtime_error(handle, err),
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

    if let Err(code) = validate_engine_ptr(engine) {
        return code;
    }
    let source_str = match c_str_to_str(source, "source string") {
        Ok(s) => s,
        Err(code) => return code,
    };
    let handle = match borrow_engine_mut(engine) {
        Ok(h) => h,
        Err(code) => return code,
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
                set_engine_load_error(handle, first)
            } else {
                set_engine_error_message(
                    handle,
                    FerricError::InternalError,
                    "internal error: load failed without diagnostics".to_string(),
                )
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
    let handle = match borrow_engine_mut(engine) {
        Ok(h) => h,
        Err(code) => return code,
    };

    let key_data = slotmap::KeyData::from_ffi(fact_id);
    let fid = ferric_core::FactId::from(key_data);

    match handle.engine.retract(fid) {
        Ok(()) => FerricError::Ok,
        Err(ref err) => set_engine_runtime_error(handle, err),
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
    let Ok(handle) = borrow_engine_checked(engine) else {
        return ptr::null();
    };
    if channel.is_null() {
        return ptr::null();
    }

    let Ok(channel_str) = CStr::from_ptr(channel).to_str() else {
        return ptr::null();
    };

    match handle.engine.get_output(channel_str) {
        Some(output) if !output.is_empty() => OUTPUT_CSTRINGS.with(|cache| {
            use std::collections::hash_map::Entry;

            let mut cache = cache.borrow_mut();
            match cache.entry(channel_str.to_string()) {
                Entry::Occupied(mut entry) => {
                    if entry.get().snapshot != output {
                        entry.insert(CachedOutputCString {
                            snapshot: output.to_string(),
                            cstring: CString::new(output).unwrap_or_default(),
                        });
                    }
                    entry.get().cstring.as_ptr()
                }
                Entry::Vacant(entry) => {
                    let slot = entry.insert(CachedOutputCString {
                        snapshot: output.to_string(),
                        cstring: CString::new(output).unwrap_or_default(),
                    });
                    slot.cstring.as_ptr()
                }
            }
        }),
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
    let handle = match borrow_engine_checked(engine) {
        Ok(h) => h,
        Err(code) => return code,
    };
    if out_count.is_null() {
        set_global_error("out_count pointer is null".to_string());
        return FerricError::NullPointer;
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

    let handle = match borrow_engine_checked(engine) {
        Ok(h) => h,
        Err(code) => {
            *out_len = 0;
            return code;
        }
    };

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
    let handle = match borrow_engine_mut(engine) {
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
    if let Err(code) = validate_engine_ptr(engine) {
        return code;
    }
    let name_str = match c_str_to_str(name, "name") {
        Ok(s) => s,
        Err(code) => return code,
    };
    if out_value.is_null() {
        set_global_error("out_value pointer is null".to_string());
        return FerricError::NullPointer;
    }
    let handle = match borrow_engine_checked(engine) {
        Ok(h) => h,
        Err(code) => return code,
    };

    if let Some(val) = handle.engine.get_global(name_str) {
        *out_value = value_to_ferric(val, &handle.engine);
        FerricError::Ok
    } else {
        set_global_error(format!("global variable not found: {name_str}"));
        FerricError::NotFound
    }
}

// ---------------------------------------------------------------------------
// C API: Fact iteration
// ---------------------------------------------------------------------------

/// Copy all user-visible fact IDs to a caller-provided array.
///
/// - Size query: `out_ids == NULL && max_ids == 0` → `*out_count` receives total count.
/// - Partial copy: copies up to `max_ids` IDs, `*out_count` always receives total count.
///
/// # Safety
///
/// - `engine` must be a valid engine pointer.
/// - `out_count` must be a valid pointer.
/// - If `out_ids` is non-null, it must point to space for at least `max_ids` `u64`s.
#[no_mangle]
pub unsafe extern "C" fn ferric_engine_fact_ids(
    engine: *const FerricEngine,
    out_ids: *mut u64,
    max_ids: usize,
    out_count: *mut usize,
) -> FerricError {
    use slotmap::Key as _;

    let handle = match borrow_engine_checked(engine) {
        Ok(h) => h,
        Err(code) => return code,
    };
    if out_count.is_null() {
        set_global_error("out_count pointer is null".to_string());
        return FerricError::NullPointer;
    }

    match handle.engine.facts() {
        Ok(iter) => {
            let ids: Vec<u64> = iter.map(|(fid, _)| fid.data().as_ffi()).collect();
            *out_count = ids.len();
            if !out_ids.is_null() {
                let copy_count = ids.len().min(max_ids);
                for (i, &id) in ids.iter().enumerate().take(copy_count) {
                    *out_ids.add(i) = id;
                }
            }
            FerricError::Ok
        }
        Err(ref err) => set_engine_error_global(err),
    }
}

/// Find fact IDs by relation name.
///
/// Same size-query pattern as `ferric_engine_fact_ids`.
///
/// # Safety
///
/// - `engine` must be a valid engine pointer.
/// - `relation` must be a valid NUL-terminated string.
/// - `out_count` must be a valid pointer.
/// - If `out_ids` is non-null, it must point to space for at least `max_ids` `u64`s.
#[no_mangle]
pub unsafe extern "C" fn ferric_engine_find_fact_ids(
    engine: *const FerricEngine,
    relation: *const c_char,
    out_ids: *mut u64,
    max_ids: usize,
    out_count: *mut usize,
) -> FerricError {
    use slotmap::Key as _;

    let handle = match borrow_engine_checked(engine) {
        Ok(h) => h,
        Err(code) => return code,
    };
    let relation_str = match c_str_to_str(relation, "relation") {
        Ok(s) => s,
        Err(code) => return code,
    };
    if out_count.is_null() {
        set_global_error("out_count pointer is null".to_string());
        return FerricError::NullPointer;
    }

    match handle.engine.find_facts(relation_str) {
        Ok(facts) => {
            let ids: Vec<u64> = facts.iter().map(|(fid, _)| fid.data().as_ffi()).collect();
            *out_count = ids.len();
            if !out_ids.is_null() {
                let copy_count = ids.len().min(max_ids);
                for (i, &id) in ids.iter().enumerate().take(copy_count) {
                    *out_ids.add(i) = id;
                }
            }
            FerricError::Ok
        }
        Err(ref err) => set_engine_error_global(err),
    }
}

// ---------------------------------------------------------------------------
// C API: Fact type and names
// ---------------------------------------------------------------------------

/// Discriminate ordered vs. template fact.
///
/// # Safety
///
/// - `engine` must be a valid engine pointer.
/// - `out_type` must be a valid pointer.
#[no_mangle]
pub unsafe extern "C" fn ferric_engine_get_fact_type(
    engine: *const FerricEngine,
    fact_id: u64,
    out_type: *mut FerricFactType,
) -> FerricError {
    let handle = match borrow_engine_checked(engine) {
        Ok(h) => h,
        Err(code) => return code,
    };
    if out_type.is_null() {
        set_global_error("out_type pointer is null".to_string());
        return FerricError::NullPointer;
    }

    let key_data = slotmap::KeyData::from_ffi(fact_id);
    let fid = ferric_core::FactId::from(key_data);

    match handle.engine.get_fact(fid) {
        Ok(Some(fact)) => {
            use ferric_core::fact::Fact;
            *out_type = match fact {
                Fact::Ordered(_) => FerricFactType::Ordered,
                Fact::Template(_) => FerricFactType::Template,
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

/// Get the relation name for an ordered fact.
///
/// Standard buffer copy pattern.
///
/// # Safety
///
/// - `engine` must be a valid engine pointer.
/// - `out_len` must be a valid pointer.
/// - If `buf` is non-null, it must point to at least `buf_len` writable bytes.
#[no_mangle]
pub unsafe extern "C" fn ferric_engine_get_fact_relation(
    engine: *const FerricEngine,
    fact_id: u64,
    buf: *mut c_char,
    buf_len: usize,
    out_len: *mut usize,
) -> FerricError {
    let handle = match borrow_engine_checked(engine) {
        Ok(h) => h,
        Err(code) => return code,
    };
    if out_len.is_null() {
        set_global_error("out_len pointer is null".to_string());
        return FerricError::NullPointer;
    }

    let key_data = slotmap::KeyData::from_ffi(fact_id);
    let fid = ferric_core::FactId::from(key_data);

    match handle.engine.get_fact(fid) {
        Ok(Some(fact)) => {
            use ferric_core::fact::Fact;
            match fact {
                Fact::Ordered(o) => {
                    let name = handle
                        .engine
                        .resolve_symbol(o.relation)
                        .unwrap_or("<unknown>");
                    copy_str_to_buffer(name, buf, buf_len, out_len)
                }
                Fact::Template(_) => {
                    set_global_error("fact is a template fact, not an ordered fact".to_string());
                    FerricError::InvalidArgument
                }
            }
        }
        Ok(None) => {
            set_global_error(format!("fact not found: {fact_id}"));
            FerricError::NotFound
        }
        Err(ref err) => set_engine_error_global(err),
    }
}

/// Get the template name for a template fact.
///
/// Standard buffer copy pattern.
///
/// # Safety
///
/// - `engine` must be a valid engine pointer.
/// - `out_len` must be a valid pointer.
/// - If `buf` is non-null, it must point to at least `buf_len` writable bytes.
#[no_mangle]
pub unsafe extern "C" fn ferric_engine_get_fact_template_name(
    engine: *const FerricEngine,
    fact_id: u64,
    buf: *mut c_char,
    buf_len: usize,
    out_len: *mut usize,
) -> FerricError {
    let handle = match borrow_engine_checked(engine) {
        Ok(h) => h,
        Err(code) => return code,
    };
    if out_len.is_null() {
        set_global_error("out_len pointer is null".to_string());
        return FerricError::NullPointer;
    }

    let key_data = slotmap::KeyData::from_ffi(fact_id);
    let fid = ferric_core::FactId::from(key_data);

    match handle.engine.get_fact(fid) {
        Ok(Some(fact)) => {
            use ferric_core::fact::Fact;
            match fact {
                Fact::Template(t) => {
                    let name = handle
                        .engine
                        .template_name_by_id(t.template_id)
                        .unwrap_or("<unknown>");
                    copy_str_to_buffer(name, buf, buf_len, out_len)
                }
                Fact::Ordered(_) => {
                    set_global_error("fact is an ordered fact, not a template fact".to_string());
                    FerricError::InvalidArgument
                }
            }
        }
        Ok(None) => {
            set_global_error(format!("fact not found: {fact_id}"));
            FerricError::NotFound
        }
        Err(ref err) => set_engine_error_global(err),
    }
}

// ---------------------------------------------------------------------------
// C API: Structured assertion
// ---------------------------------------------------------------------------

/// Assert an ordered fact from structured values, bypassing CLIPS source parsing.
///
/// # Safety
///
/// - `engine` must be a valid engine pointer.
/// - `relation` must be a valid NUL-terminated string.
/// - If `fields` is non-null, it must point to `field_count` valid `FerricValue`s.
/// - `out_fact_id` may be null.
#[no_mangle]
pub unsafe extern "C" fn ferric_engine_assert_ordered(
    engine: *mut FerricEngine,
    relation: *const c_char,
    fields: *const FerricValue,
    field_count: usize,
    out_fact_id: *mut u64,
) -> FerricError {
    use slotmap::Key as _;

    let relation_str = match c_str_to_str(relation, "relation") {
        Ok(s) => s,
        Err(code) => return code,
    };
    if fields.is_null() && field_count > 0 {
        set_global_error("fields pointer is null with non-zero field_count".to_string());
        return FerricError::NullPointer;
    }

    let handle = match borrow_engine_mut(engine) {
        Ok(h) => h,
        Err(code) => return code,
    };

    // Convert FerricValue array to Vec<Value>
    let mut values = Vec::with_capacity(field_count);
    for i in 0..field_count {
        let fv = &*fields.add(i);
        match ferric_to_value(fv, &mut handle.engine) {
            Ok(v) => values.push(v),
            Err(msg) => {
                return set_engine_error_message(
                    handle,
                    FerricError::InvalidArgument,
                    format!("field {i}: {msg}"),
                );
            }
        }
    }

    match handle.engine.assert_ordered(relation_str, values) {
        Ok(fid) => {
            if !out_fact_id.is_null() {
                *out_fact_id = fid.data().as_ffi();
            }
            FerricError::Ok
        }
        Err(ref err) => set_engine_runtime_error(handle, err),
    }
}

// ---------------------------------------------------------------------------
// C API: Template introspection
// ---------------------------------------------------------------------------

/// Get the number of registered templates.
///
/// # Safety
///
/// - `engine` must be a valid engine pointer.
/// - `out_count` must be a valid pointer.
#[no_mangle]
pub unsafe extern "C" fn ferric_engine_template_count(
    engine: *const FerricEngine,
    out_count: *mut usize,
) -> FerricError {
    let handle = match borrow_engine_checked(engine) {
        Ok(h) => h,
        Err(code) => return code,
    };
    if out_count.is_null() {
        set_global_error("out_count pointer is null".to_string());
        return FerricError::NullPointer;
    }
    *out_count = handle.engine.templates().len();
    FerricError::Ok
}

/// Get the name of a template by zero-based index.
///
/// Standard buffer copy pattern.
///
/// # Safety
///
/// - `engine` must be a valid engine pointer.
/// - `out_len` must be a valid pointer.
/// - If `buf` is non-null, it must point to at least `buf_len` writable bytes.
#[no_mangle]
pub unsafe extern "C" fn ferric_engine_template_name(
    engine: *const FerricEngine,
    index: usize,
    buf: *mut c_char,
    buf_len: usize,
    out_len: *mut usize,
) -> FerricError {
    let handle = match borrow_engine_checked(engine) {
        Ok(h) => h,
        Err(code) => return code,
    };
    if out_len.is_null() {
        set_global_error("out_len pointer is null".to_string());
        return FerricError::NullPointer;
    }

    let templates = handle.engine.templates();
    if index >= templates.len() {
        set_global_error(format!(
            "template index {index} out of bounds (count: {})",
            templates.len()
        ));
        return FerricError::InvalidArgument;
    }
    copy_str_to_buffer(templates[index], buf, buf_len, out_len)
}

/// Get the number of slots in a named template.
///
/// # Safety
///
/// - `engine` must be a valid engine pointer.
/// - `template_name` must be a valid NUL-terminated string.
/// - `out_count` must be a valid pointer.
#[no_mangle]
pub unsafe extern "C" fn ferric_engine_template_slot_count(
    engine: *const FerricEngine,
    template_name: *const c_char,
    out_count: *mut usize,
) -> FerricError {
    let handle = match borrow_engine_checked(engine) {
        Ok(h) => h,
        Err(code) => return code,
    };
    let name_str = match c_str_to_str(template_name, "template_name") {
        Ok(s) => s,
        Err(code) => return code,
    };
    if out_count.is_null() {
        set_global_error("out_count pointer is null".to_string());
        return FerricError::NullPointer;
    }

    if let Some(slots) = handle.engine.template_slot_names(name_str) {
        *out_count = slots.len();
        FerricError::Ok
    } else {
        set_global_error(format!("template not found: {name_str}"));
        FerricError::NotFound
    }
}

/// Get the name of a slot in a named template by zero-based index.
///
/// Standard buffer copy pattern.
///
/// # Safety
///
/// - `engine` must be a valid engine pointer.
/// - `template_name` must be a valid NUL-terminated string.
/// - `out_len` must be a valid pointer.
/// - If `buf` is non-null, it must point to at least `buf_len` writable bytes.
#[no_mangle]
pub unsafe extern "C" fn ferric_engine_template_slot_name(
    engine: *const FerricEngine,
    template_name: *const c_char,
    slot_index: usize,
    buf: *mut c_char,
    buf_len: usize,
    out_len: *mut usize,
) -> FerricError {
    let handle = match borrow_engine_checked(engine) {
        Ok(h) => h,
        Err(code) => return code,
    };
    let name_str = match c_str_to_str(template_name, "template_name") {
        Ok(s) => s,
        Err(code) => return code,
    };
    if out_len.is_null() {
        set_global_error("out_len pointer is null".to_string());
        return FerricError::NullPointer;
    }

    if let Some(slots) = handle.engine.template_slot_names(name_str) {
        if slot_index >= slots.len() {
            set_global_error(format!(
                "slot index {slot_index} out of bounds (count: {})",
                slots.len()
            ));
            return FerricError::InvalidArgument;
        }
        copy_str_to_buffer(slots[slot_index], buf, buf_len, out_len)
    } else {
        set_global_error(format!("template not found: {name_str}"));
        FerricError::NotFound
    }
}

// ---------------------------------------------------------------------------
// C API: Rule introspection
// ---------------------------------------------------------------------------

/// Get the number of registered rules.
///
/// # Safety
///
/// - `engine` must be a valid engine pointer.
/// - `out_count` must be a valid pointer.
#[no_mangle]
pub unsafe extern "C" fn ferric_engine_rule_count(
    engine: *const FerricEngine,
    out_count: *mut usize,
) -> FerricError {
    let handle = match borrow_engine_checked(engine) {
        Ok(h) => h,
        Err(code) => return code,
    };
    if out_count.is_null() {
        set_global_error("out_count pointer is null".to_string());
        return FerricError::NullPointer;
    }
    *out_count = handle.engine.rules().len();
    FerricError::Ok
}

/// Get the name and salience of a rule by zero-based index.
///
/// The rule name is written to `buf` using the standard buffer copy pattern.
/// Salience is written to `*out_salience` if non-null.
///
/// # Safety
///
/// - `engine` must be a valid engine pointer.
/// - `out_len` must be a valid pointer.
/// - If `buf` is non-null, it must point to at least `buf_len` writable bytes.
/// - `out_salience` may be null.
#[no_mangle]
pub unsafe extern "C" fn ferric_engine_rule_info(
    engine: *const FerricEngine,
    index: usize,
    buf: *mut c_char,
    buf_len: usize,
    out_len: *mut usize,
    out_salience: *mut i32,
) -> FerricError {
    let handle = match borrow_engine_checked(engine) {
        Ok(h) => h,
        Err(code) => return code,
    };
    if out_len.is_null() {
        set_global_error("out_len pointer is null".to_string());
        return FerricError::NullPointer;
    }

    let rules = handle.engine.rules();
    if index >= rules.len() {
        set_global_error(format!(
            "rule index {index} out of bounds (count: {})",
            rules.len()
        ));
        return FerricError::InvalidArgument;
    }
    let (name, salience) = rules[index];
    if !out_salience.is_null() {
        *out_salience = salience;
    }
    copy_str_to_buffer(name, buf, buf_len, out_len)
}

// ---------------------------------------------------------------------------
// C API: Module operations
// ---------------------------------------------------------------------------

/// Get the current module name.
///
/// Standard buffer copy pattern.
///
/// # Safety
///
/// - `engine` must be a valid engine pointer.
/// - `out_len` must be a valid pointer.
/// - If `buf` is non-null, it must point to at least `buf_len` writable bytes.
#[no_mangle]
pub unsafe extern "C" fn ferric_engine_current_module(
    engine: *const FerricEngine,
    buf: *mut c_char,
    buf_len: usize,
    out_len: *mut usize,
) -> FerricError {
    let handle = match borrow_engine_checked(engine) {
        Ok(h) => h,
        Err(code) => return code,
    };
    if out_len.is_null() {
        set_global_error("out_len pointer is null".to_string());
        return FerricError::NullPointer;
    }
    let name = handle.engine.current_module();
    copy_str_to_buffer(name, buf, buf_len, out_len)
}

/// Get the name of the module at the top of the focus stack.
///
/// Standard buffer copy pattern. Returns `NotFound` if the focus stack is empty.
///
/// # Safety
///
/// - `engine` must be a valid engine pointer.
/// - `out_len` must be a valid pointer.
/// - If `buf` is non-null, it must point to at least `buf_len` writable bytes.
#[no_mangle]
pub unsafe extern "C" fn ferric_engine_get_focus(
    engine: *const FerricEngine,
    buf: *mut c_char,
    buf_len: usize,
    out_len: *mut usize,
) -> FerricError {
    let handle = match borrow_engine_checked(engine) {
        Ok(h) => h,
        Err(code) => return code,
    };
    if out_len.is_null() {
        set_global_error("out_len pointer is null".to_string());
        return FerricError::NullPointer;
    }

    if let Some(name) = handle.engine.get_focus() {
        copy_str_to_buffer(name, buf, buf_len, out_len)
    } else {
        *out_len = 0;
        FerricError::NotFound
    }
}

/// Get the depth of the focus stack.
///
/// # Safety
///
/// - `engine` must be a valid engine pointer.
/// - `out_depth` must be a valid pointer.
#[no_mangle]
pub unsafe extern "C" fn ferric_engine_focus_stack_depth(
    engine: *const FerricEngine,
    out_depth: *mut usize,
) -> FerricError {
    let handle = match borrow_engine_checked(engine) {
        Ok(h) => h,
        Err(code) => return code,
    };
    if out_depth.is_null() {
        set_global_error("out_depth pointer is null".to_string());
        return FerricError::NullPointer;
    }
    *out_depth = handle.engine.get_focus_stack().len();
    FerricError::Ok
}

/// Get a focus stack entry by zero-based index.
///
/// Index 0 = bottom of stack, last index = top (current focus).
/// Standard buffer copy pattern.
///
/// # Safety
///
/// - `engine` must be a valid engine pointer.
/// - `out_len` must be a valid pointer.
/// - If `buf` is non-null, it must point to at least `buf_len` writable bytes.
#[no_mangle]
pub unsafe extern "C" fn ferric_engine_focus_stack_entry(
    engine: *const FerricEngine,
    index: usize,
    buf: *mut c_char,
    buf_len: usize,
    out_len: *mut usize,
) -> FerricError {
    let handle = match borrow_engine_checked(engine) {
        Ok(h) => h,
        Err(code) => return code,
    };
    if out_len.is_null() {
        set_global_error("out_len pointer is null".to_string());
        return FerricError::NullPointer;
    }

    let stack = handle.engine.get_focus_stack();
    if index >= stack.len() {
        set_global_error(format!(
            "focus stack index {index} out of bounds (depth: {})",
            stack.len()
        ));
        return FerricError::InvalidArgument;
    }
    copy_str_to_buffer(stack[index], buf, buf_len, out_len)
}

/// Get the number of registered modules.
///
/// # Safety
///
/// - `engine` must be a valid engine pointer.
/// - `out_count` must be a valid pointer.
#[no_mangle]
pub unsafe extern "C" fn ferric_engine_module_count(
    engine: *const FerricEngine,
    out_count: *mut usize,
) -> FerricError {
    let handle = match borrow_engine_checked(engine) {
        Ok(h) => h,
        Err(code) => return code,
    };
    if out_count.is_null() {
        set_global_error("out_count pointer is null".to_string());
        return FerricError::NullPointer;
    }
    *out_count = handle.engine.modules().len();
    FerricError::Ok
}

/// Get the name of a module by zero-based index.
///
/// Standard buffer copy pattern.
///
/// # Safety
///
/// - `engine` must be a valid engine pointer.
/// - `out_len` must be a valid pointer.
/// - If `buf` is non-null, it must point to at least `buf_len` writable bytes.
#[no_mangle]
pub unsafe extern "C" fn ferric_engine_module_name(
    engine: *const FerricEngine,
    index: usize,
    buf: *mut c_char,
    buf_len: usize,
    out_len: *mut usize,
) -> FerricError {
    let handle = match borrow_engine_checked(engine) {
        Ok(h) => h,
        Err(code) => return code,
    };
    if out_len.is_null() {
        set_global_error("out_len pointer is null".to_string());
        return FerricError::NullPointer;
    }

    let modules = handle.engine.modules();
    if index >= modules.len() {
        set_global_error(format!(
            "module index {index} out of bounds (count: {})",
            modules.len()
        ));
        return FerricError::InvalidArgument;
    }
    copy_str_to_buffer(modules[index], buf, buf_len, out_len)
}

// ---------------------------------------------------------------------------
// C API: Agenda, halt, input, clear
// ---------------------------------------------------------------------------

/// Get the number of activations on the agenda.
///
/// # Safety
///
/// - `engine` must be a valid engine pointer.
/// - `out_count` must be a valid pointer.
#[no_mangle]
pub unsafe extern "C" fn ferric_engine_agenda_count(
    engine: *const FerricEngine,
    out_count: *mut usize,
) -> FerricError {
    let handle = match borrow_engine_checked(engine) {
        Ok(h) => h,
        Err(code) => return code,
    };
    if out_count.is_null() {
        set_global_error("out_count pointer is null".to_string());
        return FerricError::NullPointer;
    }
    *out_count = handle.engine.agenda_len();
    FerricError::Ok
}

/// Check whether the engine is halted.
///
/// Writes 1 to `*out_halted` if halted, 0 if not halted.
///
/// # Safety
///
/// - `engine` must be a valid engine pointer.
/// - `out_halted` must be a valid pointer.
#[no_mangle]
pub unsafe extern "C" fn ferric_engine_is_halted(
    engine: *const FerricEngine,
    out_halted: *mut i32,
) -> FerricError {
    let handle = match borrow_engine_checked(engine) {
        Ok(h) => h,
        Err(code) => return code,
    };
    if out_halted.is_null() {
        set_global_error("out_halted pointer is null".to_string());
        return FerricError::NullPointer;
    }
    *out_halted = i32::from(handle.engine.is_halted());
    FerricError::Ok
}

/// Request the engine to halt.
///
/// Always succeeds. Idempotent — halting an already-halted engine is a no-op.
///
/// # Safety
///
/// - `engine` must be a valid engine pointer.
#[no_mangle]
pub unsafe extern "C" fn ferric_engine_halt(engine: *mut FerricEngine) -> FerricError {
    let handle = match borrow_engine_mut(engine) {
        Ok(h) => h,
        Err(code) => return code,
    };
    handle.engine.halt();
    FerricError::Ok
}

/// Push an input line for the engine's `read`/`readline` functions.
///
/// # Safety
///
/// - `engine` must be a valid engine pointer.
/// - `line` must be a valid NUL-terminated string.
#[no_mangle]
pub unsafe extern "C" fn ferric_engine_push_input(
    engine: *mut FerricEngine,
    line: *const c_char,
) -> FerricError {
    let line_str = match c_str_to_str(line, "line") {
        Ok(s) => s,
        Err(code) => return code,
    };
    let handle = match borrow_engine_mut(engine) {
        Ok(h) => h,
        Err(code) => return code,
    };
    handle.engine.push_input(line_str);
    FerricError::Ok
}

/// Reset the engine to a blank slate.
///
/// Removes all facts, rules, templates, globals, functions, generics, and
/// modules except MAIN. Always succeeds.
///
/// # Safety
///
/// - `engine` must be a valid engine pointer.
#[no_mangle]
pub unsafe extern "C" fn ferric_engine_clear(engine: *mut FerricEngine) -> FerricError {
    let handle = match borrow_engine_mut(engine) {
        Ok(h) => h,
        Err(code) => return code,
    };
    handle.engine.clear();
    FerricError::Ok
}

// ---------------------------------------------------------------------------
// C API: Convenience and improved variants
// ---------------------------------------------------------------------------

/// Create an engine from CLIPS source with default configuration.
///
/// Returns a heap-allocated engine handle, or null on parse/compile error
/// (sets global error message). The engine has already been loaded and reset.
///
/// # Safety
///
/// - `source` must be a valid NUL-terminated UTF-8 string, or null.
/// - Returned pointer must be freed with `ferric_engine_free`.
#[no_mangle]
pub unsafe extern "C" fn ferric_engine_new_with_source(source: *const c_char) -> *mut FerricEngine {
    ferric_engine_new_with_source_config(source, ptr::null())
}

/// Create an engine from CLIPS source with explicit configuration.
///
/// If `config` is null, defaults are used.
/// Returns null on parse/compile error (sets global error message).
///
/// # Safety
///
/// - `source` must be a valid NUL-terminated UTF-8 string, or null.
/// - `config` may be null.
/// - Returned pointer must be freed with `ferric_engine_free`.
#[no_mangle]
pub unsafe extern "C" fn ferric_engine_new_with_source_config(
    source: *const c_char,
    config: *const FerricConfig,
) -> *mut FerricEngine {
    let source_str = if source.is_null() {
        set_global_error("source pointer is null".to_string());
        return ptr::null_mut();
    } else {
        match CStr::from_ptr(source).to_str() {
            Ok(s) => s,
            Err(e) => {
                set_global_error(format!("source is not valid UTF-8: {e}"));
                return ptr::null_mut();
            }
        }
    };

    let engine_config = if config.is_null() {
        EngineConfig::default()
    } else {
        match engine_config_from_ffi(&*config) {
            Ok(cfg) => cfg,
            Err(message) => {
                set_global_error(message);
                return ptr::null_mut();
            }
        }
    };

    match Engine::with_rules_config(source_str, engine_config) {
        Ok(engine) => {
            let handle = FerricEngine {
                engine,
                error_state: EngineErrorState::new(),
                error_cstring: RefCell::new(None),
            };
            Box::into_raw(Box::new(handle))
        }
        Err(err) => {
            set_global_error(match err {
                InitError::Load(ref errors) => errors
                    .first()
                    .map_or_else(|| "unknown load error".to_string(), ToString::to_string),
                InitError::Reset(ref e) => e.to_string(),
            });
            ptr::null_mut()
        }
    }
}

/// Clear a specific output channel.
///
/// Always succeeds — clearing a non-existent channel is a no-op.
///
/// # Safety
///
/// - `engine` must be a valid engine pointer.
/// - `channel` must be a valid NUL-terminated string.
#[no_mangle]
pub unsafe extern "C" fn ferric_engine_clear_output(
    engine: *mut FerricEngine,
    channel: *const c_char,
) -> FerricError {
    let channel_str = match c_str_to_str(channel, "channel") {
        Ok(s) => s,
        Err(code) => return code,
    };
    let handle = match borrow_engine_mut(engine) {
        Ok(h) => h,
        Err(code) => return code,
    };
    handle.engine.clear_output_channel(channel_str);
    FerricError::Ok
}

/// Extended run with halt reason output.
///
/// Same limit semantics as `ferric_engine_run` (negative = unlimited).
/// Additionally writes the halt reason to `*out_reason` if non-null.
///
/// # Safety
///
/// - `engine` must be a valid engine pointer.
/// - `out_fired` may be null.
/// - `out_reason` may be null.
#[no_mangle]
pub unsafe extern "C" fn ferric_engine_run_ex(
    engine: *mut FerricEngine,
    limit: i64,
    out_fired: *mut u64,
    out_reason: *mut FerricHaltReason,
) -> FerricError {
    let handle = match borrow_engine_mut(engine) {
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
            if !out_reason.is_null() {
                *out_reason = FerricHaltReason::from(result.halt_reason);
            }
            FerricError::Ok
        }
        Err(ref err) => set_engine_runtime_error(handle, err),
    }
}

// ---------------------------------------------------------------------------
// C API: Template fact assertion
// ---------------------------------------------------------------------------

/// Assert a template fact with named slots.
///
/// Looks up the template by name, resolves slot names to positions,
/// fills in defaults for unspecified slots, and asserts the fact.
///
/// `slot_names` and `slot_values` must each point to `count` elements.
/// Each `slot_names[i]` is a NUL-terminated C string naming a slot,
/// and `slot_values[i]` is the corresponding value for that slot.
///
/// # Safety
///
/// - `engine` must be a valid engine pointer.
/// - `template_name` must be a valid NUL-terminated string.
/// - If `count > 0`, `slot_names` must point to `count` valid NUL-terminated string pointers.
/// - If `count > 0`, `slot_values` must point to `count` valid `FerricValue`s.
/// - `out_fact_id` may be null.
#[no_mangle]
pub unsafe extern "C" fn ferric_engine_assert_template(
    engine: *mut FerricEngine,
    template_name: *const c_char,
    slot_names: *const *const c_char,
    slot_values: *const FerricValue,
    count: usize,
    out_fact_id: *mut u64,
) -> FerricError {
    use slotmap::Key as _;

    let tmpl_str = match c_str_to_str(template_name, "template_name") {
        Ok(s) => s,
        Err(code) => return code,
    };

    if count > 0 {
        if slot_names.is_null() {
            set_global_error("slot_names pointer is null with non-zero count".to_string());
            return FerricError::NullPointer;
        }
        if slot_values.is_null() {
            set_global_error("slot_values pointer is null with non-zero count".to_string());
            return FerricError::NullPointer;
        }
    }

    let handle = match borrow_engine_mut(engine) {
        Ok(h) => h,
        Err(code) => return code,
    };

    // Convert slot names from C strings.
    let mut names = Vec::with_capacity(count);
    for i in 0..count {
        let name_ptr = *slot_names.add(i);
        match c_str_to_str(name_ptr, &format!("slot_names[{i}]")) {
            Ok(s) => names.push(s),
            Err(code) => return code,
        }
    }

    // Convert slot values from FerricValue.
    let mut values = Vec::with_capacity(count);
    for i in 0..count {
        let fv = &*slot_values.add(i);
        match ferric_to_value(fv, &mut handle.engine) {
            Ok(v) => values.push(v),
            Err(msg) => {
                return set_engine_error_message(
                    handle,
                    FerricError::InvalidArgument,
                    format!("slot_values[{i}]: {msg}"),
                );
            }
        }
    }

    let name_refs: Vec<&str> = names.clone();

    match handle.engine.assert_template(tmpl_str, &name_refs, values) {
        Ok(fid) => {
            if !out_fact_id.is_null() {
                *out_fact_id = fid.data().as_ffi();
            }
            FerricError::Ok
        }
        Err(ref err) => set_engine_runtime_error(handle, err),
    }
}

// ---------------------------------------------------------------------------
// C API: Template slot access by name
// ---------------------------------------------------------------------------

/// Get a template fact's slot value by name.
///
/// The fact must be a template fact. For ordered facts, returns
/// `FERRIC_ERROR_INVALID_ARGUMENT`. If the slot name is not found,
/// returns `FERRIC_ERROR_NOT_FOUND`.
///
/// The returned `FerricValue` is written to `*out_value`. The caller owns
/// any heap-allocated resources and must free them with `ferric_value_free`.
///
/// # Safety
///
/// - `engine` must be a valid engine pointer.
/// - `slot_name` must be a valid NUL-terminated string.
/// - `out_value` must be a valid pointer to a `FerricValue`.
#[no_mangle]
pub unsafe extern "C" fn ferric_engine_get_fact_slot_by_name(
    engine: *const FerricEngine,
    fact_id: u64,
    slot_name: *const c_char,
    out_value: *mut FerricValue,
) -> FerricError {
    let name_str = match c_str_to_str(slot_name, "slot_name") {
        Ok(s) => s,
        Err(code) => return code,
    };

    if out_value.is_null() {
        set_global_error("out_value pointer is null".to_string());
        return FerricError::NullPointer;
    }

    let handle = match borrow_engine_checked(engine) {
        Ok(h) => h,
        Err(code) => return code,
    };

    let key_data = slotmap::KeyData::from_ffi(fact_id);
    let fid = ferric_core::FactId::from(key_data);

    match handle.engine.get_fact_slot_by_name(fid, name_str) {
        Ok(value) => {
            *out_value = value_to_ferric(value, &handle.engine);
            FerricError::Ok
        }
        Err(ref err) => set_engine_error_global(err),
    }
}

// ---------------------------------------------------------------------------
// C API: Unchecked free (for GC finalizers)
// ---------------------------------------------------------------------------

/// Free an engine handle without checking thread affinity.
///
/// This is intended for use by garbage-collected runtimes (Go, etc.) whose
/// finalizers run on arbitrary threads. In normal usage, prefer
/// `ferric_engine_free` which validates thread affinity.
///
/// Null pointers are safely ignored.
///
/// # Safety
///
/// - `engine` must be a pointer returned by `ferric_engine_new` or null.
/// - The engine must not be in use by another call when freed.
/// - The caller must guarantee that no other thread is concurrently using this engine.
#[no_mangle]
pub unsafe extern "C" fn ferric_engine_free_unchecked(engine: *mut FerricEngine) -> FerricError {
    if engine.is_null() {
        return FerricError::Ok;
    }
    drop(Box::from_raw(engine));
    FerricError::Ok
}

// ---------------------------------------------------------------------------
// Engine serialization / deserialization
// ---------------------------------------------------------------------------

/// Callback type for caller-controlled memory allocation.
///
/// When non-null, called by `ferric_engine_serialize` with the exact byte
/// count needed. The `context` parameter is passed through unchanged from
/// the serialize call.
///
/// Must return a pointer to at least `size` writable bytes, or null to
/// signal allocation failure.
#[cfg(feature = "serde")]
pub type FerricAllocFn =
    Option<unsafe extern "C" fn(size: usize, context: *mut std::ffi::c_void) -> *mut u8>;

/// Serialize engine state to bytes.
///
/// Produces a binary snapshot that can be passed to `ferric_engine_deserialize`
/// to reconstruct an equivalent engine, skipping the parse/compile pipeline.
///
/// ## Memory allocation
///
/// - If `alloc_fn` is **non-null**: the callback is called once with the exact
///   byte count needed. The serialized data is written into the returned
///   buffer. The caller owns this memory and is responsible for freeing it
///   (via their own allocator). `alloc_context` is passed through unchanged.
///
/// - If `alloc_fn` is **null**: Rust allocates the output buffer internally.
///   The caller must free it with `ferric_bytes_free(out_data, out_len)`.
///
/// In both cases, `*out_data` and `*out_len` are set on success.
///
/// # Safety
///
/// - `engine` must be a valid engine pointer.
/// - `out_data` and `out_len` must be valid, non-null pointers.
/// - If `alloc_fn` is non-null, it must return a valid pointer to `size` bytes
///   (or null to signal failure).
#[no_mangle]
#[cfg(feature = "serde")]
pub unsafe extern "C" fn ferric_engine_serialize(
    engine: *const FerricEngine,
    alloc_fn: FerricAllocFn,
    alloc_context: *mut std::ffi::c_void,
    out_data: *mut *mut u8,
    out_len: *mut usize,
) -> FerricError {
    // Validate output pointers
    if out_data.is_null() || out_len.is_null() {
        set_global_error("out_data and out_len must be non-null".to_string());
        return FerricError::NullPointer;
    }

    // Validate engine and check thread affinity
    let handle = match borrow_engine_checked(engine) {
        Ok(h) => h,
        Err(code) => return code,
    };

    // Serialize to internal Vec<u8>
    let bytes = match handle.engine.serialize_to_bytes() {
        Ok(b) => b,
        Err(e) => {
            set_global_error(e.to_string());
            return FerricError::SerializationError;
        }
    };

    let len = bytes.len();

    if let Some(alloc) = alloc_fn {
        // Caller-provided allocator path
        let buf = alloc(len, alloc_context);
        if buf.is_null() {
            set_global_error("caller allocator returned null".to_string());
            return FerricError::SerializationError;
        }
        std::ptr::copy_nonoverlapping(bytes.as_ptr(), buf, len);
        *out_data = buf;
    } else {
        // Rust-allocated path: leak the Vec as a Box<[u8]>
        let boxed: Box<[u8]> = bytes.into_boxed_slice();
        *out_data = Box::into_raw(boxed).cast::<u8>();
    }

    *out_len = len;
    FerricError::Ok
}

/// Deserialize an engine from bytes previously produced by
/// `ferric_engine_serialize`.
///
/// The returned engine handle is ready for use (e.g. `ferric_engine_run`).
/// Its thread affinity is set to the calling thread.
///
/// # Safety
///
/// - `data` must point to `len` valid, readable bytes.
/// - `out_engine` must be a valid, non-null pointer.
/// - The returned engine must be freed with `ferric_engine_free`.
#[no_mangle]
#[cfg(feature = "serde")]
pub unsafe extern "C" fn ferric_engine_deserialize(
    data: *const u8,
    len: usize,
    out_engine: *mut *mut FerricEngine,
) -> FerricError {
    if data.is_null() {
        set_global_error("data pointer is null".to_string());
        return FerricError::NullPointer;
    }
    if out_engine.is_null() {
        set_global_error("out_engine pointer is null".to_string());
        return FerricError::NullPointer;
    }

    let slice = std::slice::from_raw_parts(data, len);

    let engine = match ferric_runtime::Engine::deserialize_from_bytes(slice) {
        Ok(e) => e,
        Err(e) => {
            set_global_error(e.to_string());
            return FerricError::SerializationError;
        }
    };

    let handle = FerricEngine {
        engine,
        error_state: EngineErrorState::new(),
        error_cstring: RefCell::new(None),
    };

    *out_engine = Box::into_raw(Box::new(handle));
    FerricError::Ok
}

/// Free a byte buffer that was allocated by `ferric_engine_serialize` when
/// `alloc_fn` was null.
///
/// Null pointers and zero lengths are safely ignored.
///
/// # Safety
///
/// - `data` must be a pointer returned by `ferric_engine_serialize` (with
///   null `alloc_fn`), or null.
/// - `len` must be the length reported by the corresponding serialize call.
/// - The buffer must not have been previously freed.
#[no_mangle]
#[cfg(feature = "serde")]
pub unsafe extern "C" fn ferric_bytes_free(data: *mut u8, len: usize) {
    if data.is_null() || len == 0 {
        return;
    }
    let slice_ptr = std::ptr::slice_from_raw_parts_mut(data, len);
    drop(Box::from_raw(slice_ptr));
}
