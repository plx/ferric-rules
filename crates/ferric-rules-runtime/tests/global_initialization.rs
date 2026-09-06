//! A failed named global initializer must not leave a phantom exported name.
use ferric_rules_runtime::{Engine, EngineConfig, RunLimit, Value};

fn partial_load() -> Engine {
    let mut engine = Engine::new(EngineConfig::default());
    let errors = engine
        .load_str(include_str!("fixtures/global_incremental_failure.clp"))
        .unwrap_err();
    assert_eq!(
        errors.len(),
        1,
        "only the divide-by-zero initializer should fail: {errors:?}"
    );
    assert!(errors[0].to_string().contains("bad"));
    for (name, expected) in [
        ("first", 1),
        ("second", 2),
        ("following", 4),
        ("last", 7),
        ("answer", 14),
    ] {
        assert!(
            matches!(engine.get_global(name), Some(Value::Integer(value)) if *value == expected),
            "{name}"
        );
    }
    assert!(engine.get_global("bad").is_none());
    engine
}

#[test]
fn failed_initializer_preserves_earlier_globals_and_following_constructs() {
    let mut engine = partial_load();
    engine.reset().unwrap();
    engine.set_focus("USE").unwrap();
    assert_eq!(engine.run(RunLimit::Unlimited).unwrap().rules_fired, 1);
    assert!(engine.action_diagnostics().is_empty());
    assert_eq!(engine.get_output("t"), Some("14\n"));
}

#[cfg(feature = "serde")]
#[test]
fn partial_global_load_snapshots_resume_with_consistent_module_metadata() {
    use ferric_rules_runtime::SerializationFormat;
    let engine = partial_load();
    for &format in SerializationFormat::ALL {
        let bytes = engine.serialize(format).unwrap();
        let mut restored = Engine::deserialize(&bytes, format).unwrap();
        restored.reset().unwrap();
        restored.set_focus("USE").unwrap();
        assert_eq!(restored.run(RunLimit::Unlimited).unwrap().rules_fired, 1);
        assert_eq!(restored.get_output("t"), Some("14\n"));
    }
}
