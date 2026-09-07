//! Declared-template identity and RHS named-slot assertion regressions.

use ferric_rules_core::{Fact, TemplateFact};
use ferric_rules_runtime::FactHandle as FactId;
use ferric_rules_runtime::{Engine, HaltReason, RunLimit, Value};

fn template_facts<'a>(engine: &'a Engine, name: &str) -> Vec<(FactId, &'a TemplateFact)> {
    engine
        .facts()
        .unwrap()
        .filter_map(|(id, fact)| match fact {
            Fact::Template(fact) if engine.template_name_by_id(fact.template_id) == Some(name) => {
                Some((id, fact))
            }
            _ => None,
        })
        .collect()
}

#[test]
fn rhs_template_assert_evaluates_named_slots_fills_defaults_and_propagates() {
    let mut engine = Engine::with_rules(include_str!("fixtures/template_rhs_assert.clp")).unwrap();
    let run = engine.run(RunLimit::Unlimited).unwrap();
    assert_eq!(run.rules_fired, 2);
    assert_eq!(run.halt_reason, HaltReason::AgendaEmpty);
    assert!(engine.action_diagnostics().is_empty());
    assert_eq!(engine.get_output("t"), Some("5:ready\n"));
    assert_eq!(engine.find_facts("result").unwrap().len(), 0);
    let facts = template_facts(&engine, "result");
    assert_eq!(facts.len(), 1);
    assert!(matches!(facts[0].1.slots[0], Value::Integer(5)));
    let Value::Symbol(status) = facts[0].1.slots[1] else {
        panic!("default must be the ready symbol");
    };
    assert_eq!(engine.resolve_core_symbol(status), Some("ready"));
}

#[test]
fn template_multislot_evaluates_every_expression_and_splices_multifields() {
    let mut engine = Engine::with_rules(
        r"
        (deftemplate item (slot one) (multislot many))
        (defrule create
            =>
            (assert (item (one (+ 1 2)) (many alpha (create$ beta gamma) (+ 2 3)))))
        ",
    )
    .unwrap();
    assert_eq!(engine.run(RunLimit::Unlimited).unwrap().rules_fired, 1);
    assert!(engine.action_diagnostics().is_empty());
    let facts = template_facts(&engine, "item");
    assert_eq!(facts.len(), 1);
    assert!(matches!(facts[0].1.slots[0], Value::Integer(3)));
    let Value::Multifield(values) = &facts[0].1.slots[1] else {
        panic!("multislot must remain a multifield");
    };
    assert_eq!(values.len(), 4);
    for (value, expected) in values.iter().take(3).zip(["alpha", "beta", "gamma"]) {
        let Value::Symbol(symbol) = value else {
            panic!("expected symbol");
        };
        assert_eq!(engine.resolve_core_symbol(*symbol), Some(expected));
    }
    assert!(matches!(values[3], Value::Integer(5)));
}

#[test]
fn invalid_named_slot_syntax_rejects_the_rule_without_installing_it() {
    for (slots, diagnostic) in [
        ("(key 1) (key 2)", "duplicate slot"),
        ("(missing 1)", "unknown slot"),
        ("1", "expected named slot list"),
        ("(key 1 2)", "requires exactly one value"),
        ("(key)", "requires exactly one value"),
    ] {
        let mut engine = Engine::with_rules("(deftemplate result (slot key))").unwrap();
        let errors = engine
            .load_str(&format!("(defrule invalid => (assert (result {slots})))"))
            .unwrap_err();
        assert!(
            errors
                .iter()
                .any(|error| error.to_string().contains(diagnostic)),
            "{errors:?}"
        );
        assert!(engine.rules().is_empty());
        assert!(template_facts(&engine, "result").is_empty());
        assert_eq!(engine.run(RunLimit::Unlimited).unwrap().rules_fired, 0);
    }
}

#[test]
fn runtime_slot_value_failure_does_not_assert_a_partial_fact_or_continue_rhs() {
    let mut engine = Engine::with_rules(
        r"
        (deftemplate result (slot key) (slot status (default ready)))
        (deffunction fields () (create$ 1 2))
        (defrule invalid
            =>
            (assert (result (status changed) (key (fields))))
            (assert (after)))
        ",
    )
    .unwrap();
    let run = engine.run(RunLimit::Unlimited).unwrap();
    assert_eq!(run.rules_fired, 1);
    assert_eq!(run.halt_reason, HaltReason::ActionError);
    assert!(engine.action_diagnostics()[0]
        .to_string()
        .contains("requires one scalar value"));
    assert!(template_facts(&engine, "result").is_empty());
    assert!(engine.find_facts("result").unwrap().is_empty());
    assert!(engine.find_facts("after").unwrap().is_empty());
}

