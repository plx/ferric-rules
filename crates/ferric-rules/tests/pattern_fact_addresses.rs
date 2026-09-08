//! Issue #328: assigned-pattern addresses are available in RHS expressions.
//!
//! Exact fixture outputs were checked using CLIPS 6.30 (3/17/15), Docker image
//! `sha256:b4b99ba2f08102f9c18743c6adaecb5ab82789ddfa8628693cabfeced461a929`,
//! with `docker run --rm -i IMAGE -f2 /dev/stdin`. Ordinary runs append
//! `(reset) (run) (exit)` to the complete source. Late-install runs load the
//! source before its first `defrule`, reset, then load the rule suffix and run.
//! Both protocols produce the same goldens. The resume fixture instead loads
//! the source before `RESUME AFTER RETRACTION`, resets and runs capture, asserts
//! `(sample b)`, loads the remaining rule, and runs again.
//!
//! The suite covers RHS introspection, aliases, and scoped evaluation. It does
//! not require mutation through aliases or address expressions in LHS tests.

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
            source: include_str!(concat!("fixtures/queries/pattern_address_", $name, ".clp")),
            output: include_str!(concat!("fixtures/queries/pattern_address_", $name, ".out")),
        }
    };
}

const ORDINARY: &[Fixture] = &[
    fixture!("fact_index_address"),
    fixture!("fact_existence_lifecycle"),
    fixture!("fact_relation_ordered"),
    fixture!("fact_relation_template"),
    fixture!("fact_slot_value_single"),
    fixture!("fact_slot_value_multislot"),
    fixture!("fact_slot_value_implied"),
    fixture!("fact_slot_names_template"),
    fixture!("fact_slot_names_implied"),
    fixture!("nested_contexts"),
    fixture!("shadowing"),
    fixture!("local_shadowing"),
    fixture!("positive_mapping"),
    fixture!("or_left"),
    fixture!("or_right"),
    fixture!("alias"),
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
        engine.get_output("t").unwrap_or(""),
        fixture.output,
        "{}",
        fixture.name
    );
    assert_eq!(engine.run(RunLimit::Count(10)).unwrap().rules_fired, 0);
    assert_eq!(engine.get_output("t").unwrap_or(""), fixture.output);
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

golden_test!(original_fact_index_acceptance, "fact_index_address");
golden_test!(
    original_fact_existence_lifecycle_acceptance,
    "fact_existence_lifecycle"
);
golden_test!(
    original_ordered_fact_relation_acceptance,
    "fact_relation_ordered"
);
golden_test!(
    original_template_fact_relation_acceptance,
    "fact_relation_template"
);
golden_test!(
    original_single_slot_value_acceptance,
    "fact_slot_value_single"
);
golden_test!(
    original_multislot_value_acceptance,
    "fact_slot_value_multislot"
);
golden_test!(
    original_implied_slot_value_acceptance,
    "fact_slot_value_implied"
);
golden_test!(
    original_template_slot_names_acceptance,
    "fact_slot_names_template"
);
golden_test!(
    original_implied_slot_names_acceptance,
    "fact_slot_names_implied"
);
golden_test!(
    nested_action_conditions_bounds_queries_and_callables_see_addresses,
    "nested_contexts"
);
golden_test!(
    query_and_loop_names_shadow_and_restore_pattern_addresses,
    "shadowing"
);
golden_test!(
    query_and_loop_scopes_restore_prior_rhs_locals_and_generated_indices,
    "local_shadowing"
);
golden_test!(
    positive_address_mapping_skips_negative_exists_and_test_pass_throughs,
    "positive_mapping"
);
golden_test!(or_left_branch_uses_its_own_positive_fact_mapping, "or_left");
golden_test!(
    or_right_branch_counts_its_additional_positive_pattern,
    "or_right"
);
golden_test!(
    alias_introspection_survives_rebinding_the_original_variable,
    "alias"
);

#[test]
fn late_rule_installation_binds_addresses_of_existing_facts() {
    for fixture in ORDINARY {
        let (mut engine, rules) = before_rule_installation(fixture);
        load(&mut engine, &rules, fixture.name);
        assert_fixture_output(&mut engine, fixture);
    }
}

fn captured_and_retracted() -> Engine {
    let fixture = fixture!("resume");
    let (capture, _) = fixture
        .source
        .split_once(";; RESUME AFTER RETRACTION\n")
        .unwrap();
    let mut engine = Engine::new(EngineConfig::utf8());
    load(&mut engine, capture, fixture.name);
    engine.reset().unwrap();
    run_one(&mut engine, fixture.name);
    assert_eq!(
        engine.get_output("t").unwrap_or(""),
        "live:1:TRUE\nstale:-1:FALSE\n"
    );
    assert_eq!(engine.fact_count(), 0);
    engine
}

fn resume_with_replacement(engine: &mut Engine) {
    let fixture = fixture!("resume");
    let (_, resumed) = fixture
        .source
        .split_once(";; RESUME AFTER RETRACTION\n")
        .unwrap();
    let replacement = engine.intern_symbol("b").unwrap();
    engine.assert_ordered("sample", replacement).unwrap();
    load(engine, resumed, fixture.name);
    assert_fixture_output(engine, &fixture);
    assert_eq!(engine.fact_count(), 1);
}

#[test]
fn captured_address_stays_stale_after_retraction_and_a_new_assertion() {
    resume_with_replacement(&mut captured_and_retracted());
}

#[cfg(feature = "serde")]
#[test]
fn pending_pattern_address_activations_restore_in_all_formats() {
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
fn late_pattern_address_rule_installation_after_restore_works_in_all_formats() {
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
fn captured_stale_pattern_address_resumes_in_all_formats() {
    use ferric_rules::runtime::SerializationFormat;
    let engine = captured_and_retracted();
    for &format in SerializationFormat::ALL {
        let mut restored = Engine::deserialize(&engine.serialize(format).unwrap(), format).unwrap();
        resume_with_replacement(&mut restored);
    }
}
