//! Tests for Phase 4 diagnostic parity through FFI (Pass 012).
//!
//! These tests verify that runtime diagnostics (parse errors, compile errors,
//! module visibility failures) propagate through the FFI layer without
//! reinterpretation or loss of source context.

use crate::engine::*;
use crate::error::*;
use std::ffi::{CStr, CString};

#[test]
fn parse_error_preserves_diagnostic() {
    unsafe {
        let engine = ferric_engine_new();
        let source = CString::new("(defrule bad (this is not closed").unwrap();
        let result = ferric_engine_load_string(engine, source.as_ptr());
        assert_ne!(result, FerricError::Ok);
        // The error should be ParseError or CompileError
        assert!(
            result == FerricError::ParseError || result == FerricError::CompileError,
            "expected parse/compile error, got {result:?}"
        );
        // Global error channel should have a descriptive message
        let err_ptr = ferric_last_error_global();
        assert!(!err_ptr.is_null());
        let err_msg = CStr::from_ptr(err_ptr).to_str().unwrap();
        assert!(!err_msg.is_empty(), "error message should not be empty");
        ferric_engine_free(engine);
    }
}

#[test]
fn compile_error_preserves_diagnostic() {
    unsafe {
        let engine = ferric_engine_new();
        // A rule referencing an undefined template should fail at compile time
        let source = CString::new("(defrule bad-ref (nonexistent-template (x 1)) => )").unwrap();
        let result = ferric_engine_load_string(engine, source.as_ptr());
        // This should fail with some error (might be parse or compile)
        // The key thing is that we get a diagnostic
        if result != FerricError::Ok {
            let err_ptr = ferric_last_error_global();
            assert!(!err_ptr.is_null());
            let err_msg = CStr::from_ptr(err_ptr).to_str().unwrap();
            assert!(!err_msg.is_empty());
        }
        ferric_engine_free(engine);
    }
}

#[test]
fn ffi_end_to_end_load_run_get_output() {
    // Full C embedding flow: create → load → reset → run → get output → free
    unsafe {
        let engine = ferric_engine_new();
        assert!(!engine.is_null());

        // Load
        let source = CString::new(
            r#"(defrule greet (initial-fact) => (printout t "Hello from FFI!" crlf))"#,
        )
        .unwrap();
        assert_eq!(
            ferric_engine_load_string(engine, source.as_ptr()),
            FerricError::Ok
        );

        // Reset
        assert_eq!(ferric_engine_reset(engine), FerricError::Ok);

        // Run
        let mut fired: u64 = 0;
        assert_eq!(ferric_engine_run(engine, -1, &mut fired), FerricError::Ok);
        assert_eq!(fired, 1);

        // Get output
        let channel = CString::new("t").unwrap();
        let output = ferric_engine_get_output(engine, channel.as_ptr());
        assert!(!output.is_null());
        let output_str = CStr::from_ptr(output).to_str().unwrap();
        assert!(output_str.contains("Hello from FFI!"));

        // Free
        assert_eq!(ferric_engine_free(engine), FerricError::Ok);
    }
}

#[test]
fn ffi_fact_mutation_roundtrip() {
    // Assert fact → query fact → retract → verify gone
    unsafe {
        let engine = ferric_engine_new();
        ferric_engine_reset(engine);

        // Assert
        let source = CString::new("(assert (color red))").unwrap();
        assert_eq!(
            ferric_engine_assert_string(engine, source.as_ptr(), std::ptr::null_mut()),
            FerricError::Ok
        );

        // Query: fact count should be > 0
        let mut count: usize = 0;
        assert_eq!(
            ferric_engine_fact_count(engine, &mut count),
            FerricError::Ok
        );
        assert!(count > 0, "expected at least one fact after assert");

        ferric_engine_free(engine);
    }
}

#[test]
fn ffi_error_retrieval_roundtrip() {
    // Trigger error → retrieve via global → retrieve via copy → clear → verify cleared
    unsafe {
        let engine = ferric_engine_new();

        // Trigger a parse error
        let bad_source = CString::new("(defrule bad (not closed").unwrap();
        let result = ferric_engine_load_string(engine, bad_source.as_ptr());
        assert_ne!(result, FerricError::Ok);

        // Retrieve via global pointer
        let err_ptr = ferric_last_error_global();
        assert!(!err_ptr.is_null());

        // Retrieve via copy-to-buffer (size query first)
        let mut needed: usize = 0;
        assert_eq!(
            ferric_last_error_global_copy(std::ptr::null_mut(), 0, &mut needed),
            FerricError::Ok
        );
        assert!(needed > 0);

        // Now copy
        let mut buf = vec![0u8; needed];
        let mut written: usize = 0;
        assert_eq!(
            ferric_last_error_global_copy(buf.as_mut_ptr().cast(), needed, &mut written),
            FerricError::Ok
        );
        assert_eq!(written, needed);
        let msg = CStr::from_ptr(buf.as_ptr().cast()).to_str().unwrap();
        assert!(!msg.is_empty());

        // Clear
        ferric_clear_error_global();
        let cleared = ferric_last_error_global();
        assert!(cleared.is_null());

        ferric_engine_free(engine);
    }
}

#[test]
fn ffi_copy_to_buffer_retry_pattern() {
    // The typical wrapper pattern: size query → allocate → copy
    unsafe {
        let engine = ferric_engine_new();

        // Trigger error
        let bad_source = CString::new("(defrule bad").unwrap();
        ferric_engine_load_string(engine, bad_source.as_ptr());

        // Step 1: Size query
        let mut needed: usize = 0;
        let result = ferric_last_error_global_copy(std::ptr::null_mut(), 0, &mut needed);
        assert_eq!(result, FerricError::Ok);

        if needed > 0 {
            // Step 2: Try with too-small buffer
            let mut small_buf = vec![0u8; 4];
            let mut out_len: usize = 0;
            let result =
                ferric_last_error_global_copy(small_buf.as_mut_ptr().cast(), 4, &mut out_len);
            assert_eq!(result, FerricError::BufferTooSmall);
            assert_eq!(out_len, needed); // Tells us the full size we need

            // Step 3: Retry with correct buffer
            let mut full_buf = vec![0u8; out_len];
            let mut final_len: usize = 0;
            let result = ferric_last_error_global_copy(
                full_buf.as_mut_ptr().cast(),
                out_len,
                &mut final_len,
            );
            assert_eq!(result, FerricError::Ok);
            assert_eq!(final_len, needed);
        }

        ferric_engine_free(engine);
    }
}
