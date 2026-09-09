//! Negative matches must retain every supporting blocker across transitions.

use ferric_rules_runtime::FactHandle as FactId;
use ferric_rules_runtime::{Engine, EngineConfig, HaltReason, Multifield, RunLimit, Value};

const RULE: &str =
    "(defrule select (candidate ?key) (not (block ?key ?reason)) => (assert (selected ?key)))";

fn candidate(engine: &mut Engine, key: Value) -> FactId {
    engine.assert_ordered("candidate", vec![key]).unwrap()
}

fn blocker(engine: &mut Engine, key: Value, reason: i64) -> FactId {
    engine
        .assert_ordered("block", vec![key, Value::Integer(reason)])
        .unwrap()
}

fn fire(engine: &mut Engine, expected: usize) {
    let run = engine.run(RunLimit::Unlimited).unwrap();
    assert_eq!(run.rules_fired, expected);
    assert_eq!(run.halt_reason, HaltReason::AgendaEmpty);
    assert!(engine.action_diagnostics().is_empty());
    #[cfg(debug_assertions)]
    engine.debug_assert_consistency();
}

#[test]
fn clips_correlated_not_waits_for_the_last_blocker() {
    let mut engine = Engine::with_rules(include_str!("fixtures/not_last_blocker.clp")).unwrap();
    fire(&mut engine, 3);
    assert_eq!(
        engine.get_output("t").unwrap(),
        Some("unblocked\nselected a\n")
    );
    assert!(engine.find_facts("block").unwrap().is_empty());
    assert!(engine.find_facts("phase").unwrap().is_empty());
    let facts = engine.find_facts("result").unwrap();
    assert_eq!(facts.len(), 1);
    let ferric_rules_core::Fact::Ordered(fact) = facts[0].1 else {
        panic!("result must be an ordered fact");
    };
    let Value::Symbol(key) = fact.fields[0] else {
        panic!("result key must be a symbol");
    };
    assert_eq!(engine.resolve_core_symbol(key), Some("a"));
}

#[test]
fn late_blockers_remain_correlated_and_cancel_queued_activation() {
    for remove_first in [true, false] {
        let mut engine = Engine::with_rules(RULE).unwrap();
        candidate(&mut engine, Value::Integer(7));
        candidate(&mut engine, Value::Integer(8));
        assert_eq!(engine.agenda_len(), 2);
        let first = blocker(&mut engine, Value::Integer(7), 1);
        let second = blocker(&mut engine, Value::Integer(7), 2);
        let unrelated = blocker(&mut engine, Value::Integer(99), 1);
        fire(&mut engine, 1);
        assert_eq!(engine.find_facts("selected").unwrap().len(), 1);
        let (earlier, last) = if remove_first {
            (first, second)
        } else {
            (second, first)
        };
        engine.retract(earlier).unwrap();
        fire(&mut engine, 0);
        engine.retract(unrelated).unwrap();
        fire(&mut engine, 0);
        engine.retract(last).unwrap();
        fire(&mut engine, 1);
        assert_eq!(engine.find_facts("selected").unwrap().len(), 2);
    }
}

#[test]
fn rule_loading_and_blocker_arrival_orders_preserve_all_witnesses() {
    for load_after_facts in [false, true] {
        for candidate_first in [false, true] {
            let mut engine = Engine::new(EngineConfig::default());
            if !load_after_facts {
                engine.load_str(RULE).unwrap();
            }
            if candidate_first {
                candidate(&mut engine, Value::Integer(4));
            }
            let first = blocker(&mut engine, Value::Integer(4), 1);
            let second = blocker(&mut engine, Value::Integer(4), 2);
            if !candidate_first {
                candidate(&mut engine, Value::Integer(4));
            }
            if load_after_facts {
                engine.load_str(RULE).unwrap();
            }
            fire(&mut engine, 0);
            engine.retract(first).unwrap();
            fire(&mut engine, 0);
            engine.retract(second).unwrap();
            fire(&mut engine, 1);
        }
    }
}

