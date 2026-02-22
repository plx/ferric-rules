//! Tests for FFI value types, conversion, queries, and resource management (Pass 007).

use std::ffi::{CStr, CString};
use std::ptr;

use slotmap::Key as _;

use crate::engine::{
    ferric_engine_fact_count, ferric_engine_free, ferric_engine_get_fact_field,
    ferric_engine_get_fact_field_count, ferric_engine_get_global, ferric_engine_load_string,
    ferric_engine_new, ferric_engine_reset, ferric_engine_retract, FerricEngine,
};
use crate::error::FerricError;
use crate::types::{
    ferric_string_free, ferric_value_array_free, ferric_value_free, FerricValue, FerricValueType,
};

// ---------------------------------------------------------------------------
// Helper: get the first user-visible fact ID from within the crate
// ---------------------------------------------------------------------------

/// Return the FFI u64 representation of the first user-visible fact in the engine.
///
/// Panics if there are no user-visible facts (used in tests that assert at least one fact).
unsafe fn first_fact_id(handle: &FerricEngine) -> u64 {
    let (fact_id, _) = handle
        .engine
        .facts()
        .expect("facts() must not fail in test context")
        .next()
        .expect("expected at least one user-visible fact");
    fact_id.data().as_ffi()
}

// ---------------------------------------------------------------------------
// Value type and conversion tests
// ---------------------------------------------------------------------------

#[test]
fn value_void_is_zeroed() {
    let v = FerricValue::void();
    assert_eq!(v.value_type, FerricValueType::Void);
    assert_eq!(v.integer, 0);
    assert!(v.float.abs() < f64::EPSILON);
    assert!(v.string_ptr.is_null());
    assert!(v.multifield_ptr.is_null());
    assert_eq!(v.multifield_len, 0);
    assert_eq!(v.external_type_id, 0);
    assert!(v.external_pointer.is_null());
}

#[test]
fn value_integer_conversion() {
    unsafe {
        let engine = ferric_engine_new();
        let source = CString::new("(assert (count 42))").unwrap();
        assert_eq!(
            ferric_engine_load_string(engine, source.as_ptr()),
            FerricError::Ok
        );
        ferric_engine_reset(engine);
        // Re-assert after reset (deffacts approach via deffacts)
        let source2 = CString::new("(assert (count 42))").unwrap();
        assert_eq!(
            ferric_engine_load_string(engine, source2.as_ptr()),
            FerricError::Ok
        );

        let handle = &*engine;
        let fid = first_fact_id(handle);

        let mut out = FerricValue::void();
        // Field 0 is the integer value 42
        let result = ferric_engine_get_fact_field(engine, fid, 0, &mut out);
        assert_eq!(result, FerricError::Ok);
        assert_eq!(out.value_type, FerricValueType::Integer);
        assert_eq!(out.integer, 42);

        ferric_engine_free(engine);
    }
}

#[test]
fn value_float_conversion() {
    unsafe {
        let engine = ferric_engine_new();
        ferric_engine_reset(engine);
        let source = CString::new("(assert (temp 98.6))").unwrap();
        assert_eq!(
            ferric_engine_load_string(engine, source.as_ptr()),
            FerricError::Ok
        );

        let handle = &*engine;
        let fid = first_fact_id(handle);

        let mut out = FerricValue::void();
        let result = ferric_engine_get_fact_field(engine, fid, 0, &mut out);
        assert_eq!(result, FerricError::Ok);
        assert_eq!(out.value_type, FerricValueType::Float);
        assert!((out.float - 98.6_f64).abs() < 1e-9);

        ferric_engine_free(engine);
    }
}

#[test]
fn value_symbol_conversion() {
    unsafe {
        let engine = ferric_engine_new();
        ferric_engine_reset(engine);
        let source = CString::new("(assert (color red))").unwrap();
        assert_eq!(
            ferric_engine_load_string(engine, source.as_ptr()),
            FerricError::Ok
        );

        let handle = &*engine;
        let fid = first_fact_id(handle);

        let mut out = FerricValue::void();
        // Field 0 is the symbol `red`
        let result = ferric_engine_get_fact_field(engine, fid, 0, &mut out);
        assert_eq!(result, FerricError::Ok);
        assert_eq!(out.value_type, FerricValueType::Symbol);
        assert!(!out.string_ptr.is_null());

        let s = CStr::from_ptr(out.string_ptr).to_str().unwrap();
        assert_eq!(s, "red");

        ferric_string_free(out.string_ptr);
        ferric_engine_free(engine);
    }
}

