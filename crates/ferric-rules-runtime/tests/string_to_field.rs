//! Typed first-field parsing and evaluator-control regressions for #338.
//! Expected values are from sealed CLIPS 6.30 probes or cited wrapper source.
//! These tests do not exercise explode$ or read.

use ferric_rules_core::Value;
use ferric_rules_runtime::{Engine, EngineConfig, HaltReason, HostValue, RunLimit};

const SOURCE: &str = r"
    (defglobal ?*result* = pending ?*trace* = 0)
    (deffunction mark (?value)
      (bind ?*trace* (+ ?*trace* 1)) ?value)
    (defrule scan (input ?value) =>
      (bind ?*result* (string-to-field (mark ?value)))
      (assert (after)))
    (defrule follower (after) => (assert (followed)))
";

fn configured() -> Engine {
    Engine::with_rules(SOURCE).unwrap()
}

fn run_success(engine: &mut Engine) {
    let result = engine.run(RunLimit::Unlimited).unwrap();
    assert_eq!(result.halt_reason, HaltReason::AgendaEmpty);
    assert_eq!(result.rules_fired, 2);
    assert!(matches!(
        engine.get_global("trace"),
        Some(Value::Integer(1))
    ));
    assert_eq!(engine.find_facts("followed").unwrap().len(), 1);
}

fn string_result(engine: &Engine, expected: &[u8]) {
    let Some(Value::String(value)) = engine.get_global("result") else {
        panic!("result must be actual STRING")
    };
    assert_eq!(value.as_bytes(), expected);
}

fn symbol_result(engine: &Engine, expected: &[u8]) {
    let Some(Value::Symbol(value)) = engine.get_global("result") else {
        panic!("result must be actual SYMBOL")
    };
    assert_eq!(engine.resolve_core_symbol_bytes(*value), Some(expected));
}

fn run_string(bytes: &[u8]) -> Engine {
    let mut engine = configured();
    let input = engine.create_string_bytes(bytes).unwrap();
    engine.assert_ordered("input", [input]).unwrap();
    run_success(&mut engine);
    engine
}

#[test]
fn accepts_string_symbol_and_instance_name_payloads_once() {
    // StringToFieldFunction -> EnvArgTypeCheck(SYMBOL_OR_STRING) admits all
    // three lexical values dynamically. Input names do not require a COOL object.
    for kind in ["string", "symbol", "name"] {
        let mut engine = configured();
        let input: HostValue = match kind {
            "string" => engine.create_string_bytes(b"42 trailing").unwrap().into(),
            "symbol" => engine.symbol_value_bytes(b"42 trailing").unwrap(),
            _ => engine.instance_name_value_bytes(b"42 trailing").unwrap(),
        };
        engine.assert_ordered("input", [input]).unwrap();
        run_success(&mut engine);
        assert!(matches!(
            engine.get_global("result"),
            Some(Value::Integer(42))
        ));
        assert!(engine.action_diagnostics().is_empty(), "{kind}");
    }
}

#[test]
fn complete_numeric_grammar_preserves_result_types_and_signed_zero() {
    for (bytes, expected) in [
        (b"42 trailing".as_slice(), 42),
        (b"+001 next".as_slice(), 1),
        (b"-9223372036854775808".as_slice(), i64::MIN),
        (b"9223372036854775807".as_slice(), i64::MAX),
    ] {
        let engine = run_string(bytes);
        assert!(
            matches!(engine.get_global("result"), Some(Value::Integer(value)) if *value == expected)
        );
        assert!(engine.action_diagnostics().is_empty());
    }
    for (bytes, expected) in [
        (b".5 tail".as_slice(), 0.5_f64),
        (b"-.5".as_slice(), -0.5),
        (b"1.".as_slice(), 1.0),
        (b"2e3".as_slice(), 2000.0),
        (b"-0.0".as_slice(), -0.0),
    ] {
        let engine = run_string(bytes);
        let Some(Value::Float(value)) = engine.get_global("result") else {
            panic!("FLOAT for {bytes:?}")
        };
        assert_eq!(value.to_bits(), expected.to_bits());
        assert!(engine.action_diagnostics().is_empty());
    }
    for text in [
        "42abc", "1.abc", "1e", "1e+", "1.2.3", "+", "-", "NaN", "inf", "0x10",
    ] {
        let engine = run_string(text.as_bytes());
        symbol_result(&engine, text.as_bytes());
        assert!(engine.action_diagnostics().is_empty());
    }
}

#[test]
fn first_token_never_scans_malformed_or_overflowing_suffixes() {
    for bytes in [
        b"42 \"unterminated".as_slice(),
        b"42 99999999999999999999999999999999999999999".as_slice(),
        b"42 ?missing ( unmatched".as_slice(),
    ] {
        let engine = run_string(bytes);
        assert!(matches!(
            engine.get_global("result"),
            Some(Value::Integer(42))
        ));
        assert!(engine.action_diagnostics().is_empty());
        assert_eq!(engine.get_output_bytes("werror"), None);
        assert_eq!(engine.get_output_bytes("wwarning"), None);
    }
    let engine = run_string(b"\"first token\" \"unfinished\\");
    string_result(&engine, b"first token");
    assert!(engine.action_diagnostics().is_empty());
}

