//! Issue #321: every valid ordered multifield partition is a distinct match.
//!
//! Fixture stdout was checked with CLIPS 6.30 (3/17/15), image
//! sha256:b4b99ba2f08102f9c18743c6adaecb5ab82789ddfa8628693cabfeced461a929.

use ferric_rules::core::ConflictResolutionStrategy;
use ferric_rules::runtime::{Engine, EngineConfig, HaltReason, RunLimit};

fn check(source: &str, expected: &str, late_rules: bool) {
    check_strategy(
        source,
        expected,
        late_rules,
        ConflictResolutionStrategy::Depth,
    );
}

fn check_strategy(
    source: &str,
    expected: &str,
    late_rules: bool,
    strategy: ConflictResolutionStrategy,
) {
    let mut engine = Engine::new(EngineConfig::utf8().with_strategy(strategy));
    if late_rules {
        // Existing facts must backfill every split when a rule is loaded online.
        let rules_start = source.find("(defrule").expect("fixture has rules");
        engine.load_str(&source[..rules_start]).unwrap();
        engine.reset().unwrap();
        engine.load_str(&source[rules_start..]).unwrap();
    } else {
        engine.load_str(source).unwrap();
        engine.reset().unwrap();
    }

    #[cfg(feature = "serde")]
    for &format in ferric_rules::runtime::SerializationFormat::ALL {
        let bytes = engine.serialize(format).unwrap();
        let mut restored = Engine::deserialize(&bytes, format).unwrap();
        assert_output(&mut restored, expected);
    }
    assert_output(&mut engine, expected);
}

fn assert_output(engine: &mut Engine, expected: &str) {
    let result = engine.run(RunLimit::Count(1_000)).unwrap();
    assert_eq!(result.halt_reason, HaltReason::AgendaEmpty);
    assert_eq!(engine.get_output("t").unwrap_or(""), expected);
    assert!(
        engine.action_diagnostics().is_empty(),
        "{:?}",
        engine.action_diagnostics()
    );
}

macro_rules! fixture {
    ($name:ident) => {
        #[test]
        fn $name() {
            let source = include_str!(concat!("fixtures/core/", stringify!($name), ".clp"));
            let expected = include_str!(concat!("fixtures/core/", stringify!($name), ".out"));
            check(source, expected, false);
            check(source, expected, true);
        }
    };
}

fixture!(ordered_multifield_empty_capture);
fixture!(ordered_multifield_suffix_capture);
fixture!(ordered_multifield_middle_capture);
fixture!(ordered_multifield_prefix_capture);
fixture!(ordered_multifield_middle_empty);
fixture!(ordered_multifield_split);
fixture!(ordered_multifield_repeated_delimiters);
fixture!(ordered_multifield_three_captures);
fixture!(ordered_multifield_capture_joins);
fixture!(ordered_multifield_indexed_joins);
fixture!(ordered_multifield_retract_splits);
fixture!(ordered_multifield_negative_transitions);

#[test]
fn ordered_multifield_refraction_survives_incremental_runs() {
    let source = include_str!("fixtures/core/ordered_multifield_repeated_delimiters.clp");
    let expected = include_str!("fixtures/core/ordered_multifield_repeated_delimiters.out");
    let mut engine = Engine::new(EngineConfig::utf8());
    engine.load_str(source).unwrap();
    engine.reset().unwrap();

    // The fixture has eleven partition activations and one summary. Pausing
    // after every firing checks that one split never consumes another's match.
    let mut firings = 0;
    loop {
        let result = engine.run(RunLimit::Count(1)).unwrap();
        if result.halt_reason == HaltReason::AgendaEmpty {
            break;
        }
        firings += 1;
        assert!(
            firings <= 12,
            "a previously fired split became eligible again"
        );
        #[cfg(feature = "serde")]
        for &format in ferric_rules::runtime::SerializationFormat::ALL {
            let bytes = engine.serialize(format).unwrap();
            let mut restored = Engine::deserialize(&bytes, format).unwrap();
            assert_output(&mut restored, expected);
        }
    }
    assert_output(&mut engine, expected);
}

fn check_split_order(strategy: ConflictResolutionStrategy, expected: &str) {
    let source = include_str!("fixtures/core/ordered_multifield_split_order.clp");
    check_strategy(source, expected, false, strategy);
    check_strategy(source, expected, true, strategy);
}

