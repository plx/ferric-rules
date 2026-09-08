//! Issue #336: substring bounds and ordered argument evaluation match CLIPS.
//!
//! Thirteen positive programs preserve the exact issue original, an existing inclusive
//! position control, and sealed clipping/timing/SYMBOL cases. Reached type errors
//! have separate runtime tests; existing Unicode counting policy is preserved.
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
    fixture!("036_substring_clipped_bounds"),
    fixture!("035_substring_one_based"),
    fixture!("substring_lower_clipping"),
    fixture!("substring_empty_and_reversed"),
    fixture!("substring_both_clamps_extreme_bounds"),
    fixture!("substring_inclusive_boundaries"),
    fixture!("substring_bound_and_callable"),
    fixture!("substring_arguments_once"),
    fixture!("substring_empty_and_nonempty_evaluation"),
    fixture!("substring_skips_invalid_text"),
    fixture!("substring_symbol_literals"),
    fixture!("substring_bound_generated_symbols"),
    fixture!("substring_symbol_timing"),
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
    assert_eq!(engine.get_output("t").unwrap_or(""), fixture.output);
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
        engine.get_output("t").unwrap_or(""),
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
    original_clipped_bounds_match_reference,
    "036_substring_clipped_bounds"
);
golden_test!(
    existing_one_based_inclusive_positions_are_preserved,
    "035_substring_one_based"
);
golden_test!(
    zero_negative_and_minimum_start_indices_clip_to_one,
    "substring_lower_clipping"
);
golden_test!(
    empty_text_low_ends_and_reversed_ranges_return_empty,
    "substring_empty_and_reversed"
);
golden_test!(
    both_bounds_clip_without_integer_overflow,
    "substring_both_clamps_extreme_bounds"
);
golden_test!(
    single_character_and_inclusive_boundaries_are_preserved,
    "substring_inclusive_boundaries"
);
golden_test!(
    bound_and_callable_arguments_return_strings,
    "substring_bound_and_callable"
);
golden_test!(
    required_arguments_are_evaluated_once_in_order,
    "substring_arguments_once"
);
golden_test!(
    only_low_ends_skip_text_while_positive_ranges_reach_it,
    "substring_empty_and_nonempty_evaluation"
);
golden_test!(
    low_ends_skip_invalid_text_arguments_without_validation,
    "substring_skips_invalid_text"
);

golden_test!(
    symbol_text_is_clipped_and_returns_a_string,
    "substring_symbol_literals"
);
golden_test!(
    bound_and_generated_symbols_use_complete_spelling,
    "substring_bound_generated_symbols"
);
golden_test!(
    symbol_text_follows_the_same_argument_timing,
    "substring_symbol_timing"
);

#[test]
fn late_substring_rules_run_against_existing_facts() {
    for fixture in FIXTURES {
        let (mut engine, rule) = before_rule_installation(fixture);
        load(&mut engine, &rule, fixture.name);
        assert_fixture_output(&mut engine, fixture);
    }
}

#[cfg(feature = "serde")]
#[test]
fn pending_and_completed_substring_rules_resume_in_all_formats() {
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
fn late_substring_rules_install_after_restore_in_all_formats() {
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
