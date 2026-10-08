use ferric_rules_runtime::{Engine, EngineConfig};

#[test]
fn output_events_preserve_expression_watch_and_reset_order() {
    let mut engine = Engine::new(EngineConfig::default());
    engine.enable_output_events();
    engine.eval_str("(watch facts)").unwrap();
    engine
        .eval_str(
            "(progn (printout stdout A) (assert (p)) (printout werror B) (reset) (printout t C))",
        )
        .unwrap();
    let events = engine.drain_output_events();
    assert_eq!(
        events
            .iter()
            .map(|(channel, _)| channel.as_str())
            .collect::<Vec<_>>(),
        ["stdout", "wtrace", "werror", "wtrace", "t"]
    );
    assert_eq!(events[0].1, "A");
    assert!(events[1].1.contains("==> f-1"));
    assert!(events[1].1.contains("(p)"));
    assert_eq!(events[2].1, "B");
    assert!(events[3].1.contains("<== f-1"));
    assert!(events[3].1.contains("(p)"));
    assert_eq!(events[4].1, "C");
    assert!(engine.drain_output_events().is_empty());
}

#[test]
fn host_clear_keeps_undelivered_output_and_delivery_enabled() {
    let mut engine = Engine::new(EngineConfig::default());
    engine.enable_output_events();
    engine.eval_str("(printout stdout before)").unwrap();
    engine.clear();
    engine.eval_str("(printout t after)").unwrap();
    assert_eq!(
        engine.drain_output_events(),
        vec![
            ("stdout".to_owned(), "before".to_owned()),
            ("t".to_owned(), "after".to_owned()),
        ]
    );
}

#[cfg(feature = "serde")]
#[test]
fn snapshots_keep_channel_buffers_but_not_live_observation() {
    use ferric_rules_runtime::SerializationFormat;
    for format in [SerializationFormat::Json, SerializationFormat::Cbor] {
        let mut engine = Engine::new(EngineConfig::default());
        engine.enable_output_events();
        engine.eval_str("(watch facts)").unwrap();
        engine
            .eval_str("(progn (printout werror A) (printout t B) (printout werror C))")
            .unwrap();
        let mut restored = Engine::deserialize(&engine.serialize(format).unwrap(), format).unwrap();
        assert!(!restored.watch_facts());
        assert!(restored.drain_output_events().is_empty());
        assert_eq!(restored.get_output("werror"), Some("AC"));
        restored.enable_output_events();
        restored.eval_str("(printout stdout D)").unwrap();
        assert_eq!(
            restored.drain_output_events(),
            vec![
                ("t".to_owned(), "B".to_owned()),
                ("werror".to_owned(), "AC".to_owned()),
                ("stdout".to_owned(), "D".to_owned()),
            ]
        );
        assert!(restored.drain_output_events().is_empty());
    }
}
