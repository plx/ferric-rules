//! Named dormant seeds, deterministic reset order, and protected initial state.
use ferric_rules_core::{ConflictResolutionStrategy as Strategy, Fact};
use ferric_rules_runtime::{Engine, EngineConfig, HaltReason, RunLimit, Value};

fn run(engine: &mut Engine, count: usize) {
    let result = engine.run(RunLimit::Unlimited).unwrap();
    assert_eq!(
        result.rules_fired,
        count,
        "{:?}",
        engine.action_diagnostics()
    );
    assert_eq!(
        result.halt_reason,
        HaltReason::AgendaEmpty,
        "{:?}",
        engine.action_diagnostics()
    );
    #[cfg(debug_assertions)]
    engine.debug_assert_consistency();
}

fn integer_facts(engine: &Engine, relation: &str) -> Vec<i64> {
    engine
        .facts()
        .unwrap()
        .filter(|(_, fact)| matches!(fact, Fact::Ordered(fact) if engine.resolve_symbol(fact.relation) == Some(relation)))
        .map(|(_, fact)| {
            let Fact::Ordered(fact) = fact else {
                panic!("ordered fact required")
            };
            let [Value::Integer(value)] = fact.fields.as_slice() else {
                panic!("one integer required")
            };
            *value
        })
        .collect()
}

#[test]
fn loading_named_seeds_is_dormant_and_replacement_changes_only_the_next_reset() {
    let mut engine = Engine::new(EngineConfig::default());
    let loaded = engine.load_str("(deffacts seed (item 1))").unwrap();
    assert!(loaded.asserted_facts.is_empty());
    assert_eq!(engine.facts().unwrap().count(), 0);
    engine.reset().unwrap();
    assert_eq!(integer_facts(&engine, "item"), [1]);
    engine.load_str("(deffacts seed (item 2))").unwrap();
    assert_eq!(integer_facts(&engine, "item"), [1]);
    for _ in 0..3 {
        engine.reset().unwrap();
        assert_eq!(integer_facts(&engine, "item"), [2]);
    }
}

#[test]
fn failed_seed_replacement_keeps_old_definition_and_never_asserts_a_partial_prefix() {
    let mut engine =
        Engine::with_rules("(deftemplate record (slot key)) (deffacts seed (item 1))").unwrap();
    for invalid in [
        "(record (missing 2))",
        "(record (key 1 2))",
        "(item ?missing)",
    ] {
        assert!(engine
            .load_str(&format!("(deffacts seed (partial 8) {invalid})"))
            .is_err());
        assert!(engine.find_facts("partial").unwrap().is_empty());
        assert_eq!(integer_facts(&engine, "item"), [1]);
        engine.reset().unwrap();
        assert_eq!(integer_facts(&engine, "item"), [1]);
        assert!(engine.find_facts("partial").unwrap().is_empty());
    }
}

#[test]
fn replacement_moves_to_the_end_of_its_module_definition_order() {
    let mut engine = Engine::with_rules(
        "(deffacts first (item 1)) (deffacts second (item 2)) (deffacts first (item 3))",
    )
    .unwrap();
    for _ in 0..3 {
        assert_eq!(integer_facts(&engine, "item"), [2, 3]);
        engine.reset().unwrap();
    }
}

#[test]
fn reset_traverses_modules_then_their_current_definitions() {
    let mut engine = Engine::with_rules(
        "(defmodule A) (deftemplate A::item (slot value))
        (deffacts A::first (item (value 1))) (deffacts MAIN::main (main 2))
        (defmodule B) (deftemplate B::item (slot value)) (deffacts B::first (item (value 3)))
        (deffacts A::second (item (value 4)))",
    )
    .unwrap();
    for _ in 0..3 {
        let values: Vec<_> = engine
            .facts()
            .unwrap()
            .map(|(_, fact)| {
                let fields = match fact {
                    Fact::Ordered(fact) => fact.fields.as_slice(),
                    Fact::Template(fact) => &fact.slots,
                };
                let [Value::Integer(value)] = fields else {
                    panic!("integer seed required")
                };
                *value
            })
            .collect();
        assert_eq!(values, [2, 1, 4, 3]);
        engine.reset().unwrap();
    }
}

