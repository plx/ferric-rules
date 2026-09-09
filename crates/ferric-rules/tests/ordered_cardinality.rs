//! Issue #320: ordered pattern width applies before joins and negation.
//! Fixture outputs were checked with CLIPS 6.30 (3/17/15).

use ferric_rules::runtime::{Engine, EngineConfig, HaltReason, RunLimit};

fn check(source: &str, expected: &str, late_rules: bool) {
    let mut engine = Engine::new(EngineConfig::utf8());
    if late_rules {
        // Load facts before any rules, exercising alpha memory backfill.
        let rules_start = source.find("(defrule").expect("fixture has rules");
        engine.load_str(&source[..rules_start]).unwrap();
        engine.reset().unwrap();
        engine.load_str(&source[rules_start..]).unwrap();
    } else {
        engine.load_str(source).unwrap();
        engine.reset().unwrap();
    }
    #[cfg(feature = "serde")]
    for &format in ferric_rules::runtime::SerializationFormat::ALL {
        let bytes = engine.serialize(format).unwrap();
        let mut restored = Engine::deserialize(&bytes, format).unwrap();
        assert_output(&mut restored, expected);
    }
    assert_output(&mut engine, expected);
}

fn assert_output(engine: &mut Engine, expected: &str) {
    let result = engine.run(RunLimit::Count(100)).unwrap();
    assert_eq!(result.halt_reason, HaltReason::AgendaEmpty);
    assert_eq!(engine.get_output("t").unwrap_or(""), expected);
    assert!(
        engine.action_diagnostics().is_empty(),
        "{:?}",
        engine.action_diagnostics()
    );
}

macro_rules! fixture {
    ($name:ident) => {
        #[test]
        fn $name() {
            let source = include_str!(concat!("fixtures/core/", stringify!($name), ".clp"));
            let expected = include_str!(concat!("fixtures/core/", stringify!($name), ".out"));
            check(source, expected, false);
            check(source, expected, true);
        }
    };
}

fixture!(ordered_exact_arity);
fixture!(ordered_empty_pattern_arity);
fixture!(ordered_single_wildcard_arity);
fixture!(ordered_multifield_arity);
fixture!(ordered_multifield_minimum);
fixture!(ordered_cardinality_transitions);
fixture!(ordered_cardinality_template_control);
