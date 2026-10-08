//! CLIPS 6.30 ordering through reset, replacement, and snapshot checkpoints.

use ferric_rules_core::{ConflictResolutionStrategy, Fact};
#[cfg(feature = "serde")]
use ferric_rules_runtime::SerializationFormat;
use ferric_rules_runtime::{Engine, EngineConfig, RunLimit, Value};

#[derive(Clone, Copy)]
enum Checkpoint {
    Live,
    #[cfg(feature = "serde")]
    Snapshot(SerializationFormat),
}

const CHECKPOINTS: &[Checkpoint] = &[
    Checkpoint::Live,
    #[cfg(feature = "serde")]
    Checkpoint::Snapshot(SerializationFormat::Cbor),
    #[cfg(feature = "serde")]
    Checkpoint::Snapshot(SerializationFormat::Json),
];

const STRATEGIES: [ConflictResolutionStrategy; 2] = [
    ConflictResolutionStrategy::Lex,
    ConflictResolutionStrategy::Mea,
];

impl Checkpoint {
    fn apply(self, engine: &mut Engine) {
        assert!(engine.action_diagnostics().is_empty());
        match self {
            Self::Live => {}
            #[cfg(feature = "serde")]
            Self::Snapshot(format) => {
                let bytes = engine.serialize(format).unwrap();
                *engine = Engine::deserialize(&bytes, format).unwrap();
            }
        }
        #[cfg(debug_assertions)]
        engine.debug_assert_consistency();
    }
}

fn expect(engine: &mut Engine, checkpoint: Checkpoint, expected: &str) {
    engine.clear_output_channel("t");
    // Persist both the initial queue and every intermediate firing: restored
    // tie-breaking must preserve the original activation chronology.
    for _ in expected.lines() {
        checkpoint.apply(engine);
        assert_eq!(engine.run(RunLimit::Count(1)).unwrap().rules_fired, 1);
    }
    checkpoint.apply(engine);
    assert_eq!(engine.run(RunLimit::Unlimited).unwrap().rules_fired, 0);
    assert!(engine.action_diagnostics().is_empty());
    assert_eq!(engine.get_output("t"), Some(expected));
}

#[test]
fn reset_and_recreated_supports_keep_fact_chronology() {
    for strategy in STRATEGIES {
        for &checkpoint in CHECKPOINTS {
            let mut engine = Engine::with_rules_config(
                "(deffacts seed (item 1) (item 2))
                 (defrule show (item ?x) => (printout t ?x crlf))",
                EngineConfig::default().with_strategy(strategy),
            )
            .unwrap();
            expect(&mut engine, checkpoint, "2\n1\n");
            engine.reset().unwrap();
            let first = engine
                .find_facts("item")
                .unwrap()
                .into_iter()
                .find_map(|(id, fact)| {
                    matches!(fact, Fact::Ordered(fact)
                        if matches!(fact.fields.as_slice(), [Value::Integer(1)]))
                    .then_some(id)
                })
                .unwrap();
            engine.retract(first).unwrap();
            engine.assert_ordered("item", 1_i64).unwrap();
            expect(&mut engine, checkpoint, "1\n2\n");
            engine.reset().unwrap();
            expect(&mut engine, checkpoint, "2\n1\n");
        }
    }
}

#[test]
fn removing_and_replacing_or_variants_retains_only_live_activations() {
    for strategy in STRATEGIES {
        for &checkpoint in CHECKPOINTS {
            let mut engine = Engine::with_rules_config(
                "(deffacts seed (a 1) (b 2) (c 3))
                 (defrule choice (or (a ?x) (b ?x)) => (printout t \"old:\" ?x crlf))
                 (defrule peer (b ?x) => (printout t \"peer:\" ?x crlf))",
                EngineConfig::default().with_strategy(strategy),
            )
            .unwrap();
            expect(&mut engine, checkpoint, "peer:2\nold:2\nold:1\n");
            engine.reset().unwrap();
            engine
                .load_str(
                    "(defrule remove-choice (declare (salience 100))
                       ?request <- (remove-choice)
                       => (retract ?request) (undefrule choice))
                     (assert (remove-choice))",
                )
                .unwrap();
            checkpoint.apply(&mut engine);
            assert_eq!(engine.run(RunLimit::Count(1)).unwrap().rules_fired, 1);
            assert_eq!(engine.rules().len(), 2);
            expect(&mut engine, checkpoint, "peer:2\n");
            engine
                .load_str(
                    "(defrule choice (or (b ?x) (c ?x))
                       => (printout t \"new:\" ?x crlf))",
                )
                .unwrap();
            assert_eq!(engine.rules().len(), 4);
            expect(&mut engine, checkpoint, "new:3\nnew:2\n");
            engine.reset().unwrap();
            engine
                .load_str(
                    "(defrule choice (or (a ?x) (c ?x))
                       => (printout t \"replaced:\" ?x crlf))",
                )
                .unwrap();
            assert_eq!(engine.rules().len(), 4);
            expect(&mut engine, checkpoint, "replaced:3\npeer:2\nreplaced:1\n");
        }
    }
}

#[test]
fn first_real_fact_after_clear_outranks_absence_and_preserves_zero_suffixes() {
    for strategy in STRATEGIES {
        for &checkpoint in CHECKPOINTS {
            let mut engine = Engine::new(EngineConfig::default().with_strategy(strategy));
            engine.clear();
            engine
                .load_str(
                    "(defrule absent (not (missing)) (not (other-missing))
                       => (printout t ABSENT crlf))
                     (defrule plain (first) => (printout t PLAIN crlf))
                     (defrule longer (first) (not (missing))
                       => (printout t LONGER crlf))",
                )
                .unwrap();
            assert_eq!(engine.fact_count(), 0);
            engine.assert_ordered("first", ()).unwrap();
            expect(&mut engine, checkpoint, "LONGER\nPLAIN\nABSENT\n");
        }
    }
}
