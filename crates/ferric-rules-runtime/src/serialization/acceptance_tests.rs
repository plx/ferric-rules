//! Real-engine persistence boundaries and dormant-state validation regressions.

use super::*;
use crate::fact_initializer::PreparedFact;
use crate::{EngineConfig, RunLimit};

fn roundtrip(engine: &Engine, format: SerializationFormat) -> Engine {
    let bytes = engine.serialize(format).unwrap();
    assert!(bytes.len() <= MAX_SNAPSHOT_BYTES);
    Engine::deserialize(&bytes, format).unwrap()
}

#[test]
fn fifty_thousand_runtime_template_facts_roundtrip_in_both_codecs() {
    const COUNT: usize = 50_000;
    let mut engine = Engine::with_rules("(deftemplate item (slot id) (slot v))").unwrap();
    for index in 0..i64::try_from(COUNT).unwrap() {
        engine
            .assert_template_slots("item", [("id", index), ("v", index)])
            .unwrap();
    }
    assert_eq!(engine.facts().unwrap().count(), COUNT);

    for &format in SerializationFormat::ALL {
        let bytes = engine.serialize(format).unwrap();
        assert!(bytes.len() <= MAX_SNAPSHOT_BYTES);
        eprintln!(
            "snapshot acceptance: 50000 two-slot facts {format:?}: {} bytes",
            bytes.len()
        );
        let mut restored = Engine::deserialize(&bytes, format).unwrap();
        let mut seen = vec![false; COUNT];
        let mut middle = None;
        for (handle, _) in restored.facts().unwrap() {
            let Value::Integer(index) = restored.get_fact_slot_by_name(handle, "id").unwrap()
            else {
                panic!("item id must remain an integer");
            };
            let index = usize::try_from(*index).unwrap();
            assert!(index < COUNT);
            assert!(!std::mem::replace(&mut seen[index], true));
            assert!(
                matches!(restored.get_fact_slot_by_name(handle, "v").unwrap(),
                Value::Integer(value) if *value == i64::try_from(index).unwrap())
            );
            if index == COUNT / 2 {
                middle = Some(handle);
            }
        }
        assert!(seen.into_iter().all(|present| present));
        assert_eq!(restored.agenda_len(), 0);
        assert_eq!(restored.run(RunLimit::Unlimited).unwrap().rules_fired, 0);
        restored.retract(middle.unwrap()).unwrap();
        assert_eq!(restored.facts().unwrap().count(), COUNT - 1);
        let value = i64::try_from(COUNT / 2).unwrap();
        restored
            .assert_template_slots("item", [("id", value), ("v", value)])
            .unwrap();
        assert_eq!(restored.facts().unwrap().count(), COUNT);
    }
}

#[test]
fn five_thousand_pending_joins_resume_in_both_codecs() {
    const COUNT: usize = 5_000;
    let mut engine =
        Engine::with_rules("(defrule r (key ?k) (val ?k) => (assert (seen ?k)))").unwrap();
    for index in 0..i64::try_from(COUNT).unwrap() {
        engine.assert_ordered("key", index).unwrap();
        engine.assert_ordered("val", index).unwrap();
    }
    assert_eq!(engine.agenda_len(), COUNT);
    for &format in SerializationFormat::ALL {
        let bytes = engine.serialize(format).unwrap();
        assert!(bytes.len() <= MAX_SNAPSHOT_BYTES);
        eprintln!(
            "snapshot acceptance: 5000 pending joins {format:?}: {} bytes",
            bytes.len()
        );
        let mut restored = Engine::deserialize(&bytes, format).unwrap();
        assert_eq!(restored.facts().unwrap().count(), COUNT * 2);
        assert_eq!(restored.agenda_len(), COUNT);
        assert_eq!(
            restored.run(RunLimit::Unlimited).unwrap().rules_fired,
            COUNT
        );
        let mut seen = vec![false; COUNT];
        for (_, fact) in restored.find_facts("seen").unwrap() {
            let Fact::Ordered(fact) = fact else {
                panic!("seen must remain ordered");
            };
            let [Value::Integer(index)] = fact.fields.as_slice() else {
                panic!("seen must contain one integer key");
            };
            let index = usize::try_from(*index).unwrap();
            assert!(index < COUNT);
            assert!(!std::mem::replace(&mut seen[index], true));
        }
        assert!(seen.into_iter().all(|present| present));
        assert_eq!(restored.agenda_len(), 0);
        assert_eq!(restored.run(RunLimit::Unlimited).unwrap().rules_fired, 0);
    }
}

fn dormant_initializer() -> Engine {
    let mut engine = Engine::new(EngineConfig::default());
    engine
        .load_str("(deffacts seed (pending (progn (printout t forbidden) 7)))")
        .unwrap();
    assert!(engine.get_output("t").unwrap_or_default().is_empty());
    assert!(engine.find_facts("pending").unwrap().is_empty());
    engine
}

