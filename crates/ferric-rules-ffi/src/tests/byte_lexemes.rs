//! Byte-span value ownership, type identity, and output round trips at the C ABI.

use std::ffi::{CStr, CString};
use std::ptr;

use crate::engine::{
    ferric_engine_action_diagnostic_count, ferric_engine_assert_ordered, ferric_engine_free,
    ferric_engine_get_fact_field, ferric_engine_get_output, ferric_engine_get_output_copy,
    ferric_engine_last_error, ferric_engine_load_string, ferric_engine_new, ferric_engine_reset,
    ferric_engine_run, ferric_engine_run_ex,
};
use crate::error::FerricError;
use crate::types::{
    ferric_value_free, ferric_value_instance_name, ferric_value_multifield_copy,
    ferric_value_string_raw, ferric_value_symbol_raw, FerricHaltReason, FerricValue,
    FerricValueType,
};

type Constructor = unsafe extern "C" fn(*const u8, usize, *mut FerricValue) -> FerricError;
const CONSTRUCTORS: [(Constructor, FerricValueType); 3] = [
    (ferric_value_string_raw, FerricValueType::StringBytes),
    (ferric_value_symbol_raw, FerricValueType::SymbolBytes),
    (ferric_value_instance_name, FerricValueType::InstanceName),
];

unsafe fn bytes(value: &FerricValue) -> &[u8] {
    if value.multifield_len == 0 {
        &[]
    } else {
        std::slice::from_raw_parts(value.string_ptr.cast(), value.multifield_len)
    }
}

#[test]
fn raw_constructors_own_exact_spans_and_validate_lengths() {
    unsafe {
        for (construct, tag) in CONSTRUCTORS {
            for source in [b"a\0\xff\xc3".as_slice(), b"", "héllo".as_bytes()] {
                let mut out = FerricValue::void();
                assert_eq!(
                    construct(source.as_ptr(), source.len(), &mut out),
                    FerricError::Ok
                );
                assert_eq!(out.value_type, tag.as_raw());
                assert_eq!(bytes(&out), source);
                if !source.is_empty() {
                    assert_ne!(out.string_ptr.cast_const().cast::<u8>(), source.as_ptr());
                }
                assert_eq!(ferric_value_free(&mut out), FerricError::Ok);
            }
            let mut out = FerricValue::void();
            assert_eq!(construct(ptr::null(), 0, &mut out), FerricError::Ok);
            assert_eq!(out.multifield_len, 0);
            assert!(out.string_ptr.is_null());
            assert_eq!(ferric_value_free(&mut out), FerricError::Ok);
            assert_eq!(
                construct(ptr::null(), 1, &mut out),
                FerricError::NullPointer
            );
            assert_eq!(out.value_type, FerricValueType::Void.as_raw());
            assert_eq!(
                construct(b"x".as_ptr(), usize::MAX, &mut out),
                FerricError::InvalidArgument
            );
            assert_eq!(out.value_type, FerricValueType::Void.as_raw());
            assert_eq!(
                construct(ptr::null(), 0, ptr::null_mut()),
                FerricError::NullPointer
            );
        }
    }
}

#[test]
fn recursive_copy_and_assertion_preserve_bytes_and_semantic_types() {
    unsafe {
        let engine = ferric_engine_new();
        let source = CString::new("(defrule typed (payload ?string ?symbol ?instance) => (printout t (type ?string) \"|\" (type ?symbol) \"|\" (instance-namep ?instance)))").unwrap();
        assert_eq!(
            ferric_engine_load_string(engine, source.as_ptr()),
            FerricError::Ok
        );
        assert_eq!(ferric_engine_reset(engine), FerricError::Ok);
        let payload = [b'z', 0, 0xff, 0xc3];
        let borrowed: Vec<FerricValue> = CONSTRUCTORS
            .iter()
            .map(|(_, tag)| FerricValue {
                value_type: tag.as_raw(),
                string_ptr: payload.as_ptr().cast_mut().cast(),
                multifield_len: payload.len(),
                ..FerricValue::void()
            })
            .collect();
        let mut copied = FerricValue::void();
        assert_eq!(
            ferric_value_multifield_copy(borrowed.as_ptr(), borrowed.len(), &mut copied),
            FerricError::Ok
        );
        let elements = std::slice::from_raw_parts(copied.multifield_ptr, copied.multifield_len);
        for (element, (_, tag)) in elements.iter().zip(CONSTRUCTORS) {
            assert_eq!(element.value_type, tag.as_raw());
            assert_eq!(bytes(element), payload);
        }
        let relation = CString::new("payload").unwrap();
        let mut fact = 0;
        assert_eq!(
            ferric_engine_assert_ordered(
                engine,
                relation.as_ptr(),
                copied.multifield_ptr,
                copied.multifield_len,
                &mut fact
            ),
            FerricError::Ok
        );
        assert_eq!(ferric_value_free(&mut copied), FerricError::Ok);
        for (index, (_, tag)) in CONSTRUCTORS.iter().enumerate() {
            let mut out = FerricValue::void();
            assert_eq!(
                ferric_engine_get_fact_field(engine, fact, index, &mut out),
                FerricError::Ok
            );
            assert_eq!(out.value_type, tag.as_raw());
            assert_eq!(bytes(&out), payload);
            assert_eq!(ferric_value_free(&mut out), FerricError::Ok);
        }
        let mut fired = 0;
        assert_eq!(ferric_engine_run(engine, -1, &mut fired), FerricError::Ok);
        assert_eq!(fired, 1);
        let channel = CString::new("t").unwrap();
        assert_eq!(
            CStr::from_ptr(ferric_engine_get_output(engine, channel.as_ptr())).to_bytes(),
            b"STRING|SYMBOL|TRUE"
        );
        assert_eq!(ferric_engine_free(engine), FerricError::Ok);
    }
}

