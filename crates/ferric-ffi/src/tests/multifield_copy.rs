//! Regression tests for FR-CABI-008 (issue #87): borrowed foreign value trees
//! are deep-copied into Ferric-owned multifields with explicit provenance.

use std::ffi::CStr;
use std::ptr;

use crate::error::{ferric_last_error_global, FerricError};
use crate::types::{
    ferric_value_float, ferric_value_free, ferric_value_integer, ferric_value_multifield_copy,
    FerricValue, FerricValueType,
};

fn borrowed_text(value_type: FerricValueType, bytes: &mut [u8]) -> FerricValue {
    assert_eq!(
        bytes.last(),
        Some(&0),
        "borrowed text must be NUL-terminated"
    );
    FerricValue {
        value_type: value_type.as_raw(),
        string_ptr: bytes.as_mut_ptr().cast(),
        ..FerricValue::void()
    }
}

fn borrowed_multifield(values: &mut [FerricValue]) -> FerricValue {
    FerricValue {
        value_type: FerricValueType::Multifield.as_raw(),
        multifield_ptr: if values.is_empty() {
            ptr::null_mut()
        } else {
            values.as_mut_ptr()
        },
        multifield_len: values.len(),
        ..FerricValue::void()
    }
}

fn external_address(type_id: u32, pointer: *mut std::ffi::c_void) -> FerricValue {
    FerricValue {
        value_type: FerricValueType::ExternalAddress.as_raw(),
        external_type_id: type_id,
        external_pointer: pointer,
        ..FerricValue::void()
    }
}

fn assert_void(value: &FerricValue) {
    assert_eq!(value.value_type, FerricValueType::Void.as_raw());
    assert_eq!(value.integer, 0);
    assert_eq!(value.float.to_bits(), 0.0_f64.to_bits());
    assert!(value.string_ptr.is_null());
    assert!(value.multifield_ptr.is_null());
    assert_eq!(value.multifield_len, 0);
    assert_eq!(value.external_type_id, 0);
    assert!(value.external_pointer.is_null());
}

unsafe fn multifield_elements(value: &FerricValue) -> &[FerricValue] {
    assert_eq!(
        value.value_type,
        FerricValueType::Multifield.as_raw(),
        "expected a multifield"
    );
    if value.multifield_len == 0 {
        &[]
    } else {
        std::slice::from_raw_parts(value.multifield_ptr, value.multifield_len)
    }
}

fn nested_integer_layers(multifield_levels: usize) -> Vec<Box<[FerricValue]>> {
    let mut layers = Vec::<Box<[FerricValue]>>::with_capacity(multifield_levels + 1);
    layers.push(vec![ferric_value_integer(1)].into_boxed_slice());
    for _ in 0..multifield_levels {
        let inner = layers
            .last_mut()
            .expect("at least one input layer")
            .as_mut_ptr();
        layers.push(
            vec![FerricValue {
                value_type: FerricValueType::Multifield.as_raw(),
                multifield_ptr: inner,
                multifield_len: 1,
                ..FerricValue::void()
            }]
            .into_boxed_slice(),
        );
    }
    layers
}

#[test]
fn copy_constructs_empty_multifield_and_validates_required_pointers() {
    unsafe {
        let mut out = ferric_value_integer(91);
        assert_eq!(
            ferric_value_multifield_copy(ptr::null(), 0, &mut out),
            FerricError::Ok
        );
        assert_eq!(out.value_type, FerricValueType::Multifield.as_raw());
        assert!(out.multifield_ptr.is_null());
        assert_eq!(out.multifield_len, 0);
        assert_eq!(ferric_value_free(&mut out), FerricError::Ok);

        assert_eq!(
            ferric_value_multifield_copy(ptr::null(), 0, ptr::null_mut()),
            FerricError::NullPointer
        );

        let mut out = FerricValue {
            value_type: FerricValueType::Integer.as_raw(),
            integer: 92,
            float: 4.5,
            string_ptr: 1_usize as *mut _,
            multifield_ptr: 2_usize as *mut _,
            multifield_len: 9,
            external_type_id: 11,
            external_pointer: 3_usize as *mut _,
        };
        assert_eq!(
            ferric_value_multifield_copy(ptr::null(), 1, &mut out),
            FerricError::NullPointer
        );
        assert_void(&out);
    }
}

