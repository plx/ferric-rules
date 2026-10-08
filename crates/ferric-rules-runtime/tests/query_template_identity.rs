//! Literal and currently selected dynamic query targets retain their construct identities.

use ferric_rules_runtime::{Engine, EngineConfig, RunLimit, Value};

#[test]
fn compiled_and_pending_lhs_query_references_prevent_template_redefinition() {
    let declaration = "(deftemplate target (slot old))";
    let rule = "(defrule reader (go) (test (any-factp ((?f target)) TRUE)) =>)";
    let replacement = "(deftemplate target (slot changed))";
    for one_load in [false, true] {
        let mut engine = Engine::new(EngineConfig::utf8());
        let errors = if one_load {
            engine
                .load_str(&format!("{declaration}\n{rule}\n{replacement}"))
                .unwrap_err()
        } else {
            engine.load_str(&format!("{declaration}\n{rule}")).unwrap();
            engine.load_str(replacement).unwrap_err()
        };
        assert!(
            errors
                .iter()
                .any(|error| error.to_string().contains("in use")),
            "{errors:?}"
        );
        engine.load_str("(assert (target (old 7)))").unwrap();
    }
}

#[test]
fn query_target_in_a_field_constraint_prevents_redefinition() {
    let mut engine = Engine::new(EngineConfig::utf8());
    engine
        .load_str(
            r"
        (deftemplate target (slot old))
        (defrule reader (value ?x&:(if TRUE then (any-factp ((?f target)) TRUE) else FALSE)) =>)
    ",
        )
        .unwrap();
    assert!(engine
        .load_str("(deftemplate target (slot changed))")
        .is_err());
}

#[test]
fn earlier_dynamic_target_is_protected_while_later_restrictions_execute_and_then_released() {
    for explicit in [false, true] {
        let mut engine = Engine::new(EngineConfig::utf8());
        if explicit {
            engine.load_str("(deftemplate target)").unwrap();
        } else {
            let fact = engine.assert_ordered("target", Vec::<i64>::new()).unwrap();
            engine.retract(fact).unwrap();
        }
        engine
            .load_str(
                r#"
            (deftemplate other)
            (deffunction target-name () target)
            (defrule exercise =>
                (printout t (length$ (find-all-facts
                    ((?f (target-name) (if TRUE then
                        (printout t (build "(deftemplate target (slot changed))") "|")
                        other))) TRUE)) "|")
                (printout t (build "(deftemplate target (slot changed))") crlf))
        "#,
            )
            .unwrap();
        engine.reset().unwrap();
        assert_eq!(engine.run(RunLimit::Unlimited).unwrap().rules_fired, 1);
        assert_eq!(engine.get_output("t"), Some("FALSE|0|TRUE\n"));
        engine.load_str("(assert (target (changed 7)))").unwrap();
    }
}

#[test]
fn literal_ordered_query_reference_prevents_later_explicit_replacement() {
    for one_load in [false, true] {
        let mut engine = Engine::new(EngineConfig::utf8());
        let fact = engine.assert_ordered("target", Vec::<i64>::new()).unwrap();
        engine.retract(fact).unwrap();
        let reader = "(defrule reader (go) (test (any-factp ((?f target)) TRUE)) =>)";
        let replacement = "(deftemplate target (slot changed))";
        if one_load {
            assert!(engine
                .load_str(&format!("{reader}\n{replacement}"))
                .is_err());
        } else {
            engine.load_str(reader).unwrap();
            assert!(engine.load_str(replacement).is_err());
        }
    }
}

#[test]
fn dynamic_target_is_released_when_a_later_restriction_errors() {
    let mut engine = Engine::new(EngineConfig::utf8());
    engine
        .load_str(
            r"
        (deftemplate target)
        (deffunction target-name () target)
        (defrule exercise =>
            (find-all-facts ((?f (target-name) (if TRUE then 7 else target))) TRUE))
    ",
        )
        .unwrap();
    engine.reset().unwrap();
    assert_eq!(engine.run(RunLimit::Unlimited).unwrap().rules_fired, 1);
    assert!(!engine.action_diagnostics().is_empty());
    engine
        .load_str("(deftemplate target (slot changed))")
        .unwrap();
}

#[test]
fn qualified_template_defaults_resolve_literal_queries_in_the_owner_module() {
    let mut engine = Engine::new(EngineConfig::utf8());
    engine
        .load_str(
            r"
        (defmodule A)
        (deftemplate p (slot x))
        (defmodule MAIN)
        (deftemplate A::holder (slot n (default-dynamic (length$ (find-all-facts ((?f p)) TRUE)))))
    ",
        )
        .unwrap();
    engine
        .load_str("(defmodule A) (assert (p (x 7))) (assert (holder))")
        .unwrap();
    let count = engine
        .facts()
        .unwrap()
        .find_map(|(fact, _)| engine.get_fact_slot_by_name(fact, "n").ok())
        .unwrap();
    assert!(matches!(count, Value::Integer(1)));
}
