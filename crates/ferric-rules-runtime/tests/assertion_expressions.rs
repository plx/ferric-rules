//! Source assertions and dormant seed expressions use the normal evaluator.

use ferric_rules_core::Fact;
use ferric_rules_runtime::{Engine, EngineConfig, HaltReason, RunLimit, Value};

fn run(engine: &mut Engine, firings: usize) {
    let result = engine.run(RunLimit::Unlimited).unwrap();
    assert_eq!(result.rules_fired, firings);
    assert_eq!(result.halt_reason, HaltReason::AgendaEmpty);
    assert!(engine.action_diagnostics().is_empty());
    #[cfg(debug_assertions)]
    engine.debug_assert_consistency();
}

fn ordered<'a>(engine: &'a Engine, relation: &str) -> &'a [Value] {
    let facts = engine.find_facts(relation).unwrap();
    assert_eq!(facts.len(), 1, "{relation}");
    let Fact::Ordered(fact) = facts[0].1 else {
        panic!("expected an ordered {relation} fact");
    };
    &fact.fields
}

fn integers(engine: &Engine, relation: &str) -> Vec<i64> {
    ordered(engine, relation)
        .iter()
        .map(|value| {
            let Value::Integer(value) = value else {
                panic!("expected integer in {relation}, got {value:?}");
            };
            *value
        })
        .collect()
}

fn generated_symbol<'a>(engine: &'a Engine, relation: &str) -> &'a str {
    let [Value::Symbol(symbol)] = ordered(engine, relation) else {
        panic!("expected one symbol in {relation}");
    };
    engine.resolve_core_symbol(*symbol).unwrap()
}

