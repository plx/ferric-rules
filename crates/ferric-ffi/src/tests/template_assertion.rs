//! Tests for template fact assertion, slot-by-name access, and unchecked free.

use std::ffi::CString;
use std::ptr;

use crate::engine::{
    ferric_engine_assert_template, ferric_engine_free, ferric_engine_free_unchecked,
    ferric_engine_get_fact_field, ferric_engine_get_fact_field_count,
    ferric_engine_get_fact_slot_by_name, ferric_engine_get_fact_template_name,
    ferric_engine_get_fact_type, ferric_engine_load_string, ferric_engine_new, ferric_engine_reset,
};
use crate::error::FerricError;
use crate::types::{
    ferric_value_float, ferric_value_free, ferric_value_integer, ferric_value_string,
    ferric_value_symbol, FerricFactType, FerricValue, FerricValueType,
};

/// Helper: load a deftemplate into the engine and reset.
unsafe fn setup_person_template(engine: *mut crate::engine::FerricEngine) {
    let source = CString::new(
        "(deftemplate person
            (slot name (type STRING))
            (slot age (type INTEGER) (default 0))
            (slot active (type SYMBOL) (default TRUE)))",
    )
    .unwrap();
    let rc = ferric_engine_load_string(engine, source.as_ptr());
    assert_eq!(rc, FerricError::Ok, "failed to load deftemplate");
    let rc = ferric_engine_reset(engine);
    assert_eq!(rc, FerricError::Ok, "reset failed");
}

// ---------------------------------------------------------------------------
// ferric_engine_assert_template
// ---------------------------------------------------------------------------

#[test]
fn assert_template_basic() {
    unsafe {
        let engine = ferric_engine_new();
        setup_person_template(engine);

        let name = CString::new("person").unwrap();
        let slot_name_name = CString::new("name").unwrap();
        let slot_age_name = CString::new("age").unwrap();

        let slot_names = [slot_name_name.as_ptr(), slot_age_name.as_ptr()];
        let name_val = ferric_value_string(CString::new("Alice").unwrap().as_ptr());
        let age_val = ferric_value_integer(30);
        let slot_values = [name_val, age_val];

        let mut fact_id: u64 = 0;
        let rc = ferric_engine_assert_template(
            engine,
            name.as_ptr(),
            slot_names.as_ptr(),
            slot_values.as_ptr(),
            2,
            &mut fact_id,
        );
        assert_eq!(rc, FerricError::Ok);
        assert_ne!(fact_id, 0);

        // Verify it's a template fact.
        let mut fact_type = FerricFactType::Ordered;
        let rc = ferric_engine_get_fact_type(engine, fact_id, &mut fact_type);
        assert_eq!(rc, FerricError::Ok);
        assert_eq!(fact_type, FerricFactType::Template);

        // Verify template name.
        let mut buf = vec![0u8; 64];
        let mut out_len: usize = 0;
        let rc = ferric_engine_get_fact_template_name(
            engine,
            fact_id,
            buf.as_mut_ptr().cast(),
            buf.len(),
            &mut out_len,
        );
        assert_eq!(rc, FerricError::Ok);
        let tmpl_name = std::str::from_utf8(&buf[..out_len - 1]).unwrap();
        assert_eq!(tmpl_name, "person");

        // Verify field count (3 slots: name, age, active).
        let mut count: usize = 0;
        let rc = ferric_engine_get_fact_field_count(engine, fact_id, &mut count);
        assert_eq!(rc, FerricError::Ok);
        assert_eq!(count, 3);

        ferric_engine_free(engine);
    }
}

