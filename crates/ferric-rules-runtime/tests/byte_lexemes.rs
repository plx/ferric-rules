//! Exact byte and typed-name transport through public runtime boundaries.

use ferric_rules_core::{FerricString, Value};
use ferric_rules_runtime::{Engine, EngineConfig, EngineError, HaltReason, HostValue, RunLimit};

fn payload(engine: &mut Engine) -> Vec<HostValue> {
    vec![
        engine.create_string_bytes(b"a\0\xffz").unwrap().into(),
        engine.symbol_value_bytes(b"s\xff").unwrap(),
        engine.instance_name_value_bytes(b"n\xff").unwrap(),
    ]
}

const ECHO: &str = r#"
    (defglobal ?*result* = FALSE)
    (deffunction echo (?value) (printout t ?value))
    (defrule capture (payload ?text ?symbol ?name) =>
      (bind ?*result* (create$ ?text ?symbol ?name))
      (printout t ?text "|" ?symbol "|" ?name crlf)
      (echo ?text))
"#;

fn verify_payload(engine: &Engine) {
    let Some(Value::Multifield(fields)) = engine.get_global("result") else {
        panic!("captured result must be a multifield")
    };
    let [Value::String(text), Value::Symbol(symbol), Value::InstanceName(name)] = fields.as_slice()
    else {
        panic!("STRING, SYMBOL, and INSTANCE-NAME must retain their variants")
    };
    assert_eq!(text.as_bytes(), b"a\0\xffz");
    assert!(text.as_str().is_err());
    assert_eq!(
        engine.resolve_core_symbol_bytes(*symbol),
        Some(b"s\xff".as_slice())
    );
    assert_eq!(
        engine.resolve_core_symbol_bytes(name.as_symbol()),
        Some(b"n\xff".as_slice())
    );
}

#[test]
fn raw_output_and_callable_events_preserve_exact_bytes_and_checked_text() {
    let mut engine = Engine::with_rules(ECHO).unwrap();
    let fields = payload(&mut engine);
    engine.assert_ordered("payload", fields).unwrap();
    assert_eq!(engine.run(RunLimit::Unlimited).unwrap().rules_fired, 1);
    verify_payload(&engine);
    assert_eq!(
        engine.get_output_bytes("t"),
        Some(b"a\0\xffz|s\xff|[n\xff]\na\0\xffz".as_slice())
    );
    assert!(engine.get_output("t").is_err());
    assert_eq!(engine.get_output("absent").unwrap(), None);
    engine.clear_output_channel("t");
    assert_eq!(engine.get_output_bytes("t"), None);
}

#[test]
fn instance_name_handles_preserve_owner_and_distinct_value_identity() {
    let mut engine = Engine::new(EngineConfig::utf8());
    let name = engine.intern_instance_name_bytes(b"n\xff").unwrap();
    let symbol = engine.intern_symbol_bytes(b"n\xff").unwrap();
    assert!(engine.resolve_instance_name(name).is_err());
    assert_eq!(
        engine.resolve_instance_name_bytes(name),
        Some(b"n\xff".as_slice())
    );
    assert_eq!(
        engine.resolve_symbol_bytes(symbol),
        Some(b"n\xff".as_slice())
    );
    let owned_name = HostValue::from(name);
    let owned_symbol = HostValue::from(symbol);
    assert!(!owned_name.as_value().structural_eq(owned_symbol.as_value()));
    engine
        .assert_ordered("entry", [owned_name.clone()])
        .unwrap();
    engine.assert_ordered("entry", [owned_symbol]).unwrap();
    assert_eq!(engine.find_facts("entry").unwrap().len(), 2);
    let mut other = Engine::new(EngineConfig::utf8());
    assert!(matches!(
        other.assert_ordered("entry", [owned_name.clone()]),
        Err(EngineError::ForeignHandle)
    ));
    assert!(matches!(
        engine.assert_ordered("raw", [owned_name.as_value().clone()]),
        Err(EngineError::InvalidHostValue(_))
    ));
    engine.reset().unwrap();
    engine.assert_ordered("entry", [name]).unwrap();
    engine.clear();
    assert_eq!(engine.resolve_instance_name_bytes(name), None);
    assert!(matches!(
        engine.assert_ordered("entry", [name]),
        Err(EngineError::ForeignHandle)
    ));
}

