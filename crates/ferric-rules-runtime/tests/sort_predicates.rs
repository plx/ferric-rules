//! Public API controls for sort's values, invocation order, and failure classes.
//!
//! Requires the #334 numeric predicate repair. Nonfatal diagnostics must be
//! visible through `Engine::action_diagnostics` while preserving `AgendaEmpty`,
//! FALSE, continuation, and skipped data; a fatal action error is not equivalent.
//! Fatal sort errors also carry their measured return value through enclosing
//! expressions. The separate pinned CLIPS Void-data process faults are never
//! executed here. See the prepared failure-transport addendum for exact oracles.

use ferric_rules_core::Value;
use ferric_rules_runtime::{Engine, HaltReason, RunLimit};

fn sort_engine(arguments: &str, definitions: &str) -> Engine {
    expression_engine(&format!("(sort {arguments})"), definitions)
}

fn expression_engine(expression: &str, definitions: &str) -> Engine {
    body_engine(
        &format!("(bind ?*result* {expression}) (assert (after))"),
        definitions,
    )
}

fn body_engine(body: &str, definitions: &str) -> Engine {
    Engine::with_rules(&format!(
        "(defglobal ?*trace* = 0 ?*result* = pending)
         (deffunction mark (?digit ?value)
           (bind ?*trace* (+ (* ?*trace* 10) ?digit)) ?value)
         (deffunction fail (?digit)
           (bind ?*trace* (+ (* ?*trace* 10) ?digit)) (/ 1 0))
         {definitions}
         (defrule compute => {body})"
    ))
    .unwrap()
}

fn assert_trace(engine: &Engine, expected: i64) {
    assert!(
        matches!(engine.get_global("trace"), Some(Value::Integer(actual)) if *actual == expected),
        "trace: {:?}",
        engine.get_global("trace")
    );
}

fn run_success(engine: &mut Engine) {
    let result = engine.run(RunLimit::Count(10)).unwrap();
    assert_eq!(result.halt_reason, HaltReason::AgendaEmpty);
    assert_eq!(result.rules_fired, 1);
    assert_eq!(engine.find_facts("after").unwrap().len(), 1);
}

fn assert_integer_fields(engine: &mut Engine, expected: &[i64]) {
    run_success(engine);
    assert!(
        engine.action_diagnostics().is_empty(),
        "{:?}",
        engine.action_diagnostics()
    );
    assert_stored_fields(
        engine,
        &expected
            .iter()
            .copied()
            .map(Field::Integer)
            .collect::<Vec<_>>(),
    );
}

#[derive(Debug)]
enum Field<'a> {
    Integer(i64),
    Float(f64),
    Symbol(&'a str),
}

