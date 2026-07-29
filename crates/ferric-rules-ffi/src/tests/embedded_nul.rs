//! Regression coverage for embedded-NUL handling at the C ABI boundary.

#[cfg(feature = "serde")]
use crate::engine::{
    ferric_bytes_free, ferric_engine_deserialize_bincode, ferric_engine_serialize_bincode,
};
use crate::engine::{
    ferric_engine_free, ferric_engine_get_fact_field, ferric_engine_get_fact_slot_by_name,
    ferric_engine_get_output, ferric_engine_get_output_copy, ferric_engine_last_error,
    ferric_engine_load_string, ferric_engine_new, ferric_engine_reset, ferric_engine_run,
    FerricEngine,
};
use crate::error::{ferric_last_error_global, FerricError};
use crate::types::{
    ferric_value_free, ferric_value_string_bytes, ferric_value_symbol_bytes, FerricValue,
    FerricValueType,
};
use ferric_rules_core::{Multifield, Value};
use slotmap::Key as _;
use std::ffi::{CStr, CString};

unsafe fn assert_internal_value(engine: *mut FerricEngine, relation: &str, value: Value) -> u64 {
    (&mut *engine)
        .engine
        .assert_ordered(relation, value)
        .expect("test value should be accepted by the Rust runtime")
        .data()
        .as_ffi()
}

unsafe fn nul_string_value(engine: *mut FerricEngine) -> Value {
    Value::String(
        (&*engine)
            .engine
            .create_string("a\0b")
            .expect("the Rust runtime supports embedded NUL"),
    )
}

#[test]
fn length_aware_value_constructors_reject_nul_and_preserve_valid_bytes() {
    unsafe {
        for constructor in [ferric_value_symbol_bytes, ferric_value_string_bytes] {
            let mut out = FerricValue {
                value_type: FerricValueType::Integer.as_raw(),
                integer: 91,
                ..FerricValue::void()
            };
            assert_eq!(
                constructor(b"a\0b".as_ptr(), 3, &mut out),
                FerricError::InvalidArgument
            );
            assert_eq!(out.value_type, FerricValueType::Void.as_raw());
            assert!(out.string_ptr.is_null());
            let diagnostic = ferric_last_error_global();
            assert!(!diagnostic.is_null());
            assert!(CStr::from_ptr(diagnostic)
                .to_string_lossy()
                .contains("embedded NUL at byte 1"));

            let valid = "héllo".as_bytes();
            assert_eq!(
                constructor(valid.as_ptr(), valid.len(), &mut out),
                FerricError::Ok
            );
            assert_eq!(
                CStr::from_ptr(out.string_ptr).to_bytes(),
                valid,
                "valid UTF-8 must be copied exactly"
            );
            assert_eq!(ferric_value_free(&mut out), FerricError::Ok);

            assert_eq!(constructor(std::ptr::null(), 0, &mut out), FerricError::Ok);
            assert_eq!(CStr::from_ptr(out.string_ptr).to_bytes(), b"");
            assert_eq!(ferric_value_free(&mut out), FerricError::Ok);

            assert_eq!(
                constructor(std::ptr::null(), 1, &mut out),
                FerricError::NullPointer
            );
            assert_eq!(out.value_type, FerricValueType::Void.as_raw());

            assert_eq!(
                constructor([0xff].as_ptr(), 1, &mut out),
                FerricError::InvalidArgument
            );
            assert_eq!(out.value_type, FerricValueType::Void.as_raw());
            let diagnostic = ferric_last_error_global();
            assert!(!diagnostic.is_null());
            assert!(CStr::from_ptr(diagnostic)
                .to_string_lossy()
                .contains("not valid UTF-8"));
        }
    }
}

#[test]
fn legacy_value_egress_rejects_embedded_nul_strings_and_symbols() {
    unsafe {
        let engine = ferric_engine_new();
        assert!(!engine.is_null());

        let string_id = assert_internal_value(engine, "string-payload", nul_string_value(engine));
        let symbol = (&mut *engine)
            .engine
            .intern_symbol("a\0b")
            .expect("the Rust runtime supports embedded NUL");
        let symbol_id = assert_internal_value(engine, "symbol-payload", Value::Symbol(symbol));

        for fact_id in [string_id, symbol_id] {
            let mut out = FerricValue {
                value_type: FerricValueType::Integer.as_raw(),
                integer: 91,
                ..FerricValue::void()
            };
            assert_eq!(
                ferric_engine_get_fact_field(engine, fact_id, 0, &mut out),
                FerricError::InvalidArgument,
                "legacy FerricValue egress must reject content it cannot represent"
            );
            assert_eq!(out.value_type, FerricValueType::Void.as_raw());
            assert!(out.string_ptr.is_null());

            let diagnostic = ferric_engine_last_error(engine);
            assert!(!diagnostic.is_null());
            assert!(
                CStr::from_ptr(diagnostic)
                    .to_string_lossy()
                    .contains("embedded NUL"),
                "the rejection must explain the offending content"
            );
        }

        assert_eq!(ferric_engine_free(engine), FerricError::Ok);
    }
}

