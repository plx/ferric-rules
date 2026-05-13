//! C ABI for the Rust-owned pinned execution layer.
//!
//! `FerricPinnedEngine` owns a dedicated worker thread plus one `Engine`.
//! Calls from any thread are serialized through a bounded FIFO queue; the
//! worker drains the queue in batches and (on Apple platforms) wraps each
//! batch or each item in an autorelease pool according to the configured
//! [`FerricPinnedAutoreleasePolicy`].
//!
//! ## Sync vs async
//!
//! Sync entry points (e.g. [`ferric_pinned_engine_run`]) block the caller
//! until the worker completes the operation, then write outputs into
//! caller-provided pointers.
//!
//! Async entry points (e.g. [`ferric_pinned_engine_run_async`]) return
//! immediately on successful submission, and later invoke the supplied
//! [`FerricPinnedCompletionFn`] **on the worker thread** with an owned
//! [`FerricPinnedResult`] handle. The completion is contractually
//! transport-only — it must not perform long work or call back into
//! the same pinned engine.

use std::cell::RefCell;
use std::ffi::{CStr, CString};
use std::os::raw::{c_char, c_void};
use std::ptr;

use ferric_pinned::{
    AutoreleasePolicy, HaltReason, PinnedEngine, PinnedEngineOptions, PinnedError, RunLimit,
    RunResult,
};

use crate::error::{map_pinned_error, set_global_error, EngineErrorState, FerricError};
use crate::types::{FerricConfig, FerricHaltReason};

#[cfg(feature = "serde")]
use crate::engine::FerricSerializationFormat;

// ---------------------------------------------------------------------------
// C-facing option / enum types
// ---------------------------------------------------------------------------

/// Autorelease-pool installation policy used by the pinned worker.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FerricPinnedAutoreleasePolicy {
    /// Never install an Apple autorelease pool.
    None = 0,
    /// Install one pool per drained request.
    PerItem = 1,
    /// Install one pool per drained batch.
    PerBatch = 2,
}

impl FerricPinnedAutoreleasePolicy {
    fn from_raw(raw: u32) -> Option<Self> {
        match raw {
            0 => Some(Self::None),
            1 => Some(Self::PerItem),
            2 => Some(Self::PerBatch),
            _ => None,
        }
    }

    fn to_runtime(self) -> AutoreleasePolicy {
        match self {
            Self::None => AutoreleasePolicy::None,
            Self::PerItem => AutoreleasePolicy::PerItem,
            Self::PerBatch => AutoreleasePolicy::PerBatch,
        }
    }
}

/// C-facing options struct for [`ferric_pinned_engine_new`].
///
/// Zero / NULL values are interpreted as "use default" (see the corresponding
/// fields on [`ferric_pinned::PinnedEngineOptions`]).
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct FerricPinnedEngineOptions {
    /// Inner engine configuration.
    pub engine: FerricConfig,
    /// Raw [`FerricPinnedAutoreleasePolicy`] discriminant.
    pub autorelease_policy: u32,
    /// Maximum drain-batch size. `0` ⇒ drain everything available.
    pub max_batch_size: usize,
    /// Bounded request-queue capacity. `0` ⇒ default.
    pub queue_capacity: usize,
    /// Worker thread name (NUL-terminated). NULL ⇒ default.
    pub thread_name: *const c_char,
}

// ---------------------------------------------------------------------------
// Opaque handle types
// ---------------------------------------------------------------------------

/// Opaque handle to a Rust-owned pinned engine. Cloning is not supported;
/// the FFI handle is the unique owner of its worker thread.
pub struct FerricPinnedEngine {
    pinned: PinnedEngine,
    error_state: RefCell<EngineErrorState>,
    error_cstring: RefCell<Option<CString>>,
}

/// Opaque handle to an async-operation result. Caller must free with
/// [`ferric_pinned_result_free`].
pub struct FerricPinnedResult {
    code: FerricError,
    payload: PinnedResultPayload,
    message: Option<CString>,
}