#[test]
fn name_class_lookups_preserve_diagnostics_and_halt_at_the_c_boundary() {
    unsafe {
        for operation in ["(type ?name)", "(classify ?name)"] {
            let engine = ferric_engine_new();
            let source = CString::new(format!(
                r#"
                (defgeneric classify)
                (defmethod classify ((?value INSTANCE-NAME)) unexpected)
                (defrule inspect (payload ?name) =>
                    (printout t (instance-namep ?name))
                    {operation}
                    (printout t "continued"))
                "#
            ))
            .unwrap();
            assert_eq!(
                ferric_engine_load_string(engine, source.as_ptr()),
                FerricError::Ok
            );
            assert_eq!(ferric_engine_reset(engine), FerricError::Ok);
            let mut name = FerricValue::void();
            assert_eq!(
                ferric_value_instance_name(b"widget".as_ptr(), 6, &mut name),
                FerricError::Ok
            );
            let relation = CString::new("payload").unwrap();
            assert_eq!(
                ferric_engine_assert_ordered(engine, relation.as_ptr(), &name, 1, ptr::null_mut()),
                FerricError::Ok
            );
            assert_eq!(ferric_value_free(&mut name), FerricError::Ok);
            let mut reason = FerricHaltReason::AgendaEmpty;
            assert_eq!(
                ferric_engine_run_ex(engine, -1, ptr::null_mut(), &mut reason),
                FerricError::Ok
            );
            assert_eq!(reason, FerricHaltReason::ActionError, "{operation}");
            let channel = CString::new("t").unwrap();
            assert_eq!(
                CStr::from_ptr(ferric_engine_get_output(engine, channel.as_ptr())).to_bytes(),
                b"TRUE",
                "{operation}"
            );
            let mut count = 0;
            assert_eq!(
                ferric_engine_action_diagnostic_count(engine, &mut count),
                FerricError::Ok
            );
            assert!(count > 0, "{operation}");
            assert_eq!(ferric_engine_free(engine), FerricError::Ok);
        }
    }
}

#[test]
fn byte_output_copy_is_exact_and_text_output_reports_invalid_utf8() {
    unsafe {
        let engine = ferric_engine_new();
        let source =
            CString::new("(defrule emit (payload ?value) => (printout t ?value))").unwrap();
        assert_eq!(
            ferric_engine_load_string(engine, source.as_ptr()),
            FerricError::Ok
        );
        assert_eq!(ferric_engine_reset(engine), FerricError::Ok);
        let payload = [b'x', 0xff, 0xc3, 0];
        let mut value = FerricValue::void();
        assert_eq!(
            ferric_value_string_raw(payload.as_ptr(), payload.len(), &mut value),
            FerricError::Ok
        );
        let relation = CString::new("payload").unwrap();
        assert_eq!(
            ferric_engine_assert_ordered(engine, relation.as_ptr(), &value, 1, ptr::null_mut()),
            FerricError::Ok
        );
        assert_eq!(ferric_value_free(&mut value), FerricError::Ok);
        let mut fired = 0;
        assert_eq!(ferric_engine_run(engine, -1, &mut fired), FerricError::Ok);
        assert_eq!(fired, 1);
        let channel = CString::new("t").unwrap();
        assert!(ferric_engine_get_output(engine, channel.as_ptr()).is_null());
        assert!(CStr::from_ptr(ferric_engine_last_error(engine))
            .to_str()
            .unwrap()
            .contains("not valid UTF-8"));
        let mut needed = 0;
        assert_eq!(
            ferric_engine_get_output_copy(
                engine,
                channel.as_ptr(),
                ptr::null_mut(),
                0,
                &mut needed
            ),
            FerricError::Ok
        );
        assert_eq!(needed, payload.len() + 1);
        let mut result = vec![0; needed];
        assert_eq!(
            ferric_engine_get_output_copy(
                engine,
                channel.as_ptr(),
                result.as_mut_ptr().cast(),
                result.len(),
                &mut needed
            ),
            FerricError::Ok
        );
        assert_eq!(&result[..payload.len()], payload);
        assert_eq!(result[payload.len()], 0);
        let mut prefix = [0; 3];
        assert_eq!(
            ferric_engine_get_output_copy(
                engine,
                channel.as_ptr(),
                prefix.as_mut_ptr().cast(),
                prefix.len(),
                &mut needed
            ),
            FerricError::BufferTooSmall
        );
        assert_eq!(prefix, [b'x', 0xff, 0]);
        assert_eq!(needed, payload.len() + 1);
        assert_eq!(ferric_engine_free(engine), FerricError::Ok);
    }
}
