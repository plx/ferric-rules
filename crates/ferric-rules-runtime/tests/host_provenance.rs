//! Host identities must never alias another engine's interners or arenas.

use ferric_rules_core::{ExternalAddress, ExternalTypeId, Fact, FerricString};
use ferric_rules_runtime::{
    Engine, EngineConfig, EngineError, FactHandle, HostValue, RunLimit, Value,
};

#[test]
fn foreign_symbols_and_nested_values_fail_before_assertion() {
    let mut a = Engine::with_rules("(defrule a (left ?x) =>)").unwrap();
    let mut b = Engine::with_rules("(defrule b (right ?x) =>)").unwrap();
    let left = a.intern_symbol("alice").unwrap();
    let right = b.intern_symbol("bob").unwrap();
    assert_eq!(b.resolve_symbol(left), None);
    assert_eq!(b.resolve_symbol(right), Some("bob"));
    for value in [
        HostValue::from(left),
        HostValue::multifield(vec![left.into()]).unwrap(),
    ] {
        assert!(matches!(
            b.assert_ordered("right", value),
            Err(EngineError::ForeignHandle)
        ));
        assert_eq!(b.facts().unwrap().count(), 0);
        assert_eq!(b.agenda_len(), 0);
    }
    b.assert_ordered("right", right).unwrap();
    assert_eq!(b.run(RunLimit::Unlimited).unwrap().rules_fired, 1);
}

#[test]
fn raw_symbols_cannot_gain_provenance_from_a_genuine_neighbor() {
    let mut a = Engine::with_rules("(deffacts seeds (item foreign))").unwrap();
    let fact = a.find_facts("item").unwrap()[0].1;
    let Fact::Ordered(fact) = fact else {
        unreachable!()
    };
    let raw = fact.fields[0].clone();
    assert!(matches!(
        a.assert_ordered("copy", raw.clone()),
        Err(EngineError::InvalidHostValue(_))
    ));
    let genuine = a.symbol_value("local").unwrap();
    assert!(matches!(
        HostValue::multifield(vec![raw.into(), genuine]),
        Err(EngineError::InvalidHostValue(_))
    ));
    assert!(a.find_facts("copy").unwrap().is_empty());
}

#[test]
fn fact_handles_are_stable_local_and_never_reused_after_reset_or_clear() {
    let mut a = Engine::with_rules("(deffacts seeds (item 1))").unwrap();
    let b = Engine::with_rules("(deffacts seeds (item 2))").unwrap();
    let first = a.find_facts("item").unwrap()[0].0;
    let second = b.find_facts("item").unwrap()[0].0;
    assert_ne!(first, second);
    assert_eq!(a.facts().unwrap().next().unwrap().0, first);
    assert!(a.get_fact(second).unwrap().is_none());
    assert!(matches!(a.retract(second), Err(EngineError::FactNotFound(id)) if id == second));
    assert!(b
        .get_fact(FactHandle::from_raw(first.as_raw()))
        .unwrap()
        .is_none());
    a.reset().unwrap();
    let reset = a.find_facts("item").unwrap()[0].0;
    assert_ne!(reset, first);
    assert!(a.get_fact(first).unwrap().is_none());
    a.clear();
    let fresh = a.assert_ordered("item", 3_i64).unwrap();
    assert_ne!(fresh, reset);
    assert!(a.get_fact(reset).unwrap().is_none());
    a.retract(fresh).unwrap();
    let replacement = a.assert_ordered("item", 4_i64).unwrap();
    assert_ne!(replacement, fresh);
    assert!(a.get_fact(fresh).unwrap().is_none());
}

#[test]
fn owned_facts_and_values_retain_origin_and_template_shape() {
    let source = "(deftemplate item (slot id))";
    let mut a = Engine::with_rules(source).unwrap();
    let mut b = Engine::with_rules(source).unwrap();
    let symbol = a.symbol_value("owned").unwrap();
    let id = a.assert_template_slots("item", [("id", symbol)]).unwrap();
    let fact = a.get_fact_owned(id).unwrap().unwrap();
    assert!(matches!(
        b.assert(fact.clone()),
        Err(EngineError::ForeignHandle)
    ));
    assert!(matches!(
        b.assert_ordered("copy", fact.value(0).unwrap()),
        Err(EngineError::ForeignHandle)
    ));
    a.assert_ordered("copy", fact.value(0).unwrap()).unwrap();
    a.retract(id).unwrap();
    a.assert(fact.clone()).unwrap();
    a.reset().unwrap();
    a.load_str("(deftemplate item (slot renamed))").unwrap();
    assert!(matches!(a.assert(fact), Err(EngineError::ForeignHandle)));
    assert_eq!(a.facts().unwrap().count(), 0);
}