#[test]
fn nested_and_template_slot_value_egress_rejects_embedded_nul() {
    unsafe {
        let engine = ferric_engine_new();
        assert!(!engine.is_null());
        let source = CString::new("(deftemplate item (slot value))").unwrap();
        assert_eq!(
            ferric_engine_load_string(engine, source.as_ptr()),
            FerricError::Ok
        );
        assert_eq!(ferric_engine_reset(engine), FerricError::Ok);

        let template_value = nul_string_value(engine);
        let template_id = (&mut *engine)
            .engine
            .assert_template("item", &["value"], vec![template_value])
            .unwrap()
            .data()
            .as_ffi();
        let mut out = FerricValue {
            value_type: FerricValueType::Integer.as_raw(),
            integer: 91,
            ..FerricValue::void()
        };
        let slot_name = CString::new("value").unwrap();
        assert_eq!(
            ferric_engine_get_fact_slot_by_name(engine, template_id, slot_name.as_ptr(), &mut out,),
            FerricError::InvalidArgument
        );
        assert_eq!(out.value_type, FerricValueType::Void.as_raw());

        let mut multifield = Multifield::new();
        multifield.push(Value::Integer(7));
        multifield.push(nul_string_value(engine));
        let multifield_id = assert_internal_value(
            engine,
            "nested-payload",
            Value::Multifield(Box::new(multifield)),
        );
        out = FerricValue {
            value_type: FerricValueType::Integer.as_raw(),
            integer: 92,
            ..FerricValue::void()
        };
        assert_eq!(
            ferric_engine_get_fact_field(engine, multifield_id, 0, &mut out),
            FerricError::InvalidArgument
        );
        assert_eq!(out.value_type, FerricValueType::Void.as_raw());

        assert_eq!(ferric_engine_free(engine), FerricError::Ok);
    }
}

#[cfg(feature = "serde")]
#[test]
fn snapshot_round_trip_preserves_nul_before_legacy_egress_rejects_it() {
    unsafe {
        let engine = ferric_engine_new();
        assert!(!engine.is_null());
        assert_internal_value(engine, "snapshot-payload", nul_string_value(engine));

        let mut data = std::ptr::null_mut();
        let mut len = 0;
        assert_eq!(
            ferric_engine_serialize_bincode(
                engine,
                None,
                std::ptr::null_mut(),
                &mut data,
                &mut len,
            ),
            FerricError::Ok
        );
        assert!(!data.is_null());
        assert!(len > 0);

        let mut restored = std::ptr::null_mut();
        assert_eq!(
            ferric_engine_deserialize_bincode(data, len, &mut restored),
            FerricError::Ok
        );
        ferric_bytes_free(data, len);
        assert!(!restored.is_null());

        let fact_id = (&*restored)
            .engine
            .facts()
            .unwrap()
            .next()
            .expect("restored engine should contain the test fact")
            .0
            .data()
            .as_ffi();
        let mut out = FerricValue::void();
        assert_eq!(
            ferric_engine_get_fact_field(restored, fact_id, 0, &mut out),
            FerricError::InvalidArgument
        );
        assert_eq!(out.value_type, FerricValueType::Void.as_raw());

        assert_eq!(ferric_engine_free(restored), FerricError::Ok);
        assert_eq!(ferric_engine_free(engine), FerricError::Ok);
    }
}

#[test]
fn borrowed_output_rejects_embedded_nul_while_copy_is_lossless() {
    unsafe {
        let engine = ferric_engine_new();
        assert!(!engine.is_null());
        let source =
            CString::new("(defrule emit (payload ?value) => (printout shared ?value))").unwrap();
        assert_eq!(
            ferric_engine_load_string(engine, source.as_ptr()),
            FerricError::Ok
        );
        assert_eq!(ferric_engine_reset(engine), FerricError::Ok);
        assert_internal_value(engine, "payload", nul_string_value(engine));

        let mut fired = 0;
        assert_eq!(ferric_engine_run(engine, -1, &mut fired), FerricError::Ok);
        assert_eq!(fired, 1);
        assert_eq!((&*engine).engine.get_output("shared"), Some("a\0b"));

        let channel = CString::new("shared").unwrap();
        assert!(
            ferric_engine_get_output(engine, channel.as_ptr()).is_null(),
            "the legacy borrowed C-string API must reject embedded NUL"
        );
        let diagnostic = ferric_engine_last_error(engine);
        assert!(!diagnostic.is_null());
        assert!(CStr::from_ptr(diagnostic)
            .to_string_lossy()
            .contains("embedded NUL"));

        let mut needed = 0;
        assert_eq!(
            ferric_engine_get_output_copy(
                engine,
                channel.as_ptr(),
                std::ptr::null_mut(),
                0,
                &mut needed,
            ),
            FerricError::Ok
        );
        assert_eq!(
            needed, 4,
            "required size includes all bytes and the final NUL"
        );

        let mut copied = [0_u8; 4];
        assert_eq!(
            ferric_engine_get_output_copy(
                engine,
                channel.as_ptr(),
                copied.as_mut_ptr().cast(),
                copied.len(),
                &mut needed,
            ),
            FerricError::Ok
        );
        assert_eq!(needed, copied.len());
        assert_eq!(copied, [b'a', 0, b'b', 0]);

        assert_eq!(ferric_engine_free(engine), FerricError::Ok);
    }
}
