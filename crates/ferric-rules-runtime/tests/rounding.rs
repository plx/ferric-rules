//! Public API rounding regressions that inspect stored INTEGER values directly.

use ferric_rules_core::{Fact, Value};
use ferric_rules_runtime::{Engine, HaltReason, RunLimit};

fn result_integers(engine: &Engine) -> Vec<i64> {
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
            other => panic!("round must return INTEGER, got {other:?}"),
        })
        .collect()
}

fn rounded_values(expressions: &str) -> Vec<i64> {
    let source = format!("(defrule compute => (assert (result {expressions})))");
    let mut engine = Engine::with_rules(&source).unwrap();
    let run = engine.run(RunLimit::Count(10)).unwrap();
    assert_eq!(run.halt_reason, HaltReason::AgendaEmpty);
    assert_eq!(run.rules_fired, 1);
    assert!(
        engine.action_diagnostics().is_empty(),
        "{:?}",
        engine.action_diagnostics()
    );
    result_integers(&engine)
}

#[test]
fn integer_arguments_preserve_exact_identity_through_assertion() {
    assert_eq!(
        rounded_values(
            "(round 0) (round 7) (round -7)
             (round 9007199254740993) (round -9007199254740993)
             (round 9223372036854775807) (round -9223372036854775808)"
        ),
        [
            0,
            7,
            -7,
            9_007_199_254_740_993,
            -9_007_199_254_740_993,
            i64::MAX,
            i64::MIN,
        ]
    );
}

#[test]
fn half_ties_and_ordinary_floats_return_integer_results() {
    assert_eq!(
        rounded_values(
            "(round 2.5) (round -2.5) (round 2.49) (round -2.49)
             (round -3.5) (round -1.5) (round -0.5)
             (round 0.5) (round 1.5) (round 3.5)
             (round 2.51) (round -2.51) (round -0.0)"
        ),
        [2, -3, 2, -2, -4, -2, -1, 0, 1, 3, 3, -3, 0]
    );
}

#[test]
fn adjacent_floats_preserve_reference_subtraction_rounding() {
    // Reviewed CLIPS results, including -0.49999999999999994 -> -1.
    // Expected values are literal oracles, not recomputed with the implementation.
    assert_eq!(
        rounded_values(
            "(round 2.4999999999999996) (round 2.5) (round 2.5000000000000004)
             (round -2.5000000000000004) (round -2.5) (round -2.4999999999999996)
             (round 0.49999999999999994) (round 0.5) (round 0.5000000000000001)
             (round -0.5000000000000001) (round -0.5) (round -0.49999999999999994)"
        ),
        [2, 2, 3, -3, -3, -2, 0, 0, 1, -1, -1, -1]
    );
    assert_eq!(
        rounded_values(
            "(round 4503599627370496.0) (round 4503599627370497.0)
             (round 4503599627370498.0) (round -4503599627370496.0)
             (round -4503599627370497.0) (round -4503599627370498.0)"
        ),
        [
            4_503_599_627_370_496,
            4_503_599_627_370_496,
            4_503_599_627_370_498,
            -4_503_599_627_370_496,
            -4_503_599_627_370_498,
            -4_503_599_627_370_498,
        ]
    );
}

#[test]
fn numeric_argument_is_evaluated_once_before_rounding() {
    let mut engine = Engine::with_rules(
        "(defglobal ?*calls* = 0)
         (deffunction mark (?value)
           (bind ?*calls* (+ ?*calls* 1)) ?value)
         (defrule compute =>
           (assert (result (round (mark 2.5))
                           (round (mark 9007199254740993)))))",
    )
    .unwrap();
    let run = engine.run(RunLimit::Count(10)).unwrap();
    assert_eq!(run.halt_reason, HaltReason::AgendaEmpty);
    assert_eq!(run.rules_fired, 1);
    assert!(engine.action_diagnostics().is_empty());
    assert!(matches!(
        engine.get_global("calls"),
        Some(Value::Integer(2))
    ));
    assert_eq!(result_integers(&engine), [2, 9_007_199_254_740_993]);
}

#[test]
fn invalid_types_and_arities_preserve_evaluation_boundaries() {
    for (arguments, calls, expected_error) in [
        ("(mark \"2.5\")", 1, "INTEGER or FLOAT"),
        ("(mark wrong)", 1, "INTEGER or FLOAT"),
        ("(mark (create$ 2 3))", 1, "INTEGER or FLOAT"),
        ("", 0, "expected 1, got 0"),
        ("(mark 2.5) (mark 3.5)", 0, "expected 1, got 2"),
    ] {
        let source = format!(
            "(defglobal ?*calls* = 0)
             (deffunction mark (?value)
               (bind ?*calls* (+ ?*calls* 1)) ?value)
             (defrule compute =>
               (assert (result (round {arguments})))
               (assert (after)))"
        );
        let mut engine = Engine::with_rules(&source).unwrap();
        let run = engine.run(RunLimit::Count(10)).unwrap();
        assert_eq!(run.halt_reason, HaltReason::ActionError, "{source}");
        assert!(
            engine.action_diagnostics().iter().any(|error| {
                let message = error.to_string();
                message.contains("round") && message.contains(expected_error)
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
