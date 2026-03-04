//! Tests for the FFI expansion functions added in the ferric-ffi crate.
//!
//! Covers the following API surface areas:
//! - Fact iteration: `ferric_engine_fact_ids`, `ferric_engine_find_fact_ids`
//! - Fact type and names: `ferric_engine_get_fact_type`, `ferric_engine_get_fact_relation`,
//!   `ferric_engine_get_fact_template_name`
//! - Structured assertion: `ferric_engine_assert_ordered`
//! - Value construction helpers: `ferric_value_integer`, `ferric_value_float`,
//!   `ferric_value_symbol`, `ferric_value_string`, `ferric_value_void`
//! - Template introspection: `ferric_engine_template_count`, `ferric_engine_template_name`,
//!   `ferric_engine_template_slot_count`, `ferric_engine_template_slot_name`
//! - Rule introspection: `ferric_engine_rule_count`, `ferric_engine_rule_info`
//! - Module operations: `ferric_engine_current_module`, `ferric_engine_get_focus`,
//!   `ferric_engine_focus_stack_depth`, `ferric_engine_focus_stack_entry`,
//!   `ferric_engine_module_count`, `ferric_engine_module_name`
//! - Agenda, halt, input, clear: `ferric_engine_agenda_count`, `ferric_engine_is_halted`,
//!   `ferric_engine_halt`, `ferric_engine_push_input`, `ferric_engine_clear`
//! - Convenience variants: `ferric_engine_new_with_source`,
//!   `ferric_engine_new_with_source_config`, `ferric_engine_clear_output`,
//!   `ferric_engine_run_ex`

use std::ffi::{CStr, CString};
use std::ptr;

use crate::engine::{
    ferric_engine_agenda_count, ferric_engine_assert_ordered, ferric_engine_clear,
    ferric_engine_clear_output, ferric_engine_fact_ids, ferric_engine_find_fact_ids,
    ferric_engine_focus_stack_depth, ferric_engine_focus_stack_entry, ferric_engine_free,
    ferric_engine_get_fact_relation, ferric_engine_get_fact_template_name,
    ferric_engine_get_fact_type, ferric_engine_get_focus, ferric_engine_halt,
    ferric_engine_is_halted, ferric_engine_load_string, ferric_engine_module_count,
    ferric_engine_module_name, ferric_engine_new, ferric_engine_new_with_source,
    ferric_engine_new_with_source_config, ferric_engine_push_input, ferric_engine_reset,
    ferric_engine_rule_count, ferric_engine_rule_info, ferric_engine_run,
    ferric_engine_run_ex, ferric_engine_template_count, ferric_engine_template_name,
    ferric_engine_template_slot_count, ferric_engine_template_slot_name,
    ferric_engine_current_module,
};
use crate::error::FerricError;
use crate::types::{
    ferric_string_free, ferric_value_float, ferric_value_integer, ferric_value_string,
    ferric_value_symbol, ferric_value_void, FerricFactType, FerricHaltReason, FerricValue,
    FerricValueType,
};

// ---------------------------------------------------------------------------
// Section 1: Fact Iteration Tests
// ---------------------------------------------------------------------------

#[test]
fn fact_ids_empty_engine() {
    // A freshly-reset engine has no user-visible facts (initial-fact excluded).
    unsafe {
        let engine = ferric_engine_new();
        ferric_engine_reset(engine);

        let mut count: usize = 999;
        let result = ferric_engine_fact_ids(engine, ptr::null_mut(), 0, &mut count);

        assert_eq!(result, FerricError::Ok);
        assert_eq!(count, 0, "a fresh engine should have 0 user-visible facts");

        ferric_engine_free(engine);
    }
}

#[test]
fn fact_ids_after_assert() {
    // Asserting a fact makes it visible through the fact_ids enumeration.
    unsafe {
        let engine = ferric_engine_new();
        ferric_engine_reset(engine);

        let source = CString::new("(assert (color red))").unwrap();
        assert_eq!(
            ferric_engine_load_string(engine, source.as_ptr()),
            FerricError::Ok
        );

        let mut count: usize = 0;
        let mut ids = vec![0u64; 8];
        let result = ferric_engine_fact_ids(engine, ids.as_mut_ptr(), ids.len(), &mut count);

        assert_eq!(result, FerricError::Ok);
        assert_eq!(count, 1, "one asserted fact should appear in fact_ids");
        assert_ne!(ids[0], 0, "the returned fact ID should be non-zero");

        ferric_engine_free(engine);
    }
}

#[test]
fn fact_ids_size_query() {
    // Passing null out_ids with max_ids=0 performs a size query: total count is written
    // to out_count and Ok is returned without any copy.
    unsafe {
        let engine = ferric_engine_new();
        ferric_engine_reset(engine);

        let s1 = CString::new("(assert (a))").unwrap();
        let s2 = CString::new("(assert (b))").unwrap();
        assert_eq!(
            ferric_engine_load_string(engine, s1.as_ptr()),
            FerricError::Ok
        );
        assert_eq!(
            ferric_engine_load_string(engine, s2.as_ptr()),
            FerricError::Ok
        );

        // Size query: out_ids == null AND max_ids == 0
        let mut count: usize = 999;
        let result = ferric_engine_fact_ids(engine, ptr::null_mut(), 0, &mut count);

        assert_eq!(result, FerricError::Ok);
        assert_eq!(count, 2, "size query should report 2 facts without copying");

        ferric_engine_free(engine);
    }
}

#[test]
fn fact_ids_null_engine() {
    // A null engine pointer must return NullPointer immediately.
    unsafe {
        let mut count: usize = 0;
        let result = ferric_engine_fact_ids(ptr::null(), ptr::null_mut(), 0, &mut count);
        assert_eq!(result, FerricError::NullPointer);
    }
}

#[test]
fn fact_ids_null_out_count() {
    // A null out_count pointer must return NullPointer.
    unsafe {
        let engine = ferric_engine_new();
        ferric_engine_reset(engine);

        let result = ferric_engine_fact_ids(engine, ptr::null_mut(), 0, ptr::null_mut());
        assert_eq!(result, FerricError::NullPointer);

        ferric_engine_free(engine);
    }
}

