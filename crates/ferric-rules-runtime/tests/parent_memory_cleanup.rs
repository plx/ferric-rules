//! Retired parents must disappear from all kinds of child bookkeeping.

use ferric_rules_runtime::{Engine, RunLimit};

const RULES: &str = "
(defrule negative (item ?i) (not (block ?i)) => (assert (seen negative ?i)))
(defrule existential (item ?i) (exists (support ?i)) => (assert (seen exists ?i)))
(defrule conjunctive (item ?i) (not (and (left ?i) (right ?i)))
    => (assert (seen ncc ?i)))";

fn run(engine: &mut Engine, expected: usize) {
    let result = engine.run(RunLimit::Unlimited).unwrap();
    assert_eq!(result.rules_fired, expected);
    assert!(engine.action_diagnostics().is_empty());
    engine.rete().validate_consistency().unwrap();
}

#[test]
fn retracted_shared_parents_do_not_resurrect_after_support_changes() {
    for online in [false, true] {
        for blocked in [false, true] {
            let mut source = String::from("(defrule keep (item ?i) =>)");
            if !online {
                source.push_str(RULES);
            }
            source.push_str("(deffacts seed (item 0) (support 0)");
            if blocked {
                source.push_str(" (block 0) (left 0) (right 0)");
            }
            source.push(')');
            let mut engine = Engine::with_rules(&source).unwrap();
            if online {
                engine.load_str(RULES).unwrap();
            }
            run(&mut engine, if blocked { 2 } else { 4 });
            let item = engine.find_facts("item").unwrap()[0].0;
            engine.retract(item).unwrap();
            run(&mut engine, 0);

            for relation in ["support", "block", "left", "right"] {
                let handles: Vec<_> = engine
                    .find_facts(relation)
                    .unwrap()
                    .into_iter()
                    .map(|(id, _)| id)
                    .collect();
                for handle in handles {
                    engine.retract(handle).unwrap();
                }
                run(&mut engine, 0);
            }
            engine.assert_ordered("item", 0_i64).unwrap();
            run(&mut engine, 3); // keep, negative, and NCC; exists has no support.
            engine.assert_ordered("support", 0_i64).unwrap();
            run(&mut engine, 1);
        }
    }
}

#[cfg(feature = "serde")]
#[test]
fn snapshots_resume_after_the_last_indexed_parent_is_retracted() {
    let mut engine = Engine::with_rules(
        "(defrule joined (item ?i) (support ?i) => (assert (seen ?i)))
         (deffacts seed (item 7) (support 7))",
    )
    .unwrap();
    run(&mut engine, 1);
    let item = engine.find_facts("item").unwrap()[0].0;
    engine.retract(item).unwrap();
    run(&mut engine, 0);
    for &format in ferric_rules_runtime::SerializationFormat::ALL {
        let mut restored = Engine::deserialize(&engine.serialize(format).unwrap(), format).unwrap();
        assert!(restored.find_facts("item").unwrap().is_empty());
        run(&mut restored, 0);
        restored.assert_ordered("item", 7_i64).unwrap();
        run(&mut restored, 1);
    }
}
