//! Dormant defaults and constrained patterns retain valid template identities.

use ferric_rules_runtime::{Engine, RunLimit, Value};

fn rejected_redefinition(engine: &mut Engine, declaration: &str) {
    let errors = engine.load_str(declaration).unwrap_err();
    assert!(
        errors
            .iter()
            .any(|error| error.to_string().contains("CSTRCPSR4")),
        "{errors:?}"
    );
}

#[test]
fn dormant_dynamic_defaults_protect_queries_and_raw_assertions() {
    for expression in [
        "(any-factp ((?f source)) TRUE)",
        "(if FALSE then (any-factp ((?f source)) TRUE) else FALSE)",
        "(if FALSE then (assert (source (original 7))) else FALSE)",
        "(if FALSE then (assert (box (value (any-factp ((?f source)) TRUE)))) else FALSE)",
    ] {
        let mut engine = Engine::with_rules(&format!(
            "(deftemplate source (slot original))
             (deftemplate box (slot value))
             (deftemplate consumer (slot value (default-dynamic {expression})))"
        ))
        .unwrap_or_else(|errors| panic!("{expression}: {errors:?}"));
        rejected_redefinition(&mut engine, "(deftemplate source (slot replacement))");
        assert_eq!(engine.template_slot_names("source"), Some(vec!["original"]));
        // Removing the sole dormant reference makes an unused template replaceable again.
        engine
            .load_str("(deftemplate consumer (slot value))")
            .unwrap();
        engine
            .load_str("(deftemplate source (slot replacement))")
            .unwrap();
    }
}

#[test]
fn rejected_query_dependency_replacement_preserves_execution_and_module_identity() {
    let mut engine = Engine::with_rules(
        "(defmodule A)
         (deftemplate source (slot original))
         (deftemplate consumer
           (slot value (default-dynamic (do-for-fact ((?f source)) TRUE ?f:original))))
         (defmodule B)
         (deftemplate B::source (slot unrelated))",
    )
    .unwrap();
    // A default resolves its query declarations in its defining module.
    engine
        .load_str("(deftemplate B::source (slot replacement))")
        .unwrap();
    rejected_redefinition(&mut engine, "(deftemplate A::source (slot replacement))");
    engine
        .load_str("(defmodule A) (assert (source (original 17))) (assert (consumer))")
        .unwrap();
    let consumer = engine
        .facts()
        .unwrap()
        .find_map(|(handle, fact)| match fact {
            ferric_rules_core::Fact::Template(fact)
                if engine.template_name_by_id(fact.template_id) == Some("consumer") =>
            {
                Some(handle)
            }
            _ => None,
        })
        .unwrap();
    assert!(matches!(
        engine.get_fact_slot_by_name(consumer, "value").unwrap(),
        Value::Integer(17)
    ));
    assert!(engine.action_diagnostics().is_empty());
}

#[test]
fn dormant_dynamic_ordered_assertions_cannot_be_reinterpreted() {
    let mut engine = Engine::with_rules(
        "(deftemplate consumer (slot value (default-dynamic
            (if TRUE then (assert (item 7)) else FALSE))))",
    )
    .unwrap();
    let errors = engine
        .load_str("(deftemplate item (slot replacement))")
        .unwrap_err();
    assert!(errors
        .iter()
        .any(|error| error.to_string().contains("ordered relation is in use")));
    engine.load_str("(assert (consumer))").unwrap();
    let facts = engine.find_facts("item").unwrap();
    assert_eq!(facts.len(), 1);
    assert!(
        matches!(facts[0].1, ferric_rules_core::Fact::Ordered(fact) if matches!(fact.fields.as_slice(), [Value::Integer(7)]))
    );
}

