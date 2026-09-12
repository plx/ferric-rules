//! Source arity checks are separate from direct `RuntimeExpr` error defaults.
//! These tests guard the loader's expression/data distinction, not broad
//! builtin arity validation or exact CLIPS diagnostic wording.

use ferric_rules_runtime::{Engine, EngineConfig, HaltReason, RunLimit};

#[test]
fn read_arity_is_rejected_in_rule_callable_method_global_and_lhs_expressions() {
    for name in ["read", "readline"] {
        for template in [
            "(defrule bad => (BUILTIN t t))",
            "(defrule bad => (if TRUE then (printout t (BUILTIN t t))))",
            "(defrule bad => (while FALSE do (BUILTIN t t)))",
            "(deffunction bad () (if FALSE then 1 else (BUILTIN t t)))",
            "(defmethod bad () (BUILTIN t t))",
            "(defglobal ?*bad* = (if FALSE then (BUILTIN t t) else 1))",
            "(defrule bad (test (BUILTIN t t)) =>)",
        ] {
            let mut engine = Engine::new(EngineConfig::default());
            let source = template.replace("BUILTIN", name);
            let errors = engine.load_str(&source).expect_err(&source);
            assert!(
                errors.iter().any(|error| error.to_string().contains(name)),
                "{source}: {errors:?}"
            );
        }
    }
}

#[test]
fn fact_relation_and_template_slot_heads_named_read_remain_data() {
    let mut engine = Engine::with_rules(
        r"
        (deftemplate item (multislot read) (multislot readline))
        (defrule create =>
          (assert (read a b) (readline c d))
          (assert (item (read a b) (readline c d))))
        (defrule update ?f <- (item) (not (updated)) =>
          (assert (updated))
          (duplicate ?f (read e f) (readline g h))
          (modify ?f (read c d) (readline e f)))
        ",
    )
    .unwrap();
    let result = engine.run(RunLimit::Count(10)).unwrap();
    assert_eq!(result.halt_reason, HaltReason::AgendaEmpty);
    assert_eq!(result.rules_fired, 2);
    assert!(engine.action_diagnostics().is_empty());
}

#[test]
fn actual_read_calls_in_fact_and_slot_values_are_still_checked() {
    for name in ["read", "readline"] {
        for body in [
            "(assert (other (BUILTIN t t)))",
            "(assert (item (read (BUILTIN t t))))",
            "(modify ?f (read (BUILTIN t t)))",
            "(duplicate ?f (readline (BUILTIN t t)))",
        ] {
            let source = format!("(deftemplate item (multislot read) (multislot readline))\n(defrule bad ?f <- (item) => {})", body.replace("BUILTIN",name));
            let mut engine = Engine::new(EngineConfig::default());
            let errors = engine.load_str(&source).expect_err(&source);
            assert!(errors.iter().any(|error| error.to_string().contains(name)));
        }
    }
}

#[test]
fn invalid_read_arity_does_not_replace_a_callable_or_evaluate_global_initializers() {
    let mut engine = Engine::with_rules("(deffunction retained () 7)").unwrap();
    assert!(engine
        .load_str("(deffunction retained () (read t t))")
        .is_err());
    assert!(engine
        .load_str("(defglobal ?*first* = 1 ?*second* = (readline t t))")
        .is_err());
    assert!(engine.get_global("first").is_none());
    assert!(engine.get_global("second").is_none());
    engine
        .load_str("(defrule verify => (printout t (retained) crlf))")
        .unwrap();
    let result = engine.run(RunLimit::Count(10)).unwrap();
    assert_eq!(result.halt_reason, HaltReason::AgendaEmpty);
    assert_eq!(engine.get_output_bytes("t").unwrap_or(b""), b"7\n");
}

#[test]
fn callable_template_slot_heads_use_registered_local_or_visible_templates() {
    // Source acceptance only: callable assert execution is a separate boundary.
    for declarations in [
        "(deftemplate item (multislot read))",
        "(defmodule M (export ?ALL))
(deftemplate item (multislot read))
(defmodule APP (import M ?ALL))",
    ] {
        let source = format!("{declarations}\n(deffunction f () (assert (item (read a b))))");
        let mut engine = Engine::new(EngineConfig::default());
        let loaded = engine.load_str(&source).expect(&source);
        assert_eq!(loaded.functions.len(), 1);
    }
}

#[test]
fn later_templates_do_not_hide_read_arity_errors_in_callable_source() {
    // CLIPS classifies the fact at the callable's declaration, before the
    // later template exists. This tests source rejection, not body execution.
    for declaration in [
        "(deffunction f () (assert (item (read a b))))",
        "(defmethod f () (assert (item (read a b))))",
    ] {
        let source = format!("{declaration}\n(deftemplate item (multislot read))");
        let mut engine = Engine::new(EngineConfig::default());
        let errors = engine.load_str(&source).expect_err(&source);
        assert!(
            errors
                .iter()
                .any(|error| error.to_string().contains("read expects 0 or 1 arguments")),
            "{source}: {errors:?}"
        );
    }
}
