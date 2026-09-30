//! Host-driven lifecycles that the corpus protocol cannot express: top-level
//! assertions, host fact operations between runs, and Ferric's own run
//! boundaries.

use ferric_rules::core::{Fact, Value};
use ferric_rules::runtime::{
    Engine, EngineConfig, FactHandle, HaltReason, RunLimit, SerializationFormat,
};

fn run(engine: &mut Engine) -> usize {
    let result = engine.run(RunLimit::Count(1_000)).unwrap();
    assert_eq!(result.halt_reason, HaltReason::AgendaEmpty);
    assert!(
        engine.action_diagnostics().is_empty(),
        "{:?}",
        engine.action_diagnostics()
    );
    result.rules_fired
}

fn restored(engine: &Engine) -> Engine {
    let format = SerializationFormat::RECOMMENDED;
    Engine::deserialize(&engine.serialize(format).unwrap(), format).unwrap()
}

fn loaded(source: &str) -> Engine {
    let mut engine = Engine::new(EngineConfig::utf8());
    engine.load_str(source).unwrap();
    engine
}

/// Issue #322: top-level `assert` keeps singleton, several, empty and
/// defaulted multislots, also in a restored engine. CLIPS 6.30 prints the
/// same after `(reset)` and these assertions at its prompt.
#[test]
fn top_level_multislot_assertions() {
    let mut engine = loaded(
        "(deftemplate bag (slot id) (multislot left (default seed)) (multislot right))
         (defrule exact-fields (declare (salience 10))
           (bag (id several) (right ?first ?last))
           => (printout t \"two:\" ?first \":\" ?last crlf))
         (defrule observe (bag (id ?id) (left $?left) (right $?right))
           => (printout t ?id \" \" ?left \"|\" ?right crlf))",
    );
    engine.reset().unwrap();
    let mut restored = restored(&engine);
    for engine in [&mut engine, &mut restored] {
        engine
            .load_str(
                "(assert (bag (id singleton) (left d) (right e)))
                 (assert (bag (id several) (left d) (right e f)))
                 (assert (bag (id empty) (left) (right)))
                 (assert (bag (id omitted)))",
            )
            .unwrap();
        assert_eq!(run(engine), 5);
        assert_eq!(
            engine.get_output("t"),
            Some("two:e:f\nomitted (seed)|()\nempty ()|()\nseveral (d)|(e f)\nsingleton (d)|(e)\n")
        );
    }
}

fn item_values(engine: &Engine) -> Vec<i64> {
    let mut values: Vec<_> = engine
        .facts()
        .unwrap()
        .filter_map(|(_, fact)| match fact {
            Fact::Template(item) => match item.slots.first() {
                Some(Value::Integer(value)) => Some(*value),
                _ => None,
            },
            Fact::Ordered(_) => None,
        })
        .collect();
    values.sort_unstable();
    values
}

/// Ferric's run boundaries inside a query body (issue #327). These are not
/// CLIPS equivalence claims: CLIPS continues the query and rule body after
/// these actions. Ferric's `halt` stops the query body and finishes the outer
/// RHS; `reset` and `clear` stop both.
#[test]
fn halt_reset_and_clear_inside_a_query_body() {
    for (action, output, values, rules) in [
        ("halt", "before:10\noutside-after\n", &[20][..], 1),
        ("reset", "", &[10, 20][..], 1),
        ("clear", "", &[][..], 0),
    ] {
        let mut engine = loaded(&format!(
            "(deftemplate item (slot value))
             (deffacts seed (item (value 10)) (item (value 20)))
             (defrule probe =>
               (do-for-fact ((?f item)) TRUE
                 (retract ?f)
                 (printout t \"before:\" ?f:value crlf)
                 ({action})
                 (printout t \"inside-after\" crlf))
               (printout t \"outside-after\" crlf))"
        ));
        engine.reset().unwrap();
        let result = engine.run(RunLimit::Count(10)).unwrap();
        assert_eq!(result.halt_reason, HaltReason::HaltRequested, "{action}");
        assert_eq!(result.rules_fired, 1, "{action}");
        assert!(engine.action_diagnostics().is_empty(), "{action}");
        assert_eq!(engine.get_output("t").unwrap_or(""), output, "{action}");
        assert_eq!(item_values(&engine), values, "{action}");
        assert_eq!(engine.rules().len(), rules, "{action}");
    }
}

/// Issue #329: facts the host asserts before loading source, and after
/// `clear`, take public fact indices as CLIPS top-level assertions do:
/// `clear; assert pre10; assert pre20; load; assert item30; run` prints 3,
/// `reset; assert item30; run` prints 1, and `clear; assert pre10; load;
/// assert item30; run` prints 2. A host-made zero-field user `initial-fact`
/// follows the same policy.
#[test]
fn host_assertions_take_public_fact_indices() {
    const SOURCE: &str = "(deftemplate item (slot value))
        (defrule report => (do-for-fact ((?f item)) TRUE (printout t (fact-index ?f) crlf)))";
    for user_initial_fact in [false, true] {
        let preload = |engine: &mut Engine| {
            if user_initial_fact {
                engine.assert_ordered("initial-fact", ()).unwrap();
            } else {
                engine.assert_ordered("pre", 10_i64).unwrap();
            }
        };
        let mut observed = String::new();
        let mut engine = Engine::new(EngineConfig::utf8());
        preload(&mut engine);
        engine.assert_ordered("pre", 20_i64).unwrap();
        engine.load_str(SOURCE).unwrap();
        assert_eq!(engine.fact_count(), 2, "source load keeps both user facts");
        for step in 0..3 {
            if step == 1 {
                engine.reset().unwrap();
            } else if step == 2 {
                engine.clear();
                preload(&mut engine);
                engine.load_str(SOURCE).unwrap();
            }
            engine
                .assert_template("item", &["value"], [30_i64])
                .unwrap();
            assert_eq!(run(&mut engine), 1);
            observed.push_str(engine.get_output("t").unwrap_or(""));
        }
        assert_eq!(engine.fact_count(), 2);
        assert_eq!(
            observed, "3\n1\n2\n",
            "user initial-fact: {user_initial_fact}"
        );
    }
}

/// Issue #321 (PR #352): host retractions and reassertions update the fixed
/// prefix sequence indexes of `patterns/046`, including the partitions and
/// filtered descendants of rows removed before the first run.
#[test]
fn sequence_prefix_indexes_follow_host_retraction_and_reassertion() {
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

    fn handle(engine: &Engine, relation: &str, expected: &[i64]) -> FactHandle {
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

    fn check(engine: &mut Engine, firings: usize, counts: &[i64; 12]) {
        assert_eq!(run(engine), firings);
        for (name, expected) in GLOBALS.into_iter().zip(counts) {
            assert!(
                matches!(engine.get_global(name), Some(Value::Integer(value)) if value == expected),
                "{name}: {:?}, expected {expected}",
                engine.get_global(name)
            );
        }
        assert_eq!(engine.get_output("t").unwrap_or(""), REMOVED_OUTPUT);
    }

    let source = std::fs::read_to_string(
        super::corpus_root().join("patterns/046_ordered_sequence_prefix_indexing.clp"),
    )
    .unwrap();
    let mut engine = loaded(&source);
    engine.reset().unwrap();
    let empty = handle(&engine, "tail-after", &[3]);
    let repeated = handle(&engine, "split-after", &REPEATED_ROW);
    engine.retract(empty).unwrap();
    engine.retract(repeated).unwrap();
    check(&mut engine, 285, &AFTER_REMOVAL);

    // Removed rows and completed matches must survive a restore.
    let mut engine = restored(&engine);
    engine
        .assert_ordered("tail-after", vec![Value::Integer(3)])
        .unwrap();
    engine
        .assert_ordered("split-after", REPEATED_ROW.map(Value::Integer).to_vec())
        .unwrap();
    // The empty tail yields one match; the repeated row recreates both
    // partitions and the descendant that passes the later TEST filters.
    check(&mut engine, 4, &COMPLETE);

    let key = handle(&engine, "key-after", &[7, 207]);
    engine.retract(key).unwrap();
    engine
        .assert_ordered("key-after", vec![Value::Integer(7), Value::Integer(207)])
        .unwrap();
    // A new parent recreates two tails, three partitions and one filtered
    // descendant against the resident sequence facts.
    check(&mut engine, 6, &AFTER_NEW_KEY);
}
