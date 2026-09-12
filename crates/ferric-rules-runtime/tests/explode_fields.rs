//! #339: complete CLIPS field scanning, actual values, and wrapper boundaries.
//! Reference: sealed CLIPS 6.30 probes and `multifun.c::ExplodeFunction` /
//! `multifld.c::StringToMultifield`. Output formatting is not used as a type oracle.

use ferric_rules_core::Value;
use ferric_rules_runtime::{Engine, EngineConfig, HaltReason, HostValue, RunLimit};

const ALIASES: &[&str] = &["explode$", "str-explode"];
const INTEGER_NOTICE: &[u8] = b"[SCANNER1] WARNING: Over or underflow of long long integer.\n";
const STRING_NOTICE: &[u8] = b"\n[SCANNER1] Encountered End-Of-File while scanning a string\n";

fn source(alias: &str) -> String {
    format!(
        r"
        (defglobal ?*result* = pending ?*trace* = 0)
        (deffunction mark (?value) (bind ?*trace* (+ ?*trace* 1)) ?value)
        (defrule scan (input ?value) =>
          (bind ?*result* ({alias} (mark ?value))) (assert (after)))
        (defrule follower (after) => (assert (followed)))
        "
    )
}

fn configured(alias: &str) -> Engine {
    Engine::with_rules(&source(alias)).unwrap()
}

fn run_success(engine: &mut Engine) {
    let result = engine.run(RunLimit::Unlimited).unwrap();
    assert_eq!(result.halt_reason, HaltReason::AgendaEmpty);
    assert_eq!(result.rules_fired, 2);
    assert_eq!(engine.find_facts("followed").unwrap().len(), 1);
}

fn run_string(alias: &str, input: &[u8]) -> Engine {
    let mut engine = configured(alias);
    let input = engine.create_string_bytes(input).unwrap();
    engine.assert_ordered("input", [input]).unwrap();
    run_success(&mut engine);
    assert!(matches!(
        engine.get_global("trace"),
        Some(Value::Integer(1))
    ));
    engine
}

