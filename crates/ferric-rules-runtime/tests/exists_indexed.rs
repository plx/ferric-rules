//! Right-side existential joins must preserve order and every support witness.
use std::fmt::Write as _;

use ferric_rules_runtime::{Engine, FerricString, HaltReason, RunLimit, Value};

fn key(bucket: i64, kind: usize) -> Value {
    match kind {
        0 => Value::Integer(bucket),
        1 => Value::String(FerricString::Utf8(format!("key-{bucket}").into())),
        _ => Value::Multifield(Box::new([Value::Integer(bucket)].into_iter().collect())),
    }
}

fn fire(engine: &mut Engine, expected: usize) {
    let result = engine.run(RunLimit::Unlimited).unwrap();
    assert_eq!(result.halt_reason, HaltReason::AgendaEmpty);
    assert_eq!(result.rules_fired, expected);
    assert!(engine.action_diagnostics().is_empty());
    engine.rete().validate_consistency().unwrap();
}

#[test]
fn existential_support_preserves_order_across_indexing_and_backfill() {
    const RULE: &str = "(defrule selected (candidate ?id ?key) (exists (support ?key ?tag)) => (printout t ?id \" \"))";
    for size in [8, 32] {
        for online in [false, true] {
            for kind in 0..3 {
                let mut engine =
                    Engine::with_rules("(defrule keep (candidate ?id ?key) =>)").unwrap();
                if !online {
                    engine.load_str(RULE).unwrap();
                }
                for id in 0..size {
                    engine
                        .assert_ordered("candidate", vec![Value::Integer(id), key(id % 3, kind)])
                        .unwrap();
                }
                fire(&mut engine, usize::try_from(size).unwrap());
                if online {
                    // Attach the existential node to a populated, shared prefix.
                    engine.load_str(RULE).unwrap();
                }
                let mut expected = String::new();
                for id in (0..size).filter(|id| id % 3 == 1) {
                    write!(expected, "{id} ").unwrap();
                }
                let count = (0..size).filter(|id| id % 3 == 1).count();
                let first = engine
                    .assert_ordered("support", vec![key(1, kind), Value::Integer(1)])
                    .unwrap();
                let second = engine
                    .assert_ordered("support", vec![key(1, kind), Value::Integer(2)])
                    .unwrap();
                // Removing one witness before firing must retain exactly one
                // activation per matching parent, in the original traversal order.
                engine.retract(first).unwrap();
                fire(&mut engine, count);
                assert_eq!(engine.get_output("t").unwrap(), Some(expected.as_str()));
                engine.retract(second).unwrap();
                fire(&mut engine, 0);
                engine
                    .assert_ordered("support", vec![key(1, kind), Value::Integer(3)])
                    .unwrap();
                fire(&mut engine, count);
                assert_eq!(
                    engine.get_output("t").unwrap(),
                    Some(expected.repeat(2).as_str())
                );
            }
        }
    }
}
