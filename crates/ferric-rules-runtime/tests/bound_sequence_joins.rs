//! Parent-bound sequence placements retain capture identity across engine lifecycles.

use ferric_rules_core::{ConflictResolutionStrategy, Fact};
use ferric_rules_runtime::{Engine, EngineConfig, FactHandle, HaltReason, RunLimit};

const RULE: &str = r#"
    (defrule observe (first ?a) (second ?b)
        (lst $?before ?a $?middle ?b $?after)
        => (printout t (length$ ?before) ":" (length$ ?middle) ":" (length$ ?after)
            " " ?before "|" ?middle "|" ?after crlf))
"#;

// Pinned CLIPS fixtures 417_duplicate_placements_depth/breadth establish both
// placement order and the values captured by empty/nonempty multifields.
const DEPTH: &str = "0:1:1 ()|(a)|(b)\n0:2:0 ()|(a b)|()\n1:0:1 (a)|()|(b)\n1:1:0 (a)|(b)|()\n";
const OLDEST_FIRST: &str =
    "1:1:0 (a)|(b)|()\n1:0:1 (a)|()|(b)\n0:2:0 ()|(a b)|()\n0:1:1 ()|(a)|(b)\n";

fn strategies() -> [(ConflictResolutionStrategy, &'static str); 4] {
    [
        (ConflictResolutionStrategy::Depth, DEPTH),
        (ConflictResolutionStrategy::Breadth, OLDEST_FIRST),
        // These splits share the same fact basis and rule complexity, so LEX
        // and MEA select the oldest activation at their final tie-breaker.
        (ConflictResolutionStrategy::Lex, OLDEST_FIRST),
        (ConflictResolutionStrategy::Mea, OLDEST_FIRST),
    ]
}

fn assert_consistent(engine: &Engine) {
    assert!(engine.action_diagnostics().is_empty());
    #[cfg(debug_assertions)]
    engine.debug_assert_consistency();
}

fn run(engine: &mut Engine, expected: usize) {
    let result = engine.run(RunLimit::Unlimited).unwrap();
    assert_eq!(result.rules_fired, expected);
    assert_eq!(result.halt_reason, HaltReason::AgendaEmpty);
    assert_consistent(engine);
}

fn fact(engine: &Engine, relation: &str) -> FactHandle {
    let facts = engine.find_facts(relation).unwrap();
    assert_eq!(facts.len(), 1);
    facts[0].0
}

fn engine(strategy: ConflictResolutionStrategy, list_first: bool, online: bool) -> Engine {
    let mut engine = Engine::new(EngineConfig::utf8().with_strategy(strategy));
    let facts = if list_first {
        "(deffacts seed (lst a a b b) (first a) (second b))"
    } else {
        "(deffacts seed (first a) (second b) (lst a a b b))"
    };
    engine.load_str(facts).unwrap();
    if !online {
        engine.load_str(RULE).unwrap();
    }
    engine.reset().unwrap();
    if online {
        engine.load_str(RULE).unwrap();
    }
    assert_eq!(engine.agenda_len(), 4);
    assert_consistent(&engine);
    engine
}

#[test]
fn ordered_capture_order_survives_both_arrivals_retraction_reassertion_and_reset() {
    for (strategy, expected) in strategies() {
        for list_first in [false, true] {
            let mut engine = engine(strategy, list_first, false);
            run(&mut engine, 4);
            assert_eq!(engine.get_output("t"), Some(expected));

            let original = fact(&engine, "lst");
            engine.retract(original).unwrap();
            assert_consistent(&engine);
            engine.load_str("(assert (lst a a b b))").unwrap();
            assert_ne!(fact(&engine, "lst"), original);
            run(&mut engine, 4);
            assert_eq!(engine.get_output("t"), Some(expected.repeat(2).as_str()));

            engine.retract(fact(&engine, "first")).unwrap();
            assert_eq!(engine.agenda_len(), 0);
            engine.load_str("(assert (first a))").unwrap();
            run(&mut engine, 4);
            assert_eq!(engine.get_output("t"), Some(expected.repeat(3).as_str()));

            engine.reset().unwrap();
            run(&mut engine, 4);
            assert_eq!(engine.get_output("t"), Some(expected));
        }
    }
}

#[test]
fn online_install_preserves_capture_order_over_preexisting_facts() {
    for (strategy, expected) in strategies() {
        for list_first in [false, true] {
            let mut engine = engine(strategy, list_first, true);
            run(&mut engine, 4);
            assert_eq!(engine.get_output("t"), Some(expected));
        }
    }
}

#[cfg(feature = "serde")]
#[test]
fn partial_agenda_keeps_split_identity_order_and_refraction_after_restore() {
    use ferric_rules_runtime::SerializationFormat;

    for (strategy, expected) in strategies() {
        for list_first in [false, true] {
            let mut original = engine(strategy, list_first, false);
            assert_eq!(original.run(RunLimit::Count(2)).unwrap().rules_fired, 2);
            assert_eq!(original.agenda_len(), 2);
            let prefix = expected.split_inclusive('\n').take(2).collect::<String>();
            assert_eq!(original.get_output("t"), Some(prefix.as_str()));
            for &format in SerializationFormat::ALL {
                let bytes = original.serialize(format).unwrap();
                let mut restored = Engine::deserialize(&bytes, format).unwrap();
                run(&mut restored, 2);
                assert_eq!(restored.get_output("t"), Some(expected));
                run(&mut restored, 0);

                restored.retract(fact(&restored, "lst")).unwrap();
                restored.load_str("(assert (lst a a b b))").unwrap();
                run(&mut restored, 4);
                assert_eq!(restored.get_output("t"), Some(expected.repeat(2).as_str()));
                restored.reset().unwrap();
                run(&mut restored, 4);
                assert_eq!(restored.get_output("t"), Some(expected));
            }
        }
    }
}

fn list_with_length(engine: &Engine, length: usize) -> FactHandle {
    engine
        .find_facts("lst")
        .unwrap()
        .into_iter()
        .find_map(|(id, fact)| match fact {
            Fact::Ordered(fact) if fact.fields.len() == length => Some(id),
            _ => None,
        })
        .unwrap()
}

#[test]
fn bound_negative_exists_and_ncc_supports_survive_primary_removal_and_reuse() {
    for (condition, exists, ncc) in [
        ("(not (lst $? ?a $? ?b $?))", false, false),
        ("(exists (lst $? ?a $? ?b $?))", true, false),
        ("(not (and (lst $? ?a $? ?b $?) (marker)))", false, true),
    ] {
        for lists_first in [false, true] {
            let mut engine = Engine::new(EngineConfig::utf8());
            engine
                .load_str(&format!(
                    "(defrule observe (first ?a) (second ?b) {condition} => (printout t fired))"
                ))
                .unwrap();
            engine.reset().unwrap();
            engine.load_str("(assert (marker) (lst b a))").unwrap();
            if !lists_first {
                engine.load_str("(assert (first a) (second b))").unwrap();
            }
            engine
                .load_str("(assert (lst a a b b) (lst a b tail))")
                .unwrap();
            let first = list_with_length(&engine, 4);
            let second = list_with_length(&engine, 3);
            if lists_first {
                engine.load_str("(assert (first a) (second b))").unwrap();
            }
            run(&mut engine, usize::from(exists));

            // Removing the oldest support must retain the remaining support.
            engine.retract(first).unwrap();
            run(&mut engine, 0);
            engine.retract(second).unwrap();
            run(&mut engine, usize::from(!exists));

            engine.load_str("(assert (lst a a b b))").unwrap();
            assert_ne!(list_with_length(&engine, 4), first);
            run(&mut engine, usize::from(exists));
            engine.retract(fact(&engine, "first")).unwrap();
            run(&mut engine, 0);
            engine.load_str("(assert (first a))").unwrap();
            run(&mut engine, usize::from(exists));

            if ncc {
                engine.retract(fact(&engine, "marker")).unwrap();
                run(&mut engine, 1);
                engine.load_str("(assert (marker))").unwrap();
                run(&mut engine, 0);
            }
        }
    }
}

#[cfg(feature = "serde")]
#[test]
fn template_segment_capture_lengths_survive_online_install_and_partial_restore() {
    use ferric_rules_runtime::SerializationFormat;

    let mut engine = Engine::new(EngineConfig::utf8());
    engine
        .load_str(
            r"
        (deftemplate row (slot id) (multislot left) (multislot right))
        (deffacts seed (first a) (second b)
            (row (id good) (left a x a) (right b b))
            (row (id rejected) (left missing missing) (right b b b)))
    ",
        )
        .unwrap();
    engine.reset().unwrap();
    engine
        .load_str(
            r#"
        (defrule observe (first ?a) (second ?b)
            (row (left $?lp ?a $?ls) (right $?rp ?b $?rs) (id ?id))
            => (printout t ?id ":" (length$ ?lp) ":" (length$ ?ls)
                ":" (length$ ?rp) ":" (length$ ?rs) crlf))
    "#,
        )
        .unwrap();
    assert_eq!(engine.agenda_len(), 4);
    assert_eq!(engine.run(RunLimit::Count(1)).unwrap().rules_fired, 1);
    assert_eq!(engine.get_output("t"), Some("good:0:2:0:1\n"));
    // Pinned 417_template_segments_depth output; the two slots form an
    // independent product and the rejected row contributes no placements.
    let expected = "good:0:2:0:1\ngood:0:2:1:0\ngood:2:0:0:1\ngood:2:0:1:0\n";
    for &format in SerializationFormat::ALL {
        let bytes = engine.serialize(format).unwrap();
        let mut restored = Engine::deserialize(&bytes, format).unwrap();
        run(&mut restored, 3);
        assert_eq!(restored.get_output("t"), Some(expected));
        restored.retract(fact(&restored, "second")).unwrap();
        restored.load_str("(assert (second b))").unwrap();
        run(&mut restored, 4);
        assert_eq!(restored.get_output("t"), Some(expected.repeat(2).as_str()));
    }
}
