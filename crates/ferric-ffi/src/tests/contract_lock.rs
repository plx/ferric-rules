//! FFI contract lock tests (Phase 6, Pass 005).
//!
//! These tests document and lock the stable C API surface against ABI drift.
//! They serve as explicit regression guards — if any of these tests fail after
//! a change, that change has broken a published contract.
//!
//! Unlike functional tests elsewhere in the suite, these tests are deliberately
//! narrow: each one targets a single documented invariant.

#[allow(unused_imports)]
use crate::engine::{
    ferric_engine_action_diagnostic_copy, ferric_engine_action_diagnostic_count,
    ferric_engine_assert_string, ferric_engine_clear_action_diagnostics, ferric_engine_clear_error,
    ferric_engine_fact_count, ferric_engine_free, ferric_engine_get_fact_field,
    ferric_engine_get_fact_field_count, ferric_engine_get_global, ferric_engine_get_output,
    ferric_engine_last_error, ferric_engine_last_error_copy, ferric_engine_load_string,
    ferric_engine_new, ferric_engine_new_with_config, ferric_engine_reset, ferric_engine_retract,
    ferric_engine_run, ferric_engine_step, FerricEngine,
};
use crate::error::{
    ferric_clear_error_global, ferric_last_error_global, ferric_last_error_global_copy, FerricError,
};
use crate::types::{
    ferric_string_free, ferric_value_array_free, ferric_value_free, FerricConfig,
    FerricConflictStrategy, FerricStringEncoding, FerricValue,
};
use std::os::raw::c_char;

// ---------------------------------------------------------------------------
// 1. Canonical exported function signatures
// ---------------------------------------------------------------------------

/// Verify that the stable C entry points exist with their documented signatures.
///
/// This is a compile-time contract test. If any function is renamed, removed,
/// or has its signature changed, this test will fail to compile.
#[test]
fn contract_lock_canonical_function_names_exist() {
    // engine lifecycle
    let _: unsafe extern "C" fn() -> *mut FerricEngine = ferric_engine_new;
    let _: unsafe extern "C" fn(*const FerricConfig) -> *mut FerricEngine =
        ferric_engine_new_with_config;
    let _: unsafe extern "C" fn(*mut FerricEngine) -> FerricError = ferric_engine_free;

    // source loading
    let _: unsafe extern "C" fn(*mut FerricEngine, *const c_char) -> FerricError =
        ferric_engine_load_string;

    // execution
    let _: unsafe extern "C" fn(*mut FerricEngine, i64, *mut u64) -> FerricError =
        ferric_engine_run;
    let _: unsafe extern "C" fn(*mut FerricEngine, *mut i32) -> FerricError = ferric_engine_step;

    // error retrieval — per-engine
    let _: unsafe extern "C" fn(*const FerricEngine) -> *const c_char = ferric_engine_last_error;
    let _: unsafe extern "C" fn(*mut FerricEngine) -> FerricError = ferric_engine_clear_error;
    let _: unsafe extern "C" fn(
        *const FerricEngine,
        *mut c_char,
        usize,
        *mut usize,
    ) -> FerricError = ferric_engine_last_error_copy;

    // error retrieval — global
    let _: unsafe extern "C" fn() -> *const c_char = ferric_last_error_global;
    let _: extern "C" fn() = ferric_clear_error_global;
    let _: unsafe extern "C" fn(*mut c_char, usize, *mut usize) -> FerricError =
        ferric_last_error_global_copy;

    // reset
    let _: unsafe extern "C" fn(*mut FerricEngine) -> FerricError = ferric_engine_reset;

    // fact mutation
    let _: unsafe extern "C" fn(*mut FerricEngine, *const c_char, *mut u64) -> FerricError =
        ferric_engine_assert_string;
    let _: unsafe extern "C" fn(*mut FerricEngine, u64) -> FerricError = ferric_engine_retract;

    // fact queries
    let _: unsafe extern "C" fn(*const FerricEngine, *mut usize) -> FerricError =
        ferric_engine_fact_count;
    let _: unsafe extern "C" fn(*const FerricEngine, u64, *mut usize) -> FerricError =
        ferric_engine_get_fact_field_count;
    let _: unsafe extern "C" fn(*const FerricEngine, u64, usize, *mut FerricValue) -> FerricError =
        ferric_engine_get_fact_field;

    // global variable queries
    let _: unsafe extern "C" fn(
        *const FerricEngine,
        *const c_char,
        *mut FerricValue,
    ) -> FerricError = ferric_engine_get_global;

    // output capture
    let _: unsafe extern "C" fn(*const FerricEngine, *const c_char) -> *const c_char =
        ferric_engine_get_output;

    // action diagnostics
    let _: unsafe extern "C" fn(*const FerricEngine, *mut usize) -> FerricError =
        ferric_engine_action_diagnostic_count;
    let _: unsafe extern "C" fn(
        *const FerricEngine,
        usize,
        *mut c_char,
        usize,
        *mut usize,
    ) -> FerricError = ferric_engine_action_diagnostic_copy;
    let _: unsafe extern "C" fn(*mut FerricEngine) -> FerricError =
        ferric_engine_clear_action_diagnostics;

    // value resource management
    let _: unsafe extern "C" fn(*mut c_char) = ferric_string_free;
    let _: unsafe extern "C" fn(*mut FerricValue) = ferric_value_free;
    let _: unsafe extern "C" fn(*mut FerricValue, usize) = ferric_value_array_free;
}