#[test]
fn value_string_conversion() {
    unsafe {
        let engine = ferric_engine_new();
        ferric_engine_reset(engine);
        let source = CString::new(r#"(assert (msg "hello world"))"#).unwrap();
        assert_eq!(
            ferric_engine_load_string(engine, source.as_ptr()),
            FerricError::Ok
        );

        let handle = &*engine;
        let fid = first_fact_id(handle);

        let mut out = FerricValue::void();
        // Field 0 is the string "hello world"
        let result = ferric_engine_get_fact_field(engine, fid, 0, &mut out);
        assert_eq!(result, FerricError::Ok);
        assert_eq!(out.value_type, FerricValueType::String);
        assert!(!out.string_ptr.is_null());

        let s = CStr::from_ptr(out.string_ptr).to_str().unwrap();
        assert_eq!(s, "hello world");

        ferric_string_free(out.string_ptr);
        ferric_engine_free(engine);
    }
}

// ---------------------------------------------------------------------------
// Resource management tests
// ---------------------------------------------------------------------------

#[test]
fn string_free_null_is_safe() {
    unsafe {
        ferric_string_free(ptr::null_mut());
    }
}

#[test]
fn value_free_null_is_safe() {
    unsafe {
        ferric_value_free(ptr::null_mut());
    }
}

#[test]
fn value_array_free_null_is_safe() {
    unsafe {
        ferric_value_array_free(ptr::null_mut(), 0);
    }
}

#[test]
fn value_free_releases_string() {
    unsafe {
        let engine = ferric_engine_new();
        ferric_engine_reset(engine);
        let source = CString::new("(assert (label hello))").unwrap();
        assert_eq!(
            ferric_engine_load_string(engine, source.as_ptr()),
            FerricError::Ok
        );

        let handle = &*engine;
        let fid = first_fact_id(handle);

        let mut out = FerricValue::void();
        let result = ferric_engine_get_fact_field(engine, fid, 0, &mut out);
        assert_eq!(result, FerricError::Ok);
        assert_eq!(out.value_type, FerricValueType::Symbol);

        // Free via ferric_value_free — must not crash or leak
        ferric_value_free(&mut out);

        ferric_engine_free(engine);
    }
}

// ---------------------------------------------------------------------------
// Fact query tests
// ---------------------------------------------------------------------------

#[test]
fn fact_count_empty_engine() {
    unsafe {
        let engine = ferric_engine_new();
        ferric_engine_reset(engine);
        let mut count: usize = 999;
        let result = ferric_engine_fact_count(engine, &mut count);
        assert_eq!(result, FerricError::Ok);
        // initial-fact is excluded, so 0 user-visible facts
        assert_eq!(count, 0);
        ferric_engine_free(engine);
    }
}

#[test]
fn fact_count_after_assert() {
    unsafe {
        let engine = ferric_engine_new();
        ferric_engine_reset(engine);

        let s1 = CString::new("(assert (a 1))").unwrap();
        let s2 = CString::new("(assert (b 2))").unwrap();
        let s3 = CString::new("(assert (c 3))").unwrap();
        assert_eq!(
            ferric_engine_load_string(engine, s1.as_ptr()),
            FerricError::Ok
        );
        assert_eq!(
            ferric_engine_load_string(engine, s2.as_ptr()),
            FerricError::Ok
        );
        assert_eq!(
            ferric_engine_load_string(engine, s3.as_ptr()),
            FerricError::Ok
        );

        let mut count: usize = 0;
        let result = ferric_engine_fact_count(engine, &mut count);
        assert_eq!(result, FerricError::Ok);
        assert_eq!(count, 3);

        ferric_engine_free(engine);
    }
}

#[test]
fn fact_field_count_ordered() {
    unsafe {
        let engine = ferric_engine_new();
        ferric_engine_reset(engine);
        let source = CString::new("(assert (color red blue))").unwrap();
        assert_eq!(
            ferric_engine_load_string(engine, source.as_ptr()),
            FerricError::Ok
        );

        let handle = &*engine;
        let fid = first_fact_id(handle);

        let mut count: usize = 0;
        let result = ferric_engine_get_fact_field_count(engine, fid, &mut count);
        assert_eq!(result, FerricError::Ok);
        assert_eq!(count, 2);

        ferric_engine_free(engine);
    }
}

#[test]
fn fact_field_index_out_of_bounds() {
    unsafe {
        let engine = ferric_engine_new();
        ferric_engine_reset(engine);
        let source = CString::new("(assert (x 1))").unwrap();
        assert_eq!(
            ferric_engine_load_string(engine, source.as_ptr()),
            FerricError::Ok
        );

        let handle = &*engine;
        let fid = first_fact_id(handle);

        let mut out = FerricValue::void();
        let result = ferric_engine_get_fact_field(engine, fid, 99, &mut out);
        assert_eq!(result, FerricError::InvalidArgument);

        ferric_engine_free(engine);
    }
}

#[test]
fn fact_not_found() {
    unsafe {
        let engine = ferric_engine_new();
        ferric_engine_reset(engine);

        // Use an obviously invalid fact ID
        let mut out = FerricValue::void();
        let result = ferric_engine_get_fact_field(engine, 0xDEAD_BEEF_u64, 0, &mut out);
        assert_eq!(result, FerricError::NotFound);

        ferric_engine_free(engine);
    }
}

#[test]
fn fact_count_null_engine() {
    unsafe {
        let mut count: usize = 0;
        let result = ferric_engine_fact_count(ptr::null(), &mut count);
        assert_eq!(result, FerricError::NullPointer);
    }
}

#[test]
fn fact_count_null_out() {
    unsafe {
        let engine = ferric_engine_new();
        ferric_engine_reset(engine);
        let result = ferric_engine_fact_count(engine, ptr::null_mut());
        assert_eq!(result, FerricError::NullPointer);
        ferric_engine_free(engine);
    }
}

// ---------------------------------------------------------------------------
// Global variable tests
// ---------------------------------------------------------------------------

#[test]
fn get_global_not_found() {
    unsafe {
        let engine = ferric_engine_new();
        ferric_engine_reset(engine);
        let name = CString::new("nonexistent").unwrap();
        let mut out = FerricValue::void();
        let result = ferric_engine_get_global(engine, name.as_ptr(), &mut out);
        assert_eq!(result, FerricError::NotFound);
        ferric_engine_free(engine);
    }
}

#[test]
fn get_global_null_engine() {
    unsafe {
        let name = CString::new("x").unwrap();
        let mut out = FerricValue::void();
        let result = ferric_engine_get_global(ptr::null(), name.as_ptr(), &mut out);
        assert_eq!(result, FerricError::NullPointer);
    }
}

#[test]
fn get_global_null_name() {
    unsafe {
        let engine = ferric_engine_new();
        let mut out = FerricValue::void();
        let result = ferric_engine_get_global(engine, ptr::null(), &mut out);
        assert_eq!(result, FerricError::NullPointer);
        ferric_engine_free(engine);
    }
}

#[test]
fn get_global_null_out() {
    unsafe {
        let engine = ferric_engine_new();
        let name = CString::new("x").unwrap();
        let result = ferric_engine_get_global(engine, name.as_ptr(), ptr::null_mut());
        assert_eq!(result, FerricError::NullPointer);
        ferric_engine_free(engine);
    }
}

#[test]
fn get_global_value() {
    unsafe {
        let engine = ferric_engine_new();
        let source = CString::new("(defglobal ?*x* = 42)").unwrap();
        assert_eq!(
            ferric_engine_load_string(engine, source.as_ptr()),
            FerricError::Ok
        );
        ferric_engine_reset(engine);

        let name = CString::new("x").unwrap();
        let mut out = FerricValue::void();
        let result = ferric_engine_get_global(engine, name.as_ptr(), &mut out);
        assert_eq!(result, FerricError::Ok);
        assert_eq!(out.value_type, FerricValueType::Integer);
        assert_eq!(out.integer, 42);

        ferric_engine_free(engine);
    }
}

// ---------------------------------------------------------------------------
// Round-trip tests
// ---------------------------------------------------------------------------

#[test]
fn full_assert_query_retract_cycle() {
    unsafe {
        let engine = ferric_engine_new();
        ferric_engine_reset(engine);
        let source = CString::new("(assert (item widget 99))").unwrap();
        assert_eq!(
            ferric_engine_load_string(engine, source.as_ptr()),
            FerricError::Ok
        );

        // Verify fact is present
        let mut count: usize = 0;
        assert_eq!(
            ferric_engine_fact_count(engine, &mut count),
            FerricError::Ok
        );
        assert_eq!(count, 1);

        // Query fields
        let handle = &*engine;
        let fid = first_fact_id(handle);

        let mut field_count: usize = 0;
        assert_eq!(
            ferric_engine_get_fact_field_count(engine, fid, &mut field_count),
            FerricError::Ok
        );
        assert_eq!(field_count, 2);

        // Query field 0: symbol "widget"
        let mut f0 = FerricValue::void();
        assert_eq!(
            ferric_engine_get_fact_field(engine, fid, 0, &mut f0),
            FerricError::Ok
        );
        assert_eq!(f0.value_type, FerricValueType::Symbol);
        let s = CStr::from_ptr(f0.string_ptr).to_str().unwrap();
        assert_eq!(s, "widget");
        ferric_string_free(f0.string_ptr);

        // Query field 1: integer 99
        let mut f1 = FerricValue::void();
        assert_eq!(
            ferric_engine_get_fact_field(engine, fid, 1, &mut f1),
            FerricError::Ok
        );
        assert_eq!(f1.value_type, FerricValueType::Integer);
        assert_eq!(f1.integer, 99);

        // Retract the fact
        assert_eq!(ferric_engine_retract(engine, fid), FerricError::Ok);

        // Verify fact count is now 0
        let mut count2: usize = 999;
        assert_eq!(
            ferric_engine_fact_count(engine, &mut count2),
            FerricError::Ok
        );
        assert_eq!(count2, 0);

        // Verify get_fact_field now returns NotFound
        let mut out = FerricValue::void();
        let result = ferric_engine_get_fact_field(engine, fid, 0, &mut out);
        assert_eq!(result, FerricError::NotFound);

        ferric_engine_free(engine);
    }
}
