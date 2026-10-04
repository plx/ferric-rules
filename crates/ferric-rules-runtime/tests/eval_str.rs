use ferric_rules_core::Value;
use ferric_rules_runtime::{Engine, EngineConfig, EvalStrError};

#[test]
fn roots_have_fresh_locals_but_one_expression_can_bind_and_read() {
    let mut engine = Engine::new(EngineConfig::default());
    assert!(matches!(
        engine.eval_str("(+ 1 2)").unwrap(),
        Value::Integer(3)
    ));
    assert!(matches!(
        engine.eval_str("(bind ?x 3)").unwrap(),
        Value::Integer(3)
    ));
    assert!(matches!(
        engine.eval_str("?x"),
        Err(EvalStrError::Evaluation(_))
    ));
    assert!(matches!(
        engine.eval_str("(progn (bind ?x 3) (+ ?x 1))").unwrap(),
        Value::Integer(4)
    ));
    assert!(matches!(
        engine.eval_str("; comment\n 7 ; trailing\n").unwrap(),
        Value::Integer(7)
    ));
}

#[test]
fn multiple_forms_and_source_errors_do_not_evaluate_a_prefix() {
    let mut engine = Engine::new(EngineConfig::default());
    let error = engine
        .eval_str("(assert (unexpected))\n(+ 1 2)")
        .unwrap_err();
    assert!(matches!(error, EvalStrError::Source(_)));
    assert!(error.to_string().contains("line 2, column 1"));
    assert_eq!(engine.facts().unwrap().count(), 0);
    assert!(matches!(
        engine.eval_str("; empty"),
        Err(EvalStrError::Source(_))
    ));
    let error = engine.eval_str("\n(+ 1").unwrap_err();
    assert!(matches!(error, EvalStrError::Source(_)));
    let EvalStrError::Source(errors) = error else {
        unreachable!()
    };
    assert!(
        matches!(&errors[0], ferric_rules_runtime::LoadError::Parse(error) if error.span.start.line == 2)
    );
}

#[test]
fn runtime_errors_preserve_prior_output_and_effects() {
    let mut engine = Engine::new(EngineConfig::default());
    assert!(matches!(
        engine.eval_str("(progn (printout t kept) (assert (p 1)) (/ 1 0))"),
        Err(EvalStrError::Evaluation(_))
    ));
    assert_eq!(engine.get_output("t"), Some("kept"));
    assert_eq!(engine.facts().unwrap().count(), 1);
    assert!(matches!(
        engine.eval_str("(progn (printout t more) ?missing)"),
        Err(EvalStrError::Evaluation(_))
    ));
    assert_eq!(engine.get_output("t"), Some("keptmore"));
    assert!(matches!(
        engine.eval_str("(+ 2 3)").unwrap(),
        Value::Integer(5)
    ));
}

#[test]
fn root_clear_preserves_compiled_literals_output_input_and_stale_identity() {
    let mut engine = Engine::with_rules("(deffunction old () 9)").unwrap();
    engine.push_input("42");
    assert!(matches!(engine.eval_str(
        "(progn (bind ?old (assert (p))) (printout t before) (clear) (printout t after) (assert (q)) (fact-index ?old))"
    ).unwrap(), Value::Integer(-1)));
    assert_eq!(engine.get_output("t"), Some("beforeafter"));
    assert!(matches!(
        engine.eval_str("(read)").unwrap(),
        Value::Integer(42)
    ));
    assert!(matches!(
        engine.eval_str("(old)"),
        Err(EvalStrError::Source(_))
    ));
    assert!(matches!(
        engine.eval_str("(fact-index (assert (r)))").unwrap(),
        Value::Integer(2)
    ));
}

#[test]
fn in_use_callable_clear_keeps_the_callable_but_retracts_facts() {
    let mut engine = Engine::with_rules("(deffunction keep () (clear) 9)").unwrap();
    engine.eval_str("(assert (p))").unwrap();
    assert!(matches!(
        engine.eval_str("(keep)").unwrap(),
        Value::Integer(9)
    ));
    assert_eq!(engine.facts().unwrap().count(), 0);
    assert!(matches!(
        engine.eval_str("(keep)").unwrap(),
        Value::Integer(9)
    ));
}

#[test]
fn root_clear_resets_the_current_module_for_remaining_operands() {
    let mut engine = Engine::with_rules("(defmodule A)").unwrap();
    let value = engine
        .eval_str(r#"(progn (clear) (build "(deffunction after () 7)") (eval "(after)"))"#)
        .unwrap();
    assert!(matches!(value, Value::Integer(7)));
}

#[test]
fn clear_during_fact_initialization_keeps_old_facts_and_releases_guard_on_errors() {
    let mut engine = Engine::with_rules(
        "(deftemplate p (slot x (default-dynamic (progn (printout t default) (clear)))))",
    )
    .unwrap();
    engine.eval_str("(assert (old))").unwrap();
    engine.eval_str("(assert (p))").unwrap();
    engine.eval_str("(assert (ordered (clear)))").unwrap();
    assert_eq!(engine.facts().unwrap().count(), 3);
    assert_eq!(engine.get_output("t"), Some("default"));
    assert!(engine.get_output("werror").unwrap().contains("CONSTRCT1"));
    assert!(engine.eval_str("(assert (failed (/ 1 0)))").is_err());
    engine.eval_str("(clear)").unwrap();
    assert_eq!(engine.facts().unwrap().count(), 0);
    assert!(engine.eval_str("(assert (p (x 7)))").is_err());
}

#[test]
fn source_reset_selects_main_for_the_shell_and_remaining_root_operands() {
    let mut engine = Engine::new(EngineConfig::default());
    engine
        .load_str("(defrule main =>) (defmodule A) (defrule other =>)")
        .unwrap();
    assert_eq!(engine.current_module(), "A");
    engine.reset().unwrap();
    assert_eq!(engine.current_module(), "MAIN");
    engine.load_str("(defmodule A)").unwrap();
    engine
        .eval_str(r#"(progn (reset) (build "(deffunction after () 7)"))"#)
        .unwrap();
    assert_eq!(engine.current_module(), "MAIN");
    assert!(engine
        .agenda_entries()
        .iter()
        .all(|entry| entry.module_name == "MAIN"));
    assert!(matches!(
        engine.eval_str("(after)").unwrap(),
        Value::Integer(7)
    ));
}
