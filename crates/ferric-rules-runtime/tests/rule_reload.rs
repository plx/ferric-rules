//! Failure-atomic source rule replacement and bounded RETE reclamation.

use ferric_rules_runtime::{Engine, HaltReason, RunLimit, Value};

fn run(engine: &mut Engine, expected: usize) {
    let result = engine.run(RunLimit::Unlimited).unwrap();
    assert_eq!(result.rules_fired, expected);
    assert_eq!(
        result.halt_reason,
        HaltReason::AgendaEmpty,
        "{:?}",
        engine.action_diagnostics()
    );
    assert!(engine.action_diagnostics().is_empty());
    #[cfg(debug_assertions)]
    engine.debug_assert_consistency();
}

#[test]
fn replacement_removes_queued_old_actions_and_backfills_current_facts() {
    let mut engine = Engine::with_rules(
        "(deffacts data (left ready) (right ready)) (defrule choose (left ready) => (printout t old crlf) (assert (result old)))",
    ).unwrap();
    assert_eq!(engine.agenda_len(), 1);
    let fact_ids: Vec<_> = engine.facts().unwrap().map(|(id, _)| id).collect();
    engine
        .load_str("(defrule choose (right ready) => (printout t new crlf) (assert (result new)))")
        .unwrap();
    assert_eq!(engine.rules().len(), 1);
    for id in fact_ids {
        assert!(engine.get_fact(id).unwrap().is_some());
    }
    run(&mut engine, 1);
    assert_eq!(engine.get_output("t"), Some("new\n"));
    assert_eq!(engine.find_facts("result").unwrap().len(), 1);
}

#[test]
fn replacement_preserves_shared_siblings_without_refiring_them() {
    let mut engine = Engine::with_rules(
        "(deffacts data (subject a) (gate open) (proof a))
         (defrule choose (subject ?x) (gate open) => (assert (old ?x)))
         (defrule sibling (declare (salience 10)) (subject ?x) (gate open) => (printout t sibling crlf))",
    ).unwrap();
    assert_eq!(engine.run(RunLimit::Count(1)).unwrap().rules_fired, 1);
    engine
        .load_str("(defrule choose (subject ?x) (not (block ?x)) => (assert (middle ?x)))")
        .unwrap();
    engine.load_str("(defrule choose (subject ?x) (gate open) (exists (proof ?x)) => (printout t final crlf))").unwrap();
    run(&mut engine, 1);
    assert_eq!(engine.get_output("t"), Some("sibling\nfinal\n"));
    assert!(engine.find_facts("old").unwrap().is_empty());
    assert!(engine.find_facts("middle").unwrap().is_empty());
}

#[test]
fn failed_replacement_leaves_old_rule_matches_and_network_unchanged() {
    let mut engine = Engine::with_rules(
        "(deffacts data (subject ok)) (defrule choose (subject ?x) => (printout t old crlf))",
    )
    .unwrap();
    for rhs in ["(missing-callable)", "(assert (bad ?missing))"] {
        let before = engine.rete().cardinality();
        assert!(engine
            .load_str(&format!("(defrule choose (subject ?x) => {rhs})"))
            .is_err());
        assert_eq!(engine.rete().cardinality(), before);
        assert_eq!(engine.rules().len(), 1);
    }
    run(&mut engine, 1);
    assert_eq!(engine.get_output("t"), Some("old\n"));
}

#[test]
fn replacement_identity_is_module_and_local_name() {
    let mut engine = Engine::with_rules(
        "(defmodule A) (deftemplate A::item (slot value))
         (defrule A::choose (item (value ?v)) => (printout t A-old crlf))
         (defmodule B) (deftemplate B::item (slot value))
         (defrule B::choose (item (value ?v)) => (printout t B crlf))",
    )
    .unwrap();
    engine
        .load_str("(defmodule A) (defrule choose (item (value ?v)) => (printout t A-new crlf))")
        .unwrap();
    engine
        .assert_template("A::item", &["value"], vec![Value::Integer(1)])
        .unwrap();
    engine
        .assert_template("B::item", &["value"], vec![Value::Integer(1)])
        .unwrap();
    engine.set_focus("B").unwrap();
    run(&mut engine, 1);
    engine.set_focus("A").unwrap();
    run(&mut engine, 1);
    assert_eq!(engine.get_output("t"), Some("B\nA-new\n"));
    assert_eq!(engine.rules().len(), 2);
}