#[test]
fn new_blocker_while_already_blocked_survives_repeated_cycles() {
    let mut engine = Engine::with_rules(RULE).unwrap();
    candidate(&mut engine, Value::Integer(3));
    fire(&mut engine, 1);
    for reason in [10, 20] {
        let first = blocker(&mut engine, Value::Integer(3), reason);
        let second = blocker(&mut engine, Value::Integer(3), reason + 1);
        engine.retract(first).unwrap();
        let third = blocker(&mut engine, Value::Integer(3), reason + 2);
        fire(&mut engine, 0);
        engine.retract(second).unwrap();
        fire(&mut engine, 0);
        engine.retract(third).unwrap();
        fire(&mut engine, 1);
    }
    assert_eq!(engine.find_facts("selected").unwrap().len(), 1);
}

#[test]
fn retracting_blocked_parent_cleans_all_later_blocker_links() {
    let mut engine = Engine::with_rules(RULE).unwrap();
    let parent = candidate(&mut engine, Value::Integer(5));
    let first = blocker(&mut engine, Value::Integer(5), 1);
    let second = blocker(&mut engine, Value::Integer(5), 2);
    engine.retract(parent).unwrap();
    engine.retract(first).unwrap();
    engine.retract(second).unwrap();
    fire(&mut engine, 0);
    candidate(&mut engine, Value::Integer(5));
    fire(&mut engine, 1);
}

#[test]
fn shared_string_key_and_nested_parent_values_preserve_later_blockers() {
    let mut engine = Engine::with_rules(
        "(defrule select (candidate ?key ?payload) (not (block ?key ?reason)) => (assert (selected ?key ?payload)))",
    )
    .unwrap();
    let text = Value::String(
        engine
            .create_string("shared key with nested fields")
            .unwrap(),
    );
    let nested: Multifield = [Value::Integer(1), Value::Integer(2)].into_iter().collect();
    let payload: Multifield = [text.clone(), Value::Multifield(Box::new(nested))]
        .into_iter()
        .collect();
    engine
        .assert_ordered(
            "candidate",
            vec![text.clone(), Value::Multifield(Box::new(payload))],
        )
        .unwrap();
    let first = blocker(&mut engine, text.clone(), 1);
    let second = blocker(&mut engine, text.clone(), 2);
    engine.retract(first).unwrap();
    fire(&mut engine, 0);
    engine.retract(second).unwrap();
    fire(&mut engine, 1);
    let selected = engine.find_facts("selected").unwrap();
    assert_eq!(selected.len(), 1);
    let ferric_rules_core::Fact::Ordered(fact) = selected[0].1 else {
        panic!("selected must remain ordered");
    };
    assert!(fact.fields[0].structural_eq(&text));
    assert!(matches!(&fact.fields[2], Value::Multifield(values) if values.len() == 2));
}

#[cfg(feature = "serde")]
#[test]
fn snapshots_keep_late_blockers_until_the_last_retraction() {
    use ferric_rules_runtime::SerializationFormat;

    let mut engine = Engine::with_rules(RULE).unwrap();
    candidate(&mut engine, Value::Integer(6));
    let first = blocker(&mut engine, Value::Integer(6), 1);
    let second = blocker(&mut engine, Value::Integer(6), 2);
    for &format in SerializationFormat::ALL {
        let bytes = engine.serialize(format).unwrap();
        let mut restored = Engine::deserialize(&bytes, format).unwrap();
        assert!(restored.get_fact(first).unwrap().is_none());
        assert!(restored.get_fact(second).unwrap().is_none());
        let blockers = restored.find_facts("block").unwrap();
        let first = blockers
            .iter()
            .find_map(|(id, fact)| match fact {
                ferric_rules_core::Fact::Ordered(fact)
                    if matches!(fact.fields[1], Value::Integer(1)) =>
                {
                    Some(*id)
                }
                _ => None,
            })
            .unwrap();
        let second = blockers
            .iter()
            .find_map(|(id, fact)| match fact {
                ferric_rules_core::Fact::Ordered(fact)
                    if matches!(fact.fields[1], Value::Integer(2)) =>
                {
                    Some(*id)
                }
                _ => None,
            })
            .unwrap();
        restored.retract(first).unwrap();
        fire(&mut restored, 0);
        restored.retract(second).unwrap();
        fire(&mut restored, 1);
        assert_eq!(restored.find_facts("selected").unwrap().len(), 1);
    }
}
