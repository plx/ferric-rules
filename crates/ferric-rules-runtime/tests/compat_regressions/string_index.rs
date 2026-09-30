//! Public API controls for str-index result types and operand evaluation.
//!
//! Argument results and error traces were checked against pinned CLIPS 6.30.
//! These tests require the measured behavior without pinning diagnostic wording.

use ferric_rules_core::{Fact, Value};
use ferric_rules_runtime::{Engine, HaltReason, RunLimit};

fn string_index_engine(arguments: &str) -> Engine {
    Engine::with_rules(&format!(
        "(defglobal ?*trace* = 0)
         (deffunction mark (?digit ?value)
           (bind ?*trace* (+ (* ?*trace* 10) ?digit)) ?value)
         (deffunction fail (?digit)
           (bind ?*trace* (+ (* ?*trace* 10) ?digit)) (/ 1 0))
         (defrule compute =>
           (assert (result (str-index {arguments})))
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

fn assert_success(engine: &mut Engine, expected: Option<i64>) {
    let run = engine.run(RunLimit::Count(10)).unwrap();
    assert_eq!(run.halt_reason, HaltReason::AgendaEmpty);
    assert_eq!(run.rules_fired, 1);
    assert!(
        engine.action_diagnostics().is_empty(),
        "{:?}",
        engine.action_diagnostics()
    );
    let facts = engine.find_facts("result").unwrap();
    assert_eq!(facts.len(), 1);
    let Fact::Ordered(fact) = facts[0].1 else {
        panic!("result must be an ordered fact")
    };
    match (fact.fields.as_slice(), expected) {
        ([Value::Integer(actual)], Some(expected)) => assert_eq!(*actual, expected),
        ([Value::Symbol(symbol)], None) => {
            assert_eq!(engine.resolve_core_symbol(*symbol), Some("FALSE"));
        }
        (actual, expected) => panic!("expected INTEGER {expected:?} or FALSE, got {actual:?}"),
    }
    assert_eq!(engine.find_facts("after").unwrap().len(), 1);
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
fn results_are_integer_positions_or_the_false_symbol() {
    for (arguments, expected) in [
        ("\"\" \"abc\"", Some(4)),
        ("\"\" \"\"", Some(1)),
        ("\"\" abc-def", Some(8)),
        ("(sym-cat \"\") \"abc\"", Some(4)),
        ("\"\" (sym-cat \"\")", Some(1)),
        ("ana banana", Some(2)),
        ("\"na\" banana", Some(3)),
        ("z \"banana\"", None),
        ("\"abc\" \"ab\"", None),
        ("\"a\" \"\"", None),
        ("\"\" \"é🙂\"", Some(3)),
        ("\"🙂\" \"é🙂\"", Some(2)),
        ("\"\" \"e\u{301}\"", Some(3)),
        ("\"\u{301}\" \"e\u{301}\"", Some(2)),
    ] {
        let mut engine = string_index_engine(arguments);
        assert_success(&mut engine, expected);
    }
}

#[test]
fn exact_arity_is_checked_before_argument_effects() {
    for arguments in [
        "",
        "(mark 1 \"\")",
        "(mark 1 \"\") (mark 2 \"abc\") (mark 3 \"extra\")",
    ] {
        let mut engine = string_index_engine(arguments);
        assert_failure(&mut engine, "expected 2");
        assert_trace(&engine, 0);
    }
}

#[test]
fn required_lexemes_are_evaluated_once_from_left_to_right() {
    for (needle, haystack, expected) in [
        ("\"\"", "\"abc\"", Some(4)),
        ("(sym-cat \"\")", "abc", Some(4)),
        ("\"b\"", "\"abc\"", Some(2)),
        ("a", "\"\"", None),
    ] {
        let arguments = format!("(mark 1 {needle}) (mark 2 {haystack})");
        let mut engine = string_index_engine(&arguments);
        assert_success(&mut engine, expected);
        assert_trace(&engine, 12);
    }
}

#[test]
fn first_lexeme_is_validated_before_evaluating_the_second() {
    for invalid_needle in ["42", "1.5", "(create$ a b)"] {
        for second_argument in ["(mark 2 \"abc\")", "(fail 2)"] {
            let arguments = format!("(mark 1 {invalid_needle}) {second_argument}");
            let mut engine = string_index_engine(&arguments);
            assert_failure(&mut engine, "STRING, SYMBOL, or INSTANCE-NAME");
            assert_trace(&engine, 1);
        }
    }
}

#[test]
fn an_empty_needle_still_evaluates_and_validates_the_second_lexeme() {
    for needle in ["\"\"", "(sym-cat \"\")", "\"b\""] {
        for invalid_haystack in ["42", "1.5", "(create$ a b)"] {
            let arguments = format!("(mark 1 {needle}) (mark 2 {invalid_haystack})");
            let mut engine = string_index_engine(&arguments);
            assert_failure(&mut engine, "STRING, SYMBOL, or INSTANCE-NAME");
            assert_trace(&engine, 12);
        }
    }
}

#[test]
fn argument_evaluation_errors_stop_later_work_and_preserve_prior_effects() {
    for (arguments, trace) in [
        ("(fail 1) (mark 2 \"abc\")", 1),
        ("(mark 1 \"\") (fail 2)", 12),
        ("(mark 1 (sym-cat \"\")) (fail 2)", 12),
        ("(mark 1 \"b\") (fail 2)", 12),
    ] {
        let mut engine = string_index_engine(arguments);
        assert_failure(&mut engine, "division by zero");
        assert_trace(&engine, trace);
    }
}
