//! Public API regressions for extrema selection, independent of output formatting.

use ferric_rules_core::{Fact, Value};
use ferric_rules_runtime::{Engine, HaltReason, RunLimit};

fn result_fields(expressions: &str) -> Vec<Value> {
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
    let results = engine.find_facts("result").unwrap();
    assert_eq!(results.len(), 1);
    let Fact::Ordered(result) = results[0].1 else {
        panic!("result must be an ordered fact")
    };
    result.fields.to_vec()
}

#[test]
fn selected_operand_types_and_first_winner_ties_reach_asserted_facts() {
    let fields = result_fields(
        "(min 4.0 2 8.0) (max 4.0 2 8)
         (min 4 2.5 8) (max 4 2 8.5)
         (min 2 2.0) (min 2.0 2) (max 2 2.0) (max 2.0 2)
         (min 9.0 2 2.0 3) (max -9.0 2 2.0 1)",
    );
    assert!(matches!(
        fields.as_slice(),
        [
            Value::Integer(2),
            Value::Integer(8),
            Value::Float(minimum_float),
            Value::Float(maximum_float),
            Value::Integer(2),
            Value::Float(minimum_tie),
            Value::Integer(2),
            Value::Float(maximum_tie),
            Value::Integer(2),
            Value::Integer(2),
        ] if minimum_float.to_bits() == 2.5_f64.to_bits()
            && maximum_float.to_bits() == 8.5_f64.to_bits()
            && minimum_tie.to_bits() == 2.0_f64.to_bits()
            && maximum_tie.to_bits() == 2.0_f64.to_bits()
    ));
}

#[test]
fn signed_zero_ties_preserve_the_selected_float_bits() {
    let fields = result_fields(
        "(min -0.0 0.0) (min 0.0 -0.0)
         (max -0.0 0.0) (max 0.0 -0.0)
         (min 0 -0.0) (max 0 -0.0)",
    );
    let expected = [-0.0_f64, 0.0, -0.0, 0.0];
    for (actual, expected) in fields[..4].iter().zip(expected) {
        let Value::Float(actual) = actual else {
            panic!("expected a float, got {actual:?}")
        };
        assert_eq!(actual.to_bits(), expected.to_bits());
    }
    assert!(matches!(
        &fields[4..],
        [Value::Integer(0), Value::Integer(0)]
    ));
}

#[test]
fn integer_order_remains_exact_including_after_a_float_loses() {
    let fields = result_fields(
        "(min 9007199254740993 9007199254740992)
         (max 9007199254740992 9007199254740993)
         (min 9223372036854775807 -9223372036854775808)
         (max -9223372036854775808 9223372036854775807)
         (max 0.0 9007199254740992 9007199254740993)
         (min 1.0e20 9007199254740993 9007199254740992)",
    );
    assert!(matches!(
        fields.as_slice(),
        [
            Value::Integer(9_007_199_254_740_992),
            Value::Integer(9_007_199_254_740_993),
            Value::Integer(i64::MIN),
            Value::Integer(i64::MAX),
            Value::Integer(9_007_199_254_740_993),
            Value::Integer(9_007_199_254_740_992),
        ]
    ));
}

#[test]
fn rounded_mixed_ties_keep_the_original_type_and_integer_payload() {
    let fields = result_fields(
        "(min 9007199254740992.0 9007199254740993)
         (min 9007199254740993 9007199254740992.0)
         (max 9007199254740992.0 9007199254740993)
         (max 9007199254740993 9007199254740992.0)",
    );
    assert!(matches!(
        fields.as_slice(),
        [
            Value::Float(first),
            Value::Integer(9_007_199_254_740_993),
            Value::Float(third),
            Value::Integer(9_007_199_254_740_993),
        ] if first.to_bits() == 9_007_199_254_740_992.0_f64.to_bits()
            && third.to_bits() == first.to_bits()
    ));
}

#[test]
fn unordered_nan_comparisons_keep_the_first_operand() {
    // Pinned CLIPS 6.30 also produces NaN from sin of this overflow literal.
    // Inspect the stored value without depending on NaN output formatting.
    let fields = result_fields(
        "(min (sin 1.0e309) 1) (min 1 (sin 1.0e309))
         (max (sin 1.0e309) 1) (max 1 (sin 1.0e309))",
    );
    assert!(matches!(
        fields.as_slice(),
        [Value::Float(first), Value::Integer(1), Value::Float(third), Value::Integer(1)]
            if first.is_nan() && third.is_nan()
    ));
}

#[test]
fn nonnumeric_rejection_preserves_eager_left_to_right_argument_effects() {
    // Existing eager evaluation is observable even when numeric validation
    // rejects an early operand. No later RHS action should then execute.
    let arguments = [
        "(mark 1 wrong) (mark 2 4) (mark 3 8)",
        "(mark 1 4) (mark 2 \"wrong\") (mark 3 8)",
        "(mark 1 4) (mark 2 8) (mark 3 (create$ 1 2))",
    ];
    for function in ["min", "max"] {
        for arguments in arguments {
            let source = format!(
                "(defglobal ?*trace* = 0)
                 (deffunction mark (?digit ?value)
                   (bind ?*trace* (+ (* ?*trace* 10) ?digit)) ?value)
                 (defrule compute =>
                   (assert (result ({function} {arguments})))
                   (assert (after)))"
            );
            let mut engine = Engine::with_rules(&source).unwrap();
            let run = engine.run(RunLimit::Count(10)).unwrap();
            assert_eq!(run.halt_reason, HaltReason::ActionError, "{source}");
            assert!(
                engine.action_diagnostics().iter().any(|error| {
                    let message = error.to_string();
                    message.contains(function) && message.contains("INTEGER or FLOAT")
                }),
                "{source}: {:?}",
                engine.action_diagnostics()
            );
            assert!(matches!(
                engine.get_global("trace"),
                Some(Value::Integer(123))
            ));
            assert!(engine.find_facts("result").unwrap().is_empty());
            assert!(engine.find_facts("after").unwrap().is_empty());
        }
    }
}