#[test]
fn find_fact_ids_by_relation() {
    // Facts can be looked up by their relation name.
    // Asserting two "(color ...)" facts means find_fact_ids("color") returns exactly 2.
    unsafe {
        let engine = ferric_engine_new();
        ferric_engine_reset(engine);

        let s1 = CString::new("(assert (color red))").unwrap();
        let s2 = CString::new("(assert (color blue))").unwrap();
        let s3 = CString::new("(assert (shape circle))").unwrap();
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

        let relation = CString::new("color").unwrap();
        let mut count: usize = 0;
        let mut ids = vec![0u64; 8];
        let result = ferric_engine_find_fact_ids(
            engine,
            relation.as_ptr(),
            ids.as_mut_ptr(),
            ids.len(),
            &mut count,
        );

        assert_eq!(result, FerricError::Ok);
        assert_eq!(count, 2, "only the two 'color' facts should be found");

        ferric_engine_free(engine);
    }
}

#[test]
fn find_fact_ids_unknown_relation() {
    // Searching for a relation that has no matching facts returns Ok with count = 0.
    unsafe {
        let engine = ferric_engine_new();
        ferric_engine_reset(engine);

        let relation = CString::new("nonexistent-relation").unwrap();
        let mut count: usize = 999;
        let result = ferric_engine_find_fact_ids(
            engine,
            relation.as_ptr(),
            ptr::null_mut(),
            0,
            &mut count,
        );

        assert_eq!(result, FerricError::Ok);
        assert_eq!(count, 0, "unknown relation should yield 0 matching facts");

        ferric_engine_free(engine);
    }
}

#[test]
fn find_fact_ids_null_relation() {
    // A null relation pointer must return NullPointer.
    unsafe {
        let engine = ferric_engine_new();
        ferric_engine_reset(engine);

        let mut count: usize = 0;
        let result = ferric_engine_find_fact_ids(
            engine,
            ptr::null(),
            ptr::null_mut(),
            0,
            &mut count,
        );
        assert_eq!(result, FerricError::NullPointer);

        ferric_engine_free(engine);
    }
}

// ---------------------------------------------------------------------------
// Section 2: Fact Type & Names Tests
// ---------------------------------------------------------------------------

#[test]
fn get_fact_type_ordered() {
    // An asserted ordered fact (e.g., "(color red)") reports type Ordered.
    unsafe {
        let engine = ferric_engine_new();
        ferric_engine_reset(engine);

        let source = CString::new("(assert (color red))").unwrap();
        let mut fact_id: u64 = 0;
        assert_eq!(
            crate::engine::ferric_engine_assert_string(engine, source.as_ptr(), &mut fact_id),
            FerricError::Ok
        );

        let mut fact_type = FerricFactType::Template; // initialise to wrong value
        let result = ferric_engine_get_fact_type(engine, fact_id, &mut fact_type);

        assert_eq!(result, FerricError::Ok);
        assert_eq!(
            fact_type,
            FerricFactType::Ordered,
            "a plain assert should produce an Ordered fact"
        );

        ferric_engine_free(engine);
    }
}

#[test]
fn get_fact_type_template() {
    // A fact asserted against a deftemplate is reported as type Template.
    // We use a deffacts construct so the template fact is automatically
    // re-asserted when the engine is reset.
    unsafe {
        let engine = ferric_engine_new();
        let source = CString::new(
            "(deftemplate person (slot name) (slot age))\
             (deffacts initial-people (person (name Alice) (age 30)))",
        )
        .unwrap();
        assert_eq!(
            ferric_engine_load_string(engine, source.as_ptr()),
            FerricError::Ok
        );
        // reset() triggers deffacts to assert the template fact
        ferric_engine_reset(engine);

        // Obtain the fact ID of the template fact via fact_ids
        let mut count: usize = 0;
        let mut ids = vec![0u64; 8];
        assert_eq!(
            ferric_engine_fact_ids(engine, ids.as_mut_ptr(), ids.len(), &mut count),
            FerricError::Ok
        );
        assert_eq!(count, 1, "one deffacts fact should exist after reset");
        let fact_id = ids[0];

        let mut fact_type = FerricFactType::Ordered;
        let result = ferric_engine_get_fact_type(engine, fact_id, &mut fact_type);

        assert_eq!(result, FerricError::Ok);
        assert_eq!(
            fact_type,
            FerricFactType::Template,
            "a deftemplate-backed fact should be typed as Template"
        );

        ferric_engine_free(engine);
    }
}

#[test]
fn get_fact_type_not_found() {
    // An invalid fact ID returns NotFound.
    unsafe {
        let engine = ferric_engine_new();
        ferric_engine_reset(engine);

        let mut fact_type = FerricFactType::Ordered;
        let result = ferric_engine_get_fact_type(engine, 0xDEAD_BEEF_u64, &mut fact_type);

        assert_eq!(result, FerricError::NotFound);

        ferric_engine_free(engine);
    }
}

#[test]
fn get_fact_type_null_out_type() {
    // Passing null for out_type returns NullPointer.
    unsafe {
        let engine = ferric_engine_new();
        ferric_engine_reset(engine);

        let result = ferric_engine_get_fact_type(engine, 1, ptr::null_mut());
        assert_eq!(result, FerricError::NullPointer);

        ferric_engine_free(engine);
    }
}

#[test]
fn get_fact_relation_ordered() {
    // The relation name of an ordered fact can be read back into a buffer.
    unsafe {
        let engine = ferric_engine_new();
        ferric_engine_reset(engine);

        let source = CString::new("(assert (color red))").unwrap();
        let mut fact_id: u64 = 0;
        assert_eq!(
            crate::engine::ferric_engine_assert_string(engine, source.as_ptr(), &mut fact_id),
            FerricError::Ok
        );

        let mut buf = vec![0i8; 64];
        let mut out_len: usize = 0;
        let result =
            ferric_engine_get_fact_relation(engine, fact_id, buf.as_mut_ptr(), buf.len(), &mut out_len);

        assert_eq!(result, FerricError::Ok);
        let name = CStr::from_ptr(buf.as_ptr()).to_str().unwrap();
        assert_eq!(name, "color", "relation name must match the asserted fact head");

        ferric_engine_free(engine);
    }
}

