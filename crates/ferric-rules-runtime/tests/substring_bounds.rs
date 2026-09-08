//! Public API controls for substring argument validation and empty-range timing.
//!
//! Argument traces were checked against pinned CLIPS 6.30, including operand
//! expression failures and text expressions skipped by ends below one.
//! Error assertions preserve behavior without pinning CLIPS diagnostic text.

use ferric_rules_core::{Fact, Value};
use ferric_rules_runtime::{Engine, HaltReason, RunLimit};

fn substring_engine(arguments: &str) -> Engine {
    Engine::with_rules(&format!(
        "(defglobal ?*trace* = 0)
         (deffunction mark (?digit ?value)
           (bind ?*trace* (+ (* ?*trace* 10) ?digit)) ?value)
         (defrule compute =>
           (assert (result (sub-string {arguments})))
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

fn assert_success(engine: &mut Engine, expected: &str) {
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
    let [Value::String(value)] = fact.fields.as_slice() else {
        panic!("sub-string must return STRING: {:?}", fact.fields)
    };
    assert_eq!(value.as_str(), expected);
    assert_eq!(engine.find_facts("after").unwrap().len(), 1);
}

fn assert_failure(engine: &mut Engine, expected_error: &str) {
    let run = engine.run(RunLimit::Count(10)).unwrap();
    assert_eq!(run.halt_reason, HaltReason::ActionError);
    assert!(
        engine.action_diagnostics().iter().any(|error| {
            let message = error.to_string();
            message.contains("sub-string") && message.contains(expected_error)
        }),
        "{:?}",
        engine.action_diagnostics()
    );
    assert!(engine.find_facts("result").unwrap().is_empty());
    assert!(engine.find_facts("after").unwrap().is_empty());
}

#[test]
fn exact_arity_is_checked_before_argument_effects() {
    for arguments in [
        "",
        "(mark 1 0)",
        "(mark 1 0) (mark 2 2)",
        "(mark 1 0) (mark 2 2) (mark 3 \"abc\") (mark 4 9)",
    ] {
        let mut engine = substring_engine(arguments);
        assert_failure(&mut engine, "expected 3");
        assert_trace(&engine, 0);
    }
}

#[test]
fn each_index_is_validated_before_evaluating_the_next_argument() {
    for (arguments, expected_error, trace) in [
        (
            "(mark 1 \"bad\") (mark 2 2) (mark 3 \"abc\")",
            "INTEGER (start position)",
            1,
        ),
        (
            "(mark 1 0) (mark 2 \"bad\") (mark 3 \"abc\")",
            "INTEGER (end position)",
            12,
        ),
    ] {
        let mut engine = substring_engine(arguments);
        assert_failure(&mut engine, expected_error);
        assert_trace(&engine, trace);
    }
}

#[test]
fn ends_below_one_skip_text_evaluation_and_type_validation() {
    for end in ["0", "-1", "-9223372036854775808"] {
        for invalid_text in ["42", "1.5", "(create$ 1 2)"] {
            let arguments = format!("(mark 1 0) (mark 2 {end}) (mark 3 {invalid_text})");
            let mut engine = substring_engine(&arguments);
            assert_success(&mut engine, "");
            assert_trace(&engine, 12);
        }
    }
}

#[test]
fn positive_ends_reach_invalid_text_even_for_reversed_or_out_of_range_bounds() {
    for (start, end) in [(0, 2), (3, 2), (4, 9)] {
        for invalid_text in ["42", "1.5", "(create$ 1 2)"] {
            let arguments = format!("(mark 1 {start}) (mark 2 {end}) (mark 3 {invalid_text})");
            let mut engine = substring_engine(&arguments);
            assert_failure(&mut engine, "STRING or SYMBOL");
            assert_trace(&engine, 123);
        }
    }
}

#[test]
fn clipped_results_remain_strings_and_preserve_character_boundaries() {
    // UTF-8 scalar positions are Ferric's existing policy, not a new CLIPS golden.
    for (arguments, expected) in [
        ("0 2 \"héllo\"", "hé"),
        ("-9223372036854775808 9223372036854775807 \"abc\"", "abc"),
        ("0 2 \"\"", ""),
        ("3 2 \"abc\"", ""),
    ] {
        let mut engine = substring_engine(arguments);
        assert_success(&mut engine, expected);
    }
}

#[test]
fn symbol_text_uses_its_spelling_and_always_returns_a_string() {
    for (arguments, expected) in [
        ("0 2 abc", "ab"),
        ("2 9 abc", "bc"),
        ("3 2 abc", ""),
        ("-2 99 FALSE", "FALSE"),
        ("0 99 (sym-cat \"alpha\" \"-\" \"beta\")", "alpha-beta"),
        ("2 3 (sym-cat \"a b\")", " b"),
    ] {
        let mut engine = substring_engine(arguments);
        assert_success(&mut engine, expected);
    }
}

#[test]
fn operand_expression_failures_propagate_only_when_reached() {
    for (arguments, trace, fails) in [
        ("(fail 1) (mark 2 2) (mark 3 \"abc\")", 1, true),
        ("(mark 1 0) (fail 2) (mark 3 \"abc\")", 12, true),
        ("(mark 1 0) (mark 2 2) (fail 3)", 123, true),
        ("(fail 1) (mark 2 0) (fail 3)", 1, true),
        ("(mark 1 -9223372036854775808) (fail 2) (fail 3)", 12, true),
        ("(mark 1 0) (mark 2 0) (fail 3)", 12, false),
        ("(mark 1 3) (mark 2 2) (fail 3)", 123, true),
        (
            "(mark 1 9223372036854775807) (mark 2 9223372036854775807) (fail 3)",
            123,
            true,
        ),
    ] {
        let source = format!(
            "(defglobal ?*trace* = 0)
             (deffunction mark (?digit ?value)
               (bind ?*trace* (+ (* ?*trace* 10) ?digit)) ?value)
             (deffunction fail (?digit)
               (bind ?*trace* (+ (* ?*trace* 10) ?digit)) (/ 1 0))
             (defrule compute =>
               (assert (result (sub-string {arguments})))
               (assert (after)))"
        );
        let mut engine = Engine::with_rules(&source).unwrap();
        if fails {
            let run = engine.run(RunLimit::Count(10)).unwrap();
            assert_eq!(run.halt_reason, HaltReason::ActionError, "{arguments}");
            assert!(
                engine
                    .action_diagnostics()
                    .iter()
                    .any(|error| error.to_string().contains("division by zero in `/`")),
                "{arguments}: {:?}",
                engine.action_diagnostics()
            );
            assert!(engine.find_facts("result").unwrap().is_empty());
            assert!(engine.find_facts("after").unwrap().is_empty());
        } else {
            assert_success(&mut engine, "");
        }
        assert_trace(&engine, trace);
    }
}
