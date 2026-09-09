//! Primitive template constraints must be retained across every assertion path.
use ferric_rules_runtime::{Engine, EngineConfig, EngineError, HaltReason, RunLimit, Value};

#[test]
fn primitive_and_union_defaults_match_clips_priority() {
    let mut engine = Engine::with_rules(
        r#"(deftemplate defaults
            (slot integer (type INTEGER)) (slot float (type FLOAT))
            (slot number (type NUMBER)) (slot symbol (type SYMBOL))
            (slot string (type STRING)) (slot lexeme (type LEXEME))
            (slot float-int (type FLOAT INTEGER))
            (slot int-string (type INTEGER STRING))
            (slot string-symbol (type STRING SYMBOL))
            (multislot items (type INTEGER)))
            (defrule report (defaults (integer ?i) (float ?f) (number ?n)
              (symbol ?s) (string ?t) (lexeme ?l) (float-int ?fi)
              (int-string ?is) (string-symbol ?ss) (items $?items))
              => (printout t ?i "|" ?f "|" ?n "|" ?s "|" (str-length ?t) "|"
                ?l "|" ?fi "|" (str-length ?is) "|" ?ss "|" (length$ ?items) crlf))"#,
    )
    .unwrap();
    engine.assert_template("defaults", &[], ()).unwrap();
    assert_eq!(engine.run(RunLimit::Unlimited).unwrap().rules_fired, 1);
    assert_eq!(
        engine.get_output("t").unwrap(),
        Some("0|0.0|0|nil|0|nil|0|0|nil|0\n")
    );
}

