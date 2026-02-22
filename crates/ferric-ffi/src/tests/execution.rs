//! Tests for FFI execution and fact mutation APIs (Pass 005).

use crate::engine::{
    ferric_engine_assert_string, ferric_engine_free, ferric_engine_get_output,
    ferric_engine_load_string, ferric_engine_new, ferric_engine_reset, ferric_engine_retract,
    ferric_engine_run, ferric_engine_step,
};
use crate::error::FerricError;

#[test]
fn run_empty_engine_fires_nothing() {
    unsafe {
        let engine = ferric_engine_new();
        ferric_engine_reset(engine);
        let mut fired: u64 = 999;
        let result = ferric_engine_run(engine, -1, &mut fired);
        assert_eq!(result, FerricError::Ok);
        assert_eq!(fired, 0);
        ferric_engine_free(engine);
    }
}

#[test]
fn run_with_simple_rule() {
    unsafe {
        let engine = ferric_engine_new();
        let source = std::ffi::CString::new(
            r#"(defrule hello (initial-fact) => (printout t "hello" crlf))"#,
        )
        .unwrap();
        assert_eq!(
            ferric_engine_load_string(engine, source.as_ptr()),
            FerricError::Ok
        );
        ferric_engine_reset(engine);

        let mut fired: u64 = 0;
        let result = ferric_engine_run(engine, -1, &mut fired);
        assert_eq!(result, FerricError::Ok);
        assert_eq!(fired, 1);

        let channel = std::ffi::CString::new("t").unwrap();
        let output = ferric_engine_get_output(engine, channel.as_ptr());
        assert!(!output.is_null());
        let output_str = std::ffi::CStr::from_ptr(output).to_str().unwrap();
        assert!(output_str.contains("hello"), "output was: {output_str}");

        ferric_engine_free(engine);
    }
}

#[test]
fn run_with_limit() {
    unsafe {
        let engine = ferric_engine_new();
        let source = std::ffi::CString::new(
            "(defrule r1 (initial-fact) => (assert (a)))\n\
             (defrule r2 (a) => (assert (b)))\n\
             (defrule r3 (b) => (assert (c)))",
        )
        .unwrap();
        assert_eq!(
            ferric_engine_load_string(engine, source.as_ptr()),
            FerricError::Ok
        );
        ferric_engine_reset(engine);

        let mut fired: u64 = 0;
        let result = ferric_engine_run(engine, 2, &mut fired);
        assert_eq!(result, FerricError::Ok);
        assert_eq!(fired, 2);

        ferric_engine_free(engine);
    }
}

#[test]
fn run_null_out_fired_is_ok() {
    unsafe {
        let engine = ferric_engine_new();
        ferric_engine_reset(engine);
        let result = ferric_engine_run(engine, -1, std::ptr::null_mut());
        assert_eq!(result, FerricError::Ok);
        ferric_engine_free(engine);
    }
}

#[test]
fn run_null_engine() {
    unsafe {
        let mut fired: u64 = 0;
        let result = ferric_engine_run(std::ptr::null_mut(), -1, &mut fired);
        assert_eq!(result, FerricError::NullPointer);
    }
}

#[test]
fn step_empty_returns_agenda_empty() {
    unsafe {
        let engine = ferric_engine_new();
        ferric_engine_reset(engine);
        let mut status: i32 = 99;
        let result = ferric_engine_step(engine, &mut status);
        assert_eq!(result, FerricError::Ok);
        assert_eq!(status, 0);
        ferric_engine_free(engine);
    }
}

#[test]
fn step_fires_one_rule() {
    unsafe {
        let engine = ferric_engine_new();
        let source = std::ffi::CString::new(
            r#"(defrule test (initial-fact) => (printout t "stepped" crlf))"#,
        )
        .unwrap();
        assert_eq!(
            ferric_engine_load_string(engine, source.as_ptr()),
            FerricError::Ok
        );
        ferric_engine_reset(engine);

        let mut status: i32 = 0;
        let result = ferric_engine_step(engine, &mut status);
        assert_eq!(result, FerricError::Ok);
        assert_eq!(status, 1);

        // Second step should see an empty agenda
        let result = ferric_engine_step(engine, &mut status);
        assert_eq!(result, FerricError::Ok);
        assert_eq!(status, 0);

        ferric_engine_free(engine);
    }
}

