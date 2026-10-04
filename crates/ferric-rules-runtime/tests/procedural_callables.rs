//! Callable load validation supports forward declarations without publishing bad bodies.

use ferric_rules_runtime::evaluator::EvalError;
use ferric_rules_runtime::{ActionError, Engine, EngineConfig, HaltReason, RunLimit};

fn run_output(engine: &mut Engine, action: &str) -> String {
    engine
        .load_str(&format!("(defrule exercise => {action})"))
        .unwrap();
    let module = engine.current_module().to_string();
    engine.reset().unwrap();
    engine.set_focus(&module).unwrap();
    let result = engine.run(RunLimit::Count(10)).unwrap();
    assert_eq!(result.halt_reason, HaltReason::AgendaEmpty);
    assert_eq!(result.rules_fired, 1);
    assert!(engine.action_diagnostics().is_empty());
    engine.get_output("t").unwrap_or_default().to_string()
}

#[test]
fn functions_and_methods_resolve_forward_calls_and_mutual_recursion() {
    let mut engine = Engine::new(EngineConfig::default());
    engine
        .load_str(
            r"
            (deffunction top (?x) (twice ?x))
            (deffunction even (?n) (if (= ?n 0) then TRUE else (odd (- ?n 1))))
            (deffunction odd (?n) (if (= ?n 0) then FALSE else (even (- ?n 1))))
            (defmethod later ((?x INTEGER)) (twice ?x))
            (deffunction twice (?x) (+ ?x ?x))
            ",
        )
        .unwrap();
    assert_eq!(
        run_output(
            &mut engine,
            r#"(printout t (top 3) " " (even 4) " " (odd 4) " " (later 5) crlf)"#,
        ),
        "6 TRUE FALSE 10\n"
    );
}

#[test]
fn nested_and_qualified_unknown_calls_reject_functions_and_methods() {
    for expression in [
        "(+ (missing ?x) 1)",
        "(MAIN::missing ?x)",
        "(UNKNOWN::f ?x)",
    ] {
        for definition in [
            format!("(deffunction rejected (?x) {expression})"),
            format!("(defmethod rejected ((?x INTEGER)) {expression})"),
        ] {
            let mut engine = Engine::new(EngineConfig::default());
            let errors = engine.load_str(&definition).expect_err(&definition);
            assert!(
                errors
                    .iter()
                    .any(|error| error.to_string().contains("[EXPRNPSR3]")),
                "{definition}: {errors:?}"
            );
            assert!(engine.load_str("(defrule probe => (rejected 1))").is_err());
        }
    }
}

#[test]
fn invalid_function_replacement_preserves_the_previous_body() {
    let mut engine = Engine::new(EngineConfig::default());
    engine
        .load_str("(deffunction existing (?x) (+ ?x 1))")
        .unwrap();
    assert!(engine
        .load_str("(deffunction existing (?x) (+ (missing ?x) 1))")
        .is_err());
    assert_eq!(
        run_output(&mut engine, "(printout t (existing 6) crlf)"),
        "7\n"
    );
}

#[test]
fn invalid_new_callable_invalidates_its_callers_without_removing_valid_definitions() {
    let mut engine = Engine::new(EngineConfig::default());
    assert!(engine
        .load_str(
            r"
            (deffunction first () (second))
            (deffunction second () (third))
            (deffunction third () (missing))
            (deffunction independent () 7)
            "
        )
        .is_err());
    for name in ["first", "second", "third"] {
        assert!(
            engine
                .load_str(&format!("(defrule probe-{name} => ({name}))"))
                .is_err(),
            "invalid callable {name} remained registered"
        );
    }
    assert_eq!(
        run_output(&mut engine, "(printout t (independent) crlf)"),
        "7\n"
    );
}

#[test]
fn rejected_method_does_not_change_existing_dispatch() {
    let mut engine = Engine::new(EngineConfig::default());
    engine
        .load_str("(defmethod kind ((?x INTEGER)) 7)")
        .unwrap();
    assert!(engine
        .load_str("(defmethod kind ((?x SYMBOL)) (missing ?x))")
        .is_err());
    assert_eq!(run_output(&mut engine, "(printout t (kind 3) crlf)"), "7\n");

    engine.load_str("(defrule exercise => (kind a))").unwrap();
    engine.reset().unwrap();
    let result = engine.run(RunLimit::Count(10)).unwrap();
    assert_eq!(result.halt_reason, HaltReason::ActionError);
    assert!(matches!(
        engine.action_diagnostics(),
        [ActionError::Evaluator(EvalError::NoApplicableMethod { name, .. })] if name == "kind"
    ));
}

#[test]
fn unknown_calls_in_method_restriction_queries_are_rejected() {
    let mut engine = Engine::new(EngineConfig::default());
    let errors = engine
        .load_str("(defmethod rejected ((?x SYMBOL (missing ?x))) TRUE)")
        .expect_err("query callable must be validated");
    assert!(
        errors
            .iter()
            .any(|error| error.to_string().contains("[EXPRNPSR3]")),
        "{errors:?}"
    );
    assert!(engine.load_str("(defrule probe => (rejected a))").is_err());
}

