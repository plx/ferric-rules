//! Conditional element normalization preserves support lifetime and lexical scope.

use ferric_rules_core::Fact;
use ferric_rules_runtime::{Engine, EngineConfig, HaltReason, RunLimit, Value};

fn fire(engine: &mut Engine, expected: usize) {
    assert_eq!(engine.agenda_len(), expected);
    let result = engine.run(RunLimit::Unlimited).unwrap();
    assert_eq!(result.rules_fired, expected);
    assert_eq!(result.halt_reason, HaltReason::AgendaEmpty);
    assert!(engine.action_diagnostics().is_empty());
    engine.rete().validate_consistency().unwrap();
}

fn retract_item(engine: &mut Engine, value: i64) {
    let handle = engine
        .find_facts("item")
        .unwrap()
        .into_iter()
        .find_map(|(handle, fact)| match fact {
            Fact::Ordered(fact) if matches!(fact.fields.last(), Some(Value::Integer(found)) if *found == value) => Some(handle),
            _ => None,
        })
        .unwrap();
    engine.retract(handle).unwrap();
}

fn forall_lifecycle(late: bool, checkpoint: impl Fn(&mut Engine)) {
    let mut engine = Engine::new(EngineConfig::utf8());
    let rule = "(defrule all-positive (key ?group)
      (forall (item ?group ?local) (test (> ?local 0)))
      => (printout t ?group crlf))";
    if !late {
        engine.load_str(rule).unwrap();
    }
    engine.assert_ordered("key", 1_i64).unwrap();
    engine.assert_ordered("item", [2_i64, -9_i64]).unwrap();
    if late {
        engine.load_str(rule).unwrap();
    }
    checkpoint(&mut engine);
    fire(&mut engine, 1); // No matching antecedents: vacuously true.
    engine.assert_ordered("item", [1_i64, 4_i64]).unwrap();
    checkpoint(&mut engine);
    fire(&mut engine, 0); // A passing tuple does not recreate the activation.
    engine.assert_ordered("item", [1_i64, -1_i64]).unwrap();
    checkpoint(&mut engine);
    fire(&mut engine, 0);
    engine.assert_ordered("item", [1_i64, -2_i64]).unwrap();
    retract_item(&mut engine, -1);
    checkpoint(&mut engine);
    fire(&mut engine, 0); // The other counterexample still blocks the rule.
    retract_item(&mut engine, -2);
    checkpoint(&mut engine);
    fire(&mut engine, 1);
    engine.assert_ordered("item", [1_i64, -3_i64]).unwrap();
    fire(&mut engine, 0);
    retract_item(&mut engine, 4);
    retract_item(&mut engine, -3);
    checkpoint(&mut engine);
    fire(&mut engine, 1); // Empty again, with all old support metadata removed.
    assert_eq!(engine.get_output("t"), Some("1\n1\n1\n"));

    engine.reset().unwrap();
    fire(&mut engine, 0);
    engine.assert_ordered("key", 1_i64).unwrap();
    checkpoint(&mut engine);
    fire(&mut engine, 1);
}

#[test]
fn forall_test_tracks_counterexamples_vacuity_outer_bindings_and_late_install() {
    for late in [false, true] {
        forall_lifecycle(late, |_| {});
    }
}

#[cfg(feature = "serde")]
#[test]
fn forall_test_retains_local_predicates_and_supports_across_snapshots() {
    use ferric_rules_runtime::SerializationFormat;
    for format in [SerializationFormat::Json, SerializationFormat::Cbor] {
        for late in [false, true] {
            forall_lifecycle(late, |engine| {
                let bytes = engine.serialize(format).unwrap();
                *engine = Engine::deserialize(&bytes, format).unwrap();
            });
        }
    }
}

#[test]
fn forall_test_respects_antecedent_predicates_and_constant_false_vacuity() {
    let mut engine = Engine::with_rules(
        "(defrule bounded
           (forall (item ?x&:(> ?x 0)) (test (< ?x 10))) =>)
         (defrule empty-only
           (forall (item ?x) (test (= 1 0))) =>)",
    )
    .unwrap();
    fire(&mut engine, 2);
    engine.assert_ordered("item", -1_i64).unwrap();
    fire(&mut engine, 0);
    engine.assert_ordered("item", 5_i64).unwrap();
    fire(&mut engine, 0);
    engine.assert_ordered("item", 20_i64).unwrap();
    fire(&mut engine, 0);
    retract_item(&mut engine, 20);
    fire(&mut engine, 1);
    retract_item(&mut engine, 5);
    retract_item(&mut engine, -1);
    fire(&mut engine, 1);
}

