//! Loading a template must not reinterpret an existing ordered relation.
use ferric_rules_runtime::{Engine, EngineConfig, RunLimit, Value};

fn rejects_template(engine: &mut Engine, name: &str) {
    let errors = engine
        .load_str(&format!("(deftemplate {name} (slot x))"))
        .expect_err("a live ordered identity must remain ordered");
    assert!(
        errors
            .iter()
            .any(|error| error.to_string().contains("ordered relation is in use")),
        "{errors:?}"
    );
}

#[test]
fn later_template_cannot_reinterpret_an_installed_rhs_assertion() {
    let mut engine = Engine::new(EngineConfig::utf8());
    engine
        .load_str("(defrule make => (assert (item 1)))")
        .unwrap();
    rejects_template(&mut engine, "item");
    assert_eq!(engine.run(RunLimit::Unlimited).unwrap().rules_fired, 1);
    assert!(engine.action_diagnostics().is_empty());
    let facts = engine.find_facts("item").unwrap();
    assert_eq!(facts.len(), 1);
    assert!(
        matches!(facts[0].1, ferric_rules_core::Fact::Ordered(fact) if matches!(fact.fields.as_slice(), [Value::Integer(1)]))
    );
}

#[test]
fn current_facts_seeds_and_unmatched_ordered_patterns_keep_their_identity() {
    for source in [
        "(assert (item 1))",
        "(deffacts seed (item 1))",
        "(defrule match (item ?x) => (assert (matched ?x)))",
        "(defrule absent (not (item ?)) =>)",
    ] {
        let mut engine = Engine::new(EngineConfig::utf8());
        engine.load_str(source).unwrap();
        rejects_template(&mut engine, "item");
        engine
            .assert_ordered("item", vec![Value::Integer(2)])
            .unwrap();
        engine.run(RunLimit::Unlimited).unwrap();
        assert!(engine.action_diagnostics().is_empty(), "{source}");
        #[cfg(debug_assertions)]
        engine.debug_assert_consistency();
    }
}

#[test]
fn earlier_constructs_in_one_load_keep_ordered_classification() {
    for source in [
        "(defrule make => (assert (item 1)))",
        "(defrule match (item ?x) =>)",
        "(deffacts seed (item 1))",
        "(assert (item 1))",
    ] {
        let mut engine = Engine::new(EngineConfig::utf8());
        let errors = engine
            .load_str(&format!("{source} (deftemplate item (slot x))"))
            .unwrap_err();
        assert!(
            errors
                .iter()
                .any(|error| error.to_string().contains("ordered relation is in use")),
            "{source}: {errors:?}"
        );
        engine.run(RunLimit::Unlimited).unwrap();
        assert!(engine.action_diagnostics().is_empty(), "{source}");
        assert!(!engine.templates().contains(&"item"));
    }
}

#[test]
fn callable_and_nested_rhs_assertions_cannot_be_reinterpreted() {
    for source in [
        "(deffunction make () (assert (item 1)))",
        "(defgeneric make) (defmethod make () (assert (item 1)))",
        "(defrule make => (if TRUE then (assert (item 1))))",
    ] {
        let mut engine = Engine::new(EngineConfig::utf8());
        engine.load_str(source).unwrap();
        rejects_template(&mut engine, "item");
    }
}

#[test]
fn reserved_initial_identity_cannot_be_shadowed_even_before_first_load() {
    let mut engine = Engine::new(EngineConfig::utf8());
    rejects_template(&mut engine, "initial-fact");
    engine
        .load_str("(defrule boot (initial-fact) => (assert (started)))")
        .unwrap();
    assert_eq!(engine.run(RunLimit::Unlimited).unwrap().rules_fired, 1);
    assert_eq!(engine.find_facts("started").unwrap().len(), 1);
}

#[test]
fn unrelated_and_already_explicit_template_identities_still_work() {
    let mut engine = Engine::new(EngineConfig::utf8());
    engine
        .load_str("(defrule ordered => (assert (item 1))) (deftemplate other (slot x))")
        .unwrap();
    engine
        .load_str(
            "(defmodule A) (deftemplate item (slot x)) (defrule make => (assert (item (x 2))))",
        )
        .unwrap_err();
    // A live global ordered identity is protected across modules as well.
    let mut engine = Engine::new(EngineConfig::utf8());
    engine.load_str("(defmodule A) (deftemplate item (slot x)) (defrule make => (assert (item (x 2)))) (defmodule B) (deftemplate B::item (slot y))").unwrap();
    assert!(engine.action_diagnostics().is_empty());
}

#[test]
fn qualified_ordered_references_cannot_be_reinterpreted() {
    for source in [
        "(defrule make => (assert (MAIN::item 1)))",
        "(defrule match (MAIN::item ?x) =>)",
        "(deffacts seed (MAIN::item 1))",
        "(assert (MAIN::item 1))",
    ] {
        let mut engine = Engine::new(EngineConfig::utf8());
        // Some qualified ordered forms are explicitly unsupported at runtime,
        // but a later template must not silently change their classification.
        engine.load_str(source).unwrap();
        rejects_template(&mut engine, "MAIN::item");
        let mut engine = Engine::new(EngineConfig::utf8());
        let errors = engine
            .load_str(&format!("{source} (deftemplate MAIN::item (slot x))"))
            .unwrap_err();
        assert!(
            errors
                .iter()
                .any(|error| error.to_string().contains("ordered relation is in use")),
            "{source}: {errors:?}"
        );
    }
    let mut engine = Engine::new(EngineConfig::utf8());
    engine
        .assert_ordered("MAIN::item", vec![Value::Integer(1)])
        .unwrap();
    rejects_template(&mut engine, "MAIN::item");
}
