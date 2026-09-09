//! Issue #300: exact nonlinear negative constraints through the public facade.
//! Source and golden bytes match pinned CLIPS 6.30 image
//! sha256:b4b99ba2f08102f9c18743c6adaecb5ab82789ddfa8628693cabfeced461a929.

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
            source: include_str!(concat!("fixtures/constraints/", $name, ".clp")),
            output: include_str!(concat!("fixtures/constraints/", $name, ".out")),
        }
    };
}

const FIXTURES: &[Fixture] = &[
    fixture!("nonlinear-predicate"),
    fixture!("explicit-ncc-equivalent"),
    fixture!("simple-linear-control"),
    fixture!("nonlinear-self-return"),
    fixture!("nonlinear-outer-return"),
];

fn pending(fixture: &Fixture, late: bool) -> Engine {
    let mut engine = Engine::new(EngineConfig::utf8());
    if late {
        let (prefix, rule) = fixture.source.split_once("(defrule").unwrap();
        engine.load_str(prefix).unwrap();
        engine.reset().unwrap();
        engine.load_str(&format!("(defrule{rule}")).unwrap();
    } else {
        engine.load_str(fixture.source).unwrap();
        engine.reset().unwrap();
    }
    engine
}

fn assert_output(engine: &mut Engine, fixture: &Fixture) {
    let result = engine.run(RunLimit::Count(10)).unwrap();
    assert_eq!(
        result.halt_reason,
        HaltReason::AgendaEmpty,
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
}

#[test]
fn exact_normal_and_late_negative_constraint_fixtures() {
    for fixture in FIXTURES {
        for late in [false, true] {
            assert_output(&mut pending(fixture, late), fixture);
        }
    }
}

#[cfg(feature = "serde")]
#[test]
fn pending_and_completed_negative_constraints_resume_in_every_format() {
    use ferric_rules::runtime::SerializationFormat;
    for fixture in FIXTURES {
        for late in [false, true] {
            let engine = pending(fixture, late);
            for &format in SerializationFormat::ALL {
                let mut restored = Engine::deserialize(&engine.serialize(format).unwrap(), format)
                    .unwrap_or_else(|error| panic!("{} / {format:?}: {error}", fixture.name));
                assert_output(&mut restored, fixture);
                let mut completed =
                    Engine::deserialize(&restored.serialize(format).unwrap(), format).unwrap();
                assert_eq!(completed.run(RunLimit::Count(10)).unwrap().rules_fired, 0);
                assert_eq!(completed.get_output("t").unwrap_or(""), fixture.output);
            }
        }
    }
}

#[cfg(feature = "serde")]
#[test]
fn negative_rules_install_after_restore_in_every_format() {
    use ferric_rules::runtime::SerializationFormat;
    for fixture in FIXTURES {
        let (prefix, suffix) = fixture.source.split_once("(defrule").unwrap();
        let mut engine = Engine::new(EngineConfig::utf8());
        engine.load_str(prefix).unwrap();
        engine.reset().unwrap();
        for &format in SerializationFormat::ALL {
            let mut restored =
                Engine::deserialize(&engine.serialize(format).unwrap(), format).unwrap();
            restored.load_str(&format!("(defrule{suffix}")).unwrap();
            assert_output(&mut restored, fixture);
        }
    }
}
