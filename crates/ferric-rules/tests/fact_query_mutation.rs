//! Issue #327: action-query members and ordinary aliases can mutate facts.
//!
//! All fixture outputs, including the invalid shadow diagnostic, were verified
//! byte-for-byte against CLIPS 6.30 using the pinned image
//! `sha256:b4b99ba2f08102f9c18743c6adaecb5ab82789ddfa8628693cabfeced461a929`
//! and `docker run --rm -i IMAGE -f2 /dev/stdin`. Normal runs load/reset/run;
//! late runs load the prefix before the first defrule, reset, then install/run.
//! The completed-resume fixture removes its seeds, asserts 40/50, retracts 40,
//! reasserts 40, installs its observer and runs, then resets and runs again.
//! Ferric additionally restores snapshots after the completed removal. The
//! partial-invalid-target oracle prints surviving facts after its failed run;
//! Ferric checks those facts directly through the host API.
//!
//! Compact query slots retain the selected fact's data after retraction. These
//! tests do not extend that contract to explicit stale `fact-slot-value` calls.
//! The halt/reset/clear tests below preserve Ferric's existing engine boundaries;
//! their outputs are deliberately separate from the CLIPS fixture goldens.

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
            source: include_str!(concat!("fixtures/queries/mutation_query_", $name, ".clp")),
            output: include_str!(concat!("fixtures/queries/mutation_query_", $name, ".out")),
        }
    };
}

