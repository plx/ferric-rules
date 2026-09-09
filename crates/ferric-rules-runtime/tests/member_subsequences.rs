//! Public API controls for member$/member return shapes and evaluation.
//!
//! Values are captured in a global so a returned multifield is inspected as a
//! Value, without assertion-time field flattening. Reference source diagnostics
//! are separate from these runtime type/effect assertions.

use ferric_rules_core::Value;
use ferric_rules_runtime::{Engine, HaltReason, RunLimit};

#[derive(Clone, Copy, Debug)]
enum Expected {
    Index(i64),
    Range(i64, i64),
    False,
}

fn member_engine(function: &str, arguments: &str) -> Engine {
    Engine::with_rules(&format!(
        "(defglobal ?*trace* = 0 ?*result* = pending)
         (deffunction mark (?digit ?value)
           (bind ?*trace* (+ (* ?*trace* 10) ?digit)) ?value)
         (deffunction fail (?digit)
           (bind ?*trace* (+ (* ?*trace* 10) ?digit)) (/ 1 0))
         (defrule compute =>
           (bind ?*result* ({function} {arguments}))
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

fn assert_result(engine: &mut Engine, expected: Expected) {
    let run = engine.run(RunLimit::Count(10)).unwrap();
    assert_eq!(run.halt_reason, HaltReason::AgendaEmpty);
    assert_eq!(run.rules_fired, 1);
    assert!(
        engine.action_diagnostics().is_empty(),
        "{:?}",
        engine.action_diagnostics()
    );
    assert_eq!(engine.find_facts("after").unwrap().len(), 1);
    let value = engine.get_global("result").unwrap();
    match (expected, value) {
        (Expected::Index(expected), Value::Integer(actual)) => assert_eq!(*actual, expected),
        (Expected::Range(start, end), Value::Multifield(actual)) => {
            assert!(
                matches!(actual.as_slice(), [Value::Integer(a), Value::Integer(b)] if *a == start && *b == end),
                "expected two INTEGER endpoints {start}, {end}; got {actual:?}"
            );
        }
        (Expected::False, Value::Symbol(symbol)) => {
            assert_eq!(engine.resolve_core_symbol(*symbol), Some("FALSE"));
        }
        (expected, actual) => panic!("expected {expected:?}; got {actual:?}"),
    }
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
    assert!(engine.find_facts("after").unwrap().is_empty());
    let Some(Value::Symbol(symbol)) = engine.get_global("result") else {
        panic!("failed evaluation must leave the previous result unchanged")
    };
    assert_eq!(engine.resolve_core_symbol(*symbol), Some("pending"));
}

#[test]
fn scalar_singleton_and_subsequence_results_have_exact_value_types() {
    for function in ["member$", "member"] {
        for (arguments, expected) in [
            ("b (create$ a b c)", Expected::Index(2)),
            ("(create$ b) (create$ a b c)", Expected::Index(2)),
            ("(create$ a b) (create$ a b c)", Expected::Range(1, 2)),
            ("(create$ b c) (create$ a b c)", Expected::Range(2, 3)),
            ("(create$ a b c) (create$ a b c)", Expected::Range(1, 3)),
            ("(create$ a c) (create$ a b c)", Expected::False),
            ("(create$ a b c d) (create$ a b c)", Expected::False),
            ("z (create$ a b c)", Expected::False),
        ] {
            assert_result(&mut member_engine(function, arguments), expected);
        }
    }
}

#[test]
fn empty_needle_range_depends_on_whether_the_haystack_is_empty() {
    for function in ["member$", "member"] {
        for (arguments, expected) in [
            ("a (create$)", Expected::False),
            ("(create$ a) (create$)", Expected::False),
            ("(create$) (create$)", Expected::False),
            ("(create$) (create$ a)", Expected::Range(1, 0)),
            ("(create$) (create$ a b)", Expected::Range(1, 0)),
        ] {
            assert_result(&mut member_engine(function, arguments), expected);
        }
    }
}

#[test]
fn field_equality_preserves_types_float_bits_and_exact_large_integers() {
    for function in ["member$", "member"] {
        for (arguments, expected) in [
            ("2 (create$ 2.0 2)", Expected::Index(2)),
            ("2.0 (create$ 2 2.0)", Expected::Index(2)),
            ("2.0 (create$ 2)", Expected::False),
            ("\"red\" (create$ red \"red\")", Expected::Index(2)),
            ("red (create$ \"red\" red)", Expected::Index(2)),
            ("\"A\" (create$ \"a\")", Expected::False),
            ("A (create$ a)", Expected::False),
            ("FALSE (create$ TRUE FALSE)", Expected::Index(2)),
            ("-0.0 (create$ 0.0 -0.0)", Expected::Index(2)),
            (
                "9007199254740993 (create$ 9007199254740992 9007199254740993)",
                Expected::Index(2),
            ),
            (
                "(create$ 2 3.0) (create$ 2.0 3.0 2 3.0)",
                Expected::Range(3, 4),
            ),
            (
                "(create$ \"red\" blue) (create$ red blue \"red\" blue)",
                Expected::Range(3, 4),
            ),
            ("(create$ 2.0 \"red\") (create$ 2 \"red\")", Expected::False),
            (
                "(create$ -0.0 x) (create$ 0.0 x -0.0 x)",
                Expected::Range(3, 4),
            ),
            (
                "(create$ 9007199254740993 x) (create$ 9007199254740992 x 9007199254740993 x)",
                Expected::Range(3, 4),
            ),
        ] {
            assert_result(&mut member_engine(function, arguments), expected);
        }
    }
}

#[test]
fn first_complete_match_and_slice_relative_indices_are_preserved() {
    for function in ["member$", "member"] {
        for (arguments, expected) in [
            ("(create$ b c) (create$ a b c b c)", Expected::Range(2, 3)),
            ("(create$ a a) (create$ a a a)", Expected::Range(1, 2)),
            ("(create$ a a b) (create$ a a a b)", Expected::Range(2, 4)),
            (
                "(subseq$ (create$ x b c y) 2 3) (subseq$ (create$ x a b c d y) 2 5)",
                Expected::Range(2, 3),
            ),
            (
                "(subseq$ (create$ x c y) 2 2) (subseq$ (create$ x a b c y) 2 4)",
                Expected::Index(3),
            ),
        ] {
            assert_result(&mut member_engine(function, arguments), expected);
        }
    }
}

#[test]
fn both_operands_are_evaluated_once_before_searching_even_when_empty() {
    for function in ["member$", "member"] {
        for (needle, haystack, expected) in [
            ("b", "(create$ a b c)", Expected::Index(2)),
            ("(create$ b c)", "(create$ a b c)", Expected::Range(2, 3)),
            ("(create$)", "(create$ a)", Expected::Range(1, 0)),
            ("(create$)", "(create$)", Expected::False),
            ("(create$ z)", "(create$ a b)", Expected::False),
        ] {
            let arguments = format!("(mark 1 {needle}) (mark 2 {haystack})");
            let mut engine = member_engine(function, &arguments);
            assert_result(&mut engine, expected);
            assert_trace(&engine, 12);
        }
    }
}

#[test]
fn void_needle_is_an_accepted_scalar_and_does_not_skip_the_haystack() {
    for function in ["member$", "member"] {
        let mut engine =
            member_engine(function, "(printout t \"first\" crlf) (mark 2 (create$ a))");
        assert_result(&mut engine, Expected::False);
        assert_eq!(engine.get_output("t").unwrap(), Some("first\n"));
        assert_trace(&engine, 2);
    }
}

#[test]
fn even_an_empty_needle_requires_a_multifield_second_argument() {
    for function in ["member$", "member"] {
        for needle in ["a", "(create$ a)", "(create$)"] {
            let mut engine = member_engine(function, &format!("(mark 1 {needle}) (mark 2 99)"));
            assert_failure(&mut engine, "MULTIFIELD");
            assert_trace(&engine, 12);
        }
    }
}

#[test]
fn operand_errors_preserve_prior_effects_and_stop_later_actions() {
    for function in ["member$", "member"] {
        for (arguments, trace) in [
            ("(fail 1) (mark 2 (create$ a))", 1),
            ("(mark 1 a) (fail 2)", 12),
            ("(mark 1 (create$)) (fail 2)", 12),
        ] {
            let mut engine = member_engine(function, arguments);
            assert_failure(&mut engine, "division by zero");
            assert_trace(&engine, trace);
        }
    }
}

#[test]
fn exact_arity_is_checked_before_argument_effects() {
    for function in ["member$", "member"] {
        for arguments in [
            "",
            "(mark 1 a)",
            "(mark 1 a) (mark 2 (create$ a)) (mark 3 z)",
        ] {
            let mut engine = member_engine(function, arguments);
            assert_failure(&mut engine, "expected 2");
            assert_trace(&engine, 0);
        }
    }
}
