//! Public API controls for variadic numeric comparison and operand evaluation.

use ferric_rules_core::{Fact, Value};
use ferric_rules_runtime::{Engine, HaltReason, RunLimit};

const COMPARISONS: [&str; 7] = ["=", "!=", "<>", "<", ">", "<=", ">="];

fn comparison_engine(expressions: &str) -> Engine {
    let source = format!(
        "(defglobal ?*trace* = 0)
         (deffunction mark (?digit ?value)
           (bind ?*trace* (+ (* ?*trace* 10) ?digit)) ?value)
         (defrule compute =>
           (assert (result {expressions}))
           (assert (after)))"
    );
    Engine::with_rules(&source).unwrap()
}

fn result_booleans(engine: &Engine) -> Vec<bool> {
    let results = engine.find_facts("result").unwrap();
    assert_eq!(results.len(), 1);
    let Fact::Ordered(result) = results[0].1 else {
        panic!("result must be an ordered fact")
    };
    result
        .fields
        .iter()
        .map(|value| {
            let Value::Symbol(symbol) = value else {
                panic!("comparison must return a symbol, got {value:?}")
            };
            match engine.resolve_core_symbol(*symbol) {
                Some("TRUE") => true,
                Some("FALSE") => false,
                other => panic!("comparison must return TRUE or FALSE, got {other:?}"),
            }
        })
        .collect()
}

fn run_success(expressions: &str) -> Engine {
    let mut engine = comparison_engine(expressions);
    let run = engine.run(RunLimit::Count(10)).unwrap();
    assert_eq!(run.halt_reason, HaltReason::AgendaEmpty, "{expressions}");
    assert_eq!(run.rules_fired, 1);
    assert!(
        engine.action_diagnostics().is_empty(),
        "{expressions}: {:?}",
        engine.action_diagnostics()
    );
    assert_eq!(engine.find_facts("after").unwrap().len(), 1);
    engine
}

fn assert_trace(engine: &Engine, expected: i64) {
    assert!(
        matches!(engine.get_global("trace"), Some(Value::Integer(actual)) if *actual == expected),
        "trace: {:?}",
        engine.get_global("trace")
    );
}

fn assert_failed_expression(engine: &mut Engine, expected_error: &str) {
    let run = engine.run(RunLimit::Count(10)).unwrap();
    assert_eq!(run.halt_reason, HaltReason::ActionError);
    assert!(
        engine
            .action_diagnostics()
            .iter()
            .any(|error| error.to_string().contains(expected_error)),
        "{:?}",
        engine.action_diagnostics()
    );
    assert!(engine.find_facts("result").unwrap().is_empty());
    assert!(engine.find_facts("after").unwrap().is_empty());
}

#[test]
fn minimum_arity_is_checked_before_any_operand_evaluation() {
    for operator in COMPARISONS {
        for arguments in ["", "(mark 1 9)"] {
            let mut engine = comparison_engine(&format!("({operator} {arguments})"));
            assert_failed_expression(&mut engine, "expected 2+");
            assert_trace(&engine, 0);
        }
    }
}

#[test]
fn successful_chains_evaluate_once_in_order_and_false_prefixes_stop() {
    // Literal CLIPS control vectors; inequality's repeated later operands are valid.
    for (operator, true_values, false_values) in [
        ("=", [1, 1, 1], [1, 2, 2]),
        ("!=", [1, 2, 2], [1, 1, 2]),
        ("<>", [1, 2, 2], [1, 1, 2]),
        ("<", [1, 2, 3], [2, 1, 3]),
        (">", [3, 2, 1], [1, 2, 0]),
        ("<=", [1, 2, 2], [2, 1, 3]),
        (">=", [3, 2, 2], [1, 2, 0]),
    ] {
        for (values, expected, trace) in [(true_values, true, 123), (false_values, false, 12)] {
            let [first, second, third] = values;
            let expression =
                format!("({operator} (mark 1 {first}) (mark 2 {second}) (mark 3 {third}))");
            let engine = run_success(&expression);
            assert_eq!(result_booleans(&engine), [expected], "{expression}");
            assert_trace(&engine, trace);
        }
    }
}