#[test]
fn invalid_literal_rule_replacement_preserves_the_previous_consumer() {
    let mut engine = Engine::with_rules(
        "(deftemplate counter (slot n (type INTEGER)))
         (deffacts seed (counter (n 5)))
         (defrule valid (counter (n ?n)) => (printout t \"valid \" ?n crlf))",
    )
    .unwrap();
    let errors = engine
        .load_str("(defrule valid => (assert (counter (n \"wrong\"))))")
        .unwrap_err();
    assert!(errors
        .iter()
        .any(|error| error.to_string().contains("allowed types")));
    assert_eq!(engine.run(RunLimit::Unlimited).unwrap().rules_fired, 1);
    assert_eq!(engine.get_output("t").unwrap(), Some("valid 5\n"));
}

#[test]
fn every_literal_multislot_field_is_checked_before_rule_installation() {
    for value in [
        "1 wrong",
        "(create$ 1 wrong)",
        "(create$ 1 (create$ wrong))",
    ] {
        let mut engine =
            Engine::with_rules("(deftemplate item (multislot n (type NUMBER)))").unwrap();
        let errors = engine
            .load_str(&format!("(defrule invalid => (assert (item (n {value}))))"))
            .unwrap_err();
        assert!(errors
            .iter()
            .any(|error| error.to_string().contains("allowed types")));
        assert!(engine.rules().is_empty());
    }
}

#[test]
fn named_seed_replacement_and_defaults_validate_every_field_atomically() {
    let mut engine = Engine::with_rules(
        "(deftemplate item (multislot n (type NUMBER) (default 1 2.5 3)))
         (deffacts seed (item (n 7 8)))",
    )
    .unwrap();
    assert!(engine
        .load_str("(deffacts seed (item (n 9)) (item (n 10 wrong)))")
        .is_err());
    engine.reset().unwrap();
    let fact = engine.facts().unwrap().next().unwrap().0;
    let Value::Multifield(values) = engine.get_fact_slot_by_name(fact, "n").unwrap() else {
        panic!()
    };
    assert_eq!(values.len(), 2);
    assert!(matches!(values[0], Value::Integer(7)));
    assert!(matches!(values[1], Value::Integer(8)));
    let defaults = engine.assert_template("item", &[], ()).unwrap();
    let Value::Multifield(values) = engine.get_fact_slot_by_name(defaults, "n").unwrap() else {
        panic!()
    };
    assert_eq!(values.len(), 3);
    assert!(matches!(values[1], Value::Float(2.5)));
}

#[test]
fn invalid_defaults_cannot_replace_an_unused_template() {
    let mut engine = Engine::with_rules("(deftemplate item (slot n (type INTEGER)))").unwrap();
    assert!(engine
        .load_str("(deftemplate item (slot n (type INTEGER) (default wrong)))")
        .is_err());
    let fact = engine.assert_template("item", &[], ()).unwrap();
    assert!(matches!(
        engine.get_fact_slot_by_name(fact, "n").unwrap(),
        Value::Integer(0)
    ));
    for source in [
        "(deftemplate x (multislot n (type NUMBER) (default 1 wrong)))",
        "(deftemplate x (slot n (default (create$ 1))))",
    ] {
        assert!(engine.load_str(source).is_err(), "{source}");
    }
}

#[test]
fn runtime_types_reject_assert_modify_and_duplicate_before_mutation() {
    for action in [
        "(assert (item (n ?bad)))",
        "(modify ?fact (n ?bad))",
        "(duplicate ?fact (n ?bad))",
    ] {
        let mut engine = Engine::with_rules(&format!(
            "(deftemplate item (slot n (type INTEGER)))
             (deffacts seed (item (n 5)) (bad \"wrong\"))
             (defrule invalid ?fact <- (item (n 5)) (bad ?bad) => {action} (assert (after)))"
        ))
        .unwrap();
        let run = engine.run(RunLimit::Unlimited).unwrap();
        assert_eq!(run.halt_reason, HaltReason::ActionError, "{action}");
        assert!(engine.action_diagnostics()[0]
            .to_string()
            .contains("allowed types"));
        assert!(engine.find_facts("after").unwrap().is_empty());
        let items: Vec<_> = engine
            .facts()
            .unwrap()
            .filter(|(_, fact)| matches!(fact, ferric_rules_core::Fact::Template(_)))
            .collect();
        assert_eq!(items.len(), 1, "{action}");
        assert!(matches!(
            engine.get_fact_slot_by_name(items[0].0, "n").unwrap(),
            Value::Integer(5)
        ));
    }
}

#[test]
fn host_template_values_obey_declared_scalar_and_multislot_types() {
    let mut engine = Engine::with_rules(
        "(deftemplate item (slot n (type INTEGER)) (multislot list (type NUMBER)))",
    )
    .unwrap();
    assert!(matches!(
        engine.assert_template("item", &["n"], vec![Value::Float(1.5)]),
        Err(EngineError::InvalidSlotValue { .. })
    ));
    let wrong = Value::String(
        ferric_rules_core::FerricString::new("wrong", ferric_rules_core::StringEncoding::Utf8)
            .unwrap(),
    );
    assert!(engine
        .assert_template(
            "item",
            &["list"],
            vec![Value::Multifield(Box::new(
                [Value::Integer(1), wrong].into_iter().collect()
            ))]
        )
        .is_err());
    assert_eq!(engine.facts().unwrap().count(), 0);
    let fact = engine
        .assert_template("item", &["list"], vec![Value::Float(1.5)])
        .unwrap();
    assert!(matches!(
        engine.get_fact_slot_by_name(fact, "n").unwrap(),
        Value::Integer(0)
    ));
}

#[test]
fn instance_name_slots_accept_typed_defaults_and_values_but_reject_symbols() {
    let mut engine =
        Engine::with_rules("(deftemplate named (slot value (type INSTANCE-NAME)))").unwrap();
    let default = engine.assert_template("named", &[], ()).unwrap();
    let Value::InstanceName(name) = engine.get_fact_slot_by_name(default, "value").unwrap() else {
        panic!("INSTANCE-NAME defaults must retain their value type")
    };
    assert_eq!(
        engine.resolve_core_symbol_bytes(name.as_symbol()),
        Some(b"nil".as_slice())
    );

    let name = engine.instance_name_value("widget").unwrap();
    let explicit = engine
        .assert_template("named", &["value"], vec![name])
        .unwrap();
    let Value::InstanceName(name) = engine.get_fact_slot_by_name(explicit, "value").unwrap() else {
        panic!("a typed name must remain an INSTANCE-NAME")
    };
    assert_eq!(
        engine.resolve_core_symbol_bytes(name.as_symbol()),
        Some(b"widget".as_slice())
    );

    let symbol = engine.symbol_value("widget").unwrap();
    assert!(matches!(
        engine.assert_template("named", &["value"], vec![symbol]),
        Err(EngineError::InvalidSlotValue { .. })
    ));
    assert_eq!(engine.facts().unwrap().count(), 2);
}

#[test]
fn ignored_or_unrepresentable_constraint_attributes_are_explicit_errors() {
    for attribute in [
        "(range 1 10)",
        "(allowed-values 1 2)",
        "(cardinality 1 2)",
        "(default-dynamic (+ 1 2))",
        "(default (+ 1 2))",
        "(type FACT-ADDRESS)",
        "(type)",
        "(type INTEGER) (type FLOAT)",
    ] {
        let mut engine = Engine::new(EngineConfig::default());
        assert!(
            engine
                .load_str(&format!("(deftemplate item (slot n {attribute}))"))
                .is_err(),
            "{attribute}"
        );
        assert!(matches!(
            engine.assert_template("item", &[], ()),
            Err(EngineError::TemplateNotFound(_))
        ));
    }
    let mut engine = Engine::new(EngineConfig::default());
    assert!(engine
        .load_str("(deftemplate token (slot value (type EXTERNAL-ADDRESS)))")
        .is_err());
    engine
        .load_str("(deftemplate token (slot value (type EXTERNAL-ADDRESS) (default ?NONE)))")
        .unwrap();
}

#[cfg(feature = "serde")]
#[test]
fn restored_constraints_and_named_seeds_preserve_type_validation() {
    use ferric_rules_runtime::SerializationFormat;
    let engine = Engine::with_rules("(deftemplate item (slot n (type INTEGER))) (deffacts seed (item (n 5))) (defrule consume (item (n ?n)) => (printout t ?n crlf))").unwrap();
    for &format in SerializationFormat::ALL {
        let mut restored = Engine::deserialize(&engine.serialize(format).unwrap(), format).unwrap();
        assert!(restored
            .assert_template("item", &["n"], vec![Value::Float(1.5)])
            .is_err());
        assert!(restored
            .load_str("(defrule invalid => (assert (item (n bad))))")
            .is_err());
        assert_eq!(restored.run(RunLimit::Unlimited).unwrap().rules_fired, 1);
        restored.reset().unwrap();
        assert_eq!(restored.run(RunLimit::Unlimited).unwrap().rules_fired, 1);
        assert_eq!(restored.get_output("t").unwrap(), Some("5\n"));
    }
}