#[test]
fn assert_template_defaults_filled() {
    // When we only specify "name", "age" and "active" should get their defaults.
    unsafe {
        let engine = ferric_engine_new();
        setup_person_template(engine);

        let tmpl_name = CString::new("person").unwrap();
        let slot_name_str = CString::new("name").unwrap();
        let slot_names = [slot_name_str.as_ptr()];
        let name_val = ferric_value_string(CString::new("Bob").unwrap().as_ptr());
        let slot_values = [name_val];

        let mut fact_id: u64 = 0;
        let rc = ferric_engine_assert_template(
            engine,
            tmpl_name.as_ptr(),
            slot_names.as_ptr(),
            slot_values.as_ptr(),
            1,
            &mut fact_id,
        );
        assert_eq!(rc, FerricError::Ok);

        // age default is 0
        let mut age_val = FerricValue::void();
        let rc = ferric_engine_get_fact_field(engine, fact_id, 1, &mut age_val);
        assert_eq!(rc, FerricError::Ok);
        assert_eq!(age_val.value_type, FerricValueType::Integer);
        assert_eq!(age_val.integer, 0);

        // active default is TRUE (a symbol)
        let mut active_val = FerricValue::void();
        let rc = ferric_engine_get_fact_field(engine, fact_id, 2, &mut active_val);
        assert_eq!(rc, FerricError::Ok);
        assert_eq!(active_val.value_type, FerricValueType::Symbol);
        let active_str = std::ffi::CStr::from_ptr(active_val.string_ptr)
            .to_str()
            .unwrap();
        assert_eq!(active_str, "TRUE");
        ferric_value_free(&mut active_val);

        ferric_engine_free(engine);
    }
}

#[test]
fn assert_template_unknown_template() {
    unsafe {
        let engine = ferric_engine_new();
        setup_person_template(engine);

        let tmpl_name = CString::new("nonexistent").unwrap();
        let rc = ferric_engine_assert_template(
            engine,
            tmpl_name.as_ptr(),
            ptr::null(),
            ptr::null(),
            0,
            ptr::null_mut(),
        );
        assert_eq!(rc, FerricError::NotFound);

        ferric_engine_free(engine);
    }
}

#[test]
fn assert_template_unknown_slot() {
    unsafe {
        let engine = ferric_engine_new();
        setup_person_template(engine);

        let tmpl_name = CString::new("person").unwrap();
        let bad_slot = CString::new("nonexistent_slot").unwrap();
        let slot_names = [bad_slot.as_ptr()];
        let val = ferric_value_integer(42);
        let slot_values = [val];

        let rc = ferric_engine_assert_template(
            engine,
            tmpl_name.as_ptr(),
            slot_names.as_ptr(),
            slot_values.as_ptr(),
            1,
            ptr::null_mut(),
        );
        assert_eq!(rc, FerricError::NotFound);

        ferric_engine_free(engine);
    }
}

#[test]
fn assert_template_zero_slots() {
    // Assert with no slot overrides — all defaults.
    unsafe {
        let engine = ferric_engine_new();
        setup_person_template(engine);

        let tmpl_name = CString::new("person").unwrap();
        let mut fact_id: u64 = 0;
        let rc = ferric_engine_assert_template(
            engine,
            tmpl_name.as_ptr(),
            ptr::null(),
            ptr::null(),
            0,
            &mut fact_id,
        );
        assert_eq!(rc, FerricError::Ok);
        assert_ne!(fact_id, 0);

        ferric_engine_free(engine);
    }
}

#[test]
fn assert_template_null_out_id() {
    unsafe {
        let engine = ferric_engine_new();
        setup_person_template(engine);

        let tmpl_name = CString::new("person").unwrap();
        let rc = ferric_engine_assert_template(
            engine,
            tmpl_name.as_ptr(),
            ptr::null(),
            ptr::null(),
            0,
            ptr::null_mut(), // null out_fact_id
        );
        assert_eq!(rc, FerricError::Ok);

        ferric_engine_free(engine);
    }
}

// ---------------------------------------------------------------------------
// ferric_engine_get_fact_slot_by_name
// ---------------------------------------------------------------------------

