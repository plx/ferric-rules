//! Pinned CLIPS traversal order for matches created by one incoming join fact.
use ferric_rules_core::ConflictResolutionStrategy as Strategy;
use ferric_rules_runtime::{Engine, EngineConfig, RunLimit, Value};
use std::fmt::Write as _;

fn source(gate_first: bool) -> &'static str {
    if gate_first {
        "(defrule joined (gate ?key) (item ?key ?id) => (printout t ?id \" \"))"
    } else {
        "(defrule joined (item ?key ?id) (gate ?key) => (printout t ?id \" \"))"
    }
}

fn prepared(gate_first: bool, size: i64, strategy: Strategy) -> Engine {
    let mut config = EngineConfig::utf8();
    config.strategy = strategy;
    let mut engine = Engine::new(config);
    engine.load_str(source(gate_first)).unwrap();
    for id in 0..size {
        engine
            .assert_ordered("item", vec![Value::Integer(1), Value::Integer(id)])
            .unwrap();
    }
    engine
}

fn expected_output(ids: &[i64], gate_first: bool, strategy: Strategy) -> String {
    let mut ids = ids.to_vec();
    if gate_first == (strategy == Strategy::Depth) {
        ids.reverse();
    }
    ids.into_iter().fold(String::new(), |mut output, id| {
        write!(&mut output, "{id} ").unwrap();
        output
    })
}

fn fire_gate(engine: &mut Engine, expected: &str, count: usize) {
    let gate = engine
        .assert_ordered("gate", vec![Value::Integer(1)])
        .unwrap();
    let result = engine.run(RunLimit::Unlimited).unwrap();
    assert_eq!(result.rules_fired, count);
    assert!(engine.action_diagnostics().is_empty());
    assert_eq!(engine.get_output("t").unwrap(), Some(expected));
    engine.clear_output_channel("t");
    engine.retract(gate).unwrap();
    #[cfg(debug_assertions)]
    engine.debug_assert_consistency();
}

#[test]
fn both_join_directions_preserve_order_below_and_above_the_index_threshold() {
    for size in [3, 32] {
        for gate_first in [false, true] {
            for strategy in [Strategy::Depth, Strategy::Breadth] {
                let mut engine = prepared(gate_first, size, strategy);
                let ids: Vec<_> = (0..size).collect();
                let expected = expected_output(&ids, gate_first, strategy);
                for _ in 0..3 {
                    fire_gate(&mut engine, &expected, usize::try_from(size).unwrap());
                }
            }
        }
    }
}

#[test]
fn removed_and_reused_token_slots_keep_creation_order_when_an_index_is_added_later() {
    let mut engine = Engine::new(EngineConfig::utf8());
    engine
        .load_str("(defrule hold (item ?key ?id) =>)")
        .unwrap();
    let mut facts = Vec::new();
    for id in 0..32 {
        facts.push(
            engine
                .assert_ordered("item", vec![Value::Integer(1), Value::Integer(id)])
                .unwrap(),
        );
    }
    engine.retract(facts[5]).unwrap();
    engine
        .assert_ordered("item", vec![Value::Integer(1), Value::Integer(100)])
        .unwrap();
    assert_eq!(engine.run(RunLimit::Unlimited).unwrap().rules_fired, 32);
    engine.load_str(source(false)).unwrap();
    let ids: Vec<_> = (0..32).filter(|id| *id != 5).chain([100]).collect();
    fire_gate(
        &mut engine,
        &expected_output(&ids, false, Strategy::Depth),
        32,
    );
}

#[cfg(feature = "serde")]
#[test]
fn snapshots_preserve_join_order_after_removal_and_reinsertion() {
    use ferric_rules_runtime::SerializationFormat;
    for gate_first in [false, true] {
        let mut engine = prepared(gate_first, 32, Strategy::Depth);
        let id = engine.find_facts("item").unwrap().into_iter().find_map(|(id, fact)| {
            matches!(fact, ferric_rules_core::Fact::Ordered(fact) if matches!(fact.fields.as_slice(), [_, Value::Integer(5)])).then_some(id)
        }).unwrap();
        engine.retract(id).unwrap();
        engine
            .assert_ordered("item", vec![Value::Integer(1), Value::Integer(100)])
            .unwrap();
        let ids: Vec<_> = (0..32).filter(|id| *id != 5).chain([100]).collect();
        for &format in SerializationFormat::ALL {
            let mut restored =
                Engine::deserialize(&engine.serialize(format).unwrap(), format).unwrap();
            fire_gate(
                &mut restored,
                &expected_output(&ids, gate_first, Strategy::Depth),
                32,
            );
        }
    }
}

#[test]
fn joined_predicate_matches_follow_the_same_creation_order() {
    let mut engine = Engine::with_rules(
        "(deffacts seed (quote a 10 2) (quote b 8 5) (quote c 3 1) (budget 15))
         (defrule affordable (quote ?id ?price ?discount) (budget ?budget)
         (test (and (> ?price 5) (<= (- ?price ?discount) ?budget)))
         => (printout t ?id \" \" (- ?price ?discount) crlf))",
    )
    .unwrap();
    assert_eq!(engine.run(RunLimit::Unlimited).unwrap().rules_fired, 2);
    assert_eq!(engine.get_output("t").unwrap(), Some("a 8\nb 3\n"));
}