#[test]
fn load_str_and_load_file_assert_the_four_expression_facts() {
    const ASSERTIONS: &str = "(assert (p (+ 1 2) ?*g* q))
        (assert (r (create$ a b) c))
        (assert (item (n (+ 1 2))))
        (assert (item (tags ?*g*)))";
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("assertions.clp");
    std::fs::write(&path, ASSERTIONS).unwrap();
    for from_file in [false, true] {
        let mut engine = Engine::with_rules(
            "(deftemplate item (slot n) (multislot tags))
             (defglobal ?*g* = 5)
             (defrule observe (p 3 5 q) (r a b c)
               (item (n 3) (tags)) (item (n nil) (tags 5))
               => (printout t \"all four\" crlf))",
        )
        .unwrap();
        let loaded = if from_file {
            engine.load_file(&path)
        } else {
            engine.load_str(ASSERTIONS)
        }
        .unwrap();
        assert!(loaded.warnings.is_empty());
        assert_eq!(loaded.asserted_facts.len(), 4);
        assert_eq!(engine.fact_count(), 4);
        run(&mut engine, 1);
        assert_eq!(engine.get_output("t"), Some("all four\n"));
    }
}

#[test]
fn source_assertions_splice_global_and_function_multifields() {
    let mut engine = Engine::with_rules(
        "(deftemplate item (slot one) (multislot many))
         (defglobal ?*many* = (create$ g1 g2))
         (deffunction values () (create$ x (create$ y z)))
         (defrule observe (row head g1 g2 x y z 3 tail)
           (item (one 5) (many head g1 g2 x y z tail))
           => (printout t \"spliced\" crlf))",
    )
    .unwrap();
    let loaded = engine
        .load_str(
            "(assert
               (row head (create$) ?*many* (values) (+ 1 2) tail)
               (item (one (+ 2 3)) (many head ?*many* (values) (create$) tail)))",
        )
        .unwrap();
    assert_eq!(loaded.asserted_facts.len(), 2);
    assert!(loaded.warnings.is_empty());
    run(&mut engine, 1);
    assert_eq!(engine.get_output("t"), Some("spliced\n"));
}

#[test]
fn one_assert_evaluates_every_fact_and_field_in_source_order() {
    let mut engine = Engine::with_rules(
        "(deftemplate item (slot one) (multislot many))
         (defglobal ?*calls* = 0)
         (deffunction next () (bind ?*calls* (+ ?*calls* 1)) ?*calls*)
         (defrule observe (first 1 2) (item (one 3) (many 4 5)) (last 6)
           => (printout t \"ordered\" crlf))",
    )
    .unwrap();
    let loaded = engine
        .load_str(
            "(assert (first (next) (next))
               (item (one (next)) (many (next) (next))) (last (next)))",
        )
        .unwrap();
    assert_eq!(loaded.asserted_facts.len(), 3);
    assert!(matches!(
        engine.get_global("calls"),
        Some(Value::Integer(6))
    ));
    run(&mut engine, 1);
    assert_eq!(engine.get_output("t"), Some("ordered\n"));
}

#[test]
fn invalid_source_fields_report_errors_without_asserting_truncated_facts() {
    for expression in ["?missing", "?*missing*", "(unknown 1)", "(+ 1 ?missing)"] {
        for fact in [
            format!("(bad before {expression} after)"),
            format!("(item (one {expression}))"),
            format!("(item (many before {expression} after))"),
        ] {
            let mut engine =
                Engine::with_rules("(deftemplate item (slot one) (multislot many))").unwrap();
            engine.load_str("(assert (kept 7))").unwrap();
            let errors = engine.load_str(&format!("(assert {fact})")).unwrap_err();
            assert!(!errors.is_empty(), "{fact}");
            assert_eq!(engine.fact_count(), 1, "{fact}");
            assert_eq!(integers(&engine, "kept"), [7]);
            assert!(engine.find_facts("bad").unwrap().is_empty());
        }
    }
}

#[test]
fn scalar_template_slots_reject_even_singleton_multifield_results() {
    for expression in ["(create$)", "(create$ 1)", "(create$ 1 2)", "(fields)"] {
        let mut engine = Engine::with_rules(
            "(deftemplate item (slot one))
             (deffunction fields () (create$ 1))",
        )
        .unwrap();
        engine.load_str("(assert (item (one 7)))").unwrap();
        assert!(engine
            .load_str(&format!("(assert (item (one {expression})))"))
            .is_err());
        assert_eq!(engine.fact_count(), 1, "{expression}");
        let handle = engine.facts().unwrap().next().unwrap().0;
        assert!(matches!(
            engine.get_fact_slot_by_name(handle, "one").unwrap(),
            Value::Integer(7)
        ));
    }
}

const DORMANT_SOURCE: &str = "(defglobal ?*base* = 5 ?*calls* = 0)
    (deffunction value () (bind ?*calls* (+ ?*calls* 1)) (+ ?*base* ?*calls*))
    (deffacts seed (row (value) (value)) (token (gensym*)))";

#[test]
fn dormant_expressions_run_on_each_reset_after_globals_and_use_current_functions() {
    let mut engine = Engine::new(EngineConfig::default());
    let loaded = engine.load_str(DORMANT_SOURCE).unwrap();
    assert!(loaded.asserted_facts.is_empty());
    assert_eq!(engine.fact_count(), 0);
    assert!(matches!(
        engine.get_global("calls"),
        Some(Value::Integer(0))
    ));

    engine.reset().unwrap();
    assert_eq!(integers(&engine, "row"), [6, 7]);
    assert_eq!(generated_symbol(&engine, "token"), "gen1");
    assert!(matches!(
        engine.get_global("calls"),
        Some(Value::Integer(2))
    ));

    engine
        .load_str("(assert (temporary (bind ?*base* 9)))")
        .unwrap();
    assert!(matches!(engine.get_global("base"), Some(Value::Integer(9))));
    engine.reset().unwrap();
    assert_eq!(integers(&engine, "row"), [6, 7]);
    assert_eq!(generated_symbol(&engine, "token"), "gen2");

    engine
        .load_str("(deffunction value () (+ ?*base* 100))")
        .unwrap();
    engine.reset().unwrap();
    assert_eq!(integers(&engine, "row"), [105, 105]);
    assert_eq!(generated_symbol(&engine, "token"), "gen3");
    assert!(matches!(
        engine.get_global("calls"),
        Some(Value::Integer(0))
    ));
}

#[test]
fn invalid_deferred_results_fail_reset_without_inserting_a_malformed_fact() {
    for source in [
        "(deffacts seed (bad ?*missing*))",
        "(deftemplate item (slot one))
         (deffunction fields () (create$ 1))
         (deffacts seed (item (one (fields))))",
    ] {
        let mut engine = Engine::new(EngineConfig::default());
        engine.load_str(source).unwrap();
        assert!(engine.reset().is_err(), "{source}");
        assert_eq!(engine.fact_count(), 0, "{source}");
    }
}

#[test]
fn reset_evaluates_seed_functions_in_each_definition_module() {
    let mut engine = Engine::new(EngineConfig::default());
    engine
        .load_str(
            "(defmodule A)
             (defglobal ?*base* = 10)
             (deffunction value () (+ ?*base* 1))
             (deftemplate A::item (slot n))
             (deffacts seed (item (n (value))))
             (defrule show (item (n ?n)) => (printout t \"A:\" ?n crlf))
             (defmodule B)
             (defglobal ?*base* = 20)
             (deffunction value () (+ ?*base* 2))
             (deftemplate B::item (slot n))
             (deffacts seed (item (n (value))))
             (defrule show (item (n ?n)) => (printout t \"B:\" ?n crlf))",
        )
        .unwrap();
    for _ in 0..2 {
        engine.reset().unwrap();
        engine.push_focus("B").unwrap();
        engine.push_focus("A").unwrap();
        run(&mut engine, 2);
        assert_eq!(engine.get_output("t"), Some("A:11\nB:22\n"));
    }
}

#[cfg(feature = "serde")]
#[test]
fn snapshots_preserve_pending_expressions_before_the_first_reset() {
    use ferric_rules_runtime::SerializationFormat;

    let mut original = Engine::new(EngineConfig::default());
    original.load_str(DORMANT_SOURCE).unwrap();
    for &format in SerializationFormat::ALL {
        let bytes = original.serialize(format).unwrap();
        let mut engine = Engine::deserialize(&bytes, format).unwrap();
        assert_eq!(engine.fact_count(), 0);
        assert!(matches!(
            engine.get_global("calls"),
            Some(Value::Integer(0))
        ));
        engine.reset().unwrap();
        assert_eq!(integers(&engine, "row"), [6, 7]);
        assert_eq!(generated_symbol(&engine, "token"), "gen1");
        engine
            .load_str("(deffunction value () (+ ?*base* 100))")
            .unwrap();
        engine.reset().unwrap();
        assert_eq!(integers(&engine, "row"), [105, 105]);
        assert_eq!(generated_symbol(&engine, "token"), "gen2");
    }
    assert_eq!(original.fact_count(), 0);
}

#[cfg(feature = "serde")]
#[test]
fn snapshots_keep_seed_globals_that_will_be_defined_before_reset() {
    use ferric_rules_runtime::SerializationFormat;

    let mut original = Engine::new(EngineConfig::default());
    original
        .load_str("(deffacts seed (row ?*late* (+ ?*late* 1)))")
        .unwrap();
    for &format in SerializationFormat::ALL {
        let bytes = original.serialize(format).unwrap();
        let mut engine = Engine::deserialize(&bytes, format).unwrap();
        engine.load_str("(defglobal ?*late* = 7)").unwrap();
        engine.reset().unwrap();
        assert_eq!(integers(&engine, "row"), [7, 8]);
    }
}

#[test]
fn one_top_level_assert_shares_local_bindings_across_its_facts() {
    let mut engine = Engine::new(EngineConfig::default());
    engine
        .load_str("(assert (first (bind ?x 3) ?x) (second (+ ?x 1)))")
        .unwrap();
    assert_eq!(integers(&engine, "first"), [3, 3]);
    assert_eq!(integers(&engine, "second"), [4]);
}

#[test]
fn deffacts_rejects_local_reads_even_after_an_earlier_bind() {
    for facts in [
        "(first (bind ?x 3) ?x)",
        "(first (bind ?x 3)) (second ?x)",
        "(first (if TRUE then (bind ?x 3) ?x))",
    ] {
        let mut engine = Engine::new(EngineConfig::default());
        assert!(engine
            .load_str(&format!("(deffacts seed {facts})"))
            .is_err());
        engine.reset().unwrap();
        assert_eq!(engine.fact_count(), 0);
    }
}

#[test]
fn load_facts_stays_literal_only_and_preserves_earlier_facts_on_failure() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("expressions.fct");
    let escaped = path
        .to_str()
        .unwrap()
        .replace('\\', "\\\\")
        .replace('"', "\\\"");
    for invalid in [
        "(bad (mark))",
        "(bad ?*g*)",
        "(bad ?local)",
        "(item (one (mark)))",
        "(item (one ?*g*))",
        "(item (many (create$ a b)))",
        "(item (many ?*g*))",
    ] {
        std::fs::write(&path, format!("(before 7)\n{invalid}\n(after 9)\n")).unwrap();
        let mut engine = Engine::with_rules(&format!(
            "(deftemplate item (slot one) (multislot many))
             (defglobal ?*g* = 5 ?*calls* = 0)
             (deffunction mark () (bind ?*calls* (+ ?*calls* 1)) ?*calls*)
             (defrule read => (load-facts \"{escaped}\"))"
        ))
        .unwrap();
        let result = engine.run(RunLimit::Unlimited).unwrap();
        assert_eq!(result.halt_reason, HaltReason::ActionError, "{invalid}");
        assert!(!engine.action_diagnostics().is_empty());
        assert_eq!(integers(&engine, "before"), [7]);
        assert!(engine.find_facts("after").unwrap().is_empty());
        assert!(engine.find_facts("bad").unwrap().is_empty());
        assert_eq!(engine.fact_count(), 1, "{invalid}");
        assert!(matches!(
            engine.get_global("calls"),
            Some(Value::Integer(0))
        ));
    }
}

fn load_both_ways(source: &str, check: impl Fn(&Engine)) {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("source.clp");
    std::fs::write(&path, source).unwrap();
    for from_file in [false, true] {
        let mut engine = Engine::new(EngineConfig::default());
        let loaded = if from_file {
            engine.load_file(&path)
        } else {
            engine.load_str(source)
        };
        assert!(loaded.is_ok(), "from_file={from_file}: {loaded:?}");
        assert_eq!(engine.current_module(), "B", "from_file={from_file}");
        check(&engine);
    }
}

fn template_slot(engine: &Engine, slot: &str) -> Vec<Value> {
    engine
        .facts()
        .unwrap()
        .filter(|(_, fact)| matches!(fact, Fact::Template(_)))
        .map(|(handle, _)| engine.get_fact_slot_by_name(handle, slot).unwrap().clone())
        .collect()
}

/// A top-level assertion runs in the module current at its source position.
/// CLIPS 6.30 batches this source and prints `f-1 (p 1)` for `(facts A)`,
/// with B left current.
#[test]
fn source_assertions_call_functions_of_the_module_current_at_their_position() {
    load_both_ways(
        "(defmodule A (export ?ALL))
         (deffunction value () 1)
         (assert (p (value)))
         (defmodule B)
         (deffunction value () 2)",
        |engine| assert_eq!(integers(engine, "p"), [1]),
    );
}

/// CLIPS 6.30 prints `f-1 (item (n 3))` and `f-2 (item (n 1))` for `(facts A)`.
#[test]
fn source_assertions_resolve_templates_of_the_module_current_at_their_position() {
    load_both_ways(
        "(defmodule A (export ?ALL))
         (deftemplate A::item (slot n))
         (assert (item (n (+ 1 2))))
         (assert (item (n 1)))
         (defmodule B)",
        |engine| {
            let mut values = template_slot(engine, "n");
            values.sort_by_key(|value| match value {
                Value::Integer(n) => *n,
                _ => panic!("expected an integer slot, got {value:?}"),
            });
            assert!(matches!(
                values.as_slice(),
                [Value::Integer(1), Value::Integer(3)]
            ));
        },
    );
}

/// CLIPS 6.30 prints `f-1 (q 5)` for `(facts A)`.
#[test]
fn source_assertions_read_globals_of_the_module_current_at_their_position() {
    load_both_ways(
        "(defmodule A (export ?ALL))
         (defglobal A ?*x* = 5)
         (assert (q ?*x*))
         (defmodule B)
         (defglobal B ?*x* = 7)",
        |engine| assert_eq!(integers(engine, "q"), [5]),
    );
}
