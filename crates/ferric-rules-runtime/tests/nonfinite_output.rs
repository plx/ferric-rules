//! Explicit host values separate NaN rendering from platform libm choices.

use ferric_rules_runtime::{Engine, HaltReason, RunLimit, Value};

#[test]
fn printout_and_implode_render_both_explicit_nan_signs_as_nan() {
    let mut engine = Engine::with_rules(
        r#"(defrule render (values ?positive ?negative) =>
          (printout t ?positive "|" ?negative crlf)
          (printout t (create$ ?positive ?negative) crlf)
          (printout t (implode$ (create$ ?positive ?negative)) crlf))"#,
    )
    .unwrap();
    engine
        .assert_ordered(
            "values",
            [
                Value::Float(f64::from_bits(0x7ff8_0000_0000_0001)),
                Value::Float(f64::from_bits(0xfff8_0000_0000_0001)),
            ],
        )
        .unwrap();

    let result = engine.run(RunLimit::Unlimited).unwrap();
    assert_eq!(result.rules_fired, 1);
    assert_eq!(result.halt_reason, HaltReason::AgendaEmpty);
    assert!(engine.action_diagnostics().is_empty());
    assert_eq!(
        engine.get_output("t"),
        Some("nan.0|nan.0\n(nan.0 nan.0)\nnan.0 nan.0\n")
    );
}
