//! Public Value/byte controls for #345, plus neighboring formatter isolation.
//!
//! Direct-print expectations follow the pinned CLIPS sources. The explicit
//! str-cat/sym-cat numeric isolation preserves their existing Ferric policy;
//! format has a separate STRING-content control. The numeric isolation is not
//! a claim of CLIPS parity for those separate functions.

use ferric_rules_core::Value;
use ferric_rules_runtime::{Engine, EngineConfig, HaltReason, HostValue, RunLimit};

fn engine(source: &str) -> Engine {
    let mut engine = Engine::new(EngineConfig::utf8());
    engine.load_str(source).unwrap();
    engine.reset().unwrap();
    engine
}

fn run(engine: &mut Engine) {
    let result = engine.run(RunLimit::Count(10)).unwrap();
    assert_eq!(result.halt_reason, HaltReason::AgendaEmpty);
    assert_eq!(result.rules_fired, 1);
    assert!(engine.action_diagnostics().is_empty());
}

fn source_string(value: &str) -> String {
    format!("\"{}\"", value.replace('\\', "\\\\").replace('"', "\\\""))
}

fn global_string<'a>(engine: &'a Engine, name: &str) -> &'a str {
    let Some(Value::String(value)) = engine.get_global(name) else {
        panic!(
            "expected actual STRING {name}, got {:?}",
            engine.get_global(name)
        )
    };
    value
        .as_str()
        .expect("fixture STRING must contain valid UTF-8")
}

#[test]
fn both_print_paths_preserve_actual_field_types_values_and_void_return() {
    let mut engine = engine(
        r#"(defglobal ?*fields* = (create$ "" "a\"b" "a\\b" 9007199254740993 -0.0 (sym-cat "two words"))
                     ?*result* = pending)
           (deffunction emit (?value) (printout t ?value crlf))
           (defrule exercise =>
             (printout t ?*fields* crlf)
             (bind ?*result* (emit ?*fields*)))"#,
    );
    let before = engine.get_global("fields").unwrap().clone();
    run(&mut engine);
    let after = engine.get_global("fields").unwrap();
    assert!(before.structural_eq(after));
    let Value::Multifield(fields) = after else {
        panic!("printing must preserve MULTIFIELD identity")
    };
    assert!(
        matches!(fields.as_slice(), [Value::String(empty), Value::String(quote),
        Value::String(slash), Value::Integer(9_007_199_254_740_993), Value::Float(zero),
        Value::Symbol(symbol)] if empty.as_bytes().is_empty()
        && quote.as_bytes() == b"a\"b" && slash.as_bytes() == b"a\\b"
        && zero.to_bits() == (-0.0_f64).to_bits()
        && engine.resolve_core_symbol(*symbol) == Some("two words"))
    );
    assert!(matches!(engine.get_global("result"), Some(Value::Void)));
    let line = "(\"\" \"a\"b\" \"a\\b\" 9007199254740993 -0.0 two words)\n";
    assert_eq!(
        engine.get_output_bytes("t"),
        Some(format!("{line}{line}").as_bytes())
    );
}

