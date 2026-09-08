//! Issue #345: direct multifield printing quotes STRING fields without escaping
//! their contents; top-level STRING printing remains raw.
//!
//! Twelve finite reference fixtures use ordinary/late loading and all five
//! snapshot codecs. The nonfinite source fixture has separate ordinary/late and
//! four-codec pending/completed tests because JSON cannot preserve infinite
//! source AST literals. Late installation after a finite prefix uses all five.
//!
//! Every source/golden pair is byte-identical to sealed CLIPS 6.30 evidence.
//! Pinned image: sha256:b4b99ba2f08102f9c18743c6adaecb5ab82789ddfa8628693cabfeced461a929.
//! Normal protocol is source/reset/run; late protocol loads the prefix before
//! the first defrule, resets, installs the rule suffix, then runs.
//! `literal_control_bytes.clp/.out` require file-specific Git `-text` attributes.

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
    fixture!("068_printout_multifield_string_quotes"),
    fixture!("printout_multifield_top_and_multifield_strings"),
    fixture!("printout_multifield_control_symbol_context"),
    fixture!("printout_multifield_literal_control_bytes"),
    fixture!("printout_multifield_unicode_fields"),
    fixture!("printout_multifield_empty_flat_sliced"),
    fixture!("printout_multifield_generated_symbols_stay_raw"),
    fixture!("printout_multifield_integer_float_spellings"),
    fixture!("printout_multifield_float_rounding_cutovers"),
    fixture!("printout_multifield_callable_and_method_paths"),
    fixture!("printout_multifield_bound_multifield"),
    fixture!("printout_multifield_evaluate_once"),
];

const NONFINITE: Fixture = fixture!("printout_multifield_source_nonfinite_floats");

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
    original_multifield_strings_are_quoted,
    "068_printout_multifield_string_quotes"
);
golden_test!(
    top_and_multifield_strings,
    "printout_multifield_top_and_multifield_strings"
);
golden_test!(
    control_symbol_context,
    "printout_multifield_control_symbol_context"
);
golden_test!(
    literal_control_bytes,
    "printout_multifield_literal_control_bytes"
);
golden_test!(unicode_fields, "printout_multifield_unicode_fields");
golden_test!(empty_flat_sliced, "printout_multifield_empty_flat_sliced");
golden_test!(
    generated_symbols_stay_raw,
    "printout_multifield_generated_symbols_stay_raw"
);
golden_test!(
    integer_float_spellings,
    "printout_multifield_integer_float_spellings"
);
golden_test!(
    float_rounding_cutovers,
    "printout_multifield_float_rounding_cutovers"
);
golden_test!(
    callable_and_method_paths,
    "printout_multifield_callable_and_method_paths"
);
golden_test!(bound_multifield, "printout_multifield_bound_multifield");
golden_test!(evaluate_once, "printout_multifield_evaluate_once");

#[test]
fn late_multifield_printout_rules_run_against_existing_facts() {
    for fixture in FIXTURES {
        let (mut engine, rule) = before_rule_installation(fixture);
        load(&mut engine, &rule, fixture.name);
        assert_fixture_output(&mut engine, fixture);
    }
}

#[cfg(feature = "serde")]
#[test]
fn pending_and_completed_multifield_printout_rules_resume_in_all_formats() {
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
fn late_multifield_printout_rules_install_after_restore_in_all_formats() {
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