#[test]
fn get_fact_relation_template_is_error() {
    // Calling get_fact_relation on a template fact is an invalid argument —
    // template facts have a template name, not a relation.
    // We use deffacts so the template fact is present after reset.
    unsafe {
        let engine = ferric_engine_new();
        let source = CString::new(
            "(deftemplate widget (slot id))\
             (deffacts initial-widgets (widget (id 1)))",
        )
        .unwrap();
        assert_eq!(
            ferric_engine_load_string(engine, source.as_ptr()),
            FerricError::Ok
        );
        ferric_engine_reset(engine);

        // Obtain the template fact ID
        let mut count: usize = 0;
        let mut ids = vec![0u64; 8];
        assert_eq!(
            ferric_engine_fact_ids(engine, ids.as_mut_ptr(), ids.len(), &mut count),
            FerricError::Ok
        );
        assert_eq!(count, 1);
        let fact_id = ids[0];

        let mut buf = vec![0i8; 64];
        let mut out_len: usize = 0;
        let result =
            ferric_engine_get_fact_relation(engine, fact_id, buf.as_mut_ptr(), buf.len(), &mut out_len);

        assert_eq!(
            result,
            FerricError::InvalidArgument,
            "get_fact_relation on a template fact must return InvalidArgument"
        );

        ferric_engine_free(engine);
    }
}

#[test]
fn get_fact_relation_buffer_pattern() {
    // The standard buffer-copy pattern: size query, exact fit, undersized buffer.
    unsafe {
        let engine = ferric_engine_new();
        ferric_engine_reset(engine);

        let source = CString::new("(assert (hello-world))").unwrap();
        let mut fact_id: u64 = 0;
        assert_eq!(
            crate::engine::ferric_engine_assert_string(engine, source.as_ptr(), &mut fact_id),
            FerricError::Ok
        );

        // "hello-world" is 11 bytes + NUL = 12
        let expected_len = "hello-world".len() + 1;

        // Size query: null buf, buf_len = 0
        let mut out_len: usize = 0;
        let result = ferric_engine_get_fact_relation(engine, fact_id, ptr::null_mut(), 0, &mut out_len);
        assert_eq!(result, FerricError::Ok);
        assert_eq!(out_len, expected_len, "size query must report exact byte count");

        // Exact-fit copy
        let mut exact_buf = vec![0i8; expected_len];
        let result = ferric_engine_get_fact_relation(
            engine,
            fact_id,
            exact_buf.as_mut_ptr(),
            exact_buf.len(),
            &mut out_len,
        );
        assert_eq!(result, FerricError::Ok);
        let name = CStr::from_ptr(exact_buf.as_ptr()).to_str().unwrap();
        assert_eq!(name, "hello-world");

        // Undersized buffer: only room for "hello\0" (6 bytes)
        let mut tiny_buf = vec![0i8; 6];
        let result = ferric_engine_get_fact_relation(
            engine,
            fact_id,
            tiny_buf.as_mut_ptr(),
            tiny_buf.len(),
            &mut out_len,
        );
        assert_eq!(result, FerricError::BufferTooSmall);
        assert_eq!(out_len, expected_len, "out_len still reports full needed size");
        // The truncated buffer is NUL-terminated
        let truncated = CStr::from_ptr(tiny_buf.as_ptr()).to_str().unwrap();
        assert_eq!(truncated.len(), 5, "truncated to 5 chars + NUL in a 6-byte buffer");

        ferric_engine_free(engine);
    }
}

#[test]
fn get_fact_template_name() {
    // The template name for a template fact can be read back correctly.
    // We use deffacts so the template fact is present after reset.
    unsafe {
        let engine = ferric_engine_new();
        let source = CString::new(
            "(deftemplate person (slot name))\
             (deffacts initial-people (person (name Alice)))",
        )
        .unwrap();
        assert_eq!(
            ferric_engine_load_string(engine, source.as_ptr()),
            FerricError::Ok
        );
        ferric_engine_reset(engine);

        // Obtain the template fact ID
        let mut count: usize = 0;
        let mut ids = vec![0u64; 8];
        assert_eq!(
            ferric_engine_fact_ids(engine, ids.as_mut_ptr(), ids.len(), &mut count),
            FerricError::Ok
        );
        assert_eq!(count, 1);
        let fact_id = ids[0];

        let mut buf = vec![0i8; 64];
        let mut out_len: usize = 0;
        let result = ferric_engine_get_fact_template_name(
            engine,
            fact_id,
            buf.as_mut_ptr(),
            buf.len(),
            &mut out_len,
        );

        assert_eq!(result, FerricError::Ok);
        let name = CStr::from_ptr(buf.as_ptr()).to_str().unwrap();
        assert_eq!(name, "person");

        ferric_engine_free(engine);
    }
}

#[test]
fn get_fact_template_name_ordered_is_error() {
    // Calling get_fact_template_name on an ordered fact is an invalid argument.
    unsafe {
        let engine = ferric_engine_new();
        ferric_engine_reset(engine);

        let source = CString::new("(assert (color red))").unwrap();
        let mut fact_id: u64 = 0;
        assert_eq!(
            crate::engine::ferric_engine_assert_string(engine, source.as_ptr(), &mut fact_id),
            FerricError::Ok
        );

        let mut buf = vec![0i8; 64];
        let mut out_len: usize = 0;
        let result = ferric_engine_get_fact_template_name(
            engine,
            fact_id,
            buf.as_mut_ptr(),
            buf.len(),
            &mut out_len,
        );

        assert_eq!(
            result,
            FerricError::InvalidArgument,
            "get_fact_template_name on an ordered fact must return InvalidArgument"
        );

        ferric_engine_free(engine);
    }
}

// ---------------------------------------------------------------------------
// Section 3: Structured Assertion Tests
// ---------------------------------------------------------------------------

