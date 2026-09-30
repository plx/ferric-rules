//! Public API controls for nth$ result identity, safe bounds, and evaluation.
//!
//! Reference: CLIPS 6.30, Debian 6.30-4.1 (linux/arm64). Runtime FLOAT
//! coercion is measured separately from CLIPS's literal FLOAT source rejection;
//! these tests reach it through a callable parameter. Diagnostic wording and
//! general source-time type validation are not part of these runtime assertions.

use ferric_rules_core::{Fact, Value};
use ferric_rules_runtime::{Engine, HaltReason, RunLimit};

fn nth_engine(function: &str, arguments: &str) -> Engine {
    Engine::with_rules(&format!(
        "(defglobal ?*trace* = 0)
         (deffunction mark (?digit ?value)
           (bind ?*trace* (+ (* ?*trace* 10) ?digit)) ?value)
         (deffunction identity (?value) ?value)
         (deffunction fail (?digit)
           (bind ?*trace* (+ (* ?*trace* 10) ?digit)) (/ 1 0))
         (defrule compute =>
           (assert (result ({function} {arguments})))
           (assert (after)))"
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

fn success_value(engine: &mut Engine) -> Value {
    let run = engine.run(RunLimit::Count(10)).unwrap();
    assert_eq!(run.halt_reason, HaltReason::AgendaEmpty);
    assert_eq!(run.rules_fired, 1);
    assert!(
        engine.action_diagnostics().is_empty(),
        "{:?}",
        engine.action_diagnostics()
    );
    assert_eq!(engine.find_facts("after").unwrap().len(), 1);
    let facts = engine.find_facts("result").unwrap();
    assert_eq!(facts.len(), 1);
    let Fact::Ordered(fact) = facts[0].1 else {
        panic!("result must be an ordered fact")
    };
    let [value] = fact.fields.as_slice() else {
        panic!("result must contain exactly one scalar value")
    };
    value.clone()
}

fn assert_symbol(engine: &mut Engine, expected: &str) {
    let Value::Symbol(symbol) = success_value(engine) else {
        panic!("result must be an actual SYMBOL, including absent-index nil")
    };
    assert_eq!(engine.resolve_core_symbol(symbol), Some(expected));
}

fn assert_failure(engine: &mut Engine, expected_error: &str) {
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
fn absent_positions_return_actual_nil_symbols_without_panicking() {
    for function in ["nth$", "nth"] {
        for arguments in [
            "0 (create$ a b)",
            "-1 (create$ a b)",
            "3 (create$ a b)",
            "-9223372036854775808 (create$ a b)",
            "9223372036854775807 (create$ a b)",
            "-9223372036854775808 (create$)",
            "0 (create$)",
            "1 (create$)",
            "9223372036854775807 (create$)",
        ] {
            let mut engine = nth_engine(function, arguments);
            assert_symbol(&mut engine, "nil");
        }
    }
}

#[test]
fn selected_fields_keep_their_original_types_and_values() {
    for function in ["nth$", "nth"] {
        let fields = "(create$ 9223372036854775807 2.5 \"two words\" red nil FALSE)";
        for position in 1..=6 {
            let mut engine = nth_engine(function, &format!("{position} {fields}"));
            let value = success_value(&mut engine);
            match (position, value) {
                (1, Value::Integer(value)) => assert_eq!(value, i64::MAX),
                (2, Value::Float(value)) => assert_eq!(value.to_bits(), 2.5_f64.to_bits()),
                (3, Value::String(value)) => assert_eq!(value.as_str(), "two words"),
                (4..=6, Value::Symbol(symbol)) => {
                    let expected = match position {
                        4 => "red",
                        5 => "nil",
                        _ => "FALSE",
                    };
                    assert_eq!(engine.resolve_core_symbol(symbol), Some(expected));
                }
                (position, actual) => panic!("position {position}: unexpected {actual:?}"),
            }
        }
    }
}

#[test]
fn runtime_float_indices_truncate_toward_zero() {
    for (index, expected) in [
        ("1.0", "a"),
        ("1.9", "a"),
        ("2.9", "b"),
        ("3.1", "c"),
        ("0.9", "nil"),
        ("-0.9", "nil"),
        ("-1.9", "nil"),
    ] {
        let mut engine = nth_engine("nth$", &format!("(identity {index}) (create$ a b c)"));
        assert_symbol(&mut engine, expected);
    }
}

#[test]
fn required_operands_are_evaluated_once_even_for_absent_positions() {
    for (index, expected) in [
        ("1", "a"),
        ("0", "nil"),
        ("-1", "nil"),
        ("3", "nil"),
        ("1.0", "a"),
    ] {
        let mut engine = nth_engine("nth$", &format!("(mark 1 {index}) (mark 2 (create$ a b))"));
        assert_symbol(&mut engine, expected);
        assert_trace(&engine, 12);
    }
}

#[test]
fn nonnumeric_index_validation_precedes_the_second_operand() {
    for index in ["red", "(create$ 1)"] {
        for second in ["(mark 2 (create$ a b))", "(fail 2)"] {
            let mut engine = nth_engine("nth$", &format!("(mark 1 {index}) {second}"));
            assert_failure(&mut engine, "type error");
            assert_trace(&engine, 1);
        }
    }
}

#[test]
fn an_absent_position_still_requires_a_multifield_second_operand() {
    for index in ["1", "0", "-1", "3"] {
        let mut engine = nth_engine("nth$", &format!("(mark 1 {index}) (mark 2 42)"));
        assert_failure(&mut engine, "MULTIFIELD");
        assert_trace(&engine, 12);
    }
}

#[test]
fn operand_errors_preserve_prior_effects_and_stop_later_actions() {
    for (arguments, trace) in [
        ("(fail 1) (mark 2 (create$ a b))", 1),
        ("(mark 1 1) (fail 2)", 12),
        ("(mark 1 0) (fail 2)", 12),
        ("(mark 1 -1) (fail 2)", 12),
        ("(mark 1 3) (fail 2)", 12),
        ("(mark 1 1.0) (fail 2)", 12),
    ] {
        let mut engine = nth_engine("nth$", arguments);
        assert_failure(&mut engine, "division by zero");
        assert_trace(&engine, trace);
    }
}

#[test]
fn exact_arity_is_validated_before_argument_effects() {
    for function in ["nth$", "nth"] {
        for arguments in [
            "",
            "(mark 1 1)",
            "(mark 1 1) (mark 2 (create$ a b)) (mark 3 3)",
        ] {
            let mut engine = nth_engine(function, arguments);
            assert_failure(&mut engine, "expected 2");
            assert_trace(&engine, 0);
        }
    }
}
