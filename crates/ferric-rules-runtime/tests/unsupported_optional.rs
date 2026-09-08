use ferric_rules_runtime::{Engine, EngineConfig, HaltReason, RunLimit};

#[test]
fn malformed_or_unsupported_queries_reject_before_replacing_a_rule() {
    for expression in [
        "(any-factp ((?f missing)) TRUE)",
        "(find-fact () TRUE)",
        "(find-all-facts ((?f item) (?f item)) TRUE)",
        "(any-factp ((?f item)) (bind ?temporary 1))",
        "(length$ (do-for-all-facts ((?f item)) TRUE (printout t ignored)))",
    ] {
        let mut engine =
            Engine::with_rules("(deftemplate item (slot id)) (defrule keep => (assert (kept)))")
                .unwrap();
        let error = engine
            .load_str(&format!("(defrule keep => (assert (result {expression})))"))
            .unwrap_err();
        assert!(!error.is_empty(), "{expression}");
        assert_eq!(engine.run(RunLimit::Unlimited).unwrap().rules_fired, 1);
        assert_eq!(engine.find_facts("kept").unwrap().len(), 1);
        assert!(engine.find_facts("result").unwrap().is_empty());
    }
}

#[test]
fn callable_result_queries_return_matching_and_empty_results() {
    for (expression, empty_expression, expected) in [
        (
            "(any-factp ((?f item)) TRUE)",
            "(any-factp ((?f item)) FALSE)",
            "TRUE:FALSE\n",
        ),
        (
            "(length$ (find-fact ((?f item)) TRUE))",
            "(length$ (find-fact ((?f item)) FALSE))",
            "1:0\n",
        ),
        (
            "(length$ (find-all-facts ((?f item)) TRUE))",
            "(length$ (find-all-facts ((?f item)) FALSE))",
            "1:0\n",
        ),
    ] {
        let mut engine = Engine::with_rules(&format!(
            "(deftemplate item (slot id))
             (deffacts seed (item (id 7)))
             (deffunction query () {expression})
             (deffunction empty-query () {empty_expression})
             (defrule choose => (printout t (query) \":\" (empty-query) crlf) (assert (after-query)))"
        ))
        .unwrap();
        let result = engine.run(RunLimit::Unlimited).unwrap();
        assert_eq!(result.halt_reason, HaltReason::AgendaEmpty);
        assert!(
            engine.action_diagnostics().is_empty(),
            "{:?}",
            engine.action_diagnostics()
        );
        assert_eq!(engine.get_output("t"), Some(expected));
        assert_eq!(engine.find_facts("after-query").unwrap().len(), 1);
    }
}

#[test]
fn callable_action_queries_remain_explicitly_unsupported() {
    for action_query in [
        "do-for-fact",
        "do-for-all-facts",
        "delayed-do-for-all-facts",
    ] {
        let mut engine = Engine::with_rules(&format!(
            "(deftemplate item (slot id))
             (deffacts seed (item (id 7)))
             (deffunction query () ({action_query} ((?f item)) TRUE (assert (wrong))))
             (defrule choose => (query) (assert (after-query)))"
        ))
        .unwrap();
        let result = engine.run(RunLimit::Unlimited).unwrap();
        assert_eq!(result.halt_reason, HaltReason::ActionError);
        assert!(engine
            .action_diagnostics()
            .iter()
            .any(|e| e.to_string().contains("unsupported operation")));
        assert!(engine.find_facts("wrong").unwrap().is_empty());
        assert!(engine.find_facts("after-query").unwrap().is_empty());
        assert_eq!(engine.facts().unwrap().count(), 1);
    }
}

#[test]
fn invalid_result_query_declarations_are_rejected_in_callable_definitions() {
    for expression in [
        "(any-factp ((?f missing)) TRUE)",
        "(find-fact () TRUE)",
        "(find-all-facts ((?f item) (?f item)) TRUE)",
        "(any-factp ((?f item)) (bind ?unrelated 1))",
    ] {
        for callable in [
            format!("(deffunction query () {expression})"),
            format!("(defgeneric query) (defmethod query ((?minimum INTEGER)) {expression})"),
        ] {
            let mut engine = Engine::new(EngineConfig::default());
            engine.load_str("(deftemplate item (slot id))").unwrap();
            assert!(engine.load_str(&callable).is_err(), "accepted {callable}");
        }
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