#[test]
fn step_null_out_status_is_ok() {
    unsafe {
        let engine = ferric_engine_new();
        ferric_engine_reset(engine);
        let result = ferric_engine_step(engine, std::ptr::null_mut());
        assert_eq!(result, FerricError::Ok);
        ferric_engine_free(engine);
    }
}

#[test]
fn assert_string_ordered_fact() {
    unsafe {
        let engine = ferric_engine_new();
        ferric_engine_reset(engine);

        let source = std::ffi::CString::new("(assert (color red))").unwrap();
        let result = ferric_engine_assert_string(engine, source.as_ptr(), std::ptr::null_mut());
        assert_eq!(result, FerricError::Ok);

        ferric_engine_free(engine);
    }
}

#[test]
fn assert_string_null_engine() {
    unsafe {
        let source = std::ffi::CString::new("(assert (color red))").unwrap();
        let result = ferric_engine_assert_string(
            std::ptr::null_mut(),
            source.as_ptr(),
            std::ptr::null_mut(),
        );
        assert_eq!(result, FerricError::NullPointer);
    }
}

#[test]
fn assert_string_null_source() {
    unsafe {
        let engine = ferric_engine_new();
        let result = ferric_engine_assert_string(engine, std::ptr::null(), std::ptr::null_mut());
        assert_eq!(result, FerricError::NullPointer);
        ferric_engine_free(engine);
    }
}

#[test]
fn assert_string_invalid_syntax() {
    unsafe {
        let engine = ferric_engine_new();
        let source = std::ffi::CString::new("(assert (this is not closed").unwrap();
        let result = ferric_engine_assert_string(engine, source.as_ptr(), std::ptr::null_mut());
        assert_ne!(result, FerricError::Ok);
        ferric_engine_free(engine);
    }
}

#[test]
fn retract_nonexistent_fact() {
    unsafe {
        let engine = ferric_engine_new();
        ferric_engine_reset(engine);
        let result = ferric_engine_retract(engine, 0xDEAD_BEEF);
        assert_eq!(result, FerricError::NotFound);
        ferric_engine_free(engine);
    }
}

#[test]
fn retract_null_engine() {
    unsafe {
        let result = ferric_engine_retract(std::ptr::null_mut(), 1);
        assert_eq!(result, FerricError::NullPointer);
    }
}

#[test]
fn get_output_null_engine() {
    unsafe {
        let channel = std::ffi::CString::new("stdout").unwrap();
        let result = ferric_engine_get_output(std::ptr::null(), channel.as_ptr());
        assert!(result.is_null());
    }
}

#[test]
fn get_output_null_channel() {
    unsafe {
        let engine = ferric_engine_new();
        let result = ferric_engine_get_output(engine, std::ptr::null());
        assert!(result.is_null());
        ferric_engine_free(engine);
    }
}

#[test]
fn get_output_no_output_is_null() {
    unsafe {
        let engine = ferric_engine_new();
        ferric_engine_reset(engine);
        let channel = std::ffi::CString::new("stdout").unwrap();
        let result = ferric_engine_get_output(engine, channel.as_ptr());
        assert!(result.is_null());
        ferric_engine_free(engine);
    }
}

#[test]
fn full_load_reset_run_get_output_cycle() {
    unsafe {
        let engine = ferric_engine_new();

        let source = std::ffi::CString::new(
            r#"(defrule greet (initial-fact) => (printout t "Hello, FFI!" crlf))"#,
        )
        .unwrap();
        assert_eq!(
            ferric_engine_load_string(engine, source.as_ptr()),
            FerricError::Ok
        );

        assert_eq!(ferric_engine_reset(engine), FerricError::Ok);

        let mut fired: u64 = 0;
        assert_eq!(ferric_engine_run(engine, -1, &mut fired), FerricError::Ok);
        assert_eq!(fired, 1);

        let channel = std::ffi::CString::new("t").unwrap();
        let output = ferric_engine_get_output(engine, channel.as_ptr());
        assert!(!output.is_null());
        let output_str = std::ffi::CStr::from_ptr(output).to_str().unwrap();
        assert!(output_str.contains("Hello, FFI!"));

        ferric_engine_free(engine);
    }
}
