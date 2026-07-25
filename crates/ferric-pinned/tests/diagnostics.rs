//! Diagnostics produced across pinned run chunks remain available afterward.

use ferric_pinned::{HaltReason, PinnedEngine, PinnedEngineOptions, RunLimit};

#[test]
fn run_accumulates_action_diagnostics_across_chunks() {
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

    assert_eq!(result.rules_fired, 65);
    assert_eq!(result.halt_reason, HaltReason::AgendaEmpty);
    assert_eq!(diagnostic_count, 65);
}
