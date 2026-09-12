//! Issue #324: expression queries return real matching fact-address tuples.
//!
//! Exact fixture outputs and diagnostics were verified against CLIPS 6.30,
//! image `sha256:b4b99ba2f08102f9c18743c6adaecb5ab82789ddfa8628693cabfeced461a929`,
//! using `docker run --rm -i IMAGE -f2 /dev/stdin`. Normal installation loads
//! source/reset/run; late installation loads the prefix before the first
//! defrule, resets, installs the rule, and runs. The lifecycle fixture instead
//! resets, retracts item30, asserts item5 then item1, runs, resets, and runs.
//! Ferric additionally snapshots between the two new assertions.
//!
//! Diagnostic goldens document the reference contract; Ferric errors are
//! checked separately. The ambiguous-module negative is a rejected import
//! configuration, not a valid CLIPS environment with ambiguous visibility.
//! Callable positives use parameters and return expressions, independently
//! of the separate callable-local-bind issue #330.

use ferric_rules::core::{Fact, Value};
use ferric_rules::runtime::{Engine, EngineConfig, FactHandle, HaltReason, RunLimit};

struct Fixture {
    name: &'static str,
    source: &'static str,
    output: &'static str,
}

macro_rules! fixture {
    ($name:literal) => {
        Fixture {
            name: $name,
            source: include_str!(concat!("fixtures/queries/expression_query_", $name, ".clp")),
            output: include_str!(concat!("fixtures/queries/expression_query_", $name, ".out")),
        }
    };
}

const ORDINARY: &[Fixture] = &[
    fixture!("any_factp_match"),
    fixture!("any_factp_filter"),
    fixture!("find_fact_first"),
    fixture!("find_all_facts_count"),
    fixture!("find_all_facts_filter"),
    fixture!("empty_and_no_match"),
    fixture!("flattened_tuples_and_first"),
    fixture!("same_template_filtered_order"),
    fixture!("nested_distinct_predicate_scopes"),
    fixture!("nested_same_name_predicate_scope"),
    fixture!("rhs_expression_positions"),
    fixture!("deffunction_expression_positions"),
    fixture!("typed_defmethod_expression_position"),
    fixture!("predicate_global_bind_and_early_exit"),
    fixture!("outer_rhs_local_scope"),
    fixture!("module_imported"),
    fixture!("module_callable_scope"),
];

const INVALID: &[Fixture] = &[
    fixture!("predicate_local_bind_rejection"),
    fixture!("invalid_empty_bindings"),
    fixture!("invalid_duplicate_members"),
    fixture!("invalid_unknown_template"),
    fixture!("invalid_invisible_template"),
    fixture!("invalid_qualified_template"),
    fixture!("invalid_ambiguous_template"),
];

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

golden_test!(original_any_factp_match, "any_factp_match");
golden_test!(original_any_factp_filter, "any_factp_filter");
golden_test!(original_find_fact_first, "find_fact_first");
golden_test!(original_find_all_count, "find_all_facts_count");
golden_test!(original_find_all_filter, "find_all_facts_filter");
golden_test!(
    empty_templates_and_rejected_candidates_preserve_empty_shapes,
    "empty_and_no_match"
);
golden_test!(
    multiple_bindings_return_flat_tuples_in_declaration_order,
    "flattened_tuples_and_first"
);
golden_test!(
    same_template_first_tuple_uses_assertion_order,
    "same_template_filtered_order"
);
golden_test!(
    nested_queries_read_distinct_outer_members,
    "nested_distinct_predicate_scopes"
);
golden_test!(
    nested_same_name_queries_restore_outer_members,
    "nested_same_name_predicate_scope"
);
golden_test!(
    queries_work_in_nested_rhs_expression_positions,
    "rhs_expression_positions"
);
golden_test!(
    deffunction_queries_use_parameters_and_return_fact_multifields,
    "deffunction_expression_positions"
);
golden_test!(
    typed_method_queries_use_parameters_and_return_fact_multifields,
    "typed_defmethod_expression_position"
);
golden_test!(
    global_predicate_effects_are_lazy_and_first_queries_stop_early,
    "predicate_global_bind_and_early_exit"
);
golden_test!(
    query_members_mask_and_restore_ordinary_rhs_locals,
    "outer_rhs_local_scope"
);
golden_test!(
    unqualified_imported_templates_are_visible_to_queries,
    "module_imported"
);
golden_test!(
    callable_query_visibility_uses_the_defining_module,
    "module_callable_scope"
);

#[test]
fn late_rule_installation_queries_existing_facts() {
    for fixture in ORDINARY {
        let (mut engine, rules) = before_rule_installation(fixture);
        load(&mut engine, &rules, fixture.name);
        assert_fixture_output(&mut engine, fixture);
    }
}