#[derive(Debug)]
enum Field<'a> {
    Integer(i64),
    Float(f64),
    String(&'a [u8]),
    Symbol(&'a [u8]),
    Name(&'a [u8]),
}

fn assert_fields(engine: &Engine, expected: &[Field<'_>]) {
    let Some(Value::Multifield(fields)) = engine.get_global("result") else {
        panic!("result must be an actual MULTIFIELD")
    };
    assert_eq!(fields.len(), expected.len());
    for (actual, expected) in fields.iter().zip(expected) {
        match (actual, expected) {
            (Value::Integer(actual), Field::Integer(expected)) => assert_eq!(actual, expected),
            (Value::Float(actual), Field::Float(expected)) => {
                assert_eq!(actual.to_bits(), expected.to_bits());
            }
            (Value::String(actual), Field::String(expected)) => {
                assert_eq!(actual.as_bytes(), *expected);
            }
            (Value::Symbol(actual), Field::Symbol(expected)) => {
                assert_eq!(engine.resolve_core_symbol_bytes(*actual), Some(*expected));
            }
            (Value::InstanceName(actual), Field::Name(expected)) => {
                assert_eq!(
                    engine.resolve_core_symbol_bytes(actual.as_symbol()),
                    Some(*expected)
                );
            }
            _ => panic!("wrong field type: {actual:?}, expected {expected:?}"),
        }
    }
}

#[test]
fn quotes_empty_fields_adjacency_and_field_escapes_preserve_types() {
    use Field::{Integer as I, String as T, Symbol as S};
    for alias in ALIASES {
        let engine = run_string(alias, b"a \"two words\" 3 \"\" \"3\"");
        assert_fields(&engine, &[S(b"a"), T(b"two words"), I(3), T(b""), T(b"3")]);
        assert!(engine.action_diagnostics().is_empty());
        let engine = run_string(
            alias,
            b"a\"two words\"3 \"x\"\"y\" \"a\\nb\\t\\q\\\"\\\\\" \"a\nb\"",
        );
        assert_fields(
            &engine,
            &[
                S(b"a"),
                T(b"two words"),
                I(3),
                T(b"x"),
                T(b"y"),
                T(b"anbtq\"\\"),
                T(b"a\nb"),
            ],
        );
        assert!(engine.action_diagnostics().is_empty());
    }
}

#[test]
fn numeric_grammar_keeps_exact_integer_and_float_types() {
    use Field::{Float as F, Integer as I, Symbol as S};
    let engine = run_string(
        "explode$",
        b"42 -17 +17 00042 .5 -.5 1. 2e3 -0.0 9223372036854775807 -9223372036854775808",
    );
    assert_fields(
        &engine,
        &[
            I(42),
            I(-17),
            I(17),
            I(42),
            F(0.5),
            F(-0.5),
            F(1.0),
            F(2000.0),
            F(-0.0),
            I(i64::MAX),
            I(i64::MIN),
        ],
    );
    assert!(engine.action_diagnostics().is_empty());
    let engine = run_string("str-explode", b"42abc 1.abc 1e 1e+ 1.2.3 + - NaN inf 0x10");
    assert_fields(
        &engine,
        &[
            S(b"42abc"),
            S(b"1.abc"),
            S(b"1e"),
            S(b"1e+"),
            S(b"1.2.3"),
            S(b"+"),
            S(b"-"),
            S(b"NaN"),
            S(b"inf"),
            S(b"0x10"),
        ],
    );
    assert!(engine.action_diagnostics().is_empty());
}

#[test]
fn nonvalue_print_forms_and_unknown_fields_do_not_end_scanning() {
    use Field::{Integer as I, String as T, Symbol as S};
    for alias in ALIASES {
        let engine = run_string(alias, b"(a) ?x $?x ?*x* ? $? & | ~ = <- : \x0b42 tail");
        assert_fields(
            &engine,
            &[
                T(b"("),
                S(b"a"),
                T(b")"),
                T(b"?x"),
                T(b"$?x"),
                T(b"?*x*"),
                T(b"?"),
                T(b"$?"),
                T(b"&"),
                T(b"|"),
                T(b"~"),
                S(b"="),
                S(b"<-"),
                S(b":"),
                T(b"<<<unprintable character>>>"),
                I(42),
                S(b"tail"),
            ],
        );
        assert!(engine.action_diagnostics().is_empty());
        assert_eq!(engine.get_output_bytes("werror"), None);
    }
}

#[test]
fn stop_empty_comments_and_control_bytes_contribute_no_eof_atom() {
    use Field::{Integer as I, Symbol as S};
    for alias in ALIASES {
        for input in [
            b"".as_slice(),
            b" \t\r\n\x0c; comment".as_slice(),
            b"\0ignored".as_slice(),
            b"\x03ignored".as_slice(),
        ] {
            let engine = run_string(alias, input);
            assert_fields(&engine, &[]);
            assert!(engine.action_diagnostics().is_empty());
        }
        for input in [
            b"a 3\0\"unfinished".as_slice(),
            b"a 3\x03\"unfinished".as_slice(),
        ] {
            let engine = run_string(alias, input);
            assert_fields(&engine, &[S(b"a"), I(3)]);
            assert!(engine.action_diagnostics().is_empty());
        }
        let engine = run_string(alias, b"a; ignored\r\n42\x0ctail a\xc2\xa0b");
        assert_fields(&engine, &[S(b"a"), I(42), S(b"tail"), S(b"a\xc2\xa0b")]);
        assert!(engine.action_diagnostics().is_empty());
    }
}

#[test]
fn raw_string_symbol_and_instance_name_fields_retain_their_bytes() {
    use Field::{Name as N, String as T, Symbol as S};
    for alias in ALIASES {
        let engine = run_string(alias, b"\"a\xff\xc3\" a\xc3 [n\xc3] [widget]");
        assert_fields(
            &engine,
            &[T(b"a\xff\xc3"), S(b"a\xc3"), N(b"n\xc3"), N(b"widget")],
        );
        assert!(engine.action_diagnostics().is_empty());
    }
}

#[test]
fn notices_keep_prior_fields_and_allow_following_actions_and_activations() {
    use Field::{Float as F, Integer as I, String as T, Symbol as S};
    for alias in ALIASES {
        let engine = run_string(
            alias,
            b"a 9223372036854775808 -9223372036854775809 1.0e309 1.0e-999 tail",
        );
        assert_fields(
            &engine,
            &[
                S(b"a"),
                I(i64::MAX),
                I(i64::MIN),
                F(f64::INFINITY),
                F(0.0),
                S(b"tail"),
            ],
        );
        assert_eq!(engine.action_diagnostics().len(), 2);
        assert_eq!(
            engine.get_output_bytes("wwarning"),
            Some(INTEGER_NOTICE.repeat(2).as_slice())
        );
        assert_eq!(engine.get_output_bytes("werror"), None);
        for (input, expected) in [
            (b"a \"unfinished".as_slice(), b"unfinished".as_slice()),
            (b"a \"unfinished\\".as_slice(), b"unfinished\xff".as_slice()),
        ] {
            let engine = run_string(alias, input);
            assert_fields(&engine, &[S(b"a"), T(expected)]);
            assert_eq!(engine.action_diagnostics().len(), 1);
            assert!(engine.action_diagnostics()[0]
                .to_string()
                .contains("SCANNER1"));
            assert!(engine.action_diagnostics()[0]
                .to_string()
                .contains("explode$"));
            assert_eq!(engine.get_output_bytes("werror"), Some(STRING_NOTICE));
        }
    }
}

#[test]
fn dynamic_nonstring_arguments_assign_empty_multifield_and_stop_after_evaluation() {
    for alias in ALIASES {
        for kind in ["integer", "symbol", "name", "multifield"] {
            let mut engine = configured(alias);
            let input: HostValue = match kind {
                "integer" => 12_i64.into(),
                "symbol" => engine.symbol_value("a b").unwrap(),
                "name" => engine.instance_name_value("a b").unwrap(),
                _ => HostValue::multifield(vec![12_i64.into()]).unwrap(),
            };
            engine.assert_ordered("input", [input]).unwrap();
            assert_eq!(
                engine.run(RunLimit::Unlimited).unwrap().halt_reason,
                HaltReason::ActionError
            );
            assert_fields(&engine, &[]);
            assert!(matches!(
                engine.get_global("trace"),
                Some(Value::Integer(1))
            ));
            assert_eq!(engine.action_diagnostics().len(), 1);
            assert!(engine.action_diagnostics()[0]
                .to_string()
                .contains("explode$"));
            assert!(engine.find_facts("after").unwrap().is_empty());
        }
    }
}

#[test]
fn throwing_operand_preserves_effects_and_empty_result_before_halting() {
    for alias in ALIASES {
        let mut engine = Engine::with_rules(&format!(
            r"
            (defglobal ?*result* = pending ?*trace* = 0)
            (deffunction fail () (bind ?*trace* (+ ?*trace* 1)) (/ 1 0))
            (defrule scan => (bind ?*result* ({alias} (fail))) (assert (after)))
        "
        ))
        .unwrap();
        assert_eq!(
            engine.run(RunLimit::Unlimited).unwrap().halt_reason,
            HaltReason::ActionError
        );
        assert_fields(&engine, &[]);
        assert!(matches!(
            engine.get_global("trace"),
            Some(Value::Integer(1))
        ));
        assert_eq!(engine.action_diagnostics().len(), 1);
        assert!(engine.action_diagnostics()[0]
            .to_string()
            .contains("division by zero"));
        assert!(engine.find_facts("after").unwrap().is_empty());
    }
}

#[test]
fn notices_drain_at_load_and_match_boundaries() {
    use Field::{Integer as I, Symbol as S};
    let mut engine = Engine::new(EngineConfig::utf8());
    let loaded = engine
        .load_str(r#"(defglobal ?*result* = (explode$ "a 9223372036854775808"))"#)
        .unwrap();
    assert_eq!(loaded.warnings.len(), 1);
    assert_eq!(engine.action_diagnostics().len(), 1);
    assert_eq!(engine.get_output_bytes("wwarning"), Some(INTEGER_NOTICE));
    assert_fields(&engine, &[S(b"a"), I(i64::MAX)]);
    let mut matched = Engine::with_rules(
        r"
        (defrule match (input ?text) (test (= (length$ (str-explode ?text)) 2)) => (assert (after)))
    ",
    )
    .unwrap();
    let value = matched
        .create_string_bytes(b"a 9223372036854775808")
        .unwrap();
    matched.assert_ordered("input", [value]).unwrap();
    assert_eq!(matched.agenda_len(), 1);
    assert_eq!(matched.action_diagnostics().len(), 1);
    assert_eq!(matched.get_output_bytes("wwarning"), Some(INTEGER_NOTICE));
}

#[test]
fn strict_ascii_policy_rejects_generated_raw_field_without_partial_result() {
    // Local strict encoding policy, not a CLIPS encoding mode. The scanner's
    // prior nonfatal notice survives the subsequent conversion failure.
    let mut engine = Engine::new(EngineConfig::ascii());
    engine.load_str(&source("explode$")).unwrap();
    let input = engine.create_string_bytes(b"a \"unfinished\\").unwrap();
    engine.assert_ordered("input", [input]).unwrap();
    assert_eq!(
        engine.run(RunLimit::Unlimited).unwrap().halt_reason,
        HaltReason::ActionError
    );
    assert_fields(&engine, &[]);
    assert_eq!(engine.action_diagnostics().len(), 2);
    assert!(engine.action_diagnostics()[0]
        .to_string()
        .contains("SCANNER1"));
    assert!(engine.action_diagnostics()[1]
        .to_string()
        .contains("encoding"));
    assert_eq!(engine.get_output_bytes("werror"), Some(STRING_NOTICE));
    assert!(engine.find_facts("after").unwrap().is_empty());
}

#[cfg(feature = "serde")]
#[test]
fn every_codec_preserves_pending_completed_raw_fields_notices_and_fresh_run_clear() {
    use ferric_rules_runtime::SerializationFormat;
    use Field::{Name as N, String as T, Symbol as S};
    for &format in SerializationFormat::ALL {
        let mut engine = configured("explode$");
        let input = engine
            .create_string_bytes(b"a\xc3 [n\xc3] \"unfinished\\")
            .unwrap();
        engine.assert_ordered("input", [input]).unwrap();
        let mut pending = Engine::deserialize(&engine.serialize(format).unwrap(), format).unwrap();
        run_success(&mut pending);
        assert_fields(&pending, &[S(b"a\xc3"), N(b"n\xc3"), T(b"unfinished\xff")]);
        let diagnostic = pending.action_diagnostics()[0].to_string();
        let mut completed =
            Engine::deserialize(&pending.serialize(format).unwrap(), format).unwrap();
        assert_fields(
            &completed,
            &[S(b"a\xc3"), N(b"n\xc3"), T(b"unfinished\xff")],
        );
        assert_eq!(completed.action_diagnostics()[0].to_string(), diagnostic);
        assert_eq!(completed.get_output_bytes("werror"), Some(STRING_NOTICE));
        assert_eq!(completed.run(RunLimit::Unlimited).unwrap().rules_fired, 0);
        assert!(completed.action_diagnostics().is_empty());
        completed.reset().unwrap();
        assert_eq!(completed.get_output_bytes("werror"), None);
        let input = completed.create_string_bytes(b"\"fresh\"").unwrap();
        completed.assert_ordered("input", [input]).unwrap();
        run_success(&mut completed);
        assert_fields(&completed, &[T(b"fresh")]);
        assert!(completed.action_diagnostics().is_empty());
    }
}

#[cfg(feature = "serde")]
#[test]
fn every_codec_preserves_completed_empty_error_value_without_persisting_halt() {
    use ferric_rules_runtime::SerializationFormat;
    for &format in SerializationFormat::ALL {
        let mut engine = configured("str-explode");
        engine.assert_ordered("input", [12_i64]).unwrap();
        assert_eq!(
            engine.run(RunLimit::Unlimited).unwrap().halt_reason,
            HaltReason::ActionError
        );
        assert_fields(&engine, &[]);
        let diagnostic = engine.action_diagnostics()[0].to_string();
        let mut restored = Engine::deserialize(&engine.serialize(format).unwrap(), format).unwrap();
        assert_fields(&restored, &[]);
        assert_eq!(restored.action_diagnostics()[0].to_string(), diagnostic);
        assert!(restored.find_facts("after").unwrap().is_empty());
        assert_eq!(restored.run(RunLimit::Unlimited).unwrap().rules_fired, 0);
        assert!(restored.action_diagnostics().is_empty());
        restored.reset().unwrap();
        let input = restored.create_string_bytes(b"42").unwrap();
        restored.assert_ordered("input", [input]).unwrap();
        run_success(&mut restored);
        assert_fields(&restored, &[Field::Integer(42)]);
    }
}