enum PinnedResultPayload {
    Empty,
    Run {
        fired: u64,
        reason: FerricHaltReason,
    },
}

// ---------------------------------------------------------------------------
// Async-callback plumbing
// ---------------------------------------------------------------------------

/// C completion-callback type. The result handle is non-NULL and owned by
/// the caller; the caller must release it with [`ferric_pinned_result_free`].
///
/// **Threading contract**: the callback runs on the Rust pinned worker
/// thread. It must be transport-only — resume a continuation, signal an
/// event, post to an actor — and must not call back into the same
/// `FerricPinnedEngine` synchronously or perform long work.
pub type FerricPinnedCompletionFn = Option<
    unsafe extern "C" fn(context: *mut c_void, code: FerricError, result: *mut FerricPinnedResult),
>;

struct CompletionCallback {
    context: *mut c_void,
    func: unsafe extern "C" fn(*mut c_void, FerricError, *mut FerricPinnedResult),
}

// SAFETY: The caller is contractually responsible for ensuring that the
// context pointer (and any state it transitively references) is safe to
// access from the worker thread. We propagate that responsibility to the
// FFI consumer rather than enforcing it in Rust.
unsafe impl Send for CompletionCallback {}

impl CompletionCallback {
    fn fire(self, code: FerricError, result: Box<FerricPinnedResult>) {
        let raw = Box::into_raw(result);
        // SAFETY: `self.func` is a C function pointer the caller supplied;
        // we honor its documented signature.
        unsafe { (self.func)(self.context, code, raw) };
    }
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

unsafe fn validate_engine_ptr<'a>(
    engine: *const FerricPinnedEngine,
) -> Result<&'a FerricPinnedEngine, FerricError> {
    if engine.is_null() {
        set_global_error("pinned engine pointer is null".to_string());
        return Err(FerricError::NullPointer);
    }
    Ok(&*engine)
}

unsafe fn c_str_to_string(ptr: *const c_char, label: &str) -> Result<String, FerricError> {
    if ptr.is_null() {
        set_global_error(format!("{label} pointer is null"));
        return Err(FerricError::NullPointer);
    }
    CStr::from_ptr(ptr)
        .to_str()
        .map(str::to_owned)
        .map_err(|e| {
            set_global_error(format!("{label} is not valid UTF-8: {e}"));
            FerricError::InvalidArgument
        })
}

fn record_pinned_error(handle: &FerricPinnedEngine, err: &PinnedError) -> FerricError {
    let code = map_pinned_error(err);
    let message = err.to_string();
    handle.error_state.borrow_mut().set(message.clone());
    set_global_error(message);
    code
}

fn build_options_from_ffi(
    options: &FerricPinnedEngineOptions,
) -> Result<PinnedEngineOptions, FerricError> {
    let engine_config = ferric_runtime::EngineConfig::try_from(&options.engine).map_err(|m| {
        set_global_error(m);
        FerricError::InvalidArgument
    })?;
    let policy_enum = FerricPinnedAutoreleasePolicy::from_raw(options.autorelease_policy)
        .ok_or_else(|| {
            set_global_error(format!(
                "invalid autorelease_policy: {}",
                options.autorelease_policy
            ));
            FerricError::InvalidArgument
        })?;
    let thread_name = if options.thread_name.is_null() {
        None
    } else {
        // SAFETY: caller guarantees thread_name is a valid NUL-terminated string when non-null.
        Some(unsafe { c_str_to_string(options.thread_name, "thread_name") }?)
    };
    Ok(PinnedEngineOptions {
        engine_config,
        autorelease_policy: policy_enum.to_runtime(),
        max_batch_size: options.max_batch_size,
        queue_capacity: options.queue_capacity,
        thread_name,
    })
}

fn run_limit_from_i64(limit: i64) -> RunLimit {
    if limit < 0 {
        RunLimit::Unlimited
    } else {
        #[allow(clippy::cast_sign_loss, clippy::cast_possible_truncation)]
        RunLimit::Count(limit as usize)
    }
}

