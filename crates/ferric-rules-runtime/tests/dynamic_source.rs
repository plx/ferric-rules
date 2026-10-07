//! Dynamic builtins preserve caller limits while isolating textual local variables.
use ferric_rules_runtime::{Engine, EngineConfig, HaltReason, RunLimit, Value};

fn run(source: &str) -> Engine {
    let mut engine = Engine::with_rules(source).unwrap();
    let result = engine.run(RunLimit::Count(30)).unwrap();
    assert_eq!(
        result.halt_reason,
        HaltReason::AgendaEmpty,
        "{:?}",
        engine.action_diagnostics()
    );
    engine
}

#[test]
fn issue_403_composed_builtin_example() {
    let engine = run(r#"(defrule r => (seed 42)
      (printout t (eval "(+ 1 2)") " "
        (delete-member$ (create$ a b a c) a) " "
        (+ (expand$ (create$ 1 2 3))) crlf))"#);
    assert_eq!(engine.get_output("t"), Some("3 (b c) 6\n"));
}

#[test]
fn eval_and_assert_string_evaluate_first_form_and_ignore_trailing_source() {
    let engine = run(r#"(deftemplate p (slot x (default 4)))
      (defrule run =>
        (printout t (eval "(+ 1 2) (other)") " " (eval "42 )") "|")
        (printout t (fact-slot-value (assert-string "(p)") x) " "
          (fact-slot-value (str-assert "(p (x (+ 3 4))) (p (x 9))") x) crlf))"#);
    assert_eq!(engine.get_output("t"), Some("3 42|4 7\n"));
    assert_eq!(engine.fact_count(), 2);
}

#[test]
fn build_installs_executable_constructs_during_a_run() {
    let engine = run(r#"(defrule run =>
      (printout t (build "(deftemplate p (slot n))") " ")
      (printout t (build "(defrule later (p (n ?n)) => (printout t later ?n crlf))") crlf)
      (assert-string "(p (n 8))"))"#);
    assert_eq!(engine.get_output("t"), Some("TRUE TRUE\nlater8\n"));
    assert_eq!(engine.fact_count(), 1);
}

#[test]
fn build_failure_returns_false_and_preserves_a_previous_definition() {
    let engine = run(r#"(deftemplate p (slot n (default 7)))
      (defrule run =>
        (printout t (build "(deftemplate p (slot n (type INTEGER) (default bad)))") " ")
        (printout t (build "(assert (p))") " ")
        (printout t (fact-slot-value (assert-string "(p)") n) crlf))"#);
    assert_eq!(engine.get_output("t"), Some("FALSE FALSE 7\n"));
}

#[test]
fn textual_eval_cannot_capture_or_modify_a_callers_local_bindings() {
    let mut engine = Engine::with_rules(
        r#"(deffunction f (?x) (eval "?x"))
      (defrule run => (printout t prefix ":" (f 7) "after"))"#,
    )
    .unwrap();
    assert_eq!(
        engine.run(RunLimit::Unlimited).unwrap().halt_reason,
        HaltReason::ActionError
    );
    assert_eq!(engine.get_output("t"), Some("prefix:"));
    let engine = run(r#"(defrule run => (bind ?x 9)
      (printout t (eval "(bind ?x 4)") " " ?x " ")
      (printout t (eval "(loop-for-count (?i 1 2) (printout t ?i))") crlf))"#);
    assert_eq!(engine.get_output("t"), Some("4 9 12FALSE\n"));
}

#[test]
fn dynamic_source_retains_effect_restrictions_and_loop_limits() {
    let mut engine = Engine::with_rules(
        r#"(deffunction effect () (eval "(assert (bad))"))
      (defrule run (test (effect)) => (assert (after)))"#,
    )
    .unwrap();
    engine.run(RunLimit::Unlimited).unwrap();
    assert_eq!(engine.fact_count(), 0);
    let mut config = EngineConfig::default();
    config.max_action_loop_iterations = 3;
    let mut engine = Engine::new(config);
    engine
        .load_str(r#"(defrule run => (loop-for-count 2 (eval "(loop-for-count 2 1)")))"#)
        .unwrap();
    engine.reset().unwrap();
    assert_eq!(
        engine.run(RunLimit::Unlimited).unwrap().halt_reason,
        HaltReason::ActionError
    );
    assert!(engine.action_diagnostics()[0]
        .to_string()
        .contains("iteration"));
}

#[test]
fn construct_initializers_reject_reentrant_build_without_corrupting_templates() {
    for source in [
        r#"(deftemplate p (slot x (default (build "(deftemplate p (slot y))"))))"#,
        r#"(defglobal ?*g* = (build "(deftemplate p)"))"#,
    ] {
        let mut engine = Engine::new(EngineConfig::default());
        assert!(engine.load_str(source).is_err());
        engine
            .load_str("(deftemplate p (slot n (default 7)))")
            .unwrap();
        let fact = engine.assert_template("p", &[], ()).unwrap();
        assert!(matches!(
            engine.get_fact_slot_by_name(fact, "n").unwrap(),
            Value::Integer(7)
        ));
    }
    let mut engine = Engine::new(EngineConfig::default());
    engine
        .load_str(r#"(assert (created (build "(deftemplate p (slot n))")))"#)
        .unwrap();
    engine.assert_template("p", &[], ()).unwrap();
}

// A `build` reached while a fact of the template is being assembled must not
// redefine that template. CLIPS 6.30 rejects the redefinition, keeps the fact
// as `(x)` and halts the rule at the `modify` of a slot the template lacks:
//
//   [CSTRCPSR4] Cannot redefine deftemplate p while it is in use.
//
//   ERROR:
//   (deftemplate MAIN::p
//   (x)
//
//   [TMPLTDEF1] Invalid slot z not defined in corresponding deftemplate p.
//   [PRCCODE4] Execution halted during the actions of defrule r.
//
// These stay Rust tests rather than a corpus golden: Ferric reports the
// CSTRCPSR4 rejection but does not echo the failed construct (`ERROR:` and
// `(deftemplate MAIN::p`) as CLIPS's parser does.
fn assert_rejected_template_redefinition(source: &str) {
    let mut engine = Engine::with_rules(source).unwrap();
    assert_eq!(
        engine.run(RunLimit::Count(30)).unwrap().halt_reason,
        HaltReason::ActionError
    );
    assert_eq!(engine.get_output("t"), Some("FALSE (x)\n"));
    assert!(engine
        .get_output("werror")
        .is_some_and(|text| text.contains("CSTRCPSR4")));
    assert!(format!("{:?}", engine.action_diagnostics()).contains("modify"));
}

#[test]
fn eval_assert_cannot_redefine_its_template_from_a_slot_expression() {
    assert_rejected_template_redefinition(
        r#"(deftemplate p (slot x))
      (deffunction mk ()
        (eval "(assert (p (x (build \"(deftemplate p (slot y) (slot z))\"))))"))
      (defrule r => (bind ?f (mk))
        (printout t (fact-slot-value ?f x) " " (fact-slot-names ?f) crlf)
        (modify ?f (z 3)))"#,
    );
}

#[test]
fn a_dynamic_default_cannot_redefine_the_template_it_is_filling() {
    assert_rejected_template_redefinition(
        r#"(deftemplate p
        (slot x (default-dynamic (build "(deftemplate p (slot y) (slot z))"))))
      (defrule r => (bind ?f (eval "(assert (p))"))
        (printout t (fact-slot-value ?f x) " " (fact-slot-names ?f) crlf)
        (modify ?f (z 3)))"#,
    );
}

#[test]
fn host_template_assertion_keeps_its_template_while_defaults_build() {
    let mut engine = Engine::with_rules(
        r#"(deftemplate p
        (slot x (default-dynamic (build "(deftemplate p (slot y) (slot z))"))))"#,
    )
    .unwrap();
    let fact = engine.assert_template("p", &[], ()).unwrap();
    assert!(matches!(
        engine.get_fact_slot_by_name(fact, "x").unwrap(),
        Value::Symbol(_)
    ));
    assert!(engine.get_fact_slot_by_name(fact, "z").is_err());
    assert!(engine
        .get_output("werror")
        .is_some_and(|text| text.contains("CSTRCPSR4")));
    engine
        .load_str(
            "(defrule r ?f <- (p (x ?x))
          => (printout t ?x \" \" (fact-slot-names ?f) crlf) (modify ?f (z 3)))",
        )
        .unwrap();
    assert_eq!(
        engine.run(RunLimit::Count(30)).unwrap().halt_reason,
        HaltReason::ActionError
    );
    assert_eq!(engine.get_output("t"), Some("FALSE (x)\n"));
    assert!(format!("{:?}", engine.action_diagnostics()).contains("modify"));
}

// The same holds for an ordered relation that only dynamically evaluated
// source names: its fact keeps the relation implied while its fields run
// `build`. CLIPS 6.30 prints the following; Ferric reports its own rejection
// text on werror instead of the CSTRCPSR4 diagnostic and construct echo:
//
//   [CSTRCPSR4] Cannot redefine deftemplate p while it is in use.
//
//   ERROR:
//   (deftemplate MAIN::p
//   p (implied) (implied)
//   later FALSE
#[test]
fn eval_assert_keeps_its_ordered_relation_implied_while_fields_build() {
    let engine = run(r#"(defrule r =>
      (bind ?f (eval "(assert (p (build \"(deftemplate p (slot x))\")))"))
      (printout t (fact-relation ?f) " " (fact-slot-names ?f) " "
        (deftemplate-slot-names p) crlf)
      (build "(defrule later (p ?x) => (printout t later \" \" ?x crlf))"))"#);
    assert_eq!(
        engine.get_output("t"),
        Some("p (implied) (implied)\nlater FALSE\n")
    );
    assert!(engine
        .get_output("werror")
        .is_some_and(|text| text.contains("ordered relation is in use")));
}

// A top-level assertion holds its relation the same way. CLIPS 6.30 rejects
// the build and `(deftemplate-slot-names p)` is then `(implied)`.
#[test]
fn top_level_assert_keeps_its_ordered_relation_implied_while_fields_build() {
    let mut engine = Engine::new(EngineConfig::utf8());
    engine
        .load_str(r#"(assert (p (build "(deftemplate p (slot x))")))"#)
        .unwrap();
    assert!(engine
        .get_output("werror")
        .is_some_and(|text| text.contains("ordered relation is in use")));
    engine
        .load_str("(defrule show => (printout t (deftemplate-slot-names p) crlf))")
        .unwrap();
    engine.reset().unwrap();
    engine.run(RunLimit::Count(30)).unwrap();
    assert_eq!(engine.get_output("t"), Some("(implied)\n"));
}