// ---------------------------------------------------------------------------
// 2. FerricError discriminant values are stable
// ---------------------------------------------------------------------------

/// Lock the numeric discriminants of all `FerricError` variants.
///
/// These are ABI-stable values embedded in bindings and header files.
/// They must never change.
#[test]
fn contract_lock_error_code_discriminants_are_stable() {
    assert_eq!(FerricError::Ok as i32, 0);
    assert_eq!(FerricError::NullPointer as i32, 1);
    assert_eq!(FerricError::ThreadViolation as i32, 2);
    assert_eq!(FerricError::NotFound as i32, 3);
    assert_eq!(FerricError::ParseError as i32, 4);
    assert_eq!(FerricError::CompileError as i32, 5);
    assert_eq!(FerricError::RuntimeError as i32, 6);
    assert_eq!(FerricError::IoError as i32, 7);
    assert_eq!(FerricError::BufferTooSmall as i32, 8);
    assert_eq!(FerricError::InvalidArgument as i32, 9);
    assert_eq!(FerricError::InternalError as i32, 99);
}

// ---------------------------------------------------------------------------
// 3. FerricConfig encoding and strategy discriminant values are stable
// ---------------------------------------------------------------------------

/// Lock the numeric discriminants of encoding and strategy enums.
#[test]
fn contract_lock_config_enum_discriminants_are_stable() {
    assert_eq!(FerricStringEncoding::Ascii as u32, 0);
    assert_eq!(FerricStringEncoding::Utf8 as u32, 1);
    assert_eq!(FerricStringEncoding::AsciiSymbolsUtf8Strings as u32, 2);

    assert_eq!(FerricConflictStrategy::Depth as u32, 0);
    assert_eq!(FerricConflictStrategy::Breadth as u32, 1);
    assert_eq!(FerricConflictStrategy::Lex as u32, 2);
    assert_eq!(FerricConflictStrategy::Mea as u32, 3);
}

// ---------------------------------------------------------------------------
// 4. Configured construction contract
// ---------------------------------------------------------------------------

/// Verify that each combination of encoding + strategy constructs successfully.
///
/// This locks the contract that valid raw discriminant values always produce
/// a non-null engine handle.
#[test]
fn contract_lock_configured_construction_all_valid_combos() {
    let encodings = [
        FerricStringEncoding::Ascii.as_raw(),
        FerricStringEncoding::Utf8.as_raw(),
        FerricStringEncoding::AsciiSymbolsUtf8Strings.as_raw(),
    ];
    let strategies = [
        FerricConflictStrategy::Depth.as_raw(),
        FerricConflictStrategy::Breadth.as_raw(),
        FerricConflictStrategy::Lex.as_raw(),
        FerricConflictStrategy::Mea.as_raw(),
    ];

    for &enc in &encodings {
        for &strat in &strategies {
            let config = FerricConfig {
                string_encoding: enc,
                strategy: strat,
                max_call_depth: 64,
            };
            unsafe {
                let engine = ferric_engine_new_with_config(&config);
                assert!(
                    !engine.is_null(),
                    "engine should be non-null for encoding={enc} strategy={strat}"
                );
                ferric_engine_free(engine);
            }
        }
    }
}

