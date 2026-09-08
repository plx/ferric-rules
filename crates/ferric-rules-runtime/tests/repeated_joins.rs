//! One assertion must contribute each repeated-relation match exactly once.
use ferric_rules_runtime::{Engine, RunLimit, Value};

const FOUR: &str =
    "(defrule repeated (p ?a) (p ?b) (p ?c) (p ?d) => (printout t ?a ?b ?c ?d crlf))";

#[test]
fn one_fact_matches_four_repeated_patterns_once() {
    let mut engine = Engine::with_rules(FOUR).unwrap();
    engine.assert_ordered("p", vec![Value::Integer(1)]).unwrap();
    assert_eq!(engine.agenda_len(), 1);
    assert_eq!(engine.run(RunLimit::Unlimited).unwrap().rules_fired, 1);
    assert_eq!(engine.get_output("t").unwrap(), Some("1111\n"));
}

#[test]
fn two_facts_produce_the_sixteen_distinct_four_position_tuples() {
    let mut engine = Engine::with_rules(FOUR).unwrap();
    for value in [1, 2] {
        engine
            .assert_ordered("p", vec![Value::Integer(value)])
            .unwrap();
    }
    assert_eq!(engine.agenda_len(), 16);
    assert_eq!(engine.run(RunLimit::Unlimited).unwrap().rules_fired, 16);
    let mut tuples: Vec<_> = engine.get_output("t").unwrap().unwrap().lines().collect();
    tuples.sort_unstable();
    tuples.dedup();
    assert_eq!(tuples.len(), 16);
    assert_eq!(engine.get_output("t").unwrap(), Some("2222\n2221\n2212\n2211\n2122\n2121\n2112\n2111\n1222\n1221\n1212\n1211\n1122\n1121\n1112\n1111\n"));
}

#[test]
fn indexed_repeated_join_matches_each_pair_once_across_retraction() {
    use std::collections::HashSet;
    let mut engine = Engine::with_rules(
        "(defrule pairs (p ?key ?left) (p ?key ?right) => (printout t ?left \":\" ?right crlf))",
    )
    .unwrap();
    let facts: Vec<_> = (0..32)
        .map(|id| {
            engine
                .assert_ordered("p", vec![Value::Integer(0), Value::Integer(id)])
                .unwrap()
        })
        .collect();
    assert_eq!(engine.agenda_len(), 1024);
    assert_eq!(engine.run(RunLimit::Unlimited).unwrap().rules_fired, 1024);
    let pairs: HashSet<_> = engine.get_output("t").unwrap().unwrap().lines().collect();
    assert_eq!(pairs.len(), 1024);
    engine.retract(facts[11]).unwrap();
    assert_eq!(engine.agenda_len(), 0);
    engine.clear_output_channel("t");
    engine
        .assert_ordered("p", vec![Value::Integer(0), Value::Integer(11)])
        .unwrap();
    assert_eq!(engine.run(RunLimit::Unlimited).unwrap().rules_fired, 63);
    let pairs: HashSet<_> = engine.get_output("t").unwrap().unwrap().lines().collect();
    assert_eq!(pairs.len(), 63);
    assert!(pairs
        .iter()
        .all(|pair| pair.starts_with("11:") || pair.ends_with(":11")));
    #[cfg(debug_assertions)]
    engine.debug_assert_consistency();
}

#[test]
fn overlapping_broad_and_constant_alpha_paths_do_not_duplicate_matches() {
    for patterns in ["(p ?a) (p 1)", "(p 1) (p ?a)"] {
        let mut engine = Engine::with_rules(&format!("(defrule repeated {patterns} =>)")).unwrap();
        engine.assert_ordered("p", vec![Value::Integer(1)]).unwrap();
        assert_eq!(engine.run(RunLimit::Unlimited).unwrap().rules_fired, 1);
        #[cfg(debug_assertions)]
        engine.debug_assert_consistency();
    }
}