// ---------------------------------------------------------------------------
// Lifecycle
// ---------------------------------------------------------------------------

/// Construct a new pinned engine.
///
/// Returns a heap-allocated handle on success, or NULL on failure (with the
/// error message in the global error channel).
///
/// # Safety
///
/// - `options` must point to a valid [`FerricPinnedEngineOptions`] or be NULL.
/// - The returned handle must be freed with [`ferric_pinned_engine_free`].
#[no_mangle]
pub unsafe extern "C" fn ferric_pinned_engine_new(
    options: *const FerricPinnedEngineOptions,
) -> *mut FerricPinnedEngine {
    let pinned_opts = if options.is_null() {
        PinnedEngineOptions::default()
    } else {
        match build_options_from_ffi(&*options) {
            Ok(o) => o,
            Err(_) => return ptr::null_mut(),
        }
    };
    match PinnedEngine::new(pinned_opts) {
        Ok(pinned) => Box::into_raw(Box::new(FerricPinnedEngine {
            pinned,
            error_state: RefCell::new(EngineErrorState::new()),
            error_cstring: RefCell::new(None),
        })),
        Err(err) => {
            set_global_error(err.to_string());
            ptr::null_mut()
        }
    }
}

/// Stop accepting requests, drain any already-queued requests, and join the
/// worker. Idempotent.
///
/// # Safety
///
/// - `engine` must be a valid handle or NULL.
#[no_mangle]
pub unsafe extern "C" fn ferric_pinned_engine_close(
    engine: *mut FerricPinnedEngine,
) -> FerricError {
    let Ok(handle) = validate_engine_ptr(engine) else {
        return FerricError::NullPointer;
    };
    match handle.pinned.close() {
        Ok(()) => FerricError::Ok,
        Err(e) => record_pinned_error(handle, &e),
    }
}

/// Free a pinned engine handle. Closes it first if needed.
///
/// # Safety
///
/// - `engine` must be a pointer returned by [`ferric_pinned_engine_new`], or NULL.
/// - The pointer must not be used after this call.
#[no_mangle]
pub unsafe extern "C" fn ferric_pinned_engine_free(engine: *mut FerricPinnedEngine) -> FerricError {
    if engine.is_null() {
        return FerricError::Ok;
    }
    let handle = &*engine;
    let _ = handle.pinned.close();
    drop(Box::from_raw(engine));
    FerricError::Ok
}

/// Returns `true` once close has begun.
///
/// # Safety
///
/// - `engine` must be a valid handle (NULL ⇒ `false`).
#[no_mangle]
pub unsafe extern "C" fn ferric_pinned_engine_is_closed(engine: *const FerricPinnedEngine) -> bool {
    match validate_engine_ptr(engine) {
        Ok(h) => h.pinned.is_closed(),
        Err(_) => false,
    }
}

/// Flip the shared cancel flag. The currently-running (or next-dispatched)
/// `run` will exit with `HaltRequested` at the next cancel-chunk boundary.
///
/// # Safety
///
/// - `engine` must be a valid handle.
#[no_mangle]
pub unsafe extern "C" fn ferric_pinned_engine_halt(engine: *mut FerricPinnedEngine) -> FerricError {
    let Ok(handle) = validate_engine_ptr(engine) else {
        return FerricError::NullPointer;
    };
    handle.pinned.halt();
    FerricError::Ok
}

/// Retrieve the last per-engine error message as a C string.
///
/// # Safety
///
/// - `engine` must be a valid handle (NULL ⇒ NULL return).
#[no_mangle]
pub unsafe extern "C" fn ferric_pinned_engine_last_error(
    engine: *const FerricPinnedEngine,
) -> *const c_char {
    let Ok(handle) = validate_engine_ptr(engine) else {
        return ptr::null();
    };
    match handle.error_state.borrow().message() {
        Some(msg) => {
            let cstring = CString::new(msg).unwrap_or_default();
            let mut slot = handle.error_cstring.borrow_mut();
            *slot = Some(cstring);
            slot.as_ref().map_or(ptr::null(), |cs| cs.as_ptr())
        }
        None => ptr::null(),
    }
}

