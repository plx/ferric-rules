//! Issue #334: variadic numeric comparisons preserve anchor, precision, and laziness.
//!
//! Six exact corpus originals are accompanied by six distinct controls. The
//! positive goldens include skipped nonnumeric tails, but do not pin diagnostic
//! text or source-versus-runtime error timing for invalid calls.
//!
//! Fixture stdout was checked byte-for-byte against CLIPS 6.30, pinned image
//! `sha256:b4b99ba2f08102f9c18743c6adaecb5ab82789ddfa8628693cabfeced461a929`,
//! using `docker run --rm -i IMAGE -f2 /dev/stdin`. Normal installation loads
//! source/reset/run; late installation loads the prefix before the first
//! defrule, resets, installs the rule suffix, and runs.

use ferric_rules::runtime::{Engine, EngineConfig, HaltReason, RunLimit};

struct Fixture {
    name: &'static str,
    source: &'static str,
    output: &'static str,
    firings: usize,
}

macro_rules! fixture {
    ($name:literal) => {
        fixture!($name, 1)
    };
    ($name:literal, $firings:expr) => {
        Fixture {
            name: $name,
            source: include_str!(concat!("fixtures/stdlib/", $name, ".clp")),
            output: include_str!(concat!("fixtures/stdlib/", $name, ".out")),
            firings: $firings,
        }
    };
}

const FIXTURES: &[Fixture] = &[
    fixture!("046_numeric_equal_chain"),
    fixture!("047_numeric_less_chain"),
    fixture!("048_numeric_greater_chain"),
    fixture!("049_numeric_inequality_chain"),
    fixture!("065_numeric_less_equal_chain"),
    fixture!("066_numeric_greater_equal_chain"),
    fixture!("numeric_chain_anchor_types"),
    fixture!("numeric_chain_binary_precision"),
    fixture!("numeric_chain_false_prefix_trace"),
    fixture!("numeric_chain_true_trace"),
    fixture!("numeric_chain_skips_invalid_tail"),
    fixture!("numeric_chain_expression_contexts", 5),
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
    let result = engine.run(RunLimit::Count(20)).unwrap();
    assert_eq!(result.rules_fired, 0, "{}", fixture.name);
    assert_eq!(result.halt_reason, HaltReason::AgendaEmpty);
    assert_eq!(engine.get_output("t").unwrap_or(""), fixture.output);
    assert!(engine.action_diagnostics().is_empty());
}

fn assert_fixture_output(engine: &mut Engine, fixture: &Fixture) {
    let result = engine.run(RunLimit::Count(20)).unwrap();
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
    assert_eq!(result.rules_fired, fixture.firings, "{}", fixture.name);
    assert_eq!(
        engine.get_output("t").unwrap_or(""),
        fixture.output,
        "{}",
        fixture.name
    );
    assert_no_refiring(engine, fixture);
}

macro_rules! golden_test {
    ($test:ident, $name:literal) => {
        golden_test!($test, $name, 1);
    };
    ($test:ident, $name:literal, $firings:expr) => {
        #[test]
        fn $test() {
            let fixture = fixture!($name, $firings);
            assert_fixture_output(&mut pending(&fixture), &fixture);
        }
    };
}

golden_test!(original_numeric_equal_chain, "046_numeric_equal_chain");
golden_test!(original_numeric_less_chain, "047_numeric_less_chain");
golden_test!(original_numeric_greater_chain, "048_numeric_greater_chain");
golden_test!(
    original_numeric_inequality_chain,
    "049_numeric_inequality_chain"
);
golden_test!(
    original_numeric_less_equal_chain,
    "065_numeric_less_equal_chain"
);
golden_test!(
    original_numeric_greater_equal_chain,
    "066_numeric_greater_equal_chain"
);
golden_test!(
    numeric_equality_keeps_its_first_typed_operand,
    "numeric_chain_anchor_types"
);
golden_test!(
    binary_numeric_comparisons_keep_integer_and_float_precision,
    "numeric_chain_binary_precision"
);
golden_test!(
    false_prefix_skips_later_side_effects,
    "numeric_chain_false_prefix_trace"
);
golden_test!(
    true_chains_evaluate_each_operand_once_in_order,
    "numeric_chain_true_trace"
);
golden_test!(
    false_prefix_skips_nonnumeric_returning_calls,
    "numeric_chain_skips_invalid_tail"
);
golden_test!(
    numeric_chains_work_in_callable_and_positive_condition_contexts,
    "numeric_chain_expression_contexts",
    5
);

#[test]
fn late_comparison_rules_run_against_existing_facts() {
    for fixture in FIXTURES {
        let (mut engine, rules) = before_rule_installation(fixture);
        load(&mut engine, &rules, fixture.name);
        assert_fixture_output(&mut engine, fixture);
    }
}

#[cfg(feature = "serde")]
#[test]
fn pending_and_completed_comparison_rules_resume_in_all_formats() {
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
fn late_comparison_rules_install_after_restore_in_all_formats() {
    use ferric_rules::runtime::SerializationFormat;
    for fixture in FIXTURES {
        let (engine, rules) = before_rule_installation(fixture);
        for &format in SerializationFormat::ALL {
            let mut restored = Engine::deserialize(&engine.serialize(format).unwrap(), format)
                .unwrap_or_else(|error| panic!("{} / {format:?}: {error:?}", fixture.name));
            load(&mut restored, &rules, fixture.name);
            assert_fixture_output(&mut restored, fixture);
        }
    }
}