#[test]
fn captured_symbol_free_values_are_portable_even_after_source_clear() {
    let text = Value::String(FerricString::Utf8("portable λ".into()));
    let nested = Value::Multifield(Box::new(
        [
            Value::Integer(7),
            Value::Multifield(Box::new(
                [text.clone(), Value::Float(1.5)].into_iter().collect(),
            )),
        ]
        .into_iter()
        .collect(),
    ));
    for value in [
        Value::Integer(7),
        Value::Float(1.5),
        text,
        nested,
        Value::ExternalAddress(ExternalAddress {
            type_id: ExternalTypeId(1),
            token: 42,
        }),
    ] {
        for template in [false, true] {
            let mut source = Engine::with_rules(
                "(deftemplate scalar (slot value)) (deftemplate multiple (multislot value))",
            )
            .unwrap();
            let id = if template {
                let name = if matches!(&value, Value::Multifield(_)) {
                    "multiple"
                } else {
                    "scalar"
                };
                source
                    .assert_template_slots(name, [("value", value.clone())])
                    .unwrap()
            } else {
                source.assert_ordered("item", value.clone()).unwrap()
            };
            let fact = source.get_fact_owned(id).unwrap().unwrap();
            let before_clear = fact.value(0).unwrap();
            let mut destination = Engine::new(EngineConfig::default());
            assert!(matches!(
                destination.assert(fact.clone()),
                Err(EngineError::ForeignHandle)
            ));
            let copied = destination
                .assert_ordered("copy", before_clear.clone())
                .unwrap();
            assert!(destination
                .get_fact_owned(copied)
                .unwrap()
                .unwrap()
                .value(0)
                .unwrap()
                .as_value()
                .structural_eq(&value));
            source.clear();
            assert!(matches!(
                source.assert(fact.clone()),
                Err(EngineError::ForeignHandle)
            ));
            for captured in [before_clear, fact.value(0).unwrap()] {
                source.assert_ordered("copy", captured.clone()).unwrap();
                destination.assert_ordered("copy", captured).unwrap();
            }
            assert!(fact.value(1).is_none());
        }
    }
}

#[test]
fn captured_direct_and_nested_symbols_reject_foreign_and_stale_owners() {
    for template in [false, true] {
        for nested in [false, true] {
            let mut source = Engine::with_rules(
                "(deftemplate scalar (slot value)) (deftemplate multiple (multislot value))",
            )
            .unwrap();
            let symbol = source.symbol_value("owned").unwrap();
            let value = if nested {
                HostValue::multifield(vec![
                    7_i64.into(),
                    HostValue::multifield(vec![symbol]).unwrap(),
                ])
                .unwrap()
            } else {
                symbol
            };
            let id = if template {
                source
                    .assert_template_slots(
                        if nested { "multiple" } else { "scalar" },
                        [("value", value)],
                    )
                    .unwrap()
            } else {
                source.assert_ordered("item", value).unwrap()
            };
            let fact = source.get_fact_owned(id).unwrap().unwrap();
            let before_clear = fact.value(0).unwrap();
            source.assert_ordered("copy", before_clear.clone()).unwrap();
            let mut destination = Engine::new(EngineConfig::default());
            assert!(matches!(
                destination.assert_ordered("copy", before_clear.clone()),
                Err(EngineError::ForeignHandle)
            ));
            source.clear();
            for captured in [before_clear, fact.value(0).unwrap()] {
                assert!(matches!(
                    source.assert_ordered("copy", captured.clone()),
                    Err(EngineError::ForeignHandle)
                ));
                assert!(matches!(
                    destination.assert_ordered("copy", captured),
                    Err(EngineError::ForeignHandle)
                ));
            }
            assert_eq!(source.fact_count(), 0);
            assert_eq!(destination.fact_count(), 0);
        }
    }
}

#[test]
fn symbol_handles_survive_reset_and_fail_after_clear() {
    let mut engine = Engine::new(EngineConfig::default());
    let symbol = engine.intern_symbol("ready").unwrap();
    engine.reset().unwrap();
    engine.assert_ordered("state", symbol).unwrap();
    engine.clear();
    assert_eq!(engine.resolve_symbol(symbol), None);
    assert!(matches!(
        engine.assert_ordered("state", symbol),
        Err(EngineError::ForeignHandle)
    ));
    assert_eq!(engine.facts().unwrap().count(), 0);
}

#[test]
fn recursive_void_and_invalid_ascii_values_are_rejected() {
    let mut engine = Engine::new(EngineConfig::default());
    for value in [
        Value::Multifield(Box::new([Value::Void].into_iter().collect())),
        Value::String(FerricString::Ascii(vec![255].into_boxed_slice())),
    ] {
        assert!(matches!(
            engine.assert_ordered("invalid", value),
            Err(EngineError::InvalidHostValue(_))
        ));
    }
    assert_eq!(engine.facts().unwrap().count(), 0);
}

