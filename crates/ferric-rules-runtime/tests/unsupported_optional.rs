use ferric_rules_runtime::{Engine, EngineConfig, HaltReason, RunLimit};

#[test]
fn nested_fact_queries_reject_before_replacing_a_rule() {
    for expression in [
        "(any-factp ((?f item)) TRUE)",
        "(find-fact ((?f item)) TRUE)",
        "(find-all-facts ((?f item)) TRUE)",
    ] {
        let mut engine =
            Engine::with_rules("(deftemplate item (slot id)) (defrule keep => (assert (kept)))")
                .unwrap();
        let error = engine
            .load_str(&format!("(defrule keep => (assert (result {expression})))"))
            .unwrap_err();
        assert!(
            error.iter().any(|e| e.to_string().contains("unsupported")),
            "{error:?}"
        );
        assert_eq!(engine.run(RunLimit::Unlimited).unwrap().rules_fired, 1);
        assert_eq!(engine.find_facts("kept").unwrap().len(), 1);
        assert!(engine.find_facts("result").unwrap().is_empty());
    }
}

#[test]
fn callable_queries_fail_explicitly_instead_of_returning_false_or_empty() {
    for expression in [
        "(any-factp ((?f item)) TRUE)",
        "(find-fact ((?f item)) TRUE)",
        "(find-all-facts ((?f item)) TRUE)",
    ] {
        let mut engine = Engine::with_rules(&format!(
            "(deftemplate item (slot id))
             (deffacts seed (item (id 7)))
             (deffunction query () {expression})
             (defrule choose => (assert (result (query))) (assert (after-query)))"
        ))
        .unwrap();
        let result = engine.run(RunLimit::Unlimited).unwrap();
        assert_eq!(result.halt_reason, HaltReason::ActionError);
        assert!(engine
            .action_diagnostics()
            .iter()
            .any(|e| e.to_string().contains("unsupported operation")));
        assert!(engine.find_facts("result").unwrap().is_empty());
        assert!(engine.find_facts("after-query").unwrap().is_empty());
        assert_eq!(engine.facts().unwrap().count(), 1);
    }
}

#[test]
fn rhs_query_actions_keep_their_supported_fact_iteration() {
    let mut engine = Engine::with_rules(
        "(deftemplate item (slot id))
         (deffacts seed (item (id 1)) (item (id 2)))
         (defrule inspect => (do-for-all-facts ((?f item)) TRUE (assert (seen ?f))))",
    )
    .unwrap();
    let result = engine.run(RunLimit::Unlimited).unwrap();
    assert_eq!(
        result.halt_reason,
        HaltReason::AgendaEmpty,
        "{:?}",
        engine.action_diagnostics()
    );
    assert_eq!(result.rules_fired, 1);
    assert_eq!(engine.find_facts("seen").unwrap().len(), 2);
}

#[test]
fn refresh_and_dynamic_salience_cannot_promise_unimplemented_behavior() {
    for source in [
        "(defrule keep => (refresh-agenda) (assert (wrong)))",
        "(defrule keep => (assert (wrong (refresh-agenda))))",
        "(defrule keep (declare (salience (+ 1 2))) => (assert (wrong)))",
        "(defrule keep (declare (salience 4294967296)) => (assert (wrong)))",
        "(defrule keep (declare (salience 10001)) => (assert (wrong)))",
        "(defrule keep (declare (auto-focus TRUE)) => (assert (wrong)))",
        "(defrule keep (declare (salience 1) (salience 2)) => (assert (wrong)))",
        "(defrule keep (declare (salience 1)) (declare (salience 2)) => (assert (wrong)))",
    ] {
        let mut engine = Engine::with_rules("(defrule keep => (assert (kept)))").unwrap();
        assert!(engine.load_str(source).is_err(), "accepted {source}");
        assert_eq!(engine.run(RunLimit::Unlimited).unwrap().rules_fired, 1);
        assert_eq!(engine.find_facts("kept").unwrap().len(), 1);
        assert!(engine.find_facts("wrong").unwrap().is_empty());
    }
    let mut engine = Engine::with_rules(
        "(deffunction refresh () (refresh-agenda))
         (defrule choose => (refresh) (assert (wrong)))",
    )
    .unwrap();
    assert_eq!(
        engine.run(RunLimit::Unlimited).unwrap().halt_reason,
        HaltReason::ActionError
    );
    assert!(engine
        .action_diagnostics()
        .iter()
        .any(|e| e.to_string().contains("refresh-agenda")));
    assert!(engine.find_facts("wrong").unwrap().is_empty());
}

#[test]
fn unsupported_strategy_commands_reject_without_installing_a_rule() {
    for strategy in ["simplicity", "complexity", "random"] {
        let mut engine = Engine::new(EngineConfig::default());
        let errors = engine
            .load_str(&format!("(defrule choose => (set-strategy {strategy}))"))
            .unwrap_err();
        assert!(errors
            .iter()
            .any(|e| e.to_string().contains("set-strategy")));
        assert!(engine.rules().is_empty());
    }
}