#[test]
fn assert_ordered_simple() {
    // A fact can be asserted from structured FerricValue fields rather than
    // CLIPS source text, and the resulting fact is readable.
    unsafe {
        let engine = ferric_engine_new();
        ferric_engine_reset(engine);

        let relation = CString::new("color").unwrap();
        let sym_name = CString::new("red").unwrap();
        let field = ferric_value_symbol(sym_name.as_ptr());

        let mut out_fact_id: u64 = 0;
        let result = ferric_engine_assert_ordered(
            engine,
            relation.as_ptr(),
            &field,
            1,
            &mut out_fact_id,
        );

        assert_eq!(result, FerricError::Ok, "structured assert must succeed");
        assert_ne!(out_fact_id, 0, "a valid fact ID must be returned");

        // Free the owned string inside the symbol field
        ferric_string_free(field.string_ptr);

        ferric_engine_free(engine);
    }
}

#[test]
fn assert_ordered_integer_fields() {
    // Integer-typed fields round-trip correctly through the structured assert API.
    unsafe {
        let engine = ferric_engine_new();
        ferric_engine_reset(engine);

        let relation = CString::new("score").unwrap();
        let fields = [ferric_value_integer(42), ferric_value_integer(100)];
        let mut out_fact_id: u64 = 0;
        let result = ferric_engine_assert_ordered(
            engine,
            relation.as_ptr(),
            fields.as_ptr(),
            fields.len(),
            &mut out_fact_id,
        );

        assert_eq!(result, FerricError::Ok);
        assert_ne!(out_fact_id, 0);

        // Read back the first field and verify it is the integer 42
        let mut field_out = FerricValue::void();
        let get_result =
            crate::engine::ferric_engine_get_fact_field(engine, out_fact_id, 0, &mut field_out);
        assert_eq!(get_result, FerricError::Ok);
        assert_eq!(field_out.value_type, FerricValueType::Integer);
        assert_eq!(field_out.integer, 42);

        ferric_engine_free(engine);
    }
}

#[test]
fn assert_ordered_null_relation() {
    // A null relation pointer must return NullPointer.
    unsafe {
        let engine = ferric_engine_new();
        ferric_engine_reset(engine);

        let mut out_fact_id: u64 = 0;
        let result = ferric_engine_assert_ordered(
            engine,
            ptr::null(),
            ptr::null(),
            0,
            &mut out_fact_id,
        );

        assert_eq!(result, FerricError::NullPointer);

        ferric_engine_free(engine);
    }
}

#[test]
fn assert_ordered_null_fields_nonzero_count() {
    // A null fields pointer combined with a non-zero field_count must return NullPointer,
    // because the caller promised fields that weren't provided.
    unsafe {
        let engine = ferric_engine_new();
        ferric_engine_reset(engine);

        let relation = CString::new("x").unwrap();
        let mut out_fact_id: u64 = 0;
        let result = ferric_engine_assert_ordered(
            engine,
            relation.as_ptr(),
            ptr::null(),
            3, // claimed 3 fields but provided null
            &mut out_fact_id,
        );

        assert_eq!(result, FerricError::NullPointer);

        ferric_engine_free(engine);
    }
}

#[test]
fn assert_ordered_zero_fields() {
    // Asserting with null fields and field_count=0 is valid and produces a zero-field fact.
    unsafe {
        let engine = ferric_engine_new();
        ferric_engine_reset(engine);

        let relation = CString::new("marker").unwrap();
        let mut out_fact_id: u64 = 0;
        let result = ferric_engine_assert_ordered(
            engine,
            relation.as_ptr(),
            ptr::null(), // null is acceptable when count == 0
            0,
            &mut out_fact_id,
        );

        assert_eq!(result, FerricError::Ok, "zero-field ordered assert must succeed");
        assert_ne!(out_fact_id, 0);

        ferric_engine_free(engine);
    }
}

#[test]
fn assert_ordered_null_out_fact_id() {
    // When out_fact_id is null, the fact is still asserted successfully;
    // the caller simply does not receive the ID.
    unsafe {
        let engine = ferric_engine_new();
        ferric_engine_reset(engine);

        let relation = CString::new("flag").unwrap();
        let result = ferric_engine_assert_ordered(
            engine,
            relation.as_ptr(),
            ptr::null(),
            0,
            ptr::null_mut(), // caller does not need the fact ID
        );

        assert_eq!(result, FerricError::Ok);

        // Verify the fact was still created
        let mut count: usize = 0;
        assert_eq!(
            crate::engine::ferric_engine_fact_count(engine, &mut count),
            FerricError::Ok
        );
        assert_eq!(count, 1, "fact must exist even when out_fact_id was null");

        ferric_engine_free(engine);
    }
}

// ---------------------------------------------------------------------------
// Section 4: Value Construction Tests
// ---------------------------------------------------------------------------

#[test]
fn value_integer_constructor() {
    // ferric_value_integer produces a value with the correct type and numeric content.
    let v = ferric_value_integer(42);
    assert_eq!(v.value_type, FerricValueType::Integer);
    assert_eq!(v.integer, 42);
    assert!(v.string_ptr.is_null());
    assert!(v.multifield_ptr.is_null());
}

#[test]
fn value_float_constructor() {
    // ferric_value_float produces a value with the correct type and float content.
    let v = ferric_value_float(42.5);
    assert_eq!(v.value_type, FerricValueType::Float);
    assert!((v.float - 42.5_f64).abs() < 1e-9);
    assert!(v.string_ptr.is_null());
}

#[test]
fn value_symbol_constructor() {
    // ferric_value_symbol copies the string onto the heap and marks the type as Symbol.
    unsafe {
        let name = CString::new("mySymbol").unwrap();
        let v = ferric_value_symbol(name.as_ptr());

        assert_eq!(v.value_type, FerricValueType::Symbol);
        assert!(!v.string_ptr.is_null(), "symbol must own a heap string");
        let s = CStr::from_ptr(v.string_ptr).to_str().unwrap();
        assert_eq!(s, "mySymbol");

        ferric_string_free(v.string_ptr);
    }
}

#[test]
fn value_string_constructor() {
    // ferric_value_string copies the string onto the heap and marks the type as String.
    unsafe {
        let raw = CString::new("hello, world").unwrap();
        let v = ferric_value_string(raw.as_ptr());

        assert_eq!(v.value_type, FerricValueType::String);
        assert!(!v.string_ptr.is_null(), "string value must own a heap string");
        let s = CStr::from_ptr(v.string_ptr).to_str().unwrap();
        assert_eq!(s, "hello, world");

        ferric_string_free(v.string_ptr);
    }
}

