//! Tests for FFI action-diagnostic retrieval APIs.

use crate::engine::{
    ferric_engine_action_diagnostic_copy, ferric_engine_action_diagnostic_count,
    ferric_engine_clear_action_diagnostics, ferric_engine_free, ferric_engine_load_string,
    ferric_engine_new, ferric_engine_reset, ferric_engine_run,
};
use crate::error::FerricError;
use std::ffi::CString;

fn load_visibility_warning_program(engine: *mut crate::engine::FerricEngine) {
    // Runtime warning: MAIN cannot call non-exported MATH::add.
    let source = CString::new(
        r"
        (defmodule MATH (export ?NONE))
        (deffunction add (?x ?y) (+ ?x ?y))

        (defmodule MAIN)
        (defrule test-call (go) => (printout t (MATH::add 3 4) crlf))
        (deffacts startup (go))
        ",
    )
    .unwrap();

    unsafe {
        assert_eq!(
            ferric_engine_load_string(engine, source.as_ptr()),
            FerricError::Ok
        );
        assert_eq!(ferric_engine_reset(engine), FerricError::Ok);
    }
}

#[test]
fn action_diagnostic_count_is_zero_initially() {
    unsafe {
        let engine = ferric_engine_new();
        let mut count: usize = usize::MAX;
        assert_eq!(
            ferric_engine_action_diagnostic_count(engine, &mut count),
            FerricError::Ok
        );
        assert_eq!(count, 0);
        ferric_engine_free(engine);
    }
}

#[test]
fn action_diagnostic_copy_returns_not_found_when_empty() {
    unsafe {
        let engine = ferric_engine_new();
        let mut out_len: usize = usize::MAX;
        let code =
            ferric_engine_action_diagnostic_copy(engine, 0, std::ptr::null_mut(), 0, &mut out_len);
        assert_eq!(code, FerricError::NotFound);
        assert_eq!(out_len, 0);
        ferric_engine_free(engine);
    }
}

#[test]
fn action_diagnostics_capture_phase4_visibility_warning() {
    unsafe {
        let engine = ferric_engine_new();
        load_visibility_warning_program(engine);

        let mut fired: u64 = 0;
        assert_eq!(ferric_engine_run(engine, -1, &mut fired), FerricError::Ok);
        assert_eq!(fired, 1);

        let mut count: usize = 0;
        assert_eq!(
            ferric_engine_action_diagnostic_count(engine, &mut count),
            FerricError::Ok
        );
        assert!(count >= 1, "expected at least one action diagnostic");

        // Size query
        let mut needed: usize = 0;
        assert_eq!(
            ferric_engine_action_diagnostic_copy(engine, 0, std::ptr::null_mut(), 0, &mut needed),
            FerricError::Ok
        );
        assert!(needed > 1);

        let mut buf = vec![0_u8; needed];
        let mut out_len: usize = 0;
        assert_eq!(
            ferric_engine_action_diagnostic_copy(
                engine,
                0,
                buf.as_mut_ptr().cast(),
                buf.len(),
                &mut out_len
            ),
            FerricError::Ok
        );
        assert_eq!(out_len, needed);

        let msg = std::ffi::CStr::from_ptr(buf.as_ptr().cast())
            .to_str()
            .unwrap();
        assert!(
            msg.contains("not visible")
                || msg.contains("not accessible")
                || msg.contains("NotVisible"),
            "expected visibility diagnostic message, got: {msg}"
        );

        ferric_engine_free(engine);
    }
}

#[test]
fn clear_action_diagnostics_resets_count() {
    unsafe {
        let engine = ferric_engine_new();
        load_visibility_warning_program(engine);
        let mut fired: u64 = 0;
        assert_eq!(ferric_engine_run(engine, -1, &mut fired), FerricError::Ok);

        let mut count_before: usize = 0;
        assert_eq!(
            ferric_engine_action_diagnostic_count(engine, &mut count_before),
            FerricError::Ok
        );
        assert!(count_before > 0);

        assert_eq!(
            ferric_engine_clear_action_diagnostics(engine),
            FerricError::Ok
        );

        let mut count_after: usize = usize::MAX;
        assert_eq!(
            ferric_engine_action_diagnostic_count(engine, &mut count_after),
            FerricError::Ok
        );
        assert_eq!(count_after, 0);

        ferric_engine_free(engine);
    }
}

#[test]
fn clear_action_diagnostics_thread_violation_preserves_messages() {
    unsafe {
        let engine = ferric_engine_new();
        load_visibility_warning_program(engine);
        let mut fired: u64 = 0;
        assert_eq!(ferric_engine_run(engine, -1, &mut fired), FerricError::Ok);

        let engine_addr = engine as usize;
        let result = std::thread::spawn(move || {
            let engine = engine_addr as *mut crate::engine::FerricEngine;
            ferric_engine_clear_action_diagnostics(engine)
        })
        .join()
        .unwrap();
        assert_eq!(result, FerricError::ThreadViolation);

        let mut count: usize = 0;
        assert_eq!(
            ferric_engine_action_diagnostic_count(engine, &mut count),
            FerricError::Ok
        );
        assert!(
            count > 0,
            "diagnostics should remain after thread violation"
        );

        ferric_engine_free(engine);
    }
}

#[test]
fn action_diagnostic_api_null_pointer_paths() {
    unsafe {
        let mut count: usize = 0;
        assert_eq!(
            ferric_engine_action_diagnostic_count(std::ptr::null(), &mut count),
            FerricError::NullPointer
        );

        assert_eq!(
            ferric_engine_action_diagnostic_count(std::ptr::null(), std::ptr::null_mut()),
            FerricError::NullPointer
        );

        let mut out_len: usize = 0;
        assert_eq!(
            ferric_engine_action_diagnostic_copy(
                std::ptr::null(),
                0,
                std::ptr::null_mut(),
                0,
                &mut out_len
            ),
            FerricError::NullPointer
        );
        assert_eq!(out_len, 0);

        assert_eq!(
            ferric_engine_clear_action_diagnostics(std::ptr::null_mut()),
            FerricError::NullPointer
        );
    }
}

#[test]
fn action_diagnostic_copy_requires_out_len() {
    unsafe {
        let engine = ferric_engine_new();
        let mut buf = [0_i8; 8];
        assert_eq!(
            ferric_engine_action_diagnostic_copy(
                engine,
                0,
                buf.as_mut_ptr(),
                buf.len(),
                std::ptr::null_mut()
            ),
            FerricError::InvalidArgument
        );
        ferric_engine_free(engine);
    }
}