#[test]
fn modify_and_duplicate_preserve_complete_multislot_values() {
    let mut engine = Engine::with_rules(
        r"
        (deftemplate item (slot one) (multislot many))
        (defrule seed => (assert (item (one 1) (many a b))))
        (defrule update
            ?fact <- (item (one 1))
            =>
            (modify ?fact (one 2) (many b c (create$ d e))))
        (defrule copy
            ?fact <- (item (one 2))
            =>
            (duplicate ?fact (one 3) (many)))
        ",
    )
    .unwrap();
    let run = engine.run(RunLimit::Unlimited).unwrap();
    assert_eq!(run.rules_fired, 3);
    assert_eq!(run.halt_reason, HaltReason::AgendaEmpty);
    assert!(engine.action_diagnostics().is_empty());
    let facts = template_facts(&engine, "item");
    assert_eq!(facts.len(), 2);
    for (_, fact) in facts {
        let Value::Multifield(values) = &fact.slots[1] else {
            panic!("multislot must remain a multifield");
        };
        match fact.slots[0] {
            Value::Integer(2) => {
                let names: Vec<_> = values
                    .iter()
                    .map(|value| match value {
                        Value::Symbol(symbol) => engine.resolve_core_symbol(*symbol).unwrap(),
                        _ => panic!("expected a symbol"),
                    })
                    .collect();
                assert_eq!(names, ["b", "c", "d", "e"]);
            }
            Value::Integer(3) => assert!(values.is_empty()),
            _ => panic!("modify must replace the original fact"),
        }
    }
}

#[test]
fn imported_template_rhs_assert_uses_the_rule_module() {
    let mut engine = Engine::with_rules(
        r"
        (defmodule DATA (export deftemplate result))
        (deftemplate DATA::result (slot key (default 7)))
        (defmodule APP (import DATA deftemplate result))
        (defrule APP::create => (assert (result)))
        (defrule APP::observe (result (key ?key)) => (printout t ?key crlf))
        ",
    )
    .unwrap();
    engine.push_focus("APP").unwrap();
    let run = engine.run(RunLimit::Unlimited).unwrap();
    assert_eq!(run.rules_fired, 2);
    assert!(engine.action_diagnostics().is_empty());
    assert_eq!(engine.get_output("t"), Some("7\n"));
    assert_eq!(template_facts(&engine, "DATA::result").len(), 1);
}

#[test]
fn empty_template_pattern_matches_and_retraction_unblocks_negation() {
    let mut engine = Engine::with_rules(
        r"
        (deftemplate blocker (slot seq))
        (deffacts startup (blocker (seq 1)) (blocker (seq 2)) (blocker (seq 3)))
        (defrule remove (declare (salience 10)) ?b <- (blocker) => (retract ?b))
        (defrule absent (not (blocker)) => (assert (clear)))
        ",
    )
    .unwrap();
    let run = engine.run(RunLimit::Unlimited).unwrap();
    assert_eq!(run.rules_fired, 4);
    assert!(engine.action_diagnostics().is_empty());
    assert!(template_facts(&engine, "blocker").is_empty());
    assert_eq!(engine.find_facts("clear").unwrap().len(), 1);
    engine
        .assert_template("blocker", &["seq"], vec![Value::Integer(4)])
        .unwrap();
    assert_eq!(engine.run(RunLimit::Unlimited).unwrap().rules_fired, 2);
}

#[test]
fn empty_template_fact_body_and_exists_share_template_identity() {
    let mut engine = Engine::with_rules(
        r"
        (deftemplate marker)
        (deffacts startup (marker))
        (defrule present (exists (marker)) => (assert (observed)))
        ",
    )
    .unwrap();
    assert_eq!(template_facts(&engine, "marker").len(), 1);
    assert!(engine.find_facts("marker").unwrap().is_empty());
    assert_eq!(engine.run(RunLimit::Unlimited).unwrap().rules_fired, 1);
    assert_eq!(engine.find_facts("observed").unwrap().len(), 1);
}

#[test]
fn declared_template_rejects_positional_lhs_fields() {
    let mut engine = Engine::with_rules("(deftemplate item (slot one))").unwrap();
    let errors = engine
        .load_str("(defrule invalid (item ?one) =>)")
        .unwrap_err();
    assert!(errors.iter().any(|error| error
        .to_string()
        .contains("requires named slot constraints")));
    assert!(engine.rules().is_empty());
}