#[test]
fn nonvalue_tokens_use_print_forms_while_unknown_is_a_silent_error_string() {
    for text in ["(", ")", "?x", "$?x", "?*x*", "?", "$?", "&", "|", "~"] {
        let engine = run_string(text.as_bytes());
        string_result(&engine, text.as_bytes());
        assert!(engine.action_diagnostics().is_empty());
    }
    for text in ["=", "<-", ":"] {
        symbol_result(&run_string(text.as_bytes()), text.as_bytes());
    }
    for bytes in [b"\x0b trailing".as_slice(), b"\xff trailing".as_slice()] {
        let engine = run_string(bytes);
        string_result(&engine, b"*** ERROR ***");
        assert!(engine.action_diagnostics().is_empty());
        assert_eq!(engine.get_output_bytes("werror"), None);
    }
}

#[test]
fn string_source_eof_comments_nul_and_non_ascii_bytes_are_distinct() {
    // OpenStringSource uses first-NUL termination. This is the scanner source
    // boundary, not a change to byte values or printout storage.
    for bytes in [
        b"".as_slice(),
        b" \t\r\n\x0c; comment".as_slice(),
        b"\0not-scanned".as_slice(),
        b"\x03not-scanned".as_slice(),
    ] {
        let engine = run_string(bytes);
        symbol_result(&engine, b"EOF");
        assert!(engine.action_diagnostics().is_empty());
    }
    assert!(matches!(
        run_string(b"42\0broken").get_global("result"),
        Some(Value::Integer(42))
    ));
    symbol_result(&run_string(b"a\xc3 tail"), b"a\xc3");
    symbol_result(&run_string(b"\xc3 tail"), b"\xc3");
    symbol_result(
        &run_string("\u{a0}word tail".as_bytes()),
        "\u{a0}word".as_bytes(),
    );
    let engine = run_string(b"[n\xc3] trailing");
    let Some(Value::InstanceName(name)) = engine.get_global("result") else {
        panic!("scanner must return INSTANCE-NAME")
    };
    assert_eq!(
        engine.resolve_core_symbol_bytes(name.as_symbol()),
        Some(b"n\xc3".as_slice())
    );
}

#[test]
fn quoted_bytes_use_field_escapes_and_keep_partial_warning_values() {
    for (input, expected) in [
        (b"\"\"".as_slice(), b"".as_slice()),
        (b"\"a b\" ignored".as_slice(), b"a b".as_slice()),
        (b"\"a\\nb\\q\\\"\\\\\"".as_slice(), b"anbq\"\\".as_slice()),
        (b"\"a\xff\xc3\"".as_slice(), b"a\xff\xc3".as_slice()),
    ] {
        let engine = run_string(input);
        string_result(&engine, expected);
        assert!(engine.action_diagnostics().is_empty());
    }
    for (input, expected) in [
        (b"\"unfinished".as_slice(), b"unfinished".as_slice()),
        (b"\"unfinished\\".as_slice(), b"unfinished\xff".as_slice()),
    ] {
        let engine = run_string(input);
        string_result(&engine, expected);
        assert_eq!(engine.action_diagnostics().len(), 1);
        assert!(engine.action_diagnostics()[0]
            .to_string()
            .contains("SCANNER1"));
        assert_eq!(
            engine.get_output_bytes("werror"),
            Some(b"\n[SCANNER1] Encountered End-Of-File while scanning a string\n".as_slice())
        );
    }
}

#[test]
fn integer_overflow_warns_without_halting_following_work() {
    for (text, expected) in [
        ("9223372036854775808", i64::MAX),
        ("-9223372036854775809", i64::MIN),
    ] {
        let engine = run_string(text.as_bytes());
        assert!(
            matches!(engine.get_global("result"), Some(Value::Integer(value)) if *value == expected)
        );
        assert_eq!(engine.action_diagnostics().len(), 1);
        assert!(engine.action_diagnostics()[0]
            .to_string()
            .contains("SCANNER1"));
        assert_eq!(
            engine.get_output_bytes("wwarning"),
            Some(b"[SCANNER1] WARNING: Over or underflow of long long integer.\n".as_slice())
        );
    }
}

#[test]
fn invalid_or_throwing_argument_assigns_error_string_then_stops() {
    for (expression, expected_message) in [
        ("(mark 12)", "string-to-field"),
        ("(mark (create$ 1 2))", "string-to-field"),
        ("(/ (mark 1) 0)", "division by zero"),
    ] {
        let mut engine = Engine::with_rules(&format!(
            r"
            (defglobal ?*result* = pending ?*trace* = 0)
            (deffunction mark (?value) (bind ?*trace* (+ ?*trace* 1)) ?value)
            (defrule run => (bind ?*result* (string-to-field {expression})) (assert (after)))
        "
        ))
        .unwrap();
        assert_eq!(
            engine.run(RunLimit::Unlimited).unwrap().halt_reason,
            HaltReason::ActionError
        );
        string_result(&engine, b"*** ERROR ***");
        assert!(matches!(
            engine.get_global("trace"),
            Some(Value::Integer(1))
        ));
        assert_eq!(engine.action_diagnostics().len(), 1);
        assert!(engine.action_diagnostics()[0]
            .to_string()
            .contains(expected_message));
        assert!(engine.find_facts("after").unwrap().is_empty());
    }
}

