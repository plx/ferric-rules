//! Issue #330: callable local bindings last for one invocation.
//!
//! Exact fixture stdout was verified against CLIPS 6.30 using image
//! `sha256:b4b99ba2f08102f9c18743c6adaecb5ab82789ddfa8628693cabfeced461a929`
//! and `docker run --rm -i IMAGE -f2 /dev/stdin`. Normal runs load/reset/run;
//! late calls load the prefix before the first defrule, reset, then install/run.
//! Late definitions reset an empty engine before loading/resetting/running the
//! complete source. Staged fixtures run the prefix to completion, then install
//! and run the suffix. Ferric also snapshots at each corresponding boundary.
//!
//! Error goldens document the reference diagnostics; Ferric checks unbound
//! variable errors and skipped later actions without matching diagnostic text.
//! The untouched runtime tests retain existing global-bind arity/target policy.
//! Callable iterator rebinding is checked at load time; generated index names
//! remain ordinary bind targets with separate lexical reads during iteration.
//! Adjacent undeclared-variable loader checks and no-value global reset
//! semantics remain outside these fixtures.

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
            source: include_str!(concat!("fixtures/callables/local_bind_", $name, ".clp")),
            output: include_str!(concat!("fixtures/callables/local_bind_", $name, ".out")),
        }
    };
}

const ORDINARY: &[Fixture] = &[
    fixture!("branches_and_last_bind_value"),
    fixture!("colon_local_and_compact_member"),
    fixture!("early_return_isolation"),
    fixture!("generic_method_local_and_next"),
    fixture!("globals_and_locals_remain_distinct"),
    fixture!("loop_progn_and_foreach_scopes"),
    fixture!("multifield_local_and_rest_parameter"),
    fixture!("nested_and_caller_isolation"),
    fixture!("nested_bind_values"),
    fixture!("original_local"),
    fixture!("original_parameter"),
    fixture!("parameter_unbind_falls_back"),
    fixture!("progn_generated_index_scope"),
    fixture!("progn_index_bind_runtime"),
    fixture!("query_callee_compact_isolation"),
    fixture!("query_member_shadows_parameter"),
    fixture!("query_member_shadows_rebound_parameter"),
    fixture!("query_predicate_calls_local_function"),
    fixture!("rebound_iterator_parameter_scopes"),
    fixture!("recursive_frame_isolation"),
    fixture!("rest_aliases"),
    fixture!("sibling_argument_local_updates"),
    fixture!("switch_branch_local"),
    fixture!("while_and_loop_accumulation"),
    fixture!("while_condition_local_update"),
    fixture!("zero_and_multi_value_bind"),
];

const UNBOUND: &[(Fixture, &str)] = &[
    (fixture!("lazy_conditional_bind"), "local"),
    (fixture!("local_unbind_is_unbound"), "local"),
    (fixture!("caller_locals_do_not_seed_callee"), "local"),
    (fixture!("query_members_do_not_seed_callee"), "f"),
];