// ---------------------------------------------------------------------------
// Sync operations
// ---------------------------------------------------------------------------

/// Load a CLIPS source string (synchronous).
///
/// # Safety
///
/// - `engine` must be a valid handle.
/// - `source` must be a valid NUL-terminated UTF-8 string.
#[no_mangle]
pub unsafe extern "C" fn ferric_pinned_engine_load_string(
    engine: *mut FerricPinnedEngine,
    source: *const c_char,
) -> FerricError {
    let Ok(handle) = validate_engine_ptr(engine) else {
        return FerricError::NullPointer;
    };
    let source_str = match c_str_to_string(source, "source string") {
        Ok(s) => s,
        Err(code) => return code,
    };
    match handle.pinned.load_str(&source_str) {
        Ok(_) => FerricError::Ok,
        Err(e) => record_pinned_error(handle, &e),
    }
}

/// Reset the engine state (synchronous).
///
/// # Safety
///
/// - `engine` must be a valid handle.
#[no_mangle]
pub unsafe extern "C" fn ferric_pinned_engine_reset(
    engine: *mut FerricPinnedEngine,
) -> FerricError {
    let Ok(handle) = validate_engine_ptr(engine) else {
        return FerricError::NullPointer;
    };
    match handle.pinned.reset() {
        Ok(()) => FerricError::Ok,
        Err(e) => record_pinned_error(handle, &e),
    }
}

/// Clear the engine state (synchronous).
///
/// # Safety
///
/// - `engine` must be a valid handle.
#[no_mangle]
pub unsafe extern "C" fn ferric_pinned_engine_clear(
    engine: *mut FerricPinnedEngine,
) -> FerricError {
    let Ok(handle) = validate_engine_ptr(engine) else {
        return FerricError::NullPointer;
    };
    match handle.pinned.clear() {
        Ok(()) => FerricError::Ok,
        Err(e) => record_pinned_error(handle, &e),
    }
}

/// Run the engine until the agenda is empty, the limit is reached, or halt is
/// requested. Synchronous: blocks the caller until the worker completes.
///
/// - `limit`: `-1` ⇒ unlimited; ≥ 0 ⇒ count limit.
/// - `out_fired`: optional pointer to receive rules-fired count.
/// - `out_reason`: optional pointer to receive halt reason.
///
/// # Safety
///
/// - `engine` must be a valid handle.
/// - `out_fired` and `out_reason` may be NULL.
#[no_mangle]
pub unsafe extern "C" fn ferric_pinned_engine_run(
    engine: *mut FerricPinnedEngine,
    limit: i64,
    out_fired: *mut u64,
    out_reason: *mut FerricHaltReason,
) -> FerricError {
    let Ok(handle) = validate_engine_ptr(engine) else {
        return FerricError::NullPointer;
    };
    match handle.pinned.run(run_limit_from_i64(limit)) {
        Ok(result) => {
            if !out_fired.is_null() {
                *out_fired = result.rules_fired as u64;
            }
            if !out_reason.is_null() {
                *out_reason = FerricHaltReason::from(result.halt_reason);
            }
            FerricError::Ok
        }
        Err(e) => record_pinned_error(handle, &e),
    }
}

