//! Error values follow the failing CLIPS 6.30 handler's state, not one universal
//! FALSE substitute. Expectations below follow bmathfun.c/GetNumericArgument and
//! `FuncallFunction`; successful extrema also retain the selected operand after
//! composition with the numeric-extrema repair.

use ferric_rules_core::Value;
use ferric_rules_runtime::{Engine, HaltReason, RunLimit};

#[derive(Clone, Copy, Debug)]
enum Expected {
    Integer(i64),
    Float(f64),
    Symbol(&'static str),
}

fn engine_for(expression: &str) -> Engine {
    Engine::with_rules(&format!(
        "(defglobal ?*trace* = 0 ?*count* = 0 ?*result* = pending)
         (deffunction mark (?digit ?value)
           (bind ?*trace* (+ (* ?*trace* 10) ?digit)) ?value)
         (defrule compute =>
           (bind ?*result* (sort > {expression})) (assert (after)))"
    ))
    .unwrap()
}

fn assert_error_value(expression: &str, expected: Expected, trace: i64) -> Engine {
    let mut engine = engine_for(expression);
    let run = engine.run(RunLimit::Count(10)).unwrap();
    assert_eq!(run.halt_reason, HaltReason::ActionError, "{expression}");
    assert_eq!(run.rules_fired, 1, "{expression}");
    assert!(!engine.action_diagnostics().is_empty(), "{expression}");
    assert!(engine.find_facts("after").unwrap().is_empty());
    assert!(
        matches!(engine.get_global("trace"), Some(Value::Integer(actual)) if *actual == trace),
        "{expression}: trace {:?}",
        engine.get_global("trace")
    );
    let Some(Value::Multifield(fields)) = engine.get_global("result") else {
        panic!("{expression}: expected retained MULTIFIELD")
    };
    assert_eq!(fields.len(), 1, "{expression}: {fields:?}");
    let matches = match (&fields[0], &expected) {
        (Value::Integer(actual), Expected::Integer(expected)) => actual == expected,
        (Value::Float(actual), Expected::Float(expected)) => actual.to_bits() == expected.to_bits(),
        (Value::Symbol(actual), Expected::Symbol(expected)) => {
            engine.resolve_core_symbol(*actual) == Some(*expected)
        }
        _ => false,
    };
    assert!(
        matches,
        "{expression}: actual {:?}, expected {expected:?}",
        fields[0]
    );
    engine
}

#[test]
fn addition_and_subtraction_retain_typed_partial_totals_and_stop_later_operands() {
    for (expression, expected, trace) in [
        ("(+ (mark 1 bad) (mark 2 9))", Expected::Integer(0), 1),
        (
            "(+ (mark 1 7) (mark 2 bad) (mark 3 9))",
            Expected::Integer(7),
            12,
        ),
        (
            "(- (mark 1 7) (mark 2 bad) (mark 3 9))",
            Expected::Integer(7),
            12,
        ),
        (
            "(+ (mark 1 7.5) (mark 2 bad) (mark 3 9))",
            Expected::Float(7.5),
            12,
        ),
        (
            "(- (mark 1 -0.0) (mark 2 bad) (mark 3 9))",
            Expected::Float(-0.0),
            12,
        ),
    ] {
        assert_error_value(expression, expected, trace);
    }
}

#[test]
fn error_totals_convert_the_exact_integer_prefix_only_when_float_is_reached() {
    for expression in [
        "(+ 9007199254740993 -9007199254740992 0.0 (mark 1 bad) (mark 2 9))",
        "(- 9007199254740993 9007199254740992 0.0 (mark 1 bad) (mark 2 9))",
    ] {
        assert_error_value(expression, Expected::Float(1.0), 1);
    }
}

#[test]
fn multiplication_folds_failed_numeric_zero_into_the_current_product() {
    for (expression, expected) in [
        (
            "(* (mark 1 -2) (mark 2 bad) (mark 3 9))",
            Expected::Integer(0),
        ),
        (
            "(* (mark 1 -2.0) (mark 2 bad) (mark 3 9))",
            Expected::Float(-0.0),
        ),
    ] {
        assert_error_value(expression, expected, 12);
    }
}

#[test]
fn division_distinguishes_first_type_failure_from_zero_divisor_defaults() {
    for (expression, expected, trace) in [
        ("(/ (mark 1 bad) (mark 2 9))", Expected::Float(1.0), 1),
        ("(div (mark 1 bad) (mark 2 9))", Expected::Integer(0), 1),
        ("(/ (mark 1 8) (mark 2 0))", Expected::Float(1.0), 12),
        ("(div (mark 1 8) (mark 2 0))", Expected::Integer(1), 12),
    ] {
        assert_error_value(expression, expected, trace);
    }
    for (expression, expected) in [
        ("(/ (mark 1 8) (mark 2 bad))", Expected::Float(1.0)),
        ("(div (mark 1 8) (mark 2 bad))", Expected::Integer(1)),
    ] {
        let engine = assert_error_value(expression, expected, 12);
        let diagnostics: Vec<_> = engine
            .action_diagnostics()
            .iter()
            .map(ToString::to_string)
            .collect();
        assert!(diagnostics.iter().any(|error| error.contains("type error")));
        assert!(diagnostics
            .iter()
            .any(|error| error.contains("division by zero")));
    }
}

#[test]
fn extrema_failures_keep_the_selected_operand_type_instead_of_promoting_it() {
    for (expression, expected, trace) in [
        (
            "(min (mark 1 7.5) (mark 2 3) (mark 3 bad) (mark 4 9))",
            Expected::Integer(3),
            123,
        ),
        (
            "(max (mark 1 7.5) (mark 2 9) (mark 3 bad) (mark 4 99))",
            Expected::Integer(9),
            123,
        ),
        (
            "(min (mark 1 7) (mark 2 2.5) (mark 3 bad))",
            Expected::Float(2.5),
            123,
        ),
        ("(max (mark 1 bad) (mark 2 9))", Expected::Integer(0), 1),
    ] {
        assert_error_value(expression, expected, trace);
    }
}

#[test]
fn extrema_observe_child_error_without_rechecking_or_duplicating_its_diagnostic() {
    for (expression, expected, trace) in [
        ("(min (/ 1 0) (bind ?*count* 7))", Expected::Integer(0), 0),
        (
            "(max (mark 1 7.5) (/ 1 0) (bind ?*count* 7))",
            Expected::Float(7.5),
            1,
        ),
    ] {
        let engine = assert_error_value(expression, expected, trace);
        assert!(matches!(
            engine.get_global("count"),
            Some(Value::Integer(0))
        ));
        assert_eq!(engine.action_diagnostics().len(), 1, "{expression}");
        assert!(engine.action_diagnostics()[0]
            .to_string()
            .contains("division by zero"));
    }
}

#[test]
fn funcall_stops_argument_collection_after_a_child_error() {
    let engine = assert_error_value(
        "(funcall + (/ 1 0) (bind ?*count* 7))",
        Expected::Symbol("FALSE"),
        0,
    );
    assert!(matches!(
        engine.get_global("count"),
        Some(Value::Integer(0))
    ));
    assert_eq!(engine.action_diagnostics().len(), 1);
}

#[test]
fn funcall_does_not_invoke_a_builtin_while_its_name_check_observes_error() {
    // A literal name still goes through EnvArgTypeCheck in CLIPS. It observes
    // the preceding division's EvaluationError before dispatching the target.
    let mut engine = engine_for("(/ 1 0) (funcall + 1 2)");
    let run = engine.run(RunLimit::Count(10)).unwrap();
    assert_eq!(run.halt_reason, HaltReason::ActionError);
    let Some(Value::Multifield(fields)) = engine.get_global("result") else {
        panic!("expected retained MULTIFIELD")
    };
    assert_eq!(fields.len(), 2);
    assert!(matches!(&fields[0], Value::Float(value) if value.to_bits() == 1.0_f64.to_bits()));
    let Value::Symbol(symbol) = &fields[1] else {
        panic!("funcall must return actual SYMBOL FALSE")
    };
    assert_eq!(engine.resolve_core_symbol(*symbol), Some("FALSE"));
    assert!(engine.find_facts("after").unwrap().is_empty());
}
