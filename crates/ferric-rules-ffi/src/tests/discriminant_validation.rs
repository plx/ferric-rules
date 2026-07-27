//! Regression tests for FR-CABI-001 (issue #85): caller-populated
//! `FerricValue.value_type` discriminants cross the ABI as raw `u32` values
//! and are validated before interpretation.
//!
//! Every API that reads a caller-populated `FerricValue` must reject unknown
//! discriminants with `FerricError::InvalidArgument` — at top level and
//! nested inside multifields — and the resource-freeing paths must never
//! interpret an unknown tag. The companion C harness
//! (`tests/c/discriminant_abuse.c`, run via `just ffi-c-harness`) exercises
//! the same contract from a real C subprocess under ASan/UBSan.

use std::ffi::CString;
use std::ptr;

use crate::engine::{
    ferric_engine_assert_ordered, ferric_engine_assert_template, ferric_engine_fact_count,
    ferric_engine_free, ferric_engine_load_string, ferric_engine_new, ferric_engine_reset,
    FerricEngine,
};
use crate::error::FerricError;
use crate::types::{
    ferric_value_array_free, ferric_value_free, ferric_value_integer, FerricValue, FerricValueType,
};

/// Discriminants that must be rejected: the first out-of-range value,
/// arbitrary garbage, and all-ones.
const INVALID_TAGS: [u32; 4] = [7, 42, 0xDEAD_BEEF, u32::MAX];

/// A `FerricValue` with an invalid discriminant and poisoned payload fields
/// that must never be interpreted (dangling-looking pointers, huge length).
fn poisoned_value(tag: u32) -> FerricValue {
    FerricValue {
        value_type: tag,
        string_ptr: 0xDEAD_0001_usize as *mut _,
        multifield_ptr: 0xDEAD_0002_usize as *mut FerricValue,
        multifield_len: usize::MAX,
        ..FerricValue::void()
    }
}

unsafe fn new_engine_with_template() -> *mut FerricEngine {
    let engine = ferric_engine_new();
    let source =
        CString::new("(deftemplate person (slot name) (slot age) (multislot tags))").unwrap();
    assert_eq!(
        ferric_engine_load_string(engine, source.as_ptr()),
        FerricError::Ok
    );
    assert_eq!(ferric_engine_reset(engine), FerricError::Ok);
    engine
}

/// Assert a valid fact to prove the engine survived a rejected call.
unsafe fn assert_engine_usable(engine: *mut FerricEngine, probe_value: i64) {
    let relation = CString::new("probe").unwrap();
    let field = ferric_value_integer(probe_value);
    assert_eq!(
        ferric_engine_assert_ordered(engine, relation.as_ptr(), &field, 1, ptr::null_mut()),
        FerricError::Ok,
        "engine must stay usable after a rejected discriminant"
    );
}

#[test]
fn value_type_raw_round_trip() {
    for tag in 0..=6_u32 {
        let vt = FerricValueType::from_raw(tag).expect("0..=6 must be valid discriminants");
        assert_eq!(vt.as_raw(), tag);
    }
    for tag in INVALID_TAGS {
        assert!(FerricValueType::from_raw(tag).is_none());
        assert!(FerricValueType::try_from(tag).is_err());
    }
}

#[test]
fn assert_ordered_rejects_invalid_top_level_discriminant() {
    unsafe {
        let engine = new_engine_with_template();
        let relation = CString::new("bad").unwrap();

        for tag in INVALID_TAGS {
            let bad = poisoned_value(tag);
            let mut fact_id = 0_u64;
            assert_eq!(
                ferric_engine_assert_ordered(engine, relation.as_ptr(), &bad, 1, &mut fact_id),
                FerricError::InvalidArgument,
                "discriminant {tag} must be rejected"
            );
            assert_global_diag_names_tag(tag);
            assert_engine_usable(engine, i64::from(tag));
        }

        // No `bad` fact must have been asserted (only the usable-probe facts).
        let mut count = 0_usize;
        assert_eq!(
            ferric_engine_fact_count(engine, &mut count),
            FerricError::Ok
        );
        assert_eq!(count, INVALID_TAGS.len());

        ferric_engine_free(engine);
    }
}

#[test]
fn assert_ordered_rejects_invalid_nested_discriminant() {
    unsafe {
        let engine = new_engine_with_template();
        let relation = CString::new("bad").unwrap();

        for tag in INVALID_TAGS {
            let mut elems = [
                ferric_value_integer(10),
                poisoned_value(tag),
                ferric_value_integer(20),
            ];
            let mf = FerricValue {
                value_type: FerricValueType::Multifield.as_raw(),
                multifield_ptr: elems.as_mut_ptr(),
                multifield_len: elems.len(),
                ..FerricValue::void()
            };
            assert_eq!(
                ferric_engine_assert_ordered(engine, relation.as_ptr(), &mf, 1, ptr::null_mut()),
                FerricError::InvalidArgument,
                "nested discriminant {tag} must be rejected"
            );
            assert_global_diag_names_tag(tag);
            assert_engine_usable(engine, i64::from(tag));
        }

        ferric_engine_free(engine);
    }
}

#[test]
fn assert_ordered_rejects_invalid_doubly_nested_discriminant() {
    unsafe {
        let engine = new_engine_with_template();
        let relation = CString::new("bad").unwrap();

        let mut inner_bad = poisoned_value(0x00BA_DBAD);
        let mut inner = FerricValue {
            value_type: FerricValueType::Multifield.as_raw(),
            multifield_ptr: &mut inner_bad,
            multifield_len: 1,
            ..FerricValue::void()
        };
        let outer = FerricValue {
            value_type: FerricValueType::Multifield.as_raw(),
            multifield_ptr: &mut inner,
            multifield_len: 1,
            ..FerricValue::void()
        };
        assert_eq!(
            ferric_engine_assert_ordered(engine, relation.as_ptr(), &outer, 1, ptr::null_mut()),
            FerricError::InvalidArgument
        );

        ferric_engine_free(engine);
    }
}

