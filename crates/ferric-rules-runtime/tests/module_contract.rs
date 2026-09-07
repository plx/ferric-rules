//! Basic module visibility and focus lifecycle regressions against CLIPS6.30.
use ferric_rules_runtime::{Engine, EngineConfig, RunLimit, Value};

#[test]
fn omitted_exports_including_default_main_expose_nothing() {
    for source in [
        "(deftemplate item (slot key)) (defmodule APP (import MAIN ?ALL))",
        "(defmodule DATA) (deftemplate item (slot key)) (defmodule APP (import DATA ?ALL))",
        "(defmodule DATA (export ?NONE)) (deftemplate item (slot key)) (defmodule APP (import DATA ?ALL))",
    ] {
        let mut engine = Engine::new(EngineConfig::default());
        let error = engine.load_str(source).unwrap_err();
        assert!(error.iter().any(|e| e.to_string().contains("does not export")), "{error:?}");
        assert!(!engine.modules().contains(&"APP"));
        assert_eq!(engine.templates().len(), 1);
    }
}

#[test]
fn explicit_exports_and_imports_enable_template_matching() {
    for exports in ["?ALL", "deftemplate item", "deftemplate ?ALL"] {
        let mut engine = Engine::with_rules(&format!(
            r#"
            (defmodule DATA (export {exports}))
            (deftemplate item (slot key))
            (defmodule APP (import DATA deftemplate item))
            (deffacts inputs (item (key 7)))
            (defrule print-item (item (key ?key)) => (printout t ?key "|"))
        "#
        ))
        .unwrap();
        engine.push_focus("APP").unwrap();
        assert_eq!(engine.run(RunLimit::Unlimited).unwrap().rules_fired, 1);
        assert_eq!(engine.get_output("t"), Some("7|"));
        assert_eq!(engine.get_focus(), None);
    }
}

#[test]
fn exhausted_focus_is_empty_and_a_new_run_defaults_to_main() {
    let mut engine = Engine::with_rules(
        r#"
        (defrule consume ?f <- (item ?n) => (retract ?f) (printout t ?n "|"))
    "#,
    )
    .unwrap();
    assert_eq!(engine.run(RunLimit::Unlimited).unwrap().rules_fired, 0);
    assert_eq!(engine.get_focus(), None);
    engine.assert_ordered("item", Value::Integer(1)).unwrap();
    assert_eq!(engine.run(RunLimit::Unlimited).unwrap().rules_fired, 1);
    assert_eq!(engine.get_focus(), None);
    engine.assert_ordered("item", Value::Integer(2)).unwrap();
    assert_eq!(engine.run(RunLimit::Count(0)).unwrap().rules_fired, 0);
    assert_eq!(engine.get_focus(), Some("MAIN"));
    assert_eq!(engine.run(RunLimit::Unlimited).unwrap().rules_fired, 1);
    assert_eq!(engine.get_output("t"), Some("1|2|"));
    assert!(engine.get_focus_stack().is_empty());
}

#[test]
fn focus_is_visible_to_later_rhs_actions_and_each_call_keeps_argument_order() {
    let mut engine = Engine::with_rules(
        r#"
        (defmodule A)
        (defmodule B)
        (defmodule C)
        (defmodule MAIN)
        (defrule start (go) =>
            (focus A B)
            (printout t (get-focus) "|")
            (list-focus-stack)
            (focus C)
            (printout t (get-focus) "|")
            (list-focus-stack))
        "#,
    )
    .unwrap();
    engine.assert_ordered("go", Vec::<Value>::new()).unwrap();
    assert_eq!(engine.run(RunLimit::Count(1)).unwrap().rules_fired, 1);
    assert_eq!(
        engine.get_output("t"),
        Some("A|A\nB\nMAIN\nC|C\nA\nB\nMAIN\n")
    );
    assert_eq!(engine.get_focus_stack(), vec!["MAIN", "B", "A", "C"]);
}

#[test]
fn a_test_ce_cannot_change_focus() {
    let mut engine = Engine::new(EngineConfig::default());
    // Focus is an action, not an expression builtin. Predicate evaluation has
    // immutable module access and must reject this call without changing it.
    let loaded = engine.load_str("(defmodule A) (defmodule MAIN) (defrule invalid (go) (test (focus A)) => (printout t bad))");
    if loaded.is_ok() {
        engine.assert_ordered("go", Vec::<Value>::new()).unwrap();
        assert!(engine.agenda_len() == 0);
        assert!(!engine.action_diagnostics().is_empty());
    }
    assert_eq!(engine.get_focus_stack(), vec!["MAIN"]);
    assert_eq!(engine.get_output("t"), None);
}
