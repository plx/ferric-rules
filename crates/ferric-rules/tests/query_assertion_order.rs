//! Issue #326: RHS fact-query traversal follows assertion chronology.
//!
//! Goldens were verified using CLIPS 6.30 (3/17/15), Docker image
//! `sha256:b4b99ba2f08102f9c18743c6adaecb5ab82789ddfa8628693cabfeced461a929`,
//! with `docker run --rm -i IMAGE -f2 /dev/stdin`. Normal runs append
//! `(reset) (run) (exit)` to the fixture. Late runs load the source before its
//! first `defrule`, reset, then load the rule suffix and run. Both protocols
//! produce the same exact stdout for the eight ordinary fixtures.
//!
//! Lifecycle inputs load the complete fixture, reset, retract item10 via a
//! CLIPS query, assert item5, run, reset, then run again. The restored-append
//! oracle also asserts item1 after item5 before the first run; Ferric tests
//! insert a serialization roundtrip before that final assertion. Goldens
//! concatenate the output before and after reset. All outputs use fact values,
//! avoiding any dependency on the separate public `fact-index` correction.
//!
//! Query bodies only print. These tests do not broaden query mutation or
//! delayed-predicate semantics.

use ferric_rules::core::{Fact, Value};
use ferric_rules::runtime::{Engine, EngineConfig, HaltReason, RunLimit};

struct Fixture {
    name: &'static str,
    source: &'static str,
    output: &'static str,
}

macro_rules! fixture {
    ($name:literal) => {
        Fixture {
            name: $name,
            source: include_str!(concat!("fixtures/queries/query_order_", $name, ".clp")),
            output: include_str!(concat!("fixtures/queries/query_order_", $name, ".out")),
        }
    };
}

const ORDINARY: &[Fixture] = &[
    fixture!("do_for_fact_first"),
    fixture!("do_for_all_facts_order"),
    fixture!("filtered_first"),
    fixture!("nonnumeric"),
    fixture!("cross_product"),
    fixture!("same_template_product"),
    fixture!("delayed_order"),
    fixture!("duplicates"),
];

fn load(engine: &mut Engine, source: &str, context: &str) {
    engine
        .load_str(source)
        .unwrap_or_else(|errors| panic!("{context}: load failed: {errors:?}"));
}

fn run_one(engine: &mut Engine, context: &str) {
    let result = engine.run(RunLimit::Count(10)).unwrap();
    assert!(
        engine.action_diagnostics().is_empty(),
        "{context}: {:?}",
        engine.action_diagnostics()
    );
    assert_eq!(result.halt_reason, HaltReason::AgendaEmpty, "{context}");
    assert_eq!(result.rules_fired, 1, "{context}");
}

fn pending(fixture: &Fixture) -> Engine {
    let mut engine = Engine::new(EngineConfig::utf8());
    load(&mut engine, fixture.source, fixture.name);
    engine.reset().unwrap();
    engine
}

fn before_rule_installation(fixture: &Fixture) -> (Engine, String) {
    let (prefix, suffix) = fixture.source.split_once("(defrule").unwrap();
    let mut engine = Engine::new(EngineConfig::utf8());
    load(&mut engine, prefix, fixture.name);
    engine.reset().unwrap();
    (engine, format!("(defrule{suffix}"))
}

fn assert_fixture_output(engine: &mut Engine, fixture: &Fixture) {
    run_one(engine, fixture.name);
    assert_eq!(
        engine.get_output("t").unwrap().unwrap_or(""),
        fixture.output,
        "{}",
        fixture.name
    );
    assert_eq!(engine.run(RunLimit::Count(10)).unwrap().rules_fired, 0);
    assert_eq!(
        engine.get_output("t").unwrap().unwrap_or(""),
        fixture.output
    );
}

macro_rules! golden_test {
    ($test:ident, $name:literal) => {
        #[test]
        fn $test() {
            let fixture = fixture!($name);
            assert_fixture_output(&mut pending(&fixture), &fixture);
        }
    };
}

golden_test!(
    original_do_for_fact_selects_the_oldest_matching_fact,
    "do_for_fact_first"
);
golden_test!(
    original_do_for_all_facts_visits_assertion_order,
    "do_for_all_facts_order"
);
golden_test!(filtered_first_skips_earlier_nonmatches, "filtered_first");
golden_test!(symbol_values_are_visited_in_assertion_order, "nonnumeric");
golden_test!(
    cartesian_products_follow_binding_declaration_nesting,
    "cross_product"
);
golden_test!(
    same_template_bindings_include_self_pairs_in_nested_order,
    "same_template_product"
);
golden_test!(
    delayed_query_actions_follow_assertion_order,
    "delayed_order"
);

