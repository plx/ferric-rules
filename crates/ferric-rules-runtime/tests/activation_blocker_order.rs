//! CLIPS 6.30 blocker migration order across host mutation and snapshot boundaries.
use ferric_rules_core::{ConflictResolutionStrategy, Fact};
use ferric_rules_runtime::{Engine, EngineConfig, HaltReason, RunLimit, Value};
use std::fmt::Write as _;

const RULE: &str = "(defrule r (item ?x) (not (blocker ?)) => (printout t ?x crlf))";

struct Case {
    name: &'static str,
    // Positive values are items; negative values identify blockers.
    assertions: &'static [i64],
    retractions: &'static [i64],
    depth: &'static [i64],
}

// Captured from the pinned CLIPS 6.30 reference: all removal permutations of
// two and three blockers, asserted before, after, or between three left facts.
const CASES: &[Case] = &[
    Case {
        name: "2-before-ab",
        assertions: &[-1, -2, 1, 2, 3],
        retractions: &[1, 2],
        depth: &[3, 2, 1],
    },
    Case {
        name: "2-before-ba",
        assertions: &[-1, -2, 1, 2, 3],
        retractions: &[2, 1],
        depth: &[1, 2, 3],
    },
    Case {
        name: "2-after-ab",
        assertions: &[1, 2, 3, -1, -2],
        retractions: &[1, 2],
        depth: &[1, 2, 3],
    },
    Case {
        name: "2-after-ba",
        assertions: &[1, 2, 3, -1, -2],
        retractions: &[2, 1],
        depth: &[3, 2, 1],
    },
    Case {
        name: "2-interleaved-ab",
        assertions: &[1, -1, 2, -2, 3],
        retractions: &[1, 2],
        depth: &[3, 2, 1],
    },
    Case {
        name: "2-interleaved-ba",
        assertions: &[1, -1, 2, -2, 3],
        retractions: &[2, 1],
        depth: &[1, 2, 3],
    },
    Case {
        name: "3-before-abc",
        assertions: &[-1, -2, -3, 1, 2, 3],
        retractions: &[1, 2, 3],
        depth: &[1, 2, 3],
    },
    Case {
        name: "3-before-acb",
        assertions: &[-1, -2, -3, 1, 2, 3],
        retractions: &[1, 3, 2],
        depth: &[3, 2, 1],
    },
    Case {
        name: "3-before-bac",
        assertions: &[-1, -2, -3, 1, 2, 3],
        retractions: &[2, 1, 3],
        depth: &[3, 2, 1],
    },
    Case {
        name: "3-before-bca",
        assertions: &[-1, -2, -3, 1, 2, 3],
        retractions: &[2, 3, 1],
        depth: &[1, 2, 3],
    },
    Case {
        name: "3-before-cab",
        assertions: &[-1, -2, -3, 1, 2, 3],
        retractions: &[3, 1, 2],
        depth: &[3, 2, 1],
    },
    Case {
        name: "3-before-cba",
        assertions: &[-1, -2, -3, 1, 2, 3],
        retractions: &[3, 2, 1],
        depth: &[1, 2, 3],
    },
    Case {
        name: "3-after-abc",
        assertions: &[1, 2, 3, -1, -2, -3],
        retractions: &[1, 2, 3],
        depth: &[3, 2, 1],
    },
    Case {
        name: "3-after-acb",
        assertions: &[1, 2, 3, -1, -2, -3],
        retractions: &[1, 3, 2],
        depth: &[1, 2, 3],
    },
    Case {
        name: "3-after-bac",
        assertions: &[1, 2, 3, -1, -2, -3],
        retractions: &[2, 1, 3],
        depth: &[1, 2, 3],
    },
    Case {
        name: "3-after-bca",
        assertions: &[1, 2, 3, -1, -2, -3],
        retractions: &[2, 3, 1],
        depth: &[3, 2, 1],
    },
    Case {
        name: "3-after-cab",
        assertions: &[1, 2, 3, -1, -2, -3],
        retractions: &[3, 1, 2],
        depth: &[1, 2, 3],
    },
    Case {
        name: "3-after-cba",
        assertions: &[1, 2, 3, -1, -2, -3],
        retractions: &[3, 2, 1],
        depth: &[3, 2, 1],
    },
    Case {
        name: "3-interleaved-abc",
        assertions: &[1, -1, 2, -2, 3, -3],
        retractions: &[1, 2, 3],
        depth: &[1, 2, 3],
    },
    Case {
        name: "3-interleaved-acb",
        assertions: &[1, -1, 2, -2, 3, -3],
        retractions: &[1, 3, 2],
        depth: &[3, 2, 1],
    },
    Case {
        name: "3-interleaved-bac",
        assertions: &[1, -1, 2, -2, 3, -3],
        retractions: &[2, 1, 3],
        depth: &[3, 2, 1],
    },
    Case {
        name: "3-interleaved-bca",
        assertions: &[1, -1, 2, -2, 3, -3],
        retractions: &[2, 3, 1],
        depth: &[1, 2, 3],
    },
    Case {
        name: "3-interleaved-cab",
        assertions: &[1, -1, 2, -2, 3, -3],
        retractions: &[3, 1, 2],
        depth: &[3, 2, 1],
    },
    Case {
        name: "3-interleaved-cba",
        assertions: &[1, -1, 2, -2, 3, -3],
        retractions: &[3, 2, 1],
        depth: &[1, 2, 3],
    },
];