#[test]
fn replacement_reclaims_all_disjunction_variants() {
    let mut engine = Engine::with_rules(
        "(deffacts data (left a) (right a)) (defrule choose (or (left ?x) (right ?x)) => (assert (old ?x)))",
    ).unwrap();
    assert_eq!(engine.agenda_len(), 2);
    engine
        .load_str("(defrule choose (right ?x) => (assert (new ?x)))")
        .unwrap();
    run(&mut engine, 1);
    assert!(engine.find_facts("old").unwrap().is_empty());
    assert_eq!(engine.find_facts("new").unwrap().len(), 1);
}

#[test]
fn repeated_replacement_keeps_positive_negative_exists_and_ncc_state_bounded() {
    let patterns = [
        "(subject ?x) (gate open)",
        "(subject ?x) (not (block ?x))",
        "(subject ?x) (exists (proof ?x))",
        "(subject ?x) (not (and (block ?x) (proof ?x)))",
        "(subject ?x) (test (> ?x 0))",
    ];
    let mut engine = Engine::with_rules(
        "(deffacts data (subject 1) (gate open) (proof 1) (block 1))
         (defrule sibling (subject ?x) (gate open) => (assert (sibling ?x)))",
    )
    .unwrap();
    run(&mut engine, 1);
    let mut counts = Vec::new();
    for cycle in 0..100 {
        for (index, lhs) in patterns.iter().enumerate() {
            engine
                .load_str(&format!("(defrule choose {lhs} => (assert (chosen)))"))
                .unwrap();
            run(&mut engine, usize::from(index != 1 && index != 3));
            let cardinality = engine.rete().cardinality();
            if cycle == 0 {
                counts.push(cardinality);
            } else {
                assert_eq!(cardinality, counts[index], "network grew in cycle {cycle}");
            }
            assert_eq!(engine.rules().len(), 2);
        }
    }
}

#[test]
fn undefrule_cycles_reclaim_graphs_and_keep_independent_rules_live() {
    let mut engine =
        Engine::with_rules("(defrule survivor (stable ?x) => (assert (survived ?x)))").unwrap();
    let baseline = engine.rete().cardinality();
    for cycle in 0..100 {
        engine
            .load_str(&format!(
                "(defrule temporary-{cycle} (unused-{cycle}) =>)
             (defrule erase (declare (salience 100)) => (undefrule temporary-{cycle} erase))"
            ))
            .unwrap();
        run(&mut engine, 1);
        assert_eq!(engine.rete().cardinality(), baseline, "cycle {cycle}");
        assert_eq!(engine.rules().len(), 1);
    }
    engine
        .assert_ordered("stable", vec![Value::Integer(4)])
        .unwrap();
    run(&mut engine, 1);
    assert_eq!(engine.find_facts("survived").unwrap().len(), 1);
}

#[cfg(feature = "serde")]
#[test]
fn replacement_after_restore_preserves_compiler_sharing_and_retirement() {
    use ferric_rules_runtime::SerializationFormat;
    let mut engine = Engine::with_rules(
        "(deffacts data (subject 1) (proof 1)) (defrule choose (subject ?x) => (assert (old)))",
    )
    .unwrap();
    engine
        .load_str("(defrule choose (subject ?x) (exists (proof ?x)) => (assert (middle)))")
        .unwrap();
    for &format in SerializationFormat::ALL {
        let mut restored = Engine::deserialize(&engine.serialize(format).unwrap(), format).unwrap();
        restored
            .load_str("(defrule choose (subject ?x) (not (block ?x)) => (assert (new)))")
            .unwrap();
        run(&mut restored, 1);
        assert!(restored.find_facts("old").unwrap().is_empty());
        assert!(restored.find_facts("middle").unwrap().is_empty());
        assert_eq!(restored.find_facts("new").unwrap().len(), 1);
    }
}