#[test]
fn value_void_constructor() {
    // ferric_value_void produces a fully zeroed value of type Void.
    let v = ferric_value_void();
    assert_eq!(v.value_type, FerricValueType::Void);
    assert_eq!(v.integer, 0);
    assert!(v.float.abs() < f64::EPSILON);
    assert!(v.string_ptr.is_null());
    assert!(v.multifield_ptr.is_null());
}

#[test]
fn value_symbol_null_returns_void() {
    // A null name pointer to ferric_value_symbol returns a Void value rather than crashing.
    unsafe {
        let v = ferric_value_symbol(ptr::null());
        assert_eq!(
            v.value_type,
            FerricValueType::Void,
            "null symbol name must produce a Void value"
        );
        assert!(v.string_ptr.is_null());
    }
}

#[test]
fn value_string_null_returns_void() {
    // A null string pointer to ferric_value_string returns a Void value rather than crashing.
    unsafe {
        let v = ferric_value_string(ptr::null());
        assert_eq!(
            v.value_type,
            FerricValueType::Void,
            "null string pointer must produce a Void value"
        );
        assert!(v.string_ptr.is_null());
    }
}

// ---------------------------------------------------------------------------
// Section 5: Template Introspection Tests
// ---------------------------------------------------------------------------

#[test]
fn template_count_empty() {
    // A newly-created engine has no user-defined templates.
    unsafe {
        let engine = ferric_engine_new();
        ferric_engine_reset(engine);

        let mut count: usize = 999;
        let result = ferric_engine_template_count(engine, &mut count);

        assert_eq!(result, FerricError::Ok);
        assert_eq!(count, 0, "a fresh engine has no templates");

        ferric_engine_free(engine);
    }
}

#[test]
fn template_count_after_define() {
    // Loading a deftemplate increments the template count by one.
    unsafe {
        let engine = ferric_engine_new();
        let source = CString::new("(deftemplate person (slot name) (slot age))").unwrap();
        assert_eq!(
            ferric_engine_load_string(engine, source.as_ptr()),
            FerricError::Ok
        );
        ferric_engine_reset(engine);

        let mut count: usize = 0;
        let result = ferric_engine_template_count(engine, &mut count);

        assert_eq!(result, FerricError::Ok);
        assert_eq!(count, 1, "one deftemplate should produce count = 1");

        ferric_engine_free(engine);
    }
}

#[test]
fn template_name_by_index() {
    // The template name at index 0 matches the name given in the deftemplate.
    unsafe {
        let engine = ferric_engine_new();
        let source = CString::new("(deftemplate person (slot name))").unwrap();
        assert_eq!(
            ferric_engine_load_string(engine, source.as_ptr()),
            FerricError::Ok
        );
        ferric_engine_reset(engine);

        let mut buf = vec![0i8; 64];
        let mut out_len: usize = 0;
        let result =
            ferric_engine_template_name(engine, 0, buf.as_mut_ptr(), buf.len(), &mut out_len);

        assert_eq!(result, FerricError::Ok);
        let name = CStr::from_ptr(buf.as_ptr()).to_str().unwrap();
        assert_eq!(name, "person");

        ferric_engine_free(engine);
    }
}

#[test]
fn template_name_out_of_bounds() {
    // Requesting a template at an index >= count returns InvalidArgument.
    unsafe {
        let engine = ferric_engine_new();
        ferric_engine_reset(engine); // no templates loaded

        let mut buf = vec![0i8; 64];
        let mut out_len: usize = 0;
        let result =
            ferric_engine_template_name(engine, 0, buf.as_mut_ptr(), buf.len(), &mut out_len);

        assert_eq!(
            result,
            FerricError::InvalidArgument,
            "index 0 is out of bounds when no templates exist"
        );

        ferric_engine_free(engine);
    }
}

#[test]
fn template_slot_count() {
    // A template with two slots reports slot_count = 2.
    unsafe {
        let engine = ferric_engine_new();
        let source =
            CString::new("(deftemplate person (slot name) (slot age))").unwrap();
        assert_eq!(
            ferric_engine_load_string(engine, source.as_ptr()),
            FerricError::Ok
        );
        ferric_engine_reset(engine);

        let template_name = CString::new("person").unwrap();
        let mut count: usize = 0;
        let result = ferric_engine_template_slot_count(engine, template_name.as_ptr(), &mut count);

        assert_eq!(result, FerricError::Ok);
        assert_eq!(count, 2, "the 'person' template has exactly 2 slots");

        ferric_engine_free(engine);
    }
}

#[test]
fn template_slot_name_by_index() {
    // Slot names can be enumerated by index in definition order.
    unsafe {
        let engine = ferric_engine_new();
        let source =
            CString::new("(deftemplate person (slot name) (slot age))").unwrap();
        assert_eq!(
            ferric_engine_load_string(engine, source.as_ptr()),
            FerricError::Ok
        );
        ferric_engine_reset(engine);

        let template_name = CString::new("person").unwrap();
        let mut buf = vec![0i8; 64];
        let mut out_len: usize = 0;

        // Slot 0 should be "name"
        let result = ferric_engine_template_slot_name(
            engine,
            template_name.as_ptr(),
            0,
            buf.as_mut_ptr(),
            buf.len(),
            &mut out_len,
        );
        assert_eq!(result, FerricError::Ok);
        let slot0 = CStr::from_ptr(buf.as_ptr()).to_str().unwrap();
        assert_eq!(slot0, "name");

        // Slot 1 should be "age"
        let result = ferric_engine_template_slot_name(
            engine,
            template_name.as_ptr(),
            1,
            buf.as_mut_ptr(),
            buf.len(),
            &mut out_len,
        );
        assert_eq!(result, FerricError::Ok);
        let slot1 = CStr::from_ptr(buf.as_ptr()).to_str().unwrap();
        assert_eq!(slot1, "age");

        ferric_engine_free(engine);
    }
}

