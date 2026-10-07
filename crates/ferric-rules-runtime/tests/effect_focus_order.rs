use ferric_rules_runtime::{Engine, EngineConfig, HaltReason, RunLimit};

fn focus_output(expression: &str) -> String {
    let mut engine = Engine::new(EngineConfig::utf8());
    engine
        .load_str(&format!(
            r#"
            (defmodule A)
            (defmodule B)
            (defmodule MAIN)
            (deffunction pick (?marker ?module) (printout t ?marker) ?module)
            (defrule probe =>
              (printout t "[" {expression} "]:" (implode$ (get-focus-stack)) crlf)
              (halt))
            "#
        ))
        .unwrap();
    engine.reset().unwrap();
    let result = engine.run(RunLimit::Count(5)).unwrap();
    assert_eq!(result.halt_reason, HaltReason::HaltRequested);
    assert!(engine.action_diagnostics().is_empty());
    engine.get_output("t").unwrap().to_owned()
}

#[test]
fn focus_evaluates_and_pushes_arguments_from_right_to_left() {
    assert_eq!(
        focus_output("(focus (pick L A) (pick R B))"),
        "[RLTRUE]:A B MAIN\n"
    );
}

#[test]
fn focus_failure_preserves_prior_pushes_and_skips_remaining_operands() {
    assert_eq!(
        focus_output("(focus (pick L A) (pick M MISSING) (pick R B))"),
        "[RMFALSE]:B MAIN\n"
    );
    assert_eq!(
        focus_output("(focus (pick L A) (pick M MISSING))"),
        "[MFALSE]:MAIN\n"
    );
}
