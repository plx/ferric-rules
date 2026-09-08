//! Issue #323: unrestricted `defmethod` parameters use bare `?name` syntax.
//!
//! Goldens were checked with CLIPS 6.30 (3/17/15), Docker image
//! `sha256:b4b99ba2f08102f9c18743c6adaecb5ab82789ddfa8628693cabfeced461a929`,
//! using `docker run --rm -i IMAGE -f2 /dev/stdin`. Normal input is the complete
//! fixture followed by `(reset) (run) (exit)`. Late-install input removes the
//! `BEGIN METHODS` / `END METHODS` block, loads the remaining source, performs
//! `(reset)`, then evaluates that method block before `(run) (exit)`. Both
//! protocols produced each valid fixture's exact `.out`. Rejection goldens
//! preserve CLIPS diagnostics; Ferric is checked for rejection at load time.

use ferric_rules::core::Value;
use ferric_rules::runtime::evaluator::EvalError;
use ferric_rules::runtime::{ActionError, Engine, EngineConfig, HaltReason, LoadError, RunLimit};

struct Fixture {
    name: &'static str,
    source: &'static str,
    output: &'static str,
}

macro_rules! fixture {
    ($name:literal) => {
        Fixture {
            name: $name,
            source: include_str!(concat!("fixtures/generics/unrestricted_", $name, ".clp")),
            output: include_str!(concat!("fixtures/generics/unrestricted_", $name, ".out")),
        }
    };
}