fn assert_stored_fields(engine: &Engine, expected: &[Field<'_>]) {
    let Some(Value::Multifield(values)) = engine.get_global("result") else {
        panic!(
            "sort must return an actual MULTIFIELD: {:?}",
            engine.get_global("result")
        )
    };
    assert_eq!(values.len(), expected.len());
    for (actual, expected) in values.iter().zip(expected) {
        let same = match (actual, expected) {
            (Value::Integer(actual), Field::Integer(expected)) => actual == expected,
            (Value::Float(actual), Field::Float(expected)) => {
                actual.to_bits() == expected.to_bits()
            }
            (Value::Symbol(actual), Field::Symbol(expected)) => {
                engine.resolve_core_symbol(*actual) == Some(*expected)
            }
            _ => false,
        };
        assert!(same, "actual: {actual:?}; expected: {expected:?}");
    }
}

fn assert_global_symbol(engine: &Engine, name: &str, expected: &str) {
    let Some(Value::Symbol(symbol)) = engine.get_global(name) else {
        panic!(
            "expected actual SYMBOL {expected}, got {:?}",
            engine.get_global(name)
        )
    };
    assert_eq!(engine.resolve_core_symbol(*symbol), Some(expected));
}

fn assert_failure(engine: &mut Engine, expected_error: &str) {
    let result = engine.run(RunLimit::Count(10)).unwrap();
    assert_eq!(result.halt_reason, HaltReason::ActionError);
    assert!(
        engine
            .action_diagnostics()
            .iter()
            .any(|error| error.to_string().contains(expected_error)),
        "{:?}",
        engine.action_diagnostics()
    );
    assert!(engine.find_facts("after").unwrap().is_empty());
}

fn assert_nonfatal_name_failure(engine: &mut Engine) {
    run_success(engine);
    let Some(Value::Symbol(symbol)) = engine.get_global("result") else {
        panic!("invalid comparator must return actual FALSE, not an empty list")
    };
    assert_eq!(engine.resolve_core_symbol(*symbol), Some("FALSE"));
    assert_trace(engine, 1);
    // The existing public diagnostic list observes the warning; its presence
    // must not be confused with the fatal execute_actions error vector.
    assert!(
        engine
            .action_diagnostics()
            .iter()
            .any(|diagnostic| diagnostic.to_string().contains("sort")),
        "a nonfatal sort diagnostic must be observable: {:?}",
        engine.action_diagnostics()
    );
}

#[test]
fn variadic_inputs_flatten_into_actual_multifields() {
    for (arguments, expected) in [
        ("<", &[][..]),
        ("< (create$)", &[][..]),
        ("< 7", &[7][..]),
        ("> 3 (create$ 1 4) (create$) 2", &[1, 2, 3, 4][..]),
    ] {
        assert_integer_fields(&mut sort_engine(arguments, ""), expected);
    }
}

#[test]
fn builtin_and_user_predicates_share_exchange_direction() {
    let definitions = "(deffunction descending (?a ?b) (< ?a ?b))
                       (deffunction ascending (?a ?b) (> ?a ?b))";
    for (predicate, expected) in [
        ("<", &[3, 2, 1, 1][..]),
        ("descending", &[3, 2, 1, 1][..]),
        (">", &[1, 1, 2, 3][..]),
        ("ascending", &[1, 1, 2, 3][..]),
    ] {
        assert_integer_fields(
            &mut sort_engine(&format!("{predicate} (create$ 3 1 2 1)"), definitions),
            expected,
        );
    }
}

#[test]
fn equal_numeric_keys_are_stable_and_keep_their_value_types() {
    let mut engine = sort_engine(
        "> (create$ 2 1.0 1 2.0 9007199254740993 9007199254740992)",
        "",
    );
    run_success(&mut engine);
    assert!(engine.action_diagnostics().is_empty());
    let Some(Value::Multifield(values)) = engine.get_global("result") else {
        panic!("expected MULTIFIELD")
    };
    assert!(matches!(values.as_slice(), [
        Value::Float(a), Value::Integer(1), Value::Integer(2), Value::Float(b),
        Value::Integer(9_007_199_254_740_992), Value::Integer(9_007_199_254_740_993)
    ] if a.to_bits() == 1.0_f64.to_bits() && b.to_bits() == 2.0_f64.to_bits()));
    let definitions = "(deffunction exchange (?a ?b) (> (div ?a 10) (div ?b 10)))";
    assert_integer_fields(
        &mut sort_engine("exchange (create$ 21 11 22 12 23)", definitions),
        &[11, 12, 21, 22, 23],
    );
}

#[test]
fn merge_traversal_calls_each_head_pair_once_in_clips_order() {
    let definitions = "(defglobal ?*calls* = \"\")
         (deffunction exchange (?a ?b)
           (bind ?*calls* (str-cat ?*calls* ?a \":\" ?b \";\")) (> ?a ?b))";
    for (fields, expected, trace) in [
        ("3 1 2", &[1, 2, 3][..], "3:1;1:2;3:2;"),
        ("4 2 3 1", &[1, 2, 3, 4][..], "4:2;3:1;2:1;2:3;4:3;"),
        (
            "5 1 4 2 3",
            &[1, 2, 3, 4, 5][..],
            "5:1;1:4;5:4;2:3;1:2;4:2;4:3;",
        ),
    ] {
        let mut engine = sort_engine(&format!("exchange (create$ {fields})"), definitions);
        assert_integer_fields(&mut engine, expected);
        let Some(Value::String(calls)) = engine.get_global("calls") else {
            panic!("expected comparator trace string")
        };
        assert_eq!(calls.as_str(), trace);
    }
}

#[test]
fn only_the_false_symbol_prevents_exchange_including_void_results() {
    for (body, expected) in [
        ("FALSE", &[3, 1, 2][..]),
        ("TRUE", &[2, 1, 3][..]),
        ("0", &[2, 1, 3][..]),
        ("0.0", &[2, 1, 3][..]),
        ("\"FALSE\"", &[2, 1, 3][..]),
        ("nil", &[2, 1, 3][..]),
        ("(create$)", &[2, 1, 3][..]),
        ("(printout t \"called;\")", &[2, 1, 3][..]),
    ] {
        let definitions = format!("(deffunction exchange (?a ?b) {body})");
        let mut engine = sort_engine("exchange (create$ 3 1 2)", &definitions);
        assert_integer_fields(&mut engine, expected);
        if body.starts_with("(printout") {
            assert_eq!(engine.get_output("t"), Some("called;called;"));
        }
    }
}

#[test]
fn generic_and_variadic_predicates_are_really_invoked() {
    let definitions = "(defglobal ?*calls* = 0)
         (defgeneric exchange)
         (defmethod exchange ((?a INTEGER) (?b INTEGER))
           (bind ?*calls* (+ ?*calls* 1)) (< ?a ?b))";
    let mut engine = sort_engine("exchange (create$ 3 1 2)", definitions);
    assert_integer_fields(&mut engine, &[3, 2, 1]);
    assert!(matches!(
        engine.get_global("calls"),
        Some(Value::Integer(3))
    ));
    let definitions = "(deffunction exchange ($?args)
         (> (nth$ 1 ?args) (nth$ 2 ?args)))";
    assert_integer_fields(
        &mut sort_engine("exchange (create$ 3 1 2)", definitions),
        &[1, 2, 3],
    );
    assert_integer_fields(&mut sort_engine("+ (create$ 3 1 2)", ""), &[2, 1, 3]);
}

#[test]
fn all_data_evaluation_precedes_comparisons_and_occurs_once() {
    let definitions = "(deffunction exchange (?a ?b)
         (bind ?*trace* (+ (* ?*trace* 10) 4)) (> ?a ?b))";
    let mut engine = sort_engine(
        "(mark 1 exchange) (mark 2 (create$ 3 1)) (mark 3 2)",
        definitions,
    );
    assert_integer_fields(&mut engine, &[1, 2, 3]);
    assert_trace(&engine, 123_444);
}

#[test]
fn empty_and_singleton_lists_do_not_invoke_valid_predicates() {
    let definitions = "(deffunction exchange (?a ?b) (fail 3))";
    for (arguments, expected) in [("exchange", &[][..]), ("exchange 7", &[7][..])] {
        let mut engine = sort_engine(arguments, definitions);
        assert_integer_fields(&mut engine, expected);
        assert_trace(&engine, 0);
    }
}

#[test]
fn comparator_name_type_is_checked_before_data() {
    for name in ["42", "\">\""] {
        let mut engine = sort_engine(&format!("(mark 1 {name}) (mark 2 (create$ 3 1 2))"), "");
        assert_failure(&mut engine, "SYMBOL");
        assert_global_symbol(&engine, "result", "FALSE");
        assert_trace(&engine, 1);
    }
}

#[test]
fn invalid_names_and_comparator_arities_return_false_without_running_data() {
    for (arguments, definitions) in [
        ("(mark 1 missing) (mark 2 (create$ 3 1 2))", ""),
        ("(mark 1 missing)", ""),
        ("(mark 1 abs) (mark 2 (create$ 3 1 2))", ""),
        (
            "(mark 1 hidden) (mark 2 (create$ 3 1 2))",
            "(defmodule M (export deffunction hidden))
             (deffunction hidden (?a ?b) (> ?a ?b)) (defmodule MAIN)",
        ),
        (
            "(mark 1 exchange) (mark 2 (create$ 3 1 2))",
            "(deffunction exchange (?a) ?a)",
        ),
    ] {
        assert_nonfatal_name_failure(&mut sort_engine(arguments, definitions));
    }
}

#[test]
fn qualified_comparator_names_are_nonfatal_false_even_when_exported() {
    let definitions = "(defmodule M (export deffunction exchange))
         (deffunction exchange (?a ?b) (> ?a ?b))
         (defmodule MAIN)";
    assert_nonfatal_name_failure(&mut sort_engine(
        "(mark 1 M::exchange) (mark 2 (create$ 3 1 2))",
        definitions,
    ));
}

#[test]
fn expression_errors_preserve_sort_values_and_skip_later_user_bodies() {
    let mut engine = sort_engine("(fail 1) (mark 2 (create$ 3 1 2))", "");
    assert_failure(&mut engine, "division by zero");
    assert_global_symbol(&engine, "result", "FALSE");
    assert_trace(&engine, 1);

    let mut engine = sort_engine("(mark 1 >) (fail 2) (mark 3 2)", "");
    assert_failure(&mut engine, "division by zero");
    assert_stored_fields(&engine, &[Field::Symbol("FALSE"), Field::Symbol("FALSE")]);
    assert_trace(&engine, 12);
}

#[test]
fn data_errors_still_collect_defaults_and_merge_with_builtin_predicates() {
    for (predicate, first, second) in [(">", 1, 3), ("exchange", 3, 1)] {
        let mut engine = sort_engine(
            &format!("(mark 1 {predicate}) (mark 2 (create$ 3 1)) (fail 3) (mark 4 2)"),
            "(deffunction exchange (?a ?b) (mark 5 (> ?a ?b)))",
        );
        assert_failure(&mut engine, "division by zero");
        assert_stored_fields(
            &engine,
            &[
                Field::Integer(first),
                Field::Integer(second),
                Field::Symbol("FALSE"),
                Field::Symbol("FALSE"),
            ],
        );
        assert_trace(&engine, 123);
    }
}

#[test]
fn comparator_failures_return_the_partially_merged_values() {
    let definitions = "(defglobal ?*count* = 0)
         (deffunction exchange (?a ?b)
           (bind ?*count* (+ ?*count* 1))
           (bind ?*trace* (+ (* ?*trace* 10) 3))
           (if (= ?*count* 2) then (/ 1 0) else (> ?a ?b)))";
    let mut engine = sort_engine("(mark 1 exchange) (mark 2 (create$ 4 2 3 1))", definitions);
    assert_failure(&mut engine, "division by zero");
    assert_trace(&engine, 1233);
    assert_stored_fields(
        &engine,
        &[
            Field::Integer(2),
            Field::Integer(4),
            Field::Integer(3),
            Field::Integer(1),
        ],
    );
    assert!(matches!(
        engine.get_global("count"),
        Some(Value::Integer(2))
    ));
}

#[test]
fn builtin_type_and_generic_method_failures_are_not_suppressed() {
    let mut engine = sort_engine("(mark 1 >) (mark 2 (create$ red blue green))", "");
    assert_failure(&mut engine, "type error");
    assert_stored_fields(
        &engine,
        &[
            Field::Symbol("red"),
            Field::Symbol("blue"),
            Field::Symbol("green"),
        ],
    );
    assert_trace(&engine, 12);
    let definitions = "(defgeneric exchange)
         (defmethod exchange ((?a INTEGER) (?b INTEGER)) (> ?a ?b))";
    let mut engine = sort_engine("(mark 1 exchange) (mark 2 (create$ red blue))", definitions);
    assert_failure(&mut engine, "applicable");
    assert_stored_fields(&engine, &[Field::Symbol("red"), Field::Symbol("blue")]);
    assert_trace(&engine, 12);
}

#[test]
fn a_comparator_argument_is_required() {
    let mut engine = sort_engine("", "");
    assert_failure(&mut engine, "expected 1+");
    assert_global_symbol(&engine, "result", "FALSE");
    assert_trace(&engine, 0);
}

#[test]
fn generic_method_arity_is_checked_only_when_comparison_is_needed() {
    let definitions = "(defgeneric exchange)
         (defmethod exchange ((?a INTEGER)) TRUE)
         (defmethod exchange ((?a INTEGER) (?b INTEGER) (?c INTEGER)) TRUE)";
    for (arguments, expected, trace) in [
        ("(mark 1 exchange)", &[][..], 1),
        ("(mark 1 exchange) (mark 2 (create$))", &[][..], 12),
        ("(mark 1 exchange) (mark 2 7)", &[7][..], 12),
    ] {
        let mut engine = sort_engine(arguments, definitions);
        assert_integer_fields(&mut engine, expected);
        assert_trace(&engine, trace);
    }
    let mut engine = sort_engine("(mark 1 exchange) (mark 2 (create$ 3 1))", definitions);
    assert_failure(&mut engine, "applicable");
    assert_stored_fields(&engine, &[Field::Integer(3), Field::Integer(1)]);
    assert_trace(&engine, 12);
}

#[test]
fn failed_builtin_data_keeps_its_float_default_and_later_builtin_effects() {
    for (tail, expected, count) in [
        ("(mark 2 2)", Field::Symbol("FALSE"), 0),
        ("(bind ?*count* 7)", Field::Integer(7), 7),
        ("(+ 1 2)", Field::Integer(3), 0),
    ] {
        let mut engine = sort_engine(
            &format!("(mark 1 >) (/ 1 0) {tail}"),
            "(defglobal ?*count* = 0)",
        );
        assert_failure(&mut engine, "division by zero");
        assert_stored_fields(&engine, &[Field::Float(1.0), expected]);
        assert_trace(&engine, 1);
        assert!(
            matches!(engine.get_global("count"), Some(Value::Integer(actual)) if *actual == count)
        );
    }
}

#[test]
fn builtin_predicate_error_defaults_still_determine_exchange() {
    // Numeric comparison errors return FALSE; + errors return INTEGER 0, which
    // is true for sort. Neither return value clears the fatal outcome.
    for (predicate, expected) in [
        (">", ["red", "blue", "green"]),
        ("+", ["green", "blue", "red"]),
    ] {
        let mut engine = sort_engine(
            &format!("(mark 1 {predicate}) (mark 2 (create$ red blue green))"),
            "",
        );
        assert_failure(&mut engine, "type error");
        assert_stored_fields(&engine, &expected.map(Field::Symbol));
        assert_trace(&engine, 12);
    }
    let mut engine = sort_engine("(mark 1 /) (mark 2 (create$ 3 0 2))", "");
    assert_failure(&mut engine, "division by zero");
    assert_stored_fields(
        &engine,
        &[Field::Integer(2), Field::Integer(0), Field::Integer(3)],
    );
    assert_trace(&engine, 12);
}

const BOOM: &str = "(deffunction boom (?a ?b) (fail 3) (mark 4 TRUE))";
const FAILING_SORT: &str = "(sort (mark 1 boom) (mark 2 (create$ 3 1 2)))";

#[test]
fn enclosing_builtins_observe_evaluation_error_separately_from_sticky_halt() {
    // With two fields the failed comparison is the last callable evaluation;
    // with three, a later skipped call clears EvaluationError but not Halt.
    let two = "(sort (mark 1 boom) (mark 2 (create$ 3 1)))";
    let mut engine = expression_engine(&format!("(create$ {two})"), BOOM);
    assert_failure(&mut engine, "division by zero");
    assert_stored_fields(&engine, &[]);
    assert_trace(&engine, 123);
    let mut engine = expression_engine(&format!("(length$ {two})"), BOOM);
    assert_failure(&mut engine, "division by zero");
    assert!(matches!(
        engine.get_global("result"),
        Some(Value::Integer(2))
    ));
    assert_trace(&engine, 123);
    let mut engine = expression_engine(&format!("(nth$ 1 {two})"), BOOM);
    assert_failure(&mut engine, "division by zero");
    assert_global_symbol(&engine, "result", "nil");
    assert_trace(&engine, 123);
    let mut engine = expression_engine(&format!("(create$ {FAILING_SORT})"), BOOM);
    assert_failure(&mut engine, "division by zero");
    assert_stored_fields(
        &engine,
        &[Field::Integer(3), Field::Integer(1), Field::Integer(2)],
    );
    assert_trace(&engine, 123);
    for expression in [
        format!("(length$ {FAILING_SORT})"),
        format!("(nth$ 1 {FAILING_SORT})"),
    ] {
        let mut engine = expression_engine(&expression, BOOM);
        assert_failure(&mut engine, "division by zero");
        assert!(matches!(
            engine.get_global("result"),
            Some(Value::Integer(3))
        ));
        assert_trace(&engine, 123);
    }
    let mut engine = expression_engine(&format!("(+ {FAILING_SORT} (mark 5 1))"), BOOM);
    assert_failure(&mut engine, "division by zero");
    assert!(matches!(
        engine.get_global("result"),
        Some(Value::Integer(0))
    ));
    assert_trace(&engine, 123);

    let mut engine = expression_engine(&format!("(create$ {FAILING_SORT} (mark 5 9))"), BOOM);
    assert_failure(&mut engine, "division by zero");
    assert_stored_fields(
        &engine,
        &[
            Field::Integer(3),
            Field::Integer(1),
            Field::Integer(2),
            Field::Symbol("FALSE"),
        ],
    );
    assert_trace(&engine, 123);
}

#[test]
fn callable_boundaries_return_false_but_keep_completed_inner_assignments() {
    let definitions = format!("{BOOM} (deffunction capture (?value) (mark 5 ?value))");
    let mut engine = expression_engine(&format!("(capture {FAILING_SORT})"), &definitions);
    assert_failure(&mut engine, "division by zero");
    assert_global_symbol(&engine, "result", "FALSE");
    assert_trace(&engine, 123);

    let definitions = format!(
        "{BOOM} (defglobal ?*outer* = pending)
        (deffunction wrapper () (bind ?*result* {FAILING_SORT}) (mark 5 99))"
    );
    let mut engine = body_engine("(bind ?*outer* (wrapper)) (assert (after))", &definitions);
    assert_failure(&mut engine, "division by zero");
    assert_stored_fields(
        &engine,
        &[Field::Integer(3), Field::Integer(1), Field::Integer(2)],
    );
    assert_global_symbol(&engine, "outer", "FALSE");
    assert_trace(&engine, 123);
}

#[test]
fn enclosing_printout_and_if_stop_before_emitting_or_running_selected_values() {
    let mut engine = body_engine(
        &format!("(printout t \"prefix:\" {FAILING_SORT} \":suffix\" crlf) (assert (after))"),
        BOOM,
    );
    assert_failure(&mut engine, "division by zero");
    assert_eq!(engine.get_output("t"), Some("prefix:"));
    assert_global_symbol(&engine, "result", "pending");
    assert_trace(&engine, 123);

    let definitions = format!(
        "{BOOM} (defglobal ?*second* = 0)
        (defrule later (declare (salience -10)) => (bind ?*second* 1))"
    );
    let mut engine = body_engine(
        &format!("(if {FAILING_SORT} then (bind ?*result* branch) else (bind ?*result* fallback)) (assert (after))"), &definitions,
    );
    assert_failure(&mut engine, "division by zero");
    assert_global_symbol(&engine, "result", "pending");
    assert_trace(&engine, 123);
    assert!(matches!(
        engine.get_global("second"),
        Some(Value::Integer(0))
    ));
}

#[test]
fn return_comparator_unwinds_the_rule_without_halting_other_activations() {
    for (data, expected, after) in [
        ("", &[][..], 1),
        ("(create$)", &[][..], 1),
        ("7", &[7][..], 1),
        ("(create$ 3 1)", &[1, 3][..], 0),
    ] {
        let mut engine = Engine::with_rules(&format!(
            "(defglobal ?*result* = pending ?*second* = 0)
             (defrule compute (declare (salience 10)) =>
               (bind ?*result* (sort return {data})) (assert (after)))
             (defrule later => (bind ?*second* 1))"
        ))
        .unwrap();
        let result = engine.run(RunLimit::Count(10)).unwrap();
        assert_eq!(result.halt_reason, HaltReason::AgendaEmpty);
        assert_eq!(result.rules_fired, 2);
        assert!(engine.action_diagnostics().is_empty());
        assert_eq!(engine.find_facts("after").unwrap().len(), after);
        assert!(matches!(
            engine.get_global("second"),
            Some(Value::Integer(1))
        ));
        assert_stored_fields(
            &engine,
            &expected
                .iter()
                .copied()
                .map(Field::Integer)
                .collect::<Vec<_>>(),
        );
    }
}
