//! Raw C handles transfer ownership; simultaneous runtime access is rejected.

use crate::engine::*;
use crate::error::{
    ferric_clear_error_global, ferric_last_error_global, set_global_error, FerricError,
};
use std::ffi::{CStr, CString};

#[test]
fn engine_survives_creator_exit_and_copied_output_survives_free() {
    let owned = std::thread::spawn(|| unsafe {
        let engine = ferric_engine_new();
        let source =
            CString::new("(deffacts data (item 7)) (defrule r (item ?x) => (printout t ?x crlf))")
                .unwrap();
        assert_eq!(
            ferric_engine_load_string(engine, source.as_ptr()),
            FerricError::Ok
        );
        assert_eq!(ferric_engine_reset(engine), FerricError::Ok);
        // Box carries genuine exclusive ownership and derives Send structurally.
        Box::from_raw(engine)
    })
    .join()
    .unwrap();

    let copied = std::thread::spawn(move || unsafe {
        let engine = Box::into_raw(owned);
        let mut fired = 0;
        assert_eq!(ferric_engine_run(engine, -1, &mut fired), FerricError::Ok);
        assert_eq!(fired, 1);
        let mut output = [0_i8; 32];
        let mut written = 0;
        let channel = CString::new("t").unwrap();
        assert_eq!(
            ferric_engine_get_output_copy(
                engine,
                channel.as_ptr(),
                output.as_mut_ptr(),
                output.len(),
                &mut written
            ),
            FerricError::Ok
        );
        assert_eq!(written, 3);
        let text = CStr::from_ptr(output.as_ptr()).to_str().unwrap().to_owned();
        assert_eq!(ferric_engine_free(engine), FerricError::Ok);
        text
    })
    .join()
    .unwrap();
    assert_eq!(copied, "7\n");
}

#[test]
fn errors_follow_the_handle_but_global_errors_stay_on_the_calling_thread() {
    unsafe {
        set_global_error("parent error".to_owned());
        let owned = Box::from_raw(ferric_engine_new());
        let (owned, copied) = std::thread::spawn(move || {
            ferric_clear_error_global();
            assert!(ferric_last_error_global().is_null());
            let engine = Box::into_raw(owned);
            assert_eq!(
                ferric_engine_retract(engine, u64::MAX),
                FerricError::NotFound
            );
            let text = CStr::from_ptr(ferric_last_error_global())
                .to_str()
                .unwrap()
                .to_owned();
            (Box::from_raw(engine), text)
        })
        .join()
        .unwrap();
        assert_eq!(
            CStr::from_ptr(ferric_last_error_global()).to_str().unwrap(),
            "parent error"
        );
        let engine = Box::into_raw(owned);
        let mut output = [0_i8; 512];
        let mut written = 0;
        assert_eq!(
            ferric_engine_last_error_copy(engine, output.as_mut_ptr(), output.len(), &mut written),
            FerricError::Ok
        );
        assert_eq!(CStr::from_ptr(output.as_ptr()).to_str().unwrap(), copied);
        assert_eq!(ferric_engine_free(engine), FerricError::Ok);
    }
}

#[cfg(feature = "serde")]
#[test]
fn overlapping_calls_and_free_are_rejected_while_allocator_holds_admission() {
    use std::sync::{Arc, Barrier};
    struct Context {
        entered: Arc<Barrier>,
        release: Arc<Barrier>,
        bytes: Vec<u8>,
    }
    unsafe extern "C" fn allocator(size: usize, context: *mut std::ffi::c_void) -> *mut u8 {
        let context = &mut *context.cast::<Context>();
        context.entered.wait();
        context.release.wait();
        context.bytes.resize(size, 0);
        context.bytes.as_mut_ptr()
    }
    unsafe {
        let mut owned = Box::from_raw(ferric_engine_new());
        let address = std::sync::atomic::AtomicPtr::new(std::ptr::addr_of_mut!(*owned));
        let entered = Arc::new(Barrier::new(2));
        let release = Arc::new(Barrier::new(2));
        let mut context = Context {
            entered: Arc::clone(&entered),
            release: Arc::clone(&release),
            bytes: Vec::new(),
        };
        let worker = std::thread::spawn(move || {
            let engine = Box::into_raw(owned);
            let mut bytes = std::ptr::null_mut();
            let mut len = 0;
            assert_eq!(
                ferric_engine_serialize_bincode(
                    engine,
                    Some(allocator),
                    std::ptr::addr_of_mut!(context).cast(),
                    &mut bytes,
                    &mut len
                ),
                FerricError::Ok
            );
            assert_eq!(len, context.bytes.len());
            Box::from_raw(engine)
        });
        entered.wait();
        // The callback cannot release admission or free the allocation until
        // this thread releases its barrier. These attempts cannot succeed in
        // destroying it; this does not test or permit a race with successful free.
        let engine = address.load(std::sync::atomic::Ordering::Relaxed);
        assert_eq!(ferric_engine_reset(engine), FerricError::InternalError);
        assert_eq!(ferric_engine_free(engine), FerricError::InternalError);
        let mut count = usize::MAX;
        assert_eq!(
            ferric_engine_fact_count(engine, &mut count),
            FerricError::InternalError
        );
        assert_eq!(count, usize::MAX);
        let mut error = [0_i8; 512];
        let mut len = 0;
        assert_eq!(
            ferric_engine_last_error_copy(engine, error.as_mut_ptr(), error.len(), &mut len),
            FerricError::Ok
        );
        assert!(CStr::from_ptr(error.as_ptr())
            .to_str()
            .unwrap()
            .contains("overlapping"));
        release.wait();
        let owned = worker.join().unwrap();
        let engine = Box::into_raw(owned);
        assert_eq!(ferric_engine_reset(engine), FerricError::Ok);
        assert_eq!(ferric_engine_free(engine), FerricError::Ok);
    }
}
