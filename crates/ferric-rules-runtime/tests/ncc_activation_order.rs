//! NCC retraction follows the reference's primary-blocker chronology.
use std::fmt::Write as _;

use ferric_rules_core::ConflictResolutionStrategy;
use ferric_rules_runtime::{Engine, EngineConfig, HaltReason, RunLimit};

const MIGRATIONS: &[(&str, &str, &str, &str)] = &[
    (
        "2-before-ab",
        "(blocker a) (blocker b) (item 1) (item 2) (item 3)",
        "ab",
        "321",
    ),
    (
        "2-before-ba",
        "(blocker a) (blocker b) (item 1) (item 2) (item 3)",
        "ba",
        "123",
    ),
    (
        "2-after-ab",
        "(item 1) (item 2) (item 3) (blocker a) (blocker b)",
        "ab",
        "123",
    ),
    (
        "2-after-ba",
        "(item 1) (item 2) (item 3) (blocker a) (blocker b)",
        "ba",
        "321",
    ),
    (
        "2-interleaved-ab",
        "(item 1) (blocker a) (item 2) (blocker b) (item 3)",
        "ab",
        "321",
    ),
    (
        "2-interleaved-ba",
        "(item 1) (blocker a) (item 2) (blocker b) (item 3)",
        "ba",
        "123",
    ),
    (
        "3-before-abc",
        "(blocker a) (blocker b) (blocker c) (item 1) (item 2) (item 3)",
        "abc",
        "123",
    ),
    (
        "3-before-acb",
        "(blocker a) (blocker b) (blocker c) (item 1) (item 2) (item 3)",
        "acb",
        "321",
    ),
    (
        "3-before-bac",
        "(blocker a) (blocker b) (blocker c) (item 1) (item 2) (item 3)",
        "bac",
        "321",
    ),
    (
        "3-before-bca",
        "(blocker a) (blocker b) (blocker c) (item 1) (item 2) (item 3)",
        "bca",
        "123",
    ),
    (
        "3-before-cab",
        "(blocker a) (blocker b) (blocker c) (item 1) (item 2) (item 3)",
        "cab",
        "321",
    ),
    (
        "3-before-cba",
        "(blocker a) (blocker b) (blocker c) (item 1) (item 2) (item 3)",
        "cba",
        "123",
    ),
    (
        "3-after-abc",
        "(item 1) (item 2) (item 3) (blocker a) (blocker b) (blocker c)",
        "abc",
        "321",
    ),
    (
        "3-after-acb",
        "(item 1) (item 2) (item 3) (blocker a) (blocker b) (blocker c)",
        "acb",
        "123",
    ),
    (
        "3-after-bac",
        "(item 1) (item 2) (item 3) (blocker a) (blocker b) (blocker c)",
        "bac",
        "123",
    ),
    (
        "3-after-bca",
        "(item 1) (item 2) (item 3) (blocker a) (blocker b) (blocker c)",
        "bca",
        "321",
    ),
    (
        "3-after-cab",
        "(item 1) (item 2) (item 3) (blocker a) (blocker b) (blocker c)",
        "cab",
        "123",
    ),
    (
        "3-after-cba",
        "(item 1) (item 2) (item 3) (blocker a) (blocker b) (blocker c)",
        "cba",
        "321",
    ),
    (
        "3-interleaved-abc",
        "(item 1) (blocker a) (item 2) (blocker b) (item 3) (blocker c)",
        "abc",
        "123",
    ),
    (
        "3-interleaved-acb",
        "(item 1) (blocker a) (item 2) (blocker b) (item 3) (blocker c)",
        "acb",
        "321",
    ),
    (
        "3-interleaved-bac",
        "(item 1) (blocker a) (item 2) (blocker b) (item 3) (blocker c)",
        "bac",
        "321",
    ),
    (
        "3-interleaved-bca",
        "(item 1) (blocker a) (item 2) (blocker b) (item 3) (blocker c)",
        "bca",
        "123",
    ),
    (
        "3-interleaved-cab",
        "(item 1) (blocker a) (item 2) (blocker b) (item 3) (blocker c)",
        "cab",
        "321",
    ),
    (
        "3-interleaved-cba",
        "(item 1) (blocker a) (item 2) (blocker b) (item 3) (blocker c)",
        "cba",
        "123",
    ),
];