const INVALID_ITERATORS: &[(Fixture, &str)] = &[
    (fixture!("invalid_loop_iterator_bind"), "PRCDRPSR1"),
    (fixture!("invalid_progn_iterator_bind"), "MULTIFUN2"),
    (fixture!("invalid_foreach_iterator_bind"), "MULTIFUN2"),
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

fn before_call_installation(fixture: &Fixture) -> (Engine, String) {
    let (prefix, suffix) = fixture.source.split_once("(defrule").unwrap();
    let mut engine = Engine::new(EngineConfig::utf8());
    load(&mut engine, prefix, fixture.name);
    engine.reset().unwrap();
    (engine, format!("(defrule{suffix}"))
}

fn before_definition_installation() -> Engine {
    let mut engine = Engine::new(EngineConfig::utf8());
    engine.reset().unwrap();
    engine
}

fn install_definitions_and_calls(engine: &mut Engine, fixture: &Fixture) {
    load(engine, fixture.source, fixture.name);
    engine.reset().unwrap();
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

golden_test!(branches_and_last_bind_value, "branches_and_last_bind_value");
golden_test!(
    colon_local_and_compact_member,
    "colon_local_and_compact_member"
);
golden_test!(early_return_isolation, "early_return_isolation");
golden_test!(
    generic_method_local_and_next,
    "generic_method_local_and_next"
);
golden_test!(
    globals_and_locals_remain_distinct,
    "globals_and_locals_remain_distinct"
);
golden_test!(
    loop_progn_and_foreach_scopes,
    "loop_progn_and_foreach_scopes"
);
golden_test!(
    multifield_local_and_rest_parameter,
    "multifield_local_and_rest_parameter"
);
golden_test!(nested_and_caller_isolation, "nested_and_caller_isolation");
golden_test!(nested_bind_values, "nested_bind_values");
golden_test!(original_local, "original_local");
golden_test!(original_parameter, "original_parameter");
golden_test!(
    generated_index_writes_persist_after_lexical_iteration,
    "progn_index_bind_runtime"
);
golden_test!(parameter_unbind_falls_back, "parameter_unbind_falls_back");
golden_test!(progn_generated_index_scope, "progn_generated_index_scope");
golden_test!(
    query_callee_compact_isolation,
    "query_callee_compact_isolation"
);
golden_test!(
    query_member_shadows_parameter,
    "query_member_shadows_parameter"
);
golden_test!(
    query_member_shadows_rebound_parameter,
    "query_member_shadows_rebound_parameter"
);
golden_test!(
    query_predicate_calls_local_function,
    "query_predicate_calls_local_function"
);
golden_test!(
    rebound_iterator_parameter_scopes,
    "rebound_iterator_parameter_scopes"
);
golden_test!(recursive_frame_isolation, "recursive_frame_isolation");
golden_test!(rest_aliases, "rest_aliases");
golden_test!(
    sibling_argument_local_updates,
    "sibling_argument_local_updates"
);
golden_test!(switch_branch_local, "switch_branch_local");
golden_test!(while_and_loop_accumulation, "while_and_loop_accumulation");
golden_test!(while_condition_local_update, "while_condition_local_update");
golden_test!(zero_and_multi_value_bind, "zero_and_multi_value_bind");

#[test]
fn late_calls_use_already_registered_callable_definitions() {
    for fixture in ORDINARY {
        let (mut engine, rule) = before_call_installation(fixture);
        load(&mut engine, &rule, fixture.name);
        assert_fixture_output(&mut engine, fixture);
    }
}

#[test]
fn late_definitions_and_calls_work_after_an_empty_reset() {
    for fixture in ORDINARY {
        let mut engine = before_definition_installation();
        install_definitions_and_calls(&mut engine, fixture);
        assert_fixture_output(&mut engine, fixture);
    }
}

fn assert_unbound(engine: &mut Engine, variable: &str, prior_output: &str) {
    let result = engine.run(RunLimit::Count(10)).unwrap();
    assert_eq!(result.halt_reason, HaltReason::ActionError);
    assert!(
        engine.action_diagnostics().iter().any(|error| {
            let message = error.to_string();
            message.contains("unbound variable") && message.contains(&format!("?{variable}"))
        }),
        "{:?}",
        engine.action_diagnostics()
    );
    assert_eq!(engine.get_output("t").unwrap_or(""), prior_output);
}

#[test]
fn untaken_unbound_and_caller_scoped_values_do_not_initialize_callee_locals() {
    for (fixture, variable) in UNBOUND {
        assert_unbound(&mut pending(fixture), variable, "");
        let (mut engine, rule) = before_call_installation(fixture);
        load(&mut engine, &rule, fixture.name);
        assert_unbound(&mut engine, variable, "");
        let mut engine = before_definition_installation();
        install_definitions_and_calls(&mut engine, fixture);
        assert_unbound(&mut engine, variable, "");
    }
}

fn assert_invalid_iterator_definition(engine: &mut Engine, fixture: &Fixture, code: &str) {
    let errors = engine.load_str(fixture.source).unwrap_err();
    assert!(
        errors.iter().any(|error| error.to_string().contains(code)),
        "{}: {errors:?}",
        fixture.name
    );
    assert_eq!(engine.get_output("t").unwrap_or(""), "");
}

#[test]
fn callable_iterator_names_cannot_be_rebound_by_local_bind() {
    for (fixture, code) in INVALID_ITERATORS {
        let mut engine = Engine::new(EngineConfig::utf8());
        assert_invalid_iterator_definition(&mut engine, fixture, code);
        let mut engine = before_definition_installation();
        assert_invalid_iterator_definition(&mut engine, fixture, code);
    }
}

fn after_completed_call(fixture: &Fixture, expected: &str) -> Engine {
    let (prefix, _) = fixture
        .source
        .split_once(";; CALL AFTER COMPLETION\n")
        .unwrap();
    let mut engine = Engine::new(EngineConfig::utf8());
    load(&mut engine, prefix, fixture.name);
    engine.reset().unwrap();
    run_one(&mut engine, fixture.name);
    assert_eq!(engine.get_output("t").unwrap_or(""), expected);
    engine
}

fn install_next_call(engine: &mut Engine, fixture: &Fixture) {
    let (_, suffix) = fixture
        .source
        .split_once(";; CALL AFTER COMPLETION\n")
        .unwrap();
    load(engine, suffix, fixture.name);
}

fn finish_completed_calls(engine: &mut Engine) {
    let fixture = fixture!("completed_calls");
    install_next_call(engine, &fixture);
    assert_fixture_output(engine, &fixture);
}

#[test]
fn completed_function_and_method_calls_allow_fresh_later_calls() {
    let mut engine = after_completed_call(&fixture!("completed_calls"), "first:8:(13 99 4)\n");
    finish_completed_calls(&mut engine);
}

fn finish_completed_unbound_call(engine: &mut Engine) {
    install_next_call(engine, &fixture!("completed_call_unbound"));
    assert_unbound(engine, "local", "first:10\n");
}

#[test]
fn completed_invocation_local_is_unbound_in_the_next_call() {
    let mut engine = after_completed_call(&fixture!("completed_call_unbound"), "first:10\n");
    finish_completed_unbound_call(&mut engine);
}

#[cfg(feature = "serde")]
#[test]
fn pending_callable_calls_restore_in_all_formats() {
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
fn late_calls_use_restored_callable_definitions_in_all_formats() {
    use ferric_rules::runtime::SerializationFormat;
    for fixture in ORDINARY {
        let (engine, rule) = before_call_installation(fixture);
        for &format in SerializationFormat::ALL {
            let mut restored = Engine::deserialize(&engine.serialize(format).unwrap(), format)
                .unwrap_or_else(|error| panic!("{} / {format:?}: {error:?}", fixture.name));
            load(&mut restored, &rule, fixture.name);
            assert_fixture_output(&mut restored, fixture);
        }
    }
}

#[cfg(feature = "serde")]
#[test]
fn late_definitions_and_calls_install_after_restore_in_all_formats() {
    use ferric_rules::runtime::SerializationFormat;
    let engine = before_definition_installation();
    for fixture in ORDINARY {
        for &format in SerializationFormat::ALL {
            let mut restored = Engine::deserialize(&engine.serialize(format).unwrap(), format)
                .unwrap_or_else(|error| panic!("{} / {format:?}: {error:?}", fixture.name));
            install_definitions_and_calls(&mut restored, fixture);
            assert_fixture_output(&mut restored, fixture);
        }
    }
}

#[cfg(feature = "serde")]
#[test]
fn unbound_errors_survive_pending_and_late_restore_in_all_formats() {
    use ferric_rules::runtime::SerializationFormat;
    for (fixture, variable) in UNBOUND {
        let engine = pending(fixture);
        let (before_calls, rule) = before_call_installation(fixture);
        let before_definitions = before_definition_installation();
        for &format in SerializationFormat::ALL {
            let mut restored =
                Engine::deserialize(&engine.serialize(format).unwrap(), format).unwrap();
            assert_unbound(&mut restored, variable, "");
            let mut restored =
                Engine::deserialize(&before_calls.serialize(format).unwrap(), format).unwrap();
            load(&mut restored, &rule, fixture.name);
            assert_unbound(&mut restored, variable, "");
            let mut restored =
                Engine::deserialize(&before_definitions.serialize(format).unwrap(), format)
                    .unwrap();
            install_definitions_and_calls(&mut restored, fixture);
            assert_unbound(&mut restored, variable, "");
        }
    }
}

#[cfg(feature = "serde")]
#[test]
fn completed_function_and_method_calls_restore_before_new_calls_in_all_formats() {
    use ferric_rules::runtime::SerializationFormat;
    let engine = after_completed_call(&fixture!("completed_calls"), "first:8:(13 99 4)\n");
    for &format in SerializationFormat::ALL {
        let mut restored = Engine::deserialize(&engine.serialize(format).unwrap(), format).unwrap();
        finish_completed_calls(&mut restored);
    }
}

#[cfg(feature = "serde")]
#[test]
fn completed_invocation_locals_do_not_persist_in_any_snapshot_format() {
    use ferric_rules::runtime::SerializationFormat;
    let engine = after_completed_call(&fixture!("completed_call_unbound"), "first:10\n");
    for &format in SerializationFormat::ALL {
        let mut restored = Engine::deserialize(&engine.serialize(format).unwrap(), format).unwrap();
        finish_completed_unbound_call(&mut restored);
    }
}

#[cfg(feature = "serde")]
#[test]
fn iterator_bind_definitions_are_rejected_after_restore_in_all_formats() {
    use ferric_rules::runtime::SerializationFormat;
    let engine = before_definition_installation();
    for (fixture, code) in INVALID_ITERATORS {
        for &format in SerializationFormat::ALL {
            let mut restored =
                Engine::deserialize(&engine.serialize(format).unwrap(), format).unwrap();
            assert_invalid_iterator_definition(&mut restored, fixture, code);
        }
    }
}
