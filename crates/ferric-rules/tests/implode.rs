//! Issue #344: implode$ returns escaped, quoted STRING fields with CLIPS
//! numeric spelling. The direct-print cross-control protects the raw field mode.
//!
//! Every promoted source/golden pair is byte-identical to sealed CLIPS 6.30
//! evidence; image sha256:b4b99ba2f08102f9c18743c6adaecb5ab82789ddfa8628693cabfeced461a929.
//! Finite fixtures cover normal/late load and all five snapshot codecs.
//! The two round-trip fixtures compose #339 scanning with #344 quoting; they
//! cover selected scannable values, not arbitrary symbols, floats, or NUL data.
//! Nonfinite rule ASTs use four codecs plus explicit JSON rejection; late
//! installation after a finite prefix still covers all five codecs.
//! Literal CR/CRLF fixture paths require exact file-specific Git -text rules.

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
    fixture!("063_implode_preserves_string_quotes"),
    fixture!("implode_empty_singleton_types"),
    fixture!("implode_mixed_fields"),
    fixture!("implode_quote_backslash_spacing"),
    fixture!("implode_literal_control_characters"),
    fixture!("implode_unicode_fields"),
    fixture!("implode_generated_symbol_spellings"),
    fixture!("implode_integer_float_spellings"),
    fixture!("implode_float_boundary_spellings"),
    fixture!("implode_slice_relative_fields"),
    fixture!("implode_flattened_generated_fields"),
    fixture!("implode_bound_callable_method"),
    fixture!("implode_string_special_symbol_controls"),
    fixture!("implode_literal_crlf_preservation"),
    fixture!("implode_float_rounding_cutovers"),
    fixture!("implode_empty_mf_evaluates_once"),
    fixture!("implode_multifield_print_formatter_control"),
    fixture!("implode_quoted_round_trip"),
    fixture!("implode_typed_round_trip"),
];

const NONFINITE: Fixture = fixture!("implode_source_nonfinite_floats");

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
    assert_eq!(engine.get_output_bytes("t").unwrap_or(b""), fixture.output);
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
        engine.get_output_bytes("t").unwrap_or(b""),
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
    original_quoted_string_fields_match_reference,
    "063_implode_preserves_string_quotes"
);
golden_test!(
    empty_singleton_types_matches_reference,
    "implode_empty_singleton_types"
);
golden_test!(mixed_fields_matches_reference, "implode_mixed_fields");
golden_test!(
    quote_backslash_spacing_matches_reference,
    "implode_quote_backslash_spacing"
);
golden_test!(
    literal_control_characters_matches_reference,
    "implode_literal_control_characters"
);
golden_test!(unicode_fields_matches_reference, "implode_unicode_fields");
golden_test!(
    generated_symbol_spellings_matches_reference,
    "implode_generated_symbol_spellings"
);
golden_test!(
    integer_float_spellings_matches_reference,
    "implode_integer_float_spellings"
);
golden_test!(
    float_boundary_spellings_matches_reference,
    "implode_float_boundary_spellings"
);
golden_test!(
    slice_relative_fields_matches_reference,
    "implode_slice_relative_fields"
);
golden_test!(
    flattened_generated_fields_matches_reference,
    "implode_flattened_generated_fields"
);
golden_test!(
    bound_callable_method_matches_reference,
    "implode_bound_callable_method"
);
golden_test!(
    string_special_symbol_controls_matches_reference,
    "implode_string_special_symbol_controls"
);
golden_test!(
    literal_crlf_preservation_matches_reference,
    "implode_literal_crlf_preservation"
);
golden_test!(
    float_rounding_cutovers_matches_reference,
    "implode_float_rounding_cutovers"
);
golden_test!(
    empty_mf_evaluates_once_matches_reference,
    "implode_empty_mf_evaluates_once"
);
golden_test!(
    multifield_print_formatter_control_matches_reference,
    "implode_multifield_print_formatter_control"
);

golden_test!(
    implode_quoted_round_trip_matches_reference,
    "implode_quoted_round_trip"
);

golden_test!(
    implode_typed_round_trip_matches_reference,
    "implode_typed_round_trip"
);

#[test]
fn late_implode_rules_run_against_existing_facts() {
    for fixture in FIXTURES {
        let (mut engine, rule) = before_rule_installation(fixture);
        load(&mut engine, &rule, fixture.name);
        assert_fixture_output(&mut engine, fixture);
    }
}

#[cfg(feature = "serde")]
#[test]
fn pending_and_completed_implode_rules_resume_in_all_formats() {
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
fn late_implode_rules_install_after_restore_in_all_formats() {
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

#[test]
fn source_nonfinite_values_print_in_normal_and_late_rules() {
    assert_fixture_output(&mut pending(&NONFINITE), &NONFINITE);
    let (mut engine, rule) = before_rule_installation(&NONFINITE);
    load(&mut engine, &rule, NONFINITE.name);
    assert_fixture_output(&mut engine, &NONFINITE);
}

// The source AST contains infinite f64 literals before any rule runs. JSON
// cannot preserve those numeric literals; this is a source representation
// boundary, not a reason to omit the ordinary or late nonfinite output case.
// The finite FIXTURES above still exercise every SerializationFormat.
#[cfg(feature = "serde")]
const NONFINITE_FORMATS: &[ferric_rules::runtime::SerializationFormat] = &[
    ferric_rules::runtime::SerializationFormat::Bincode,
    ferric_rules::runtime::SerializationFormat::Cbor,
    ferric_rules::runtime::SerializationFormat::MessagePack,
    ferric_rules::runtime::SerializationFormat::Postcard,
];

#[cfg(feature = "serde")]
#[test]
fn nonfinite_source_pending_and_completed_rules_resume_in_non_json_formats() {
    let engine = pending(&NONFINITE);
    for &format in NONFINITE_FORMATS {
        let mut restored = Engine::deserialize(&engine.serialize(format).unwrap(), format)
            .unwrap_or_else(|error| panic!("{} / {format:?}: {error:?}", NONFINITE.name));
        assert_fixture_output(&mut restored, &NONFINITE);
        let mut completed = Engine::deserialize(&restored.serialize(format).unwrap(), format)
            .unwrap_or_else(|error| panic!("{} / {format:?}: {error:?}", NONFINITE.name));
        assert_no_refiring(&mut completed, &NONFINITE);
    }
}

#[cfg(feature = "serde")]
#[test]
fn nonfinite_source_rules_install_after_restore_in_all_formats() {
    use ferric_rules::runtime::SerializationFormat;
    // This prefix has no nonfinite literals: the rule is installed only after
    // restoring it, so the JSON snapshot is valid too.
    let (engine, rule) = before_rule_installation(&NONFINITE);
    for &format in SerializationFormat::ALL {
        let mut restored = Engine::deserialize(&engine.serialize(format).unwrap(), format)
            .unwrap_or_else(|error| panic!("{} / {format:?}: {error:?}", NONFINITE.name));
        load(&mut restored, &rule, NONFINITE.name);
        assert_fixture_output(&mut restored, &NONFINITE);
    }
}

#[cfg(feature = "serde")]
#[test]
fn json_rejects_nonfinite_source_snapshots_before_and_after_execution() {
    use ferric_rules::runtime::SerializationFormat;
    let mut engine = pending(&NONFINITE);
    assert!(engine.serialize(SerializationFormat::Json).is_err());
    assert_fixture_output(&mut engine, &NONFINITE);
    // Running does not remove the infinite literal from the stored rule AST.
    assert!(engine.serialize(SerializationFormat::Json).is_err());
}
