//! Issue #343: sort executes CLIPS exchange predicates with stable call order.
//!
//! Twenty-four positive programs cover exact originals, builtin/callable
//! direction, variadic inputs, stable ties and merge traces, FALSE-only truth,
//! modules/generics, and operand-before-comparator effects. Three equivalent
//! direct-print trace probes remain separate evidence; global traces here
//! isolate comparator order from outer printout streaming.
//!
//! Goldens match CLIPS 6.30 image
//! sha256:b4b99ba2f08102f9c18743c6adaecb5ab82789ddfa8628693cabfeced461a929.
//! Normal loading uses source/reset/run; late loading uses definitions before
//! the first defrule/reset/rule suffix/run. No reference crash is a test oracle.

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
            source: include_str!(concat!("fixtures/stdlib/", $name, ".clp")),
            output: include_str!(concat!("fixtures/stdlib/", $name, ".out")),
        }
    };
}

const FIXTURES: &[Fixture] = &[
    fixture!("sort_predicate_false_like_scalars"),
    fixture!("061_sort_comparator_direction"),
    fixture!("062_sort_user_function"),
    fixture!("sort_predicate_equivalent_callable_directions"),
    fixture!("sort_predicate_variadic_flatten_empty_singleton"),
    fixture!("sort_predicate_stable_equal_keys"),
    fixture!("sort_predicate_mixed_numeric_types"),
    fixture!("sort_predicate_predicate_return_false"),
    fixture!("sort_predicate_predicate_return_true"),
    fixture!("sort_predicate_predicate_return_zero"),
    fixture!("sort_predicate_predicate_return_nil"),
    fixture!("sort_predicate_predicate_return_empty_multifield"),
    fixture!("sort_predicate_predicate_return_void"),
    fixture!("sort_predicate_generic_predicate"),
    fixture!("sort_predicate_argument_evaluation_once"),
    fixture!("sort_predicate_predicate_error_empty_singleton"),
    fixture!("sort_predicate_comparison_global_trace_three"),
    fixture!("sort_predicate_comparison_global_trace_four"),
    fixture!("sort_predicate_comparison_global_trace_five"),
    fixture!("sort_predicate_lexical_comparator_module"),
    fixture!("sort_predicate_imported_comparator_name"),
    fixture!("sort_predicate_lexeme_predicate"),
    fixture!("sort_predicate_variadic_callable_predicates"),
    fixture!("sort_predicate_argument_and_predicate_phases"),
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

fn assert_no_refiring(engine: &mut Engine, fixture: &Fixture) {
    let result = engine.run(RunLimit::Count(10)).unwrap();
    assert_eq!(result.rules_fired, 0, "{}", fixture.name);
    assert_eq!(result.halt_reason, HaltReason::AgendaEmpty);
    assert_eq!(
        engine
            .get_output("t")
            .expect("fixture output is UTF-8")
            .unwrap_or(""),
        fixture.output
    );
    assert!(engine.action_diagnostics().is_empty());
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
        engine
            .get_output("t")
            .expect("fixture output is UTF-8")
            .unwrap_or(""),
        fixture.output,
        "{}",
        fixture.name
    );
    assert_no_refiring(engine, fixture);
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
    original_061_sort_comparator_direction,
    "061_sort_comparator_direction"
);
golden_test!(original_062_sort_user_function, "062_sort_user_function");
golden_test!(
    sort_predicate_equivalent_callable_directions,
    "sort_predicate_equivalent_callable_directions"
);
golden_test!(
    sort_predicate_variadic_flatten_empty_singleton,
    "sort_predicate_variadic_flatten_empty_singleton"
);
golden_test!(
    sort_predicate_stable_equal_keys,
    "sort_predicate_stable_equal_keys"
);
golden_test!(
    sort_predicate_mixed_numeric_types,
    "sort_predicate_mixed_numeric_types"
);
golden_test!(
    sort_predicate_predicate_return_false,
    "sort_predicate_predicate_return_false"
);
golden_test!(
    sort_predicate_predicate_return_true,
    "sort_predicate_predicate_return_true"
);
golden_test!(
    sort_predicate_predicate_return_zero,
    "sort_predicate_predicate_return_zero"
);
golden_test!(
    sort_predicate_predicate_return_nil,
    "sort_predicate_predicate_return_nil"
);
golden_test!(
    sort_predicate_predicate_return_empty_multifield,
    "sort_predicate_predicate_return_empty_multifield"
);
golden_test!(
    sort_predicate_predicate_return_void,
    "sort_predicate_predicate_return_void"
);
golden_test!(
    sort_predicate_generic_predicate,
    "sort_predicate_generic_predicate"
);
golden_test!(
    sort_predicate_argument_evaluation_once,
    "sort_predicate_argument_evaluation_once"
);
golden_test!(
    sort_predicate_predicate_error_empty_singleton,
    "sort_predicate_predicate_error_empty_singleton"
);
golden_test!(
    sort_predicate_comparison_global_trace_three,
    "sort_predicate_comparison_global_trace_three"
);
golden_test!(
    sort_predicate_comparison_global_trace_four,
    "sort_predicate_comparison_global_trace_four"
);
golden_test!(
    sort_predicate_comparison_global_trace_five,
    "sort_predicate_comparison_global_trace_five"
);
golden_test!(
    sort_predicate_lexical_comparator_module,
    "sort_predicate_lexical_comparator_module"
);
golden_test!(
    sort_predicate_imported_comparator_name,
    "sort_predicate_imported_comparator_name"
);
golden_test!(
    sort_predicate_lexeme_predicate,
    "sort_predicate_lexeme_predicate"
);
golden_test!(
    sort_predicate_variadic_callable_predicates,
    "sort_predicate_variadic_callable_predicates"
);
golden_test!(
    sort_predicate_argument_and_predicate_phases,
    "sort_predicate_argument_and_predicate_phases"
);