#[test]
fn method_queries_validate_free_variables_including_nested_fact_queries() {
    for query in [
        "(> ?missing 0)",
        "(any-factp ((?f item)) (= ?missing 1))",
        "(any-factp ((?f item)) (= ?unknown:n 1))",
    ] {
        let mut engine = Engine::new(EngineConfig::default());
        let source = format!(
            "(deftemplate item (slot n))
             (defmethod rejected ((?x INTEGER {query})) TRUE)"
        );
        let errors = engine.load_str(&source).expect_err(query);
        assert!(
            errors
                .iter()
                .any(|error| error.to_string().contains("PRCCODE3")),
            "{query}: {errors:?}"
        );
    }
    let mut engine = Engine::new(EngineConfig::default());
    engine
        .load_str("(defmethod later ((?x SYMBOL (eq ?y allowed)) ?y) ?x)")
        .unwrap();
    assert_eq!(
        run_output(&mut engine, "(printout t (later valid allowed) crlf)"),
        "valid\n"
    );
}

#[test]
fn method_query_templates_must_exist_at_the_definition_site() {
    let mut engine = Engine::new(EngineConfig::default());
    let errors = engine
        .load_str(
            "(defmethod rejected ((?x INTEGER (any-factp ((?f item)) (= ?f:n 1)))) TRUE)
         (deftemplate item (slot n))",
        )
        .expect_err("a later template cannot repair the method's query");
    assert!(
        errors
            .iter()
            .any(|error| error.to_string().contains("item")),
        "{errors:?}"
    );
    assert!(engine.load_str("(defrule probe => (rejected 1))").is_err());
}

#[test]
fn unqualified_callable_names_require_module_visibility() {
    let mut engine = Engine::new(EngineConfig::default());
    engine
        .load_str(
            "(defmodule A (export ?ALL))
         (deffunction visible () 1)
         (defmodule B)",
        )
        .unwrap();
    let errors = engine
        .load_str("(deffunction rejected () (visible))")
        .unwrap_err();
    assert!(
        errors
            .iter()
            .any(|error| error.to_string().contains("EXPRNPSR3")),
        "{errors:?}"
    );
    engine
        .load_str("(deffunction qualified () (A::visible))")
        .unwrap();
    engine
        .load_str(
            "(defmodule C (import A ?ALL))
         (deffunction imported () (visible))",
        )
        .unwrap();
    assert_eq!(
        run_output(&mut engine, "(printout t (imported) crlf)"),
        "1\n"
    );
}

#[test]
fn query_callers_resolve_the_recovered_callable_kind() {
    for (definitions, kind) in [
        (
            "(deffunction predicate () (missing)) (defmethod predicate () TRUE)",
            "defgeneric",
        ),
        (
            "(defmethod predicate () (missing)) (deffunction predicate () TRUE)",
            "deffunction",
        ),
    ] {
        let mut engine = Engine::new(EngineConfig::default());
        let source = format!(
            "(defmodule A (export ?ALL)) {definitions}
             (defmodule B (import A {kind} ?ALL))
             (deftemplate item (slot n))
             (deffacts seed (item (n 1)))
             (deffunction caller () (any-factp ((?f item)) (predicate)))
             (defmethod method-caller ((?n INTEGER (any-factp ((?f item)) (predicate)))) ?n)"
        );
        assert!(engine.load_str(&source).is_err());
        assert_eq!(
            run_output(
                &mut engine,
                r#"(printout t (caller) " " (method-caller 7) crlf)"#
            ),
            "TRUE 7\n",
            "{kind}"
        );
    }
}

#[test]
fn query_caller_survives_after_an_invalid_import_stops_being_ambiguous() {
    let mut engine = Engine::new(EngineConfig::default());
    assert!(engine
        .load_str(
            "(defmodule A (export ?ALL))
         (deffunction predicate () TRUE)
         (defmodule B (export ?ALL))
         (deffunction predicate () (missing))
         (defmodule C (import A ?ALL) (import B ?ALL))
         (deftemplate item (slot n))
         (deffacts seed (item (n 1)))
         (deffunction caller () (any-factp ((?f item)) (predicate)))"
        )
        .is_err());
    assert_eq!(
        run_output(&mut engine, "(printout t (caller) crlf)"),
        "TRUE\n"
    );
}

#[test]
fn fact_and_slot_heads_do_not_create_callable_recovery_cycles() {
    for data_action in [
        "(assert (caller 1))",
        "(assert (item (caller 1)))",
        "(modify 1 (caller 1))",
        "(duplicate 1 (caller 1))",
    ] {
        let mut engine = Engine::new(EngineConfig::default());
        let source = format!(
            "(defmodule C (export ?ALL))
             (defmodule B (export ?ALL))
             (defmodule A (import B deffunction ?ALL) (export ?ALL))
             (defmodule C (export ?ALL))
             (defmethod h () TRUE)
             (deffunction h () TRUE)
             (defmodule B (import C deffunction ?ALL) (import A deffunction ?ALL) (export ?ALL))
             (deftemplate item (slot caller))
             (defmethod g () {data_action} (h))
             (deffunction g () TRUE)
             (defmodule A (import B deffunction ?ALL) (export ?ALL))
             (deffunction caller () (g))"
        );
        assert!(engine.load_str(&source).is_err());
        assert_eq!(
            run_output(&mut engine, "(printout t (caller) crlf)"),
            "TRUE\n",
            "{data_action}"
        );
    }
}