#[test]
fn raw_string_bytes_are_only_surrounded_by_quotes_inside_a_multifield() {
    for text in [
        "",
        "two words",
        "a\"b",
        "a\\b",
        "a\r\nb",
        "a\t\u{b}\u{c}b",
        "e\u{301}雪🙂",
    ] {
        let literal = source_string(text);
        let mut engine = engine(&format!(
            "(defglobal ?*text* = {literal})
             (deffunction emit (?value) (printout t \"[\" ?value \"]|\" (create$ ?value) crlf))
             (defrule exercise =>
               (printout t \"[\" ?*text* \"]|\" (create$ ?*text*) crlf)
               (emit ?*text*))"
        ));
        run(&mut engine);
        assert_eq!(global_string(&engine, "text").as_bytes(), text.as_bytes());
        let line = format!("[{text}]|(\"{text}\")\n");
        assert_eq!(
            engine.get_output_bytes("t").unwrap(),
            format!("{line}{line}").as_bytes()
        );
    }
}

#[test]
fn finite_float_spelling_keeps_the_underlying_bits_in_both_print_paths() {
    for (literal, expected) in [
        ("1.0", "1.0"),
        ("-0.0", "-0.0"),
        ("0.0001", "0.0001"),
        ("1.0e-5", "1e-05"),
        ("1.0e14", "100000000000000.0"),
        ("1.0e15", "1e+15"),
        ("1.2345678901234567", "1.23456789012346"),
        ("5.0e-324", "4.94065645841247e-324"),
        ("999999999999999.5", "1e+15"),
    ] {
        let mut engine = engine(&format!(
            "(defglobal ?*number* = {literal})
             (deffunction emit (?value) (printout t ?value \"|\" (create$ ?value) crlf))
             (defrule exercise =>
               (printout t ?*number* \"|\" (create$ ?*number*) crlf)
               (emit ?*number*))"
        ));
        run(&mut engine);
        assert!(
            matches!(engine.get_global("number"), Some(Value::Float(value))
            if value.to_bits() == literal.parse::<f64>().unwrap().to_bits())
        );
        let line = format!("{expected}|({expected})\n");
        assert_eq!(
            engine.get_output_bytes("t"),
            Some(format!("{line}{line}").as_bytes())
        );
    }
}

#[test]
fn integer_fields_never_pass_through_float_formatting() {
    let mut engine = engine(
        "(defglobal ?*fields* = (create$ -9223372036854775808 9223372036854775807 9007199254740993))
         (defrule exercise => (printout t ?*fields* crlf))",
    );
    run(&mut engine);
    let Some(Value::Multifield(fields)) = engine.get_global("fields") else {
        panic!("expected MULTIFIELD")
    };
    assert!(matches!(
        fields.as_slice(),
        [
            Value::Integer(i64::MIN),
            Value::Integer(i64::MAX),
            Value::Integer(9_007_199_254_740_993)
        ]
    ));
    assert_eq!(
        engine.get_output_bytes("t"),
        Some(b"(-9223372036854775808 9223372036854775807 9007199254740993)\n".as_slice())
    );
}

#[test]
fn control_symbols_expand_only_at_top_level_and_are_not_retyped() {
    for (name, control) in [
        ("crlf", '\n'),
        ("tab", '\t'),
        ("vtab", '\u{b}'),
        ("ff", '\u{c}'),
    ] {
        let mut engine = engine(&format!(
            "(defglobal ?*control* = (sym-cat \"{name}\"))
             (deffunction emit (?value) (printout t \"[\" ?value \"]|\" (create$ ?value) crlf))
             (defrule exercise =>
               (printout t \"[\" ?*control* \"]|\" (create$ ?*control*) crlf)
               (emit ?*control*))"
        ));
        run(&mut engine);
        let Some(Value::Symbol(symbol)) = engine.get_global("control") else {
            panic!("printing must not convert control SYMBOL to STRING")
        };
        assert_eq!(engine.resolve_core_symbol(*symbol), Some(name));
        let line = format!("[{control}]|({name})\n");
        assert_eq!(
            engine.get_output_bytes("t"),
            Some(format!("{line}{line}").as_bytes())
        );
    }
}

#[test]
fn formatting_does_not_repeat_operand_effects() {
    let mut engine = engine(
        r#"(defglobal ?*trace* = 0)
           (deffunction mark (?digit ?value)
             (bind ?*trace* (+ (* ?*trace* 10) ?digit)) ?value)
           (defrule exercise =>
             (printout t "[" (mark 1 (create$ "a" "two words")) "|" (mark 2 "plain words") "]" crlf))"#,
    );
    run(&mut engine);
    assert!(matches!(
        engine.get_global("trace"),
        Some(Value::Integer(12))
    ));
    assert_eq!(
        engine.get_output_bytes("t"),
        Some(b"[(\"a\" \"two words\")|plain words]\n".as_slice())
    );
}

#[test]
fn action_only_println_keeps_its_newline_and_uses_the_same_field_mode() {
    // println is a Ferric convenience action, not a CLIPS comparator/builtin claim.
    let mut engine = engine(r#"(defrule exercise => (println "raw " (create$ "two words")))"#);
    run(&mut engine);
    assert_eq!(
        engine.get_output_bytes("t"),
        Some(b"raw (\"two words\")\n".as_slice())
    );
}

#[test]
fn neighboring_string_builders_keep_their_existing_raw_content_policy() {
    let mut engine = engine(
        r#"(defglobal ?*string* = pending ?*symbol* = pending ?*format* = pending
                       ?*number-string* = pending ?*number-symbol* = pending)
           (defrule exercise =>
             (bind ?*string* (str-cat "a\"b" "|" "a\\b" "|two words"))
             (bind ?*symbol* (sym-cat "a\"b" "|" "a\\b" "|two words"))
             (bind ?*format* (format nil "%s" "two words"))
             (bind ?*number-string* (str-cat 1.0e20))
             (bind ?*number-symbol* (sym-cat 1.0e20)))"#,
    );
    run(&mut engine);
    assert_eq!(global_string(&engine, "string"), "a\"b|a\\b|two words");
    let Some(Value::Symbol(symbol)) = engine.get_global("symbol") else {
        panic!("sym-cat must keep SYMBOL identity")
    };
    assert_eq!(
        engine.resolve_core_symbol(*symbol),
        Some("a\"b|a\\b|two words")
    );
    assert_eq!(global_string(&engine, "format"), "two words");
    // Deliberately preserve these neighboring functions' preexisting spelling:
    // #345 changes printout's numeric writer, not str-cat/sym-cat semantics.
    assert_eq!(
        global_string(&engine, "number-string"),
        "100000000000000000000.0"
    );
    let Some(Value::Symbol(symbol)) = engine.get_global("number-symbol") else {
        panic!("sym-cat must keep SYMBOL identity")
    };
    assert_eq!(
        engine.resolve_core_symbol(*symbol),
        Some("100000000000000000000.0")
    );
}

#[test]
fn save_facts_keeps_escaped_string_fields_instead_of_printout_raw_quotes() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("quoted.fct");
    let path_literal = source_string(path.to_str().unwrap());
    let mut engine = engine(&format!(
        r#"(deffacts seed (row "a\"b" "a\\b" "two words" 3.5))
           (defrule exercise =>
             (printout t (create$ "a\"b" "a\\b" "two words" 3.5) crlf)
             (save-facts {path_literal}))"#
    ));
    run(&mut engine);
    assert_eq!(
        engine.get_output_bytes("t"),
        Some(b"(\"a\"b\" \"a\\b\" \"two words\" 3.5)\n".as_slice())
    );
    let saved = std::fs::read_to_string(path).unwrap();
    assert!(
        saved
            .lines()
            .any(|line| line == r#"(row "a\"b" "a\\b" "two words" 3.5)"#),
        "{saved:?}"
    );
}

// PR379 host-byte contract extensions, not newly executed CLIPS source oracles.
// Direct CLIPS field spelling supplies the delimiters: STRING gets surrounding
// quotes only inside a multifield, SYMBOL stays raw, INSTANCE-NAME gets brackets.
// Payload bytes, including NUL and invalid UTF-8, must remain untouched.
struct HostBytePrintCase {
    text: &'static [u8],
    symbol: &'static [u8],
    name: &'static [u8],
    line: &'static [u8],
    valid_utf8_output: bool,
}

const HOST_BYTE_PRINT_CASES: &[HostBytePrintCase] = &[
    HostBytePrintCase {
        text: b"t\0\xff\"\\z",
        symbol: b"s\0\xc3",
        name: b"n\0\xff",
        line: b"t\0\xff\"\\z|s\0\xc3|[n\0\xff]|(\"t\0\xff\"\\z\" s\0\xc3 [n\0\xff])\n",
        valid_utf8_output: false,
    },
    HostBytePrintCase {
        text: b"",
        symbol: b"crlf\0tail",
        name: b"crlf",
        line: b"|crlf\0tail|[crlf]|(\"\" crlf\0tail [crlf])\n",
        valid_utf8_output: true,
    },
];

const HOST_BYTE_PRINT_SOURCE: &str = r#"
    (defglobal ?*captured* = pending ?*result* = pending)
    (deffunction emit (?text ?symbol ?name)
      (printout t ?text "|" ?symbol "|" ?name "|"
        (create$ ?text ?symbol ?name) crlf))
    (defrule exercise (raw-payload ?text ?symbol ?name) =>
      (bind ?*captured* (create$ ?text ?symbol ?name))
      (printout t ?text "|" ?symbol "|" ?name "|"
        ?*captured* crlf)
      (bind ?*result* (emit ?text ?symbol ?name)))
"#;

fn host_byte_print_engine(case: &HostBytePrintCase) -> Engine {
    let mut engine = engine(HOST_BYTE_PRINT_SOURCE);
    let values: Vec<HostValue> = vec![
        engine.create_string_bytes(case.text).unwrap().into(),
        engine.symbol_value_bytes(case.symbol).unwrap(),
        engine.instance_name_value_bytes(case.name).unwrap(),
    ];
    engine.assert_ordered("raw-payload", values).unwrap();
    engine
}

fn assert_host_byte_print(engine: &Engine, case: &HostBytePrintCase) {
    let Some(Value::Multifield(fields)) = engine.get_global("captured") else {
        panic!("captured fields must retain an actual MULTIFIELD")
    };
    let [Value::String(text), Value::Symbol(symbol), Value::InstanceName(name)] = fields.as_slice()
    else {
        panic!("STRING, SYMBOL, and INSTANCE-NAME must retain distinct variants")
    };
    assert_eq!(text.as_bytes(), case.text);
    assert_eq!(engine.resolve_core_symbol_bytes(*symbol), Some(case.symbol));
    assert_eq!(
        engine.resolve_core_symbol_bytes(name.as_symbol()),
        Some(case.name)
    );
    assert!(matches!(engine.get_global("result"), Some(Value::Void)));
    let expected = case.line.repeat(2);
    assert_eq!(engine.get_output_bytes("t"), Some(expected.as_slice()));
    if case.valid_utf8_output {
        // NUL is valid UTF-8 and must not be mistaken for a terminator or error.
        assert_eq!(
            engine.get_output("t").unwrap().unwrap().as_bytes(),
            expected
        );
    } else {
        assert!(engine.get_output("t").is_err());
    }
    assert!(engine.action_diagnostics().is_empty());
}

#[test]
fn host_byte_contract_preserves_raw_fields_in_action_and_callable_print_paths() {
    for case in HOST_BYTE_PRINT_CASES {
        let mut engine = host_byte_print_engine(case);
        run(&mut engine);
        assert_host_byte_print(&engine, case);
    }
}

#[cfg(feature = "serde")]
#[test]
fn host_byte_contract_preserves_pending_completed_print_state_in_all_formats() {
    use ferric_rules_runtime::SerializationFormat;

    for case in HOST_BYTE_PRINT_CASES {
        let engine = host_byte_print_engine(case);
        for &format in SerializationFormat::ALL {
            let mut pending = Engine::deserialize(&engine.serialize(format).unwrap(), format)
                .unwrap_or_else(|error| panic!("pending {format:?}: {error:?}"));
            assert_eq!(pending.get_output_bytes("t"), None);
            run(&mut pending);
            assert_host_byte_print(&pending, case);
            let mut completed = Engine::deserialize(&pending.serialize(format).unwrap(), format)
                .unwrap_or_else(|error| panic!("completed {format:?}: {error:?}"));
            assert_host_byte_print(&completed, case);
            let result = completed.run(RunLimit::Count(10)).unwrap();
            assert_eq!(result.rules_fired, 0);
            assert_eq!(result.halt_reason, HaltReason::AgendaEmpty);
            assert_host_byte_print(&completed, case);
        }
    }
}
