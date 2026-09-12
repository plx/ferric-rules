//! Issue #344: inspect actual returned STRINGs and unchanged input Values.
//! Success expectations follow pinned CLIPS 6.30 evidence. Error controls
//! exercise Ferric's runtime gates without claiming CLIPS source-rejection
//! timing or the separate printout error-prefix protocol is repaired.

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
    assert_eq!(result.rules_fired, 1);
    assert_eq!(result.halt_reason, HaltReason::AgendaEmpty);
    assert!(engine.action_diagnostics().is_empty());
}

fn string<'a>(engine: &'a Engine, name: &str) -> &'a str {
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

fn source_string(value: &str) -> String {
    format!("\"{}\"", value.replace('\\', "\\\\").replace('"', "\\\""))
}

#[test]
fn escaped_fields_preserve_input_types_and_differ_from_direct_printing() {
    let mut engine = engine(
        r#"(defglobal ?*fields* = (create$ "" "a\"b" "a\\b" 9007199254740993 -0.0 (sym-cat "two words"))
                      ?*result* = pending)
           (defrule exercise =>
             (bind ?*result* (implode$ ?*fields*))
             (printout t ?*fields* "|" ?*result* crlf))"#,
    );
    let before = engine.get_global("fields").unwrap().clone();
    run(&mut engine);
    let after = engine.get_global("fields").unwrap();
    assert!(before.structural_eq(after));
    let Value::Multifield(fields) = after else {
        panic!("expected MULTIFIELD")
    };
    assert!(matches!(fields.as_slice(),
        [Value::String(empty), Value::String(quote), Value::String(slash),
         Value::Integer(9_007_199_254_740_993), Value::Float(zero), Value::Symbol(symbol)]
        if empty.as_bytes().is_empty() && quote.as_bytes() == b"a\"b"
        && slash.as_bytes() == b"a\\b" && zero.to_bits() == (-0.0_f64).to_bits()
        && engine.resolve_core_symbol(*symbol) == Some("two words")));
    assert_eq!(
        string(&engine, "result"),
        r#""" "a\"b" "a\\b" 9007199254740993 -0.0 two words"#
    );
    assert_eq!(
        engine.get_output("t").unwrap(),
        Some(concat!(
            "(\"\" \"a\"b\" \"a\\b\" 9007199254740993 -0.0 two words)|",
            "\"\" \"a\\\"b\" \"a\\\\b\" 9007199254740993 -0.0 two words\n"
        ))
    );
}

#[test]
fn escaping_preserves_literal_controls_and_unicode_bytes() {
    for (text, expected) in [
        ("", "\"\""),
        ("two words", "\"two words\""),
        ("a\"b", "\"a\\\"b\""),
        ("a\\b", "\"a\\\\b\""),
        ("a\\\"b", "\"a\\\\\\\"b\""),
        (" leading ", "\" leading \""),
        ("a\r\nb", "\"a\r\nb\""),
        ("a\n\tb\u{b}\u{c}", "\"a\n\tb\u{b}\u{c}\""),
        ("e\u{301}雪🙂", "\"e\u{301}雪🙂\""),
    ] {
        let mut engine = engine(&format!(
            "(defglobal ?*text* = {} ?*result* = pending ?*called* = pending)
             (deffunction render (?value) (implode$ (create$ ?value)))
             (defrule exercise =>
               (bind ?*result* (implode$ (create$ ?*text*)))
               (bind ?*called* (render ?*text*)))",
            source_string(text)
        ));
        run(&mut engine);
        assert_eq!(string(&engine, "text").as_bytes(), text.as_bytes());
        assert_eq!(string(&engine, "result").as_bytes(), expected.as_bytes());
        assert_eq!(string(&engine, "called").as_bytes(), expected.as_bytes());
    }
}