#[test]
fn template_slot_count_not_found() {
    // Querying slot count for a template name that was never defined returns NotFound.
    unsafe {
        let engine = ferric_engine_new();
        ferric_engine_reset(engine);

        let template_name = CString::new("ghost-template").unwrap();
        let mut count: usize = 0;
        let result = ferric_engine_template_slot_count(engine, template_name.as_ptr(), &mut count);

        assert_eq!(
            result,
            FerricError::NotFound,
            "unknown template name must return NotFound"
        );

        ferric_engine_free(engine);
    }
}

// ---------------------------------------------------------------------------
// Section 6: Rule Introspection Tests
// ---------------------------------------------------------------------------

#[test]
fn rule_count_empty() {
    // A freshly-reset engine with no rules loaded reports rule_count = 0.
    unsafe {
        let engine = ferric_engine_new();
        ferric_engine_reset(engine);

        let mut count: usize = 999;
        let result = ferric_engine_rule_count(engine, &mut count);

        assert_eq!(result, FerricError::Ok);
        assert_eq!(count, 0, "no rules loaded means count must be 0");

        ferric_engine_free(engine);
    }
}

#[test]
fn rule_count_after_define() {
    // Loading a defrule increments the rule count by one.
    unsafe {
        let engine = ferric_engine_new();
        let source =
            CString::new("(defrule my-rule (initial-fact) => (printout t \"hi\" crlf))").unwrap();
        assert_eq!(
            ferric_engine_load_string(engine, source.as_ptr()),
            FerricError::Ok
        );
        ferric_engine_reset(engine);

        let mut count: usize = 0;
        let result = ferric_engine_rule_count(engine, &mut count);

        assert_eq!(result, FerricError::Ok);
        assert_eq!(count, 1);

        ferric_engine_free(engine);
    }
}

#[test]
fn rule_info_name_and_salience() {
    // The rule name and declared salience are correctly exposed through rule_info.
    unsafe {
        let engine = ferric_engine_new();
        // Salience 75 is deliberately non-default (default is 0) to verify the
        // value is actually read from the rule and not hardcoded.
        let source = CString::new(
            "(defrule priority-rule (declare (salience 75)) (initial-fact) => )",
        )
        .unwrap();
        assert_eq!(
            ferric_engine_load_string(engine, source.as_ptr()),
            FerricError::Ok
        );
        ferric_engine_reset(engine);

        let mut buf = vec![0i8; 64];
        let mut out_len: usize = 0;
        let mut salience: i32 = 0;
        let result = ferric_engine_rule_info(
            engine,
            0,
            buf.as_mut_ptr(),
            buf.len(),
            &mut out_len,
            &mut salience,
        );

        assert_eq!(result, FerricError::Ok);
        let name = CStr::from_ptr(buf.as_ptr()).to_str().unwrap();
        assert_eq!(name, "priority-rule");
        assert_eq!(salience, 75, "declared salience 75 must be reported");

        ferric_engine_free(engine);
    }
}

#[test]
fn rule_info_out_of_bounds() {
    // Requesting rule info at an index >= rule count returns InvalidArgument.
    unsafe {
        let engine = ferric_engine_new();
        ferric_engine_reset(engine); // no rules

        let mut buf = vec![0i8; 64];
        let mut out_len: usize = 0;
        let result = ferric_engine_rule_info(
            engine,
            0,
            buf.as_mut_ptr(),
            buf.len(),
            &mut out_len,
            ptr::null_mut(),
        );

        assert_eq!(
            result,
            FerricError::InvalidArgument,
            "index 0 must be out of bounds when no rules are loaded"
        );

        ferric_engine_free(engine);
    }
}

// ---------------------------------------------------------------------------
// Section 7: Module Operations Tests
// ---------------------------------------------------------------------------

#[test]
fn current_module_is_main() {
    // The default current module for a new engine is "MAIN".
    unsafe {
        let engine = ferric_engine_new();
        ferric_engine_reset(engine);

        let mut buf = vec![0i8; 64];
        let mut out_len: usize = 0;
        let result = ferric_engine_current_module(engine, buf.as_mut_ptr(), buf.len(), &mut out_len);

        assert_eq!(result, FerricError::Ok);
        let name = CStr::from_ptr(buf.as_ptr()).to_str().unwrap();
        assert_eq!(name, "MAIN", "the default module is always MAIN");

        ferric_engine_free(engine);
    }
}

#[test]
fn get_focus_is_main() {
    // After reset, the focus stack top is the MAIN module.
    unsafe {
        let engine = ferric_engine_new();
        ferric_engine_reset(engine);

        let mut buf = vec![0i8; 64];
        let mut out_len: usize = 0;
        let result = ferric_engine_get_focus(engine, buf.as_mut_ptr(), buf.len(), &mut out_len);

        assert_eq!(result, FerricError::Ok);
        let name = CStr::from_ptr(buf.as_ptr()).to_str().unwrap();
        assert_eq!(name, "MAIN", "default focus must be MAIN");

        ferric_engine_free(engine);
    }
}

#[test]
fn focus_stack_depth() {
    // After reset, the focus stack contains exactly one entry (MAIN).
    unsafe {
        let engine = ferric_engine_new();
        ferric_engine_reset(engine);

        let mut depth: usize = 0;
        let result = ferric_engine_focus_stack_depth(engine, &mut depth);

        assert_eq!(result, FerricError::Ok);
        assert_eq!(depth, 1, "the focus stack has exactly one entry by default");

        ferric_engine_free(engine);
    }
}

#[test]
fn focus_stack_entry() {
    // The focus stack entry at index 0 is "MAIN".
    unsafe {
        let engine = ferric_engine_new();
        ferric_engine_reset(engine);

        let mut buf = vec![0i8; 64];
        let mut out_len: usize = 0;
        let result =
            ferric_engine_focus_stack_entry(engine, 0, buf.as_mut_ptr(), buf.len(), &mut out_len);

        assert_eq!(result, FerricError::Ok);
        let name = CStr::from_ptr(buf.as_ptr()).to_str().unwrap();
        assert_eq!(name, "MAIN");

        ferric_engine_free(engine);
    }
}