const ORDINARY: &[Fixture] = &[
    fixture!("alias"),
    fixture!("alias_modify_duplicate"),
    fixture!("append_during_query_delayed"),
    fixture!("append_during_query_immediate"),
    fixture!("control_return"),
    fixture!("delayed_duplicate"),
    fixture!("delayed_modify"),
    fixture!("delayed_modify_shared_member"),
    fixture!("delayed_repeated_tuple_aliases"),
    fixture!("delayed_retained_stale_existence"),
    fixture!("delayed_retained_stale_versus_replacement"),
    fixture!("downstream_rete"),
    fixture!("empty_and_no_match"),
    fixture!("immediate_retract_current_inner"),
    fixture!("immediate_retract_current_outer"),
    fixture!("loop_alias"),
    fixture!("nested_scope_mutation"),
    fixture!("original"),
    fixture!("predicate_global_delayed"),
    fixture!("predicate_global_immediate"),
    fixture!("progn_other_address"),
    fixture!("reassert_future_delayed"),
    fixture!("reassert_future_immediate"),
    fixture!("reassert_future_tuples_delayed"),
    fixture!("reassert_future_tuples_immediate"),
    fixture!("repeated_retract"),
    fixture!("retract_target_predicate_before_later_effect"),
    fixture!("retract_target_predicate_not_after_later_effect"),
    fixture!("retract_future_delayed"),
    fixture!("retract_future_immediate"),
    fixture!("self_retract_compact"),
    fixture!("shadow_lhs"),
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

fn run_expected(engine: &mut Engine, expected_firings: usize, context: &str) {
    let result = engine.run(RunLimit::Count(10)).unwrap();
    assert!(
        engine.action_diagnostics().is_empty(),
        "{context}: {:?}",
        engine.action_diagnostics()
    );
    assert_eq!(result.halt_reason, HaltReason::AgendaEmpty, "{context}");
    assert_eq!(result.rules_fired, expected_firings, "{context}");
}

fn assert_fixture_output(engine: &mut Engine, fixture: &Fixture) {
    let expected_firings = match fixture.name {
        "downstream_rete" => 3,
        "retract_target_predicate_before_later_effect" => 2,
        _ => 1,
    };
    run_expected(engine, expected_firings, fixture.name);
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

golden_test!(alias, "alias");
golden_test!(alias_modify_duplicate, "alias_modify_duplicate");
golden_test!(append_during_query_delayed, "append_during_query_delayed");
golden_test!(
    append_during_query_immediate,
    "append_during_query_immediate"
);
golden_test!(control_return, "control_return");
golden_test!(delayed_duplicate, "delayed_duplicate");
golden_test!(delayed_modify, "delayed_modify");
golden_test!(delayed_modify_shared_member, "delayed_modify_shared_member");
golden_test!(
    delayed_repeated_tuple_aliases,
    "delayed_repeated_tuple_aliases"
);
golden_test!(
    delayed_retained_stale_existence,
    "delayed_retained_stale_existence"
);
golden_test!(
    delayed_retained_stale_versus_replacement,
    "delayed_retained_stale_versus_replacement"
);
golden_test!(downstream_rete, "downstream_rete");
golden_test!(empty_and_no_match, "empty_and_no_match");
golden_test!(
    immediate_retract_current_inner,
    "immediate_retract_current_inner"
);
golden_test!(
    immediate_retract_current_outer,
    "immediate_retract_current_outer"
);
golden_test!(loop_alias, "loop_alias");
golden_test!(nested_scope_mutation, "nested_scope_mutation");
golden_test!(original, "original");
golden_test!(predicate_global_delayed, "predicate_global_delayed");
golden_test!(predicate_global_immediate, "predicate_global_immediate");
golden_test!(progn_other_address, "progn_other_address");
golden_test!(reassert_future_delayed, "reassert_future_delayed");
golden_test!(reassert_future_immediate, "reassert_future_immediate");
golden_test!(
    reassert_future_tuples_delayed,
    "reassert_future_tuples_delayed"
);
golden_test!(
    reassert_future_tuples_immediate,
    "reassert_future_tuples_immediate"
);
golden_test!(repeated_retract, "repeated_retract");
golden_test!(retract_future_delayed, "retract_future_delayed");
golden_test!(retract_future_immediate, "retract_future_immediate");
golden_test!(self_retract_compact, "self_retract_compact");
golden_test!(shadow_lhs, "shadow_lhs");
golden_test!(
    matching_runs_before_evaluating_the_next_retract_target,
    "retract_target_predicate_before_later_effect"
);
golden_test!(
    later_target_effect_does_not_retroactively_change_prior_matching,
    "retract_target_predicate_not_after_later_effect"
);

#[test]
fn late_rule_installation_mutates_existing_facts() {
    for fixture in ORDINARY {
        let (mut engine, rules) = before_rule_installation(fixture);
        load(&mut engine, &rules, fixture.name);
        assert_fixture_output(&mut engine, fixture);
    }
}

fn item_values(engine: &Engine) -> Vec<i64> {
    let mut values: Vec<_> = engine
        .facts()
        .unwrap()
        .filter_map(|(_, fact)| match fact {
            Fact::Template(template) => match template.slots.first() {
                Some(Value::Integer(value)) => Some(*value),
                _ => None,
            },
            Fact::Ordered(_) => None,
        })
        .collect();
    values.sort_unstable();
    values
}

fn assert_invalid_shadow(engine: &mut Engine) {
    let result = engine.run(RunLimit::Count(10)).unwrap();
    assert_eq!(result.halt_reason, HaltReason::ActionError);
    assert!(
        engine
            .action_diagnostics()
            .iter()
            .any(|error| error.to_string().contains("retract")),
        "{:?}",
        engine.action_diagnostics()
    );
    // An invalid ordinary loop value must not fall back to the compact query
    // member's address and retract that different, still-live fact.
    assert_eq!(item_values(engine), [10, 20]);
    assert_eq!(engine.get_output("t").unwrap_or(""), "");
}

#[test]
fn invalid_ordinary_loop_target_does_not_retract_the_query_member() {
    let fixture = fixture!("shadow_progn_invalid");
    assert_invalid_shadow(&mut pending(&fixture));
    let (mut engine, rules) = before_rule_installation(&fixture);
    load(&mut engine, &rules, fixture.name);
    assert_invalid_shadow(&mut engine);
}

fn assert_partial_invalid_target(engine: &mut Engine) {
    let result = engine.run(RunLimit::Count(10)).unwrap();
    assert_eq!(result.halt_reason, HaltReason::ActionError);
    assert!(
        engine.action_diagnostics().iter().any(|error| {
            let message = error.to_string();
            message.contains("retract") && message.contains("fact-address")
        }),
        "{:?}",
        engine.action_diagnostics()
    );
    assert_eq!(engine.get_output("t").unwrap_or(""), "");
    let survivors: Vec<_> = engine
        .facts()
        .unwrap()
        .filter_map(|(_, fact)| match fact {
            Fact::Template(template) => match template.slots.first() {
                Some(Value::Symbol(kind)) => engine.resolve_core_symbol(*kind),
                _ => None,
            },
            Fact::Ordered(_) => None,
        })
        .collect();
    // Target evaluation is sequential: the first removal is committed before
    // the second target fails, and no subsequent RHS expression executes.
    assert_eq!(survivors, ["second"]);
}

#[test]
fn later_invalid_retract_target_preserves_the_completed_first_removal() {
    let fixture = fixture!("retract_partial_invalid_target");
    assert_partial_invalid_target(&mut pending(&fixture));
    let (mut engine, rules) = before_rule_installation(&fixture);
    load(&mut engine, &rules, fixture.name);
    assert_partial_invalid_target(&mut engine);
}

fn after_completed_mutation() -> Engine {
    let fixture = fixture!("completed_resume");
    let (prefix, _) = fixture
        .source
        .split_once(";; RESUME AFTER MUTATION\n")
        .unwrap();
    let mut engine = Engine::new(EngineConfig::utf8());
    load(&mut engine, prefix, fixture.name);
    engine.reset().unwrap();
    run_expected(&mut engine, 1, "completed removal");
    assert_eq!(engine.get_output("t").unwrap_or(""), "removed:3:FALSE\n");
    assert!(item_values(&engine).is_empty());
    engine
}

fn finish_after_completed_mutation(engine: &mut Engine) {
    let fixture = fixture!("completed_resume");
    let (_, suffix) = fixture
        .source
        .split_once(";; RESUME AFTER MUTATION\n")
        .unwrap();
    let first = engine.assert_template("item", &["value"], [40]).unwrap();
    engine.assert_template("item", &["value"], [50]).unwrap();
    engine.retract(first).unwrap();
    engine.assert_template("item", &["value"], [40]).unwrap();
    load(engine, suffix, fixture.name);
    engine
        .assert_ordered("observe", Vec::<Value>::new())
        .unwrap();
    run_expected(engine, 1, "fresh assertions after completed removal");
    assert_eq!(
        engine.get_output("t").unwrap_or(""),
        "removed:3:FALSE\nresumed:2:50:40:\n"
    );
    assert_eq!(item_values(engine), [40, 50]);
    let before_reset = engine.get_output("t").unwrap_or("").to_owned();
    engine.reset().unwrap();
    run_expected(engine, 1, "removal after reset");
    assert_eq!(
        format!("{before_reset}{}", engine.get_output("t").unwrap_or("")),
        fixture.output
    );
    assert!(item_values(engine).is_empty());
    assert_eq!(engine.run(RunLimit::Count(10)).unwrap().rules_fired, 0);
}

#[test]
fn completed_mutation_allows_fresh_assert_retract_reassert_and_reset() {
    finish_after_completed_mutation(&mut after_completed_mutation());
}

fn engine_with_boundary_action(action: &str) -> Engine {
    let source = format!(
        r#"
        (deftemplate item (slot value))
        (deffacts seed (item (value 10)) (item (value 20)))
        (defrule probe =>
          (do-for-fact ((?f item)) TRUE
            (retract ?f)
            (printout t "before:" ?f:value crlf)
            ({action})
            (printout t "inside-after" crlf))
          (printout t "outside-after" crlf))
        "#
    );
    let mut engine = Engine::new(EngineConfig::utf8());
    load(&mut engine, &source, action);
    engine.reset().unwrap();
    engine
}

fn assert_boundary_stops_run(engine: &mut Engine) {
    let result = engine.run(RunLimit::Count(10)).unwrap();
    assert_eq!(result.halt_reason, HaltReason::HaltRequested);
    assert_eq!(result.rules_fired, 1);
    assert!(engine.action_diagnostics().is_empty());
}

// These are Ferric engine-boundary regressions, not CLIPS equivalence claims.
// CLIPS continues the remaining query/rule body after these control forms.
// Ferric halt stops the query body, then completes the remaining outer RHS;
// reset and clear stop the outer RHS as well.
#[test]
fn halt_preserves_ferric_boundary_after_the_completed_retraction() {
    let mut engine = engine_with_boundary_action("halt");
    assert_boundary_stops_run(&mut engine);
    assert_eq!(
        engine.get_output("t").unwrap_or(""),
        "before:10\noutside-after\n"
    );
    assert_eq!(item_values(&engine), [20]);
}

#[test]
fn reset_preserves_ferric_boundary_and_reseeds_facts() {
    let mut engine = engine_with_boundary_action("reset");
    assert_boundary_stops_run(&mut engine);
    assert_eq!(engine.get_output("t").unwrap_or(""), "");
    assert_eq!(item_values(&engine), [10, 20]);
    assert_eq!(engine.rules().len(), 1);
}

#[test]
fn clear_preserves_ferric_boundary_and_removes_constructs() {
    let mut engine = engine_with_boundary_action("clear");
    assert_boundary_stops_run(&mut engine);
    assert_eq!(engine.get_output("t").unwrap_or(""), "");
    assert!(item_values(&engine).is_empty());
    assert!(engine.rules().is_empty());
}

#[cfg(feature = "serde")]
#[test]
fn pending_mutation_queries_restore_in_all_formats() {
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
fn late_mutation_queries_install_after_restore_in_all_formats() {
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
fn invalid_mutation_targets_survive_pending_and_late_restore_in_all_formats() {
    use ferric_rules::runtime::SerializationFormat;
    type ErrorCase = (Fixture, fn(&mut Engine));
    let cases: &[ErrorCase] = &[
        (fixture!("shadow_progn_invalid"), assert_invalid_shadow),
        (
            fixture!("retract_partial_invalid_target"),
            assert_partial_invalid_target,
        ),
    ];
    for (fixture, check) in cases {
        let engine = pending(fixture);
        let (before_installation, rules) = before_rule_installation(fixture);
        for &format in SerializationFormat::ALL {
            let mut restored =
                Engine::deserialize(&engine.serialize(format).unwrap(), format).unwrap();
            check(&mut restored);
            let mut restored =
                Engine::deserialize(&before_installation.serialize(format).unwrap(), format)
                    .unwrap();
            load(&mut restored, &rules, fixture.name);
            check(&mut restored);
        }
    }
}

#[cfg(feature = "serde")]
#[test]
fn completed_mutation_restores_and_rebuilds_live_traversal_in_all_formats() {
    use ferric_rules::runtime::SerializationFormat;
    let engine = after_completed_mutation();
    for &format in SerializationFormat::ALL {
        let mut restored = Engine::deserialize(&engine.serialize(format).unwrap(), format).unwrap();
        finish_after_completed_mutation(&mut restored);
    }
}