/// Invalid encoding discriminant produces null and populates the global error channel.
#[test]
fn contract_lock_invalid_config_produces_null_and_global_error() {
    let config = FerricConfig {
        string_encoding: 0xFF,
        strategy: FerricConflictStrategy::Depth.as_raw(),
        max_call_depth: 64,
    };
    unsafe {
        let engine = ferric_engine_new_with_config(&config);
        assert!(engine.is_null(), "invalid config must return null");

        let err_ptr = ferric_last_error_global();
        assert!(
            !err_ptr.is_null(),
            "global error channel must be populated on construction failure"
        );
        let msg = std::ffi::CStr::from_ptr(err_ptr).to_string_lossy();
        assert!(
            msg.contains("invalid string_encoding") || msg.contains("invalid"),
            "error should describe the invalid field, got: {msg}"
        );
    }
}

// ---------------------------------------------------------------------------
// 5. Thread-affinity contract
// ---------------------------------------------------------------------------

/// Operations from the creating thread always succeed (affinity check passes).
#[test]
fn contract_lock_thread_affinity_same_thread_succeeds() {
    unsafe {
        let engine = ferric_engine_new();
        assert!(!engine.is_null());
        assert_eq!(ferric_engine_reset(engine), FerricError::Ok);
        let source = std::ffi::CString::new("(defrule r (initial-fact) => )").unwrap();
        assert_eq!(
            ferric_engine_load_string(engine, source.as_ptr()),
            FerricError::Ok
        );
        let mut fired: u64 = 0;
        assert_eq!(ferric_engine_run(engine, -1, &mut fired), FerricError::Ok);
        ferric_engine_free(engine);
    }
}

/// Operations from a different thread return `ThreadViolation`.
#[test]
fn contract_lock_thread_affinity_violation_returns_error_code() {
    unsafe {
        let engine = ferric_engine_new();
        let engine_addr = engine as usize;

        let result = std::thread::spawn(move || {
            let eng = engine_addr as *mut FerricEngine;
            ferric_engine_reset(eng)
        })
        .join()
        .unwrap();

        assert_eq!(
            result,
            FerricError::ThreadViolation,
            "cross-thread access must return ThreadViolation"
        );

        // Global error channel must be populated by the thread-violation path.
        // NOTE: global error is thread-local, so we check on the creating thread
        // by triggering another violation and checking inline.
        let engine_addr2 = engine as usize;
        let result2 = std::thread::spawn(move || {
            let eng = engine_addr2 as *mut FerricEngine;
            // Can't read the creating thread's global error from here,
            // but we can confirm the code is correct.
            ferric_engine_run(eng, -1, std::ptr::null_mut())
        })
        .join()
        .unwrap();
        assert_eq!(result2, FerricError::ThreadViolation);

        ferric_engine_free(engine);
    }
}

// ---------------------------------------------------------------------------
// 6. Copy-to-buffer semantics contract
// ---------------------------------------------------------------------------

/// Lock: `buf = NULL, buf_len = 0` is the canonical size-query form → returns Ok.
#[test]
fn contract_lock_copy_to_buffer_size_query_form() {
    crate::error::set_global_error("contract lock size query".to_string());
    unsafe {
        let mut out_len: usize = 0;
        let result = ferric_last_error_global_copy(std::ptr::null_mut(), 0, &mut out_len);
        assert_eq!(
            result,
            FerricError::Ok,
            "null buf + zero len must return Ok (size query)"
        );
        // out_len must report message length + 1 (NUL terminator)
        let expected = "contract lock size query".len() + 1;
        assert_eq!(
            out_len, expected,
            "*out_len must equal message.len() + 1 for size query"
        );
    }
    crate::error::clear_global_error();
}