#[test]
fn assert_template_rejects_invalid_discriminants() {
    unsafe {
        let engine = new_engine_with_template();
        let template = CString::new("person").unwrap();
        let age = CString::new("age").unwrap();
        let tags = CString::new("tags").unwrap();

        // Top-level slot value.
        for tag in INVALID_TAGS {
            let bad = poisoned_value(tag);
            let names = [age.as_ptr()];
            assert_eq!(
                ferric_engine_assert_template(
                    engine,
                    template.as_ptr(),
                    names.as_ptr(),
                    &bad,
                    1,
                    ptr::null_mut(),
                ),
                FerricError::InvalidArgument,
                "slot discriminant {tag} must be rejected"
            );
            assert_global_diag_names_tag(tag);
        }

        // Nested inside a multislot value.
        let mut elems = [ferric_value_integer(1), poisoned_value(7)];
        let mf = FerricValue {
            value_type: FerricValueType::Multifield.as_raw(),
            multifield_ptr: elems.as_mut_ptr(),
            multifield_len: elems.len(),
            ..FerricValue::void()
        };
        let names = [tags.as_ptr()];
        assert_eq!(
            ferric_engine_assert_template(
                engine,
                template.as_ptr(),
                names.as_ptr(),
                &mf,
                1,
                ptr::null_mut(),
            ),
            FerricError::InvalidArgument
        );

        assert_engine_usable(engine, 1);
        ferric_engine_free(engine);
    }
}

/// Assert that the global error channel holds a diagnostic identifying the
/// invalid raw discriminant `tag`.
fn assert_global_diag_names_tag(tag: u32) {
    crate::error::with_global_error(|msg| {
        let msg = msg.expect("a diagnostic must be recorded in the global error channel");
        let needle = format!("invalid value_type discriminant: {tag}");
        assert!(
            msg.contains(&needle),
            "diagnostic must contain {needle:?}, got {msg:?}"
        );
    });
}

#[test]
fn value_free_rejects_invalid_discriminant() {
    unsafe {
        // The poisoned payload pointers are not allocations; interpreting
        // them would crash the test process (and abort under sanitizers).
        for tag in INVALID_TAGS {
            let mut bad = poisoned_value(tag);
            assert_eq!(ferric_value_free(&mut bad), FerricError::InvalidArgument);
            assert_global_diag_names_tag(tag);
        }
    }
}

#[test]
fn value_free_null_and_valid_return_ok() {
    unsafe {
        assert_eq!(ferric_value_free(ptr::null_mut()), FerricError::Ok);
        assert_eq!(ferric_value_array_free(ptr::null_mut(), 0), FerricError::Ok);
        let mut ok = ferric_value_integer(5);
        assert_eq!(ferric_value_free(&mut ok), FerricError::Ok);

        let values = vec![ferric_value_integer(1), ferric_value_integer(2)];
        let len = values.len();
        let arr = Box::into_raw(values.into_boxed_slice()).cast::<FerricValue>();
        assert_eq!(ferric_value_array_free(arr, len), FerricError::Ok);
    }
}

#[test]
fn value_array_free_rejects_but_frees_known_siblings() {
    unsafe {
        // Build an FFI-owned array the way `value_to_ferric` does, then
        // corrupt one element's tag: the free path must report the invalid
        // tag while still freeing known siblings (the boxed string below —
        // a leak would be reported by leak-checking runs) and the array
        // allocation itself.
        let sibling = CString::new("owned-sibling").unwrap();
        let values = vec![
            FerricValue {
                value_type: FerricValueType::String.as_raw(),
                string_ptr: sibling.into_raw(),
                ..FerricValue::void()
            },
            ferric_value_integer(2),
        ];
        let len = values.len();
        let arr = Box::into_raw(values.into_boxed_slice()).cast::<FerricValue>();

        (*arr.add(1)).value_type = 0xDEAD_BEEF;
        (*arr.add(1)).string_ptr = 0xDEAD_0003_usize as *mut _;

        assert_eq!(
            ferric_value_array_free(arr, len),
            FerricError::InvalidArgument
        );
        assert_global_diag_names_tag(0xDEAD_BEEF);
    }
}

#[test]
fn value_free_rejects_invalid_nested_tag_in_owned_multifield() {
    unsafe {
        // An FFI-owned multifield whose nested element tag is corrupted:
        // ferric_value_free must return InvalidArgument, skip the corrupted
        // element's payload, and still free the sibling and the array.
        let sibling = CString::new("owned-nested-sibling").unwrap();
        let elems = vec![
            FerricValue {
                value_type: FerricValueType::String.as_raw(),
                string_ptr: sibling.into_raw(),
                ..FerricValue::void()
            },
            ferric_value_integer(9),
        ];
        let len = elems.len();
        let arr = Box::into_raw(elems.into_boxed_slice()).cast::<FerricValue>();
        (*arr.add(1)).value_type = 999;
        (*arr.add(1)).multifield_ptr = 0xDEAD_0004_usize as *mut FerricValue;
        (*arr.add(1)).multifield_len = usize::MAX;

        let mut mf = FerricValue {
            value_type: FerricValueType::Multifield.as_raw(),
            multifield_ptr: arr,
            multifield_len: len,
            ..FerricValue::void()
        };
        assert_eq!(ferric_value_free(&mut mf), FerricError::InvalidArgument);
        assert_global_diag_names_tag(999);
    }
}
