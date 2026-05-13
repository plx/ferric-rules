//! Worker-thread panics surface as `DispatchFailed`.
//!
//! Note: in release builds with the `ffi-abort` profile, panics abort the
//! process. These tests run under the default `dev` profile which unwinds.

use ferric_pinned::{PinnedEngine, PinnedEngineOptions, PinnedError};

#[test]
fn closure_panic_returns_dispatch_failed_to_caller() {
    let engine = PinnedEngine::new(PinnedEngineOptions::default()).unwrap();

    let result = engine.with_engine(|_engine| -> Result<(), PinnedError> {
        panic!("synthetic worker panic");
    });
    assert!(
        matches!(result, Err(PinnedError::DispatchFailed)),
        "expected DispatchFailed, got {result:?}"
    );

    // Subsequent dispatches also fail — worker is dead.
    let next = engine.with_engine(|_| Ok(()));
    assert!(
        matches!(next, Err(PinnedError::DispatchFailed)),
        "expected DispatchFailed after worker death, got {next:?}"
    );
}

#[test]
fn close_after_worker_panic_returns_dispatch_failed() {
    let engine = PinnedEngine::new(PinnedEngineOptions::default()).unwrap();
    // Kill the worker.
    let _ = engine.with_engine(|_| -> Result<(), PinnedError> {
        panic!("synthetic");
    });
    // Close should detect the panicked worker.
    let close_result = engine.close();
    assert!(
        matches!(close_result, Err(PinnedError::DispatchFailed)),
        "expected DispatchFailed from close, got {close_result:?}"
    );
}
