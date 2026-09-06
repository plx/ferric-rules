//! Cross-module template identity must preserve the public name index.

use ferric_rules_runtime::{Engine, RunLimit, Value};

#[test]
fn conflicting_unqualified_template_is_rejected_without_changing_existing_state() {
    let mut engine = Engine::with_rules(
        "(defmodule A) (deftemplate item (slot left (type INTEGER)))
         (deffacts first (item (left 7)))
         (defrule observe (item (left ?value)) => (printout t ?value crlf))",
    )
    .unwrap();
    let original = engine.facts().unwrap().next().unwrap().0;
    let errors = engine
        .load_str("(defmodule B) (deftemplate item (slot right (type STRING)))")
        .unwrap_err();
    assert!(errors
        .iter()
        .any(|error| error.to_string().contains("module-qualified")));
    assert_eq!(engine.templates(), vec!["item"]);
    assert_eq!(engine.template_slot_names("item"), Some(vec!["left"]));
    assert!(matches!(
        engine.get_fact_slot_by_name(original, "left").unwrap(),
        Value::Integer(7)
    ));
    assert_eq!(engine.facts().unwrap().count(), 1);

    // The preceding defmodule is committed by incremental load. Its distinct
    // explicit template spelling is supported and cannot overwrite A's index.
    engine
        .load_str("(deftemplate B::item (slot right (type STRING))) (deffacts second (item (right \"B\")))")
        .unwrap();
    let mut templates = engine.templates();
    templates.sort_unstable();
    assert_eq!(templates, vec!["B::item", "item"]);
    assert_eq!(engine.template_slot_names("item"), Some(vec!["left"]));
    assert_eq!(engine.template_slot_names("B::item"), Some(vec!["right"]));
    engine.reset().unwrap();
    assert_eq!(engine.facts().unwrap().count(), 2);
    engine.set_focus("A").unwrap();
    assert_eq!(engine.run(RunLimit::Unlimited).unwrap().rules_fired, 1);
    assert_eq!(engine.get_output("t"), Some("7\n"));
}

#[cfg(feature = "serde")]
#[test]
fn rejected_spelling_and_distinct_qualified_templates_remain_persistable() {
    use ferric_rules_runtime::SerializationFormat;

    let mut engine = Engine::with_rules(
        "(defmodule A) (deftemplate item (slot left (type INTEGER)))
         (deffacts first (item (left 7)))
         (defrule observe (item (left ?value)) => (printout t ?value crlf))",
    )
    .unwrap();
    assert!(engine
        .load_str("(defmodule B) (deftemplate item (slot right))")
        .is_err());
    let after_rejection = engine.serialize(SerializationFormat::Cbor).unwrap();
    let restored = Engine::deserialize(&after_rejection, SerializationFormat::Cbor).unwrap();
    assert_eq!(restored.template_slot_names("item"), Some(vec!["left"]));
    assert_eq!(restored.facts().unwrap().count(), 1);

    engine
        .load_str("(deftemplate B::item (slot right (type STRING))) (deffacts second (item (right \"B\")))")
        .unwrap();
    engine.reset().unwrap();
    engine.set_focus("A").unwrap();
    let snapshot = engine.serialize(SerializationFormat::Cbor).unwrap();
    let mut restored = Engine::deserialize(&snapshot, SerializationFormat::Cbor).unwrap();
    assert_eq!(restored.templates().len(), 2);
    assert_eq!(restored.template_slot_names("item"), Some(vec!["left"]));
    assert_eq!(restored.template_slot_names("B::item"), Some(vec!["right"]));
    assert_eq!(restored.facts().unwrap().count(), 2);
    assert_eq!(restored.run(RunLimit::Unlimited).unwrap().rules_fired, 1);
    assert_eq!(restored.get_output("t"), Some("7\n"));
}
