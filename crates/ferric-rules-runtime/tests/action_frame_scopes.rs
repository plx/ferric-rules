//! Temporary action frames retain scope, alias, and introspection behavior.

use ferric_rules_runtime::{Engine, HaltReason, RunLimit};

fn output(source: &str) -> String {
    let mut engine = Engine::with_rules(source).unwrap();
    let result = engine.run(RunLimit::Unlimited).unwrap();
    assert_eq!(result.halt_reason, HaltReason::AgendaEmpty);
    assert_eq!(result.rules_fired, 1);
    assert!(engine.action_diagnostics().is_empty());
    engine.get_output("t").unwrap().unwrap_or("").to_owned()
}

#[test]
fn nested_counters_shadow_without_overwriting_the_outer_counter() {
    assert_eq!(
        output(
            r#"
        (defrule scopes =>
            (loop-for-count (?i 1 2)
                (printout t ?i ":")
                (loop-for-count (?i 7 8) (printout t ?i))
                (printout t ?i "|")))"#
        ),
        "1:781|2:782|"
    );
}

#[test]
fn runtime_bindings_keep_precedence_over_counter_updates() {
    assert_eq!(
        output(
            r#"
        (defrule rebound =>
            (loop-for-count (?i 1 3)
                (printout t ?i "|")
                (bind ?i 99)))"#
        ),
        "1|99|99|"
    );
}

#[test]
fn progn_keeps_the_evaluated_list_when_its_source_global_changes() {
    assert_eq!(
        output(
            r#"
        (defglobal ?*items* = (create$ 2 4))
        (defrule snapshot =>
            (progn$ (?item ?*items*)
                (printout t ?item ":" ?item-index "|")
                (bind ?*items* (create$ 9))))"#
        ),
        "2:1|4:2|"
    );
}

#[test]
fn local_multifield_aliases_override_matched_tail_bindings() {
    assert_eq!(
        output(
            r#"
        (deffacts seed (payload 5 7))
        (defrule aliases (payload $?items) =>
            (bind ?items (create$ 2 3))
            (loop-for-count (?i 1 2)
                (printout t (length$ $?items) ":" (nth$ ?i ?items) "|")))"#
        ),
        "2:2|2:3|"
    );
}

#[test]
fn rule_introspection_inside_a_loop_uses_the_registered_source() {
    let source = "(defrule inspect => (loop-for-count (?i 1 1) (ppdefrule inspect)))";
    assert_eq!(output(source), format!("{source}\n"));
}
