//! Public API regressions for STRING and SYMBOL length results.

use ferric_rules_core::{Fact, Value};
use ferric_rules_runtime::{Engine, EngineConfig, HaltReason, RunLimit};

fn result_lengths(engine: &Engine) -> Vec<i64> {
    let results = engine.find_facts("result").unwrap();
    assert_eq!(results.len(), 1);
    let Fact::Ordered(result) = results[0].1 else {
        panic!("result must be an ordered fact")
    };
    result
        .fields
        .iter()
        .map(|value| match value {
            Value::Integer(value) => *value,
            other => panic!("str-length must return INTEGER, got {other:?}"),
        })
        .collect()
}

fn run_success(engine: &mut Engine) {
    let run = engine.run(RunLimit::Count(10)).unwrap();
    assert_eq!(run.halt_reason, HaltReason::AgendaEmpty);
    assert_eq!(run.rules_fired, 1);
    assert!(
        engine.action_diagnostics().is_empty(),
        "{:?}",
        engine.action_diagnostics()
    );
}

#[test]
fn literal_strings_and_symbols_produce_stored_integer_lengths() {
    let mut engine = Engine::with_rules(
        r#"(defrule compute =>
             (assert (result
               (str-length "") (str-length "abc") (str-length abc)
               (str-length a) (str-length AbC) (str-length abc-def)
               (str-length TRUE) (str-length FALSE))))"#,
    )
    .unwrap();
    run_success(&mut engine);
    assert_eq!(result_lengths(&engine), [0, 3, 3, 1, 3, 7, 4, 5]);
}

#[test]
fn bound_and_generated_lexemes_keep_their_spelling_through_callables() {
    let mut engine = Engine::with_rules(
        r#"(deffunction measure (?value) (str-length ?value))
           (deffacts input (word abc) (text "wxyz"))
           (defrule compute
             (word ?symbol) (text ?string)
             => (assert (result
               (measure ?symbol) (measure ?string) (measure "")
               (measure (sym-cat "alpha" "-" "beta"))
               (measure (sym-cat "a b")))))"#,
    )
    .unwrap();
    run_success(&mut engine);
    assert_eq!(result_lengths(&engine), [3, 4, 0, 10, 3]);
}

#[test]
fn existing_utf8_string_character_count_is_preserved() {
    // This preserves the existing str_length_counts_utf8_characters unit test.
    // It does not introduce a new Unicode SYMBOL compatibility policy.
    let mut engine = Engine::with_rules_config(
        "(defrule compute => (assert (result (str-length \"é\"))))",
        EngineConfig::utf8(),
    )
    .unwrap();
    run_success(&mut engine);
    assert_eq!(result_lengths(&engine), [1]);
}

#[test]
fn string_and_symbol_arguments_are_each_evaluated_once() {
    let mut engine = Engine::with_rules(
        r#"(defglobal ?*calls* = 0)
           (deffunction mark (?value)
             (bind ?*calls* (+ ?*calls* 1)) ?value)
           (defrule compute =>
             (assert (result (str-length (mark abc))
                             (str-length (mark "abc")))))"#,
    )
    .unwrap();
    run_success(&mut engine);
    assert!(matches!(
        engine.get_global("calls"),
        Some(Value::Integer(2))
    ));
    assert_eq!(result_lengths(&engine), [3, 3]);
}

#[test]
fn nonlexeme_types_and_wrong_arities_keep_evaluation_boundaries() {
    for (arguments, calls, expected_error) in [
        ("(mark 42)", 1, "STRING or SYMBOL"),
        ("(mark 2.5)", 1, "STRING or SYMBOL"),
        ("(mark (create$ a b))", 1, "STRING or SYMBOL"),
        ("", 0, "expected 1, got 0"),
        ("(mark abc) (mark \"def\")", 0, "expected 1, got 2"),
    ] {
        let source = format!(
            "(defglobal ?*calls* = 0)
             (deffunction mark (?value)
               (bind ?*calls* (+ ?*calls* 1)) ?value)
             (defrule compute =>
               (assert (result (str-length {arguments})))
               (assert (after)))"
        );
        let mut engine = Engine::with_rules(&source).unwrap();
        let run = engine.run(RunLimit::Count(10)).unwrap();
        assert_eq!(run.halt_reason, HaltReason::ActionError, "{source}");
        assert!(
            engine.action_diagnostics().iter().any(|error| {
                let message = error.to_string();
                message.contains("str-length") && message.contains(expected_error)
            }),
            "{source}: {:?}",
            engine.action_diagnostics()
        );
        assert!(
            matches!(engine.get_global("calls"), Some(Value::Integer(actual)) if *actual == calls),
            "{source}"
        );
        assert!(engine.find_facts("result").unwrap().is_empty());
        assert!(engine.find_facts("after").unwrap().is_empty());
    }
}