#[test]
fn copy_deeply_owns_mixed_nested_tree_but_not_external_payload() {
    unsafe {
        let mut symbol = b"alpha\0".to_vec();
        let mut string = b"hello\0".to_vec();
        let mut non_utf8_symbol = vec![0xff, 0xfe, 0];
        let mut external_payload = 77_i32;
        let external_payload_ptr = ptr::addr_of_mut!(external_payload).cast::<std::ffi::c_void>();

        let mut nested = [
            borrowed_text(FerricValueType::Symbol, &mut non_utf8_symbol),
            ferric_value_integer(1234),
            ferric_value_float(2.5),
        ];
        let nested_source_ptr = nested.as_mut_ptr();
        let mut source = [
            ferric_value_integer(42),
            ferric_value_float(3.25),
            borrowed_text(FerricValueType::Symbol, &mut symbol),
            borrowed_text(FerricValueType::String, &mut string),
            borrowed_multifield(&mut nested),
            external_address(19, external_payload_ptr),
            FerricValue::void(),
        ];
        let source_ptr = source.as_mut_ptr();

        let mut out = FerricValue::void();
        assert_eq!(
            ferric_value_multifield_copy(source.as_ptr(), source.len(), &mut out),
            FerricError::Ok
        );

        let copied = multifield_elements(&out);
        assert_eq!(copied.len(), source.len());
        assert_ne!(out.multifield_ptr, source_ptr);
        assert_ne!(copied[4].multifield_ptr, nested_source_ptr);
        assert_ne!(copied[2].string_ptr.cast::<u8>(), symbol.as_mut_ptr());
        assert_ne!(copied[3].string_ptr.cast::<u8>(), string.as_mut_ptr());
        assert_ne!(
            (*copied[4].multifield_ptr).string_ptr.cast::<u8>(),
            non_utf8_symbol.as_mut_ptr()
        );

        source[0].integer = -1;
        nested[1].integer = -2;
        symbol[0] = b'X';
        string[0] = b'Y';
        non_utf8_symbol[0] = 1;
        assert_eq!(source[0].integer, -1);
        assert_eq!(nested[1].integer, -2);

        let copied = multifield_elements(&out);
        assert_eq!(copied[0].integer, 42);
        assert_eq!(copied[1].float.to_bits(), 3.25_f64.to_bits());
        assert_eq!(CStr::from_ptr(copied[2].string_ptr).to_bytes(), b"alpha");
        assert_eq!(CStr::from_ptr(copied[3].string_ptr).to_bytes(), b"hello");
        let copied_nested = multifield_elements(&copied[4]);
        assert_eq!(
            copied_nested[0].value_type,
            FerricValueType::Symbol.as_raw()
        );
        assert_eq!(
            CStr::from_ptr(copied_nested[0].string_ptr).to_bytes(),
            [0xff, 0xfe]
        );
        assert_eq!(copied_nested[1].integer, 1234);
        assert_eq!(copied_nested[2].float.to_bits(), 2.5_f64.to_bits());
        assert_eq!(copied[5].external_type_id, 19);
        assert_eq!(copied[5].external_pointer, external_payload_ptr);
        assert_eq!(copied[6].value_type, FerricValueType::Void.as_raw());

        assert_eq!(ferric_value_free(&mut out), FerricError::Ok);
        assert_eq!(external_payload, 77);
    }
}