fn source(facts: &str, removals: &str) -> String {
    let mut source = format!("(deffacts seed (other) {facts})\n(defrule r (item ?x) (not (and (blocker ?) (other))) => (printout t ?x crlf))\n");
    for (index, blocker) in removals.chars().enumerate() {
        writeln!(source, "(defrule remove-{blocker} (declare (salience {})) ?b <- (blocker {blocker}) => (retract ?b))", 100 - index).unwrap();
    }
    source
}

fn expected(order: &str, strategy: ConflictResolutionStrategy) -> String {
    let mut values: Vec<_> = order.chars().collect();
    if strategy == ConflictResolutionStrategy::Breadth {
        values.reverse();
    }
    values.into_iter().flat_map(|value| [value, '\n']).collect()
}

#[test]
fn primary_result_migration_matches_reference_before_after_and_between_parents() {
    for &(name, facts, removals, order) in MIGRATIONS {
        for strategy in [
            ConflictResolutionStrategy::Depth,
            ConflictResolutionStrategy::Breadth,
        ] {
            let mut engine = Engine::new(EngineConfig::default().with_strategy(strategy));
            engine.load_str(&source(facts, removals)).unwrap();
            for _ in 0..2 {
                engine.reset().unwrap();
                assert_eq!(
                    engine.run(RunLimit::Unlimited).unwrap().halt_reason,
                    HaltReason::AgendaEmpty,
                    "{name} {strategy:?}"
                );
                assert_eq!(
                    engine.get_output("t"),
                    Some(expected(order, strategy).as_str()),
                    "{name} {strategy:?}"
                );
                assert!(engine.action_diagnostics().is_empty());
                engine.rete().debug_assert_consistency();
            }
        }
    }
}

#[test]
fn one_shared_subnetwork_result_unblocks_every_ncc_partner() {
    let mut engine = Engine::with_rules(
        "(deffacts seed (item 1) (blocker) (other))
      (defrule r1 (item ?x) (not (and (blocker) (other))) => (printout t r1 crlf))
      (defrule r2 (item ?x) (not (and (blocker) (other))) => (printout t r2 crlf))
      (defrule release (declare (salience 10)) ?f <- (blocker) => (retract ?f))",
    )
    .unwrap();
    assert_eq!(engine.run(RunLimit::Unlimited).unwrap().rules_fired, 3);
    // Identical NCC nodes are not shared yet, so their relative tie order is
    // outside this regression. Every partner must still receive the removal.
    let mut fired: Vec<_> = engine.get_output("t").unwrap().lines().collect();
    fired.sort_unstable();
    assert_eq!(fired, ["r1", "r2"]);
    engine.rete().debug_assert_consistency();
}

#[cfg(feature = "serde")]
#[test]
fn snapshot_preserves_primary_result_and_future_migration_order() {
    use ferric_rules_runtime::SerializationFormat;
    for &(name, facts, removals, order) in MIGRATIONS {
        let mut engine = Engine::with_rules(&source(facts, removals)).unwrap();
        let bytes = engine.serialize(SerializationFormat::Cbor).unwrap();
        engine = Engine::deserialize(&bytes, SerializationFormat::Cbor).unwrap();
        engine.run(RunLimit::Unlimited).unwrap();
        assert_eq!(
            engine.get_output("t"),
            Some(expected(order, ConflictResolutionStrategy::Depth).as_str()),
            "{name}"
        );
    }
}

#[test]
fn removing_one_fact_settles_all_results_and_deleted_parent_state() {
    let mut engine = Engine::with_rules(
        "(defrule r (item ?x) (not (and (blocker) (other ?))) => (printout t ?x crlf))",
    )
    .unwrap();
    // Reusing storage after each cascade must not leave primary-result records
    // attached to either the deleted parent or a removed sibling result.
    for _ in 0..20 {
        let first = engine.assert_ordered("item", 1_i64).unwrap();
        let removed_parent = engine.assert_ordered("item", 2_i64).unwrap();
        let last = engine.assert_ordered("item", 3_i64).unwrap();
        let blocker = engine.assert_ordered("blocker", ()).unwrap();
        let first_result = engine.assert_ordered("other", 1_i64).unwrap();
        let second_result = engine.assert_ordered("other", 2_i64).unwrap();
        engine.retract(removed_parent).unwrap();
        engine.retract(blocker).unwrap();
        assert_eq!(engine.run(RunLimit::Unlimited).unwrap().rules_fired, 2);
        assert_eq!(engine.get_output("t"), Some("3\n1\n"));
        engine.clear_output_channel("t");
        for fact in [first, last, first_result, second_result] {
            engine.retract(fact).unwrap();
        }
        engine.rete().debug_assert_consistency();
        assert_eq!(engine.rete().token_store.len(), 1);
        assert!(engine.action_diagnostics().is_empty());
    }
}