/// Lock: too-small buffer → truncates to `buf_len - 1` bytes + NUL, returns `BufferTooSmall`.
#[test]
fn contract_lock_copy_to_buffer_truncation_semantics() {
    let msg = "contract lock truncation test message";
    crate::error::set_global_error(msg.to_string());
    unsafe {
        let buf_len: usize = 8;
        let mut buf = vec![0u8; buf_len];
        let mut out_len: usize = 0;
        let result =
            ferric_last_error_global_copy(buf.as_mut_ptr().cast::<c_char>(), buf_len, &mut out_len);

        assert_eq!(
            result,
            FerricError::BufferTooSmall,
            "truncated copy must return BufferTooSmall"
        );
        // *out_len must report the full required size, not what was written
        assert_eq!(
            out_len,
            msg.len() + 1,
            "*out_len must report full needed size even when truncated"
        );
        // Exactly buf_len - 1 bytes of the message are written, then NUL
        assert_eq!(
            &buf[..buf_len - 1],
            &msg.as_bytes()[..buf_len - 1],
            "first buf_len-1 bytes must match message prefix"
        );
        assert_eq!(buf[buf_len - 1], 0, "last byte must be NUL terminator");
    }
    crate::error::clear_global_error();
}

/// Lock: no error present → returns `NotFound` before inspecting `buf`/`buf_len`.
#[test]
fn contract_lock_copy_to_buffer_not_found_when_no_error() {
    crate::error::clear_global_error();
    unsafe {
        // Even with a valid buffer, NotFound is returned immediately when no error is stored.
        let mut buf = vec![0u8; 64];
        let mut out_len: usize = 999;
        let result = ferric_last_error_global_copy(
            buf.as_mut_ptr().cast::<c_char>(),
            buf.len(),
            &mut out_len,
        );
        assert_eq!(
            result,
            FerricError::NotFound,
            "must return NotFound when no error is stored"
        );
        assert_eq!(out_len, 0, "*out_len must be 0 when NotFound");
        // Buffer must be untouched (still all zeros)
        assert!(
            buf.iter().all(|&b| b == 0),
            "buffer must not be written when NotFound"
        );
    }
}

/// Lock: `out_len = NULL` → returns `InvalidArgument` regardless of other args.
#[test]
fn contract_lock_copy_to_buffer_null_out_len_is_invalid_argument() {
    crate::error::set_global_error("something".to_string());
    unsafe {
        // Null out_len must be rejected immediately
        let mut buf = vec![0u8; 64];
        let result = ferric_last_error_global_copy(
            buf.as_mut_ptr().cast::<c_char>(),
            buf.len(),
            std::ptr::null_mut(),
        );
        assert_eq!(
            result,
            FerricError::InvalidArgument,
            "null out_len must return InvalidArgument"
        );
    }
    crate::error::clear_global_error();
}

// ---------------------------------------------------------------------------
// 7. Fact-ID round-trip contract
// ---------------------------------------------------------------------------

/// Assert a fact, get its ID, retract by ID, verify it is gone.
///
/// This locks the assert → `fact_id` → retract → not-found chain.
#[test]
fn contract_lock_assert_retract_fact_id_roundtrip() {
    unsafe {
        let engine = ferric_engine_new();
        ferric_engine_reset(engine);

        // Assert a fact and capture its ID.
        let source = std::ffi::CString::new("(assert (contract-lock-fact 99))").unwrap();
        let mut fact_id: u64 = 0;
        assert_eq!(
            ferric_engine_assert_string(engine, source.as_ptr(), &mut fact_id),
            FerricError::Ok
        );
        assert_ne!(fact_id, 0, "assert must produce a non-zero fact ID");

        // Fact count should include our new fact.
        let mut count_before: usize = 0;
        assert_eq!(
            ferric_engine_fact_count(engine, &mut count_before),
            FerricError::Ok
        );
        assert!(
            count_before >= 1,
            "fact count must be at least 1 after assert"
        );

        // Retract by fact ID.
        assert_eq!(
            ferric_engine_retract(engine, fact_id),
            FerricError::Ok,
            "retract by valid fact ID must succeed"
        );

        // Second retract of the same ID must fail with NotFound.
        assert_eq!(
            ferric_engine_retract(engine, fact_id),
            FerricError::NotFound,
            "retracting an already-retracted fact must return NotFound"
        );

        ferric_engine_free(engine);
    }
}

// ---------------------------------------------------------------------------
// 8. Action diagnostics lifecycle contract
// ---------------------------------------------------------------------------

