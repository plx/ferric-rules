//! Public Value/byte controls for #345, plus neighboring formatter isolation.
//!
//! Direct-print expectations follow the pinned CLIPS sources. The explicit
//! str-cat/sym-cat numeric isolation preserves their existing Ferric policy;
//! format has a separate STRING-content control. The numeric isolation is not
//! a claim of CLIPS parity for those separate functions.

use ferric_rules_core::Value;
use ferric_rules_runtime::{Engine, EngineConfig, HaltReason, RunLimit};

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
    value.as_str()
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
        engine.get_output("t"),
        Some(format!("{line}{line}").as_str())
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
            engine.get_output("t").unwrap().as_bytes(),
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
            engine.get_output("t"),
            Some(format!("{line}{line}").as_str())
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
        engine.get_output("t"),
        Some("(-9223372036854775808 9223372036854775807 9007199254740993)\n")
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
            engine.get_output("t"),
            Some(format!("{line}{line}").as_str())
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
        engine.get_output("t"),
        Some("[(\"a\" \"two words\")|plain words]\n")
    );
}

#[test]
fn action_only_println_keeps_its_newline_and_uses_the_same_field_mode() {
    // println is a Ferric convenience action, not a CLIPS comparator/builtin claim.
    let mut engine = engine(r#"(defrule exercise => (println "raw " (create$ "two words")))"#);
    run(&mut engine);
    assert_eq!(engine.get_output("t"), Some("raw (\"two words\")\n"));
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
        engine.get_output("t"),
        Some("(\"a\"b\" \"a\\b\" \"two words\" 3.5)\n")
    );
    let saved = std::fs::read_to_string(path).unwrap();
    assert!(
        saved
            .lines()
            .any(|line| line == r#"(row "a\"b" "a\\b" "two words" 3.5)"#),
        "{saved:?}"
    );
}
