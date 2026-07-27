//! Pinned runs preserve the first action diagnostic and stop before later work.

use ferric_rules_pinned::{HaltReason, PinnedEngine, PinnedEngineOptions, RunLimit};

#[test]
fn run_stops_at_first_action_diagnostic_without_consuming_later_work() {
    let engine = PinnedEngine::new(PinnedEngineOptions::default()).unwrap();
    engine
        .load_str(
            r"
            (defrule count-with-error
                ?f <- (counter ?n&:(< ?n 65))
                =>
                (retract ?f)
                (assert (counter (+ ?n 1)))
                (bind ?quotient (/ 1 0)))
            (deffacts initial (counter 0))
            ",
        )
        .unwrap();
    engine.reset().unwrap();

    let result = engine.run(RunLimit::Unlimited).unwrap();
    let diagnostic_count = engine
        .with_engine(|engine| Ok(engine.action_diagnostics().len()))
        .unwrap();

    assert_eq!(result.rules_fired, 1);
    assert_eq!(result.halt_reason, HaltReason::ActionError);
    assert_eq!(diagnostic_count, 1);

    let next = engine.run(RunLimit::Unlimited).unwrap();
    let next_diagnostic_count = engine
        .with_engine(|engine| Ok(engine.action_diagnostics().len()))
        .unwrap();
    assert_eq!(next.rules_fired, 1);
    assert_eq!(next.halt_reason, HaltReason::ActionError);
    assert_eq!(next_diagnostic_count, 1);
}