#[test]
fn ordered_multifield_split_order_depth() {
    check_split_order(
        ConflictResolutionStrategy::Depth,
        include_str!("fixtures/core/ordered_multifield_split_order.out"),
    );
}

#[test]
fn ordered_multifield_split_order_breadth() {
    check_split_order(
        ConflictResolutionStrategy::Breadth,
        include_str!("fixtures/core/ordered_multifield_split_order.breadth.out"),
    );
}

#[test]
fn experimental_lex_mea_split_order_characterization() {
    let ferric = include_str!("fixtures/core/ordered_multifield_split_order.out");
    let clips = include_str!("fixtures/core/ordered_multifield_split_order.breadth.out");
    // Existing #155: Ferric's experimental LEX/MEA strategies use newest-first
    // activation sequence to break this equal-recency tie. Pinned CLIPS uses
    // the opposite order, matching breadth for these two partitions. This is
    // a characterization of the documented strategy gap, not CLIPS parity.
    assert_ne!(ferric, clips);
    for strategy in [
        ConflictResolutionStrategy::Lex,
        ConflictResolutionStrategy::Mea,
    ] {
        check_split_order(strategy, ferric);
    }
}

mod fixed_prefix_indexing {
    //! PR #352: fixed-prefix sequence indexes preserve full matching semantics.
    //!
    //! Twenty-four distinct keys exercise each arrival direction above the existing
    //! index threshold. Key selectors occupy physical prefix slots zero and one;
    //! later marker/constant constraints and TEST conditions still filter splits.
    //! This suite checks results and lifecycle behavior, without timing assertions.

    use ferric_rules::core::{Fact, Value};
    use ferric_rules::runtime::{Engine, EngineConfig, FactHandle, HaltReason, RunLimit};

    const SOURCE: &str = include_str!("fixtures/core/ordered_sequence_prefix_indexing.clp");
    const OUTPUT: &str = include_str!("fixtures/core/ordered_sequence_prefix_indexing.out");
    const GLOBALS: [&str; 12] = [
        "right-empty",
        "right-tail",
        "left-empty",
        "left-tail",
        "right-splits",
        "right-prefix",
        "right-suffix",
        "left-splits",
        "left-prefix",
        "left-suffix",
        "right-selected",
        "left-selected",
    ];
    const COMPLETE: [i64; 12] = [24, 24, 24, 24, 72, 48, 96, 72, 48, 96, 24, 24];
    const AFTER_REMOVAL: [i64; 12] = [23, 24, 24, 24, 70, 46, 92, 72, 48, 96, 23, 24];
    const AFTER_NEW_KEY: [i64; 12] = [24, 24, 25, 25, 72, 48, 96, 75, 50, 100, 24, 25];
    const REMOVED_OUTPUT: &str = "tails:23:24:24:24\nsplits:70:46:92:72:48:96\nselected:23:24\n";
    const REPEATED_ROW: [i64; 7] = [900, 7, 207, 7, 207, 8, 999];

    fn pending(late_rules: bool) -> Engine {
        let mut engine = Engine::new(EngineConfig::utf8());
        if late_rules {
            let (prefix, suffix) = SOURCE.split_once("(defrule").unwrap();
            engine.load_str(prefix).unwrap();
            engine.reset().unwrap();
            engine.load_str(&format!("(defrule{suffix}")).unwrap();
        } else {
            engine.load_str(SOURCE).unwrap();
            engine.reset().unwrap();
        }
        engine
    }

    fn assert_counts(engine: &Engine, expected: &[i64; 12]) {
        for (name, expected) in GLOBALS.into_iter().zip(expected) {
            assert!(
                matches!(engine.get_global(name), Some(Value::Integer(value)) if value == expected),
                "{name}: {:?}, expected {expected}",
                engine.get_global(name)
            );
        }
    }

    fn run_and_check(engine: &mut Engine, firings: usize, expected: &[i64; 12]) {
        let run = engine.run(RunLimit::Count(1_000)).unwrap();
        assert_eq!(run.halt_reason, HaltReason::AgendaEmpty);
        assert_eq!(run.rules_fired, firings);
        assert!(
            engine.action_diagnostics().is_empty(),
            "{:?}",
            engine.action_diagnostics()
        );
        assert_counts(engine, expected);
        assert_eq!(engine.run(RunLimit::Count(1_000)).unwrap().rules_fired, 0);
    }