#[test]
fn forall_locals_cannot_escape_but_later_patterns_can_bind_the_same_name() {
    for tail in ["(test (> ?local 0)) =>", "=> (printout t ?local)"] {
        let source = format!("(defrule bad (forall (item ?local) (test (> ?local 0))) {tail})");
        let mut engine = Engine::new(EngineConfig::utf8());
        assert!(engine.load_str(&source).is_err(), "{source}");
        assert!(engine.rules().is_empty());
    }
    let mut engine = Engine::with_rules(
        "(deffacts seed (item 2) (outside 9))
         (defrule fresh
           (forall (item ?local) (test (> ?local 0)))
           (outside ?local) => (printout t ?local crlf))",
    )
    .unwrap();
    fire(&mut engine, 1);
    assert_eq!(engine.get_output("t"), Some("9\n"));
}

#[test]
fn not_or_reacts_to_each_correlated_branch_without_duplicate_activations() {
    for late in [false, true] {
        let rule = "(defrule absent (key ?k)
          (not (or (left ?k) (and (right ?k) (gate)))) =>)";
        let mut engine = Engine::new(EngineConfig::utf8());
        if !late {
            engine.load_str(rule).unwrap();
        }
        engine.assert_ordered("key", 7_i64).unwrap();
        engine.assert_ordered("left", 99_i64).unwrap();
        if late {
            engine.load_str(rule).unwrap();
        }
        fire(&mut engine, 1);
        let left = engine.assert_ordered("left", 7_i64).unwrap();
        let right = engine.assert_ordered("right", 7_i64).unwrap();
        let gate = engine.assert_ordered("gate", ()).unwrap();
        fire(&mut engine, 0);
        engine.retract(left).unwrap();
        fire(&mut engine, 0);
        engine.retract(gate).unwrap();
        fire(&mut engine, 1);
        engine.retract(right).unwrap();
        fire(&mut engine, 0);
    }
}

#[test]
fn nested_not_or_preserves_conjunction_when_inner_normalization_splits() {
    let mut engine = Engine::with_rules(
        "(deffacts seed (a) (b))
         (defrule present (not (not (or (a) (b)))) => (printout t once crlf))",
    )
    .unwrap();
    fire(&mut engine, 1);
    assert_eq!(engine.get_output("t"), Some("once\n"));
}

#[test]
fn quantified_test_only_wrappers_collapse_but_positive_or_keeps_disjuncts() {
    let mut engine = Engine::with_rules(
        "(defrule plain (or (test (> 1 0)) (test (> 2 0)))
           => (printout t plain crlf))
         (defrule quantified (exists (or (test (> 1 0)) (test (> 2 0))))
           => (printout t quantified crlf))
         (defrule nested
           (not (exists (and (test (> 0 1)) (not (test (> 0 1))))))
           => (printout t nested crlf))",
    )
    .unwrap();
    fire(&mut engine, 4);
    let mut lines: Vec<_> = engine.get_output("t").unwrap().lines().collect();
    lines.sort_unstable();
    assert_eq!(lines, ["nested", "plain", "plain", "quantified"]);
}

#[test]
fn invalid_forall_test_replacement_preserves_the_old_rule_and_later_loads() {
    let mut engine = Engine::with_rules("(defrule keep => (printout t old crlf))").unwrap();
    let errors = engine
        .load_str(
            "(defrule keep
           (forall (item ?local) (test (> ?missing 0)))
           => (printout t wrong crlf))
         (defrule later => (printout t later crlf))",
        )
        .unwrap_err();
    let message = errors
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join("\n");
    assert!(message.contains("?missing"), "{message}");
    assert!(message.contains("line 2"), "{message}");
    fire(&mut engine, 2);
    let mut lines: Vec<_> = engine.get_output("t").unwrap().lines().collect();
    lines.sort_unstable();
    assert_eq!(lines, ["later", "old"]);
}

#[test]
fn forall_test_keeps_query_member_variables_lexical() {
    let mut engine = Engine::with_rules(
        "(deftemplate record (slot n))
         (deffacts seed (record (n 2)) (item 2))
         (defrule all-present
           (forall (item ?local) (test (any-factp ((?f record)) (= ?f:n ?local))))
           => (printout t matched crlf))",
    )
    .unwrap();
    fire(&mut engine, 1);
    assert_eq!(engine.get_output("t"), Some("matched\n"));
}

#[test]
fn positive_or_normalizes_each_branch_without_changing_multiplicity() {
    let mut engine = Engine::with_rules(
        "(deffacts seed (p))
         (defrule neg-test
           (or (not (test (> 1 0))) (test (> 1 0)))
           => (printout t neg-test crlf))
         (defrule inner-and
           (or (and (p) (not (test (> 0 1)))) (p))
           => (printout t inner-and crlf))
         (defrule inner-exists
           (or (exists (test (> 1 0))) (test (> 1 0)))
           => (printout t inner-exists crlf))
         (defrule split-branch
           (or (not (or (a) (b))) (test (> 1 0)))
           => (printout t split-branch crlf))",
    )
    .unwrap();
    fire(&mut engine, 7);
    let mut lines: Vec<_> = engine.get_output("t").unwrap().lines().collect();
    lines.sort_unstable();
    assert_eq!(
        lines,
        [
            "inner-and",
            "inner-and",
            "inner-exists",
            "inner-exists",
            "neg-test",
            "split-branch",
            "split-branch",
        ]
    );
    let blocker = engine.assert_ordered("a", ()).unwrap();
    fire(&mut engine, 0);
    engine.retract(blocker).unwrap();
    fire(&mut engine, 1);
}

