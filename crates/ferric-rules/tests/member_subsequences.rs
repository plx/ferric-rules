//! Issue #342: member$ and member search contiguous multifield subsequences.
//!
//! Thirteen positive programs preserve exact original/scalar controls, result
//! types, first matches, empty bounds, structural equality, and operand effects.
//! Goldens match pinned CLIPS 6.30 image
//! `sha256:b4b99ba2f08102f9c18743c6adaecb5ab82789ddfa8628693cabfeced461a929`.
//! Normal installation loads source/reset/run; late installation loads the
//! prefix before the first defrule, resets, installs the suffix, and runs.
//! Error diagnostics and unsupported value representations are outside these
//! positive goldens; focused runtime tests cover argument and error ordering.

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
    fixture!("055_member_subsequence"),
    fixture!("054_member_scalar_type_sensitive"),
    fixture!("member_subsequence_range_shapes"),
    fixture!("member_subsequence_empty_operands"),
    fixture!("member_subsequence_first_match"),
    fixture!("member_subsequence_scalar_types"),
    fixture!("member_subsequence_field_types"),
    fixture!("member_subsequence_slice_indices"),
    fixture!("member_subsequence_generated_values"),
    fixture!("member_subsequence_bound_callable"),
    fixture!("member_subsequence_argument_effects"),
    fixture!("member_subsequence_void_needle"),
    fixture!("member_subsequence_alias"),
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
    original_contiguous_subsequence_returns_inclusive_range,
    "055_member_subsequence"
);
golden_test!(
    original_scalar_membership_remains_type_sensitive,
    "054_member_scalar_type_sensitive"
);
golden_test!(
    singleton_and_longer_needles_have_distinct_result_shapes,
    "member_subsequence_range_shapes"
);
golden_test!(
    empty_needles_and_haystacks_follow_reference_boundaries,
    "member_subsequence_empty_operands"
);
golden_test!(
    first_complete_match_wins_after_partial_and_overlapping_candidates,
    "member_subsequence_first_match"
);
golden_test!(
    scalar_equality_preserves_type_bits_and_large_integers,
    "member_subsequence_scalar_types"
);
golden_test!(
    subsequence_fields_use_the_same_type_sensitive_equality,
    "member_subsequence_field_types"
);
golden_test!(
    slice_indices_are_relative_to_the_supplied_haystack,
    "member_subsequence_slice_indices"
);
golden_test!(
    flattened_and_generated_fields_preserve_sequence_membership,
    "member_subsequence_generated_values"
);
golden_test!(
    fact_captures_functions_and_methods_search_sequences,
    "member_subsequence_bound_callable"
);
golden_test!(
    each_operand_evaluates_once_in_order_even_when_empty,
    "member_subsequence_argument_effects"
);
golden_test!(
    void_scalar_needle_returns_false_and_continues,
    "member_subsequence_void_needle"
);
golden_test!(
    member_alias_matches_multifield_subsequences,
    "member_subsequence_alias"
);

#[test]
fn late_member_subsequence_rules_run_against_existing_facts() {
    for fixture in FIXTURES {
        let (mut engine, rule) = before_rule_installation(fixture);
        load(&mut engine, &rule, fixture.name);
        assert_fixture_output(&mut engine, fixture);
    }
}

#[cfg(feature = "serde")]
#[test]
fn pending_and_completed_member_subsequence_rules_resume_in_all_formats() {
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
fn late_member_subsequence_rules_install_after_restore_in_all_formats() {
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