#[test]
fn dormant_ordered_relations_reject_dangling_symbols_without_evaluation() {
    let engine = dormant_initializer();
    let original = engine.serialize(SerializationFormat::Json).unwrap();
    let original: serde_json::Value = serde_json::from_slice(&original[HEADER_LEN..]).unwrap();
    for &format in SerializationFormat::ALL {
        // Check the JSON-value bridge used to forge either codec remains valid.
        let bytes = envelope(encode(&original, format).unwrap(), format).unwrap();
        let mut control = Engine::deserialize(&bytes, format).unwrap();
        assert!(control.get_output("t").unwrap_or_default().is_empty());
        control.reset().unwrap();
        assert_eq!(control.get_output("t"), Some("forbidden"));
        let pending = control.find_facts("pending").unwrap();
        assert_eq!(pending.len(), 1);
        assert!(matches!(pending[0].1, Fact::Ordered(fact)
            if matches!(fact.fields.as_slice(), [Value::Integer(7)])));

        for pool in ["Ascii", "Utf8"] {
            let forged = serde_json::json!({pool: u32::MAX});
            let mut state = original.clone();
            state["registered_deffacts"][0]["facts"][0]["Ordered"]["relation"] = forged.clone();
            let bytes = envelope(encode(&state, format).unwrap(), format).unwrap();
            assert!(
                matches!(Engine::deserialize(&bytes, format),
                Err(SerializationError::InvalidState(message)) if message.contains("dangling symbol")),
                "{format:?} {pool}: reject before installing dormant code"
            );

            let mut invalid = dormant_initializer();
            let PreparedFact::Ordered { relation, .. } =
                &mut invalid.registered_deffacts[0].facts[0]
            else {
                panic!("ordered initializer required");
            };
            *relation = serde_json::from_value(forged).unwrap();
            assert!(matches!(invalid.serialize(format),
                Err(SerializationError::InvalidState(message)) if message.contains("dangling symbol")));
            assert!(invalid.get_output("t").unwrap_or_default().is_empty());
        }
    }
}

#[test]
fn source_nested_exists_at_three_and_four_levels_resumes_lifecycle() {
    for (condition, relations) in [
        (
            "(exists (a) (exists (b) (exists (c) (d))))",
            &['a', 'b', 'c', 'd'][..],
        ),
        (
            "(exists (a) (exists (b) (exists (c) (exists (d) (e)))))",
            &['a', 'b', 'c', 'd', 'e'][..],
        ),
    ] {
        // Each multi-pattern exists lowers to two NCCs: source depths 3/4
        // require compiled callback depths 6/8 without widening source syntax.
        let source = format!("(defrule present {condition} => (printout t hit crlf))");
        for &format in SerializationFormat::ALL {
            let mut engine = Engine::with_rules(&source).unwrap();
            engine = roundtrip(&engine, format);
            assert_eq!(engine.run(RunLimit::Unlimited).unwrap().rules_fired, 0);
            for relation in relations {
                engine.assert_ordered(&relation.to_string(), ()).unwrap();
            }
            engine = roundtrip(&engine, format);
            assert_eq!(engine.run(RunLimit::Unlimited).unwrap().rules_fired, 1);
            assert_eq!(engine.get_output("t"), Some("hit\n"));

            let leaf = relations.last().unwrap().to_string();
            let handle = engine.find_facts(&leaf).unwrap()[0].0;
            engine.retract(handle).unwrap();
            engine = roundtrip(&engine, format);
            assert_eq!(engine.run(RunLimit::Unlimited).unwrap().rules_fired, 0);
            engine.assert_ordered(&leaf, ()).unwrap();
            engine = roundtrip(&engine, format);
            assert_eq!(engine.run(RunLimit::Unlimited).unwrap().rules_fired, 1);
            assert_eq!(engine.get_output("t"), Some("hit\nhit\n"));

            engine.reset().unwrap();
            engine = roundtrip(&engine, format);
            assert_eq!(engine.run(RunLimit::Unlimited).unwrap().rules_fired, 0);
            for relation in relations {
                engine.assert_ordered(&relation.to_string(), ()).unwrap();
            }
            assert_eq!(engine.run(RunLimit::Unlimited).unwrap().rules_fired, 1);
            assert_eq!(engine.get_output("t"), Some("hit\n"));
        }
    }
}

#[test]
fn fifth_source_quantifier_level_remains_rejected() {
    let mut engine = Engine::new(EngineConfig::default());
    let errors = engine.load_str(
        "(defrule too-deep (exists (a) (exists (b) (exists (c) (exists (d) (exists (e) (f)))))) =>)",
    ).unwrap_err();
    assert!(
        errors.iter().any(|error| matches!(error,
        crate::LoadError::Validation(details) if details.iter().any(|detail|
            detail.to_string().contains("nesting depth 5 exceeds maximum of 4")
                && detail.location.is_some_and(|location| location.line == 1)))),
        "{errors:?}"
    );
    assert_eq!(engine.agenda_len(), 0);
}