/// Serialize the engine state to the specified format (synchronous).
/// Mirrors the allocator-callback contract of `ferric_engine_serialize_as`.
///
/// # Safety
///
/// - `engine` must be a valid handle.
/// - `out_data` and `out_len` must be valid, non-null pointers.
/// - If `alloc_fn` is non-null, see [`crate::engine::FerricAllocFn`].
#[no_mangle]
#[cfg(feature = "serde")]
pub unsafe extern "C" fn ferric_pinned_engine_serialize_as(
    engine: *mut FerricPinnedEngine,
    format: u32,
    alloc_fn: crate::engine::FerricAllocFn,
    alloc_context: *mut c_void,
    out_data: *mut *mut u8,
    out_len: *mut usize,
) -> FerricError {
    if out_data.is_null() || out_len.is_null() {
        set_global_error("out_data and out_len must be non-null".to_string());
        return FerricError::NullPointer;
    }
    let Ok(handle) = validate_engine_ptr(engine) else {
        return FerricError::NullPointer;
    };
    let Some(fmt) = FerricSerializationFormat::from_raw(format) else {
        set_global_error(format!("invalid serialization format: {format}"));
        return FerricError::InvalidArgument;
    };
    let bytes = match handle.pinned.serialize(fmt.to_runtime()) {
        Ok(b) => b,
        Err(e) => return record_pinned_error(handle, &e),
    };
    let len = bytes.len();
    if let Some(alloc) = alloc_fn {
        let buf = alloc(len, alloc_context);
        if buf.is_null() {
            set_global_error("caller allocator returned null".to_string());
            return FerricError::SerializationError;
        }
        std::ptr::copy_nonoverlapping(bytes.as_ptr(), buf, len);
        *out_data = buf;
    } else {
        let boxed: Box<[u8]> = bytes.into_boxed_slice();
        *out_data = Box::into_raw(boxed).cast::<u8>();
    }
    *out_len = len;
    FerricError::Ok
}

// ---------------------------------------------------------------------------
// Async operations
// ---------------------------------------------------------------------------

/// Submit a `run` asynchronously. Returns immediately on successful
/// submission. `completion` fires on the worker thread when the operation
/// completes (or fails).
///
/// `request_id` is opaque echo data — the FFI does not consume it; the
/// caller may use it to correlate completions if they wish.
///
/// # Safety
///
/// - `engine` must be a valid handle.
/// - `completion` must be a callable function pointer.
/// - `context` may be any pointer; the caller is responsible for ensuring it
///   is safe to access from the worker thread.
#[no_mangle]
pub unsafe extern "C" fn ferric_pinned_engine_run_async(
    engine: *mut FerricPinnedEngine,
    limit: i64,
    _request_id: u64,
    context: *mut c_void,
    completion: FerricPinnedCompletionFn,
) -> FerricError {
    let Ok(handle) = validate_engine_ptr(engine) else {
        return FerricError::NullPointer;
    };
    let Some(func) = completion else {
        set_global_error("completion callback is null".to_string());
        return FerricError::InvalidArgument;
    };
    let callback = CompletionCallback { context, func };
    let run_limit = run_limit_from_i64(limit);

    let submission = handle.pinned.run_async(run_limit, move |result| {
        let pinned_result = build_run_result(&result);
        let code = pinned_result.code;
        callback.fire(code, Box::new(pinned_result));
    });

    match submission {
        Ok(()) => FerricError::Ok,
        Err(e) => record_pinned_error(handle, &e),
    }
}

/// Submit `load_str` asynchronously.
///
/// # Safety
///
/// - `engine` must be a valid handle.
/// - `source` must be a valid NUL-terminated UTF-8 string. The string is
///   copied; the caller may free it immediately after this call returns.
/// - `completion` must be a callable function pointer.
#[no_mangle]
pub unsafe extern "C" fn ferric_pinned_engine_load_string_async(
    engine: *mut FerricPinnedEngine,
    source: *const c_char,
    _request_id: u64,
    context: *mut c_void,
    completion: FerricPinnedCompletionFn,
) -> FerricError {
    let Ok(handle) = validate_engine_ptr(engine) else {
        return FerricError::NullPointer;
    };
    let source_owned = match c_str_to_string(source, "source string") {
        Ok(s) => s,
        Err(code) => return code,
    };
    let Some(func) = completion else {
        set_global_error("completion callback is null".to_string());
        return FerricError::InvalidArgument;
    };
    let callback = CompletionCallback { context, func };

    let submission = handle.pinned.load_str_async(source_owned, move |result| {
        let (code, message) = match result {
            Ok(_) => (FerricError::Ok, None),
            Err(ref e) => (map_pinned_error(e), Some(e.to_string())),
        };
        callback.fire(
            code,
            Box::new(FerricPinnedResult {
                code,
                payload: PinnedResultPayload::Empty,
                message: message.and_then(|m| CString::new(m).ok()),
            }),
        );
    });

    match submission {
        Ok(()) => FerricError::Ok,
        Err(e) => record_pinned_error(handle, &e),
    }
}