#[test]
fn reset_creates_root_matches_before_seed_matches_for_depth_and_breadth() {
    for (strategy, expected) in [(Strategy::Depth, "FE"), (Strategy::Breadth, "EF")] {
        let mut config = EngineConfig::default();
        config.strategy = strategy;
        let mut engine = Engine::new(config);
        engine.load_str("(defrule empty => (printout t E)) (defrule seeded (foo) => (printout t F)) (deffacts seed (foo))").unwrap();
        assert_eq!(
            engine.agenda_len(),
            1,
            "only the non-fact root is active before reset"
        );
        for _ in 0..3 {
            engine.reset().unwrap();
            run(&mut engine, 2);
            assert_eq!(engine.get_output("t"), Some(expected));
        }
    }
}

#[test]
fn template_seed_multislots_preserve_all_typed_fields_and_empty_values() {
    let mut engine = Engine::with_rules(r#"(deftemplate packet (multislot items) (multislot empty))
        (deffacts seed (packet (items red blue 3.5) (empty)))
        (defrule inspect (packet (items $?items) (empty $?empty)) => (printout t (length$ ?items) ":" (length$ ?empty)))"#).unwrap();
    run(&mut engine, 1);
    assert_eq!(engine.get_output("t"), Some("3:0"));
    let (_, Fact::Template(fact)) = engine.facts().unwrap().next().unwrap() else {
        panic!("template required")
    };
    let Value::Multifield(fields) = &fact.slots[0] else {
        panic!("multifield required")
    };
    assert!(
        matches!(fields.as_slice(), [Value::Symbol(_), Value::Symbol(_), Value::Float(value)] if value.to_bits() == 3.5_f64.to_bits())
    );
}

#[test]
fn undeffacts_removes_only_the_selected_named_seed_without_retracting_current_facts() {
    let mut engine = Engine::with_rules("(deffacts first (item 1)) (deffacts second (item 2)) (defrule remove => (undeffacts first) (undefrule remove))").unwrap();
    run(&mut engine, 1);
    assert_eq!(integer_facts(&engine, "item"), [1, 2]);
    engine.reset().unwrap();
    assert_eq!(integer_facts(&engine, "item"), [2]);
}

#[cfg(feature = "serde")]
#[test]
fn restored_named_definitions_keep_chronology_and_replacement_identity() {
    use ferric_rules_runtime::SerializationFormat;
    let mut engine = Engine::with_rules(
        "(deffacts first (item 1)) (deffacts second (item 2)) (deffacts first (item 3))",
    )
    .unwrap();
    for &format in SerializationFormat::ALL {
        let mut restored = Engine::deserialize(&engine.serialize(format).unwrap(), format).unwrap();
        restored.reset().unwrap();
        assert_eq!(integer_facts(&restored, "item"), [2, 3]);
        restored.load_str("(deffacts second (item 4))").unwrap();
        restored.reset().unwrap();
        assert_eq!(integer_facts(&restored, "item"), [3, 4]);
    }
    engine.clear();
    engine.reset().unwrap();
    assert_eq!(engine.facts().unwrap().count(), 0);
}

#[test]
fn undeffacts_local_wildcard_and_qualified_names_preserve_other_modules() {
    for (selector, expected) in [
        ("same", vec![1, 2]),
        ("*", vec![1, 2]),
        ("A::same", vec![1, 3]),
    ] {
        let mut engine = Engine::with_rules(&format!(
            "(deffacts MAIN::same (item 1))
             (defmodule A) (deffacts same (item 2))
             (defmodule B) (deffacts same (item 3))
             (defrule B::remove => (undeffacts {selector}) (undefrule remove))"
        ))
        .unwrap();
        engine.push_focus("B").unwrap();
        run(&mut engine, 1);
        assert_eq!(integer_facts(&engine, "item"), [1, 2, 3]);
        engine.reset().unwrap();
        assert_eq!(integer_facts(&engine, "item"), expected, "{selector}");
    }
}
