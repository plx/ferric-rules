//! Dynamic construction preserves executing definitions and releases guards on exit.
use ferric_rules_runtime::{Engine, HaltReason, RunLimit};

fn run(source: &str) -> Engine {
    let mut engine = Engine::with_rules(source).unwrap();
    assert_eq!(
        engine.run(RunLimit::Count(20)).unwrap().halt_reason,
        HaltReason::AgendaEmpty,
        "{:?}",
        engine.action_diagnostics()
    );
    engine
}

#[test]
fn indirect_function_replacement_is_refused_until_the_caller_returns() {
    let engine = run(r#"
      (deffunction helper () (build "(deffunction MAIN::f () 99)"))
      (deffunction f () (printout t (helper) "|") (return 7))
      (defrule check =>
        (printout t (f) "|" (f) "|")
        (printout t (build "(deffunction f () 99)") "|" (f) crlf))"#);
    assert_eq!(engine.get_output("t"), Some("FALSE|7|FALSE|7|TRUE|99\n"));
    assert!(engine.get_output("werror").unwrap().contains("DFNXPSR4"));
}

#[test]
fn function_replacement_guard_includes_ordinary_argument_evaluation() {
    let engine = run(r#"
      (deffunction f (?x) ?x)
      (defrule check =>
        (printout t (f (build "(deffunction f (?x) 99)")) "|"
          (f 7) crlf))"#);
    assert_eq!(engine.get_output("t"), Some("FALSE|7\n"));
}

#[test]
fn failed_function_evaluation_releases_the_replacement_guard() {
    let mut engine =
        Engine::with_rules("(deffunction f () (/ 1 0)) (defrule fail => (f))").unwrap();
    assert_eq!(
        engine.run(RunLimit::Unlimited).unwrap().halt_reason,
        HaltReason::ActionError
    );
    engine
        .load_str(
            r#"(defrule install =>
      (printout t (build "(deffunction f () 99)") "|" (f) crlf))"#,
        )
        .unwrap();
    assert_eq!(
        engine.run(RunLimit::Unlimited).unwrap().halt_reason,
        HaltReason::AgendaEmpty
    );
    assert_eq!(engine.get_output("t"), Some("TRUE|99\n"));
}

#[test]
fn generic_queries_and_bodies_keep_their_generic_active() {
    let engine = run(r#"
      (defgeneric g)
      (defmethod g ((?x INTEGER
        (progn (printout t (build "(defmethod g ((?y INTEGER)) 99)") "|") TRUE)))
        (printout t (build "(defgeneric g)") "|") 8)
      (defrule check => (printout t (g 1) "|")
        (printout t (build "(defmethod g ((?y SYMBOL)) 99)") "|" (g a) crlf))"#);
    assert_eq!(engine.get_output("t"), Some("FALSE|FALSE|8|TRUE|99\n"));
    assert!(engine.get_output("werror").unwrap().contains("GENRCFUN1"));
}

#[test]
fn generic_and_funcall_arguments_can_replace_a_target_before_it_executes() {
    let engine = run(r#"
      (deffunction f (?x) ?x)
      (defgeneric g)
      (defmethod g (?x) ?x)
      (defrule check =>
        (printout t (g (progn (build "(defmethod g ((?x INTEGER)) 88)") 7)) "|"
          (funcall f (build "(deffunction f (?x) 99)")) "|"
          (funcall g (progn (build "(defmethod g ((?x SYMBOL)) 77)") a)) crlf))"#);
    assert_eq!(engine.get_output("t"), Some("88|99|77\n"));
}

#[test]
fn specific_method_arguments_keep_the_selected_generic_active() {
    let engine = run(r#"
      (defgeneric g)
      (defmethod g (?x) ?x)
      (defrule check =>
        (printout t (call-specific-method g 1
          (build "(defmethod g (?x) 99)")) "|" (g 7) crlf))"#);
    assert_eq!(engine.get_output("t"), Some("FALSE|7\n"));
}

#[test]
fn specific_method_index_effects_are_observed_before_the_generic_becomes_active() {
    let engine = run(r#"(defgeneric g) (defmethod g (?x) 7)
      (defrule check => (printout t (call-specific-method g
        (progn (build "(defmethod g 2 ((?x INTEGER)) 99)") 2) 3) crlf))"#);
    assert_eq!(engine.get_output("t"), Some("99\n"));
}

#[test]
fn source_generic_redefinition_keeps_the_documented_existing_method_boundary() {
    let mut engine = Engine::with_rules("(defgeneric g) (defmethod g 1 (?x) 7)").unwrap();
    assert!(engine.load_str("(defgeneric g)").is_err());
    assert!(engine.load_str("(defmethod g 1 (?x) 88)").is_err());
    engine
        .load_str(
            r#"(defmethod g (?x) 99)
          (defrule check => (printout t (g a) "|" (call-specific-method g 2 a) crlf))"#,
        )
        .unwrap();
    assert_eq!(
        engine.run(RunLimit::Unlimited).unwrap().halt_reason,
        HaltReason::AgendaEmpty
    );
    assert_eq!(engine.get_output("t"), Some("7|99\n"));
}
