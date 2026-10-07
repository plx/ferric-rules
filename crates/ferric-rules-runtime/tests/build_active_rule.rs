//! A dynamically built replacement must not retire an executing RHS definition.
use ferric_rules_runtime::{Engine, RunLimit};

const SOURCE: &str = r#"
(deffunction replace-original ()
  (build "(defrule MAIN::original ?f <- (go ?n) => (retract ?f) (printout t new ?n crlf))"))
(defrule MAIN::original ?f <- (go ?n) =>
  (retract ?f)
  (printout t old ?n ":" (replace-original) crlf))
(defrule install (install) => (printout t "installed:" (replace-original) crlf))
"#;

#[test]
fn indirect_build_preserves_active_rule_and_allows_replacement_after_return() {
    let mut engine = Engine::with_rules(SOURCE).unwrap();
    for value in [1, 2] {
        engine.assert_ordered("go", [value]).unwrap();
        engine.run(RunLimit::Unlimited).unwrap();
    }
    assert_eq!(engine.get_output("t"), Some("old1:FALSE\nold2:FALSE\n"));
    assert!(engine.action_diagnostics().is_empty());
    assert!(engine
        .get_output("werror")
        .unwrap()
        .contains("Cannot redefine defrule"));
    engine.assert_ordered("install", ()).unwrap();
    engine.run(RunLimit::Unlimited).unwrap();
    engine.assert_ordered("go", [3]).unwrap();
    engine.run(RunLimit::Unlimited).unwrap();
    assert_eq!(
        engine.get_output("t"),
        Some("old1:FALSE\nold2:FALSE\ninstalled:TRUE\nnew3\n")
    );
    assert!(engine.action_diagnostics().is_empty());
}

#[test]
fn failed_rhs_releases_active_rule_guard() {
    let mut engine = Engine::with_rules(
        r#"
      (deffunction replace-original ()
        (build "(defrule MAIN::original ?f <- (go ?n) => (retract ?f) (printout t new ?n crlf))"))
      (deffunction fail () (/ 1 0))
      (defrule MAIN::original ?f <- (go ?n) => (retract ?f) (fail))
      (defrule install (install) => (printout t "installed:" (replace-original) crlf))
    "#,
    )
    .unwrap();
    engine.assert_ordered("go", [1]).unwrap();
    engine.run(RunLimit::Unlimited).unwrap();
    assert!(!engine.action_diagnostics().is_empty());
    engine.clear_action_diagnostics();
    engine.assert_ordered("install", ()).unwrap();
    engine.run(RunLimit::Unlimited).unwrap();
    engine.assert_ordered("go", [2]).unwrap();
    engine.run(RunLimit::Unlimited).unwrap();
    assert_eq!(engine.get_output("t"), Some("installed:TRUE\nnew2\n"));
    assert!(engine.action_diagnostics().is_empty());
}

#[test]
fn active_rule_name_does_not_block_another_modules_rule() {
    let mut engine = Engine::with_rules(
        r#"
        (defmodule B)
        (defrule same (unused) =>)
        (defmodule A)
        (deffunction replace-other () (build "(defrule B::same => (printout t replaced crlf))"))
        (defrule same => (printout t (replace-other) crlf))
    "#,
    )
    .unwrap();
    engine.push_focus("A").unwrap();
    engine.run(RunLimit::Unlimited).unwrap();
    assert_eq!(engine.get_output("t"), Some("TRUE\n"));
    engine.push_focus("B").unwrap();
    engine.run(RunLimit::Unlimited).unwrap();
    assert_eq!(engine.get_output("t"), Some("TRUE\nreplaced\n"));
    assert!(engine.action_diagnostics().is_empty());
}
