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