/// Run rules → count diagnostics → copy one → clear → verify empty.
///
/// Locks the full lifecycle of the action diagnostic API.
#[test]
fn contract_lock_action_diagnostics_lifecycle() {
    unsafe {
        let engine = ferric_engine_new();

        // A program that produces a runtime action diagnostic: calling a
        // function in a module that exports nothing.
        let source = std::ffi::CString::new(
            r"
            (defmodule MATH (export ?NONE))
            (deffunction add (?x ?y) (+ ?x ?y))
            (defmodule MAIN)
            (defrule trigger (go) => (printout t (MATH::add 1 2) crlf))
            (deffacts startup (go))
            ",
        )
        .unwrap();

        assert_eq!(
            ferric_engine_load_string(engine, source.as_ptr()),
            FerricError::Ok
        );
        assert_eq!(ferric_engine_reset(engine), FerricError::Ok);

        let mut fired: u64 = 0;
        assert_eq!(ferric_engine_run(engine, -1, &mut fired), FerricError::Ok);
        assert_eq!(fired, 1, "one rule should have fired");

        // Count: at least one diagnostic must be recorded.
        let mut count: usize = 0;
        assert_eq!(
            ferric_engine_action_diagnostic_count(engine, &mut count),
            FerricError::Ok
        );
        assert!(
            count >= 1,
            "at least one action diagnostic must be captured after visibility error"
        );

        // Size query for diagnostic at index 0.
        let mut needed: usize = 0;
        assert_eq!(
            ferric_engine_action_diagnostic_copy(engine, 0, std::ptr::null_mut(), 0, &mut needed),
            FerricError::Ok,
            "size query for diagnostic 0 must return Ok"
        );
        assert!(
            needed > 1,
            "diagnostic message must have non-trivial length"
        );

        // Full copy into correctly-sized buffer.
        let mut buf = vec![0u8; needed];
        let mut out_len: usize = 0;
        assert_eq!(
            ferric_engine_action_diagnostic_copy(
                engine,
                0,
                buf.as_mut_ptr().cast(),
                needed,
                &mut out_len
            ),
            FerricError::Ok
        );
        assert_eq!(
            out_len, needed,
            "*out_len must equal needed after full copy"
        );
        let msg = std::ffi::CStr::from_ptr(buf.as_ptr().cast())
            .to_str()
            .unwrap();
        assert!(!msg.is_empty(), "diagnostic message must not be empty");

        // Clear.
        assert_eq!(
            ferric_engine_clear_action_diagnostics(engine),
            FerricError::Ok
        );

        // After clear, count must be 0.
        let mut count_after: usize = usize::MAX;
        assert_eq!(
            ferric_engine_action_diagnostic_count(engine, &mut count_after),
            FerricError::Ok
        );
        assert_eq!(count_after, 0, "diagnostic count must be 0 after clear");

        // After clear, copy at index 0 must return NotFound.
        let mut dummy_len: usize = 999;
        assert_eq!(
            ferric_engine_action_diagnostic_copy(
                engine,
                0,
                std::ptr::null_mut(),
                0,
                &mut dummy_len
            ),
            FerricError::NotFound,
            "copy after clear must return NotFound"
        );
        assert_eq!(dummy_len, 0, "*out_len must be 0 on NotFound");

        ferric_engine_free(engine);
    }
}

// ---------------------------------------------------------------------------
// 9. Null-pointer safety contract (universal guard)
// ---------------------------------------------------------------------------

/// Null engine pointers must be safely handled by all engine entry points.
///
/// This locks the contract that no entry point panics or produces UB on null input.
#[test]
fn contract_lock_null_engine_pointer_returns_null_pointer_error() {
    unsafe {
        let null_engine: *mut FerricEngine = std::ptr::null_mut();
        let null_const_engine: *const FerricEngine = std::ptr::null();
        let dummy_src = std::ffi::CString::new("(assert (x))").unwrap();

        assert_eq!(ferric_engine_free(null_engine), FerricError::Ok);
        assert_eq!(
            ferric_engine_load_string(null_engine, dummy_src.as_ptr()),
            FerricError::NullPointer
        );
        assert_eq!(ferric_engine_reset(null_engine), FerricError::NullPointer);
        assert_eq!(
            ferric_engine_run(null_engine, -1, std::ptr::null_mut()),
            FerricError::NullPointer
        );
        assert_eq!(
            ferric_engine_step(null_engine, std::ptr::null_mut()),
            FerricError::NullPointer
        );
        assert_eq!(
            ferric_engine_assert_string(null_engine, dummy_src.as_ptr(), std::ptr::null_mut()),
            FerricError::NullPointer
        );
        assert_eq!(
            ferric_engine_retract(null_engine, 1),
            FerricError::NullPointer
        );
        assert_eq!(
            ferric_engine_clear_error(null_engine),
            FerricError::NullPointer
        );
        assert_eq!(
            ferric_engine_clear_action_diagnostics(null_engine),
            FerricError::NullPointer
        );

        // Read-only entry points on const null pointers
        let mut count: usize = 0;
        assert_eq!(
            ferric_engine_fact_count(null_const_engine, &mut count),
            FerricError::NullPointer
        );
        assert_eq!(
            ferric_engine_action_diagnostic_count(null_const_engine, &mut count),
            FerricError::NullPointer
        );

        // ferric_engine_last_error on null → null (documented behavior, not an error code)
        let ptr = ferric_engine_last_error(null_const_engine);
        assert!(ptr.is_null(), "last_error on null engine must return null");

        // ferric_engine_get_output on null → null
        let channel = std::ffi::CString::new("t").unwrap();
        let out = ferric_engine_get_output(null_const_engine, channel.as_ptr());
        assert!(out.is_null(), "get_output on null engine must return null");
    }
}