#[test]
fn get_slot_by_name_basic() {
    unsafe {
        let engine = ferric_engine_new();
        setup_person_template(engine);

        // Assert a template fact with known values.
        let tmpl_name = CString::new("person").unwrap();
        let slot_name = CString::new("name").unwrap();
        let slot_age = CString::new("age").unwrap();
        let slot_names = [slot_name.as_ptr(), slot_age.as_ptr()];
        let name_val = ferric_value_string(CString::new("Charlie").unwrap().as_ptr());
        let age_val = ferric_value_integer(25);
        let slot_values = [name_val, age_val];

        let mut fact_id: u64 = 0;
        let rc = ferric_engine_assert_template(
            engine,
            tmpl_name.as_ptr(),
            slot_names.as_ptr(),
            slot_values.as_ptr(),
            2,
            &mut fact_id,
        );
        assert_eq!(rc, FerricError::Ok);

        // Query "name" slot by name.
        let query_name = CString::new("name").unwrap();
        let mut out_val = FerricValue::void();
        let rc =
            ferric_engine_get_fact_slot_by_name(engine, fact_id, query_name.as_ptr(), &mut out_val);
        assert_eq!(rc, FerricError::Ok);
        assert_eq!(out_val.value_type, FerricValueType::String);
        let name_str = std::ffi::CStr::from_ptr(out_val.string_ptr)
            .to_str()
            .unwrap();
        assert_eq!(name_str, "Charlie");
        ferric_value_free(&mut out_val);

        // Query "age" slot by name.
        let query_age = CString::new("age").unwrap();
        let mut out_val = FerricValue::void();
        let rc =
            ferric_engine_get_fact_slot_by_name(engine, fact_id, query_age.as_ptr(), &mut out_val);
        assert_eq!(rc, FerricError::Ok);
        assert_eq!(out_val.value_type, FerricValueType::Integer);
        assert_eq!(out_val.integer, 25);

        ferric_engine_free(engine);
    }
}

#[test]
fn get_slot_by_name_nonexistent_slot() {
    unsafe {
        let engine = ferric_engine_new();
        setup_person_template(engine);

        let tmpl_name = CString::new("person").unwrap();
        let mut fact_id: u64 = 0;
        let rc = ferric_engine_assert_template(
            engine,
            tmpl_name.as_ptr(),
            ptr::null(),
            ptr::null(),
            0,
            &mut fact_id,
        );
        assert_eq!(rc, FerricError::Ok);

        let bad_slot = CString::new("nonexistent").unwrap();
        let mut out_val = FerricValue::void();
        let rc =
            ferric_engine_get_fact_slot_by_name(engine, fact_id, bad_slot.as_ptr(), &mut out_val);
        assert_eq!(rc, FerricError::NotFound);

        ferric_engine_free(engine);
    }
}

#[test]
fn get_slot_by_name_ordered_fact_returns_error() {
    unsafe {
        let engine = ferric_engine_new();
        ferric_engine_reset(engine);

        // Assert an ordered fact.
        let source = CString::new("(assert (color red))").unwrap();
        let rc = ferric_engine_load_string(engine, source.as_ptr());
        assert_eq!(rc, FerricError::Ok);

        // Get fact IDs.
        let mut count: usize = 0;
        let rc = crate::engine::ferric_engine_fact_ids(engine, ptr::null_mut(), 0, &mut count);
        assert_eq!(rc, FerricError::Ok);
        assert!(count > 0);

        let mut ids = vec![0u64; count];
        let rc = crate::engine::ferric_engine_fact_ids(engine, ids.as_mut_ptr(), count, &mut count);
        assert_eq!(rc, FerricError::Ok);

        // Try to get a slot by name on an ordered fact — should fail.
        let slot_name = CString::new("some_slot").unwrap();
        let mut out_val = FerricValue::void();
        let rc =
            ferric_engine_get_fact_slot_by_name(engine, ids[0], slot_name.as_ptr(), &mut out_val);
        assert_eq!(rc, FerricError::InvalidArgument);

        ferric_engine_free(engine);
    }
}

#[test]
fn get_slot_by_name_nonexistent_fact() {
    unsafe {
        let engine = ferric_engine_new();
        let slot_name = CString::new("name").unwrap();
        let mut out_val = FerricValue::void();
        // Use a bogus fact_id.
        let rc =
            ferric_engine_get_fact_slot_by_name(engine, 999_999, slot_name.as_ptr(), &mut out_val);
        assert_eq!(rc, FerricError::NotFound);

        ferric_engine_free(engine);
    }
}

