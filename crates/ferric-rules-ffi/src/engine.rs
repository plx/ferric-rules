//! FFI engine APIs — lifecycle, execution, and fact operations.
//!
//! ## Thread Affinity Contract
//!
//! Runtime-facing `ferric_engine_*` entry points validate that the calling
//! thread matches the thread that created the engine before projecting a
//! reference to runtime state.
//!
//! - Thread violations return `FERRIC_ERROR_THREAD_VIOLATION` with a descriptive
//!   message in the global error channel.
//! - `ferric_engine_last_error_copy` provides synchronized, coherent snapshots
//!   to any thread.
//! - `ferric_engine_last_error` also skips affinity checks, but its returned
//!   pointer must not be used while another borrowed read or engine free may
//!   occur.
//! - `ferric_engine_free_unchecked` deliberately skips affinity solely to
//!   destroy the handle; it must not overlap any access to that engine.
//!
//! The internal `unsafe fn move_to_current_thread` is deliberately NOT
//! exposed through the C API.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::ffi::{CStr, CString};
use std::os::raw::c_char;
use std::ptr::{self, NonNull};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, MutexGuard};
use std::thread::ThreadId;

use crate::types::{
    engine_config_from_ffi, ferric_to_value, value_to_ferric, FerricConfig, FerricFactType,
    FerricHaltReason, FerricValue,
};

use crate::error::{
    copy_error_to_buffer, map_engine_error, map_load_error, set_engine_and_global_error,
    set_global_error, EngineErrorState, FerricError,
};
use ferric_rules_ffi_macros::ffi_export;
use ferric_rules_runtime::engine::EngineError;
use ferric_rules_runtime::loader::LoadError;
use ferric_rules_runtime::{Engine, EngineConfig, InitError, RunLimit};

/// Opaque engine handle exposed to C.
///
/// The runtime engine remains owner-thread-only. Per-engine error snapshots
/// are stored separately so the two last-error accessors can safely read them
/// from any thread.
///
/// Production accessors never construct a Rust reference to this whole
/// structure. They project references to individual fields from the raw
/// handle, which permits an owner-thread `&mut Engine` to coexist with a
/// foreign-thread reference to the disjoint diagnostic mutex.
///
/// C code receives `*mut FerricEngine` as an opaque pointer.
pub struct FerricEngine {
    pub(crate) engine: Engine,
    owner_thread: ThreadId,
    call_active: AtomicBool,
    logical_run_continuation_ready: Cell<bool>,
    diagnostics: Mutex<EngineDiagnostics>,
    output_cstrings: RefCell<HashMap<String, CachedOutputCString>>,
}

#[derive(Debug, Default)]
struct EngineDiagnostics {
    error_state: EngineErrorState,
    borrowed_cstring: Option<CString>,
}

impl EngineDiagnostics {
    fn new() -> Self {
        Self {
            error_state: EngineErrorState::new(),
            borrowed_cstring: None,
        }
    }
}

impl FerricEngine {
    fn new(engine: Engine) -> Self {
        Self {
            engine,
            owner_thread: std::thread::current().id(),
            call_active: AtomicBool::new(false),
            logical_run_continuation_ready: Cell::new(false),
            diagnostics: Mutex::new(EngineDiagnostics::new()),
            output_cstrings: RefCell::new(HashMap::new()),
        }
    }

    #[cfg(test)]
    pub(crate) fn set_error_for_test(&self, message: String) {
        lock_unpoisoned(&self.diagnostics).error_state.set(message);
    }
}

struct CachedOutputCString {
    snapshot: String,
    cstring: CString,
    #[cfg(test)]
    lifetime: std::sync::Arc<()>,
}

impl CachedOutputCString {
    fn new(output: &str) -> Result<Self, std::ffi::NulError> {
        Ok(Self {
            snapshot: output.to_string(),
            cstring: CString::new(output)?,
            #[cfg(test)]
            lifetime: std::sync::Arc::new(()),
        })
    }
}

fn prune_cleared_output_snapshots(
    engine: &Engine,
    output_cstrings: &RefCell<HashMap<String, CachedOutputCString>>,
) {
    output_cstrings.borrow_mut().retain(|channel, _| {
        engine
            .get_output(channel)
            .is_some_and(|output| !output.is_empty())
    });
}

#[cfg(test)]
pub(crate) unsafe fn output_cache_lifetime_for_test(
    engine: *const FerricEngine,
    channel: &str,
) -> Option<std::sync::Weak<()>> {
    let handle = NonNull::new(engine.cast_mut())?;
    output_cstrings(handle)
        .borrow()
        .get(channel)
        .map(|entry| std::sync::Arc::downgrade(&entry.lifetime))
}

#[cfg(test)]
pub(crate) unsafe fn output_cache_entry_count_for_test(
    engine: *const FerricEngine,
) -> Option<usize> {
    let handle = NonNull::new(engine.cast_mut())?;
    Some(output_cstrings(handle).borrow().len())
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

/// Validate a non-null engine pointer without constructing a Rust reference.
///
/// A whole-handle reference would overlap owner-thread access to `engine` with
/// foreign-thread diagnostic access. Callers must project only the field they
/// need from the returned token.
fn validate_engine_ptr(engine: *const FerricEngine) -> Result<NonNull<FerricEngine>, FerricError> {
    NonNull::new(engine.cast_mut()).ok_or_else(|| {
        set_global_error("engine pointer is null".to_string());
        FerricError::NullPointer
    })
}

fn lock_unpoisoned<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// Project the immutable owner-thread field from an opaque handle.
///
/// # Safety
///
/// `handle` must remain valid for the duration of the returned reference.
unsafe fn owner_thread<'a>(handle: NonNull<FerricEngine>) -> &'a ThreadId {
    &*std::ptr::addr_of!((*handle.as_ptr()).owner_thread)
}

/// Project the active-call flag from an opaque handle.
///
/// # Safety
///
/// `handle` must remain valid for the duration of the returned reference.
unsafe fn call_active<'a>(handle: NonNull<FerricEngine>) -> &'a AtomicBool {
    &*std::ptr::addr_of!((*handle.as_ptr()).call_active)
}

/// Project the owner-thread logical-run continuation state.
///
/// # Safety
///
/// `handle` must point to a live engine handle, and access must remain on the
/// engine's owner thread without overlapping another runtime call.
unsafe fn logical_run_continuation_ready<'a>(handle: NonNull<FerricEngine>) -> &'a Cell<bool> {
    &*std::ptr::addr_of!((*handle.as_ptr()).logical_run_continuation_ready)
}

/// Project the synchronized diagnostic state from an opaque handle.
///
/// # Safety
///
/// `handle` must remain valid for the duration of the returned reference.
unsafe fn diagnostics<'a>(handle: NonNull<FerricEngine>) -> &'a Mutex<EngineDiagnostics> {
    &*std::ptr::addr_of!((*handle.as_ptr()).diagnostics)
}

/// Project the owner-thread borrowed-output cache from an opaque handle.
///
/// # Safety
///
/// `handle` must point to a live engine handle, and access must remain on the
/// engine's owner thread without overlapping another runtime call.
unsafe fn output_cstrings<'a>(
    handle: NonNull<FerricEngine>,
) -> &'a RefCell<HashMap<String, CachedOutputCString>> {
    &*std::ptr::addr_of!((*handle.as_ptr()).output_cstrings)
}

/// Check thread affinity without touching the owner-thread-only runtime.
///
/// # Safety
///
/// `handle` must point to a live engine handle.
unsafe fn check_thread_affinity(handle: NonNull<FerricEngine>) -> Result<(), FerricError> {
    let creator = *owner_thread(handle);
    let current = std::thread::current().id();
    if current != creator {
        let err = EngineError::WrongThread { creator, current };
        return Err(set_engine_error_for_handle(
            handle,
            FerricError::ThreadViolation,
            err.to_string(),
        ));
    }
    Ok(())
}

struct EngineCallGuard<'a> {
    active: &'a AtomicBool,
    release_on_drop: bool,
}

impl EngineCallGuard<'_> {
    fn disarm(mut self) {
        self.release_on_drop = false;
    }
}

impl Drop for EngineCallGuard<'_> {
    fn drop(&mut self) {
        if self.release_on_drop {
            self.active.store(false, Ordering::Release);
        }
    }
}

struct EngineWriteAccess<'a> {
    engine: &'a mut Engine,
    logical_run_continuation_ready: &'a Cell<bool>,
    diagnostics: &'a Mutex<EngineDiagnostics>,
    output_cstrings: &'a RefCell<HashMap<String, CachedOutputCString>>,
    _guard: EngineCallGuard<'a>,
}

struct EngineReadAccess<'a> {
    engine: &'a Engine,
    diagnostics: &'a Mutex<EngineDiagnostics>,
    output_cstrings: &'a RefCell<HashMap<String, CachedOutputCString>>,
    _guard: EngineCallGuard<'a>,
}

/// Reject overlapping access to the owner-thread-only runtime.
///
/// This primarily protects against a host callback synchronously re-entering
/// the same raw engine. Diagnostic-only access does not use this guard and is
/// safe to perform reentrantly.
///
/// # Safety
///
/// `handle` must point to a live engine handle.
unsafe fn enter_engine_call<'a>(
    handle: NonNull<FerricEngine>,
) -> Result<EngineCallGuard<'a>, FerricError> {
    let active = call_active(handle);
    if active
        .compare_exchange(false, true, Ordering::Acquire, Ordering::Relaxed)
        .is_err()
    {
        return Err(set_engine_error_for_handle(
            handle,
            FerricError::InternalError,
            "reentrant call on a raw engine is not supported; only per-engine error accessors may be called from a host callback"
                .to_string(),
        ));
    }
    Ok(EngineCallGuard {
        active,
        release_on_drop: true,
    })
}

