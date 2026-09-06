//! Public ownership-transfer contract. These tests deliberately retain shared
//! match values outside the engine while moving, using, and destroying it.

use std::sync::{Arc, Barrier, Mutex};
use std::thread;

use ferric_rules_core::binding::ValueRef;
use ferric_rules_core::{Fact, ReteNetwork, Token};
use ferric_rules_runtime::{
    AtomKey, Engine, EngineConfig, ExternalAddress, ExternalTypeId, HaltReason, Multifield,
    RunLimit, Value,
};

#[test]
fn ownership_traits_are_structural() {
    fn assert_send<T: Send>() {}
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send::<Engine>();
    assert_send::<EngineConfig>();
    assert_send_sync::<Mutex<Engine>>();
    assert_send_sync::<Value>();
    assert_send_sync::<ValueRef>();
    assert_send_sync::<Fact>();
    assert_send_sync::<Token>();
    assert_send_sync::<ReteNetwork>();
    assert_send_sync::<ExternalAddress>();
    assert_send_sync::<AtomKey>();
}

const TRANSFER_RULES: &str = r#"
(deftemplate item (slot number))
(deffacts startup (item (number 1)))
(defglobal ?*count* = 0)
(defrule record
    (item (number ?number))
    =>
    (bind ?*count* (+ ?*count* 1))
    (assert (recorded ?number))
    (printout t ?number "|"))
"#;

#[test]
fn use_and_destroy_after_creator_thread_has_exited() {
    let mut engine = thread::spawn(|| Engine::with_rules(TRANSFER_RULES).unwrap())
        .join()
        .unwrap();
    let first = engine.run(RunLimit::Count(1)).unwrap();
    assert_eq!(first.rules_fired, 1);
    assert_eq!(engine.get_output("t"), Some("1|"));

    thread::spawn(move || {
        let id = engine
            .assert_template("item", &["number"], vec![Value::Integer(2)])
            .unwrap();
        let second = engine.run(RunLimit::Unlimited).unwrap();
        assert_eq!(second.rules_fired, 1);
        assert_eq!(second.halt_reason, HaltReason::AgendaEmpty);
        assert_eq!(engine.get_output("t"), Some("1|2|"));
        assert_eq!(engine.find_facts("recorded").unwrap().len(), 2);
        assert!(matches!(
            engine.get_global("count"),
            Some(Value::Integer(2))
        ));
        engine.retract(id).unwrap();
        assert!(engine.get_fact(id).unwrap().is_none());
        engine.reset().unwrap();
        assert_eq!(engine.run(RunLimit::Unlimited).unwrap().rules_fired, 1);
        assert_eq!(engine.get_output("t"), Some("1|"));
        drop(engine);
    })
    .join()
    .unwrap();
}

#[test]
fn a_host_mutex_serializes_independent_threads() {
    let engine = Mutex::new(
        Engine::with_rules("(defrule record (input ?n) => (assert (recorded ?n)))").unwrap(),
    );
    let start = Barrier::new(4);
    thread::scope(|scope| {
        for number in 0..4_i64 {
            let engine = &engine;
            let start = &start;
            scope.spawn(move || {
                start.wait();
                let mut engine = engine.lock().unwrap();
                engine.assert_ordered("input", number).unwrap();
                assert_eq!(engine.run(RunLimit::Unlimited).unwrap().rules_fired, 1);
            });
        }
    });
    let engine = engine.into_inner().unwrap();
    assert_eq!(engine.find_facts("input").unwrap().len(), 4);
    assert_eq!(engine.find_facts("recorded").unwrap().len(), 4);
}

#[test]
fn escaped_shared_values_survive_concurrent_engine_use_and_destruction() {
    let mut engine =
        Engine::with_rules("(defrule copy (payload ?value) => (assert (seen ?value)))").unwrap();
    let text = Value::String(engine.create_string("shared payload").unwrap());
    let nested: Multifield = [
        text.clone(),
        Value::Multifield(Box::new(
            [Value::Integer(7), text.clone()].into_iter().collect(),
        )),
    ]
    .into_iter()
    .collect();
    engine.assert_ordered("payload", text).unwrap();
    engine
        .assert_ordered("payload", [Value::Multifield(Box::new(nested))])
        .unwrap();
    assert_eq!(engine.agenda_len(), 2);

    // Retain actual handles from live RETE matches, not independently created
    // values. Their allocations will also be released by the moved engine.
    let escaped: Vec<_> = engine
        .rete()
        .agenda
        .iter_activations()
        .flat_map(|activation| {
            engine
                .rete()
                .token_store
                .get(activation.token)
                .unwrap()
                .bindings
                .as_ref()
                .iter()
                .flatten()
                .cloned()
        })
        .collect();
    assert_eq!(escaped.len(), 2);
    let weak: Vec<_> = escaped
        .iter()
        .map(|value| match value {
            ValueRef::Shared(shared) => Arc::downgrade(shared),
            ValueRef::Inline(_) => panic!("string and multifield matches must share their values"),
        })
        .collect();
    let start = Arc::new(Barrier::new(2));
    let worker_start = Arc::clone(&start);
    let worker = thread::spawn(move || {
        worker_start.wait();
        assert_eq!(engine.run(RunLimit::Unlimited).unwrap().rules_fired, 2);
        assert_eq!(engine.find_facts("seen").unwrap().len(), 2);
        assert!(engine.action_diagnostics().is_empty());
        drop(engine);
    });
    start.wait();
    for _ in 0..1_000 {
        for value in &escaped {
            let copied = value.clone();
            assert!(copied.structural_eq(value));
            drop(copied);
        }
    }
    worker.join().unwrap();
    assert!(weak.iter().all(|value| value.upgrade().is_some()));
    drop(escaped);
    assert!(weak.iter().all(|value| value.upgrade().is_none()));
}

