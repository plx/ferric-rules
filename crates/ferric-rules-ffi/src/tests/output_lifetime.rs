//! Regression coverage for engine-scoped borrowed output storage.

use crate::engine::{
    ferric_engine_clear_output, ferric_engine_free, ferric_engine_get_output,
    ferric_engine_get_output_copy, ferric_engine_last_error, ferric_engine_load_string,
    ferric_engine_new, ferric_engine_reset, ferric_engine_run, output_cache_entry_count_for_test,
    output_cache_lifetime_for_test, FerricEngine,
};
use crate::error::FerricError;
use std::ffi::{CStr, CString};

unsafe fn engine_with_output(text: &str) -> *mut FerricEngine {
    let engine = ferric_engine_new();
    assert!(!engine.is_null());
    let source = CString::new(format!(r#"(defrule emit => (printout shared "{text}"))"#)).unwrap();
    assert_eq!(
        ferric_engine_load_string(engine, source.as_ptr()),
        FerricError::Ok
    );
    assert_eq!(ferric_engine_reset(engine), FerricError::Ok);
    let mut fired = 0;
    assert_eq!(ferric_engine_run(engine, -1, &mut fired), FerricError::Ok);
    assert_eq!(fired, 1);
    engine
}

#[test]
fn borrowed_output_cache_is_engine_scoped_and_freed_with_engine() {
    unsafe {
        let channel = CString::new("shared").unwrap();
        let first = engine_with_output("engine-a");
        let second = engine_with_output("engine-b");

        assert!(!ferric_engine_get_output(first, channel.as_ptr()).is_null());
        assert_eq!(output_cache_entry_count_for_test(first), Some(1));
        let first_entry = output_cache_lifetime_for_test(first, "shared")
            .expect("first engine should have a cached output entry");

        assert!(!ferric_engine_get_output(second, channel.as_ptr()).is_null());
        assert_eq!(output_cache_entry_count_for_test(first), Some(1));
        assert_eq!(output_cache_entry_count_for_test(second), Some(1));
        let first_entry_survived_second_read = first_entry.upgrade().is_some();
        let second_entry = output_cache_lifetime_for_test(second, "shared")
            .expect("second engine should have a cached output entry");

        assert_eq!(ferric_engine_free(first), FerricError::Ok);
        assert_eq!(ferric_engine_free(second), FerricError::Ok);
        let first_entry_was_reclaimed = first_entry.upgrade().is_none();
        let second_entry_was_reclaimed = second_entry.upgrade().is_none();

        assert!(
            first_entry_survived_second_read,
            "reading engine B must not replace engine A's same-channel cache entry"
        );
        assert!(
            first_entry_was_reclaimed && second_entry_was_reclaimed,
            "freeing each engine must reclaim its cached output entries"
        );
    }
}

#[test]
fn clearing_output_and_reset_reclaim_borrowed_snapshots() {
    unsafe {
        let engine = engine_with_output("engine-a");
        let channel = CString::new("shared").unwrap();
        assert!(!ferric_engine_get_output(engine, channel.as_ptr()).is_null());
        assert_eq!(output_cache_entry_count_for_test(engine), Some(1));
        let cleared_entry = output_cache_lifetime_for_test(engine, "shared").unwrap();

        assert_eq!(
            ferric_engine_clear_output(engine, channel.as_ptr()),
            FerricError::Ok
        );
        assert_eq!(output_cache_entry_count_for_test(engine), Some(0));
        assert!(cleared_entry.upgrade().is_none());

        assert_eq!(ferric_engine_reset(engine), FerricError::Ok);
        let mut fired = 0;
        assert_eq!(ferric_engine_run(engine, -1, &mut fired), FerricError::Ok);
        assert_eq!(fired, 1);
        assert!(!ferric_engine_get_output(engine, channel.as_ptr()).is_null());
        let reset_entry = output_cache_lifetime_for_test(engine, "shared").unwrap();

        assert_eq!(ferric_engine_reset(engine), FerricError::Ok);
        assert_eq!(output_cache_entry_count_for_test(engine), Some(0));
        assert!(reset_entry.upgrade().is_none());
        assert_eq!(ferric_engine_free(engine), FerricError::Ok);
    }
}

#[test]
fn output_copy_reports_required_length_and_truncation() {
    unsafe {
        let engine = engine_with_output("engine-a");
        let channel = CString::new("shared").unwrap();
        let mut needed = 0;

        assert_eq!(
            ferric_engine_get_output_copy(
                engine,
                channel.as_ptr(),
                std::ptr::null_mut(),
                0,
                &mut needed,
            ),
            FerricError::Ok
        );
        assert_eq!(needed, "engine-a".len() + 1);

        let sentinel = std::os::raw::c_char::try_from(b'x')
            .expect("ASCII sentinel must fit in the platform C char type");
        let mut small = [sentinel; 5];
        let mut reported = 0;
        assert_eq!(
            ferric_engine_get_output_copy(
                engine,
                channel.as_ptr(),
                small.as_mut_ptr(),
                small.len(),
                &mut reported,
            ),
            FerricError::BufferTooSmall
        );
        assert_eq!(reported, needed);
        assert_eq!(
            CStr::from_ptr(small.as_ptr()).to_bytes(),
            b"engi",
            "undersized copies must be explicitly reported and NUL-terminated"
        );

        let mut exact = vec![0 as std::os::raw::c_char; needed];
        assert_eq!(
            ferric_engine_get_output_copy(
                engine,
                channel.as_ptr(),
                exact.as_mut_ptr(),
                exact.len(),
                &mut reported,
            ),
            FerricError::Ok
        );
        assert_eq!(reported, needed);
        assert_eq!(CStr::from_ptr(exact.as_ptr()).to_bytes(), b"engine-a");

        assert_eq!(ferric_engine_free(engine), FerricError::Ok);
    }
}

#[test]
fn output_copy_validates_missing_and_invalid_inputs() {
    unsafe {
        let engine = engine_with_output("engine-a");
        let channel = CString::new("shared").unwrap();
        let missing = CString::new("missing").unwrap();
        let mut out_len = usize::MAX;
        let mut byte = 0 as std::os::raw::c_char;

        assert_eq!(
            ferric_engine_get_output_copy(
                engine,
                missing.as_ptr(),
                std::ptr::null_mut(),
                0,
                &mut out_len,
            ),
            FerricError::NotFound
        );
        assert_eq!(out_len, 0);
        assert_eq!(
            CStr::from_ptr(ferric_engine_last_error(engine))
                .to_str()
                .unwrap(),
            "output channel has no captured output: missing"
        );

        out_len = usize::MAX;
        assert_eq!(
            ferric_engine_get_output_copy(
                engine,
                std::ptr::null(),
                std::ptr::null_mut(),
                0,
                &mut out_len,
            ),
            FerricError::NullPointer
        );
        assert_eq!(out_len, 0);

        assert_eq!(
            ferric_engine_get_output_copy(
                engine,
                channel.as_ptr(),
                std::ptr::null_mut(),
                1,
                &mut out_len,
            ),
            FerricError::InvalidArgument
        );
        assert_eq!(out_len, "engine-a".len() + 1);

        assert_eq!(
            ferric_engine_get_output_copy(engine, channel.as_ptr(), &mut byte, 0, &mut out_len,),
            FerricError::BufferTooSmall
        );
        assert_eq!(out_len, "engine-a".len() + 1);

        assert_eq!(
            ferric_engine_get_output_copy(
                engine,
                channel.as_ptr(),
                std::ptr::null_mut(),
                0,
                std::ptr::null_mut(),
            ),
            FerricError::InvalidArgument
        );

        out_len = usize::MAX;
        assert_eq!(
            ferric_engine_get_output_copy(
                std::ptr::null(),
                channel.as_ptr(),
                std::ptr::null_mut(),
                0,
                &mut out_len,
            ),
            FerricError::NullPointer
        );
        assert_eq!(out_len, 0);

        assert_eq!(ferric_engine_free(engine), FerricError::Ok);
    }
}