#[test]
fn omitted_unconstrained_slots_derive_nil_and_empty_multifield() {
    let mut engine = Engine::with_rules(
        "(deftemplate item (slot one) (multislot many)) (defrule create => (assert (item)))",
    )
    .unwrap();
    assert_eq!(engine.run(RunLimit::Unlimited).unwrap().rules_fired, 1);
    let facts = template_facts(&engine, "item");
    let Value::Symbol(symbol) = facts[0].1.slots[0] else {
        panic!("unconstrained single slot defaults to nil");
    };
    assert_eq!(engine.resolve_core_symbol(symbol), Some("nil"));
    assert!(matches!(&facts[0].1.slots[1], Value::Multifield(values) if values.is_empty()));
}

#[test]
fn required_default_prevents_incomplete_rhs_assertion() {
    let mut engine = Engine::with_rules("(deftemplate item (slot key (default ?NONE)))").unwrap();
    let errors = engine
        .load_str("(defrule create => (assert (item)))")
        .unwrap_err();
    assert!(errors
        .iter()
        .any(|error| error.to_string().contains("requires a value")));
    assert!(engine.rules().is_empty());
    assert!(template_facts(&engine, "item").is_empty());
}

#[cfg(feature = "serde")]
#[test]
fn restored_template_assertions_preserve_slot_cardinality() {
    use ferric_rules_runtime::SerializationFormat;
    let engine = Engine::with_rules(
        "(deftemplate item (multislot values)) (defrule create => (assert (item (values a b c))))",
    )
    .unwrap();
    for &format in SerializationFormat::ALL {
        let bytes = engine.serialize(format).unwrap();
        let mut restored = Engine::deserialize(&bytes, format).unwrap();
        assert_eq!(restored.run(RunLimit::Unlimited).unwrap().rules_fired, 1);
        let facts = template_facts(&restored, "item");
        assert!(matches!(&facts[0].1.slots[0], Value::Multifield(values) if values.len() == 3));
    }
}

#[test]
fn multislot_omits_void_expression_results_and_preserves_effects() {
    let mut engine = Engine::with_rules(
        r#"
        (deftemplate item (multislot many))
        (deffunction nothing () (printout t "side-effect" crlf))
        (defrule create
            =>
            (assert (item (many before (nothing) after)))
            (printout t "after-assert" crlf))
        "#,
    )
    .unwrap();
    let run = engine.run(RunLimit::Unlimited).unwrap();
    assert_eq!(run.rules_fired, 1);
    assert_eq!(run.halt_reason, HaltReason::AgendaEmpty);
    assert!(engine.action_diagnostics().is_empty());
    assert_eq!(engine.get_output("t"), Some("side-effect\nafter-assert\n"));
    let facts = template_facts(&engine, "item");
    assert_eq!(facts.len(), 1);
    let Value::Multifield(values) = &facts[0].1.slots[0] else {
        panic!("expected a multislot");
    };
    let fields: Vec<_> = values
        .iter()
        .map(|value| match value {
            Value::Symbol(symbol) => engine.resolve_core_symbol(*symbol).unwrap(),
            _ => panic!("only the two surrounding symbols should be retained"),
        })
        .collect();
    assert_eq!(fields, ["before", "after"]);
}

#[test]
fn a_template_multislot_binding_has_both_rhs_variable_spellings() {
    let mut engine = Engine::with_rules(
        r#"
        (deftemplate packet (slot id) (multislot items))
        (deffacts seed (source 12 red "blue" 3.5))
        (defrule create-packet (source ?id $?items) => (assert (packet (id ?id) (items $?items))))
        (deffunction count-fields ($?values) (length$ $?values))
        (defrule read-packet (packet (id ?id) (items $?items)) =>
            (printout t ?id " " (length$ ?items) crlf)
            (assert (result ?id $?items))
            (assert (copied (count-fields red "blue" 3.5))))
        "#,
    )
    .unwrap();
    let run = engine.run(RunLimit::Unlimited).unwrap();
    assert_eq!(run.rules_fired, 2);
    assert_eq!(
        run.halt_reason,
        HaltReason::AgendaEmpty,
        "{:?}",
        engine.action_diagnostics()
    );
    assert_eq!(engine.get_output("t"), Some("12 3\n"));
    let results = engine.find_facts("result").unwrap();
    assert_eq!(results.len(), 1);
    let Fact::Ordered(result) = results[0].1 else {
        panic!("ordered fact required")
    };
    assert_eq!(result.fields.len(), 4);
    assert!(
        matches!(result.fields[3], Value::Float(value) if value.to_bits() == 3.5_f64.to_bits())
    );
    let copies = engine.find_facts("copied").unwrap();
    assert_eq!(copies.len(), 1);
    let Fact::Ordered(copy) = copies[0].1 else {
        panic!("ordered fact required")
    };
    assert!(matches!(copy.fields.as_slice(), [Value::Integer(3)]));
}