#[test]
fn strict_encoding_and_nested_ownership_are_enforced() {
    let mut strict = Engine::new(EngineConfig::ascii());
    assert!(strict.create_string_bytes(b"\xff").is_err());
    assert!(strict.symbol_value_bytes(b"\xff").is_err());
    assert!(strict.instance_name_value_bytes(b"\xff").is_err());
    let mut mixed = Engine::new(EngineConfig::ascii_symbols_utf8_strings());
    assert!(mixed.create_string_bytes(b"\xff").is_ok());
    assert!(mixed.symbol_value_bytes(b"\xff").is_err());
    assert!(mixed.instance_name_value_bytes(b"\xff").is_err());
    let bytes =
        FerricString::from_bytes(b"\xff", ferric_rules_runtime::StringEncoding::Utf8).unwrap();
    assert!(strict.assert_ordered("entry", [bytes]).is_err());
    let mut left = Engine::new(EngineConfig::utf8());
    let mut right = Engine::new(EngineConfig::utf8());
    assert!(HostValue::multifield(vec![
        left.instance_name_value("x").unwrap(),
        right.instance_name_value("x").unwrap()
    ])
    .is_err());
    let nested =
        HostValue::multifield(vec![left.instance_name_value_bytes(b"\xff").unwrap()]).unwrap();
    left.assert_ordered("nested", [nested.clone()]).unwrap();
    assert!(matches!(
        right.assert_ordered("nested", [nested]),
        Err(EngineError::ForeignHandle)
    ));
}

