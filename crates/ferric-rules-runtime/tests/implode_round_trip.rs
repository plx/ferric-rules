//! Composition of #344 field quoting with #339 scanning and byte lexemes.
//!
//! The source fixture covers selected scannable names, strings, integers, and
//! simple floats. Host controls additionally retain invalid UTF-8 bytes without
//! NUL or scanner backspace editing. This is not a general serialization claim:
//! arbitrary symbol spellings, finite floats, and NUL data need not round-trip.

use ferric_rules_core::Value;
use ferric_rules_runtime::{Engine, EngineConfig, HaltReason, HostValue, RunLimit};

const PREFIX: &str = r"
    (defglobal ?*fields* = FALSE ?*encoded* = FALSE ?*restored* = FALSE)
";
const RULE: &str = r#"
    (defrule round-trip (payload ?text ?symbol ?name) =>
      (bind ?*fields* (create$ ?text ?symbol ?name "" "42" "[widget]"
                              9223372036854775807 1.25 -0.0))
      (bind ?*encoded* (implode$ ?*fields*))
      (bind ?*restored* (explode$ ?*encoded*))
      (printout t ?*encoded* crlf))
"#;

struct Case {
    text: &'static [u8],
    symbol: &'static [u8],
    name: &'static [u8],
    encoded: &'static [u8],
}

const CASES: &[Case] = &[
    Case {
        text: b"a\"b\\c",
        symbol: b"red",
        name: b"a?b",
        encoded: b"\"a\\\"b\\\\c\" red [a?b] \"\" \"42\" \"[widget]\" 9223372036854775807 1.25 -0.0",
    },
    Case {
        text: b"a\"\xff\\b\r\n\t",
        // A continuation byte after ASCII is scanner-admitted even though
        // it is not valid UTF-8. FF is retained inside the quoted STRING,
        // but is an UNKNOWN token outside quotes and cannot round-trip here.
        symbol: b"s\x80",
        name: b"n\x80",
        encoded: b"\"a\\\"\xff\\\\b\r\n\t\" s\x80 [n\x80] \"\" \"42\" \"[widget]\" 9223372036854775807 1.25 -0.0",
    },
];

fn before_rule(case: &Case) -> Engine {
    let mut engine = Engine::new(EngineConfig::utf8());
    engine.load_str(PREFIX).unwrap();
    engine.reset().unwrap();
    let fields: Vec<HostValue> = vec![
        engine.create_string_bytes(case.text).unwrap().into(),
        engine.symbol_value_bytes(case.symbol).unwrap(),
        engine.instance_name_value_bytes(case.name).unwrap(),
    ];
    engine.assert_ordered("payload", fields).unwrap();
    engine
}

fn pending(case: &Case) -> Engine {
    let mut engine = before_rule(case);
    engine.load_str(RULE).unwrap();
    engine
}

fn check_values(engine: &Engine, case: &Case) {
    let original = engine.get_global("fields").unwrap();
    let restored = engine.get_global("restored").unwrap();
    assert!(
        original.structural_eq(restored),
        "original={original:?}; restored={restored:?}; case={:?}",
        case.text
    );
    for value in [original, restored] {
        let Value::Multifield(fields) = value else {
            panic!("round-trip values must be actual MULTIFIELDs")
        };
        let [Value::String(text), Value::Symbol(symbol), Value::InstanceName(name), Value::String(empty), Value::String(number), Value::String(brackets), Value::Integer(integer), Value::Float(fraction), Value::Float(zero)] =
            fields.as_slice()
        else {
            panic!("field variants must survive quoting and scanning: {fields:?}")
        };
        assert_eq!(text.as_bytes(), case.text);
        assert_eq!(engine.resolve_core_symbol_bytes(*symbol), Some(case.symbol));
        assert_eq!(
            engine.resolve_core_symbol_bytes(name.as_symbol()),
            Some(case.name)
        );
        assert!(empty.as_bytes().is_empty());
        assert_eq!(number.as_bytes(), b"42");
        assert_eq!(brackets.as_bytes(), b"[widget]");
        assert_eq!(*integer, i64::MAX);
        assert_eq!(fraction.to_bits(), 1.25_f64.to_bits());
        assert_eq!(zero.to_bits(), (-0.0_f64).to_bits());
    }
    let Some(Value::String(encoded)) = engine.get_global("encoded") else {
        panic!("implode$ must return STRING")
    };
    assert_eq!(encoded.as_bytes(), case.encoded);
    if case.encoded.contains(&0xff) {
        assert!(encoded.as_str().is_err());
        assert!(engine.get_output("t").is_err());
    } else {
        assert_eq!(encoded.as_str().unwrap().as_bytes(), case.encoded);
    }
    let mut expected = case.encoded.to_vec();
    expected.push(b'\n');
    assert_eq!(engine.get_output_bytes("t"), Some(expected.as_slice()));
    assert!(engine.action_diagnostics().is_empty());
}

