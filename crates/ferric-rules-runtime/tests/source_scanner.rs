//! Source facts and the runtime field scanner share CLIPS numeric tokenization.

use ferric_rules_core::Fact;
use ferric_rules_runtime::{Engine, EngineConfig, HaltReason, LoadError, RunLimit, Value};

fn single_field(engine: &Engine, relation: &str) -> Value {
    let facts = engine.find_facts(relation).unwrap();
    assert_eq!(facts.len(), 1, "{relation}");
    let Fact::Ordered(fact) = facts[0].1 else {
        panic!("{relation} must be an ordered fact");
    };
    assert_eq!(fact.fields.len(), 1, "{relation}: {:?}", fact.fields);
    fact.fields[0].clone()
}

fn source_and_explode(lexeme: &str) -> (Engine, Value) {
    let mut engine = Engine::new(EngineConfig::utf8());
    engine
        .load_str(&format!(
            "(deffacts seed (source {lexeme}))
             (defrule scan => (assert (scanned (explode$ \"{lexeme}\"))))"
        ))
        .unwrap();
    engine.reset().unwrap();
    let result = engine.run(RunLimit::Unlimited).unwrap();
    assert_eq!(result.rules_fired, 1, "{lexeme}");
    assert_eq!(result.halt_reason, HaltReason::AgendaEmpty);
    assert!(engine.action_diagnostics().is_empty(), "{lexeme}");
    let source = single_field(&engine, "source");
    let scanned = single_field(&engine, "scanned");
    assert!(
        source.structural_eq(&scanned),
        "{lexeme}: {source:?} != {scanned:?}"
    );
    (engine, source)
}

#[test]
fn source_numbers_match_explode_with_integer_boundaries_and_saturation() {
    for (lexeme, expected) in [
        ("1.", Value::Float(1.0)),
        ("1.e3", Value::Float(1000.0)),
        (".5", Value::Float(0.5)),
        ("+.5", Value::Float(0.5)),
        ("-.5", Value::Float(-0.5)),
        ("-.5e1", Value::Float(-5.0)),
        ("1000000.", Value::Float(1_000_000.0)),
        (".9", Value::Float(0.9)),
        ("9223372036854775807", Value::Integer(i64::MAX)),
        ("-9223372036854775808", Value::Integer(i64::MIN)),
        ("9223372036854775808", Value::Integer(i64::MAX)),
        ("-9223372036854775809", Value::Integer(i64::MIN)),
        ("99999999999999999999", Value::Integer(i64::MAX)),
        ("+99999999999999999999", Value::Integer(i64::MAX)),
        ("-99999999999999999999", Value::Integer(i64::MIN)),
    ] {
        let (_, actual) = source_and_explode(lexeme);
        assert!(actual.structural_eq(&expected), "{lexeme}: {actual:?}");
    }
}

#[test]
fn number_like_symbols_remain_single_fields_in_source_and_explode() {
    for lexeme in [
        "1st", "5th", "2nd", "0x10", "12abc", "1.abc", "3.14.15", "1-2", "1e5x", "1e5.5", "5f",
        "5e", "1.5e", "1e+", "1e-", "12é", "-.5λ", "1e漢字",
    ] {
        let (engine, actual) = source_and_explode(lexeme);
        let Value::Symbol(symbol) = actual else {
            panic!("{lexeme} must be a symbol, got {actual:?}");
        };
        assert_eq!(engine.resolve_core_symbol(symbol), Some(lexeme));
    }
}

#[test]
fn source_integer_overflow_is_saturated_without_scanner_notices() {
    let mut engine = Engine::new(EngineConfig::default());
    let loaded = engine
        .load_str(
            "(deffacts seed
               (positive 99999999999999999999)
               (negative -99999999999999999999))",
        )
        .unwrap();
    assert!(loaded.warnings.is_empty());
    for channel in ["t", "wwarning", "werror"] {
        assert!(engine.get_output(channel).unwrap_or_default().is_empty());
    }
    engine.reset().unwrap();
    assert!(single_field(&engine, "positive").structural_eq(&Value::Integer(i64::MAX)));
    assert!(single_field(&engine, "negative").structural_eq(&Value::Integer(i64::MIN)));
}

#[test]
fn load_facts_uses_the_same_scanner_for_ordered_and_template_fields() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("numeric.fct");
    std::fs::write(
        &path,
        "; CR-only facts\r(ordered 1st .5 5e 99999999999999999999 12é)\r\
         ; template follows\r(item (n 1.e3) (tags 0x10 -.5e1 1e+))\r",
    )
    .unwrap();
    let escaped = path
        .to_str()
        .unwrap()
        .replace('\\', "\\\\")
        .replace('"', "\\\"");
    let mut engine = Engine::with_rules_config(
        &format!(
            "(deftemplate item (slot n) (multislot tags))
             (defrule read => (load-facts \"{escaped}\"))
             (defrule check (ordered $?fields) (item (n ?n) (tags $?tags))
               => (printout t ?fields crlf ?n \" \" ?tags crlf))"
        ),
        EngineConfig::utf8(),
    )
    .unwrap();
    let result = engine.run(RunLimit::Unlimited).unwrap();
    assert_eq!(result.rules_fired, 2);
    assert_eq!(result.halt_reason, HaltReason::AgendaEmpty);
    assert!(engine.action_diagnostics().is_empty());
    assert_eq!(
        engine.get_output("t"),
        Some("(1st 0.5 5e 9223372036854775807 12é)\n1000.0 (0x10 -5.0 1e+)\n")
    );
}

#[test]
fn cr_comments_end_and_diagnostic_lines_match_other_line_endings() {
    for newline in ["\r", "\n", "\r\n"] {
        let mut engine = Engine::with_rules(&format!(
            "; leading comment{newline}\
             (defrule r => (printout t \"fired\" crlf)); trailing comment{newline}"
        ))
        .unwrap();
        assert_eq!(engine.run(RunLimit::Unlimited).unwrap().rules_fired, 1);
        assert_eq!(engine.get_output("t"), Some("fired\n"));

        let errors = engine
            .load_str(&format!("; first{newline}; second{newline}){newline}"))
            .expect_err("an unmatched parenthesis after the comments must be diagnosed");
        let LoadError::Parse(error) = &errors[0] else {
            panic!("expected a parse error: {errors:?}");
        };
        assert_eq!(error.span.start.line, 3, "{newline:?}");
        assert_eq!(error.span.start.column, 1, "{newline:?}");
    }
}