#[test]
fn external_identities_match_by_host_namespace_and_token_after_transfer() {
    let identity = ExternalAddress {
        type_id: ExternalTypeId(17),
        token: u64::MAX,
    };
    let value = Value::ExternalAddress(identity);
    let key = AtomKey::from_value(&value).unwrap();
    assert!(key.to_value().structural_eq(&value));
    let mut engine =
        Engine::with_rules("(defrule match (left ?v) (right ?v) => (assert (matched)))").unwrap();
    let first = engine.assert_ordered("left", value.clone()).unwrap();
    assert_eq!(engine.assert_ordered("left", value.clone()).unwrap(), first);
    engine
        .assert_ordered(
            "right",
            Value::ExternalAddress(ExternalAddress {
                type_id: ExternalTypeId(18),
                ..identity
            }),
        )
        .unwrap();
    engine
        .assert_ordered(
            "right",
            Value::ExternalAddress(ExternalAddress {
                token: 1,
                ..identity
            }),
        )
        .unwrap();
    assert_eq!(engine.run(RunLimit::Unlimited).unwrap().rules_fired, 0);

    thread::spawn(move || {
        engine.assert_ordered("right", key.to_value()).unwrap();
        assert_eq!(engine.run(RunLimit::Unlimited).unwrap().rules_fired, 1);
        assert_eq!(engine.find_facts("matched").unwrap().len(), 1);
        let Fact::Ordered(fact) = engine.get_fact(first).unwrap().unwrap() else {
            panic!("expected ordered fact");
        };
        assert!(fact.fields[0].structural_eq(&value));
        drop(engine);
    })
    .join()
    .unwrap();
    // The engine never acquired ownership of a host object: this identity is
    // still usable as a registry key after the engine has been destroyed.
    assert_eq!(identity.token, u64::MAX);
}

#[cfg(feature = "serde")]
#[test]
fn snapshots_and_restored_engines_can_cross_threads() {
    use ferric_rules_runtime::SerializationFormat;

    for &format in SerializationFormat::ALL {
        let mut engine = Engine::with_rules(TRANSFER_RULES).unwrap();
        assert_eq!(engine.run(RunLimit::Unlimited).unwrap().rules_fired, 1);
        engine
            .assert_template("item", &["number"], vec![Value::Integer(2)])
            .unwrap();
        let bytes = engine.serialize(format).unwrap();
        let restored = thread::spawn(move || Engine::deserialize(&bytes, format).unwrap())
            .join()
            .unwrap();
        thread::spawn(move || {
            let mut restored = restored;
            assert_eq!(restored.run(RunLimit::Unlimited).unwrap().rules_fired, 1);
            assert_eq!(restored.get_output("t"), Some("1|2|"));
            assert!(matches!(
                restored.get_global("count"),
                Some(Value::Integer(2))
            ));
            assert_eq!(restored.find_facts("recorded").unwrap().len(), 2);
        })
        .join()
        .unwrap();
    }
}

#[cfg(feature = "serde")]
#[test]
fn snapshot_boundaries_reject_host_identities_including_nested_values() {
    use ferric_rules_runtime::SerializationFormat;

    for nested in [false, true] {
        let mut engine = Engine::new(EngineConfig::default());
        let mut value = Value::ExternalAddress(ExternalAddress {
            type_id: ExternalTypeId(1),
            token: 42,
        });
        if nested {
            value = Value::Multifield(Box::new([value].into_iter().collect()));
        }
        engine.assert_ordered("host", [value]).unwrap();
        for &format in SerializationFormat::ALL {
            let error = engine.serialize(format).unwrap_err();
            assert!(error.to_string().contains("ExternalAddress"));
        }
    }
}
