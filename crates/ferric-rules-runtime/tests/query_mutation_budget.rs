use std::fmt::Write;

use ferric_rules_runtime::{Engine, EngineConfig, RunLimit};

fn run_with_budget(source: &str, budget: usize) -> Engine {
    let mut config = EngineConfig::utf8();
    config.max_action_loop_iterations = budget;
    let mut engine = Engine::new(config);
    engine.load_str(source).unwrap();
    engine.reset().unwrap();
    engine.run(RunLimit::Unlimited).unwrap();
    engine
}

fn assert_budget_error(engine: &Engine) {
    let errors = engine.action_diagnostics();
    assert_eq!(errors.len(), 1, "{errors:?}");
    assert!(
        errors[0]
            .to_string()
            .contains("action iteration limit exceeded"),
        "{errors:?}"
    );
}

#[test]
fn first_match_stops_before_expanding_a_large_product() {
    let mut facts = String::new();
    for i in 0..40 {
        write!(facts, "(item (value {i}))").unwrap();
    }
    let engine = run_with_budget(
        &format!(
            "(deftemplate item (slot value)) (deffacts seed {facts})
         (defrule probe =>
            (do-for-fact ((?a item) (?b item) (?c item) (?d item) (?e item)) TRUE
                (printout t ?a:value crlf)))"
        ),
        5,
    );
    assert!(
        engine.action_diagnostics().is_empty(),
        "{:?}",
        engine.action_diagnostics()
    );
    assert_eq!(engine.get_output("t").unwrap(), Some("0\n"));
}

#[test]
fn delayed_selection_is_bounded_before_any_body_executes() {
    let mut facts = String::new();
    for i in 0..40 {
        write!(facts, "(item (value {i}))").unwrap();
    }
    let engine = run_with_budget(
        &format!(
            "(deftemplate item (slot value)) (deffacts seed {facts})
         (defrule probe =>
            (delayed-do-for-all-facts ((?a item) (?b item) (?c item) (?d item) (?e item)) FALSE
                (printout t unexpected crlf))
            (printout t after crlf))"
        ),
        10,
    );
    assert_budget_error(&engine);
    assert_eq!(engine.get_output("t").unwrap().unwrap_or(""), "");
}

#[test]
fn immediate_live_assertions_cannot_evade_the_shared_budget() {
    let engine = run_with_budget(
        "(deftemplate item (slot value)) (deffacts seed (item (value 0)))
         (defrule probe =>
            (do-for-all-facts ((?f item)) TRUE
                (printout t ?f:value crlf)
                (assert (item (value (+ ?f:value 1)))))
            (printout t after crlf))",
        5,
    );
    assert_budget_error(&engine);
    assert_eq!(engine.get_output("t").unwrap(), Some("0\n1\n2\n3\n4\n"));
}

#[test]
fn delayed_bodies_share_the_selection_budget() {
    let source = "(deftemplate item (slot value))
        (deffacts seed (item (value 10)) (item (value 20)) (item (value 30)))
        (defrule probe =>
            (delayed-do-for-all-facts ((?f item)) TRUE
                (printout t ?f:value crlf))
            (printout t after crlf))";
    let exhausted = run_with_budget(source, 3);
    assert_budget_error(&exhausted);
    assert_eq!(exhausted.get_output("t").unwrap().unwrap_or(""), "");
    let complete = run_with_budget(source, 6);
    assert!(
        complete.action_diagnostics().is_empty(),
        "{:?}",
        complete.action_diagnostics()
    );
    assert_eq!(
        complete.get_output("t").unwrap(),
        Some("10\n20\n30\nafter\n")
    );
}

#[test]
fn nested_queries_use_the_enclosing_budget() {
    let engine = run_with_budget(
        "(deftemplate item (slot value))
         (deffacts seed (item (value 1)) (item (value 2)) (item (value 3)))
         (defrule probe =>
            (do-for-all-facts ((?a item)) TRUE
                (do-for-all-facts ((?b item)) TRUE (printout t ?b:value crlf)))
            (printout t after crlf))",
        4,
    );
    assert_budget_error(&engine);
    assert_eq!(engine.get_output("t").unwrap(), Some("1\n2\n3\n"));
}

#[test]
fn empty_action_queries_need_no_iteration_budget() {
    let engine = run_with_budget(
        "(deftemplate item (slot value))
         (defrule probe =>
            (do-for-all-facts ((?f item)) TRUE (printout t unexpected crlf))
            (delayed-do-for-all-facts ((?f item)) TRUE (printout t unexpected crlf))
            (printout t after crlf))",
        0,
    );
    assert!(
        engine.action_diagnostics().is_empty(),
        "{:?}",
        engine.action_diagnostics()
    );
    assert_eq!(engine.get_output("t").unwrap(), Some("after\n"));
}