fn verify_case(
    case: &Case,
    strategy: ConflictResolutionStrategy,
    checkpoint: impl Fn(&mut Engine),
) {
    let mut engine =
        Engine::with_rules_config(RULE, EngineConfig::default().with_strategy(strategy)).unwrap();
    for &field in case.assertions {
        let (relation, value) = if field < 0 {
            ("blocker", -field)
        } else {
            ("item", field)
        };
        engine.assert_ordered(relation, [value]).unwrap();
    }
    assert_eq!(engine.agenda_len(), 0, "{} {strategy:?}", case.name);
    checkpoint(&mut engine);
    for (index, &target) in case.retractions.iter().enumerate() {
        let blocker = engine
            .find_facts("blocker")
            .unwrap()
            .into_iter()
            .find_map(|(id, fact)| match fact {
                Fact::Ordered(fact) if matches!(fact.fields.first(), Some(Value::Integer(value)) if *value == target) => {
                    Some(id)
                }
                _ => None,
            })
            .unwrap();
        engine.retract(blocker).unwrap();
        assert_eq!(
            engine.agenda_len(),
            if index + 1 == case.retractions.len() {
                3
            } else {
                0
            },
            "{} {strategy:?} removal {target}",
            case.name,
        );
        checkpoint(&mut engine);
    }
    let result = engine.run(RunLimit::Unlimited).unwrap();
    assert_eq!(result.halt_reason, HaltReason::AgendaEmpty);
    assert_eq!(result.rules_fired, 3);
    assert!(engine.action_diagnostics().is_empty());
    let mut order = case.depth.to_vec();
    if strategy == ConflictResolutionStrategy::Breadth {
        order.reverse();
    }
    let mut expected = String::new();
    for value in order {
        writeln!(expected, "{value}").unwrap();
    }
    assert_eq!(
        engine.get_output("t"),
        Some(expected.as_str()),
        "{} {strategy:?}",
        case.name
    );
}

#[test]
fn host_retractions_follow_reference_blocker_migration_order() {
    for case in CASES {
        for strategy in [
            ConflictResolutionStrategy::Depth,
            ConflictResolutionStrategy::Breadth,
        ] {
            verify_case(case, strategy, |_| {});
        }
    }
}

#[cfg(feature = "serde")]
#[test]
fn snapshots_preserve_blocker_migrations_and_pending_activation_order() {
    use ferric_rules_runtime::SerializationFormat;
    for case in CASES {
        for strategy in [
            ConflictResolutionStrategy::Depth,
            ConflictResolutionStrategy::Breadth,
        ] {
            for &format in SerializationFormat::ALL {
                verify_case(case, strategy, |engine| {
                    let bytes = engine.serialize(format).unwrap();
                    *engine = Engine::deserialize(&bytes, format).unwrap();
                });
            }
        }
    }
}