const VALID: &[Fixture] = &[
    fixture!("identity"),
    fixture!("fallback"),
    fixture!("mixed"),
    fixture!("bare_tail"),
    fixture!("wildcard"),
    fixture!("zero"),
    fixture!("body_effects"),
    fixture!("next_method"),
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

fn without_methods(fixture: &Fixture) -> (Engine, &str) {
    let (before, rest) = fixture.source.split_once(";; BEGIN METHODS\n").unwrap();
    let (methods, after) = rest.split_once(";; END METHODS\n").unwrap();
    let mut engine = Engine::new(EngineConfig::utf8());
    load(&mut engine, &format!("{before}{after}"), fixture.name);
    engine.reset().unwrap();
    (engine, methods)
}

fn assert_output(engine: &mut Engine, fixture: &Fixture) {
    let result = engine.run(RunLimit::Count(10)).unwrap();
    assert_ne!(
        result.halt_reason,
        HaltReason::LimitReached,
        "{}",
        fixture.name
    );
    assert_eq!(result.rules_fired, 1, "{}", fixture.name);
    assert!(
        engine.action_diagnostics().is_empty(),
        "{}: {:?}",
        fixture.name,
        engine.action_diagnostics()
    );
    assert_eq!(
        engine.get_output("t").unwrap_or(""),
        fixture.output,
        "{}",
        fixture.name
    );
    assert_eq!(engine.run(RunLimit::Count(10)).unwrap().rules_fired, 0);
    assert_eq!(engine.get_output("t").unwrap_or(""), fixture.output);
}

macro_rules! valid_test {
    ($test:ident, $name:literal) => {
        #[test]
        fn $test() {
            let fixture = fixture!($name);
            assert_output(&mut pending(&fixture), &fixture);
        }
    };
}

valid_test!(original_bare_parameter_returns_its_argument, "identity");
valid_test!(
    original_unrestricted_fallback_preserves_typed_specificity,
    "fallback"
);
valid_test!(
    bare_parameters_keep_their_positions_around_typed_parameters,
    "mixed"
);
valid_test!(
    bare_parameter_and_terminal_wildcard_bind_empty_and_nonempty_tails,
    "bare_tail"
);
valid_test!(
    wildcard_only_method_accepts_zero_and_multiple_arguments,
    "wildcard"
);
valid_test!(zero_parameter_method_remains_valid, "zero");
valid_test!(
    unrestricted_method_body_effects_run_once_per_invocation,
    "body_effects"
);
valid_test!(
    typed_method_can_call_unrestricted_next_method,
    "next_method"
);

#[test]
fn methods_installed_after_reset_serve_existing_rule_activations() {
    for fixture in VALID {
        let (mut engine, methods) = without_methods(fixture);
        load(&mut engine, methods, fixture.name);
        assert_output(&mut engine, fixture);
    }
}

macro_rules! rejection_test {
    ($test:ident, $name:literal, $clips_code:literal) => {
        #[test]
        fn $test() {
            let fixture = fixture!($name);
            assert!(fixture.output.starts_with($clips_code));
            let mut engine = Engine::new(EngineConfig::utf8());
            let errors = engine.load_str(fixture.source).unwrap_err();
            assert!(
                errors
                    .iter()
                    .any(|error| matches!(error, LoadError::Interpret(_))),
                "{} should be rejected during interpretation: {errors:?}",
                fixture.name
            );
        }
    };
}

rejection_test!(
    parenthesized_unrestricted_parameter_is_rejected,
    "reject_parenthesized",
    "[GENRCPSR13]"
);
rejection_test!(
    wildcard_before_regular_parameter_is_rejected,
    "reject_nonterminal_wildcard",
    "[PRCCODE8]"
);
rejection_test!(
    multiple_wildcard_parameters_are_rejected,
    "reject_duplicate_wildcard",
    "[PRCCODE8]"
);
rejection_test!(
    anonymous_single_parameter_is_rejected,
    "reject_anonymous",
    "[GENRCPSR9]"
);
rejection_test!(
    anonymous_wildcard_parameter_is_rejected,
    "reject_anonymous_wildcard",
    "[GENRCPSR9]"
);
rejection_test!(
    duplicate_regular_parameter_name_is_rejected,
    "reject_duplicate_name",
    "[PRCCODE7]"
);
rejection_test!(
    regular_and_wildcard_parameter_names_must_differ,
    "reject_duplicate_cross_kind",
    "[PRCCODE7]"
);

#[test]
fn inapplicable_calls_do_not_execute_the_method_body() {
    // Each exact source was also run using the CLIPS invocation above. CLIPS
    // reports GENRCEXE1 (no applicable method), followed by PRCCODE4, for every
    // case. A subsequent `(printout t ?*calls* crlf)` prints 0 in each case.
    for (parameters, arguments) in [
        ("?x", ""),
        ("?x", "1 2"),
        ("?x $?rest", ""),
        ("", "1"),
        ("?x (?n INTEGER)", "blue green"),
    ] {
        let source = format!(
            "(defglobal ?*calls* = 0)
             (defgeneric guarded)
             (defmethod guarded ({parameters})
               (bind ?*calls* (+ ?*calls* 1))
               (printout t \"body\" crlf))
             (defrule probe => (guarded {arguments}))"
        );
        let mut engine = Engine::new(EngineConfig::utf8());
        load(&mut engine, &source, parameters);
        engine.reset().unwrap();
        assert_eq!(engine.run(RunLimit::Count(10)).unwrap().rules_fired, 1);
        assert!(
            matches!(engine.action_diagnostics(),
                [ActionError::Evaluator(EvalError::NoApplicableMethod { name, .. })]
                if name == "guarded"),
            "{parameters} / {arguments}: {:?}",
            engine.action_diagnostics()
        );
        assert!(matches!(
            engine.get_global("calls"),
            Some(Value::Integer(0))
        ));
        assert_eq!(engine.get_output("t").unwrap_or(""), "");
    }
}

#[cfg(feature = "serde")]
#[test]
fn pending_generic_calls_restore_in_every_serialization_format() {
    use ferric_rules::runtime::SerializationFormat;

    for fixture in VALID {
        let engine = pending(fixture);
        for &format in SerializationFormat::ALL {
            let mut restored = Engine::deserialize(&engine.serialize(format).unwrap(), format)
                .unwrap_or_else(|error| panic!("{} / {format:?}: {error:?}", fixture.name));
            assert_output(&mut restored, fixture);
        }
    }
}

#[cfg(feature = "serde")]
#[test]
fn unrestricted_methods_can_be_installed_after_restore_in_every_format() {
    use ferric_rules::runtime::SerializationFormat;

    for fixture in VALID {
        let (engine, methods) = without_methods(fixture);
        for &format in SerializationFormat::ALL {
            let mut restored = Engine::deserialize(&engine.serialize(format).unwrap(), format)
                .unwrap_or_else(|error| panic!("{} / {format:?}: {error:?}", fixture.name));
            load(&mut restored, methods, fixture.name);
            assert_output(&mut restored, fixture);
        }
    }
}