#[test]
fn get_slot_by_name_null_out_value() {
    unsafe {
        let engine = ferric_engine_new();
        let slot_name = CString::new("name").unwrap();
        let rc =
            ferric_engine_get_fact_slot_by_name(engine, 1, slot_name.as_ptr(), ptr::null_mut());
        assert_eq!(rc, FerricError::NullPointer);

        ferric_engine_free(engine);
    }
}

// ---------------------------------------------------------------------------
// ferric_engine_free_unchecked
// ---------------------------------------------------------------------------

#[test]
fn free_unchecked_null_is_safe() {
    unsafe {
        let rc = ferric_engine_free_unchecked(ptr::null_mut());
        assert_eq!(rc, FerricError::Ok);
    }
}

#[test]
fn free_unchecked_works_on_creating_thread() {
    unsafe {
        let engine = ferric_engine_new();
        assert!(!engine.is_null());
        let rc = ferric_engine_free_unchecked(engine);
        assert_eq!(rc, FerricError::Ok);
    }
}

#[test]
fn free_unchecked_works_from_different_thread() {
    // This is the key test: free_unchecked should NOT return ThreadViolation.
    unsafe {
        let engine = ferric_engine_new();
        assert!(!engine.is_null());

        let handle = engine as usize; // smuggle pointer across threads
        let result = std::thread::spawn(move || {
            let engine_ptr = handle as *mut crate::engine::FerricEngine;
            ferric_engine_free_unchecked(engine_ptr)
        })
        .join()
        .unwrap();

        assert_eq!(result, FerricError::Ok);
    }
}

// ---------------------------------------------------------------------------
// Assert template with various value types
// ---------------------------------------------------------------------------

#[test]
fn assert_template_with_float_slot() {
    unsafe {
        let engine = ferric_engine_new();
        let source = CString::new(
            "(deftemplate measurement
                (slot value (type FLOAT))
                (slot unit (type SYMBOL)))",
        )
        .unwrap();
        let rc = ferric_engine_load_string(engine, source.as_ptr());
        assert_eq!(rc, FerricError::Ok);
        ferric_engine_reset(engine);

        let tmpl_name = CString::new("measurement").unwrap();
        let slot_value = CString::new("value").unwrap();
        let slot_unit = CString::new("unit").unwrap();
        let slot_names = [slot_value.as_ptr(), slot_unit.as_ptr()];
        let val = ferric_value_float(98.6);
        let unit = ferric_value_symbol(CString::new("celsius").unwrap().as_ptr());
        let slot_values = [val, unit];

        let mut fact_id: u64 = 0;
        let rc = ferric_engine_assert_template(
            engine,
            tmpl_name.as_ptr(),
            slot_names.as_ptr(),
            slot_values.as_ptr(),
            2,
            &mut fact_id,
        );
        assert_eq!(rc, FerricError::Ok);

        // Verify via slot-by-name.
        let query = CString::new("value").unwrap();
        let mut out_val = FerricValue::void();
        let rc = ferric_engine_get_fact_slot_by_name(engine, fact_id, query.as_ptr(), &mut out_val);
        assert_eq!(rc, FerricError::Ok);
        assert_eq!(out_val.value_type, FerricValueType::Float);
        assert!((out_val.float - 98.6).abs() < f64::EPSILON);

        let query_unit = CString::new("unit").unwrap();
        let mut out_val = FerricValue::void();
        let rc =
            ferric_engine_get_fact_slot_by_name(engine, fact_id, query_unit.as_ptr(), &mut out_val);
        assert_eq!(rc, FerricError::Ok);
        assert_eq!(out_val.value_type, FerricValueType::Symbol);
        let unit_str = std::ffi::CStr::from_ptr(out_val.string_ptr)
            .to_str()
            .unwrap();
        assert_eq!(unit_str, "celsius");
        ferric_value_free(&mut out_val);

        ferric_engine_free(engine);
    }
}
