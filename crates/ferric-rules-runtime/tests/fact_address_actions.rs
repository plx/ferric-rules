//! Fact addresses keep their type and captured identity across RHS operations.

use ferric_rules_runtime::{Engine, EngineConfig, HaltReason, RunLimit};

fn run(source: &str) -> Engine {
    let mut engine = Engine::new(EngineConfig::default());
    engine.load_str(source).unwrap();
    engine.reset().unwrap();
    let result = engine.run(RunLimit::Count(20)).unwrap();
    assert_eq!(result.halt_reason, HaltReason::AgendaEmpty);
    assert!(engine.action_diagnostics().is_empty());
    engine
}

#[test]
fn pattern_and_query_members_emit_the_same_typed_addresses() {
    let engine = run(r#"
        (deftemplate item (slot v))
        (deffacts seed (item (v 1)) (item (v 2)))
        (defrule inspect ?f <- (item (v 1)) =>
          (printout t ?f " " (integerp ?f) " " (numberp ?f) crlf)
          (printout t (find-all-facts ((?g item)) TRUE) crlf)
          (do-for-fact ((?g item)) (= ?g:v 1)
            (printout t ?g " " (eq ?f ?g) crlf)))
        "#);
    assert_eq!(
        engine.get_output("t"),
        Some("<Fact-1> FALSE FALSE\n(<Fact-1> <Fact-2>)\n<Fact-1> TRUE\n")
    );
}

#[test]
fn delayed_query_addresses_retain_metadata_after_prior_body_retractions() {
    let engine = run(r#"
        (deftemplate item (slot v))
        (deffacts seed (item (v 1)) (item (v 2)))
        (defrule inspect =>
          (delayed-do-for-all-facts ((?g item)) TRUE
            (printout t ?g ":" ?g:v "|")
            (do-for-all-facts ((?h item)) TRUE (retract ?h))
            (printout t ?g ":" ?g:v ":" (fact-existp ?g) crlf)))
        "#);
    assert_eq!(
        engine.get_output("t"),
        Some("<Fact-1>:1|<Fact-1>:1:FALSE\n<Fact-2>:2|<Fact-2>:2:FALSE\n")
    );
    assert_eq!(engine.fact_count(), 0);
}

#[test]
fn missing_negative_and_raw_key_integers_do_not_retract_a_fact_or_stop_the_rule() {
    let engine = run(r#"
        (defglobal ?*evaluations* = 0)
        (deffacts seed (item 7))
        (defrule inspect (item ?value) =>
          (retract 9 -1 4294967298)
          (printout t (fact-existp 1) " " (fact-relation 4294967298) crlf)
          (retract 9 (bind ?*evaluations* (+ ?*evaluations* 1)) -1)
          (printout t (fact-existp 1) " " ?*evaluations* " continued" crlf))
        "#);
    assert_eq!(
        engine.get_output("t"),
        Some("TRUE FALSE\nFALSE 1 continued\n")
    );
    assert_eq!(engine.fact_count(), 0);
}

#[test]
fn missing_mutation_indices_and_negative_retract_indices_continue_the_rule() {
    // CLIPS 6.30: a missing index makes modify/duplicate a no-op without
    // evaluating the overrides; a negative retract index stops that retract
    // before later targets are evaluated, and the rule continues.
    let engine = run(r#"
        (deftemplate item (slot value))
        (deffacts seed (item (value 1)))
        (deffunction boom () (printout t "evaluated|") 1)
        (defrule inspect ?f <- (item (value 1)) =>
          (bind ?i 9)
          (modify ?i (value (boom)))
          (duplicate ?i (value (boom)))
          (retract -1 ?f (boom))
          (printout t (fact-existp ?f) ":continued" crlf))
        "#);
    assert_eq!(engine.get_output("t"), Some("TRUE:continued\n"));
    assert_eq!(engine.fact_count(), 1);
}

#[test]
fn negative_and_wrong_type_mutation_targets_still_stop_execution() {
    for action in [
        "(bind ?i -1) (modify ?i (value 2))",
        "(bind ?i -1) (duplicate ?i (value 2))",
        "(bind ?i \"x\") (modify ?i (value 2))",
    ] {
        let mut engine = Engine::new(EngineConfig::default());
        engine
            .load_str(&format!(
                "(deftemplate item (slot value))
                 (deffacts seed (item (value 1)))
                 (defrule inspect (item) => (printout t before) {action} (printout t after))"
            ))
            .unwrap();
        engine.reset().unwrap();
        assert_eq!(
            engine.run(RunLimit::Count(10)).unwrap().halt_reason,
            HaltReason::ActionError,
            "{action}"
        );
        assert_eq!(engine.get_output("t"), Some("before"), "{action}");
        assert_eq!(engine.fact_count(), 1);
        assert_eq!(engine.action_diagnostics().len(), 1);
    }
}

#[test]
fn wrong_type_retract_targets_stop_the_rule_after_retracting_later_targets() {
    // CLIPS 6.30 reports the wrong-type operand, keeps retracting the
    // remaining targets, and then halts the rule.
    let mut engine = Engine::new(EngineConfig::default());
    engine
        .load_str(
            "(deffacts seed (item))
             (defrule inspect ?f <- (item) =>
               (printout t before)
               (retract \"not an address\" ?f)
               (printout t after))",
        )
        .unwrap();
    engine.reset().unwrap();
    assert_eq!(
        engine.run(RunLimit::Count(10)).unwrap().halt_reason,
        HaltReason::ActionError
    );
    assert_eq!(engine.get_output("t"), Some("before"));
    assert_eq!(engine.fact_count(), 0);
    assert_eq!(engine.action_diagnostics().len(), 1);
}

#[test]
fn save_facts_writes_address_spellings_as_strings() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("addresses.fct");
    let path_literal = format!("{:?}", path.to_string_lossy());
    let engine = run(&format!(
        "(deftemplate item (slot v))
         (deftemplate saved (slot address (type FACT-ADDRESS)))
         (deffacts seed (item (v 1)) (saved))
         (defrule save ?f <- (item) =>
           (assert (saved (address ?f)))
           (retract ?f)
           (save-facts {path_literal}))"
    ));
    assert_eq!(engine.fact_count(), 2);
    let saved = std::fs::read_to_string(path).unwrap();
    assert!(saved.contains("(address \"<Fact-1>\")"), "{saved}");
    assert!(saved.contains("(address \"<Dummy Fact>\")"), "{saved}");
}
