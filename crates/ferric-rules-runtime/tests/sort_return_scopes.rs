//! Measured `ReturnFlag` boundaries for sort; standalone prepared regressions.

use ferric_rules_core::Value;
use ferric_rules_runtime::{Engine, HaltReason, RunLimit};

fn run_expression(expression: &str, definitions: &str, trace: i64, after: bool) -> Engine {
    let mut engine = Engine::with_rules(&format!(
        "(defglobal ?*result* = pending ?*inner* = pending ?*trace* = 0
                    ?*after* = FALSE ?*second* = 0)
         (deffunction tap (?value) (bind ?*trace* (+ ?*trace* 1)) ?value)
         (deffunction exchange (?a ?b) (bind ?*trace* (+ ?*trace* 1)) (> ?a ?b))
         {definitions}
         (defrule compute (declare (salience 10)) =>
           (bind ?*result* {expression}) (bind ?*after* TRUE))
         (defrule later => (bind ?*second* 1))"
    ))
    .unwrap();
    let result = engine.run(RunLimit::Count(10)).unwrap();
    assert_eq!(result.halt_reason, HaltReason::AgendaEmpty);
    assert_eq!(result.rules_fired, 2);
    assert!(engine.action_diagnostics().is_empty());
    assert!(matches!(engine.get_global("trace"), Some(Value::Integer(actual)) if *actual == trace));
    assert!(matches!(
        engine.get_global("second"),
        Some(Value::Integer(1))
    ));
    let Some(Value::Symbol(symbol)) = engine.get_global("after") else {
        panic!("after must contain actual SYMBOL TRUE/FALSE")
    };
    assert_eq!(
        engine.resolve_core_symbol(*symbol),
        Some(if after { "TRUE" } else { "FALSE" })
    );
    engine
}

fn assert_fields(engine: &Engine, global: &str, expected: &[i64]) {
    let Some(Value::Multifield(values)) = engine.get_global(global) else {
        panic!("{global} must contain an actual multifield")
    };
    assert_eq!(values.len(), expected.len());
    assert!(values
        .iter()
        .zip(expected)
        .all(|(value, expected)| matches!(value, Value::Integer(actual) if actual == expected)));
}

#[test]
fn callable_boundary_consumes_return_and_preserves_completed_expression_value() {
    let engine = run_expression(
        "(wrapper)",
        "(deffunction wrapper () (sort return 3 1) (tap 99))",
        0,
        true,
    );
    assert_fields(&engine, "result", &[1, 3]);
    let engine = run_expression(
        "(wrapper)",
        "(deffunction wrapper () (bind ?*inner* (sort return 4 2 3 1)) (tap 99))",
        0,
        true,
    );
    assert_fields(&engine, "result", &[1, 3, 2, 4]);
    assert_fields(&engine, "inner", &[1, 3, 2, 4]);
    let engine = run_expression(
        "(wrapper)",
        "(deffunction wrapper () (length$ (sort return 3 1)) (tap 99))",
        0,
        true,
    );
    assert!(matches!(
        engine.get_global("result"),
        Some(Value::Integer(2))
    ));
}

#[test]
fn builtin_siblings_and_callbacks_preserve_return_until_the_rule_boundary() {
    for (expression, expected) in [
        ("(sort > (sort return 3 1) 2)", &[1, 2, 3][..]),
        ("(sort > (sort return 3 1) (+ 10 2))", &[1, 3, 12][..]),
        ("(create$ (sort return 3 1) (+ 10 2))", &[1, 3, 12][..]),
    ] {
        assert_fields(
            &run_expression(expression, "", 0, false),
            "result",
            expected,
        );
    }
}

