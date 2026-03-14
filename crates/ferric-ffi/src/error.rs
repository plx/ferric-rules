//! FFI error model — `FerricError` codes, error mapping, and retrieval APIs.

use std::cell::RefCell;
use std::ffi::CString;
use std::os::raw::c_char;

/// C-facing error codes returned by all fallible FFI entry points.
///
/// Stable numeric values — new codes may be added but existing values
/// must never change.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FerricError {
    /// Operation succeeded.
    Ok = 0,
    /// A required pointer argument was null.
    NullPointer = 1,
    /// Engine called from wrong thread.
    ThreadViolation = 2,
    /// Requested fact/item not found.
    NotFound = 3,
    /// Parse error in CLIPS source.
    ParseError = 4,
    /// Compilation/validation error.
    CompileError = 5,
    /// Runtime evaluation error.
    RuntimeError = 6,
    /// I/O error (file not found, etc).
    IoError = 7,
    /// Provided buffer too small for result.
    BufferTooSmall = 8,
    /// Invalid argument value.
    InvalidArgument = 9,
    /// Serialization or deserialization error.
    SerializationError = 10,
    /// Internal/unexpected error.
    InternalError = 99,
}

// ---------------------------------------------------------------------------
// Thread-local global error storage
// ---------------------------------------------------------------------------

thread_local! {
    static LAST_ERROR_GLOBAL: RefCell<Option<String>> = const { RefCell::new(None) };
    static LAST_CSTRING: RefCell<Option<CString>> = const { RefCell::new(None) };
}

/// Store a global (thread-local) error message.
pub(crate) fn set_global_error(msg: String) {
    LAST_ERROR_GLOBAL.with(|e| *e.borrow_mut() = Some(msg));
}

/// Clear the global error message.
pub(crate) fn clear_global_error() {
    LAST_ERROR_GLOBAL.with(|e| *e.borrow_mut() = None);
}

/// Call `f` with the current global error message (if any).
///
/// The returned string reference is valid only for the duration of the call.
pub(crate) fn with_global_error<F, R>(f: F) -> R
where
    F: FnOnce(Option<&str>) -> R,
{
    LAST_ERROR_GLOBAL.with(|e| {
        let borrow = e.borrow();
        f(borrow.as_deref())
    })
}

// ---------------------------------------------------------------------------
// Per-engine error storage
// ---------------------------------------------------------------------------

/// Per-engine error state, stored inside the opaque engine handle.
#[derive(Debug, Default)]
pub struct EngineErrorState {
    last_error: Option<String>,
}

impl EngineErrorState {
    /// Create a new, empty error state.
    pub fn new() -> Self {
        Self { last_error: None }
    }

    /// Store an error message.
    pub fn set(&mut self, msg: String) {
        self.last_error = Some(msg);
    }

    /// Clear the stored error message.
    pub fn clear(&mut self) {
        self.last_error = None;
    }

    /// Retrieve the stored error message, if any.
    pub fn message(&self) -> Option<&str> {
        self.last_error.as_deref()
    }
}

// ---------------------------------------------------------------------------
// Error mapping from runtime types
// ---------------------------------------------------------------------------

use ferric_runtime::engine::EngineError;
use ferric_runtime::loader::LoadError;

/// Map a runtime [`EngineError`] to an FFI error code.
pub(crate) fn map_engine_error(err: &EngineError) -> FerricError {
    match err {
        EngineError::WrongThread { .. } => FerricError::ThreadViolation,
        EngineError::FactNotFound(_)
        | EngineError::ModuleNotFound(_)
        | EngineError::TemplateNotFound(_)
        | EngineError::SlotNotFound { .. } => FerricError::NotFound,
        EngineError::NotATemplateFact(_) | EngineError::Encoding(_) => FerricError::InvalidArgument,
        #[allow(unreachable_patterns)]
        _ => FerricError::InternalError,
    }
}

/// Map a [`LoadError`] to an FFI error code.
pub(crate) fn map_load_error(err: &LoadError) -> FerricError {
    match err {
        LoadError::Parse(_) | LoadError::Interpret(_) => FerricError::ParseError,
        LoadError::UnsupportedForm { .. }
        | LoadError::InvalidAssert(_)
        | LoadError::InvalidDefrule(_)
        | LoadError::Compile(_)
        | LoadError::Validation(_) => FerricError::CompileError,
        LoadError::Engine(e) => map_engine_error(e),
        LoadError::Io(_) => FerricError::IoError,
    }
}

// ---------------------------------------------------------------------------
// C API: Global error retrieval
// ---------------------------------------------------------------------------