fn run_once(engine: &mut Engine, case: &Case) {
    let result = engine.run(RunLimit::Count(10)).unwrap();
    assert_eq!(result.rules_fired, 1);
    assert_eq!(result.halt_reason, HaltReason::AgendaEmpty);
    check_values(engine, case);
    assert_no_refiring(engine, case);
}

fn assert_no_refiring(engine: &mut Engine, case: &Case) {
    let result = engine.run(RunLimit::Count(10)).unwrap();
    assert_eq!(result.rules_fired, 0);
    assert_eq!(result.halt_reason, HaltReason::AgendaEmpty);
    check_values(engine, case);
}

#[test]
fn quoted_fields_round_trip_with_exact_variants_bytes_and_selected_float_bits() {
    for case in CASES {
        run_once(&mut pending(case), case);
    }
}

#[cfg(feature = "serde")]
#[test]
fn quoted_fields_resume_pending_and_completed_in_all_five_codecs() {
    use ferric_rules_runtime::SerializationFormat;
    for case in CASES {
        let engine = pending(case);
        for &format in SerializationFormat::ALL {
            let mut restored = Engine::deserialize(&engine.serialize(format).unwrap(), format)
                .unwrap_or_else(|error| panic!("pending {format:?}: {error:?}"));
            run_once(&mut restored, case);
            let mut completed = Engine::deserialize(&restored.serialize(format).unwrap(), format)
                .unwrap_or_else(|error| panic!("completed {format:?}: {error:?}"));
            assert_no_refiring(&mut completed, case);
        }
    }
}

#[cfg(feature = "serde")]
#[test]
fn round_trip_rule_installs_after_all_five_codec_restores_with_existing_facts() {
    use ferric_rules_runtime::SerializationFormat;
    for case in CASES {
        let engine = before_rule(case);
        for &format in SerializationFormat::ALL {
            let mut restored = Engine::deserialize(&engine.serialize(format).unwrap(), format)
                .unwrap_or_else(|error| panic!("late {format:?}: {error:?}"));
            restored.load_str(RULE).unwrap();
            run_once(&mut restored, case);
        }
    }
}

// This host-only byte transport control does not claim parity with CLIPS
// C-string inputs or scanner round trips for embedded NUL.
#[test]
fn implode_preserves_nul_bytes_without_claiming_scanner_round_trip() {
    let mut engine = Engine::with_rules(
        r"
        (defglobal ?*encoded* = FALSE)
        (defrule encode (payload ?text ?symbol ?name) =>
          (bind ?*encoded* (implode$ (create$ ?text ?symbol ?name)))
          (printout t ?*encoded* crlf))
        ",
    )
    .unwrap();
    let fields: Vec<HostValue> = vec![
        engine.create_string_bytes(b"a\0\"\xff\\z").unwrap().into(),
        engine.symbol_value_bytes(b"s\0\xff").unwrap(),
        engine.instance_name_value_bytes(b"n\0\xff").unwrap(),
    ];
    engine.assert_ordered("payload", fields).unwrap();
    let result = engine.run(RunLimit::Count(10)).unwrap();
    assert_eq!(result.rules_fired, 1);
    assert_eq!(result.halt_reason, HaltReason::AgendaEmpty);
    let Some(Value::String(encoded)) = engine.get_global("encoded") else {
        panic!("imploded raw bytes must be STRING")
    };
    assert_eq!(
        encoded.as_bytes(),
        b"\"a\0\\\"\xff\\\\z\" s\0\xff [n\0\xff]"
    );
    assert!(encoded.as_str().is_err());
    assert_eq!(
        engine.get_output_bytes("t"),
        Some(b"\"a\0\\\"\xff\\\\z\" s\0\xff [n\0\xff]\n".as_slice())
    );
    assert!(engine.get_output("t").is_err());
    assert!(engine.action_diagnostics().is_empty());
}
