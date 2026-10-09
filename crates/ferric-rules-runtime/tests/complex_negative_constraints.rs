//! Explicit direct-negation boundary and its supported match-time alternative.

use ferric_rules_runtime::{Engine, EngineConfig, HaltReason, LoadError, RunLimit};

#[test]
fn rejected_direct_constraints_do_not_install_and_ncc_tests_track_blockers() {
    // Both direct forms are accepted by CLIPS 6.30. Ferric deliberately rejects
    // them; the explicit NCC + test alternative below is accepted by both.
    let mut engine = Engine::new(EngineConfig::default());
    engine.assert_ordered("anchor", 2_i64).unwrap();
    let blocker = engine.assert_ordered("data", 3_i64).unwrap();
    for condition in [
        "(not (data ?x&:(> (* ?x ?x) (* ?min ?min))))",
        "(not (data ?x&=(* ?x ?x)))",
    ] {
        let source = format!(
            "(defrule candidate (anchor ?min)\n  {condition}\n  => (printout t rejected crlf))"
        );
        let errors = engine
            .load_str(&source)
            .expect_err("CLIPS-valid direct nonlinear constraints are unsupported");
        assert!(errors.iter().any(|error| matches!(
            error,
            LoadError::Compile(message)
                if message.contains("complex constraints inside negated patterns")
                    && message.contains("line ")
                    && message.contains("column ")
        )));
        assert!(engine.rules().is_empty());
        assert_eq!(engine.agenda_len(), 0);
        assert_eq!(engine.fact_count(), 2);
        assert_eq!(engine.run(RunLimit::Unlimited).unwrap().rules_fired, 0);
        assert_eq!(engine.get_output("t"), None);
    }

    engine
        .load_str(
            r#"(defrule candidate
                (anchor ?min)
                (not (and (data ?x)
                          (test (> (* ?x ?x) (* ?min ?min)))))
                => (printout t "safe" crlf))"#,
        )
        .unwrap();
    let irrelevant = engine.assert_ordered("data", 1_i64).unwrap();
    assert_eq!(engine.agenda_len(), 0);
    engine.retract(blocker).unwrap();
    assert_eq!(engine.agenda_len(), 1);
    assert_eq!(engine.get_output("t"), None);

    // The replacement witness must cancel the pending activation before run.
    let replacement = engine.assert_ordered("data", 4_i64).unwrap();
    assert_eq!(engine.agenda_len(), 0);
    assert_eq!(engine.run(RunLimit::Unlimited).unwrap().rules_fired, 0);
    assert_eq!(engine.get_output("t"), None);
    engine.retract(irrelevant).unwrap();
    assert_eq!(engine.agenda_len(), 0);
    engine.retract(replacement).unwrap();
    assert_eq!(engine.agenda_len(), 1);

    let result = engine.run(RunLimit::Unlimited).unwrap();
    assert_eq!(result.rules_fired, 1);
    assert_eq!(result.halt_reason, HaltReason::AgendaEmpty);
    assert_eq!(engine.get_output("t"), Some("safe\n"));
    assert!(engine.action_diagnostics().is_empty());
}