#[test]
fn nested_and_overlapping_ncc_paths_settle_before_rule_firing() {
    let sources = [
        "(deffacts seed (item 1) (item 2) (item 3) (blocker) (permit) (gate))
         (defrule r (item ?x) (not (and (blocker) (not (and (permit) (gate))))) => (printout t ?x crlf))
         (defrule close (declare (salience 20)) ?g <- (gate) => (retract ?g))
         (defrule release (declare (salience 10)) ?b <- (blocker) => (retract ?b))",
        "(deffacts seed (item 1) (item 2) (item 3) (trigger))
         (defrule r (item ?x) (not (and (blocker) (other))) (other) => (printout t ?x crlf))
         (defrule start (declare (salience 20)) ?t <- (trigger) => (retract ?t) (assert (blocker) (other)))
         (defrule release (declare (salience 10)) ?b <- (blocker) => (retract ?b))",
    ];
    for source in sources {
        let mut engine = Engine::with_rules(source).unwrap();
        for _ in 0..2 {
            assert_eq!(engine.run(RunLimit::Unlimited).unwrap().rules_fired, 5);
            assert_eq!(engine.get_output("t"), Some("3\n2\n1\n"));
            assert!(engine.action_diagnostics().is_empty());
            engine.rete().debug_assert_consistency();
            engine.reset().unwrap();
        }
    }
}

#[test]
fn multi_exists_retains_current_topology_ties_and_correlated_completion_order() {
    // Simple multi-pattern exists is still represented by two embedded NCCs.
    // CLIPS emits 1,2,3 for the first case; changing that pre-existing topology
    // boundary is separate from #400's blocker and successor chronology fixes.
    for (source, output) in [
        (
            "(deffacts seed (item 1) (item 2) (item 3) (blocker) (other))
          (defrule r (item ?x) (exists (blocker) (other)) => (printout t ?x crlf))",
            "3\n2\n1\n",
        ),
        (
            "(deffacts seed (key a) (key c) (sym a) (sym b) (sym d) (marker))
          (defrule r (key ?k) (exists (sym ?k|b) (marker)) => (printout t ?k crlf))",
            "c\na\n",
        ),
    ] {
        let mut engine = Engine::with_rules(source).unwrap();
        engine.run(RunLimit::Unlimited).unwrap();
        assert_eq!(engine.get_output("t"), Some(output));
        engine.rete().debug_assert_consistency();
    }
}

#[cfg(feature = "serde")]
#[test]
fn late_shared_partner_preserves_restorable_support_order() {
    use ferric_rules_runtime::SerializationFormat;
    for (removals, depth) in [("ab", "321"), ("ba", "123")] {
        for strategy in [
            ConflictResolutionStrategy::Depth,
            ConflictResolutionStrategy::Breadth,
        ] {
            let mut engine = Engine::new(EngineConfig::default().with_strategy(strategy));
            engine.load_str(
                "(deffacts seed (item 1) (item 2) (item 3) (blocker a) (blocker b) (other))
                 (defrule early (item ?x) (not (and (blocker ?) (other))) => (printout t \"early:\" ?x crlf))",
            ).unwrap();
            engine.reset().unwrap();
            let mut later = String::from(
                "(defrule late (item ?x) (not (and (blocker ?) (other))) => (printout t \"late:\" ?x crlf))",
            );
            for (index, blocker) in removals.chars().enumerate() {
                writeln!(later, "(defrule remove-{blocker} (declare (salience {})) ?b <- (blocker {blocker}) => (retract ?b))", 100 - index).unwrap();
            }
            engine.load_str(&later).unwrap();
            let bytes = engine.serialize(SerializationFormat::Cbor).unwrap();
            engine = Engine::deserialize(&bytes, SerializationFormat::Cbor).unwrap();
            assert_eq!(engine.run(RunLimit::Unlimited).unwrap().rules_fired, 8);
            let output = engine.get_output("t").unwrap();
            for prefix in ["early:", "late:"] {
                let observed: String = output
                    .lines()
                    .filter_map(|line| line.strip_prefix(prefix))
                    .flat_map(|value| value.chars().chain(std::iter::once('\n')))
                    .collect();
                assert_eq!(
                    observed,
                    expected(depth, strategy),
                    "{removals} {strategy:?} {prefix}"
                );
            }
            engine.rete().debug_assert_consistency();
        }
    }
}
