//! Derived live-query chronology must not change persisted fact state.

#![cfg(feature = "serde")]

use ferric_rules_core::{FactBase, FactId, TemplateId, Value};
use ferric_rules_runtime::SerializationFormat;
use slotmap::SlotMap;

fn encode(facts: &FactBase, format: SerializationFormat) -> Vec<u8> {
    match format {
        SerializationFormat::Bincode => bincode::serialize(facts).unwrap(),
        SerializationFormat::Json => serde_json::to_vec(facts).unwrap(),
        SerializationFormat::Cbor => {
            let mut bytes = Vec::new();
            ciborium::into_writer(facts, &mut bytes).unwrap();
            bytes
        }
        SerializationFormat::MessagePack => rmp_serde::to_vec(facts).unwrap(),
        SerializationFormat::Postcard => postcard::to_allocvec(facts).unwrap(),
    }
}

fn decode(bytes: &[u8], format: SerializationFormat) -> FactBase {
    match format {
        SerializationFormat::Bincode => bincode::deserialize(bytes).unwrap(),
        SerializationFormat::Json => serde_json::from_slice(bytes).unwrap(),
        SerializationFormat::Cbor => ciborium::from_reader(bytes).unwrap(),
        SerializationFormat::MessagePack => rmp_serde::from_slice(bytes).unwrap(),
        SerializationFormat::Postcard => postcard::from_bytes(bytes).unwrap(),
    }
}

fn chronological_ids(facts: &mut FactBase, template: TemplateId) -> Vec<FactId> {
    let mut ids = Vec::new();
    let mut after = None;
    while let Some((id, timestamp)) = facts.next_template_fact_after(template, after) {
        ids.push(id);
        after = Some(timestamp);
    }
    ids
}

#[test]
fn all_codecs_omit_chronology_and_rebuild_it_across_fact_churn() {
    for &format in SerializationFormat::ALL {
        let mut templates: SlotMap<TemplateId, ()> = SlotMap::with_key();
        let item = templates.insert(());
        let other = templates.insert(());
        let empty = templates.insert(());
        let mut facts = FactBase::new();
        let removed = facts.assert_template(item, Box::new([Value::Integer(10)]));
        let survivor = facts.assert_template(item, Box::new([Value::Integer(20)]));
        let other_fact = facts.assert_template(other, Box::new([Value::Integer(100)]));
        facts.retract(removed).unwrap();
        let replacement = facts.assert_template(item, Box::new([Value::Integer(30)]));
        assert_ne!(removed, replacement);

        let cold_bytes = encode(&facts, format);
        assert_eq!(chronological_ids(&mut facts, item), [survivor, replacement]);
        assert_eq!(chronological_ids(&mut facts, other), [other_fact]);
        assert!(chronological_ids(&mut facts, empty).is_empty());
        let warm_bytes = encode(&facts, format);
        assert_eq!(
            cold_bytes,
            warm_bytes,
            "{} persisted the cache",
            format.name()
        );

        let mut restored = decode(&warm_bytes, format);
        assert!(restored.get(removed).is_none());
        let restored_cold_bytes = encode(&restored, format);
        assert_eq!(
            chronological_ids(&mut restored, item),
            [survivor, replacement]
        );
        assert_eq!(encode(&restored, format), restored_cold_bytes);

        // Restore again and change working memory before the first cursor use.
        // Reconstruction must see these new identities, not stale cached IDs.
        let mut restored = decode(&warm_bytes, format);
        restored.retract(survivor).unwrap();
        let appended = restored.assert_template(item, Box::new([Value::Integer(40)]));
        assert_ne!(survivor, appended);
        assert_eq!(
            chronological_ids(&mut restored, item),
            [replacement, appended]
        );

        // Once rebuilt, the cache must continue tracking both retraction and
        // assertion, including an assertion after an empty template was visited.
        restored.retract(replacement).unwrap();
        let later = restored.assert_template(item, Box::new([Value::Integer(50)]));
        assert_eq!(chronological_ids(&mut restored, item), [appended, later]);
        assert!(chronological_ids(&mut restored, empty).is_empty());
        let formerly_empty = restored.assert_template(empty, Box::new([]));
        assert_eq!(chronological_ids(&mut restored, empty), [formerly_empty]);
        assert_eq!(chronological_ids(&mut restored, other), [other_fact]);
        assert!(restored.get(replacement).is_none());

        // A second snapshot of the populated cache must rebuild identically.
        let mut resumed = decode(&encode(&restored, format), format);
        assert_eq!(chronological_ids(&mut resumed, item), [appended, later]);
        assert_eq!(chronological_ids(&mut resumed, empty), [formerly_empty]);
    }
}