#[test]
fn module_count_default() {
    // A new engine always has at least the MAIN module registered.
    unsafe {
        let engine = ferric_engine_new();
        ferric_engine_reset(engine);

        let mut count: usize = 0;
        let result = ferric_engine_module_count(engine, &mut count);

        assert_eq!(result, FerricError::Ok);
        assert!(count >= 1, "at least the MAIN module must always be present");

        ferric_engine_free(engine);
    }
}

#[test]
fn module_name_by_index() {
    // The module list includes "MAIN" at one of its indices.
    unsafe {
        let engine = ferric_engine_new();
        ferric_engine_reset(engine);

        let mut total: usize = 0;
        assert_eq!(
            ferric_engine_module_count(engine, &mut total),
            FerricError::Ok
        );
        assert!(total >= 1);

        // Collect all module names
        let mut found_main = false;
        for i in 0..total {
            let mut buf = vec![0i8; 64];
            let mut out_len: usize = 0;
            let result =
                ferric_engine_module_name(engine, i, buf.as_mut_ptr(), buf.len(), &mut out_len);
            assert_eq!(result, FerricError::Ok);
            let name = CStr::from_ptr(buf.as_ptr()).to_str().unwrap();
            if name == "MAIN" {
                found_main = true;
            }
        }
        assert!(found_main, "MAIN must appear in the module list");

        ferric_engine_free(engine);
    }
}

// ---------------------------------------------------------------------------
// Section 8: Agenda, Halt, Input, Clear Tests
// ---------------------------------------------------------------------------

#[test]
fn agenda_count_empty() {
    // A fresh engine with no rules has an empty agenda.
    unsafe {
        let engine = ferric_engine_new();
        ferric_engine_reset(engine);

        let mut count: usize = 999;
        let result = ferric_engine_agenda_count(engine, &mut count);

        assert_eq!(result, FerricError::Ok);
        assert_eq!(count, 0, "no rules means agenda is empty");

        ferric_engine_free(engine);
    }
}

#[test]
fn agenda_count_after_reset() {
    // After loading a rule and resetting, the agenda has at least one activation.
    unsafe {
        let engine = ferric_engine_new();
        let source =
            CString::new("(defrule check (initial-fact) => )").unwrap();
        assert_eq!(
            ferric_engine_load_string(engine, source.as_ptr()),
            FerricError::Ok
        );
        ferric_engine_reset(engine);

        let mut count: usize = 0;
        let result = ferric_engine_agenda_count(engine, &mut count);

        assert_eq!(result, FerricError::Ok);
        assert!(count >= 1, "rule matching initial-fact should be on the agenda");

        ferric_engine_free(engine);
    }
}

#[test]
fn is_halted_false_initially() {
    // A newly-created engine is not in the halted state.
    unsafe {
        let engine = ferric_engine_new();
        ferric_engine_reset(engine);

        let mut halted: i32 = 1; // initialise to "halted" so we can verify it changes
        let result = ferric_engine_is_halted(engine, &mut halted);

        assert_eq!(result, FerricError::Ok);
        assert_eq!(halted, 0, "engine must not be halted immediately after creation");

        ferric_engine_free(engine);
    }
}

#[test]
fn halt_and_check() {
    // Calling ferric_engine_halt transitions the engine to the halted state.
    unsafe {
        let engine = ferric_engine_new();
        ferric_engine_reset(engine);

        assert_eq!(ferric_engine_halt(engine), FerricError::Ok);

        let mut halted: i32 = 0;
        let result = ferric_engine_is_halted(engine, &mut halted);

        assert_eq!(result, FerricError::Ok);
        assert_eq!(halted, 1, "engine must be reported as halted after ferric_engine_halt");

        ferric_engine_free(engine);
    }
}

#[test]
fn halt_idempotent() {
    // Halting an already-halted engine is a no-op and returns Ok.
    unsafe {
        let engine = ferric_engine_new();
        ferric_engine_reset(engine);

        assert_eq!(ferric_engine_halt(engine), FerricError::Ok);
        assert_eq!(
            ferric_engine_halt(engine),
            FerricError::Ok,
            "second halt must also return Ok"
        );

        let mut halted: i32 = 0;
        ferric_engine_is_halted(engine, &mut halted);
        assert_eq!(halted, 1, "engine remains halted after double-halt");

        ferric_engine_free(engine);
    }
}

#[test]
fn push_input_null_line() {
    // A null line pointer must return NullPointer.
    unsafe {
        let engine = ferric_engine_new();
        ferric_engine_reset(engine);

        let result = ferric_engine_push_input(engine, ptr::null());
        assert_eq!(result, FerricError::NullPointer);

        ferric_engine_free(engine);
    }
}

#[test]
fn clear_resets_engine() {
    // ferric_engine_clear removes all loaded templates and rules.
    // After clear, template_count and rule_count both return 0.
    unsafe {
        let engine = ferric_engine_new();
        let source = CString::new(
            "(deftemplate thing (slot id))\
             (defrule notice (thing (id ?x)) => )",
        )
        .unwrap();
        assert_eq!(
            ferric_engine_load_string(engine, source.as_ptr()),
            FerricError::Ok
        );
        ferric_engine_reset(engine);

        // Confirm that one template and one rule are present before the clear
        let mut tpl_count: usize = 0;
        assert_eq!(
            ferric_engine_template_count(engine, &mut tpl_count),
            FerricError::Ok
        );
        assert_eq!(tpl_count, 1, "template should be present before clear");

        // Clear wipes all user-defined constructs
        assert_eq!(ferric_engine_clear(engine), FerricError::Ok);

        let mut tpl_after: usize = 999;
        assert_eq!(
            ferric_engine_template_count(engine, &mut tpl_after),
            FerricError::Ok
        );
        assert_eq!(tpl_after, 0, "clear must remove all templates");

        let mut rule_after: usize = 999;
        assert_eq!(
            ferric_engine_rule_count(engine, &mut rule_after),
            FerricError::Ok
        );
        assert_eq!(rule_after, 0, "clear must remove all rules");

        ferric_engine_free(engine);
    }
}

// ---------------------------------------------------------------------------
// Section 9: Convenience Variant Tests
// ---------------------------------------------------------------------------