#[test]
fn retraction_and_rule_backfill_keep_repeated_matches_unique() {
    let mut engine = Engine::with_rules(FOUR).unwrap();
    let one = engine.assert_ordered("p", vec![Value::Integer(1)]).unwrap();
    assert_eq!(engine.run(RunLimit::Unlimited).unwrap().rules_fired, 1);
    let two = engine.assert_ordered("p", vec![Value::Integer(2)]).unwrap();
    assert_eq!(engine.run(RunLimit::Unlimited).unwrap().rules_fired, 15);
    engine.retract(two).unwrap();
    assert_eq!(engine.agenda_len(), 0);
    engine.assert_ordered("p", vec![Value::Integer(2)]).unwrap();
    assert_eq!(engine.run(RunLimit::Unlimited).unwrap().rules_fired, 15);
    engine.retract(one).unwrap();
    engine
        .load_str("(defrule later (p ?a) (p ?b) (p ?c) (p ?d) =>)")
        .unwrap();
    assert_eq!(engine.run(RunLimit::Unlimited).unwrap().rules_fired, 1);
    #[cfg(debug_assertions)]
    engine.debug_assert_consistency();
}

#[test]
fn replacements_with_sparse_beta_ids_preserve_tuple_order() {
    let mut engine = Engine::with_rules("(defrule keep-prefix (p ?a) (never) =>)").unwrap();
    engine.assert_ordered("p", vec![Value::Integer(1)]).unwrap();
    for _ in 0..8 {
        // Retain the shared first join while reclaiming and reallocating the
        // repeated rule's tail. Beta IDs remain sparse across replacements.
        engine.load_str(FOUR).unwrap();
        assert_eq!(engine.run(RunLimit::Unlimited).unwrap().rules_fired, 1);
        engine
            .load_str("(defrule repeated (q ?a) (q ?b) =>)")
            .unwrap();
        assert_eq!(engine.agenda_len(), 0);
    }
    engine.load_str(FOUR).unwrap();
    assert_eq!(engine.run(RunLimit::Unlimited).unwrap().rules_fired, 1);
    engine.clear_output_channel("t");
    engine.assert_ordered("p", vec![Value::Integer(2)]).unwrap();
    assert_eq!(engine.run(RunLimit::Unlimited).unwrap().rules_fired, 15);
    assert_eq!(engine.get_output("t").unwrap(), Some("2222\n2221\n2212\n2211\n2122\n2121\n2112\n2111\n1222\n1221\n1212\n1211\n1122\n1121\n1112\n"));
    #[cfg(debug_assertions)]
    engine.debug_assert_consistency();
}

#[test]
fn repeated_relation_exists_and_negative_support_stays_idempotent() {
    for (condition, count) in [("(exists (p ?a))", 1), ("(not (p ?a))", 0)] {
        let mut engine =
            Engine::with_rules(&format!("(defrule supported (p ?a) {condition} =>)")).unwrap();
        for _ in 0..3 {
            let fact = engine.assert_ordered("p", vec![Value::Integer(1)]).unwrap();
            assert_eq!(engine.run(RunLimit::Unlimited).unwrap().rules_fired, count);
            engine.retract(fact).unwrap();
            assert_eq!(engine.agenda_len(), 0);
            #[cfg(debug_assertions)]
            engine.debug_assert_consistency();
        }
    }
}

#[test]
fn sixty_four_repeated_patterns_remain_one_match_for_one_fact() {
    use std::fmt::Write as _;
    let mut source = String::from("(defrule repeated ");
    for index in 0..64 {
        write!(&mut source, "(p ?v{index}) ").unwrap();
    }
    source.push_str("=>)");
    let mut engine = Engine::with_rules(&source).unwrap();
    engine.assert_ordered("p", vec![Value::Integer(1)]).unwrap();
    assert_eq!(engine.agenda_len(), 1);
    assert_eq!(engine.run(RunLimit::Unlimited).unwrap().rules_fired, 1);
}

#[cfg(feature = "serde")]
#[test]
fn restored_repeated_join_continues_with_each_new_tuple_once() {
    use ferric_rules_runtime::SerializationFormat;
    let mut engine = Engine::with_rules(FOUR).unwrap();
    engine.assert_ordered("p", vec![Value::Integer(1)]).unwrap();
    assert_eq!(engine.run(RunLimit::Unlimited).unwrap().rules_fired, 1);
    for &format in SerializationFormat::ALL {
        let mut restored = Engine::deserialize(&engine.serialize(format).unwrap(), format).unwrap();
        restored
            .assert_ordered("p", vec![Value::Integer(2)])
            .unwrap();
        assert_eq!(restored.run(RunLimit::Unlimited).unwrap().rules_fired, 15);
        #[cfg(debug_assertions)]
        restored.debug_assert_consistency();
    }
}
