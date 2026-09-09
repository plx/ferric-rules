//! Issue #325: compact slot references resolve in action-query scopes.
//!
//! Fixture output and diagnostic goldens were checked using CLIPS 6.30,
//! image `sha256:b4b99ba2f08102f9c18743c6adaecb5ab82789ddfa8628693cabfeced461a929`,
//! with `docker run --rm -i IMAGE -f2 /dev/stdin`. Normal installation loads
//! the source, resets, and runs. Late installation loads the prefix before
//! the first `defrule`, resets, then installs the rule and runs.
//!
//! Counts, sums, and uniquely selected facts avoid depending on query traversal
//! order. Runtime failure fixtures compare Ferric's error contract separately
//! from their reference diagnostic goldens; invalid member bindings must fail
//! during loading. Existing pattern-address and compact LHS coverage remains
//! in its own suites.

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
            source: include_str!(concat!("fixtures/queries/query_slot_", $name, ".clp")),
            output: include_str!(concat!("fixtures/queries/query_slot_", $name, ".out")),
        }
    };
}

const ORDINARY: &[Fixture] = &[
    fixture!("filter_count"),
    fixture!("cartesian_count"),
    fixture!("mixed_slots"),
    fixture!("nested_scopes"),
    fixture!("outer_locals"),
    fixture!("loop_shadowing"),
    fixture!("short_circuit"),
    fixture!("bare_predicate"),
    fixture!("control_positions"),
    fixture!("delayed_filter"),
    fixture!("unique_first"),
    fixture!("colon_named_bind"),
];

const MISSING_SLOTS: &[Fixture] = &[fixture!("missing_predicate"), fixture!("missing_body")];

fn load(engine: &mut Engine, source: &str, context: &str) {
    engine
        .load_str(source)
        .unwrap_or_else(|errors| panic!("{context}: load failed: {errors:?}"));
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
    let result = engine.run(RunLimit::Count(10)).unwrap();
    assert!(
        engine.action_diagnostics().is_empty(),
        "{}: {:?}",
        fixture.name,
        engine.action_diagnostics()
    );
    assert_eq!(
        result.halt_reason,
        HaltReason::AgendaEmpty,
        "{}",
        fixture.name
    );
    assert_eq!(result.rules_fired, 1, "{}", fixture.name);
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

fn assert_missing_slot(engine: &mut Engine, fixture: &Fixture) {
    let result = engine.run(RunLimit::Count(10)).unwrap();
    assert_eq!(
        result.halt_reason,
        HaltReason::ActionError,
        "{}",
        fixture.name
    );
    assert!(
        engine
            .action_diagnostics()
            .iter()
            .any(|error| error.to_string().contains("missing")),
        "{}: {:?}",
        fixture.name,
        engine.action_diagnostics()
    );
    assert_eq!(
        engine.get_output("t").unwrap().unwrap_or(""),
        "",
        "{}",
        fixture.name
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

golden_test!(original_single_binding_filter_counts_two, "filter_count");
golden_test!(original_cartesian_filter_counts_three, "cartesian_count");
golden_test!(
    scalar_and_multislot_reads_use_each_template_layout,
    "mixed_slots"
);
golden_test!(
    nested_distinct_and_same_query_names_restore_outer_scope,
    "nested_scopes"
);
golden_test!(
    predicates_and_bodies_see_and_restore_outer_rhs_locals,
    "outer_locals"
);
golden_test!(
    loop_locals_do_not_mask_compact_query_slot_references,
    "loop_shadowing"
);
golden_test!(
    short_circuiting_skips_missing_slots_and_preserves_effect_counts,
    "short_circuit"
);
golden_test!(
    bare_query_predicates_read_slot_truth_values,
    "bare_predicate"
);
golden_test!(
    compact_query_slots_work_in_control_expression_positions,
    "control_positions"
);
golden_test!(
    delayed_action_queries_filter_compact_slot_references,
    "delayed_filter"
);
golden_test!(do_for_fact_reads_the_uniquely_matching_fact, "unique_first");
golden_test!(
    colon_named_bind_is_not_query_member_rebinding,
    "colon_named_bind"
);

#[test]
fn late_query_rules_read_existing_facts() {
    for fixture in ORDINARY {
        let (mut engine, rules) = before_rule_installation(fixture);
        load(&mut engine, &rules, fixture.name);
        assert_fixture_output(&mut engine, fixture);
    }
}

#[test]
fn evaluated_missing_slots_report_action_errors_in_predicates_and_bodies() {
    for fixture in MISSING_SLOTS {
        assert_missing_slot(&mut pending(fixture), fixture);
        let (mut engine, rules) = before_rule_installation(fixture);
        load(&mut engine, &rules, fixture.name);
        assert_missing_slot(&mut engine, fixture);
    }
}

#[test]
fn explicit_query_member_rebinding_is_rejected_even_in_nested_control() {
    for fixture in [
        fixture!("invalid_member_bind"),
        fixture!("invalid_nested_member_bind"),
    ] {
        let mut engine = Engine::new(EngineConfig::utf8());
        let errors = engine
            .load_str(fixture.source)
            .expect_err("query members cannot be explicitly rebound");
        assert!(
            errors
                .iter()
                .any(|error| error.to_string().contains("FACTQPSR3")),
            "{}: {errors:?}",
            fixture.name
        );
        engine.reset().unwrap();
        assert_eq!(engine.run(RunLimit::Count(10)).unwrap().rules_fired, 0);
        assert_eq!(engine.get_output("t").unwrap().unwrap_or(""), "");
    }
}

#[cfg(feature = "serde")]
#[test]
fn pending_query_slot_activations_restore_in_all_formats() {
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
fn late_query_slot_rules_install_after_restore_in_all_formats() {
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
fn restored_missing_slot_accesses_keep_their_errors_in_all_formats() {
    use ferric_rules::runtime::SerializationFormat;
    for fixture in MISSING_SLOTS {
        let pending = pending(fixture);
        let (before_installation, rules) = before_rule_installation(fixture);
        for &format in SerializationFormat::ALL {
            let mut restored = Engine::deserialize(&pending.serialize(format).unwrap(), format)
                .unwrap_or_else(|error| panic!("{} / {format:?}: {error:?}", fixture.name));
            assert_missing_slot(&mut restored, fixture);
            let mut restored =
                Engine::deserialize(&before_installation.serialize(format).unwrap(), format)
                    .unwrap_or_else(|error| panic!("{} / {format:?}: {error:?}", fixture.name));
            load(&mut restored, &rules, fixture.name);
            assert_missing_slot(&mut restored, fixture);
        }
    }
}
