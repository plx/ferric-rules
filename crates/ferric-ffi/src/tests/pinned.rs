//! FFI tests for the `ferric_pinned_*` surface — pinned engine lifecycle,
//! sync ops, and async completion.

use std::ffi::CString;
use std::os::raw::{c_char, c_void};
use std::ptr;
use std::sync::{Arc, Barrier, Condvar, Mutex};
use std::time::Duration;

use crate::error::FerricError;
use crate::pinned::{
    ferric_pinned_engine_close, ferric_pinned_engine_free, ferric_pinned_engine_halt,
    ferric_pinned_engine_is_closed, ferric_pinned_engine_last_error,
    ferric_pinned_engine_load_string, ferric_pinned_engine_load_string_async,
    ferric_pinned_engine_new, ferric_pinned_engine_reset, ferric_pinned_engine_run,
    ferric_pinned_engine_run_async, ferric_pinned_result_code, ferric_pinned_result_free,
    ferric_pinned_result_get_run, FerricPinnedAutoreleasePolicy, FerricPinnedEngineOptions,
    FerricPinnedResult,
};
use crate::types::{FerricConfig, FerricHaltReason};

fn default_options() -> FerricPinnedEngineOptions {
    FerricPinnedEngineOptions {
        engine: FerricConfig::default(),
        autorelease_policy: FerricPinnedAutoreleasePolicy::None as u32,
        max_batch_size: 0,
        queue_capacity: 0,
        thread_name: ptr::null(),
    }
}

fn options_with_policy(policy: FerricPinnedAutoreleasePolicy) -> FerricPinnedEngineOptions {
    FerricPinnedEngineOptions {
        autorelease_policy: policy as u32,
        ..default_options()
    }
}

#[test]
fn new_with_null_options_uses_defaults() {
    unsafe {
        let engine = ferric_pinned_engine_new(ptr::null());
        assert!(!engine.is_null());
        ferric_pinned_engine_free(engine);
    }
}

#[test]
fn new_with_each_autorelease_policy() {
    unsafe {
        for policy in [
            FerricPinnedAutoreleasePolicy::None,
            FerricPinnedAutoreleasePolicy::PerItem,
            FerricPinnedAutoreleasePolicy::PerBatch,
        ] {
            let opts = options_with_policy(policy);
            let engine = ferric_pinned_engine_new(&opts);
            assert!(!engine.is_null(), "policy {policy:?}");
            ferric_pinned_engine_free(engine);
        }
    }
}

#[test]
fn new_with_invalid_policy_returns_null() {
    unsafe {
        let mut opts = default_options();
        opts.autorelease_policy = 99;
        let engine = ferric_pinned_engine_new(&opts);
        assert!(engine.is_null());
    }
}

#[test]
fn load_reset_run_sync_path() {
    unsafe {
        let opts = default_options();
        let engine = ferric_pinned_engine_new(&opts);
        assert!(!engine.is_null());

        let source = CString::new("(defrule r => (assert (fired)))").unwrap();
        assert_eq!(
            ferric_pinned_engine_load_string(engine, source.as_ptr()),
            FerricError::Ok
        );
        assert_eq!(ferric_pinned_engine_reset(engine), FerricError::Ok);

        let mut fired: u64 = 0;
        let mut reason: FerricHaltReason = FerricHaltReason::AgendaEmpty;
        assert_eq!(
            ferric_pinned_engine_run(engine, -1, &mut fired, &mut reason),
            FerricError::Ok
        );
        assert_eq!(fired, 1);
        assert_eq!(reason as u32, FerricHaltReason::AgendaEmpty as u32);

        ferric_pinned_engine_free(engine);
    }
}

#[test]
fn null_engine_returns_null_pointer() {
    unsafe {
        let mut fired = 0;
        let mut reason = FerricHaltReason::AgendaEmpty;
        assert_eq!(
            ferric_pinned_engine_run(ptr::null_mut(), -1, &mut fired, &mut reason),
            FerricError::NullPointer
        );
        assert_eq!(
            ferric_pinned_engine_reset(ptr::null_mut()),
            FerricError::NullPointer
        );
    }
}

#[test]
fn ops_after_close_return_pinned_closed() {
    unsafe {
        let opts = default_options();
        let engine = ferric_pinned_engine_new(&opts);
        assert_eq!(ferric_pinned_engine_close(engine), FerricError::Ok);
        assert!(ferric_pinned_engine_is_closed(engine));

        let mut fired = 0;
        let mut reason = FerricHaltReason::AgendaEmpty;
        assert_eq!(
            ferric_pinned_engine_run(engine, -1, &mut fired, &mut reason),
            FerricError::PinnedClosed
        );

        // last_error_message should report the closed error.
        let msg_ptr = ferric_pinned_engine_last_error(engine);
        assert!(!msg_ptr.is_null());

        ferric_pinned_engine_free(engine);
    }
}