#[cfg(feature = "serde")]
#[test]
fn restored_state_gets_fresh_host_ids_and_continues_identical_rule_work() {
    use ferric_rules_runtime::SerializationFormat;
    let mut original = Engine::with_rules(
        "(deffacts seeds (item ready)) (defrule once ?f <- (item ready) => (retract ?f) (assert (done)))"
    ).unwrap();
    let id = original.find_facts("item").unwrap()[0].0;
    let symbol = original.intern_symbol("ready").unwrap();
    let bytes = original.serialize(SerializationFormat::Cbor).unwrap();
    let mut restored = Engine::deserialize(&bytes, SerializationFormat::Cbor).unwrap();
    assert!(restored.get_fact(id).unwrap().is_none());
    assert_eq!(restored.resolve_symbol(symbol), None);
    assert_ne!(restored.find_facts("item").unwrap()[0].0, id);
    assert_eq!(restored.run(RunLimit::Unlimited).unwrap().rules_fired, 1);
    assert_eq!(restored.find_facts("done").unwrap().len(), 1);
    assert_eq!(original.find_facts("item").unwrap().len(), 1);
}

#[test]
fn concurrent_readers_observe_one_stable_handle_per_fact() {
    use std::sync::{Arc, Barrier};
    let engine =
        Arc::new(Engine::with_rules("(deffacts seed (item 1) (item 2) (item 3))").unwrap());
    let barrier = Arc::new(Barrier::new(5));
    let threads: Vec<_> = (0..4)
        .map(|_| {
            let engine = Arc::clone(&engine);
            let barrier = Arc::clone(&barrier);
            std::thread::spawn(move || {
                barrier.wait();
                let handles: Vec<_> = engine.facts().unwrap().map(|(id, _)| id).collect();
                for &handle in &handles {
                    assert!(engine.get_fact(handle).unwrap().is_some());
                }
                handles
            })
        })
        .collect();
    barrier.wait();
    let results: Vec<_> = threads
        .into_iter()
        .map(|thread| thread.join().unwrap())
        .collect();
    assert_eq!(results[0].len(), 3);
    assert!(results.iter().all(|handles| handles == &results[0]));
    assert!(results[0]
        .iter()
        .all(|handle| handle.as_raw() > (1_u64 << 53)));
}

#[test]
fn host_value_depth_limit_accepts_the_boundary_and_rejects_the_next_level() {
    let mut engine = Engine::new(EngineConfig::default());
    let mut value = Value::Integer(1);
    for _ in 0..32 {
        value = Value::Multifield(Box::new([value].into_iter().collect()));
    }
    engine.assert_ordered("boundary", value.clone()).unwrap();
    let excessive = Value::Multifield(Box::new([value].into_iter().collect()));
    assert!(matches!(
        engine.assert_ordered("too-deep", excessive),
        Err(EngineError::InvalidHostValue(_))
    ));
    assert_eq!(engine.facts().unwrap().count(), 1);
    assert!(engine.find_facts("too-deep").unwrap().is_empty());
}

#[test]
fn ordered_host_facts_cannot_reenter_an_explicit_template_identity() {
    let mut engine = Engine::new(EngineConfig::default());
    let id = engine.assert_ordered("item", 7_i64).unwrap();
    let captured = engine.get_fact_owned(id).unwrap().unwrap();
    engine.retract(id).unwrap();
    engine.load_str("(deftemplate item (slot value))").unwrap();
    assert!(matches!(
        engine.assert(captured),
        Err(EngineError::InvalidHostValue(_))
    ));
    assert!(matches!(
        engine.assert_ordered("item", 7_i64),
        Err(EngineError::InvalidHostValue(_))
    ));
    assert_eq!(engine.fact_count(), 0);
    engine
        .assert_template_slots("item", [("value", 7_i64)])
        .unwrap();
    assert_eq!(engine.fact_count(), 1);
}

#[test]
fn owned_template_facts_cannot_escape_changed_primitive_constraints() {
    let mut engine = Engine::with_rules("(deftemplate item (slot value (type INTEGER)))").unwrap();
    let id = engine
        .assert_template_slots("item", [("value", 7_i64)])
        .unwrap();
    let captured = engine.get_fact_owned(id).unwrap().unwrap();
    engine.retract(id).unwrap();
    engine
        .load_str("(deftemplate item (slot value (type FLOAT)))")
        .unwrap();
    assert!(matches!(
        engine.assert(captured),
        Err(EngineError::ForeignHandle)
    ));
    assert!(matches!(
        engine.assert_template_slots("item", [("value", 7_i64)]),
        Err(EngineError::InvalidSlotValue { .. })
    ));
    engine
        .assert_template_slots("item", [("value", 1.5_f64)])
        .unwrap();
    assert_eq!(engine.fact_count(), 1);
}

#[test]
fn ordered_host_names_cannot_bypass_private_or_qualified_templates() {
    let mut engine =
        Engine::with_rules("(defmodule PRIVATE) (deftemplate PRIVATE::item (slot value))").unwrap();
    for relation in ["item", "PRIVATE::item", "MAIN::item"] {
        assert!(matches!(
            engine.assert_ordered(relation, 7_i64),
            Err(EngineError::InvalidHostValue(_))
        ));
    }
    assert_eq!(engine.fact_count(), 0);
}