fn assert_missing_slot(engine: &mut Engine) {
    let result = engine.run(RunLimit::Count(10)).unwrap();
    assert_eq!(result.halt_reason, HaltReason::ActionError);
    assert!(
        engine
            .action_diagnostics()
            .iter()
            .any(|error| error.to_string().contains("missing")),
        "{:?}",
        engine.action_diagnostics()
    );
    assert_eq!(engine.get_output("t").unwrap().unwrap_or(""), "");
}

#[test]
fn evaluated_missing_slots_fail_without_executing_later_actions() {
    let fixture = fixture!("missing_slot");
    assert_missing_slot(&mut pending(&fixture));
    let (mut engine, rules) = before_rule_installation(&fixture);
    load(&mut engine, &rules, fixture.name);
    assert_missing_slot(&mut engine);
}

#[test]
fn invalid_query_declarations_predicates_and_visibility_reject_loading() {
    for fixture in INVALID {
        let mut engine = Engine::new(EngineConfig::utf8());
        let errors = engine.load_str(fixture.source).unwrap_err();
        assert!(!errors.is_empty(), "{}", fixture.name);
        if fixture.name == "predicate_local_bind_rejection" {
            assert!(
                errors
                    .iter()
                    .any(|error| error.to_string().contains("FACTQPSR2")),
                "{errors:?}"
            );
        }
        assert!(engine.rules().is_empty(), "{}", fixture.name);
        engine.reset().unwrap();
        assert_eq!(engine.run(RunLimit::Count(10)).unwrap().rules_fired, 0);
        assert_eq!(engine.get_output("t").unwrap().unwrap_or(""), "");
    }
}

fn item_handle(engine: &Engine, value: i64) -> FactHandle {
    engine.facts().unwrap().find_map(|(handle, fact)| match fact {
        Fact::Template(template)
            if matches!(template.slots.first(), Some(Value::Integer(actual)) if *actual == value) => Some(handle),
        _ => None,
    }).unwrap_or_else(|| panic!("missing item {value}"))
}

fn before_lifecycle_restore() -> Engine {
    let mut engine = pending(&fixture!("lifecycle"));
    engine.retract(item_handle(&engine, 30)).unwrap();
    engine.assert_template("item", &["value"], [5]).unwrap();
    engine
}

fn finish_lifecycle(engine: &mut Engine) {
    engine.assert_template("item", &["value"], [1]).unwrap();
    run_one(engine, "lifecycle after retraction and append");
    assert_eq!(
        engine.get_output("t").unwrap().unwrap_or(""),
        "first:10\nall:4:10:20:5:1:\n"
    );
    let before_reset = engine.get_output("t").unwrap().unwrap_or("").to_owned();
    engine.reset().unwrap();
    run_one(engine, "lifecycle after reset");
    assert_eq!(
        format!(
            "{before_reset}{}",
            engine.get_output("t").unwrap().unwrap_or("")
        ),
        fixture!("lifecycle").output
    );
    assert_eq!(engine.run(RunLimit::Count(10)).unwrap().rules_fired, 0);
}

#[test]
fn retract_reassert_and_reset_preserve_assertion_order() {
    finish_lifecycle(&mut before_lifecycle_restore());
}

#[cfg(feature = "serde")]
#[test]
fn pending_expression_queries_restore_in_all_formats() {
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
fn late_expression_queries_install_after_restore_in_all_formats() {
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
fn missing_slot_errors_survive_pending_and_late_restore_in_all_formats() {
    use ferric_rules::runtime::SerializationFormat;
    let fixture = fixture!("missing_slot");
    let engine = pending(&fixture);
    let (before_installation, rules) = before_rule_installation(&fixture);
    for &format in SerializationFormat::ALL {
        let mut restored = Engine::deserialize(&engine.serialize(format).unwrap(), format).unwrap();
        assert_missing_slot(&mut restored);
        let mut restored =
            Engine::deserialize(&before_installation.serialize(format).unwrap(), format).unwrap();
        load(&mut restored, &rules, fixture.name);
        assert_missing_slot(&mut restored);
    }
}

#[cfg(feature = "serde")]
#[test]
fn query_order_after_retraction_and_post_restore_assertion_survives_all_formats() {
    use ferric_rules::runtime::SerializationFormat;
    let engine = before_lifecycle_restore();
    for &format in SerializationFormat::ALL {
        let mut restored = Engine::deserialize(&engine.serialize(format).unwrap(), format).unwrap();
        finish_lifecycle(&mut restored);
    }
}