#[test]
fn notices_drain_at_load_and_match_boundaries() {
    let mut engine = Engine::new(EngineConfig::utf8());
    let loaded = engine
        .load_str(r#"(defglobal ?*result* = (string-to-field "9223372036854775808"))"#)
        .unwrap();
    assert_eq!(loaded.warnings.len(), 1);
    assert_eq!(engine.action_diagnostics().len(), 1);
    assert_eq!(
        engine.get_output_bytes("wwarning"),
        Some(b"[SCANNER1] WARNING: Over or underflow of long long integer.\n".as_slice())
    );
    assert!(matches!(
        engine.get_global("result"),
        Some(Value::Integer(i64::MAX))
    ));
    let mut matched = Engine::with_rules(
        r"
        (defrule match (input ?text) (test (= (string-to-field ?text) 9223372036854775807))
          => (assert (after)))
    ",
    )
    .unwrap();
    let value = matched.create_string_bytes(b"9223372036854775808").unwrap();
    matched.assert_ordered("input", [value]).unwrap();
    assert_eq!(matched.agenda_len(), 1);
    assert_eq!(matched.action_diagnostics().len(), 1);
    assert!(matched.action_diagnostics()[0]
        .to_string()
        .contains("SCANNER1"));
    assert_eq!(
        matched.get_output_bytes("wwarning"),
        Some(b"[SCANNER1] WARNING: Over or underflow of long long integer.\n".as_slice())
    );
}

#[cfg(feature = "serde")]
#[test]
fn every_codec_preserves_pending_and_completed_warning_values() {
    use ferric_rules_runtime::SerializationFormat;

    for &format in SerializationFormat::ALL {
        let mut engine = configured();
        let input = engine.create_string_bytes(b"\"unfinished\\").unwrap();
        engine.assert_ordered("input", [input]).unwrap();
        let mut pending = Engine::deserialize(&engine.serialize(format).unwrap(), format).unwrap();
        run_success(&mut pending);
        string_result(&pending, b"unfinished\xff");
        let diagnostic = pending.action_diagnostics()[0].to_string();
        let mut completed =
            Engine::deserialize(&pending.serialize(format).unwrap(), format).unwrap();
        string_result(&completed, b"unfinished\xff");
        assert_eq!(completed.action_diagnostics()[0].to_string(), diagnostic);
        assert_eq!(
            completed.get_output_bytes("werror"),
            pending.get_output_bytes("werror")
        );
        assert_eq!(completed.run(RunLimit::Unlimited).unwrap().rules_fired, 0);
        assert!(completed.action_diagnostics().is_empty());
        completed.reset().unwrap();
        assert!(completed.action_diagnostics().is_empty());
        assert_eq!(completed.get_output_bytes("werror"), None);
    }
}

#[test]
fn strict_ascii_policy_rejects_a_generated_raw_field_without_replacing_it() {
    // Ferric's explicit strict policy is a local contract, not a CLIPS mode.
    let mut engine = Engine::new(EngineConfig::ascii());
    engine.load_str(SOURCE).unwrap();
    let input = engine.create_string_bytes(b"\"unfinished\\").unwrap();
    engine.assert_ordered("input", [input]).unwrap();
    assert_eq!(
        engine.run(RunLimit::Unlimited).unwrap().halt_reason,
        HaltReason::ActionError
    );
    string_result(&engine, b"*** ERROR ***");
    assert_eq!(engine.action_diagnostics().len(), 2);
    assert!(engine.action_diagnostics()[0]
        .to_string()
        .contains("SCANNER1"));
    assert!(engine.action_diagnostics()[1]
        .to_string()
        .contains("encoding"));
    assert!(engine.find_facts("after").unwrap().is_empty());
}

#[cfg(feature = "serde")]
#[test]
fn every_codec_preserves_completed_error_string_and_diagnostic() {
    use ferric_rules_runtime::SerializationFormat;

    for &format in SerializationFormat::ALL {
        let mut engine = configured();
        engine.assert_ordered("input", [12_i64]).unwrap();
        assert_eq!(
            engine.run(RunLimit::Unlimited).unwrap().halt_reason,
            HaltReason::ActionError
        );
        string_result(&engine, b"*** ERROR ***");
        let diagnostic = engine.action_diagnostics()[0].to_string();
        let mut restored = Engine::deserialize(&engine.serialize(format).unwrap(), format).unwrap();
        string_result(&restored, b"*** ERROR ***");
        assert_eq!(restored.action_diagnostics()[0].to_string(), diagnostic);
        assert!(restored.find_facts("after").unwrap().is_empty());
        assert_eq!(restored.run(RunLimit::Unlimited).unwrap().rules_fired, 0);
        assert!(restored.action_diagnostics().is_empty());
    }
}
