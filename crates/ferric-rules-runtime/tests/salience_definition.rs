//! Definition-time salience must observe its own source scope and effects.
use ferric_rules_runtime::{Engine, EngineConfig, RunLimit};

#[test]
fn unbound_salience_local_preserves_prior_effects_and_old_rule() {
    let mut engine = Engine::new(EngineConfig::default());
    engine
        .load_str("(defrule keep => (assert (old-definition)))")
        .unwrap();
    let errors = engine
        .load_str(
            "(defrule keep
               (declare (salience (progn (printout t before crlf) (assert (marker)) ?missing)))
               => (assert (wrong-definition)))
             (defrule later => (assert (later-definition)))",
        )
        .unwrap_err();
    assert!(errors
        .iter()
        .any(|error| error.to_string().contains("missing")));
    assert_eq!(engine.get_output("t"), Some("before\n"));
    assert_eq!(engine.find_facts("marker").unwrap().len(), 1);
    assert_eq!(engine.run(RunLimit::Unlimited).unwrap().rules_fired, 2);
    assert_eq!(engine.find_facts("old-definition").unwrap().len(), 1);
    assert_eq!(engine.find_facts("later-definition").unwrap().len(), 1);
    assert!(engine.find_facts("wrong-definition").unwrap().is_empty());
    engine.rete().debug_assert_consistency();
}

#[test]
fn rule_pattern_cannot_declare_a_template_before_its_salience_expression() {
    let mut engine = Engine::new(EngineConfig::default());
    let errors = engine
        .load_str(
            "(defrule bad
               (declare (salience (if (any-factp ((?f future)) TRUE) then 1 else 0)))
               (future) => (assert (wrong)))
             (defrule good => (assert (ready)))",
        )
        .unwrap_err();
    assert!(errors
        .iter()
        .any(|error| error.to_string().contains("future")));
    // The rejected rule's later LHS must not register an implied template that
    // would prevent an explicit declaration in a subsequent load.
    engine.load_str("(deftemplate future (slot n))").unwrap();
    assert_eq!(engine.run(RunLimit::Unlimited).unwrap().rules_fired, 1);
    assert_eq!(engine.find_facts("ready").unwrap().len(), 1);
    assert!(engine.find_facts("wrong").unwrap().is_empty());
    engine.rete().debug_assert_consistency();
}

#[test]
fn invalid_qualified_rule_identity_rejects_before_salience_effects() {
    for name in ["UNKNOWN::bad", "MAIN::nested::bad"] {
        let mut engine = Engine::new(EngineConfig::default());
        let source = format!(
            "(defrule {name}
               (declare (salience (progn (printout t leaked crlf) (assert (leaked)) 1))) =>)
             (defrule good => (assert (ready)))"
        );
        assert!(engine.load_str(&source).is_err(), "{name}");
        assert!(engine.get_output("t").map_or(true, str::is_empty), "{name}");
        assert!(engine.find_facts("leaked").unwrap().is_empty(), "{name}");
        assert_eq!(engine.run(RunLimit::Unlimited).unwrap().rules_fired, 1);
        assert_eq!(engine.find_facts("ready").unwrap().len(), 1);
        engine.rete().debug_assert_consistency();
    }
}
