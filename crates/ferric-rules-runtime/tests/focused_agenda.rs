//! Focus dispatch must preserve priority and dormant matches across snapshots.

use std::fmt::Write as _;

use ferric_rules_core::ConflictResolutionStrategy;
use ferric_rules_runtime::{Engine, EngineConfig, HaltReason, RunLimit};

fn prepare(strategy: ConflictResolutionStrategy, count: usize) -> Engine {
    let mut source = String::from(
        "(defmodule MAIN (export ?ALL))\n\
         (deftemplate MAIN::item (slot id))\n\
         (deffacts MAIN::seed\n",
    );
    for id in 0..count {
        writeln!(source, "(item (id {id}))").unwrap();
    }
    source.push_str(")\n\
        (defmodule DORMANT (import MAIN ?ALL))\n\
        (defmodule ACTIVE (import MAIN ?ALL))\n\
        (defrule DORMANT::wait (declare (salience 100)) (MAIN::item (id ?id)) => (printout t d ?id crlf))\n\
        (defrule ACTIVE::work (MAIN::item (id ?id)) => (printout t ?id crlf))\n\
        (defrule MAIN::start => (focus ACTIVE))\n\
        (defrule MAIN::wake (go) => (focus DORMANT))\n");
    let mut config = EngineConfig::utf8();
    config.strategy = strategy;
    let mut engine = Engine::new(config);
    engine.load_str(&source).unwrap();
    engine.reset().unwrap();
    engine
}

fn expected(strategy: ConflictResolutionStrategy, count: usize, prefix: &str) -> String {
    let mut ids: Vec<_> = (0..count).collect();
    if strategy != ConflictResolutionStrategy::Breadth {
        ids.reverse();
    }
    let mut output = String::new();
    for id in ids {
        writeln!(output, "{prefix}{id}").unwrap();
    }
    output
}

fn finish(engine: &mut Engine, strategy: ConflictResolutionStrategy, count: usize) {
    let result = engine.run(RunLimit::Unlimited).unwrap();
    assert_eq!(result.halt_reason, HaltReason::AgendaEmpty);
    assert_eq!(result.rules_fired, count + 1 - 5);
    let active = expected(strategy, count, "");
    assert_eq!(engine.get_output("t"), Some(active.as_str()));
    assert_eq!(engine.agenda_len(), count);
    assert_eq!(engine.run(RunLimit::Unlimited).unwrap().rules_fired, 0);
    engine.debug_assert_consistency();

    engine.assert_ordered("go", ()).unwrap();
    let result = engine.run(RunLimit::Unlimited).unwrap();
    assert_eq!(result.halt_reason, HaltReason::AgendaEmpty);
    assert_eq!(result.rules_fired, count + 1);
    let complete = active + &expected(strategy, count, "d");
    assert_eq!(engine.get_output("t"), Some(complete.as_str()));
    assert_eq!(engine.agenda_len(), 0);
    engine.debug_assert_consistency();
}

const STRATEGIES: [ConflictResolutionStrategy; 4] = [
    ConflictResolutionStrategy::Depth,
    ConflictResolutionStrategy::Breadth,
    ConflictResolutionStrategy::Lex,
    ConflictResolutionStrategy::Mea,
];

#[test]
fn dormant_matches_survive_partial_runs_and_later_focus_changes() {
    for strategy in STRATEGIES {
        let mut engine = prepare(strategy, 32);
        let partial = engine.run(RunLimit::Count(5)).unwrap();
        assert_eq!(partial.rules_fired, 5);
        assert_eq!(partial.halt_reason, HaltReason::LimitReached);
        finish(&mut engine, strategy, 32);
    }
}

#[cfg(feature = "serde")]
#[test]
fn focused_priority_and_dormant_matches_resume_in_every_snapshot_format() {
    use ferric_rules_runtime::SerializationFormat;
    for strategy in STRATEGIES {
        let mut original = prepare(strategy, 32);
        assert_eq!(original.run(RunLimit::Count(5)).unwrap().rules_fired, 5);
        for &format in SerializationFormat::ALL {
            let bytes = original.serialize(format).unwrap();
            let mut resumed = Engine::deserialize(&bytes, format).unwrap();
            finish(&mut resumed, strategy, 32);
        }
    }
}
