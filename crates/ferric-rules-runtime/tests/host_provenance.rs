//! Host identities must never alias another engine's interners or arenas.

use ferric_rules_core::{Fact, FerricString};
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
