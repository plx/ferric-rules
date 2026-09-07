use ferric_rules_runtime::{Engine, EngineConfig, HaltReason, LoadError, RunLimit};

#[test]
fn logical_is_rejected_in_every_original_tree_position_before_installation() {
    for condition in [
        "(logical (seed))",
        "(and (logical (seed)))",
        "(or (seed) (logical (seed)))",
        "(not (and (seed) (logical (other))))",
        "(exists (logical (seed)))",
        "?f <- (logical (seed))",
    ] {
        let mut engine = Engine::new(EngineConfig::default());
        let errors = engine
            .load_str(&format!(
                "(defrule derived\n {condition}\n => (assert (dependent)))"
            ))
            .expect_err("logical semantics must never be silently discarded");
        assert!(errors.iter().any(|error| matches!(error, LoadError::Compile(message)
            if message.contains("logical") && message.contains("truth maintenance") && message.contains("line 2"))), "{errors:?}");
        assert!(engine.rules().is_empty());
        engine.assert_ordered("seed", ()).unwrap();
        let result = engine.run(RunLimit::Unlimited).unwrap();
        assert_eq!(result.rules_fired, 0);
        assert!(engine.find_facts("dependent").unwrap().is_empty());
        #[cfg(debug_assertions)]
        engine.debug_assert_consistency();
    }
}

#[test]
fn logical_replacement_preserves_installed_rule_and_its_pending_match() {
    let mut engine =
        Engine::with_rules("(deffacts startup (seed)) (defrule derived (seed) => (assert (old)))")
            .unwrap();
    assert!(engine
        .load_str("(defrule derived (logical (seed)) => (assert (dependent)))")
        .is_err());
    let result = engine.run(RunLimit::Unlimited).unwrap();
    assert_eq!(result.rules_fired, 1);
    assert_eq!(result.halt_reason, HaltReason::AgendaEmpty);
    assert_eq!(engine.find_facts("old").unwrap().len(), 1);
    assert!(engine.find_facts("dependent").unwrap().is_empty());
}