#[test]
fn empty_flattened_and_sliced_values_return_strings_without_outer_parentheses() {
    let mut engine = engine(
        r#"(defglobal ?*empty* = pending ?*field* = pending ?*flat* = pending ?*slice* = pending)
           (defrule exercise =>
             (bind ?*empty* (implode$ (create$)))
             (bind ?*field* (implode$ (create$ "")))
             (bind ?*flat* (implode$ (create$ (create$ a "b c") (create$) (create$ d "e"))))
             (bind ?*slice* (implode$ (subseq$ (create$ before a "two words" 3 after) 2 4))))"#,
    );
    run(&mut engine);
    assert_eq!(string(&engine, "empty"), "");
    assert_eq!(string(&engine, "field"), "\"\"");
    assert_eq!(string(&engine, "flat"), "a \"b c\" d \"e\"");
    assert_eq!(string(&engine, "slice"), "a \"two words\" 3");
}

#[test]
fn generated_symbols_remain_raw_and_control_names_do_not_expand() {
    for spelling in [
        "",
        "two words",
        "a\"b",
        "a\\b",
        "(a)",
        "crlf",
        "tab",
        "vtab",
        "ff",
    ] {
        let mut engine = engine(&format!(
            "(defglobal ?*symbol* = (sym-cat {}) ?*result* = pending)
             (defrule exercise => (bind ?*result* (implode$ (create$ ?*symbol*))))",
            source_string(spelling)
        ));
        run(&mut engine);
        let Some(Value::Symbol(symbol)) = engine.get_global("symbol") else {
            panic!("expected SYMBOL")
        };
        assert_eq!(engine.resolve_core_symbol(*symbol), Some(spelling));
        assert_eq!(string(&engine, "result"), spelling);
    }
}

#[test]
fn float_spellings_use_literal_reference_results_without_changing_value_bits() {
    for (literal, expected) in [
        ("1.0", "1.0"),
        ("-0.0", "-0.0"),
        ("0.0001", "0.0001"),
        ("1.0e-5", "1e-05"),
        ("1.0e14", "100000000000000.0"),
        ("1.0e15", "1e+15"),
        ("1.0e20", "1e+20"),
        ("1.2345678901234567", "1.23456789012346"),
        ("5.0e-324", "4.94065645841247e-324"),
        ("0.00009999999999999994", "9.99999999999999e-05"),
        ("0.00009999999999999995", "0.0001"),
        ("999999999999999.4", "999999999999999.0"),
        ("999999999999999.5", "1e+15"),
        ("1.234567890123445", "1.23456789012344"),
        ("1.234567890123455", "1.23456789012345"),
    ] {
        let mut engine = engine(&format!(
            "(defglobal ?*number* = {literal} ?*result* = pending)
             (defrule exercise => (bind ?*result* (implode$ (create$ ?*number*))))"
        ));
        run(&mut engine);
        assert!(
            matches!(engine.get_global("number"), Some(Value::Float(value))
            if value.to_bits() == literal.parse::<f64>().unwrap().to_bits())
        );
        assert_eq!(string(&engine, "result"), expected);
    }
}

#[test]
fn integers_retain_exact_identity_and_decimal_spelling() {
    let mut engine = engine(
        "(defglobal ?*fields* = (create$ -9223372036854775808 9223372036854775807 9007199254740993)
                    ?*result* = pending)
         (defrule exercise => (bind ?*result* (implode$ ?*fields*)))",
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
        string(&engine, "result"),
        "-9223372036854775808 9223372036854775807 9007199254740993"
    );
}

#[test]
fn required_operand_evaluates_once_and_success_allows_later_effects() {
    for (operand, expected) in [("(create$ a \"b c\")", "a \"b c\""), ("(create$)", "")] {
        let mut engine = engine(&format!(
            "(defglobal ?*trace* = 0 ?*result* = pending ?*after* = 0)
             (deffunction mark (?value) (bind ?*trace* (+ ?*trace* 1)) ?value)
             (defrule exercise =>
               (bind ?*result* (implode$ (mark {operand})))
               (bind ?*after* 999))"
        ));
        run(&mut engine);
        assert_eq!(string(&engine, "result"), expected);
        assert!(matches!(
            engine.get_global("trace"),
            Some(Value::Integer(1))
        ));
        assert!(matches!(
            engine.get_global("after"),
            Some(Value::Integer(999))
        ));
    }
}