#[test]
fn byte_concat_format_and_case_conversion_never_replace_payloads() {
    let mut engine = Engine::with_rules(r#"
        (defglobal ?*string* = FALSE ?*symbol* = FALSE ?*format* = FALSE ?*upper* = FALSE ?*lower* = FALSE)
        (defrule transform (payload ?text ?symbol ?name) =>
          (bind ?*string* (str-cat ?text ?symbol ?name))
          (bind ?*symbol* (sym-cat ?text ?symbol ?name))
          (bind ?*format* (format nil "%s|%s|%s" ?text ?symbol ?name))
          (bind ?*upper* (upcase ?text))
          (bind ?*lower* (lowcase ?symbol)))
    "#).unwrap();
    let fields = payload(&mut engine);
    engine.assert_ordered("payload", fields).unwrap();
    assert_eq!(
        engine.run(RunLimit::Unlimited).unwrap().halt_reason,
        HaltReason::AgendaEmpty
    );
    for (global, expected) in [
        ("string", b"a\0\xffzs\xffn\xff".as_slice()),
        ("format", b"a\0\xffz|s\xff|n\xff".as_slice()),
        ("upper", b"A\0\xffZ".as_slice()),
    ] {
        let Some(Value::String(value)) = engine.get_global(global) else {
            panic!("{global} must remain STRING")
        };
        assert_eq!(value.as_bytes(), expected);
    }
    for (global, expected) in [
        ("symbol", b"a\0\xffzs\xffn\xff".as_slice()),
        ("lower", b"s\xff".as_slice()),
    ] {
        let Some(Value::Symbol(value)) = engine.get_global(global) else {
            panic!("{global} must remain SYMBOL")
        };
        assert_eq!(engine.resolve_core_symbol_bytes(*value), Some(expected));
    }
}

#[test]
fn instance_literals_match_typed_slots_and_predicates() {
    let mut engine = Engine::with_rules(
        r#"
        (deftemplate named (slot value (type INSTANCE-NAME)))
        (deffacts seed (named (value [widget])) (literal [widget]))
        (defrule check (named (value ?name)) (literal [widget]) =>
          (printout t (instance-namep ?name) ":" (instancep ?name) ":"
            (symbolp ?name) ":" (stringp ?name) ":" (lexemep ?name) ":"
            (eq ?name widget) ":" (eq ?name (symbol-to-instance-name widget)) ":"
            (instance-name-to-symbol ?name) crlf))
    "#,
    )
    .unwrap();
    assert_eq!(engine.run(RunLimit::Unlimited).unwrap().rules_fired, 1);
    assert_eq!(
        engine.get_output("t").unwrap(),
        Some("TRUE:TRUE:FALSE:FALSE:FALSE:FALSE:TRUE:widget\n")
    );
    let wrong_type = engine.symbol_value("widget").unwrap();
    assert!(engine
        .assert_template("named", &["value"], [wrong_type])
        .is_err());
}

#[test]
fn text_identifiers_reject_invalid_utf8_explicitly() {
    let mut engine =
        Engine::with_rules("(defrule check (input ?value) => (funcall ?value) (assert (after)))")
            .unwrap();
    let raw = engine.create_string_bytes(b"\xff").unwrap();
    engine.assert_ordered("input", [raw]).unwrap();
    assert_eq!(
        engine.run(RunLimit::Unlimited).unwrap().halt_reason,
        HaltReason::ActionError,
    );
    assert!(engine
        .action_diagnostics()
        .iter()
        .any(|error| error.to_string().contains("UTF-8")));
    assert!(engine.find_facts("after").unwrap().is_empty());
}

#[test]
fn raw_formats_and_byte_position_operations_preserve_non_text_data() {
    let mut engine = Engine::with_rules(r"
        (defglobal ?*formatted* = FALSE ?*length* = 0 ?*part* = FALSE ?*index* = FALSE ?*comparison* = 0)
        (defrule bytes (input ?format ?text ?needle) =>
          (bind ?*formatted* (format nil ?format ?text))
          (bind ?*length* (str-length ?text))
          (bind ?*part* (sub-string 2 3 ?text))
          (bind ?*index* (str-index ?needle ?text))
          (bind ?*comparison* (str-compare ?text ?text)))
    ").unwrap();
    let format = engine.create_string_bytes(b"\xfe[%s]\0").unwrap();
    let text = engine.create_string_bytes(b"a\0\xffz").unwrap();
    let needle = engine.create_string_bytes(b"\xff").unwrap();
    engine
        .assert_ordered("input", [format, text, needle])
        .unwrap();
    assert_eq!(
        engine.run(RunLimit::Unlimited).unwrap().halt_reason,
        HaltReason::AgendaEmpty
    );
    for (global, expected) in [
        ("formatted", b"\xfe[a\0\xffz]\0".as_slice()),
        ("part", b"\0\xff".as_slice()),
    ] {
        let Some(Value::String(value)) = engine.get_global(global) else {
            panic!("{global} STRING")
        };
        assert_eq!(value.as_bytes(), expected);
    }
    assert!(matches!(
        engine.get_global("length"),
        Some(Value::Integer(4))
    ));
    assert!(matches!(
        engine.get_global("index"),
        Some(Value::Integer(3))
    ));
    assert!(matches!(
        engine.get_global("comparison"),
        Some(Value::Integer(0))
    ));
}

#[test]
fn case_conversion_preserves_instance_name_type_and_unicode_text_behavior() {
    let mut engine = Engine::with_rules(
        r#"
        (defglobal ?*upper* = FALSE ?*lower* = FALSE ?*text* = FALSE)
        (defrule check =>
          (bind ?*upper* (upcase [mixed]))
          (bind ?*lower* (lowcase [MIXED]))
          (bind ?*text* (format nil "é:%s" (upcase "élan"))))
    "#,
    )
    .unwrap();
    assert_eq!(engine.run(RunLimit::Unlimited).unwrap().rules_fired, 1);
    for (global, expected) in [
        ("upper", b"MIXED".as_slice()),
        ("lower", b"mixed".as_slice()),
    ] {
        let Some(Value::InstanceName(value)) = engine.get_global(global) else {
            panic!("{global} INSTANCE-NAME")
        };
        assert_eq!(
            engine.resolve_core_symbol_bytes(value.as_symbol()),
            Some(expected)
        );
    }
    let Some(Value::String(value)) = engine.get_global("text") else {
        panic!("STRING")
    };
    assert_eq!(value.as_str().unwrap(), "é:ÉLAN");
}

#[test]
fn conversions_are_idempotent_and_preserve_raw_name_bytes() {
    let mut engine = Engine::with_rules(
        r"
        (defglobal ?*result* = FALSE)
        (defrule convert (input ?name ?symbol) =>
          (bind ?*result* (create$
            (symbol-to-instance-name ?name) (symbol-to-instance-name ?symbol)
            (instance-name-to-symbol ?name) (instance-name-to-symbol ?symbol))))
    ",
    )
    .unwrap();
    let name = engine.instance_name_value_bytes(b"n\xff").unwrap();
    let symbol = engine.symbol_value_bytes(b"n\xff").unwrap();
    engine.assert_ordered("input", [name, symbol]).unwrap();
    assert_eq!(
        engine.run(RunLimit::Unlimited).unwrap().halt_reason,
        HaltReason::AgendaEmpty
    );
    let Some(Value::Multifield(fields)) = engine.get_global("result") else {
        panic!("conversion results")
    };
    let [Value::InstanceName(first), Value::InstanceName(second), Value::Symbol(third), Value::Symbol(fourth)] =
        fields.as_slice()
    else {
        panic!("two names and two symbols")
    };
    for symbol in [first.as_symbol(), second.as_symbol(), *third, *fourth] {
        assert_eq!(
            engine.resolve_core_symbol_bytes(symbol),
            Some(b"n\xff".as_slice())
        );
    }
}

#[test]
fn conversions_observe_error_without_clearing_sticky_halt() {
    // inscom.c1165/1187 + argacces.c405/429: conversion checks Error and
    // accepts both name tags. The 343 recovery contract clears Error on a
    // later skipped user callback, while keeping Halt for the action boundary.
    for function in ["symbol-to-instance-name", "instance-name-to-symbol"] {
        for fields in ["a b", "a b c"] {
            let source = format!(
                r"
                (defglobal ?*result* = pending)
                (deffunction fail (?left ?right) (/ 1 0))
                (deffacts seed (request))
                (defrule first (declare (salience 10)) (request) =>
                  (bind ?*result* ({function} (nth$ 1 (sort fail {fields}))))
                  (assert (after)))
                (defrule later (request) => (assert (continued)))
            "
            );
            let mut engine = Engine::with_rules(&source).unwrap();
            let run = engine.run(RunLimit::Unlimited).unwrap();
            assert_eq!(
                run.halt_reason,
                HaltReason::ActionError,
                "{function} {fields}"
            );
            assert_eq!(run.rules_fired, 1);
            assert_eq!(engine.action_diagnostics().len(), 1);
            assert!(engine.find_facts("after").unwrap().is_empty());
            let value = engine.get_global("result").unwrap();
            if fields == "a b" {
                let Value::Symbol(symbol) = value else {
                    panic!("Error conversion returns FALSE")
                };
                assert_eq!(engine.resolve_core_symbol(*symbol), Some("FALSE"));
            } else {
                let symbol = match (function, value) {
                    ("symbol-to-instance-name", Value::InstanceName(name)) => name.as_symbol(),
                    ("instance-name-to-symbol", Value::Symbol(symbol)) => *symbol,
                    _ => panic!("Halt without Error preserves conversion value"),
                };
                assert_eq!(engine.resolve_core_symbol(symbol), Some("a"));
            }
            assert_eq!(engine.run(RunLimit::Unlimited).unwrap().rules_fired, 1);
            assert!(engine.action_diagnostics().is_empty());
            assert_eq!(engine.find_facts("continued").unwrap().len(), 1);
        }
    }
}

#[test]
fn type_and_restricted_generics_report_missing_instances_after_assigning_false() {
    for expression in [
        "(type [widget])",
        "(classify [widget])",
        "(type (printout t marker))",
    ] {
        let source = format!(
            r"
            (defglobal ?*result* = pending)
            (defgeneric classify)
            (defmethod classify ((?value INSTANCE-NAME)) (printout t method-body))
            (deffacts requests (request))
            (defrule first (declare (salience 10)) (request) =>
              (bind ?*result* {expression}) (assert (after)))
            (defrule later (request) => (assert (continued)))
        "
        );
        let mut engine = Engine::with_rules(&source).unwrap();
        assert_eq!(
            engine.run(RunLimit::Unlimited).unwrap().halt_reason,
            HaltReason::ActionError,
            "{expression}"
        );
        let Some(Value::Symbol(value)) = engine.get_global("result") else {
            panic!("actual FALSE")
        };
        assert_eq!(engine.resolve_core_symbol(*value), Some("FALSE"));
        assert_eq!(engine.action_diagnostics().len(), 1);
        assert!(engine.find_facts("after").unwrap().is_empty());
        assert!(engine.find_facts("continued").unwrap().is_empty());
        assert_eq!(
            engine.get_output("t").unwrap(),
            if expression.contains("printout") {
                Some("marker")
            } else {
                None
            }
        );
        assert_eq!(engine.run(RunLimit::Unlimited).unwrap().rules_fired, 1);
    }
}

#[test]
fn generic_applicability_skips_unreached_name_lookups_and_nested_fields() {
    for source in [
        // Wildcard parameters have no type restriction and accept name values.
        "(defgeneric f) (defmethod f ($?values) accepted) (defrule run => (printout t (f [widget]) crlf))",
        // Arity fails before checking the name restriction.
        "(defgeneric f) (defmethod f ((?value INSTANCE-NAME) (?other INTEGER)) wrong) (defmethod f ($?values) accepted) (defrule run => (printout t (f [widget]) crlf))",
        // Earlier restriction fails before the later name can be looked up.
        "(defgeneric f) (defmethod f ((?first INTEGER) (?name INSTANCE-NAME)) wrong) (defmethod f ($?values) accepted) (defrule run => (printout t (f text [widget]) crlf))",
        // A multifield is classified as a container, not as its nested names.
        "(defgeneric f) (defmethod f ((?values MULTIFIELD)) accepted) (defrule run => (printout t (f (create$ [widget])) crlf))",
    ] {
        let mut engine = Engine::with_rules(source).unwrap();
        assert_eq!(engine.run(RunLimit::Unlimited).unwrap().halt_reason, HaltReason::AgendaEmpty, "{source}");
        assert_eq!(engine.get_output("t").unwrap(), Some("accepted\n"));
        assert!(engine.action_diagnostics().is_empty());
    }
}

#[test]
fn noncanonical_public_byte_variants_are_rejected_recursively() {
    let mut engine = Engine::new(EngineConfig::utf8());
    let bad = FerricString::Bytes(b"valid text".as_slice().into());
    assert!(matches!(
        engine.assert_ordered("bad", [bad.clone()]),
        Err(EngineError::InvalidHostValue(_))
    ));
    assert!(HostValue::multifield(vec![bad.into()]).is_err());
    let nested = Value::Multifield(Box::new(
        [Value::String(FerricString::Bytes(
            b"text".as_slice().into(),
        ))]
        .into_iter()
        .collect(),
    ));
    assert!(matches!(
        engine.assert_ordered("bad", [nested]),
        Err(EngineError::InvalidHostValue(_))
    ));
}

#[test]
fn call_next_reports_failed_lookup_but_skips_search_after_an_earlier_halt() {
    for (body, expected_diagnostics) in [
        ("(call-next-method)", 2),
        ("(sort str-cat (sort fail a b) (call-next-method))", 1),
    ] {
        let source = format!(
            r"
            (defglobal ?*result* = pending)
            (deffunction fail (?left ?right) (/ 1 0))
            (defgeneric f)
            (defmethod f ((?first INTEGER) $?tail) {body})
            (defmethod f ((?first NUMBER) (?name INTEGER)) (printout t lower-body))
            (defrule run => (bind ?*result* (f 1 [widget])) (assert (after)))
        "
        );
        let mut engine = Engine::with_rules(&source).unwrap();
        assert_eq!(
            engine.run(RunLimit::Unlimited).unwrap().halt_reason,
            HaltReason::ActionError,
            "{body}"
        );
        let Some(Value::Symbol(result)) = engine.get_global("result") else {
            panic!("FALSE result")
        };
        assert_eq!(engine.resolve_core_symbol(*result), Some("FALSE"));
        assert_eq!(
            engine.action_diagnostics().len(),
            expected_diagnostics,
            "{body}: {:?}",
            engine.action_diagnostics()
        );
        let messages = engine
            .action_diagnostics()
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join("\n");
        assert_eq!(messages.contains("instance"), expected_diagnostics == 2);
        assert_eq!(
            messages.contains("no next method"),
            expected_diagnostics == 2
        );
        assert_eq!(engine.get_output("t").unwrap(), None);
        assert!(engine.find_facts("after").unwrap().is_empty());
    }
}

#[cfg(feature = "serde")]
#[test]
fn every_codec_preserves_pending_completed_and_late_byte_values() {
    use ferric_rules_runtime::SerializationFormat;
    for &format in SerializationFormat::ALL {
        for late in [false, true] {
            let mut engine = Engine::new(EngineConfig::utf8());
            if !late {
                engine.load_str(ECHO).unwrap();
            }
            let fields = payload(&mut engine);
            engine.assert_ordered("payload", fields).unwrap();
            let previous_owner = engine.instance_name_value("old-owner").unwrap();
            let bytes = engine.serialize(format).unwrap();
            let mut restored = Engine::deserialize(&bytes, format).unwrap();
            assert!(matches!(
                restored.assert_ordered("old", [previous_owner]),
                Err(EngineError::ForeignHandle)
            ));
            if late {
                restored.load_str(ECHO).unwrap();
            }
            assert_eq!(
                restored.run(RunLimit::Unlimited).unwrap().rules_fired,
                1,
                "{format:?} late={late}"
            );
            verify_payload(&restored);
            let output = restored.get_output_bytes("t").unwrap().to_vec();
            let completed = restored.serialize(format).unwrap();
            let mut restored = Engine::deserialize(&completed, format).unwrap();
            verify_payload(&restored);
            assert_eq!(restored.get_output_bytes("t"), Some(output.as_slice()));
            assert_eq!(restored.run(RunLimit::Unlimited).unwrap().rules_fired, 0);
            let fact = restored.find_facts("payload").unwrap()[0].0;
            restored.retract(fact).unwrap();
            let fields = payload(&mut restored);
            restored.assert_ordered("payload", fields).unwrap();
            assert_eq!(restored.run(RunLimit::Unlimited).unwrap().rules_fired, 1);
            restored.reset().unwrap();
            assert!(restored.find_facts("payload").unwrap().is_empty());
        }
    }
}

fn assert_byte_str_index(needle: &[u8], haystack: &[u8], expected: Option<i64>) {
    for kind in ["STRING", "SYMBOL", "INSTANCE-NAME"] {
        let mut engine = Engine::with_rules(
            r"
            (defglobal ?*result* = pending)
            (defrule locate (input ?needle ?haystack) =>
              (bind ?*result* (str-index ?needle ?haystack))
              (assert (after)))
            ",
        )
        .unwrap();
        let mut fields = Vec::new();
        for bytes in [needle, haystack] {
            fields.push(match kind {
                "STRING" => engine.create_string_bytes(bytes).unwrap().into(),
                "SYMBOL" => engine.symbol_value_bytes(bytes).unwrap(),
                "INSTANCE-NAME" => engine.instance_name_value_bytes(bytes).unwrap(),
                _ => unreachable!("known lexeme kind"),
            });
        }
        engine.assert_ordered("input", fields).unwrap();
        let run = engine.run(RunLimit::Unlimited).unwrap();
        assert_eq!(run.rules_fired, 1);
        assert_eq!(run.halt_reason, HaltReason::AgendaEmpty);
        assert!(engine.action_diagnostics().is_empty());
        assert_eq!(engine.find_facts("after").unwrap().len(), 1);
        let actual = engine.get_global("result").unwrap();
        if let Some(position) = expected {
            assert!(
                matches!(actual, Value::Integer(value) if *value == position),
                "{kind}: needle={needle:?}, haystack={haystack:?}, expected={position}, actual={actual:?}"
            );
        } else {
            let Value::Symbol(symbol) = actual else {
                panic!("missing match must be actual FALSE, got {actual:?}")
            };
            assert_eq!(engine.resolve_core_symbol(*symbol), Some("FALSE"));
        }
    }
}

#[test]
fn str_index_preserves_character_positions_for_complete_utf8_operands() {
    for (needle, haystack, expected) in [
        ("z", "éz", 2),
        ("β", "éβ", 2),
        ("z", "ééz", 3),
        ("\0z", "é\0z", 2),
    ] {
        assert_byte_str_index(needle.as_bytes(), haystack.as_bytes(), Some(expected));
    }
}

#[test]
fn str_index_uses_byte_positions_when_either_complete_operand_is_raw() {
    for (needle, haystack, expected) in [
        // A valid prefix before FF does not make the whole haystack text.
        (b"\xff".as_slice(), b"\xc3\xa9\xff".as_slice(), 3),
        // The invalid byte can also occur after the matched field.
        (b"z".as_slice(), b"\xc3\xa9z\xff".as_slice(), 3),
        // A raw needle can start on a boundary, then end mid-codepoint.
        (b"a\xc3".as_slice(), b"\xc3\xa9a\xc3\xa9".as_slice(), 3),
        // It can also start mid-codepoint; byte search must not slice a str.
        (b"\xa9".as_slice(), b"\xc3\xa9".as_slice(), 2),
        // A multibyte text needle still matches at the beginning of raw data.
        (b"\xc3\xa9".as_slice(), b"\xc3\xa9\xff".as_slice(), 1),
    ] {
        assert_byte_str_index(needle, haystack, Some(expected));
    }
}

#[test]
fn str_index_retains_empty_and_missing_results_in_both_position_modes() {
    for (needle, haystack, expected) in [
        (b"\xff".as_slice(), b"\xc3\xa9".as_slice(), None),
        (b"z".as_slice(), b"\xc3\xa9\xff".as_slice(), None),
        (b"".as_slice(), b"".as_slice(), Some(1)),
        (b"\xff".as_slice(), b"".as_slice(), None),
    ] {
        // The empty-haystack result is shared with the separate #337 repair.
        assert_byte_str_index(needle, haystack, expected);
    }
}
