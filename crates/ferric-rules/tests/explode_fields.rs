//! Issue #339: explode$ scans complete input and preserves quoted field types.
//!
//! Exact source/output pairs cover the issue original and eight controls.
//! CLIPS 6.30 reference image:
//! `sha256:b4b99ba2f08102f9c18743c6adaecb5ab82789ddfa8628693cabfeced461a929`.
//! Warning/error cases have separate tests inspecting actual values and control
//! state; these goldens have no diagnostics.

use ferric_rules::runtime::{Engine, EngineConfig, HaltReason, RunLimit};

struct Fixture {
    name: &'static str,
    source: &'static str,
    output: &'static [u8],
}

macro_rules! fixture {
    ($name:literal) => {
        Fixture {
            name: $name,
            source: include_str!(concat!("fixtures/stdlib/", $name, ".clp")),
            output: include_bytes!(concat!("fixtures/stdlib/", $name, ".out")),
        }
    };
}

const FIXTURES: &[Fixture] = &[
    fixture!("042_explode_quoted_fields"),
    fixture!("explode_ordinary_and_empty_fields"),
    fixture!("explode_quoted_adjacency_and_escapes"),
    fixture!("explode_whitespace_and_comments"),
    fixture!("explode_numeric_field_types"),
    fixture!("explode_nonvalue_tokens_and_delimiters"),
    fixture!("explode_argument_evaluated_once"),
    fixture!("explode_bound_and_callable"),
    fixture!("explode_instance_name_field"),
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
    assert_eq!(engine.get_output_bytes("t").unwrap_or(b""), fixture.output);
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
    assert_eq!(result.rules_fired, 1, "{}", fixture.name);
    assert_eq!(
        engine.get_output_bytes("t").unwrap_or(b""),
        fixture.output,
        "{}",
        fixture.name
    );
    assert_no_refiring(engine, fixture);
}

#[test]
fn exploded_field_values_match_reference_types_and_bytes() {
    for fixture in FIXTURES {
        assert_fixture_output(&mut pending(fixture), fixture);
    }
}

#[test]
fn late_exploded_field_rules_run_against_existing_facts() {
    for fixture in FIXTURES {
        let (mut engine, rules) = before_rule_installation(fixture);
        load(&mut engine, &rules, fixture.name);
        assert_fixture_output(&mut engine, fixture);
    }
}

#[cfg(feature = "serde")]
#[test]
fn pending_and_completed_exploded_field_rules_resume_in_all_formats() {
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
fn late_exploded_field_rules_install_after_restore_in_all_formats() {
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