#[test]
fn dynamic_default_fact_and_slot_heads_do_not_create_phantom_dependencies() {
    for declaration in ["", "(deftemplate target (slot original))"] {
        let mut engine = Engine::with_rules(&format!(
            "(deffunction target () 3)
             {declaration}
             (deftemplate box (slot assert))
             (deftemplate consumer (slot value (default-dynamic
               (if FALSE then (assert (box (assert (target)))) else FALSE))))",
        ))
        .unwrap();
        engine
            .load_str("(deftemplate target (slot replacement))")
            .unwrap();
    }
}

#[test]
fn all_literal_pattern_branches_are_checked_before_replacing_a_rule() {
    for field in [
        "blue",
        "~blue",
        "red|blue",
        "?x&blue",
        "?x&~blue",
        "red|red&~blue",
    ] {
        let mut engine = Engine::with_rules(
            "(deftemplate item (slot color (allowed-symbols red)))
             (deffacts seed (item (color red)))
             (defrule keep (item (color red)) => (printout t kept crlf))",
        )
        .unwrap();
        let errors = engine
            .load_str(&format!("(defrule keep (item (color {field})) =>)"))
            .unwrap_err();
        assert!(
            errors
                .iter()
                .any(|error| error.to_string().contains("CSTRNCHK1")),
            "{field}: {errors:?}"
        );
        assert_eq!(engine.run(RunLimit::Unlimited).unwrap().rules_fired, 1);
        assert_eq!(engine.get_output("t"), Some("kept\n"));
    }
}

#[test]
fn literal_pattern_types_ranges_and_multislot_members_are_checked() {
    for (slot, pattern) in [
        ("(slot value (type INTEGER))", "(value wrong)"),
        ("(slot value (type NUMBER) (range 1 3))", "(value ~4)"),
        ("(slot value (allowed-values 1))", "(value 1.0)"),
        (
            "(multislot value (allowed-symbols red))",
            "(value $?before ~blue $?after)",
        ),
    ] {
        let mut engine = Engine::with_rules(&format!("(deftemplate item {slot})")).unwrap();
        let errors = engine
            .load_str(&format!("(defrule invalid (item {pattern}) =>)"))
            .unwrap_err();
        assert!(
            errors
                .iter()
                .any(|error| error.to_string().contains("CSTRNCHK1")),
            "{slot}: {errors:?}"
        );
        assert!(engine.rules().is_empty());
    }
}

#[test]
fn fixed_multislot_cardinality_counts_variable_and_connected_fields() {
    let mut engine =
        Engine::with_rules("(deftemplate item (multislot values (cardinality 2 3)))").unwrap();
    for fields in ["", "?x", "red|blue", "?a ?b ?c ?d"] {
        let errors = engine
            .load_str(&format!("(defrule invalid (item (values {fields})) =>)"))
            .unwrap_err();
        assert!(
            errors
                .iter()
                .any(|error| error.to_string().contains("cardinality restrictions")),
            "{fields}: {errors:?}"
        );
    }
    for (index, fields) in [
        "?a ?b",
        "red|blue ?a ?b",
        "$?all",
        "?a $?tail",
        "$?head ?a $?tail",
    ]
    .iter()
    .enumerate()
    {
        engine
            .load_str(&format!(
                "(defrule valid-{index} (item (values {fields})) =>)"
            ))
            .unwrap();
    }
    assert_eq!(engine.rules().len(), 5);
}

#[cfg(feature = "serde")]
#[test]
fn restored_dormant_defaults_keep_template_and_ordered_dependencies() {
    use ferric_rules_runtime::SerializationFormat;

    let engine = Engine::with_rules(
        "(deftemplate source (slot original))
         (deftemplate consumer
           (slot value (default-dynamic (if FALSE then
             (assert (item (any-factp ((?f source)) TRUE))) else FALSE))))",
    )
    .unwrap();
    for &format in SerializationFormat::ALL {
        let mut restored = Engine::deserialize(&engine.serialize(format).unwrap(), format).unwrap();
        rejected_redefinition(&mut restored, "(deftemplate source (slot replacement))");
        assert!(restored
            .load_str("(deftemplate item (slot replacement))")
            .is_err());
    }
}