#[test]
fn exists_over_or_is_one_condition_however_many_branches_hold() {
    for (rule, seed) in [
        (
            "(defrule any (exists (or (a 1) (b 1))) => (printout t any crlf))",
            &[] as &[&str],
        ),
        (
            "(defrule any (exists (x) (or (not (c)) (b 1))) => (printout t any crlf))",
            &["x"] as &[&str],
        ),
    ] {
        for late in [false, true] {
            let mut engine = Engine::new(EngineConfig::utf8());
            if !late {
                engine.load_str(rule).unwrap();
            }
            for relation in seed {
                engine.assert_ordered(relation, ()).unwrap();
            }
            let a = engine.assert_ordered("a", 1_i64).unwrap();
            let b = engine.assert_ordered("b", 1_i64).unwrap();
            if late {
                engine.load_str(rule).unwrap();
            }
            // Every branch holds, but exists still yields one activation.
            fire(&mut engine, 1);
            engine.retract(a).unwrap();
            fire(&mut engine, 0);
            engine.retract(b).unwrap();
            let blocker = engine.assert_ordered("c", ()).unwrap();
            fire(&mut engine, 0);
            engine.retract(blocker).unwrap();
            // Without (x), the second rule's body has no tuple at all.
            fire(&mut engine, usize::from(!seed.is_empty()));
            engine.assert_ordered("b", 1_i64).unwrap();
            fire(&mut engine, usize::from(seed.is_empty()));
            assert_eq!(engine.get_output("t"), Some("any\nany\n"), "{rule}");
        }
    }
}

#[test]
fn nested_groups_flatten_and_distribute_into_rule_level_disjuncts() {
    let mut engine = Engine::with_rules(
        "(deffacts seed (b) (c) (x))
         (defrule or-or (or (or (a) (b)) (z)) => (printout t or-or crlf))
         (defrule or-and-or (or (and (or (a) (b)) (c)) (d)) => (printout t or-and-or crlf))
         (defrule and-and-or (and (and (or (a) (b)))) => (printout t and-and-or crlf))
         (defrule not-exists-or (x) (not (exists (or (a) (d)))) => (printout t not-exists-or crlf))
         (defrule not-and-and (not (and (x) (and (b) (d)))) => (printout t not-and-and crlf))
         (defrule exists-and-and (exists (and (b) (and (c) (x)))) => (printout t exists-and-and crlf))
         (defrule not-not-and (not (not (and (b) (c)))) => (printout t not-not-and crlf))",
    )
    .unwrap();
    fire(&mut engine, 7);
    let mut lines: Vec<_> = engine.get_output("t").unwrap().lines().collect();
    lines.sort_unstable();
    assert_eq!(
        lines,
        [
            "and-and-or",
            "exists-and-and",
            "not-and-and",
            "not-exists-or",
            "not-not-and",
            "or-and-or",
            "or-or",
        ]
    );
    // A second disjunct of each positive or adds its own activation.
    engine.assert_ordered("a", ()).unwrap();
    fire(&mut engine, 3);
}

#[test]
fn negated_and_existential_locals_cannot_reach_a_later_test() {
    // CLIPS rejects each of these with [ANALYSIS4]: ?x is first bound inside
    // the negated or existential CE, so the later test reads an undefined name.
    for (lhs, column) in [
        ("(exists (or (a ?x) (b ?x)))", 46),
        ("(exists (and (c) (or (a ?x) (b ?x))))", 56),
        ("(not (or (a ?x) (b ?x)))", 43),
        ("(not (a ?x))", 31),
    ] {
        let source = format!("(defrule r {lhs} (test (> ?x 0)) => (printout t fired crlf))");
        let mut engine = Engine::new(EngineConfig::utf8());
        let errors = engine.load_str(&source).expect_err(&source);
        let message = errors
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join("\n");
        assert!(
            message.contains("rule `r` variable ?x is not exported by existential or negated")
                && message.contains(&format!("at line 1, column {column}")),
            "{source}: {message}"
        );
        assert!(engine.rules().is_empty(), "{source}");
    }

    // A binding exported by an earlier positive pattern remains visible.
    let mut engine = Engine::with_rules(
        "(deffacts seed (a 1) (a 2) (a -1) (b 1) (b -1) (c 3))
         (defrule exists-or (a ?x) (exists (or (b ?x) (c ?x))) (test (> ?x 0))
           => (printout t exists ?x crlf))
         (defrule not-b (a ?x) (not (b ?x)) (test (> ?x 0))
           => (printout t not ?x crlf))",
    )
    .unwrap();
    fire(&mut engine, 2);
    let mut lines: Vec<_> = engine.get_output("t").unwrap().lines().collect();
    lines.sort_unstable();
    assert_eq!(lines, ["exists1", "not2"]);
}