// ---------------------------------------------------------------------------
// 10. FerricConfig field layout contract
// ---------------------------------------------------------------------------

/// Verify the struct layout of `FerricConfig`: field names and their default values.
///
/// This locks the contract that the C-facing config struct has the documented fields
/// with the documented defaults.
#[test]
fn contract_lock_ferric_config_default_values() {
    let cfg = FerricConfig::default();
    assert_eq!(
        cfg.string_encoding,
        FerricStringEncoding::Utf8.as_raw(),
        "default encoding must be Utf8"
    );
    assert_eq!(
        cfg.strategy,
        FerricConflictStrategy::Depth.as_raw(),
        "default strategy must be Depth"
    );
    assert_eq!(cfg.max_call_depth, 64, "default max_call_depth must be 64");
}

// ---------------------------------------------------------------------------
// 11. Error channel isolation contract
// ---------------------------------------------------------------------------

/// Load failures write to both the global channel and the per-engine channel.
///
/// This locks the documented dual-write behavior on error.
#[test]
fn contract_lock_load_error_populates_both_channels() {
    crate::error::clear_global_error();
    unsafe {
        let engine = ferric_engine_new();

        let bad_source = std::ffi::CString::new("(defrule bad (not closed").unwrap();
        let result = ferric_engine_load_string(engine, bad_source.as_ptr());
        assert_ne!(result, FerricError::Ok);

        // Global error channel must be populated.
        let global_ptr = ferric_last_error_global();
        assert!(
            !global_ptr.is_null(),
            "global error channel must be set after load failure"
        );

        // Per-engine error channel must also be populated.
        let engine_ptr = ferric_engine_last_error(engine);
        assert!(
            !engine_ptr.is_null(),
            "per-engine error channel must be set after load failure"
        );

        // Both messages must be non-empty strings.
        let global_msg = std::ffi::CStr::from_ptr(global_ptr).to_str().unwrap();
        let engine_msg = std::ffi::CStr::from_ptr(engine_ptr).to_str().unwrap();
        assert!(!global_msg.is_empty());
        assert!(!engine_msg.is_empty());

        ferric_engine_free(engine);
    }
}

/// Clearing the per-engine error does not affect the global channel.
#[test]
fn contract_lock_clear_engine_error_does_not_affect_global_channel() {
    unsafe {
        let engine = ferric_engine_new();

        // Trigger an error to populate both channels.
        let bad = std::ffi::CString::new("(defrule bad (not closed").unwrap();
        ferric_engine_load_string(engine, bad.as_ptr());

        // Global channel is set.
        assert!(!ferric_last_error_global().is_null());

        // Clear the per-engine channel.
        assert_eq!(ferric_engine_clear_error(engine), FerricError::Ok);

        // Per-engine channel is now empty.
        assert!(
            ferric_engine_last_error(engine).is_null(),
            "per-engine error must be cleared"
        );

        // Global channel is still set (clearing per-engine does not clear global).
        assert!(
            !ferric_last_error_global().is_null(),
            "global error channel must be unaffected by per-engine clear"
        );

        crate::error::clear_global_error();
        ferric_engine_free(engine);
    }
}
