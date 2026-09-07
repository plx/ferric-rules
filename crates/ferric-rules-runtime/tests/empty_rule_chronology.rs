use ferric_rules_core::ConflictResolutionStrategy;
use ferric_rules_runtime::{Engine, EngineConfig, RunLimit};

#[test]
fn reset_empty_lhs_chronology_matches_pinned_clips() {
    for (strategy, expected) in [
        (ConflictResolutionStrategy::Depth, "old\nnew\n"),
        (ConflictResolutionStrategy::Breadth, "new\nold\n"),
    ] {
        let mut config = EngineConfig::default();
        config.strategy = strategy;
        let mut engine = Engine::with_rules_config(
            "(defrule old => (printout t old crlf)) (defrule new => (printout t new crlf))",
            config,
        )
        .unwrap();
        for _ in 0..2 {
            engine.reset().unwrap();
            assert_eq!(engine.run(RunLimit::Unlimited).unwrap().rules_fired, 2);
            assert_eq!(engine.get_output("t"), Some(expected));
        }
    }
}