#[test]
fn suppressed_duplicates_stay_in_place_and_enabled_duplicates_append() {
    let fixture = fixture!("duplicates");
    let mut engine = pending(&fixture);
    assert_fixture_output(&mut engine, &fixture);
    assert_eq!(engine.fact_count(), 3);
    assert!(!engine.fact_duplication());
}

#[test]
fn queries_installed_after_reset_traverse_existing_facts_in_order() {
    for fixture in ORDINARY {
        let (mut engine, rules) = before_rule_installation(fixture);
        load(&mut engine, &rules, fixture.name);
        assert_fixture_output(&mut engine, fixture);
    }
}

fn with_reused_slot(fixture: &Fixture) -> Engine {
    let mut engine = pending(fixture);
    let first = engine
        .facts()
        .unwrap()
        .find_map(|(handle, fact)| match fact {
            Fact::Template(template)
                if matches!(template.slots.first(), Some(Value::Integer(10))) =>
            {
                Some(handle)
            }
            _ => None,
        })
        .expect("item10 must be present before host retraction");
    engine.retract(first).unwrap();
    // SlotMap can reuse item10's freed slot. A later timestamp must still
    // place item5 after the surviving item20 and item30 facts.
    engine.assert_template("item", &["value"], [5_i64]).unwrap();
    assert_eq!(engine.fact_count(), 3);
    engine
}

fn assert_order_then_reset(engine: &mut Engine, fixture: &Fixture) {
    run_one(engine, fixture.name);
    let mut observed = engine.get_output("t").unwrap().unwrap_or("").to_owned();
    engine.reset().unwrap();
    run_one(engine, fixture.name);
    observed.push_str(engine.get_output("t").unwrap().unwrap_or(""));
    assert_eq!(observed, fixture.output, "{}", fixture.name);
    assert_eq!(engine.fact_count(), 3);
}

#[test]
fn retraction_slot_reuse_and_reset_preserve_query_chronology() {
    let fixture = fixture!("lifecycle");
    assert_order_then_reset(&mut with_reused_slot(&fixture), &fixture);
}

fn append_after_reuse(engine: &mut Engine, fixture: &Fixture) {
    engine.assert_template("item", &["value"], [1_i64]).unwrap();
    assert_eq!(engine.fact_count(), 4);
    assert_order_then_reset(engine, fixture);
}

#[test]
fn another_assertion_follows_survivors_and_the_reused_slot() {
    let fixture = fixture!("restored_append");
    append_after_reuse(&mut with_reused_slot(&fixture), &fixture);
}

#[cfg(feature = "serde")]
#[test]
fn pending_queries_restore_assertion_order_in_every_format() {
    use ferric_rules::runtime::SerializationFormat;
    for fixture in ORDINARY {
        let engine = pending(fixture);
        for &format in SerializationFormat::ALL {
            let mut restored = Engine::deserialize(&engine.serialize(format).unwrap(), format)
                .unwrap_or_else(|error| panic!("{} / {format:?}: {error:?}", fixture.name));
            assert_fixture_output(&mut restored, fixture);
        }
    }
}

#[cfg(feature = "serde")]
#[test]
fn late_query_installation_after_restore_keeps_order_in_every_format() {
    use ferric_rules::runtime::SerializationFormat;
    for fixture in ORDINARY {
        let (engine, rules) = before_rule_installation(fixture);
        for &format in SerializationFormat::ALL {
            let mut restored = Engine::deserialize(&engine.serialize(format).unwrap(), format)
                .unwrap_or_else(|error| panic!("{} / {format:?}: {error:?}", fixture.name));
            load(&mut restored, &rules, fixture.name);
            assert_fixture_output(&mut restored, fixture);
        }
    }
}

#[cfg(feature = "serde")]
#[test]
fn reused_slot_history_accepts_post_restore_assertion_in_every_format() {
    use ferric_rules::runtime::SerializationFormat;
    let fixture = fixture!("restored_append");
    let engine = with_reused_slot(&fixture);
    for &format in SerializationFormat::ALL {
        let mut restored = Engine::deserialize(&engine.serialize(format).unwrap(), format).unwrap();
        // The host handle used before serialization is intentionally not kept;
        // the new assertion belongs to the restored engine's own handle space.
        append_after_reuse(&mut restored, &fixture);
    }
}
