//! Activation creation chronology, including negative reactivation and restore.
use ferric_rules_core::{ConflictResolutionStrategy, Value};
use ferric_rules_runtime::{Engine, EngineConfig, HaltReason, RunLimit};

const RULES: &str = r#"
(defrule positive (other) => (printout t "P|"))
(defrule negative (seed) (not (block)) => (printout t "N|"))
"#;

fn reactivated(strategy: ConflictResolutionStrategy) -> Engine {
    let mut engine =
        Engine::with_rules_config(RULES, EngineConfig::default().with_strategy(strategy)).unwrap();
    engine.assert_ordered("seed", [] as [Value; 0]).unwrap();
    engine.assert_ordered("other", [] as [Value; 0]).unwrap();
    let blocker = engine.assert_ordered("block", [] as [Value; 0]).unwrap();
    assert_eq!(engine.agenda_len(), 1);
    engine.retract(blocker).unwrap();
    assert_eq!(engine.agenda_len(), 2);
    engine
}

fn verify(mut engine: Engine, expected: &str) {
    let run = engine.run(RunLimit::Unlimited).unwrap();
    assert_eq!(run.rules_fired, 2);
    assert_eq!(run.halt_reason, HaltReason::AgendaEmpty);
    assert!(engine.action_diagnostics().is_empty());
    assert_eq!(engine.get_output("t"), Some(expected));
}

#[test]
fn negative_reactivation_is_new_for_depth_and_breadth() {
    verify(reactivated(ConflictResolutionStrategy::Depth), "N|P|");
    verify(reactivated(ConflictResolutionStrategy::Breadth), "P|N|");
}

#[cfg(feature = "serde")]
#[test]
fn snapshots_preserve_creation_order_and_new_reactivation() {
    use ferric_rules_runtime::SerializationFormat;
    for (strategy, expected) in [
        (ConflictResolutionStrategy::Depth, "N|P|"),
        (ConflictResolutionStrategy::Breadth, "P|N|"),
    ] {
        for &format in SerializationFormat::ALL {
            let engine = reactivated(strategy);
            let bytes = engine.serialize(format).unwrap();
            let mut restored = Engine::deserialize(&bytes, format).unwrap();
            let block = restored.assert_ordered("block", [] as [Value; 0]).unwrap();
            restored.retract(block).unwrap();
            verify(restored, expected);
        }
    }
}