#[test]
fn closed_error_state_is_safe_under_concurrent_access() {
    unsafe {
        let engine = ferric_pinned_engine_new(&default_options());
        assert_eq!(ferric_pinned_engine_close(engine), FerricError::Ok);

        let engine_addr = engine as usize;
        let thread_count = 12_usize;
        let barrier = Arc::new(Barrier::new(thread_count));
        let handles: Vec<_> = (0..thread_count)
            .map(|_| {
                let barrier = barrier.clone();
                std::thread::spawn(move || {
                    let engine = engine_addr as *mut crate::pinned::FerricPinnedEngine;
                    barrier.wait();
                    for _ in 0..100 {
                        let mut fired = 0;
                        let mut reason = FerricHaltReason::AgendaEmpty;
                        assert_eq!(
                            ferric_pinned_engine_run(engine, -1, &mut fired, &mut reason),
                            FerricError::PinnedClosed
                        );
                        let _ = ferric_pinned_engine_last_error(engine);
                    }
                })
            })
            .collect();

        for handle in handles {
            handle.join().unwrap();
        }

        ferric_pinned_engine_free(engine);
    }
}

#[test]
fn double_close_is_idempotent() {
    unsafe {
        let engine = ferric_pinned_engine_new(&default_options());
        assert_eq!(ferric_pinned_engine_close(engine), FerricError::Ok);
        assert_eq!(ferric_pinned_engine_close(engine), FerricError::Ok);
        ferric_pinned_engine_free(engine);
    }
}

#[test]
fn free_without_explicit_close() {
    unsafe {
        let engine = ferric_pinned_engine_new(&default_options());
        ferric_pinned_engine_free(engine);
    }
}

#[test]
fn free_null_is_ok() {
    unsafe {
        assert_eq!(ferric_pinned_engine_free(ptr::null_mut()), FerricError::Ok);
    }
}

// -----------------------------------------------------------------------
// Async completion tests
// -----------------------------------------------------------------------

/// Shared callback context for tests. Wraps a result pointer in a mutex.
struct CompletionInbox {
    received: Mutex<Vec<(FerricError, *mut FerricPinnedResult)>>,
    cv: Condvar,
}

// SAFETY: We only touch the inner pointer via the mutex; the worker fires the
// callback exactly once, then we read and free the pointer on a deliberate
// thread.
unsafe impl Send for CompletionInbox {}
unsafe impl Sync for CompletionInbox {}

impl CompletionInbox {
    fn new() -> Arc<Self> {
        Arc::new(Self {
            received: Mutex::new(Vec::new()),
            cv: Condvar::new(),
        })
    }

    fn wait_one(&self, timeout: Duration) -> Option<(FerricError, *mut FerricPinnedResult)> {
        let guard = self.received.lock().unwrap();
        let (mut guard, _) = self
            .cv
            .wait_timeout_while(guard, timeout, |v| v.is_empty())
            .unwrap();
        guard.pop()
    }
}

unsafe extern "C" fn record_completion(
    context: *mut c_void,
    code: FerricError,
    result: *mut FerricPinnedResult,
) {
    let inbox = &*context.cast::<CompletionInbox>();
    inbox.received.lock().unwrap().push((code, result));
    inbox.cv.notify_one();
}

#[test]
fn run_async_invokes_completion_with_result_handle() {
    unsafe {
        let engine = ferric_pinned_engine_new(&default_options());
        let source = CString::new("(defrule r => (assert (fired)))").unwrap();
        assert_eq!(
            ferric_pinned_engine_load_string(engine, source.as_ptr()),
            FerricError::Ok
        );
        assert_eq!(ferric_pinned_engine_reset(engine), FerricError::Ok);

        let inbox = CompletionInbox::new();
        let ctx_ptr = Arc::as_ptr(&inbox).cast::<c_void>().cast_mut();

        let submit =
            ferric_pinned_engine_run_async(engine, -1, 42, ctx_ptr, Some(record_completion));
        assert_eq!(submit, FerricError::Ok);

        let (code, result) = inbox
            .wait_one(Duration::from_secs(2))
            .expect("completion never fired");
        assert_eq!(code, FerricError::Ok);
        assert!(!result.is_null());

        assert_eq!(ferric_pinned_result_code(result), FerricError::Ok);
        let mut fired = 0;
        let mut reason = FerricHaltReason::AgendaEmpty;
        assert_eq!(
            ferric_pinned_result_get_run(result, &mut fired, &mut reason),
            FerricError::Ok
        );
        assert_eq!(fired, 1);
        ferric_pinned_result_free(result);
        ferric_pinned_engine_free(engine);
    }
}

