//! Issue #341: nth$ and its nth alias return nil outside one-based bounds.
//!
//! Four exact corpus pairs plus ten distinct positive controls preserve element
//! types, finite float coercion, boundaries, and required operand effects.
//! Goldens match pinned CLIPS 6.30 image
//! sha256:b4b99ba2f08102f9c18743c6adaecb5ab82789ddfa8628693cabfeced461a929.
//! Normal protocol loads/reset/runs; late protocol loads definitions before the
//! first defrule, resets, installs the remaining rule, and runs. Effect-fixture
//! trace reads occur within the sole rule, so both protocols test exact stdout.

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
    fixture!("053_nth_out_of_bounds"),
    fixture!("069_nth_negative_index"),
    fixture!("070_nth_excessive_index"),
    fixture!("052_nth_one_based"),
    fixture!("nth_indices_empty_and_extreme_indices"),
    fixture!("nth_indices_result_types_and_nil"),
    fixture!("nth_indices_slices_and_bound_callable"),
    fixture!("nth_indices_evaluation_valid"),
    fixture!("nth_indices_evaluation_zero"),
    fixture!("nth_indices_evaluation_negative"),
    fixture!("nth_indices_evaluation_excessive"),
    fixture!("nth_indices_nth_alias_control"),
    fixture!("nth_indices_runtime_float_index_conversion"),
    fixture!("nth_indices_dynamic_float_evaluation"),
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
        engine.get_output("t").unwrap().unwrap_or(""),
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
        engine.get_output("t").unwrap().unwrap_or(""),
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

golden_test!(original_053_nth_out_of_bounds, "053_nth_out_of_bounds");
golden_test!(original_069_nth_negative_index, "069_nth_negative_index");
golden_test!(original_070_nth_excessive_index, "070_nth_excessive_index");
golden_test!(original_052_nth_one_based, "052_nth_one_based");
golden_test!(
    nth_indices_empty_and_extreme_indices,
    "nth_indices_empty_and_extreme_indices"
);
golden_test!(
    nth_indices_result_types_and_nil,
    "nth_indices_result_types_and_nil"
);
golden_test!(
    nth_indices_slices_and_bound_callable,
    "nth_indices_slices_and_bound_callable"
);
golden_test!(nth_indices_evaluation_valid, "nth_indices_evaluation_valid");
golden_test!(nth_indices_evaluation_zero, "nth_indices_evaluation_zero");
golden_test!(
    nth_indices_evaluation_negative,
    "nth_indices_evaluation_negative"
);
golden_test!(
    nth_indices_evaluation_excessive,
    "nth_indices_evaluation_excessive"
);
golden_test!(
    nth_indices_nth_alias_control,
    "nth_indices_nth_alias_control"
);
golden_test!(
    nth_indices_runtime_float_index_conversion,
    "nth_indices_runtime_float_index_conversion"
);
golden_test!(
    nth_indices_dynamic_float_evaluation,
    "nth_indices_dynamic_float_evaluation"
);

#[test]
fn late_nth_index_rules_run_against_existing_facts() {
    for fixture in FIXTURES {
        let (mut engine, rule) = before_rule_installation(fixture);
        load(&mut engine, &rule, fixture.name);
        assert_fixture_output(&mut engine, fixture);
    }
}

#[cfg(feature = "serde")]
#[test]
fn pending_and_completed_nth_index_rules_resume_in_all_formats() {
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
fn late_nth_index_rules_install_after_restore_in_all_formats() {
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
