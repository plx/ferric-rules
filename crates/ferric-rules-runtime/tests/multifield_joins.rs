//! Typed, structurally equal multifields must work as ordinary join values.

use ferric_rules_runtime::{Engine, HaltReason, Multifield, RunLimit, Value};

fn fields(values: impl IntoIterator<Item = Value>) -> Value {
    Value::Multifield(Box::new(values.into_iter().collect::<Multifield>()))
}

fn nested_key(engine: &Engine, id: i64) -> Value {
    fields([
        Value::Integer(id),
        Value::String(engine.create_string("shared-key").unwrap()),
        fields([Value::Integer(1), fields([Value::Integer(2)])]),
    ])
}

fn run(engine: &mut Engine, expected: usize) {
    let result = engine.run(RunLimit::Unlimited).unwrap();
    assert_eq!(result.rules_fired, expected);
    assert_eq!(
        result.halt_reason,
        HaltReason::AgendaEmpty,
        "{:?}",
        engine.action_diagnostics()
    );
    assert!(engine.action_diagnostics().is_empty());
    #[cfg(debug_assertions)]
    engine.debug_assert_consistency();
}

#[test]
fn clips_multislot_join_matches_equal_field_types_and_values() {
    let mut engine = Engine::with_rules(include_str!("fixtures/multifield_join.clp")).unwrap();
    run(&mut engine, 2);
    assert_eq!(engine.get_output("t").unwrap(), Some("matched 2\n"));
    assert_eq!(engine.find_facts("matched").unwrap().len(), 1);
}

#[test]
fn nested_keys_match_from_both_join_directions_above_index_threshold() {
    for size in [4, 20] {
        for relation_order in [["left", "right"], ["right", "left"]] {
            let mut engine = Engine::with_rules(
                "(defrule pair (left ?key) (right ?key) => (assert (matched ?key)))",
            )
            .unwrap();
            for relation in relation_order {
                for id in 0..size {
                    let key = nested_key(&engine, id);
                    engine.assert_ordered(relation, vec![key]).unwrap();
                }
            }
            run(&mut engine, usize::try_from(size).unwrap());
            assert_eq!(
                engine.find_facts("matched").unwrap().len(),
                usize::try_from(size).unwrap()
            );
        }
    }
}

#[test]
fn multifield_inequality_distinguishes_nested_contents_and_scalar_values() {
    let mut engine = Engine::with_rules(
        "(defrule different (left ?key) (right ?other&~?key) => (assert (different ?other)))",
    )
    .unwrap();
    let key = nested_key(&engine, 1);
    engine.assert_ordered("left", vec![key.clone()]).unwrap();
    engine.assert_ordered("right", vec![key]).unwrap();
    run(&mut engine, 0);
    let different = nested_key(&engine, 2);
    engine.assert_ordered("right", vec![different]).unwrap();
    engine
        .assert_ordered("right", vec![Value::Integer(1)])
        .unwrap();
    run(&mut engine, 2);
    assert_eq!(engine.find_facts("different").unwrap().len(), 2);
}

#[test]
fn nested_key_negative_join_tracks_every_blocker() {
    for blockers_first in [false, true] {
        let mut engine = Engine::with_rules(
            "(defrule select (candidate ?key) (not (block ?key ?reason)) => (assert (selected)))",
        )
        .unwrap();
        let key = nested_key(&engine, 7);
        if !blockers_first {
            engine
                .assert_ordered("candidate", vec![key.clone()])
                .unwrap();
        }
        let first = engine
            .assert_ordered("block", vec![key.clone(), Value::Integer(1)])
            .unwrap();
        let second = engine
            .assert_ordered("block", vec![key.clone(), Value::Integer(2)])
            .unwrap();
        if blockers_first {
            engine.assert_ordered("candidate", vec![key]).unwrap();
        }
        run(&mut engine, 0);
        engine.retract(first).unwrap();
        run(&mut engine, 0);
        engine.retract(second).unwrap();
        run(&mut engine, 1);
    }
}

#[cfg(feature = "serde")]
#[test]
fn restored_nested_key_can_match_new_independent_values() {
    use ferric_rules_runtime::SerializationFormat;

    let mut engine =
        Engine::with_rules("(defrule pair (left ?key) (right ?key) => (assert (matched)))")
            .unwrap();
    let key = nested_key(&engine, 8);
    engine.assert_ordered("left", vec![key]).unwrap();
    for &format in SerializationFormat::ALL {
        let bytes = engine.serialize(format).unwrap();
        let mut restored = Engine::deserialize(&bytes, format).unwrap();
        let new_key = nested_key(&restored, 8);
        restored.assert_ordered("right", vec![new_key]).unwrap();
        run(&mut restored, 1);
        assert_eq!(restored.find_facts("matched").unwrap().len(), 1);
    }
}