#[test]
fn new_with_source_valid() {
    // ferric_engine_new_with_source creates a fully initialised engine from
    // CLIPS source text. The engine is immediately ready to run.
    unsafe {
        let source = CString::new(
            r#"(defrule hello (initial-fact) => (printout t "world" crlf))"#,
        )
        .unwrap();
        let engine = ferric_engine_new_with_source(source.as_ptr());
        assert!(!engine.is_null(), "new_with_source must return a non-null engine on valid input");

        // The engine should already be reset; running it should fire the rule once.
        let mut fired: u64 = 0;
        assert_eq!(ferric_engine_run(engine, -1, &mut fired), FerricError::Ok);
        assert_eq!(fired, 1, "rule should fire once on a freshly-created engine");

        ferric_engine_free(engine);
    }
}

#[test]
fn new_with_source_invalid() {
    // Invalid CLIPS source returns a null pointer (parse error, not a crash).
    unsafe {
        let bad_source = CString::new("(defrule broken (unclosed").unwrap();
        let engine = ferric_engine_new_with_source(bad_source.as_ptr());
        assert!(engine.is_null(), "invalid CLIPS source must return null");
    }
}

#[test]
fn new_with_source_null() {
    // A null source pointer returns null without crashing.
    unsafe {
        let engine = ferric_engine_new_with_source(ptr::null());
        assert!(engine.is_null(), "null source must return null");
    }
}

#[test]
fn new_with_source_config_null_config() {
    // When config is null, ferric_engine_new_with_source_config uses default configuration.
    unsafe {
        let source =
            CString::new(r#"(defrule r (initial-fact) => (printout t "ok" crlf))"#).unwrap();
        let engine = ferric_engine_new_with_source_config(source.as_ptr(), ptr::null());
        assert!(!engine.is_null(), "null config means default config, not failure");

        let mut fired: u64 = 0;
        assert_eq!(ferric_engine_run(engine, -1, &mut fired), FerricError::Ok);
        assert_eq!(fired, 1);

        ferric_engine_free(engine);
    }
}

#[test]
fn clear_output_channel() {
    // Clearing an output channel removes previously captured output.
    unsafe {
        let engine = ferric_engine_new();
        let source =
            CString::new(r#"(defrule emit (initial-fact) => (printout t "data" crlf))"#)
                .unwrap();
        assert_eq!(
            ferric_engine_load_string(engine, source.as_ptr()),
            FerricError::Ok
        );
        ferric_engine_reset(engine);

        let mut fired: u64 = 0;
        assert_eq!(ferric_engine_run(engine, -1, &mut fired), FerricError::Ok);
        assert_eq!(fired, 1);

        // Verify output exists before clearing
        let t_channel = CString::new("t").unwrap();
        let output_before = crate::engine::ferric_engine_get_output(engine, t_channel.as_ptr());
        assert!(!output_before.is_null(), "output must be present before clear");

        // Clear the channel
        assert_eq!(
            ferric_engine_clear_output(engine, t_channel.as_ptr()),
            FerricError::Ok
        );

        // After clearing, the output should be gone
        let output_after = crate::engine::ferric_engine_get_output(engine, t_channel.as_ptr());
        assert!(
            output_after.is_null(),
            "output must be absent after ferric_engine_clear_output"
        );

        ferric_engine_free(engine);
    }
}

#[test]
fn clear_output_null_channel() {
    // A null channel pointer returns NullPointer.
    unsafe {
        let engine = ferric_engine_new();
        ferric_engine_reset(engine);

        let result = ferric_engine_clear_output(engine, ptr::null());
        assert_eq!(result, FerricError::NullPointer);

        ferric_engine_free(engine);
    }
}

#[test]
fn run_ex_returns_halt_reason_limit_reached() {
    // When a rule limit is hit, run_ex writes LimitReached to out_reason.
    // We set up a chain of three rules to fire and only allow two firings.
    unsafe {
        let engine = ferric_engine_new();
        let source = CString::new(
            "(defrule r1 (initial-fact) => (assert (a)))\
             (defrule r2 (a) => (assert (b)))\
             (defrule r3 (b) => (assert (c)))",
        )
        .unwrap();
        assert_eq!(
            ferric_engine_load_string(engine, source.as_ptr()),
            FerricError::Ok
        );
        ferric_engine_reset(engine);

        let mut fired: u64 = 0;
        let mut reason = FerricHaltReason::AgendaEmpty;
        let result = ferric_engine_run_ex(engine, 2, &mut fired, &mut reason);

        assert_eq!(result, FerricError::Ok);
        assert_eq!(fired, 2);
        assert_eq!(
            reason,
            FerricHaltReason::LimitReached,
            "stopping at the limit must produce LimitReached"
        );

        ferric_engine_free(engine);
    }
}

#[test]
fn run_ex_agenda_empty() {
    // When there are no rules to fire, run_ex reports AgendaEmpty.
    unsafe {
        let engine = ferric_engine_new();
        ferric_engine_reset(engine);

        let mut fired: u64 = 999;
        let mut reason = FerricHaltReason::LimitReached;
        let result = ferric_engine_run_ex(engine, -1, &mut fired, &mut reason);

        assert_eq!(result, FerricError::Ok);
        assert_eq!(fired, 0);
        assert_eq!(
            reason,
            FerricHaltReason::AgendaEmpty,
            "empty agenda must produce AgendaEmpty"
        );

        ferric_engine_free(engine);
    }
}

#[test]
fn run_ex_halt_requested() {
    // When a rule fires the (halt) action, run_ex reports HaltRequested.
    unsafe {
        let engine = ferric_engine_new();
        let source =
            CString::new("(defrule stop-now (initial-fact) => (halt))").unwrap();
        assert_eq!(
            ferric_engine_load_string(engine, source.as_ptr()),
            FerricError::Ok
        );
        ferric_engine_reset(engine);

        let mut fired: u64 = 0;
        let mut reason = FerricHaltReason::AgendaEmpty;
        let result = ferric_engine_run_ex(engine, -1, &mut fired, &mut reason);

        assert_eq!(result, FerricError::Ok);
        assert_eq!(fired, 1, "the halt rule must fire once before stopping");
        assert_eq!(
            reason,
            FerricHaltReason::HaltRequested,
            "halt action must produce HaltRequested"
        );

        ferric_engine_free(engine);
    }
}