#[test]
fn run_async_halt_delivers_halt_requested_in_callback() {
    unsafe {
        let engine = ferric_pinned_engine_new(&default_options());
        let source = CString::new(
            r"(defrule cycle ?f <- (counter ?n) => (retract ?f) (assert (counter (+ ?n 1))))
              (deffacts initial (counter 0))",
        )
        .unwrap();
        assert_eq!(
            ferric_pinned_engine_load_string(engine, source.as_ptr()),
            FerricError::Ok
        );
        assert_eq!(ferric_pinned_engine_reset(engine), FerricError::Ok);

        let inbox = CompletionInbox::new();
        let ctx_ptr = Arc::as_ptr(&inbox).cast::<c_void>().cast_mut();

        let submit =
            ferric_pinned_engine_run_async(engine, -1, 7, ctx_ptr, Some(record_completion));
        assert_eq!(submit, FerricError::Ok);

        // Give the worker a moment to start, then halt.
        std::thread::sleep(Duration::from_millis(50));
        assert_eq!(ferric_pinned_engine_halt(engine), FerricError::Ok);

        let (code, result) = inbox
            .wait_one(Duration::from_secs(5))
            .expect("completion never fired");
        assert_eq!(code, FerricError::Ok);
        let mut fired = 0;
        let mut reason = FerricHaltReason::AgendaEmpty;
        assert_eq!(
            ferric_pinned_result_get_run(result, &mut fired, &mut reason),
            FerricError::Ok
        );
        assert_eq!(reason as u32, FerricHaltReason::HaltRequested as u32);
        assert!(fired > 0);
        ferric_pinned_result_free(result);
        ferric_pinned_engine_free(engine);
    }
}

#[test]
fn load_string_async_invokes_completion() {
    unsafe {
        let engine = ferric_pinned_engine_new(&default_options());
        let source = CString::new("(defrule r => (assert (a)))").unwrap();
        let inbox = CompletionInbox::new();
        let ctx_ptr = Arc::as_ptr(&inbox).cast::<c_void>().cast_mut();

        let submit = ferric_pinned_engine_load_string_async(
            engine,
            source.as_ptr(),
            99,
            ctx_ptr,
            Some(record_completion),
        );
        assert_eq!(submit, FerricError::Ok);

        let (code, result) = inbox
            .wait_one(Duration::from_secs(2))
            .expect("completion never fired");
        assert_eq!(code, FerricError::Ok);
        assert_eq!(ferric_pinned_result_code(result), FerricError::Ok);
        ferric_pinned_result_free(result);
        ferric_pinned_engine_free(engine);
    }
}

#[test]
fn result_free_null_is_safe() {
    unsafe {
        ferric_pinned_result_free(ptr::null_mut());
    }
}

#[test]
fn result_get_run_on_empty_payload_returns_invalid_argument() {
    // Use load_string_async to produce an Empty-payload result.
    unsafe {
        let engine = ferric_pinned_engine_new(&default_options());
        let source = CString::new("(defrule r => (assert (a)))").unwrap();
        let inbox = CompletionInbox::new();
        let ctx_ptr = Arc::as_ptr(&inbox).cast::<c_void>().cast_mut();

        ferric_pinned_engine_load_string_async(
            engine,
            source.as_ptr(),
            1,
            ctx_ptr,
            Some(record_completion),
        );

        let (_code, result) = inbox
            .wait_one(Duration::from_secs(2))
            .expect("completion never fired");
        let mut fired = 0;
        let mut reason = FerricHaltReason::AgendaEmpty;
        assert_eq!(
            ferric_pinned_result_get_run(result, &mut fired, &mut reason),
            FerricError::InvalidArgument
        );
        ferric_pinned_result_free(result);
        ferric_pinned_engine_free(engine);
    }
}

#[test]
fn run_async_on_null_engine_returns_null_pointer() {
    unsafe {
        let inbox = CompletionInbox::new();
        let ctx_ptr = Arc::as_ptr(&inbox).cast::<c_void>().cast_mut();
        let code = ferric_pinned_engine_run_async(
            ptr::null_mut(),
            -1,
            0,
            ctx_ptr,
            Some(record_completion),
        );
        assert_eq!(code, FerricError::NullPointer);
    }
}

#[test]
fn run_async_with_null_completion_returns_invalid_argument() {
    unsafe {
        let engine = ferric_pinned_engine_new(&default_options());
        let code = ferric_pinned_engine_run_async(engine, -1, 0, ptr::null_mut(), None);
        assert_eq!(code, FerricError::InvalidArgument);
        ferric_pinned_engine_free(engine);
    }
}

#[test]
fn custom_thread_name_accepted() {
    unsafe {
        let name = CString::new("test-worker").unwrap();
        let opts = FerricPinnedEngineOptions {
            thread_name: name.as_ptr().cast::<c_char>(),
            ..default_options()
        };
        let engine = ferric_pinned_engine_new(&opts);
        assert!(!engine.is_null());
        ferric_pinned_engine_free(engine);
    }
}
