//! Regression coverage for synchronized global and per-engine error channels.

use crate::engine::{
    ferric_engine_fact_count, ferric_engine_free, ferric_engine_get_fact_field_count,
    ferric_engine_get_global, ferric_engine_last_error, ferric_engine_load_string,
    ferric_engine_new, ferric_engine_reset, FerricEngine,
};
use crate::error::{ferric_clear_error_global, ferric_last_error_global, FerricError};
use crate::types::FerricValue;
use std::ffi::{CStr, CString};

unsafe fn engine_error(engine: *const FerricEngine) -> Option<String> {
    let message = ferric_engine_last_error(engine);
    (!message.is_null()).then(|| CStr::from_ptr(message).to_string_lossy().into_owned())
}

unsafe fn global_error() -> Option<String> {
    let message = ferric_last_error_global();
    (!message.is_null()).then(|| CStr::from_ptr(message).to_string_lossy().into_owned())
}

unsafe fn assert_current_error(engine: *const FerricEngine, expected: &str) {
    let engine_message = engine_error(engine).expect("engine error should be populated");
    let global_message = global_error().expect("global error should be populated");
    assert_eq!(engine_message, expected);
    assert_eq!(global_message, expected);
}

#[test]
fn valid_engine_failures_replace_both_error_channels() {
    unsafe {
        let engine = ferric_engine_new();
        let invalid_source = CString::new("(defrule stale-parse").unwrap();
        assert_ne!(
            ferric_engine_load_string(engine, invalid_source.as_ptr()),
            FerricError::Ok
        );
        let stale_parse = engine_error(engine).expect("parse failure should set engine error");

        let missing_fact = u64::MAX;
        let mut field_count = 0;
        assert_eq!(
            ferric_engine_get_fact_field_count(engine, missing_fact, &mut field_count),
            FerricError::NotFound
        );
        let missing_fact_message = format!("fact not found: {missing_fact}");
        assert_current_error(engine, &missing_fact_message);
        assert_ne!(missing_fact_message, stale_parse);

        let missing_global = CString::new("current-global").unwrap();
        let mut value = FerricValue::void();
        assert_eq!(
            ferric_engine_get_global(engine, missing_global.as_ptr(), &mut value),
            FerricError::NotFound
        );
        assert_current_error(engine, "global variable not found: current-global");

        assert_eq!(
            ferric_engine_fact_count(engine, std::ptr::null_mut()),
            FerricError::NullPointer
        );
        assert_current_error(engine, "out_count pointer is null");

        assert_eq!(ferric_engine_free(engine), FerricError::Ok);
    }
}

#[test]
fn interleaved_engines_keep_independent_current_snapshots() {
    unsafe {
        let first = ferric_engine_new();
        let second = ferric_engine_new();
        let mut count = 0;
        let first_fact = u64::MAX;
        let second_fact = u64::MAX - 1;

        assert_eq!(
            ferric_engine_get_fact_field_count(first, first_fact, &mut count),
            FerricError::NotFound
        );
        assert_eq!(
            ferric_engine_get_fact_field_count(second, second_fact, &mut count),
            FerricError::NotFound
        );

        let first_message = format!("fact not found: {first_fact}");
        let second_message = format!("fact not found: {second_fact}");
        assert_eq!(engine_error(first).as_deref(), Some(first_message.as_str()));
        assert_eq!(
            engine_error(second).as_deref(),
            Some(second_message.as_str())
        );
        assert_eq!(global_error().as_deref(), Some(second_message.as_str()));

        let missing_global = CString::new("first-only").unwrap();
        let mut value = FerricValue::void();
        assert_eq!(
            ferric_engine_get_global(first, missing_global.as_ptr(), &mut value),
            FerricError::NotFound
        );
        assert_current_error(first, "global variable not found: first-only");
        assert_eq!(
            engine_error(second).as_deref(),
            Some(second_message.as_str()),
            "updating one engine must not contaminate another"
        );

        assert_eq!(ferric_engine_free(first), FerricError::Ok);
        assert_eq!(ferric_engine_free(second), FerricError::Ok);
    }
}

#[test]
fn pre_handle_failure_updates_only_the_global_channel() {
    unsafe {
        let engine = ferric_engine_new();
        let invalid_source = CString::new("(defrule retained-engine-error").unwrap();
        assert_ne!(
            ferric_engine_load_string(engine, invalid_source.as_ptr()),
            FerricError::Ok
        );
        let retained = engine_error(engine).expect("parse failure should set engine error");

        ferric_clear_error_global();
        let mut count = 0;
        assert_eq!(
            ferric_engine_fact_count(std::ptr::null(), &mut count),
            FerricError::NullPointer
        );
        assert_eq!(global_error().as_deref(), Some("engine pointer is null"));
        assert_eq!(
            engine_error(engine).as_deref(),
            Some(retained.as_str()),
            "a failure before handle validation cannot target an engine"
        );

        assert_eq!(ferric_engine_free(engine), FerricError::Ok);
    }
}

#[test]
fn valid_engine_thread_violation_updates_both_channels() {
    unsafe {
        let engine = ferric_engine_new();
        let engine_address = engine as usize;
        let (result, thread_global) = std::thread::spawn(move || {
            let engine = engine_address as *mut FerricEngine;
            let result = ferric_engine_reset(engine);
            let thread_global = global_error();
            (result, thread_global)
        })
        .join()
        .unwrap();

        assert_eq!(result, FerricError::ThreadViolation);
        let thread_global = thread_global.expect("thread failure should set its global channel");
        assert_eq!(
            engine_error(engine).as_deref(),
            Some(thread_global.as_str())
        );
        assert!(thread_global.contains("wrong thread"));

        assert_eq!(ferric_engine_free(engine), FerricError::Ok);
    }
}
