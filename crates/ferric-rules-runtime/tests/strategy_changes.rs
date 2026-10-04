use ferric_rules_runtime::{Engine, EngineConfig, RunLimit};

const SOURCE: &str = "
    (deffacts seed (p 1) (p 2) (p 3))
    (defrule show (p ?n) => (printout t ?n crlf))";

fn strategy_value(engine: &mut Engine, source: &str) -> String {
    let value = engine.eval_str(source).unwrap();
    engine.format_value(&value)
}

#[test]
fn strategy_changes_reorder_pending_matches_and_survive_reset_clear() {
    let mut engine = Engine::with_rules(SOURCE).unwrap();
    engine.reset().unwrap();
    assert_eq!(engine.run(RunLimit::Count(1)).unwrap().rules_fired, 1);
    assert_eq!(
        strategy_value(&mut engine, "(set-strategy breadth)"),
        "depth"
    );
    assert_eq!(engine.run(RunLimit::Unlimited).unwrap().rules_fired, 2);
    assert_eq!(engine.get_output("t"), Some("3\n1\n2\n"));
    engine.reset().unwrap();
    engine.run(RunLimit::Unlimited).unwrap();
    assert_eq!(engine.get_output("t"), Some("1\n2\n3\n"));
    engine.eval_str("(clear)").unwrap();
    assert_eq!(strategy_value(&mut engine, "(get-strategy)"), "breadth");
    engine.load_str(SOURCE).unwrap();
    engine.reset().unwrap();
    engine.run(RunLimit::Unlimited).unwrap();
    assert_eq!(engine.get_output("t"), Some("1\n2\n3\n"));
}

#[test]
fn nested_strategy_changes_return_the_value_before_argument_evaluation() {
    let mut engine = Engine::new(EngineConfig::default());
    assert_eq!(
        strategy_value(&mut engine, "(set-strategy (progn (set-strategy mea) lex))"),
        "depth"
    );
    assert_eq!(strategy_value(&mut engine, "(get-strategy)"), "lex");
    assert!(engine.eval_str("(set-strategy unsupported)").is_err());
    assert!(engine
        .eval_str("(set-strategy (sym-cat unsupported))")
        .is_err());
    assert_eq!(strategy_value(&mut engine, "(get-strategy)"), "lex");
}

#[test]
fn rhs_strategy_changes_apply_to_the_remaining_agenda() {
    let mut engine = Engine::with_rules(&format!(
        "{SOURCE}\n(defrule change (declare (salience 10)) => (set-strategy breadth))"
    ))
    .unwrap();
    engine.reset().unwrap();
    assert_eq!(engine.run(RunLimit::Unlimited).unwrap().rules_fired, 4);
    assert_eq!(engine.get_output("t"), Some("1\n2\n3\n"));
}

#[test]
fn match_conditions_cannot_reorder_the_agenda_through_a_callable() {
    let mut engine = Engine::with_rules(
        "(deffunction reorder () (set-strategy breadth) TRUE)
         (defrule unsafe-predicate (test (reorder)) => (assert (fired)))",
    )
    .unwrap();
    assert_eq!(engine.run(RunLimit::Unlimited).unwrap().rules_fired, 0);
    assert_eq!(strategy_value(&mut engine, "(get-strategy)"), "depth");
    assert!(engine.find_facts("fired").unwrap().is_empty());
}

#[cfg(feature = "serde")]
#[test]
fn changed_strategy_and_pending_order_survive_both_snapshot_formats() {
    use ferric_rules_runtime::SerializationFormat;
    for format in [SerializationFormat::Json, SerializationFormat::Cbor] {
        let mut engine = Engine::with_rules(SOURCE).unwrap();
        engine.reset().unwrap();
        engine.eval_str("(set-strategy breadth)").unwrap();
        let mut restored = Engine::deserialize(&engine.serialize(format).unwrap(), format).unwrap();
        assert_eq!(strategy_value(&mut restored, "(get-strategy)"), "breadth");
        assert_eq!(restored.run(RunLimit::Unlimited).unwrap().rules_fired, 3);
        assert_eq!(restored.get_output("t"), Some("1\n2\n3\n"));
    }
}
