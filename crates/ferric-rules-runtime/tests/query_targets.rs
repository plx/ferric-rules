//! Fact-query alternatives preserve source order, lexical scope, and live cursors.

use ferric_rules_runtime::{Engine, EngineConfig, HaltReason, RunLimit};

fn run(source: &str) -> Engine {
    let mut engine = Engine::new(EngineConfig::utf8());
    engine.load_str(source).unwrap();
    engine.reset().unwrap();
    let result = engine.run(RunLimit::Count(1)).unwrap();
    assert_eq!(result.rules_fired, 1);
    assert_ne!(
        result.halt_reason,
        HaltReason::ActionError,
        "{:?}",
        engine.action_diagnostics()
    );
    engine
}

#[test]
fn mixed_ordered_and_template_targets_preserve_alternatives_and_duplicates() {
    let engine = run(r"
        (deftemplate p (slot x)) (deftemplate q (slot x))
        (deffacts seed (p (x 1)) (q (x 2)) (p (x 3)) (item 4 5))
        (defrule exercise =>
          (bind ?targets (create$ p p q p))
          (do-for-all-facts ((?f ?targets)) TRUE (printout t ?f:x))
          (do-for-all-facts ((?f item)) TRUE (printout t ?f:implied))
          (printout t (length$ (find-all-facts ((?a p q) (?b item p)) TRUE)) crlf))
    ");
    assert_eq!(engine.get_output("t"), Some("1313213(4 5)18\n"));
}

#[test]
fn restriction_expressions_run_once_before_empty_product_detection() {
    let engine = run(r"
        (deftemplate empty) (deftemplate p (slot x))
        (deffacts seed (p (x 1)) (p (x 2)))
        (deffunction target (?label ?name) (printout t ?label) ?name)
        (defrule exercise =>
          (printout t (any-factp ((?a (target A empty)) (?b (target B p) (target C p))) TRUE) crlf))
    ");
    assert_eq!(engine.get_output("t"), Some("ABCFALSE\n"));
}

#[test]
fn immediate_alternatives_observe_appends_and_skip_retracted_duplicate_targets() {
    let engine = run(r"
        (deftemplate p (slot x)) (deftemplate q (slot x))
        (deffacts seed (p (x 1)) (q (x 2)))
        (defrule exercise =>
          (do-for-all-facts ((?f p q)) TRUE
            (printout t ?f:x)
            (if (= ?f:x 1) then (assert (p (x 3)) (q (x 4)))))
          (printout t crlf)
          (do-for-all-facts ((?f p p q)) TRUE
            (printout t ?f:x)
            (if (= ?f:x 1) then (retract ?f))))
    ");
    assert_eq!(engine.get_output("t"), Some("1324\n13324"));
}

#[test]
fn reset_expires_each_target_chain_while_retaining_selected_outer_payloads() {
    let engine = run(r#"
        (deftemplate p (slot x)) (deftemplate q (slot x))
        (deftemplate r (slot x)) (deftemplate s (slot x))
        (deffacts seed (p (x 1)) (q (x 2)) (r (x 3)) (s (x 4)))
        (defrule exercise =>
          (bind ?did FALSE)
          (do-for-all-facts ((?a p q) (?b r s)) TRUE
            (if (not ?did) then (bind ?did TRUE) (reset))
            (printout t ?a:x ":" ?b:x ":" (fact-existp ?a) ":" (fact-existp ?b) crlf)))
    "#);
    assert_eq!(
        engine.get_output("t"),
        Some("1:3:FALSE:FALSE\n1:4:FALSE:TRUE\n2:3:TRUE:TRUE\n2:4:TRUE:TRUE\n")
    );
}

#[test]
fn delayed_alternatives_keep_owned_payloads_across_repeated_resets() {
    let engine = run(r#"
        (deftemplate p (slot x)) (deftemplate q (slot x))
        (deffacts seed (p (x 1)) (q (x 2)))
        (defrule exercise =>
          (delayed-do-for-all-facts ((?a p q) (?b p q)) TRUE
            (reset)
            (printout t ?a:x ":" ?b:x ":" (fact-existp ?a) crlf)))
    "#);
    // Retained values remain safe even where CLIPS 6.30 reuses freed fact storage.
    assert_eq!(
        engine.get_output("t"),
        Some("1:1:FALSE\n1:2:FALSE\n2:1:FALSE\n2:2:FALSE\n")
    );
}

#[test]
fn query_member_restrictions_reject_early_reads_but_allow_inner_lexical_shadows() {
    for restriction in ["?f", "?f:x", "(if TRUE then ?f else p)"] {
        let mut engine = Engine::new(EngineConfig::utf8());
        let errors = engine.load_str(&format!("(deftemplate p (slot x))\n(defrule bad => (find-all-facts ((?f {restriction})) TRUE))")).unwrap_err();
        assert!(
            errors.iter().any(|error| error
                .to_string()
                .contains("cannot be read in its restriction")),
            "{errors:?}"
        );
    }
    let engine = run(r"
        (deftemplate p (slot x)) (deftemplate q (slot x))
        (deffacts seed (p (x 1)) (q (x 2)))
        (defrule exercise =>
          (do-for-all-facts ((?f (progn$ (?f (create$ p)) ?f))) TRUE (printout t ?f:x))
          (do-for-all-facts ((?f (if (any-factp ((?f q)) (= ?f:x 2)) then p else q))) TRUE (printout t ?f:x))
          (do-for-all-facts ((?f (bind ?f p))) TRUE (printout t ?f:x crlf)))
    ");
    assert_eq!(engine.get_output("t"), Some("111\n"));
}

#[test]
fn lhs_query_members_shadow_hidden_existential_variables() {
    let engine = run(r"
        (deftemplate p (slot x)) (deftemplate q (slot x))
        (deffacts seed (q (x 2)) (p (x 1)))
        (defrule exercise (exists (p (x ?f)))
          (test (any-factp ((?f q)) (= ?f:x 2))) => (printout t matched crlf))
    ");
    assert_eq!(engine.get_output("t"), Some("matched\n"));
}

#[test]
fn source_order_failure_preserves_previous_definition_and_later_constructs() {
    let mut engine = Engine::new(EngineConfig::utf8());
    engine
        .load_str("(defrule keep => (printout t old crlf))")
        .unwrap();
    let errors = engine
        .load_str(
            r"
        (defrule keep (test (any-factp ((?f later)) TRUE)) (later ?) =>)
        (defrule good => (assert (later 7)))
    ",
        )
        .unwrap_err();
    assert!(
        errors
            .iter()
            .any(|error| error.to_string().contains("unknown template `later`")),
        "{errors:?}"
    );
    engine.reset().unwrap();
    assert_eq!(engine.run(RunLimit::Unlimited).unwrap().rules_fired, 2);
    assert_eq!(engine.get_output("t"), Some("old\n"));
    engine.load_str("(defrule inspect (later ?x) (test (any-factp ((?f later)) TRUE)) => (printout t ?x crlf))").unwrap();
    engine.run(RunLimit::Unlimited).unwrap();
    assert_eq!(engine.get_output("t"), Some("old\n7\n"));
}

#[test]
fn dynamic_source_checks_query_targets_before_later_declarations() {
    let mut engine = Engine::with_rules(
        r#"
        (defrule exercise =>
          (eval "(progn (any-factp ((?f later)) TRUE) (assert (later 1)))"))
    "#,
    )
    .unwrap();
    assert_eq!(
        engine.run(RunLimit::Unlimited).unwrap().halt_reason,
        HaltReason::ActionError
    );
    assert!(engine.action_diagnostics()[0]
        .to_string()
        .contains("unknown template `later`"));
    assert_eq!(engine.fact_count(), 0);

    let engine = run(r#"
        (defrule exercise =>
          (printout t (eval "(progn (assert (item 3)) (any-factp ((?f item)) (= (nth$ 1 ?f:implied) 3)))") crlf)
          (assert-string "(self (any-factp ((?f self)) TRUE))"))
    "#);
    assert_eq!(engine.get_output("t"), Some("TRUE\n"));
    assert_eq!(engine.fact_count(), 2);
}

#[test]
fn queued_deffacts_protect_ordered_query_targets_from_replacement() {
    let deffacts = "(deffacts d (holder (any-factp ((?f target)) TRUE)))";
    let template = "(deftemplate target (slot x))";
    let declared = || {
        let mut engine = Engine::new(EngineConfig::utf8());
        engine.load_str("(assert (target 1))").unwrap();
        // Reset leaves the ordered relation declared but without live facts.
        engine.reset().unwrap();
        engine
    };
    let assert_conflict = |error: String| {
        assert!(
            error.contains("cannot define template `target` while its ordered relation is in use"),
            "{error}"
        );
    };

    let mut engine = declared();
    let error = engine
        .load_str(&format!("{deffacts}\n{template}"))
        .unwrap_err();
    assert_conflict(format!("{error:?}"));

    let mut engine = declared();
    engine.load_str(deffacts).unwrap();
    let error = engine.load_str(template).unwrap_err();
    assert_conflict(format!("{error:?}"));
}