unsafe fn borrow_engine_mut<'a>(
    engine: *mut FerricEngine,
) -> Result<EngineWriteAccess<'a>, FerricError> {
    let access = borrow_engine_mut_preserving_logical_run(engine)?;
    access.logical_run_continuation_ready.set(false);
    Ok(access)
}

unsafe fn borrow_engine_mut_preserving_logical_run<'a>(
    engine: *mut FerricEngine,
) -> Result<EngineWriteAccess<'a>, FerricError> {
    let handle = validate_engine_ptr(engine)?;
    check_thread_affinity(handle)?;
    let guard = enter_engine_call(handle)?;
    let runtime = &mut *std::ptr::addr_of_mut!((*handle.as_ptr()).engine);
    let continuation_ready = logical_run_continuation_ready(handle);
    let diagnostic_state = diagnostics(handle);
    let output_cache = output_cstrings(handle);
    Ok(EngineWriteAccess {
        engine: runtime,
        logical_run_continuation_ready: continuation_ready,
        diagnostics: diagnostic_state,
        output_cstrings: output_cache,
        _guard: guard,
    })
}

unsafe fn borrow_engine_checked<'a>(
    engine: *const FerricEngine,
) -> Result<EngineReadAccess<'a>, FerricError> {
    let handle = validate_engine_ptr(engine)?;
    check_thread_affinity(handle)?;
    let guard = enter_engine_call(handle)?;
    let runtime = &*std::ptr::addr_of!((*handle.as_ptr()).engine);
    let diagnostic_state = diagnostics(handle);
    let output_cache = output_cstrings(handle);
    Ok(EngineReadAccess {
        engine: runtime,
        diagnostics: diagnostic_state,
        output_cstrings: output_cache,
        _guard: guard,
    })
}

fn set_engine_error_message(
    diagnostics: &Mutex<EngineDiagnostics>,
    code: FerricError,
    message: String,
) -> FerricError {
    set_engine_and_global_error(&mut lock_unpoisoned(diagnostics).error_state, message);
    code
}

/// Best-effort per-engine diagnostic update after a generated C wrapper
/// contains a panic.
///
/// # Safety
///
/// `engine` must be null or a live raw-engine handle whose lifetime covers
/// this call.
pub(crate) unsafe fn record_boundary_panic(engine: *const FerricEngine, message: String) {
    let Some(handle) = NonNull::new(engine.cast_mut()) else {
        return;
    };
    lock_unpoisoned(diagnostics(handle))
        .error_state
        .set(message);
}

unsafe fn set_engine_error_for_handle(
    handle: NonNull<FerricEngine>,
    code: FerricError,
    message: String,
) -> FerricError {
    set_engine_error_message(diagnostics(handle), code, message)
}

fn set_engine_runtime_error(
    diagnostics: &Mutex<EngineDiagnostics>,
    err: &EngineError,
) -> FerricError {
    set_engine_error_message(diagnostics, map_engine_error(err), err.to_string())
}

fn set_engine_load_error(diagnostics: &Mutex<EngineDiagnostics>, err: &LoadError) -> FerricError {
    set_engine_error_message(diagnostics, map_load_error(err), err.to_string())
}

unsafe fn engine_c_str_to_str<'a>(
    ptr: *const c_char,
    label: &str,
    diagnostics: &Mutex<EngineDiagnostics>,
) -> Result<&'a str, FerricError> {
    if ptr.is_null() {
        return Err(set_engine_error_message(
            diagnostics,
            FerricError::NullPointer,
            format!("{label} pointer is null"),
        ));
    }
    let c_str = CStr::from_ptr(ptr);
    c_str.to_str().map_err(|error| {
        set_engine_error_message(
            diagnostics,
            FerricError::InvalidArgument,
            format!("{label} is not valid UTF-8: {error}"),
        )
    })
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
    diagnostics: &Mutex<EngineDiagnostics>,
) -> FerricError {
    let needed = s.len() + 1; // string bytes + NUL

    if buf.is_null() {
        *out_len = needed;
        return if buf_len == 0 {
            FerricError::Ok
        } else {
            set_engine_error_message(
                diagnostics,
                FerricError::InvalidArgument,
                "non-zero buf_len with null buf".to_string(),
            )
        };
    }

    if buf_len == 0 {
        *out_len = needed;
        return set_engine_error_message(
            diagnostics,
            FerricError::BufferTooSmall,
            format!("output buffer is too small: need {needed} bytes, got 0"),
        );
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
        set_engine_error_message(
            diagnostics,
            FerricError::BufferTooSmall,
            format!("output buffer is too small: need {needed} bytes, got {buf_len}"),
        )
    }
}

