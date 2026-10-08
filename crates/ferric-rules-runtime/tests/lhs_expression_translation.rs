//! Match-time expressions use the same typed conditional grammar as ordinary expressions.

use ferric_rules_runtime::{Engine, EngineConfig, RunLimit};

#[test]
fn conditionals_work_in_test_predicate_return_and_connected_constraint_positions() {
    let mut engine = Engine::new(EngineConfig::utf8());
    engine.load_str(r"
        (deffacts seed (value -1) (value 2))
        (defrule test-if (value ?x) (test (if (> ?x 0) then TRUE else FALSE)) => (printout t A))
        (defrule test-switch (value ?x) (test (switch ?x (case 2 then TRUE) (default FALSE))) => (printout t B))
        (defrule predicate (value ?x&:(if (> ?x 0) then TRUE else FALSE)) => (printout t C))
        (defrule returned (value =(if TRUE then 2 else 0)) => (printout t D))
        (defrule connected (value ?x&:(if (> ?x 0) then TRUE else FALSE)|99) => (printout t E))
    ").unwrap();
    engine.reset().unwrap();
    assert_eq!(engine.run(RunLimit::Unlimited).unwrap().rules_fired, 5);
    let mut output: Vec<_> = engine.get_output("t").unwrap().chars().collect();
    output.sort_unstable();
    assert_eq!(output, ['A', 'B', 'C', 'D', 'E']);
    assert!(engine.action_diagnostics().is_empty());
}

#[test]
fn conditional_test_inside_ncc_filters_each_counterexample() {
    let mut engine = Engine::new(EngineConfig::utf8());
    engine
        .load_str(
            r"
        (defrule no-positive
            (not (and (value ?x) (test (if (> ?x 0) then TRUE else FALSE))))
            => (printout t clear))
    ",
        )
        .unwrap();
    engine.reset().unwrap();
    engine.assert_ordered("value", -1_i64).unwrap();
    assert_eq!(engine.agenda_len(), 1);
    let blocker = engine.assert_ordered("value", 2_i64).unwrap();
    assert_eq!(engine.agenda_len(), 0);
    engine.retract(blocker).unwrap();
    assert_eq!(engine.run(RunLimit::Unlimited).unwrap().rules_fired, 1);
    assert_eq!(engine.get_output("t"), Some("clear"));
    assert!(engine.action_diagnostics().is_empty());
}

#[test]
fn malformed_lhs_conditional_has_a_located_parser_diagnostic() {
    let mut engine = Engine::new(EngineConfig::utf8());
    let errors = engine
        .load_str("(defrule bad\n (go)\n (test (if TRUE wrong 1))\n =>)")
        .unwrap_err();
    let diagnostic = errors
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join("\n");
    assert!(diagnostic.contains("line 3, column 8"), "{diagnostic}");
    assert!(diagnostic.contains("then"), "{diagnostic}");
    assert!(!diagnostic.contains("List(["), "{diagnostic}");
}

#[test]
fn typed_lhs_conditional_preserves_engine_effect_rejection() {
    let mut engine = Engine::new(EngineConfig::utf8());
    let loaded = engine.load_str(
        r"
        (defmodule OTHER)
        (defmodule MAIN)
        (defrule guarded (go) (test (if TRUE then (focus OTHER) else FALSE)) => (printout t bad))
    ",
    );
    if loaded.is_ok() {
        engine.assert_ordered("go", Vec::<i64>::new()).unwrap();
        assert_eq!(engine.agenda_len(), 0);
        assert!(!engine.action_diagnostics().is_empty());
    }
    assert_eq!(engine.get_focus_stack(), vec!["MAIN"]);
    assert_eq!(engine.get_output("t"), None);
}

#[test]
fn lhs_result_queries_reject_variables_not_bound_at_their_source_position() {
    for conditions in [
        "(go) (test (any-factp ((?f p)) (= ?f:x ?missing)))",
        "(go) (test (any-factp ((?f ?missing)) TRUE))",
        "(value ?x&:(any-factp ((?f p)) (= ?f:x ?missing)))",
        "(value =(length$ (find-all-facts ((?f p)) (= ?f:x ?missing))))",
        "(not (and (value ?x) (test (any-factp ((?f p)) (= ?f:x ?missing)))))",
        "(go) (test (any-factp ((?f p)) (= ?f:x ?missing))) (value ?missing)",
        "(value ?x&:(any-factp ((?f p)) (= ?f:x ?missing)) ?missing)",
        "(pair (a ?x&:(any-factp ((?f p)) (= ?f:x ?missing))) (b ?missing))",
        "(go) (test (do-for-all-facts ((?f p)) (= ?f:x ?missing) TRUE))",
        "(go) (test (do-for-all-facts ((?f p)) TRUE ?missing))",
    ] {
        let mut engine = Engine::new(EngineConfig::utf8());
        let errors = engine
            .load_str(&format!(
                "(deftemplate p (slot x)) (deftemplate pair (slot a) (slot b))\n(defrule invalid {conditions} =>)"
            ))
            .expect_err(conditions);
        let diagnostic = errors
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join("\n");
        assert!(diagnostic.contains("missing"), "{conditions}: {diagnostic}");
        assert!(diagnostic.contains("line 2, column"), "{diagnostic}");
    }
}

#[test]
fn lhs_result_queries_keep_outer_field_existential_and_nested_member_scopes() {
    let mut engine = Engine::new(EngineConfig::utf8());
    engine
        .load_str(
            r"
        (deftemplate p (slot x))
        (deftemplate pair (slot a) (slot b))
        (deffacts seed (p (x 7)) (value 7) (two 7 7) (pair (a 7) (b 7)) (kind p))
        (defrule outer (value ?x) (test (any-factp ((?f p)) (= ?f:x ?x))) => (printout t A))
        (defrule field (value ?x&:(any-factp ((?f p)) (= ?f:x ?x))) => (printout t B))
        (defrule local (not (and (value ?x) (test (any-factp ((?f p)) (> ?f:x ?x))))) => (printout t C))
        (defrule nested (value ?x)
            (test (any-factp ((?f p)) (any-factp ((?g p)) (= ?f:x ?g:x ?x))))
            => (printout t D))
        (defrule earlier-field (two ?x ?y&:(any-factp ((?f p)) (= ?f:x ?x ?y))) => (printout t E))
        (defrule earlier-slot (pair (b ?x) (a ?y&:(any-factp ((?f p)) (= ?f:x ?x ?y)))) => (printout t F))
        (defrule own-address ?a <- (p (x ?x))
            (test (any-factp ((?f p)) (eq ?f ?a))) => (printout t G))
        (defrule dynamic (kind ?name) (test (any-factp ((?f ?name)) TRUE)) => (printout t H))
    ",
        )
        .unwrap();
    engine.reset().unwrap();
    assert_eq!(
        engine.run(RunLimit::Unlimited).unwrap().rules_fired,
        8,
        "output: {:?}, diagnostics: {:?}",
        engine.get_output("t"),
        engine.action_diagnostics()
    );
    let mut output: Vec<_> = engine.get_output("t").unwrap().chars().collect();
    output.sort_unstable();
    assert_eq!(output, ['A', 'B', 'C', 'D', 'E', 'F', 'G', 'H']);
    assert!(engine.action_diagnostics().is_empty());
}

#[cfg(feature = "serde")]
#[test]
fn lhs_query_pattern_addresses_survive_reset_and_snapshot_continuation() {
    use ferric_rules_runtime::SerializationFormat;

    for &format in SerializationFormat::ALL {
        let mut engine = Engine::new(EngineConfig::utf8());
        engine
            .load_str(
                r"
            (deftemplate p (slot x))
            (deffacts seed (p (x 7)))
            (defrule address ?a <- (p (x ?x))
                (test (any-factp ((?f p)) (eq ?f ?a))) => (printout t matched))
        ",
            )
            .unwrap();
        engine.reset().unwrap();
        assert_eq!(engine.agenda_len(), 1);
        let bytes = engine.serialize(format).unwrap();
        let mut restored = Engine::deserialize(&bytes, format).unwrap();
        assert_eq!(restored.run(RunLimit::Unlimited).unwrap().rules_fired, 1);
        assert_eq!(restored.get_output("t"), Some("matched"));
        assert!(restored.action_diagnostics().is_empty());

        restored.reset().unwrap();
        assert_eq!(restored.agenda_len(), 1);
        let bytes = restored.serialize(format).unwrap();
        let mut restored = Engine::deserialize(&bytes, format).unwrap();
        assert_eq!(restored.run(RunLimit::Unlimited).unwrap().rules_fired, 1);
        assert_eq!(restored.get_output("t"), Some("matched"));
        assert!(restored.action_diagnostics().is_empty());
    }
}
