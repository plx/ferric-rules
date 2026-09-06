//! Public host assertions must not silently discard or reshape supplied slots.

use ferric_rules_runtime::{Engine, EngineError, Multifield, RunLimit, Value};

fn engine() -> Engine {
    Engine::with_rules(
        "(deftemplate item (slot id (default ?NONE)) (slot optional (default 7)) (multislot tags))
         (defrule observe (item (id ?id)) => (assert (observed ?id)))",
    )
    .unwrap()
}

#[test]
fn slot_count_errors_report_both_lengths_and_leave_no_work() {
    let mut engine = engine();
    for (names, values) in [
        (vec!["id", "optional"], vec![Value::Integer(1)]),
        (vec!["id"], vec![Value::Integer(1), Value::Integer(2)]),
    ] {
        let counts = (names.len(), values.len());
        let error = engine.assert_template("item", &names, values).unwrap_err();
        assert!(
            matches!(error, EngineError::SlotCountMismatch { names, values }
            if (names, values) == counts)
        );
        assert!(error.to_string().contains(&format!("{} names", counts.0)));
        assert!(error.to_string().contains(&format!("{} values", counts.1)));
        assert_eq!(engine.facts().unwrap().count(), 0);
        assert_eq!(engine.agenda_len(), 0);
        assert_eq!(engine.run(RunLimit::Unlimited).unwrap().rules_fired, 0);
    }
}

#[test]
fn invalid_overrides_reject_before_fact_or_activation_is_created() {
    let mut engine = engine();
    for (names, values, expected) in [
        (
            vec!["id", "id"],
            vec![Value::Integer(1), Value::Integer(2)],
            "duplicate slot",
        ),
        (
            vec!["id", "missing"],
            vec![Value::Integer(1), Value::Integer(2)],
            "no slot",
        ),
        (vec!["optional"], vec![Value::Integer(1)], "default ?NONE"),
        (
            vec!["id"],
            vec![Value::Multifield(Box::new(Multifield::new()))],
            "one scalar",
        ),
        (vec!["id"], vec![Value::Void], "void"),
    ] {
        let error = engine.assert_template("item", &names, values).unwrap_err();
        assert!(error.to_string().contains(expected), "{error}");
        assert_eq!(engine.facts().unwrap().count(), 0);
        assert_eq!(engine.agenda_len(), 0);
    }
    let id = engine
        .assert_template_slots("item", [("id", Value::Integer(42))])
        .unwrap();
    assert!(matches!(
        engine.get_fact_slot_by_name(id, "id").unwrap(),
        Value::Integer(42)
    ));
    assert_eq!(engine.run(RunLimit::Unlimited).unwrap().rules_fired, 1);
}

#[test]
fn paired_slots_preserve_values_defaults_and_multislot_cardinality() {
    let mut engine = engine();
    let fields = Box::new(
        [Value::Integer(10), Value::Integer(20)]
            .into_iter()
            .collect(),
    );
    let id = engine
        .assert_template_slots(
            "item",
            [
                ("tags", Value::Multifield(fields)),
                ("id", Value::Integer(1)),
            ],
        )
        .unwrap();
    assert!(matches!(
        engine.get_fact_slot_by_name(id, "optional").unwrap(),
        Value::Integer(7)
    ));
    assert!(
        matches!(engine.get_fact_slot_by_name(id, "tags").unwrap(), Value::Multifield(values)
        if matches!(values.as_slice(), [Value::Integer(10), Value::Integer(20)]))
    );
    let scalar = engine
        .assert_template_slots(
            "item",
            [("id", Value::Integer(2)), ("tags", Value::Integer(99))],
        )
        .unwrap();
    assert!(
        matches!(engine.get_fact_slot_by_name(scalar, "tags").unwrap(), Value::Multifield(values)
        if matches!(values.as_slice(), [Value::Integer(99)]))
    );
    assert_eq!(engine.run(RunLimit::Unlimited).unwrap().rules_fired, 2);
    assert_eq!(engine.find_facts("observed").unwrap().len(), 2);
}