#[test]
fn a_user_callback_runs_its_first_expression_then_consumes_incoming_return() {
    // The first callback returns its counter bind (1), before reaching >.
    // Later callbacks execute normally after that callable consumed ReturnFlag.
    for (expression, expected, trace) in [
        ("(sort exchange (sort return 3 1) 2)", &[2, 3, 1][..], 2),
        (
            "(sort exchange (sort return 4 2 3 1) 7)",
            &[2, 3, 1, 4, 7][..],
            6,
        ),
        ("(sort > (sort return 3 1) (tap 7))", &[1, 1, 3][..], 1),
        ("(create$ (sort return 3 1) (tap 7))", &[1, 3, 1][..], 1),
    ] {
        assert_fields(
            &run_expression(expression, "", trace, true),
            "result",
            expected,
        );
    }
    let engine = run_expression("(tap (sort return 3 1))", "", 1, true);
    assert!(matches!(
        engine.get_global("result"),
        Some(Value::Integer(1))
    ));
    let engine = run_expression(
        "(wrapper)",
        "(deffunction wrapper () (sort > (sort return 3 1) (tap 7)) (tap 99))",
        2,
        true,
    );
    assert!(matches!(
        engine.get_global("result"),
        Some(Value::Integer(99))
    ));
}

#[test]
fn a_cleared_evaluation_error_allows_a_match_while_halt_still_stops_the_current_rhs() {
    let declarations = "
        (deffunction broken (?a ?b) (/ 1 0))
        (defrule matched (item) (test (sort broken 3 1 2)) => (assert (matched)))
        (defrule follower (item) => (assert (followed)))";
    let mut host = Engine::with_rules(declarations).unwrap();
    host.assert_ordered("item", ()).unwrap();
    assert!(!host.action_diagnostics().is_empty());
    assert_eq!(host.agenda_len(), 2);
    let run = host.run(RunLimit::Count(10)).unwrap();
    assert_eq!(run.halt_reason, HaltReason::AgendaEmpty);
    assert_eq!(run.rules_fired, 2);
    assert_eq!(host.find_facts("matched").unwrap().len(), 1);
    assert_eq!(host.find_facts("followed").unwrap().len(), 1);

    let mut rhs = Engine::with_rules(&format!(
        "{declarations}
        (defrule trigger => (assert (item)) (assert (after-assert)))"
    ))
    .unwrap();
    let first = rhs.run(RunLimit::Count(10)).unwrap();
    assert_eq!(first.halt_reason, HaltReason::ActionError);
    assert_eq!(first.rules_fired, 1);
    assert!(!rhs.action_diagnostics().is_empty());
    assert!(rhs.find_facts("after-assert").unwrap().is_empty());
    assert!(rhs.find_facts("matched").unwrap().is_empty());
    assert!(rhs.find_facts("followed").unwrap().is_empty());
    let second = rhs.run(RunLimit::Count(10)).unwrap();
    assert_eq!(second.halt_reason, HaltReason::AgendaEmpty);
    assert_eq!(second.rules_fired, 2);
    assert_eq!(rhs.find_facts("matched").unwrap().len(), 1);
    assert_eq!(rhs.find_facts("followed").unwrap().len(), 1);
}

#[test]
fn explicit_return_consumes_nested_sort_return_and_preserves_failed_callable_false() {
    let engine = run_expression(
        "(wrapper)",
        "(deffunction wrapper () (return (sort return 3 1)))",
        0,
        true,
    );
    assert_fields(&engine, "result", &[1, 3]);

    let mut engine = Engine::with_rules(
        "(defglobal ?*result* = pending)
         (deffunction broken (?a ?b) (return (/ 1 0)))
         (defrule compute =>
           (bind ?*result* (sort broken 3 1)) (assert (after)))",
    )
    .unwrap();
    let run = engine.run(RunLimit::Count(10)).unwrap();
    assert_eq!(run.halt_reason, HaltReason::ActionError);
    assert_eq!(run.rules_fired, 1);
    assert!(engine.find_facts("after").unwrap().is_empty());
    assert_eq!(engine.action_diagnostics().len(), 1);
    assert_fields(&engine, "result", &[3, 1]);
}
