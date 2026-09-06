//! Template identities remain coherent across unused and rejected live reloads.

use ferric_rules_runtime::{Engine, EngineConfig, RunLimit, Value};

fn rejected(engine: &mut Engine) {
    let errors = engine
        .load_str("(deftemplate record (slot changed))")
        .unwrap_err();
    assert!(
        errors
            .iter()
            .any(|error| error.to_string().contains("CSTRCPSR4")),
        "{errors:?}"
    );
    assert_eq!(engine.template_slot_names("record"), Some(vec!["original"]));
}

fn run(engine: &mut Engine, expected: usize) {
    assert_eq!(
        engine.run(RunLimit::Unlimited).unwrap().rules_fired,
        expected
    );
    assert!(
        engine.action_diagnostics().is_empty(),
        "{:?}",
        engine.action_diagnostics()
    );
    #[cfg(debug_assertions)]
    engine.debug_assert_consistency();
}

#[test]
fn unused_template_reload_reuses_identity_without_ambiguous_orphan_definitions() {
    let mut engine = Engine::with_rules("(deftemplate record (slot obsolete))").unwrap();
    for i in 0..100 {
        engine
            .load_str(&format!(
                "(deftemplate record (slot current (default {i})) (multislot tags))"
            ))
            .unwrap();
        assert_eq!(engine.templates(), vec!["record"]);
    }
    engine
        .load_str("(defrule read-record (record (current ?v)) => (assert (result ?v)))")
        .unwrap();
    engine.assert_template("record", &[], ()).unwrap();
    run(&mut engine, 1);
    let results = engine.find_facts("result").unwrap();
    let ferric_rules_core::Fact::Ordered(result) = results[0].1 else {
        panic!("ordered result expected")
    };
    assert!(matches!(result.fields.as_slice(), [Value::Integer(99)]));
}

#[test]
fn live_template_rejection_preserves_fact_identity_and_original_slot_behavior() {
    let mut engine = Engine::with_rules("(deftemplate record (slot original))").unwrap();
    let fact = engine
        .assert_template("record", &["original"], vec![Value::Integer(9)])
        .unwrap();
    rejected(&mut engine);
    assert!(engine.get_fact(fact).unwrap().is_some());
    engine
        .load_str(
            r#"(defrule read-record (record (original ?v)) => (printout t "original:" ?v crlf))"#,
        )
        .unwrap();
    run(&mut engine, 1);
    assert_eq!(engine.get_output("t"), Some("original:9\n"));
}

#[test]
fn rule_reference_blocks_reload_until_undefrule_reclaims_its_alpha_path() {
    let mut engine = Engine::with_rules(
        "(deftemplate record (slot original)) (defrule use-record (record (original ?x)) =>)",
    )
    .unwrap();
    rejected(&mut engine);
    engine
        .load_str("(defrule remove-record-rule => (undefrule use-record remove-record-rule))")
        .unwrap();
    run(&mut engine, 1);
    engine
        .load_str("(deftemplate record (slot changed))")
        .unwrap();
    engine
        .assert_template("record", &["changed"], vec![Value::Integer(7)])
        .unwrap();
}

#[test]
fn source_references_without_current_facts_block_template_redefinition() {
    let uses = [
        "(defrule create-record => (assert (record (original 2))))",
        "(defrule create-record => (if TRUE then (assert (record (original 2)))))",
        "(deffunction create-record () (assert (record (original 2))))",
        "(defmethod create-record () (assert (record (original 2))))",
        "(deffunction inspect-record () (find-all-facts ((?f record)) TRUE))",
        "(defrule query-record => (do-for-all-facts ((?f record)) TRUE (printout t ?f)))",
    ];
    for source in uses {
        let mut engine = Engine::with_rules("(deftemplate record (slot original))").unwrap();
        engine
            .load_str(source)
            .unwrap_or_else(|error| panic!("{source}: {error:?}"));
        rejected(&mut engine);
    }
}

#[test]
fn reset_seed_keeps_template_live_even_after_all_current_facts_are_retracted() {
    let mut engine = Engine::with_rules(
        "(deftemplate record (slot original)) (deffacts seed (record (original 8)))",
    )
    .unwrap();
    let facts: Vec<_> = engine.facts().unwrap().map(|(id, _)| id).collect();
    for fact in facts {
        engine.retract(fact).unwrap();
    }
    rejected(&mut engine);
    engine.reset().unwrap();
    engine
        .load_str("(defrule read-record (record (original ?v)) => (printout t ?v crlf))")
        .unwrap();
    run(&mut engine, 1);
    assert_eq!(engine.get_output("t"), Some("8\n"));
}

#[test]
fn pending_constructs_retain_the_original_template_during_incremental_load() {
    for consumer in [
        "(defrule read-record (record (original ?v)) => (printout t ?v crlf))",
        "(deffacts seed (record (original 3)))",
    ] {
        let mut engine = Engine::new(EngineConfig::default());
        let errors = engine.load_str(&format!("(deftemplate record (slot original)) {consumer} (deftemplate record (slot changed))")).unwrap_err();
        assert_eq!(errors.len(), 1, "{consumer}: {errors:?}");
        assert!(errors[0].to_string().contains("CSTRCPSR4"));
        assert_eq!(engine.template_slot_names("record"), Some(vec!["original"]));
        if engine.rules().is_empty() {
            engine
                .load_str("(defrule read-record (record (original ?v)) => (printout t ?v crlf))")
                .unwrap();
            engine.reset().unwrap();
        } else {
            engine
                .assert_template("record", &["original"], vec![Value::Integer(3)])
                .unwrap();
        }
        run(&mut engine, 1);
        assert_eq!(engine.get_output("t"), Some("3\n"));
    }
}

#[test]
fn reload_identity_is_module_and_local_name_with_stable_host_spelling() {
    let mut engine = Engine::with_rules(
        "(defmodule A) (deftemplate A::record (slot original))
        (deffacts seed-a (record (original 4)))
        (defmodule B) (deftemplate B::record (slot obsolete))",
    )
    .unwrap();
    engine
        .load_str("(defmodule B) (deftemplate record (slot current))")
        .unwrap();
    assert_eq!(
        engine.template_slot_names("B::record"),
        Some(vec!["current"])
    );
    let errors = engine
        .load_str("(deftemplate A::record (slot changed))")
        .unwrap_err();
    assert!(errors[0].to_string().contains("CSTRCPSR4"));
    engine
        .load_str("(defrule A::read-record (record (original ?v)) => (printout t ?v crlf))")
        .unwrap();
    engine.set_focus("A").unwrap();
    run(&mut engine, 1);
    assert_eq!(engine.get_output("t"), Some("4\n"));
}

#[cfg(feature = "serde")]
#[test]
fn restored_template_references_preserve_the_reload_contract() {
    use ferric_rules_runtime::SerializationFormat;
    let mut engine = Engine::with_rules("(deftemplate record (slot original))").unwrap();
    engine
        .load_str("(deftemplate record (slot original))")
        .unwrap();
    engine
        .load_str("(defrule create-record => (assert (record (original 5))))")
        .unwrap();
    for &format in SerializationFormat::ALL {
        let mut restored = Engine::deserialize(&engine.serialize(format).unwrap(), format).unwrap();
        rejected(&mut restored);
        run(&mut restored, 1);
        assert_eq!(restored.facts().unwrap().count(), 1);
    }
}