/// Retrieve the last global error message as a C string pointer.
///
/// Returns a pointer to a NUL-terminated UTF-8 string, or null if no error
/// is stored. The returned pointer is valid only until the next FFI call
/// that may modify the global error channel.
///
/// # Safety
///
/// The returned pointer must not be freed by the caller and must not be
/// used after any subsequent FFI call that may modify the error channel.
#[no_mangle]
pub unsafe extern "C" fn ferric_last_error_global() -> *const c_char {
    with_global_error(|msg| match msg {
        Some(msg) => {
            let cstring = CString::new(msg).unwrap_or_default();
            LAST_CSTRING.with(|c| {
                *c.borrow_mut() = Some(cstring);
                c.borrow()
                    .as_ref()
                    .map_or(std::ptr::null(), |cs| cs.as_ptr())
            })
        }
        None => std::ptr::null(),
    })
}

/// Clear the global error channel.
#[no_mangle]
pub extern "C" fn ferric_clear_error_global() {
    clear_global_error();
}

/// Copy the last global error message into a caller-provided buffer.
///
/// ## Contract
///
/// | Condition | Return | `*out_len` |
/// |-----------|--------|------------|
/// | No error stored | `NotFound` | 0 |
/// | `buf` is null AND `buf_len` is 0 (size query) | `Ok` | required size (incl. NUL) |
/// | `buf` non-null, `buf_len` >= needed | `Ok` | bytes written (incl. NUL) |
/// | `buf` non-null, `buf_len` < needed | `BufferTooSmall` | full needed size (incl. NUL) |
/// | `buf_len` is 0 with non-null `buf` | `BufferTooSmall` | full needed size (incl. NUL) |
/// | `out_len` is null | `InvalidArgument` | (not written) |
///
/// When truncation occurs (`BufferTooSmall`), the buffer receives `buf_len - 1`
/// bytes of the message followed by a NUL terminator. If `buf_len` is 0,
/// nothing is written.
///
/// # Safety
///
/// - `buf` must point to `buf_len` writable bytes, or be null for size query.
/// - `out_len` must be a valid pointer (non-null).
#[no_mangle]
pub unsafe extern "C" fn ferric_last_error_global_copy(
    buf: *mut c_char,
    buf_len: usize,
    out_len: *mut usize,
) -> FerricError {
    if out_len.is_null() {
        return FerricError::InvalidArgument;
    }
    with_global_error(|msg| copy_error_to_buffer(msg, buf, buf_len, out_len))
}

// ---------------------------------------------------------------------------
// Copy-to-buffer helper (shared by global and per-engine copy APIs)
// ---------------------------------------------------------------------------

/// Copy an error message into a caller-provided buffer.
///
/// `out_len` must already have been validated as non-null before calling this
/// function. Writes the appropriate length value to `*out_len` in every branch.
///
/// # Safety
///
/// - `out_len` must be a valid, non-null pointer.
/// - If `buf` is non-null, it must point to at least `buf_len` writable bytes.
pub(crate) unsafe fn copy_error_to_buffer(
    msg: Option<&str>,
    buf: *mut c_char,
    buf_len: usize,
    out_len: *mut usize,
) -> FerricError {
    let Some(s) = msg else {
        *out_len = 0;
        return FerricError::NotFound;
    };

    let needed = s.len() + 1; // message bytes + NUL terminator

    if buf.is_null() {
        *out_len = needed;
        return if buf_len == 0 {
            FerricError::Ok // size query: null buf + zero len → report needed size
        } else {
            FerricError::InvalidArgument // nonsensical: non-zero len but null buf
        };
    }

    if buf_len == 0 {
        *out_len = needed;
        return FerricError::BufferTooSmall;
    }

    if buf_len >= needed {
        // Full copy: message bytes then NUL terminator
        std::ptr::copy_nonoverlapping(s.as_ptr(), buf.cast::<u8>(), s.len());
        *buf.add(s.len()) = 0;
        *out_len = needed;
        FerricError::Ok
    } else {
        // Truncation: copy buf_len - 1 bytes then NUL terminator
        let copy_len = buf_len - 1;
        std::ptr::copy_nonoverlapping(s.as_ptr(), buf.cast::<u8>(), copy_len);
        *buf.add(copy_len) = 0;
        *out_len = needed;
        FerricError::BufferTooSmall
    }
}

// ---------------------------------------------------------------------------
// Error mapping convenience
// ---------------------------------------------------------------------------

/// Map an [`EngineError`] to [`FerricError`], storing the message in the global channel.
pub(crate) fn set_engine_error_global(err: &EngineError) -> FerricError {
    let code = map_engine_error(err);
    set_global_error(err.to_string());
    code
}