#[test]
fn copy_rejects_malformed_nested_inputs_and_leaves_void_output() {
    unsafe {
        let mut out = ferric_value_integer(1);
        let invalid = FerricValue {
            value_type: u32::MAX,
            string_ptr: 1_usize as *mut _,
            multifield_ptr: 2_usize as *mut _,
            multifield_len: usize::MAX,
            ..FerricValue::void()
        };
        assert_eq!(
            ferric_value_multifield_copy(&invalid, 1, &mut out),
            FerricError::InvalidArgument
        );
        assert_void(&out);
        let diagnostic = CStr::from_ptr(ferric_last_error_global())
            .to_string_lossy()
            .into_owned();
        assert!(
            diagnostic.contains("invalid value_type discriminant"),
            "unexpected diagnostic: {diagnostic}"
        );

        let null_string = FerricValue {
            value_type: FerricValueType::String.as_raw(),
            ..FerricValue::void()
        };
        out = ferric_value_integer(2);
        assert_eq!(
            ferric_value_multifield_copy(&null_string, 1, &mut out),
            FerricError::InvalidArgument
        );
        assert_void(&out);

        let null_nested = FerricValue {
            value_type: FerricValueType::Multifield.as_raw(),
            multifield_len: 1,
            ..FerricValue::void()
        };
        out = ferric_value_integer(3);
        assert_eq!(
            ferric_value_multifield_copy(&null_nested, 1, &mut out),
            FerricError::InvalidArgument
        );
        assert_void(&out);

        let mut cycle = FerricValue::void();
        cycle.value_type = FerricValueType::Multifield.as_raw();
        cycle.multifield_ptr = &mut cycle;
        cycle.multifield_len = 1;
        out = ferric_value_integer(4);
        assert_eq!(
            ferric_value_multifield_copy(&cycle, 1, &mut out),
            FerricError::InvalidArgument
        );
        assert_void(&out);

        let mut overlap = [FerricValue::void(), ferric_value_integer(9)];
        overlap[0].value_type = FerricValueType::Multifield.as_raw();
        overlap[0].multifield_ptr = overlap.as_mut_ptr().add(1);
        overlap[0].multifield_len = 1;
        out = ferric_value_integer(5);
        assert_eq!(
            ferric_value_multifield_copy(overlap.as_ptr(), overlap.len(), &mut out),
            FerricError::InvalidArgument
        );
        assert_void(&out);
    }
}

#[test]
fn copy_cleans_partial_nested_values_after_late_validation_failure() {
    unsafe {
        let mut first = b"first\0".to_vec();
        let mut second = b"second\0".to_vec();
        let mut third = b"third\0".to_vec();
        let mut nested = [
            borrowed_text(FerricValueType::Symbol, &mut second),
            borrowed_text(FerricValueType::String, &mut third),
            FerricValue {
                value_type: 77,
                ..FerricValue::void()
            },
        ];
        let source = [
            borrowed_text(FerricValueType::String, &mut first),
            borrowed_multifield(&mut nested),
        ];

        for index in 0..64 {
            let mut out = ferric_value_integer(index);
            assert_eq!(
                ferric_value_multifield_copy(source.as_ptr(), source.len(), &mut out),
                FerricError::InvalidArgument
            );
            assert_void(&out);
        }
    }
}

#[test]
fn copy_handles_large_borrowed_array_and_source_destruction() {
    unsafe {
        let mut text = b"borrowed-large\0".to_vec();
        let mut source = Vec::with_capacity(1025);
        for index in 0..1025 {
            if index % 5 == 0 {
                source.push(borrowed_text(FerricValueType::String, &mut text));
            } else {
                source.push(ferric_value_integer(index));
            }
        }

        let mut out = FerricValue::void();
        assert_eq!(
            ferric_value_multifield_copy(source.as_ptr(), source.len(), &mut out),
            FerricError::Ok
        );
        text[0] = b'X';
        drop(source);

        let copied = multifield_elements(&out);
        assert_eq!(copied.len(), 1025);
        assert_eq!(
            CStr::from_ptr(copied[0].string_ptr).to_bytes(),
            b"borrowed-large"
        );
        assert_eq!(copied[1024].integer, 1024);
        assert_eq!(ferric_value_free(&mut out), FerricError::Ok);
    }
}

#[test]
fn copy_rejects_excessive_acyclic_nesting_before_recursive_free_is_unsafe() {
    unsafe {
        let accepted_layers = nested_integer_layers(128);
        let accepted_outermost = accepted_layers.last().expect("outer input layer");
        let mut accepted = FerricValue::void();
        assert_eq!(
            ferric_value_multifield_copy(
                accepted_outermost.as_ptr(),
                accepted_outermost.len(),
                &mut accepted,
            ),
            FerricError::Ok
        );
        assert_eq!(ferric_value_free(&mut accepted), FerricError::Ok);

        let rejected_layers = nested_integer_layers(129);
        let rejected_outermost = rejected_layers.last().expect("outer input layer");
        let mut out = ferric_value_integer(5);
        assert_eq!(
            ferric_value_multifield_copy(
                rejected_outermost.as_ptr(),
                rejected_outermost.len(),
                &mut out,
            ),
            FerricError::InvalidArgument
        );
        assert_void(&out);
        let diagnostic = CStr::from_ptr(ferric_last_error_global()).to_string_lossy();
        assert!(
            diagnostic.contains("nesting depth exceeds maximum of 128"),
            "unexpected diagnostic: {diagnostic}"
        );
    }
}