golden_test!(
    string_false_and_float_zero_request_exchange,
    "sort_predicate_false_like_scalars"
);

#[test]
fn late_sort_predicate_rules_run_against_existing_facts() {
    for fixture in FIXTURES {
        let (mut engine, rule) = before_rule_installation(fixture);
        load(&mut engine, &rule, fixture.name);
        assert_fixture_output(&mut engine, fixture);
    }
}

#[cfg(feature = "serde")]
#[test]
fn pending_and_completed_sort_predicate_rules_resume_in_all_formats() {
    use ferric_rules::runtime::SerializationFormat;
    for fixture in FIXTURES {
        let engine = pending(fixture);
        for &format in SerializationFormat::ALL {
            let mut restored = Engine::deserialize(&engine.serialize(format).unwrap(), format)
                .unwrap_or_else(|error| panic!("{} / {format:?}: {error:?}", fixture.name));
            assert_fixture_output(&mut restored, fixture);
            let mut completed = Engine::deserialize(&restored.serialize(format).unwrap(), format)
                .unwrap_or_else(|error| panic!("{} / {format:?}: {error:?}", fixture.name));
            assert_no_refiring(&mut completed, fixture);
        }
    }
}

#[cfg(feature = "serde")]
#[test]
fn late_sort_predicate_rules_install_after_restore_in_all_formats() {
    use ferric_rules::runtime::SerializationFormat;
    for fixture in FIXTURES {
        let (engine, rule) = before_rule_installation(fixture);
        for &format in SerializationFormat::ALL {
            let mut restored = Engine::deserialize(&engine.serialize(format).unwrap(), format)
                .unwrap_or_else(|error| panic!("{} / {format:?}: {error:?}", fixture.name));
            load(&mut restored, &rule, fixture.name);
            assert_fixture_output(&mut restored, fixture);
        }
    }
}