    fn fact_handle(engine: &Engine, relation: &str, expected: &[i64]) -> FactHandle {
        engine
            .find_facts(relation)
            .unwrap()
            .into_iter()
            .find_map(|(handle, fact)| match fact {
                Fact::Ordered(row)
                    if row.fields.len() == expected.len()
                        && row.fields.iter().zip(expected).all(
                            |(field, expected)| matches!(field, Value::Integer(value) if value == expected),
                        ) =>
                {
                    Some(handle)
                }
                _ => None,
            })
            .unwrap_or_else(|| panic!("missing {relation} {expected:?}"))
    }

    fn remove_pending_rows(engine: &mut Engine) {
        let empty = fact_handle(engine, "tail-after", &[3]);
        let repeated = fact_handle(engine, "split-after", &REPEATED_ROW);
        engine.retract(empty).unwrap();
        engine.retract(repeated).unwrap();
    }

    fn finish_lifecycle(engine: &mut Engine) {
        engine
            .assert_ordered("tail-after", vec![Value::Integer(3)])
            .unwrap();
        engine
            .assert_ordered(
                "split-after",
                REPEATED_ROW
                    .into_iter()
                    .map(Value::Integer)
                    .collect::<Vec<_>>(),
            )
            .unwrap();
        // Restoring the empty tail yields one match; the repeated row recreates
        // both partitions and the descendant that passes the later TEST filters.
        run_and_check(engine, 4, &COMPLETE);
        assert_eq!(engine.get_output("t").unwrap_or(""), REMOVED_OUTPUT);

        let key = fact_handle(engine, "key-after", &[7, 207]);
        engine.retract(key).unwrap();
        engine
            .assert_ordered("key-after", vec![Value::Integer(7), Value::Integer(207)])
            .unwrap();
        // A new parent recreates two tails, three partitions and one filtered
        // descendant against already resident sequence facts.
        run_and_check(engine, 6, &AFTER_NEW_KEY);
        assert_eq!(engine.get_output("t").unwrap_or(""), REMOVED_OUTPUT);
    }

    #[test]
    fn fixed_prefix_indexes_preserve_both_arrivals_all_splits_and_later_constraints() {
        for late_rules in [false, true] {
            let mut engine = pending(late_rules);
            run_and_check(&mut engine, 289, &COMPLETE);
            assert_eq!(engine.get_output("t").unwrap_or(""), OUTPUT);
        }
    }

    #[test]
    fn retraction_and_reassertion_update_prefix_candidates_and_all_descendants() {
        for late_rules in [false, true] {
            let mut engine = pending(late_rules);
            remove_pending_rows(&mut engine);
            run_and_check(&mut engine, 285, &AFTER_REMOVAL);
            assert_eq!(engine.get_output("t").unwrap_or(""), REMOVED_OUTPUT);
            finish_lifecycle(&mut engine);
        }
    }

    #[cfg(feature = "serde")]
    #[test]
    fn indexed_sequence_matches_restore_in_all_formats() {
        use ferric_rules::runtime::SerializationFormat;
        for late_rules in [false, true] {
            let engine = pending(late_rules);
            for &format in SerializationFormat::ALL {
                let mut restored =
                    Engine::deserialize(&engine.serialize(format).unwrap(), format).unwrap();
                run_and_check(&mut restored, 289, &COMPLETE);
                assert_eq!(restored.get_output("t").unwrap_or(""), OUTPUT);
            }
        }
    }

    #[cfg(feature = "serde")]
    #[test]
    fn removed_rows_and_completed_matches_restore_before_fresh_arrivals() {
        use ferric_rules::runtime::SerializationFormat;
        for late_rules in [false, true] {
            let mut engine = pending(late_rules);
            remove_pending_rows(&mut engine);
            for &format in SerializationFormat::ALL {
                let mut restored =
                    Engine::deserialize(&engine.serialize(format).unwrap(), format).unwrap();
                run_and_check(&mut restored, 285, &AFTER_REMOVAL);
                assert_eq!(restored.get_output("t").unwrap_or(""), REMOVED_OUTPUT);
                let mut resumed =
                    Engine::deserialize(&restored.serialize(format).unwrap(), format).unwrap();
                finish_lifecycle(&mut resumed);
            }
        }
    }
}