unsafe fn write_value_to_ffi(
    value: &ferric_rules_core::Value,
    engine: &Engine,
    out_value: *mut FerricValue,
    diagnostics: &Mutex<EngineDiagnostics>,
) -> FerricError {
    ptr::write(out_value, FerricValue::void());
    match value_to_ferric(value, engine) {
        Ok(converted) => {
            ptr::write(out_value, converted);
            FerricError::Ok
        }
        Err(message) => {
            set_engine_error_message(diagnostics, FerricError::InvalidArgument, message)
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
#[cfg_attr(ferric_ffi_compile, ffi_export)]
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
#[cfg_attr(ferric_ffi_compile, ffi_export)]
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
    Box::into_raw(Box::new(FerricEngine::new(engine)))
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
#[cfg_attr(ferric_ffi_compile, ffi_export(global_only))]
#[no_mangle]
pub unsafe extern "C" fn ferric_engine_free(engine: *mut FerricEngine) -> FerricError {
    if engine.is_null() {
        return FerricError::Ok;
    }
    let handle = match validate_engine_ptr(engine) {
        Ok(handle) => handle,
        Err(code) => return code,
    };
    if let Err(code) = check_thread_affinity(handle) {
        return code;
    }
    let guard = match enter_engine_call(handle) {
        Ok(guard) => guard,
        Err(code) => return code,
    };
    // The allocation is about to be destroyed, so the active flag must not be
    // touched by the guard's destructor.
    guard.disarm();
    drop(Box::from_raw(engine));
    FerricError::Ok
}

/// Load a CLIPS source string into the engine.
///
/// # Safety
///
/// - `engine` must be a valid engine pointer.
/// - `source` must be a valid NUL-terminated UTF-8 string.
#[cfg_attr(ferric_ffi_compile, ffi_export)]
#[no_mangle]
pub unsafe extern "C" fn ferric_engine_load_string(
    engine: *mut FerricEngine,
    source: *const c_char,
) -> FerricError {
    let handle = match borrow_engine_mut(engine) {
        Ok(h) => h,
        Err(code) => return code,
    };
    let source_str = match engine_c_str_to_str(source, "source string", handle.diagnostics) {
        Ok(s) => s,
        Err(code) => return code,
    };

    match handle.engine.load_str(source_str) {
        Ok(_) => FerricError::Ok,
        Err(errors) => {
            if let Some(first) = errors.first() {
                set_engine_load_error(handle.diagnostics, first)
            } else {
                set_engine_error_message(
                    handle.diagnostics,
                    FerricError::InternalError,
                    "internal error: load failed without diagnostics".to_string(),
                )
            }
        }
    }
}

/// Retrieve the last per-engine error message.
///
/// Returns a pointer to a NUL-terminated string, or null if no error is
/// stored. This accessor may be called from any thread, including from a host
/// callback.
///
/// The pointer is borrowed from the engine and may be invalidated by the next
/// call to `ferric_engine_last_error` on the same engine or by freeing the
/// engine. Do not dereference or otherwise use the pointer while another
/// borrowed read or engine destruction may occur. Use
/// `ferric_engine_last_error_copy` when pointer-use windows could overlap.
///
/// # Safety
///
/// - `engine` must be a valid engine pointer or null.
#[cfg_attr(ferric_ffi_compile, ffi_export)]
#[no_mangle]
pub unsafe extern "C" fn ferric_engine_last_error(engine: *const FerricEngine) -> *const c_char {
    // Deliberately skip thread-affinity and active-call checks. Error snapshots
    // are synchronized separately and are safe to query reentrantly.
    let Ok(handle) = validate_engine_ptr(engine) else {
        return ptr::null();
    };

    let mut state = lock_unpoisoned(diagnostics(handle));
    let message = state.error_state.message().map(str::to_owned);
    match message {
        Some(msg) => {
            state.borrowed_cstring = Some(CString::new(msg).unwrap_or_default());
            state
                .borrowed_cstring
                .as_ref()
                .map_or(ptr::null(), |cs| cs.as_ptr())
        }
        None => ptr::null(),
    }
}

/// Copy the per-engine error message into a caller-provided buffer.
///
/// Same contract as `ferric_last_error_global_copy` but reads from the
/// per-engine error channel. This accessor may be called concurrently from
/// any thread and from a host callback. Each invocation observes and copies
/// one coherent error snapshot.
///
/// A size query and a later copy are separate snapshots. If the error changes
/// between those calls, the copy may return `BufferTooSmall` with the newer
/// required size; callers should resize and retry.
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
#[cfg_attr(ferric_ffi_compile, ffi_export)]
#[no_mangle]
pub unsafe extern "C" fn ferric_engine_last_error_copy(
    engine: *const FerricEngine,
    buf: *mut c_char,
    buf_len: usize,
    out_len: *mut usize,
) -> FerricError {
    let handle = match validate_engine_ptr(engine) {
        Ok(handle) => handle,
        Err(code) => {
            if !out_len.is_null() {
                *out_len = 0;
            }
            return code;
        }
    };
    if out_len.is_null() {
        return set_engine_error_for_handle(
            handle,
            FerricError::InvalidArgument,
            "out_len pointer is null".to_string(),
        );
    }
    let state = lock_unpoisoned(diagnostics(handle));
    copy_error_to_buffer(state.error_state.message(), buf, buf_len, out_len)
}

/// Clear the per-engine error state.
///
/// # Safety
///
/// - `engine` must be a valid engine pointer or null (null returns `NullPointer`).
#[cfg_attr(ferric_ffi_compile, ffi_export)]
#[no_mangle]
pub unsafe extern "C" fn ferric_engine_clear_error(engine: *mut FerricEngine) -> FerricError {
    let handle = match validate_engine_ptr(engine) {
        Ok(handle) => handle,
        Err(code) => return code,
    };
    if let Err(code) = check_thread_affinity(handle) {
        return code;
    }
    lock_unpoisoned(diagnostics(handle)).error_state.clear();
    FerricError::Ok
}

/// Reset the engine to its initial state.
///
/// # Safety
///
/// - `engine` must be a valid engine pointer.
#[cfg_attr(ferric_ffi_compile, ffi_export)]
#[no_mangle]
pub unsafe extern "C" fn ferric_engine_reset(engine: *mut FerricEngine) -> FerricError {
    let handle = match borrow_engine_mut(engine) {
        Ok(h) => h,
        Err(code) => return code,
    };

    match handle.engine.reset() {
        Ok(()) => {
            handle.output_cstrings.borrow_mut().clear();
            FerricError::Ok
        }
        Err(ref err) => set_engine_runtime_error(handle.diagnostics, err),
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
#[cfg_attr(ferric_ffi_compile, ffi_export)]
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
            prune_cleared_output_snapshots(handle.engine, handle.output_cstrings);
            if !out_fired.is_null() {
                *out_fired = result.rules_fired as u64;
            }
            FerricError::Ok
        }
        Err(ref err) => set_engine_runtime_error(handle.diagnostics, err),
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
#[cfg_attr(ferric_ffi_compile, ffi_export)]
#[no_mangle]
pub unsafe extern "C" fn ferric_engine_step(
    engine: *mut FerricEngine,
    out_status: *mut i32,
) -> FerricError {
    let handle = match borrow_engine_mut(engine) {
        Ok(h) => h,
        Err(code) => return code,
    };

    let result = handle.engine.step();
    if result.is_ok() {
        prune_cleared_output_snapshots(handle.engine, handle.output_cstrings);
    }

    match result {
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
        Err(ref err) => set_engine_runtime_error(handle.diagnostics, err),
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
#[cfg_attr(ferric_ffi_compile, ffi_export)]
#[no_mangle]
pub unsafe extern "C" fn ferric_engine_assert_string(
    engine: *mut FerricEngine,
    source: *const c_char,
    out_fact_id: *mut u64,
) -> FerricError {
    use slotmap::Key as _;

    let handle = match borrow_engine_mut(engine) {
        Ok(h) => h,
        Err(code) => return code,
    };
    let source_str = match engine_c_str_to_str(source, "source string", handle.diagnostics) {
        Ok(s) => s,
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
                set_engine_load_error(handle.diagnostics, first)
            } else {
                set_engine_error_message(
                    handle.diagnostics,
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
#[cfg_attr(ferric_ffi_compile, ffi_export)]
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
    let fid = ferric_rules_core::FactId::from(key_data);

    match handle.engine.retract(fid) {
        Ok(()) => FerricError::Ok,
        Err(ref err) => set_engine_runtime_error(handle.diagnostics, err),
    }
}

/// Get the engine's captured output for a named channel (e.g., `"stdout"`).
///
/// Returns a pointer to a NUL-terminated string, or null if the channel has
/// no output, the engine pointer is null, or the channel pointer is null.
/// The returned pointer is an engine-owned snapshot. It remains valid until a
/// later `ferric_engine_get_output` call for the same engine and channel
/// replaces that channel's snapshot; that channel is cleared; the engine is
/// reset or cleared; or the engine is destroyed. Calls involving another
/// engine never invalidate it. Output written after this call is not reflected
/// in the snapshot.
///
/// If the captured output contains embedded NUL, this legacy C-string accessor
/// returns null and records `InvalidArgument`. Use
/// `ferric_engine_get_output_copy` to preserve every byte.
///
/// Prefer `ferric_engine_get_output_copy` when retaining a borrowed pointer
/// would be inconvenient or when pointer-use windows could overlap.
///
/// # Safety
///
/// - `engine` must be a valid engine pointer or null.
/// - `channel` must be a valid NUL-terminated UTF-8 string or null.
#[cfg_attr(ferric_ffi_compile, ffi_export)]
#[no_mangle]
pub unsafe extern "C" fn ferric_engine_get_output(
    engine: *const FerricEngine,
    channel: *const c_char,
) -> *const c_char {
    let Ok(handle) = borrow_engine_checked(engine) else {
        return ptr::null();
    };
    let Ok(channel_str) = engine_c_str_to_str(channel, "channel", handle.diagnostics) else {
        return ptr::null();
    };

    match handle.engine.get_output(channel_str) {
        Some(output) if !output.is_empty() => {
            use std::collections::hash_map::Entry;

            if let Some(position) = output.as_bytes().iter().position(|byte| *byte == 0) {
                handle.output_cstrings.borrow_mut().remove(channel_str);
                set_engine_error_message(
                    handle.diagnostics,
                    FerricError::InvalidArgument,
                    format!(
                        "output channel {channel_str:?} contains embedded NUL at byte {position}; \
                         use ferric_engine_get_output_copy for length-aware access"
                    ),
                );
                return ptr::null();
            }

            let mut cache = handle.output_cstrings.borrow_mut();
            match cache.entry(channel_str.to_string()) {
                Entry::Occupied(mut entry) => {
                    if entry.get().snapshot != output {
                        let Ok(snapshot) = CachedOutputCString::new(output) else {
                            set_engine_error_message(
                                handle.diagnostics,
                                FerricError::InvalidArgument,
                                "captured output cannot be represented as a C string".to_string(),
                            );
                            return ptr::null();
                        };
                        entry.insert(snapshot);
                    }
                    entry.get().cstring.as_ptr()
                }
                Entry::Vacant(entry) => {
                    let Ok(snapshot) = CachedOutputCString::new(output) else {
                        set_engine_error_message(
                            handle.diagnostics,
                            FerricError::InvalidArgument,
                            "captured output cannot be represented as a C string".to_string(),
                        );
                        return ptr::null();
                    };
                    let slot = entry.insert(snapshot);
                    slot.cstring.as_ptr()
                }
            }
        }
        _ => ptr::null(),
    }
}

/// Copy the engine's captured output for a named channel.
///
/// This is the preferred output accessor for hosts that do not want to retain
/// an engine-owned pointer. `*out_len` always reports the full required byte
/// count including the trailing NUL when output exists. The copied payload
/// preserves embedded NUL; `*out_len`, not C-string scanning, is authoritative.
///
/// ## Contract
///
/// | Condition | Return | `*out_len` |
/// |-----------|--------|------------|
/// | `engine` is null | `NullPointer` | 0 |
/// | `channel` is null or invalid UTF-8 | `NullPointer` / `InvalidArgument` | 0 |
/// | `out_len` is null | `InvalidArgument` | (not written) |
/// | Channel has no non-empty output | `NotFound` | 0 |
/// | `buf` is null AND `buf_len` is 0 (size query) | `Ok` | required size (incl. NUL) |
/// | `buf` non-null, `buf_len` >= needed | `Ok` | bytes written (incl. NUL) |
/// | `buf` non-null, `buf_len` < needed | `BufferTooSmall` | full needed size (incl. NUL) |
///
/// An undersized non-empty buffer receives a NUL-terminated prefix and a
/// `BufferTooSmall` status; truncation is never reported as success.
///
/// # Safety
///
/// - `engine` must be a valid engine pointer or null.
/// - `channel` must be a valid NUL-terminated UTF-8 string or null.
/// - `buf` must point to `buf_len` writable bytes, or be null for a size query.
/// - `out_len` must be a valid non-null pointer.
#[cfg_attr(ferric_ffi_compile, ffi_export)]
#[no_mangle]
pub unsafe extern "C" fn ferric_engine_get_output_copy(
    engine: *const FerricEngine,
    channel: *const c_char,
    buf: *mut c_char,
    buf_len: usize,
    out_len: *mut usize,
) -> FerricError {
    let handle = match borrow_engine_checked(engine) {
        Ok(handle) => handle,
        Err(code) => {
            if !out_len.is_null() {
                *out_len = 0;
            }
            return code;
        }
    };
    if out_len.is_null() {
        return set_engine_error_message(
            handle.diagnostics,
            FerricError::InvalidArgument,
            "out_len pointer is null".to_string(),
        );
    }
    *out_len = 0;
    let channel_str = match engine_c_str_to_str(channel, "channel", handle.diagnostics) {
        Ok(channel) => channel,
        Err(code) => return code,
    };

    match handle.engine.get_output(channel_str) {
        Some(output) if !output.is_empty() => {
            copy_str_to_buffer(output, buf, buf_len, out_len, handle.diagnostics)
        }
        _ => set_engine_error_message(
            handle.diagnostics,
            FerricError::NotFound,
            format!("output channel has no captured output: {channel_str}"),
        ),
    }
}
/// Get the number of action diagnostics captured during recent execution.
///
/// Diagnostics are collected by `run`/`step` when action evaluation fails.
/// Such a failure stops the current activation and `run`, but does not
/// invalidate the engine.
///
/// # Safety
///
/// - `engine` must be a valid engine pointer.
/// - `out_count` must be a valid pointer.
#[cfg_attr(ferric_ffi_compile, ffi_export)]
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
        return set_engine_error_message(
            handle.diagnostics,
            FerricError::NullPointer,
            "out_count pointer is null".to_string(),
        );
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
#[cfg_attr(ferric_ffi_compile, ffi_export)]
#[no_mangle]
pub unsafe extern "C" fn ferric_engine_action_diagnostic_copy(
    engine: *const FerricEngine,
    index: usize,
    buf: *mut c_char,
    buf_len: usize,
    out_len: *mut usize,
) -> FerricError {
    let handle = match borrow_engine_checked(engine) {
        Ok(h) => h,
        Err(code) => {
            if !out_len.is_null() {
                *out_len = 0;
            }
            return code;
        }
    };
    if out_len.is_null() {
        return set_engine_error_message(
            handle.diagnostics,
            FerricError::InvalidArgument,
            "out_len pointer is null".to_string(),
        );
    }

    let message = handle
        .engine
        .action_diagnostics()
        .get(index)
        .map(ToString::to_string);
    let result = copy_error_to_buffer(message.as_deref(), buf, buf_len, out_len);
    match result {
        FerricError::Ok => FerricError::Ok,
        FerricError::NotFound => set_engine_error_message(
            handle.diagnostics,
            result,
            format!("action diagnostic index {index} not found"),
        ),
        FerricError::InvalidArgument => set_engine_error_message(
            handle.diagnostics,
            result,
            "non-zero buf_len with null buf".to_string(),
        ),
        FerricError::BufferTooSmall => set_engine_error_message(
            handle.diagnostics,
            result,
            format!(
                "output buffer is too small: need {} bytes, got {buf_len}",
                *out_len
            ),
        ),
        _ => result,
    }
}

/// Clear all stored action diagnostics.
///
/// # Safety
///
/// - `engine` must be a valid engine pointer or null (null returns `NullPointer`).
#[cfg_attr(ferric_ffi_compile, ffi_export)]
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
#[cfg_attr(ferric_ffi_compile, ffi_export)]
#[no_mangle]
pub unsafe extern "C" fn ferric_engine_fact_count(
    engine: *const FerricEngine,
    out_count: *mut usize,
) -> FerricError {
    let handle = match borrow_engine_checked(engine) {
        Ok(h) => h,
        Err(code) => return code,
    };
    if out_count.is_null() {
        return set_engine_error_message(
            handle.diagnostics,
            FerricError::NullPointer,
            "out_count pointer is null".to_string(),
        );
    }
    // facts() does its own thread check
    match handle.engine.facts() {
        Ok(iter) => {
            *out_count = iter.count();
            FerricError::Ok
        }
        Err(ref err) => set_engine_runtime_error(handle.diagnostics, err),
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
#[cfg_attr(ferric_ffi_compile, ffi_export)]
#[no_mangle]
pub unsafe extern "C" fn ferric_engine_get_fact_field_count(
    engine: *const FerricEngine,
    fact_id: u64,
    out_count: *mut usize,
) -> FerricError {
    let handle = match borrow_engine_checked(engine) {
        Ok(h) => h,
        Err(code) => return code,
    };
    if out_count.is_null() {
        return set_engine_error_message(
            handle.diagnostics,
            FerricError::NullPointer,
            "out_count pointer is null".to_string(),
        );
    }

    let key_data = slotmap::KeyData::from_ffi(fact_id);
    let fid = ferric_rules_core::FactId::from(key_data);

    match handle.engine.get_fact(fid) {
        Ok(Some(fact)) => {
            use ferric_rules_core::fact::Fact;
            *out_count = match fact {
                Fact::Ordered(o) => o.fields.len(),
                Fact::Template(t) => t.slots.len(),
            };
            FerricError::Ok
        }
        Ok(None) => set_engine_error_message(
            handle.diagnostics,
            FerricError::NotFound,
            format!("fact not found: {fact_id}"),
        ),
        Err(ref err) => set_engine_runtime_error(handle.diagnostics, err),
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
/// A Symbol/String containing embedded NUL (including inside a multifield)
/// cannot be represented by this legacy C-string value and returns
/// `InvalidArgument`; `*out_value` is left as Void.
///
/// # Safety
///
/// - `engine` must be a valid engine pointer.
/// - `out_value` must be a valid pointer to a `FerricValue`.
#[cfg_attr(ferric_ffi_compile, ffi_export)]
#[no_mangle]
pub unsafe extern "C" fn ferric_engine_get_fact_field(
    engine: *const FerricEngine,
    fact_id: u64,
    index: usize,
    out_value: *mut FerricValue,
) -> FerricError {
    let handle = match borrow_engine_checked(engine) {
        Ok(h) => h,
        Err(code) => return code,
    };
    if out_value.is_null() {
        return set_engine_error_message(
            handle.diagnostics,
            FerricError::NullPointer,
            "out_value pointer is null".to_string(),
        );
    }

    let key_data = slotmap::KeyData::from_ffi(fact_id);
    let fid = ferric_rules_core::FactId::from(key_data);

    match handle.engine.get_fact(fid) {
        Ok(Some(fact)) => {
            use ferric_rules_core::fact::Fact;
            let field_value = match fact {
                Fact::Ordered(o) => o.fields.get(index),
                Fact::Template(t) => t.slots.get(index),
            };
            if let Some(val) = field_value {
                write_value_to_ffi(val, handle.engine, out_value, handle.diagnostics)
            } else {
                set_engine_error_message(
                    handle.diagnostics,
                    FerricError::InvalidArgument,
                    format!("field index {index} out of bounds"),
                )
            }
        }
        Ok(None) => set_engine_error_message(
            handle.diagnostics,
            FerricError::NotFound,
            format!("fact not found: {fact_id}"),
        ),
        Err(ref err) => set_engine_runtime_error(handle.diagnostics, err),
    }
}

/// Get a global variable's value.
///
/// The name should NOT include the `?*` prefix/suffix — pass just the base name
/// (e.g., `"x"` for `?*x*`).
///
/// Module/global visibility resolution follows the runtime's standard rules.
/// Ambiguity and not-found conditions produce runtime-authored diagnostics.
/// A Symbol/String containing embedded NUL (including inside a multifield)
/// returns `InvalidArgument`, and `*out_value` is left as Void.
///
/// # Safety
///
/// - `engine` must be a valid engine pointer.
/// - `name` must be a valid NUL-terminated UTF-8 string.
/// - `out_value` must be a valid pointer to a `FerricValue`.
#[cfg_attr(ferric_ffi_compile, ffi_export)]
#[no_mangle]
pub unsafe extern "C" fn ferric_engine_get_global(
    engine: *const FerricEngine,
    name: *const c_char,
    out_value: *mut FerricValue,
) -> FerricError {
    let handle = match borrow_engine_checked(engine) {
        Ok(h) => h,
        Err(code) => return code,
    };
    let name_str = match engine_c_str_to_str(name, "name", handle.diagnostics) {
        Ok(s) => s,
        Err(code) => return code,
    };
    if out_value.is_null() {
        return set_engine_error_message(
            handle.diagnostics,
            FerricError::NullPointer,
            "out_value pointer is null".to_string(),
        );
    }

    if let Some(val) = handle.engine.get_global(name_str) {
        write_value_to_ffi(val, handle.engine, out_value, handle.diagnostics)
    } else {
        set_engine_error_message(
            handle.diagnostics,
            FerricError::NotFound,
            format!("global variable not found: {name_str}"),
        )
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
#[cfg_attr(ferric_ffi_compile, ffi_export)]
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
        return set_engine_error_message(
            handle.diagnostics,
            FerricError::NullPointer,
            "out_count pointer is null".to_string(),
        );
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
        Err(ref err) => set_engine_runtime_error(handle.diagnostics, err),
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
#[cfg_attr(ferric_ffi_compile, ffi_export)]
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
    let relation_str = match engine_c_str_to_str(relation, "relation", handle.diagnostics) {
        Ok(s) => s,
        Err(code) => return code,
    };
    if out_count.is_null() {
        return set_engine_error_message(
            handle.diagnostics,
            FerricError::NullPointer,
            "out_count pointer is null".to_string(),
        );
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
        Err(ref err) => set_engine_runtime_error(handle.diagnostics, err),
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
#[cfg_attr(ferric_ffi_compile, ffi_export)]
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
        return set_engine_error_message(
            handle.diagnostics,
            FerricError::NullPointer,
            "out_type pointer is null".to_string(),
        );
    }

    let key_data = slotmap::KeyData::from_ffi(fact_id);
    let fid = ferric_rules_core::FactId::from(key_data);

    match handle.engine.get_fact(fid) {
        Ok(Some(fact)) => {
            use ferric_rules_core::fact::Fact;
            *out_type = match fact {
                Fact::Ordered(_) => FerricFactType::Ordered,
                Fact::Template(_) => FerricFactType::Template,
            };
            FerricError::Ok
        }
        Ok(None) => set_engine_error_message(
            handle.diagnostics,
            FerricError::NotFound,
            format!("fact not found: {fact_id}"),
        ),
        Err(ref err) => set_engine_runtime_error(handle.diagnostics, err),
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
#[cfg_attr(ferric_ffi_compile, ffi_export)]
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
        return set_engine_error_message(
            handle.diagnostics,
            FerricError::NullPointer,
            "out_len pointer is null".to_string(),
        );
    }

    let key_data = slotmap::KeyData::from_ffi(fact_id);
    let fid = ferric_rules_core::FactId::from(key_data);

    match handle.engine.get_fact(fid) {
        Ok(Some(fact)) => {
            use ferric_rules_core::fact::Fact;
            match fact {
                Fact::Ordered(o) => {
                    let name = handle
                        .engine
                        .resolve_symbol(o.relation)
                        .unwrap_or("<unknown>");
                    copy_str_to_buffer(name, buf, buf_len, out_len, handle.diagnostics)
                }
                Fact::Template(_) => set_engine_error_message(
                    handle.diagnostics,
                    FerricError::InvalidArgument,
                    "fact is a template fact, not an ordered fact".to_string(),
                ),
            }
        }
        Ok(None) => set_engine_error_message(
            handle.diagnostics,
            FerricError::NotFound,
            format!("fact not found: {fact_id}"),
        ),
        Err(ref err) => set_engine_runtime_error(handle.diagnostics, err),
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
#[cfg_attr(ferric_ffi_compile, ffi_export)]
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
        return set_engine_error_message(
            handle.diagnostics,
            FerricError::NullPointer,
            "out_len pointer is null".to_string(),
        );
    }

    let key_data = slotmap::KeyData::from_ffi(fact_id);
    let fid = ferric_rules_core::FactId::from(key_data);

    match handle.engine.get_fact(fid) {
        Ok(Some(fact)) => {
            use ferric_rules_core::fact::Fact;
            match fact {
                Fact::Template(t) => {
                    let name = handle
                        .engine
                        .template_name_by_id(t.template_id)
                        .unwrap_or("<unknown>");
                    copy_str_to_buffer(name, buf, buf_len, out_len, handle.diagnostics)
                }
                Fact::Ordered(_) => set_engine_error_message(
                    handle.diagnostics,
                    FerricError::InvalidArgument,
                    "fact is an ordered fact, not a template fact".to_string(),
                ),
            }
        }
        Ok(None) => set_engine_error_message(
            handle.diagnostics,
            FerricError::NotFound,
            format!("fact not found: {fact_id}"),
        ),
        Err(ref err) => set_engine_runtime_error(handle.diagnostics, err),
    }
}

// ---------------------------------------------------------------------------
// C API: Structured assertion
// ---------------------------------------------------------------------------

/// Assert an ordered fact from structured values, bypassing CLIPS source parsing.
///
/// `fields` and every active string or multifield reachable from it are
/// borrowed for the duration of this call. Ferric converts the values into
/// engine-owned runtime data and never retains or frees caller storage. The
/// complete input tree must remain readable and unchanged until the call
/// returns.
///
/// # Safety
///
/// - `engine` must be a valid engine pointer.
/// - `relation` must be a valid NUL-terminated string.
/// - If `fields` is non-null, it must point to `field_count` valid `FerricValue`s.
/// - `out_fact_id` may be null.
#[cfg_attr(ferric_ffi_compile, ffi_export)]
#[no_mangle]
pub unsafe extern "C" fn ferric_engine_assert_ordered(
    engine: *mut FerricEngine,
    relation: *const c_char,
    fields: *const FerricValue,
    field_count: usize,
    out_fact_id: *mut u64,
) -> FerricError {
    use slotmap::Key as _;

    let handle = match borrow_engine_mut(engine) {
        Ok(h) => h,
        Err(code) => return code,
    };
    let relation_str = match engine_c_str_to_str(relation, "relation", handle.diagnostics) {
        Ok(s) => s,
        Err(code) => return code,
    };
    if fields.is_null() && field_count > 0 {
        return set_engine_error_message(
            handle.diagnostics,
            FerricError::NullPointer,
            "fields pointer is null with non-zero field_count".to_string(),
        );
    }

    // Convert FerricValue array to Vec<Value>
    let mut values = Vec::with_capacity(field_count);
    for i in 0..field_count {
        let fv = &*fields.add(i);
        match ferric_to_value(fv, handle.engine) {
            Ok(v) => values.push(v),
            Err(msg) => {
                return set_engine_error_message(
                    handle.diagnostics,
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
        Err(ref err) => set_engine_runtime_error(handle.diagnostics, err),
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
#[cfg_attr(ferric_ffi_compile, ffi_export)]
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
        return set_engine_error_message(
            handle.diagnostics,
            FerricError::NullPointer,
            "out_count pointer is null".to_string(),
        );
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
#[cfg_attr(ferric_ffi_compile, ffi_export)]
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
        return set_engine_error_message(
            handle.diagnostics,
            FerricError::NullPointer,
            "out_len pointer is null".to_string(),
        );
    }

    let templates = handle.engine.templates();
    if index >= templates.len() {
        return set_engine_error_message(
            handle.diagnostics,
            FerricError::InvalidArgument,
            format!(
                "template index {index} out of bounds (count: {})",
                templates.len()
            ),
        );
    }
    copy_str_to_buffer(templates[index], buf, buf_len, out_len, handle.diagnostics)
}

/// Get the number of slots in a named template.
///
/// # Safety
///
/// - `engine` must be a valid engine pointer.
/// - `template_name` must be a valid NUL-terminated string.
/// - `out_count` must be a valid pointer.
#[cfg_attr(ferric_ffi_compile, ffi_export)]
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
    let name_str = match engine_c_str_to_str(template_name, "template_name", handle.diagnostics) {
        Ok(s) => s,
        Err(code) => return code,
    };
    if out_count.is_null() {
        return set_engine_error_message(
            handle.diagnostics,
            FerricError::NullPointer,
            "out_count pointer is null".to_string(),
        );
    }

    if let Some(slots) = handle.engine.template_slot_names(name_str) {
        *out_count = slots.len();
        FerricError::Ok
    } else {
        set_engine_error_message(
            handle.diagnostics,
            FerricError::NotFound,
            format!("template not found: {name_str}"),
        )
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
#[cfg_attr(ferric_ffi_compile, ffi_export)]
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
    let name_str = match engine_c_str_to_str(template_name, "template_name", handle.diagnostics) {
        Ok(s) => s,
        Err(code) => return code,
    };
    if out_len.is_null() {
        return set_engine_error_message(
            handle.diagnostics,
            FerricError::NullPointer,
            "out_len pointer is null".to_string(),
        );
    }

    if let Some(slots) = handle.engine.template_slot_names(name_str) {
        if slot_index >= slots.len() {
            return set_engine_error_message(
                handle.diagnostics,
                FerricError::InvalidArgument,
                format!(
                    "slot index {slot_index} out of bounds (count: {})",
                    slots.len()
                ),
            );
        }
        copy_str_to_buffer(slots[slot_index], buf, buf_len, out_len, handle.diagnostics)
    } else {
        set_engine_error_message(
            handle.diagnostics,
            FerricError::NotFound,
            format!("template not found: {name_str}"),
        )
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
#[cfg_attr(ferric_ffi_compile, ffi_export)]
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
        return set_engine_error_message(
            handle.diagnostics,
            FerricError::NullPointer,
            "out_count pointer is null".to_string(),
        );
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
#[cfg_attr(ferric_ffi_compile, ffi_export)]
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
        return set_engine_error_message(
            handle.diagnostics,
            FerricError::NullPointer,
            "out_len pointer is null".to_string(),
        );
    }

    let rules = handle.engine.rules();
    if index >= rules.len() {
        return set_engine_error_message(
            handle.diagnostics,
            FerricError::InvalidArgument,
            format!("rule index {index} out of bounds (count: {})", rules.len()),
        );
    }
    let (name, salience) = rules[index];
    if !out_salience.is_null() {
        *out_salience = salience;
    }
    copy_str_to_buffer(name, buf, buf_len, out_len, handle.diagnostics)
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
#[cfg_attr(ferric_ffi_compile, ffi_export)]
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
        return set_engine_error_message(
            handle.diagnostics,
            FerricError::NullPointer,
            "out_len pointer is null".to_string(),
        );
    }
    let name = handle.engine.current_module();
    copy_str_to_buffer(name, buf, buf_len, out_len, handle.diagnostics)
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
#[cfg_attr(ferric_ffi_compile, ffi_export)]
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
        return set_engine_error_message(
            handle.diagnostics,
            FerricError::NullPointer,
            "out_len pointer is null".to_string(),
        );
    }

    if let Some(name) = handle.engine.get_focus() {
        copy_str_to_buffer(name, buf, buf_len, out_len, handle.diagnostics)
    } else {
        *out_len = 0;
        set_engine_error_message(
            handle.diagnostics,
            FerricError::NotFound,
            "focus stack is empty".to_string(),
        )
    }
}

/// Get the depth of the focus stack.
///
/// # Safety
///
/// - `engine` must be a valid engine pointer.
/// - `out_depth` must be a valid pointer.
#[cfg_attr(ferric_ffi_compile, ffi_export)]
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
        return set_engine_error_message(
            handle.diagnostics,
            FerricError::NullPointer,
            "out_depth pointer is null".to_string(),
        );
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
#[cfg_attr(ferric_ffi_compile, ffi_export)]
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
        return set_engine_error_message(
            handle.diagnostics,
            FerricError::NullPointer,
            "out_len pointer is null".to_string(),
        );
    }

    let stack = handle.engine.get_focus_stack();
    if index >= stack.len() {
        return set_engine_error_message(
            handle.diagnostics,
            FerricError::InvalidArgument,
            format!(
                "focus stack index {index} out of bounds (depth: {})",
                stack.len()
            ),
        );
    }
    copy_str_to_buffer(stack[index], buf, buf_len, out_len, handle.diagnostics)
}

/// Get the number of registered modules.
///
/// # Safety
///
/// - `engine` must be a valid engine pointer.
/// - `out_count` must be a valid pointer.
#[cfg_attr(ferric_ffi_compile, ffi_export)]
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
        return set_engine_error_message(
            handle.diagnostics,
            FerricError::NullPointer,
            "out_count pointer is null".to_string(),
        );
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
#[cfg_attr(ferric_ffi_compile, ffi_export)]
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
        return set_engine_error_message(
            handle.diagnostics,
            FerricError::NullPointer,
            "out_len pointer is null".to_string(),
        );
    }

    let modules = handle.engine.modules();
    if index >= modules.len() {
        return set_engine_error_message(
            handle.diagnostics,
            FerricError::InvalidArgument,
            format!(
                "module index {index} out of bounds (count: {})",
                modules.len()
            ),
        );
    }
    copy_str_to_buffer(modules[index], buf, buf_len, out_len, handle.diagnostics)
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
#[cfg_attr(ferric_ffi_compile, ffi_export)]
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
        return set_engine_error_message(
            handle.diagnostics,
            FerricError::NullPointer,
            "out_count pointer is null".to_string(),
        );
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
#[cfg_attr(ferric_ffi_compile, ffi_export)]
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
        return set_engine_error_message(
            handle.diagnostics,
            FerricError::NullPointer,
            "out_halted pointer is null".to_string(),
        );
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
#[cfg_attr(ferric_ffi_compile, ffi_export)]
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
#[cfg_attr(ferric_ffi_compile, ffi_export)]
#[no_mangle]
pub unsafe extern "C" fn ferric_engine_push_input(
    engine: *mut FerricEngine,
    line: *const c_char,
) -> FerricError {
    let handle = match borrow_engine_mut(engine) {
        Ok(h) => h,
        Err(code) => return code,
    };
    let line_str = match engine_c_str_to_str(line, "line", handle.diagnostics) {
        Ok(s) => s,
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
#[cfg_attr(ferric_ffi_compile, ffi_export)]
#[no_mangle]
pub unsafe extern "C" fn ferric_engine_clear(engine: *mut FerricEngine) -> FerricError {
    let handle = match borrow_engine_mut(engine) {
        Ok(h) => h,
        Err(code) => return code,
    };
    handle.engine.clear();
    handle.output_cstrings.borrow_mut().clear();
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
#[cfg_attr(ferric_ffi_compile, ffi_export)]
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
#[cfg_attr(ferric_ffi_compile, ffi_export)]
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
        Ok(engine) => Box::into_raw(Box::new(FerricEngine::new(engine))),
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
#[cfg_attr(ferric_ffi_compile, ffi_export)]
#[no_mangle]
pub unsafe extern "C" fn ferric_engine_clear_output(
    engine: *mut FerricEngine,
    channel: *const c_char,
) -> FerricError {
    let handle = match borrow_engine_mut(engine) {
        Ok(h) => h,
        Err(code) => return code,
    };
    let channel_str = match engine_c_str_to_str(channel, "channel", handle.diagnostics) {
        Ok(s) => s,
        Err(code) => return code,
    };
    handle.engine.clear_output_channel(channel_str);
    handle.output_cstrings.borrow_mut().remove(channel_str);
    FerricError::Ok
}

/// Extended run with halt reason output.
///
/// Always starts a fresh logical run, clearing any prior halt request and
/// action diagnostics without resetting working memory, the agenda, globals,
/// or output. Same limit semantics as `ferric_engine_run` (negative =
/// unlimited). Additionally writes the halt reason to `*out_reason` if
/// non-null.
///
/// A successful `LimitReached` result may be continued with
/// `ferric_engine_continue_run_ex`. Other halt reasons are terminal.
///
/// # Safety
///
/// - `engine` must be a valid engine pointer.
/// - `out_fired` may be null.
/// - `out_reason` may be null.
#[cfg_attr(ferric_ffi_compile, ffi_export)]
#[no_mangle]
pub unsafe extern "C" fn ferric_engine_run_ex(
    engine: *mut FerricEngine,
    limit: i64,
    out_fired: *mut u64,
    out_reason: *mut FerricHaltReason,
) -> FerricError {
    // `borrow_engine_mut` already ends any in-flight logical run.
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
            let reason = FerricHaltReason::from(result.halt_reason);
            handle
                .logical_run_continuation_ready
                .set(reason == FerricHaltReason::LimitReached);
            prune_cleared_output_snapshots(handle.engine, handle.output_cstrings);
            if !out_fired.is_null() {
                *out_fired = result.rules_fired as u64;
            }
            if !out_reason.is_null() {
                *out_reason = reason;
            }
            FerricError::Ok
        }
        Err(ref err) => set_engine_runtime_error(handle.diagnostics, err),
    }
}

/// Continue a logical run previously started by `ferric_engine_run_ex`.
///
/// Call this only after `ferric_engine_run_ex` or a previous continuation
/// returned `FerricHaltReason::LimitReached`. Unlike a fresh run, continuation
/// preserves a pending halt request and action diagnostics from earlier chunks.
///
/// Each successful call writes the number of rules fired by that chunk, not a
/// cumulative total. Hosts should accumulate `out_fired` across chunks. A
/// result other than `LimitReached` is terminal for the logical run.
///
/// Read-only raw-engine queries and `ferric_engine_clear_error` may be called
/// between chunks. Any other raw-engine call that reaches engine state ends the
/// current logical run — whether or not it then succeeds — and a later
/// continuation attempt returns `FerricError::InvalidArgument`.
///
/// A call rejected before it reaches engine state changes nothing, including
/// continuation eligibility: a null handle, a thread-affinity violation, and a
/// reentrant call from a host callback all leave the logical run intact for the
/// owner thread to continue. On any error, output parameters are left
/// unchanged.
///
/// Host cancellation is distinct from an engine halt: stop calling this
/// function, report cancellation in the host API, and use
/// `ferric_engine_run_ex` to start the next logical run.
///
/// # Safety
///
/// - `engine` must be a valid engine pointer.
/// - `out_fired` may be null.
/// - `out_reason` may be null.
#[cfg_attr(ferric_ffi_compile, ffi_export)]
#[no_mangle]
pub unsafe extern "C" fn ferric_engine_continue_run_ex(
    engine: *mut FerricEngine,
    limit: i64,
    out_fired: *mut u64,
    out_reason: *mut FerricHaltReason,
) -> FerricError {
    let handle = match borrow_engine_mut_preserving_logical_run(engine) {
        Ok(h) => h,
        Err(code) => return code,
    };

    if !handle.logical_run_continuation_ready.get() {
        return set_engine_error_message(
            handle.diagnostics,
            FerricError::InvalidArgument,
            "ferric_engine_continue_run_ex requires a preceding logical-run chunk that returned LimitReached"
                .to_string(),
        );
    }
    handle.logical_run_continuation_ready.set(false);

    let run_limit = if limit < 0 {
        RunLimit::Unlimited
    } else {
        #[allow(clippy::cast_sign_loss, clippy::cast_possible_truncation)]
        RunLimit::Count(limit as usize)
    };

    match handle.engine.continue_run(run_limit) {
        Ok(result) => {
            let reason = FerricHaltReason::from(result.halt_reason);
            handle
                .logical_run_continuation_ready
                .set(reason == FerricHaltReason::LimitReached);
            prune_cleared_output_snapshots(handle.engine, handle.output_cstrings);
            if !out_fired.is_null() {
                *out_fired = result.rules_fired as u64;
            }
            if !out_reason.is_null() {
                *out_reason = reason;
            }
            FerricError::Ok
        }
        Err(ref err) => set_engine_runtime_error(handle.diagnostics, err),
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
/// Both arrays and every active string or multifield reachable from the slot
/// values are borrowed for the duration of this call. Ferric never retains or
/// frees caller storage. The complete input tree must remain readable and
/// unchanged until the call returns.
///
/// # Safety
///
/// - `engine` must be a valid engine pointer.
/// - `template_name` must be a valid NUL-terminated string.
/// - If `count > 0`, `slot_names` must point to `count` valid NUL-terminated string pointers.
/// - If `count > 0`, `slot_values` must point to `count` valid `FerricValue`s.
/// - `out_fact_id` may be null.
#[cfg_attr(ferric_ffi_compile, ffi_export)]
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

    let handle = match borrow_engine_mut(engine) {
        Ok(h) => h,
        Err(code) => return code,
    };
    let tmpl_str = match engine_c_str_to_str(template_name, "template_name", handle.diagnostics) {
        Ok(s) => s,
        Err(code) => return code,
    };

    if count > 0 {
        if slot_names.is_null() {
            return set_engine_error_message(
                handle.diagnostics,
                FerricError::NullPointer,
                "slot_names pointer is null with non-zero count".to_string(),
            );
        }
        if slot_values.is_null() {
            return set_engine_error_message(
                handle.diagnostics,
                FerricError::NullPointer,
                "slot_values pointer is null with non-zero count".to_string(),
            );
        }
    }

    // Convert slot names from C strings.
    let mut names = Vec::with_capacity(count);
    for i in 0..count {
        let name_ptr = *slot_names.add(i);
        match engine_c_str_to_str(name_ptr, &format!("slot_names[{i}]"), handle.diagnostics) {
            Ok(s) => names.push(s),
            Err(code) => return code,
        }
    }

    // Convert slot values from FerricValue.
    let mut values = Vec::with_capacity(count);
    for i in 0..count {
        let fv = &*slot_values.add(i);
        match ferric_to_value(fv, handle.engine) {
            Ok(v) => values.push(v),
            Err(msg) => {
                return set_engine_error_message(
                    handle.diagnostics,
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
        Err(ref err) => set_engine_runtime_error(handle.diagnostics, err),
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
/// A Symbol/String containing embedded NUL (including inside a multifield)
/// returns `InvalidArgument`, and `*out_value` is left as Void.
///
/// # Safety
///
/// - `engine` must be a valid engine pointer.
/// - `slot_name` must be a valid NUL-terminated string.
/// - `out_value` must be a valid pointer to a `FerricValue`.
#[cfg_attr(ferric_ffi_compile, ffi_export)]
#[no_mangle]
pub unsafe extern "C" fn ferric_engine_get_fact_slot_by_name(
    engine: *const FerricEngine,
    fact_id: u64,
    slot_name: *const c_char,
    out_value: *mut FerricValue,
) -> FerricError {
    let handle = match borrow_engine_checked(engine) {
        Ok(h) => h,
        Err(code) => return code,
    };
    let name_str = match engine_c_str_to_str(slot_name, "slot_name", handle.diagnostics) {
        Ok(s) => s,
        Err(code) => return code,
    };

    if out_value.is_null() {
        return set_engine_error_message(
            handle.diagnostics,
            FerricError::NullPointer,
            "out_value pointer is null".to_string(),
        );
    }

    let key_data = slotmap::KeyData::from_ffi(fact_id);
    let fid = ferric_rules_core::FactId::from(key_data);

    match handle.engine.get_fact_slot_by_name(fid, name_str) {
        Ok(value) => write_value_to_ffi(value, handle.engine, out_value, handle.diagnostics),
        Err(ref err) => set_engine_runtime_error(handle.diagnostics, err),
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
#[cfg_attr(ferric_ffi_compile, ffi_export(global_only))]
#[no_mangle]
pub unsafe extern "C" fn ferric_engine_free_unchecked(engine: *mut FerricEngine) -> FerricError {
    if engine.is_null() {
        return FerricError::Ok;
    }
    let handle = match validate_engine_ptr(engine) {
        Ok(handle) => handle,
        Err(code) => return code,
    };
    let guard = match enter_engine_call(handle) {
        Ok(guard) => guard,
        Err(code) => return code,
    };
    guard.disarm();
    drop(Box::from_raw(engine));
    FerricError::Ok
}

// ---------------------------------------------------------------------------
// Engine serialization / deserialization
// ---------------------------------------------------------------------------

/// Serialization format selector for `ferric_engine_serialize_as` and
/// `ferric_engine_deserialize_as`.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg(feature = "serde")]
pub enum FerricSerializationFormat {
    /// Compact binary (bincode). Fast and small.
    Bincode = 0,
    /// JSON (human-readable, larger output).
    Json = 1,
    /// CBOR (Concise Binary Object Representation).
    Cbor = 2,
    /// `MessagePack` (compact binary, JSON-like schema).
    MessagePack = 3,
    /// Postcard (compact, `no_std`-friendly binary).
    Postcard = 4,
}

#[cfg(feature = "serde")]
impl FerricSerializationFormat {
    /// Try to convert a raw C integer to a valid format variant.
    /// Returns `None` for out-of-range discriminants.
    pub(crate) fn from_raw(raw: u32) -> Option<Self> {
        match raw {
            0 => Some(Self::Bincode),
            1 => Some(Self::Json),
            2 => Some(Self::Cbor),
            3 => Some(Self::MessagePack),
            4 => Some(Self::Postcard),
            _ => None,
        }
    }

    pub(crate) fn to_runtime(self) -> ferric_rules_runtime::SerializationFormat {
        match self {
            Self::Bincode => ferric_rules_runtime::SerializationFormat::Bincode,
            Self::Json => ferric_rules_runtime::SerializationFormat::Json,
            Self::Cbor => ferric_rules_runtime::SerializationFormat::Cbor,
            Self::MessagePack => ferric_rules_runtime::SerializationFormat::MessagePack,
            Self::Postcard => ferric_rules_runtime::SerializationFormat::Postcard,
        }
    }
}

/// Callback type for caller-controlled memory allocation.
///
/// When non-null, called by serialization functions with the exact byte
/// count needed. The `context` parameter is passed through unchanged from
/// the serialize call.
///
/// Must return a pointer to at least `size` writable bytes, or null to
/// signal allocation failure.
///
/// The callback may query the same raw engine's last-error channel. Other
/// same-engine runtime calls are rejected with `InternalError` while
/// serialization is active, and the callback must not free the engine.
#[cfg(feature = "serde")]
pub type FerricAllocFn =
    Option<unsafe extern "C" fn(size: usize, context: *mut std::ffi::c_void) -> *mut u8>;

/// Internal helper: serialize an engine in the given format, writing output
/// through either a caller-provided allocator or Rust allocation.
#[cfg(feature = "serde")]
unsafe fn serialize_engine_impl(
    engine: *const FerricEngine,
    format: ferric_rules_runtime::SerializationFormat,
    alloc_fn: FerricAllocFn,
    alloc_context: *mut std::ffi::c_void,
    out_data: *mut *mut u8,
    out_len: *mut usize,
) -> FerricError {
    let validated = match validate_engine_ptr(engine) {
        Ok(handle) => handle,
        Err(code) => return code,
    };

    // Validate output pointers
    if out_data.is_null() || out_len.is_null() {
        return set_engine_error_for_handle(
            validated,
            FerricError::NullPointer,
            "out_data and out_len must be non-null".to_string(),
        );
    }

    // Validate engine and check thread affinity
    let handle = match borrow_engine_checked(engine) {
        Ok(h) => h,
        Err(code) => return code,
    };

    // Serialize to internal Vec<u8>
    let bytes = match handle.engine.serialize(format) {
        Ok(b) => b,
        Err(e) => {
            return set_engine_error_message(
                handle.diagnostics,
                FerricError::SerializationError,
                e.to_string(),
            );
        }
    };

    let len = bytes.len();

    if let Some(alloc) = alloc_fn {
        // Caller-provided allocator path
        let buf = alloc(len, alloc_context);
        if buf.is_null() {
            return set_engine_error_message(
                handle.diagnostics,
                FerricError::SerializationError,
                "caller allocator returned null".to_string(),
            );
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

/// Internal helper: deserialize an engine from bytes in the given format.
#[cfg(feature = "serde")]
unsafe fn deserialize_engine_impl(
    data: *const u8,
    len: usize,
    format: ferric_rules_runtime::SerializationFormat,
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

    let engine = match ferric_rules_runtime::Engine::deserialize(slice, format) {
        Ok(e) => e,
        Err(e) => {
            set_global_error(e.to_string());
            return FerricError::SerializationError;
        }
    };

    *out_engine = Box::into_raw(Box::new(FerricEngine::new(engine)));
    FerricError::Ok
}

// ── Omnibus format-parameterized API ─────────────────────────────────────

/// Serialize engine state to bytes in the specified format.
///
/// `format` is a `u32` corresponding to `FerricSerializationFormat` discriminants
/// (0 = Bincode, 1 = JSON, 2 = CBOR, 3 = `MessagePack`, 4 = Postcard).
/// Returns `FERRIC_ERROR_INVALID_ARGUMENT` for out-of-range values.
///
/// See `ferric_engine_serialize_bincode` for memory allocation details.
///
/// # Safety
///
/// - `engine` must be a valid engine pointer.
/// - `out_data` and `out_len` must be valid, non-null pointers.
/// - If `alloc_fn` is non-null, it must return a valid pointer to `size` bytes
///   (or null to signal failure).
#[cfg_attr(ferric_ffi_compile, ffi_export)]
#[no_mangle]
#[cfg(feature = "serde")]
pub unsafe extern "C" fn ferric_engine_serialize_as(
    engine: *const FerricEngine,
    format: u32,
    alloc_fn: FerricAllocFn,
    alloc_context: *mut std::ffi::c_void,
    out_data: *mut *mut u8,
    out_len: *mut usize,
) -> FerricError {
    let Some(fmt) = FerricSerializationFormat::from_raw(format) else {
        return match validate_engine_ptr(engine) {
            Ok(handle) => set_engine_error_for_handle(
                handle,
                FerricError::InvalidArgument,
                format!("invalid serialization format: {format}"),
            ),
            Err(code) => code,
        };
    };
    serialize_engine_impl(
        engine,
        fmt.to_runtime(),
        alloc_fn,
        alloc_context,
        out_data,
        out_len,
    )
}

/// Deserialize an engine from bytes in the specified format.
///
/// `format` is a `u32` corresponding to `FerricSerializationFormat` discriminants
/// (0 = Bincode, 1 = JSON, 2 = CBOR, 3 = `MessagePack`, 4 = Postcard).
/// Returns `FERRIC_ERROR_INVALID_ARGUMENT` for out-of-range values.
///
/// See `ferric_engine_deserialize_bincode` for details.
///
/// # Safety
///
/// - `data` must point to `len` valid, readable bytes.
/// - `out_engine` must be a valid, non-null pointer.
/// - The returned engine must be freed with `ferric_engine_free`.
#[cfg_attr(ferric_ffi_compile, ffi_export)]
#[no_mangle]
#[cfg(feature = "serde")]
pub unsafe extern "C" fn ferric_engine_deserialize_as(
    data: *const u8,
    len: usize,
    format: u32,
    out_engine: *mut *mut FerricEngine,
) -> FerricError {
    let Some(fmt) = FerricSerializationFormat::from_raw(format) else {
        set_global_error(format!("invalid serialization format: {format}"));
        return FerricError::InvalidArgument;
    };
    deserialize_engine_impl(data, len, fmt.to_runtime(), out_engine)
}

// ── Per-format convenience functions ─────────────────────────────────────

/// Serialize engine state to bincode.
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
#[cfg_attr(ferric_ffi_compile, ffi_export)]
#[no_mangle]
#[cfg(feature = "serde")]
pub unsafe extern "C" fn ferric_engine_serialize_bincode(
    engine: *const FerricEngine,
    alloc_fn: FerricAllocFn,
    alloc_context: *mut std::ffi::c_void,
    out_data: *mut *mut u8,
    out_len: *mut usize,
) -> FerricError {
    serialize_engine_impl(
        engine,
        ferric_rules_runtime::SerializationFormat::Bincode,
        alloc_fn,
        alloc_context,
        out_data,
        out_len,
    )
}

/// Deserialize an engine from bincode bytes.
///
/// The returned engine handle is ready for use (e.g. `ferric_engine_run`).
/// Its thread affinity is set to the calling thread.
///
/// # Safety
///
/// - `data` must point to `len` valid, readable bytes.
/// - `out_engine` must be a valid, non-null pointer.
/// - The returned engine must be freed with `ferric_engine_free`.
#[cfg_attr(ferric_ffi_compile, ffi_export)]
#[no_mangle]
#[cfg(feature = "serde")]
pub unsafe extern "C" fn ferric_engine_deserialize_bincode(
    data: *const u8,
    len: usize,
    out_engine: *mut *mut FerricEngine,
) -> FerricError {
    deserialize_engine_impl(
        data,
        len,
        ferric_rules_runtime::SerializationFormat::Bincode,
        out_engine,
    )
}

/// Serialize engine state to JSON.
///
/// See `ferric_engine_serialize_bincode` for memory allocation details.
///
/// # Safety
///
/// Same safety requirements as `ferric_engine_serialize_bincode`.
#[cfg_attr(ferric_ffi_compile, ffi_export)]
#[no_mangle]
#[cfg(feature = "serde")]
pub unsafe extern "C" fn ferric_engine_serialize_json(
    engine: *const FerricEngine,
    alloc_fn: FerricAllocFn,
    alloc_context: *mut std::ffi::c_void,
    out_data: *mut *mut u8,
    out_len: *mut usize,
) -> FerricError {
    serialize_engine_impl(
        engine,
        ferric_rules_runtime::SerializationFormat::Json,
        alloc_fn,
        alloc_context,
        out_data,
        out_len,
    )
}

/// Deserialize an engine from JSON bytes.
///
/// # Safety
///
/// Same safety requirements as `ferric_engine_deserialize_bincode`.
#[cfg_attr(ferric_ffi_compile, ffi_export)]
#[no_mangle]
#[cfg(feature = "serde")]
pub unsafe extern "C" fn ferric_engine_deserialize_json(
    data: *const u8,
    len: usize,
    out_engine: *mut *mut FerricEngine,
) -> FerricError {
    deserialize_engine_impl(
        data,
        len,
        ferric_rules_runtime::SerializationFormat::Json,
        out_engine,
    )
}

/// Serialize engine state to CBOR.
///
/// See `ferric_engine_serialize_bincode` for memory allocation details.
///
/// # Safety
///
/// Same safety requirements as `ferric_engine_serialize_bincode`.
#[cfg_attr(ferric_ffi_compile, ffi_export)]
#[no_mangle]
#[cfg(feature = "serde")]
pub unsafe extern "C" fn ferric_engine_serialize_cbor(
    engine: *const FerricEngine,
    alloc_fn: FerricAllocFn,
    alloc_context: *mut std::ffi::c_void,
    out_data: *mut *mut u8,
    out_len: *mut usize,
) -> FerricError {
    serialize_engine_impl(
        engine,
        ferric_rules_runtime::SerializationFormat::Cbor,
        alloc_fn,
        alloc_context,
        out_data,
        out_len,
    )
}

/// Deserialize an engine from CBOR bytes.
///
/// # Safety
///
/// Same safety requirements as `ferric_engine_deserialize_bincode`.
#[cfg_attr(ferric_ffi_compile, ffi_export)]
#[no_mangle]
#[cfg(feature = "serde")]
pub unsafe extern "C" fn ferric_engine_deserialize_cbor(
    data: *const u8,
    len: usize,
    out_engine: *mut *mut FerricEngine,
) -> FerricError {
    deserialize_engine_impl(
        data,
        len,
        ferric_rules_runtime::SerializationFormat::Cbor,
        out_engine,
    )
}

/// Serialize engine state to `MessagePack`.
///
/// See `ferric_engine_serialize_bincode` for memory allocation details.
///
/// # Safety
///
/// Same safety requirements as `ferric_engine_serialize_bincode`.
#[cfg_attr(ferric_ffi_compile, ffi_export)]
#[no_mangle]
#[cfg(feature = "serde")]
pub unsafe extern "C" fn ferric_engine_serialize_msgpack(
    engine: *const FerricEngine,
    alloc_fn: FerricAllocFn,
    alloc_context: *mut std::ffi::c_void,
    out_data: *mut *mut u8,
    out_len: *mut usize,
) -> FerricError {
    serialize_engine_impl(
        engine,
        ferric_rules_runtime::SerializationFormat::MessagePack,
        alloc_fn,
        alloc_context,
        out_data,
        out_len,
    )
}

/// Deserialize an engine from `MessagePack` bytes.
///
/// # Safety
///
/// Same safety requirements as `ferric_engine_deserialize_bincode`.
#[cfg_attr(ferric_ffi_compile, ffi_export)]
#[no_mangle]
#[cfg(feature = "serde")]
pub unsafe extern "C" fn ferric_engine_deserialize_msgpack(
    data: *const u8,
    len: usize,
    out_engine: *mut *mut FerricEngine,
) -> FerricError {
    deserialize_engine_impl(
        data,
        len,
        ferric_rules_runtime::SerializationFormat::MessagePack,
        out_engine,
    )
}

/// Serialize engine state to Postcard.
///
/// See `ferric_engine_serialize_bincode` for memory allocation details.
///
/// # Safety
///
/// Same safety requirements as `ferric_engine_serialize_bincode`.
#[cfg_attr(ferric_ffi_compile, ffi_export)]
#[no_mangle]
#[cfg(feature = "serde")]
pub unsafe extern "C" fn ferric_engine_serialize_postcard(
    engine: *const FerricEngine,
    alloc_fn: FerricAllocFn,
    alloc_context: *mut std::ffi::c_void,
    out_data: *mut *mut u8,
    out_len: *mut usize,
) -> FerricError {
    serialize_engine_impl(
        engine,
        ferric_rules_runtime::SerializationFormat::Postcard,
        alloc_fn,
        alloc_context,
        out_data,
        out_len,
    )
}

/// Deserialize an engine from Postcard bytes.
///
/// # Safety
///
/// Same safety requirements as `ferric_engine_deserialize_bincode`.
#[cfg_attr(ferric_ffi_compile, ffi_export)]
#[no_mangle]
#[cfg(feature = "serde")]
pub unsafe extern "C" fn ferric_engine_deserialize_postcard(
    data: *const u8,
    len: usize,
    out_engine: *mut *mut FerricEngine,
) -> FerricError {
    deserialize_engine_impl(
        data,
        len,
        ferric_rules_runtime::SerializationFormat::Postcard,
        out_engine,
    )
}

/// Free a byte buffer that was allocated by a serialize function when
/// `alloc_fn` was null.
///
/// Null pointers and zero lengths are safely ignored.
///
/// # Safety
///
/// - `data` must be a pointer returned by a serialize function (with
///   null `alloc_fn`), or null.
/// - `len` must be the length reported by the corresponding serialize call.
/// - The buffer must not have been previously freed.
#[cfg_attr(ferric_ffi_compile, ffi_export)]
#[no_mangle]
#[cfg(feature = "serde")]
pub unsafe extern "C" fn ferric_bytes_free(data: *mut u8, len: usize) {
    if data.is_null() || len == 0 {
        return;
    }
    let slice_ptr = std::ptr::slice_from_raw_parts_mut(data, len);
    drop(Box::from_raw(slice_ptr));
}
