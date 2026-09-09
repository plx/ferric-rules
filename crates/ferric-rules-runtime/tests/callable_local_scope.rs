use ferric_rules_runtime::{Engine, EngineConfig, HaltReason, RunLimit, Value};

fn assert_clean_output(engine: &mut Engine, expected: &str) {
    assert_eq!(
        engine.run(RunLimit::Unlimited).unwrap().halt_reason,
        HaltReason::AgendaEmpty
    );
    assert!(
        engine.action_diagnostics().is_empty(),
        "{:?}",
        engine.action_diagnostics()
    );
    assert_eq!(engine.get_output("t").unwrap().unwrap_or(""), expected);
}

fn assert_action_error(engine: &mut Engine, expected: &str) {
    assert_eq!(
        engine.run(RunLimit::Unlimited).unwrap().halt_reason,
        HaltReason::ActionError
    );
    let errors = engine.action_diagnostics();
    assert_eq!(errors.len(), 1, "{errors:?}");
    assert!(errors[0].to_string().contains(expected), "{errors:?}");
}

#[test]
fn global_initializers_get_independent_callable_frames_after_success_and_error() {
    let mut engine = Engine::with_rules(
        "(deffunction calculate (?fail ?x)
            (bind ?saved (+ ?x 1))
            (bind ?x 99)
            (if ?fail then (/ 1 0) else (* ?saved 2)))
         (defglobal ?*a* = (calculate FALSE 3) ?*b* = (calculate FALSE 5))",
    )
    .unwrap();
    assert!(matches!(engine.get_global("a"), Some(Value::Integer(8))));
    assert!(matches!(engine.get_global("b"), Some(Value::Integer(12))));

    let errors = engine
        .load_str("(defglobal ?*failed* = (calculate TRUE 7))")
        .unwrap_err();
    assert!(
        errors
            .iter()
            .any(|error| error.to_string().contains("division by zero")),
        "{errors:?}"
    );
    assert!(engine.get_global("failed").is_none());
    engine
        .load_str(
            "(defglobal ?*c* = (calculate FALSE 9))
             (defrule probe =>
                (printout t ?*a* \":\" ?*b* \":\" ?*c* \":\"
                    (calculate FALSE 11) crlf))",
        )
        .unwrap();
    assert_clean_output(&mut engine, "8:12:20:24\n");
}

#[test]
fn locals_stay_with_the_invocation_and_globals_use_the_definition_module() {
    let mut engine = Engine::with_rules(
        "(defmodule MATH (export deffunction ?ALL))
         (defglobal ?*offset* = 100)
         (deffunction adjust (?x)
            (bind ?local (+ ?x ?*offset*))
            (bind ?x -1)
            ?local)
         (defmodule MAIN (import MATH deffunction ?ALL))
         (defglobal ?*offset* = 1)
         (deffunction caller (?x)
            (bind ?local (+ ?x ?*offset*))
            (bind ?callee (adjust ?x))
            (create$ ?local ?callee ?x))
         (defrule probe =>
            (printout t (caller 3) \":\" (caller 5) \":\" ?*offset* crlf))",
    )
    .unwrap();
    assert_clean_output(&mut engine, "(4 103 3):(6 105 5):1\n");
}

#[test]
fn returned_and_failed_invocations_do_not_leave_locals_in_later_calls() {
    for mode in ["return", "error"] {
        let mut engine = Engine::with_rules(&format!(
            "(deffunction attempt (?mode ?x)
                (if (eq ?mode return) then (bind ?temporary 99) (return ?temporary))
                (if (eq ?mode error) then (bind ?temporary 88) (/ 1 0))
                (if (eq ?mode read) then ?temporary else (+ ?x 1)))
             (defrule first => (printout t (attempt {mode} 3) crlf))"
        ))
        .unwrap();
        if mode == "return" {
            assert_clean_output(&mut engine, "99\n");
        } else {
            assert_action_error(&mut engine, "division by zero");
        }
        engine.clear_action_diagnostics();
        engine.clear_output_channel("t");

        // The local name is valid source syntax because another branch binds
        // it. A new invocation must still find it unbound on this branch.
        engine
            .load_str("(defrule read-next => (attempt read 5))")
            .unwrap();
        assert_action_error(&mut engine, "unbound variable `?temporary`");
        engine.clear_action_diagnostics();
        engine
            .load_str("(defrule recovered => (printout t (attempt normal 7) crlf))")
            .unwrap();
        assert_clean_output(&mut engine, "8\n");
    }
}

#[test]
fn nested_callable_locals_share_the_iteration_budget_and_recover_next_run() {
    let mut config = EngineConfig::utf8();
    config.max_action_loop_iterations = 5;
    let mut engine = Engine::with_rules_config(
        "(deffunction worker (?n)
            (bind ?sum 0)
            (loop-for-count (?i 1 ?n) (bind ?sum (+ ?sum 1)))
            ?sum)
         (deffunction nested ()
            (bind ?sum 0)
            (loop-for-count (?i 1 3) (bind ?sum (+ ?sum (worker 2))))
            ?sum)
         (defrule exhaust => (nested) (printout t unexpected crlf))",
        config,
    )
    .unwrap();
    assert_action_error(&mut engine, "action iteration limit exceeded");
    assert_eq!(engine.get_output("t").unwrap().unwrap_or(""), "");
    engine.clear_action_diagnostics();
    engine
        .load_str("(defrule recovered => (printout t (worker 2) crlf))")
        .unwrap();
    assert_clean_output(&mut engine, "2\n");
}

#[test]
fn recursive_local_frames_drop_after_depth_exhaustion() {
    let mut config = EngineConfig::utf8();
    config.max_call_depth = 3;
    let mut engine = Engine::with_rules_config(
        "(deffunction descend (?n)
            (bind ?saved ?n)
            (if (> ?n 0)
                then (bind ?child (descend (- ?n 1)))
                else (bind ?child 0))
            (+ ?saved ?child))
         (defrule exhaust => (descend 8) (printout t unexpected crlf))",
        config,
    )
    .unwrap();
    assert_action_error(&mut engine, "recursion limit exceeded");
    assert_eq!(engine.get_output("t").unwrap().unwrap_or(""), "");
    engine.clear_action_diagnostics();
    engine
        .load_str("(defrule recovered => (printout t (descend 1) crlf))")
        .unwrap();
    assert_clean_output(&mut engine, "1\n");
}