#[test]
fn create_fields_omits_void_before_implode_and_preserves_its_effect() {
    // Keep the operand effect outside an enclosing printout buffer. The sealed
    // nested-printout prefix ordering case belongs to the separate I/O protocol.
    let mut engine = engine(
        r#"(defglobal ?*result* = pending)
           (defrule exercise =>
             (bind ?*result* (implode$ (create$ a (printout t "effect" crlf) "b c"))))"#,
    );
    run(&mut engine);
    assert_eq!(string(&engine, "result"), "a \"b c\"");
    assert_eq!(engine.get_output("t").unwrap(), Some("effect\n"));
}

fn failure(expression: &str, trace: i64) -> Engine {
    let mut engine = engine(&format!(
        "(defglobal ?*trace* = 0 ?*result* = pending ?*after* = 0)
         (deffunction mark (?value) (bind ?*trace* (+ ?*trace* 1)) ?value)
         (deffunction nothing () (bind ?*trace* (+ ?*trace* 1)) (printout t \"effect\" crlf))
         (deffunction fail () (bind ?*trace* (+ ?*trace* 1)) (/ 1 0))
         (defrule exercise => (bind ?*result* {expression}) (bind ?*after* 999))"
    ));
    let result = engine.run(RunLimit::Count(10)).unwrap();
    assert_eq!(result.rules_fired, 1);
    assert_eq!(result.halt_reason, HaltReason::ActionError);
    assert!(matches!(engine.get_global("trace"), Some(Value::Integer(value)) if *value == trace));
    assert!(matches!(
        engine.get_global("after"),
        Some(Value::Integer(0))
    ));
    let Some(Value::Symbol(symbol)) = engine.get_global("result") else {
        panic!("failed bind must preserve its previous value")
    };
    assert_eq!(engine.resolve_core_symbol(*symbol), Some("pending"));
    assert!(!engine.action_diagnostics().is_empty());
    engine
}

#[test]
fn runtime_arity_gate_rejects_before_operand_effects() {
    // CLIPS rejects these source forms while loading; Ferric's existing gate
    // is evaluated at runtime. This test does not claim source-time parity.
    for expression in [
        "(implode$)",
        "(implode$ (mark (create$ a)) (mark (create$ b)))",
    ] {
        let engine = failure(expression, 0);
        assert!(engine
            .action_diagnostics()
            .iter()
            .any(|error| error.to_string().contains("implode$")));
    }
}

#[test]
fn dynamic_wrong_types_fail_after_one_operand_effect_and_skip_later_actions() {
    for expression in [
        "(implode$ (mark 7))",
        "(implode$ (mark 2.5))",
        "(implode$ (mark symbol))",
        "(implode$ (mark \"text\"))",
        "(implode$ (nothing))",
    ] {
        let engine = failure(expression, 1);
        assert!(engine
            .action_diagnostics()
            .iter()
            .any(|error| error.to_string().contains("implode$")));
    }
}

#[test]
fn operand_failure_propagates_without_an_implode_type_error() {
    let engine = failure("(implode$ (fail))", 1);
    assert!(engine
        .action_diagnostics()
        .iter()
        .any(|error| error.to_string().contains("zero")));
    assert!(engine
        .action_diagnostics()
        .iter()
        .all(|error| !error.to_string().contains("implode$")));
}

#[test]
fn neighboring_string_builders_keep_their_existing_unescaped_policy() {
    let mut engine = engine(
        r#"(defglobal ?*string* = pending ?*symbol* = pending ?*number* = pending)
           (defrule exercise =>
             (bind ?*string* (str-cat "a\"b" "|" "a\\b"))
             (bind ?*symbol* (sym-cat "a\"b" "|" "a\\b"))
             (bind ?*number* (str-cat 1.0e20)))"#,
    );
    run(&mut engine);
    assert_eq!(string(&engine, "string"), "a\"b|a\\b");
    let Some(Value::Symbol(symbol)) = engine.get_global("symbol") else {
        panic!("expected SYMBOL")
    };
    assert_eq!(engine.resolve_core_symbol(*symbol), Some("a\"b|a\\b"));
    // Neighboring numeric spelling is existing Ferric behavior, not a CLIPS
    // conformance assertion for str-cat in this implode$ repair.
    assert_eq!(string(&engine, "number"), "100000000000000000000.0");
}
