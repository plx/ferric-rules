//! Panic containment shared by every exported C ABI wrapper.

use std::panic::{catch_unwind, AssertUnwindSafe};

use crate::engine::FerricEngine;
use crate::error::{try_set_global_error, FerricError};
use crate::pinned::FerricPinnedEngine;
use crate::types::FerricValue;

/// Diagnostic target associated with one exported call.
#[derive(Clone, Copy)]
pub(crate) enum PanicTarget {
    /// Only the thread-local global channel is available.
    Global,
    /// A live raw-engine handle was supplied.
    RawEngine(*const FerricEngine),
    /// A live pinned-engine handle was supplied.
    PinnedEngine(*const FerricPinnedEngine),
}

/// Defined sentinel returned after a panic is contained.
pub(crate) trait PanicSentinel {
    fn panic_sentinel() -> Self;
}

impl PanicSentinel for FerricError {
    fn panic_sentinel() -> Self {
        Self::InternalError
    }
}

impl<T> PanicSentinel for *const T {
    fn panic_sentinel() -> Self {
        std::ptr::null()
    }
}

impl<T> PanicSentinel for *mut T {
    fn panic_sentinel() -> Self {
        std::ptr::null_mut()
    }
}

impl PanicSentinel for FerricValue {
    fn panic_sentinel() -> Self {
        Self::void()
    }
}

impl PanicSentinel for bool {
    fn panic_sentinel() -> Self {
        false
    }
}

impl PanicSentinel for u64 {
    fn panic_sentinel() -> Self {
        0
    }
}

impl PanicSentinel for () {
    fn panic_sentinel() {}
}

/// Invoke one non-extern implementation while containing ordinary Rust
/// panics inside the generated C wrapper.
///
/// Panic payloads are deliberately neither formatted nor downcast. They may
/// contain arbitrary application data or non-string types; the stable
/// diagnostic identifies only the export where containment occurred.
pub(crate) fn invoke<R, F>(name: &'static str, target: PanicTarget, operation: F) -> R
where
    R: PanicSentinel,
    F: FnOnce() -> R,
{
    match catch_unwind(AssertUnwindSafe(|| {
        maybe_inject_test_panic(name);
        operation()
    })) {
        Ok(result) => result,
        Err(payload) => {
            discard_panic_payload(payload);

            let message = format!("internal Rust panic contained in C ABI export `{name}`");
            try_set_global_error(message.clone());

            // A valid supplied engine receives the same diagnostic. This is
            // best-effort because the containment path itself must not permit
            // a secondary diagnostic-state panic to escape the wrapper.
            if let Err(payload) = catch_unwind(AssertUnwindSafe(|| unsafe {
                match target {
                    PanicTarget::Global => {}
                    PanicTarget::RawEngine(engine) => {
                        crate::engine::record_boundary_panic(engine, message);
                    }
                    PanicTarget::PinnedEngine(engine) => {
                        crate::pinned::record_boundary_panic(engine, message);
                    }
                }
            })) {
                discard_panic_payload(payload);
            }

            R::panic_sentinel()
        }
    }
}

/// Dispose of a caught payload without trusting its destructor to return
/// normally. If that destructor panics, contain the secondary panic and leak
/// only its payload rather than allowing another unwind to reach C.
fn discard_panic_payload(payload: Box<dyn std::any::Any + Send>) {
    if let Err(secondary_payload) = catch_unwind(AssertUnwindSafe(|| drop(payload))) {
        std::mem::forget(secondary_payload);
    }
}

#[cfg(ferric_ffi_test_panic_injection)]
fn maybe_inject_test_panic(name: &str) {
    if std::env::var_os("FERRIC_FFI_TEST_PANIC")
        .is_some_and(|selected| selected == std::ffi::OsStr::new(name))
    {
        panic!("test-only injected FFI boundary panic");
    }
}

#[cfg(not(ferric_ffi_test_panic_injection))]
fn maybe_inject_test_panic(_name: &str) {}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;

    use super::{invoke, PanicTarget};
    use crate::error::{with_global_error, FerricError};

    struct PanicsOnDrop(Arc<AtomicBool>);

    impl Drop for PanicsOnDrop {
        fn drop(&mut self) {
            self.0.store(true, Ordering::Relaxed);
            panic!("panic payload destructor");
        }
    }

    #[test]
    fn panicking_payload_destructor_is_also_contained() {
        let payload_was_dropped = Arc::new(AtomicBool::new(false));
        let payload_flag = Arc::clone(&payload_was_dropped);

        let result: FerricError = invoke("payload_test", PanicTarget::Global, || {
            std::panic::panic_any(PanicsOnDrop(payload_flag));
        });

        assert_eq!(result, FerricError::InternalError);
        assert!(payload_was_dropped.load(Ordering::Relaxed));
        with_global_error(|message| {
            assert_eq!(
                message,
                Some("internal Rust panic contained in C ABI export `payload_test`")
            );
        });
    }
}