fn build_run_result(result: &Result<RunResult, PinnedError>) -> FerricPinnedResult {
    match result {
        Ok(r) => FerricPinnedResult {
            code: FerricError::Ok,
            payload: PinnedResultPayload::Run {
                fired: r.rules_fired as u64,
                reason: FerricHaltReason::from(match r.halt_reason {
                    HaltReason::AgendaEmpty => HaltReason::AgendaEmpty,
                    HaltReason::LimitReached => HaltReason::LimitReached,
                    HaltReason::HaltRequested => HaltReason::HaltRequested,
                }),
            },
            message: None,
        },
        Err(e) => FerricPinnedResult {
            code: map_pinned_error(e),
            payload: PinnedResultPayload::Empty,
            message: CString::new(e.to_string()).ok(),
        },
    }
}

// ---------------------------------------------------------------------------
// Result handle accessors
// ---------------------------------------------------------------------------

/// Read the result's `FerricError` code.
///
/// # Safety
///
/// - `result` must be a valid handle returned via a completion callback.
#[no_mangle]
pub unsafe extern "C" fn ferric_pinned_result_code(
    result: *const FerricPinnedResult,
) -> FerricError {
    if result.is_null() {
        return FerricError::NullPointer;
    }
    (*result).code
}

/// Read a Run-typed result (`rules_fired` and halt reason).
///
/// Returns [`FerricError::InvalidArgument`] if the result does not carry a Run payload.
///
/// # Safety
///
/// - `result` must be a valid handle.
/// - `out_fired` and `out_reason` may be NULL.
#[no_mangle]
pub unsafe extern "C" fn ferric_pinned_result_get_run(
    result: *const FerricPinnedResult,
    out_fired: *mut u64,
    out_reason: *mut FerricHaltReason,
) -> FerricError {
    if result.is_null() {
        return FerricError::NullPointer;
    }
    match (*result).payload {
        PinnedResultPayload::Run { fired, reason } => {
            if !out_fired.is_null() {
                *out_fired = fired;
            }
            if !out_reason.is_null() {
                *out_reason = reason;
            }
            FerricError::Ok
        }
        PinnedResultPayload::Empty => {
            set_global_error("result has no Run payload".to_string());
            FerricError::InvalidArgument
        }
    }
}

/// Read the result's error message as a borrowed C string. Valid until
/// [`ferric_pinned_result_free`] is called on the handle. Returns NULL
/// if the result has no message.
///
/// # Safety
///
/// - `result` must be a valid handle.
#[no_mangle]
pub unsafe extern "C" fn ferric_pinned_result_error_message(
    result: *const FerricPinnedResult,
) -> *const c_char {
    if result.is_null() {
        return ptr::null();
    }
    match (*result).message.as_ref() {
        Some(c) => c.as_ptr(),
        None => ptr::null(),
    }
}

/// Free a result handle. Idempotent for NULL.
///
/// # Safety
///
/// - `result` must be a handle obtained from a completion callback, or NULL.
/// - The handle must not be used after this call.
#[no_mangle]
pub unsafe extern "C" fn ferric_pinned_result_free(result: *mut FerricPinnedResult) {
    if result.is_null() {
        return;
    }
    drop(Box::from_raw(result));
}