#[test]
fn bad_later_types_are_skipped_or_reached_at_the_comparison_boundary() {
    for (operator, passing, failing) in [
        ("=", [1, 1], [1, 2]),
        ("!=", [1, 2], [1, 1]),
        ("<>", [1, 2], [1, 1]),
        ("<", [1, 2], [2, 1]),
        (">", [2, 1], [1, 2]),
        ("<=", [1, 2], [2, 1]),
        (">=", [2, 1], [1, 2]),
    ] {
        for bad_value in ["wrong", "\"2\"", "(create$ 2 3)"] {
            let [first, second] = failing;
            let skipped = format!(
                "({operator} (mark 1 {first}) (mark 2 {second})
                   (mark 3 {bad_value}) (mark 4 9))"
            );
            let engine = run_success(&skipped);
            assert_eq!(result_booleans(&engine), [false], "{skipped}");
            assert_trace(&engine, 12);

            let [first, second] = passing;
            let reached = format!(
                "({operator} (mark 1 {first}) (mark 2 {second})
                   (mark 3 {bad_value}) (mark 4 9))"
            );
            let mut engine = comparison_engine(&reached);
            assert_failed_expression(&mut engine, "INTEGER or FLOAT");
            assert_trace(&engine, 123);
        }
    }
}

#[test]
fn each_numeric_argument_is_validated_before_evaluating_the_next() {
    for operator in COMPARISONS {
        for (arguments, trace) in [
            ("(mark 1 wrong) (mark 2 9) (mark 3 9)", 1),
            ("(mark 1 9) (mark 2 wrong) (mark 3 9)", 12),
        ] {
            let mut engine = comparison_engine(&format!("({operator} {arguments})"));
            assert_failed_expression(&mut engine, "INTEGER or FLOAT");
            assert_trace(&engine, trace);
        }
    }
}

#[test]
fn equality_and_inequality_use_first_anchor_while_ordering_uses_neighbors() {
    let engine = run_success(
        "(= 2 2.0 2) (<> 1 2 2) (!= 1 2 2) (<> 1 2 1) (!= 1 2 1)
         (= 9007199254740992.0 9007199254740993 9007199254740992)
         (= 9007199254740993 9007199254740992.0 9007199254740992)
         (< 1 2 2) (> 3 2 2) (<= 1 2 2) (>= 3 2 2)
         (< 1 3 2) (> 3 1 2) (<= 1 3 2) (>= 3 1 2)",
    );
    assert_eq!(
        result_booleans(&engine),
        [
            true, true, true, false, false, true, false, false, false, true, true, false, false,
            false, false,
        ]
    );
}

#[test]
fn binary_comparisons_preserve_exact_integers_and_ordinary_float_equality() {
    let engine = run_success(
        "(= 2 2.0) (= 2 3) (!= 2 3) (!= 2 2.0) (<> 2 3) (<> 2 2.0)
         (< 2 3) (< 2 2) (> 3 2) (> 2 2) (<= 2 2.0) (<= 3 2)
         (>= 2 2.0) (>= 2 3)
         (= 9007199254740992 9007199254740993)
         (< 9007199254740992 9007199254740993)
         (> 9007199254740993 9007199254740992)
         (<= 9007199254740993 9007199254740992)
         (>= 9007199254740992 9007199254740993)
         (= 0.0 1e-20) (!= 0.0 1e-20) (<> 0.0 1e-20) (= -0.0 0.0)
         (< -9223372036854775808 9223372036854775807)
         (> 9223372036854775807 -9223372036854775808)",
    );
    assert_eq!(
        result_booleans(&engine),
        [
            true, false, true, false, true, false, true, false, true, false, true, false, true,
            false, false, true, true, false, false, false, true, true, true, true, true,
        ]
    );
}

#[test]
fn callable_test_ce_and_positive_slot_predicate_use_variadic_comparison() {
    let mut engine = Engine::with_rules(
        "(deffunction in-range (?value) (< 0 ?value 10))
         (deffacts input (number -1) (number 5) (number 11))
         (defrule with-test
           (number ?value) (test (<= 0 ?value 10))
           => (assert (test-hit ?value)))
         (defrule with-predicate
           (number ?value&:(< 0 ?value 10))
           => (assert (predicate-hit ?value)))
         (defrule with-callable
           (number ?value) (test (in-range ?value))
           => (assert (callable-hit ?value)))",
    )
    .unwrap();
    let run = engine.run(RunLimit::Count(10)).unwrap();
    assert_eq!(run.halt_reason, HaltReason::AgendaEmpty);
    assert_eq!(run.rules_fired, 3);
    assert!(engine.action_diagnostics().is_empty());
    for relation in ["test-hit", "predicate-hit", "callable-hit"] {
        let facts = engine.find_facts(relation).unwrap();
        assert_eq!(facts.len(), 1, "{relation}");
        let Fact::Ordered(fact) = facts[0].1 else {
            panic!("{relation} must be ordered")
        };
        assert!(matches!(fact.fields.as_slice(), [Value::Integer(5)]));
    }
}
