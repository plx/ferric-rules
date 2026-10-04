//! Existing built-ins agree with CLIPS value, failure, and source-validation rules.
use ferric_rules_runtime::{Engine, EngineConfig, HaltReason, RunLimit};

fn output(source: &str) -> String {
    let mut engine = Engine::new(EngineConfig::utf8());
    engine.load_str(source).unwrap();
    engine.run(RunLimit::Unlimited).unwrap();
    assert!(
        engine.action_diagnostics().is_empty(),
        "{:?}",
        engine.action_diagnostics()
    );
    engine.get_output("t").unwrap_or_default().to_owned()
}

#[test]
fn equality_is_variadic_type_sensitive_and_short_circuits() {
    assert_eq!(
        output(
            r#"(defrule check => (printout t
      (eq a a a) "|" (eq a a b) "|" (neq a b b) "|" (neq a b a) "|"
      (eq 1 1.0 1) "|" (eq a b (/ 1 0)) "|" (neq a a (/ 1 0)) "|"
      (eq (create$ a b) (create$ a b) (create$ a b)) crlf))"#
        ),
        "TRUE|FALSE|TRUE|FALSE|FALSE|FALSE|FALSE|TRUE\n"
    );
}

#[test]
fn modulo_preserves_fractional_operands_and_result_kind() {
    assert_eq!(
        output(
            r#"(defrule check => (printout t
      (mod 5 2) "|" (mod 5 2.0) "|" (mod 7.5 2) "|" (mod 5 0.5) "|"
      (mod -7.5 2) "|" (mod 7.5 -2) "|" (mod -7.5 -2) crlf))"#
        ),
        "1|1.0|1.5|0.0|-1.5|1.5|-1.5\n"
    );
}

#[test]
fn both_length_names_count_lexeme_bytes_and_multifield_fields() {
    assert_eq!(
        output(
            r#"(defrule check => (printout t
      (length abc) "|" (length$ abc) "|" (length "héllo") "|" (length$ "héllo") "|"
      (length (create$ a b c)) "|" (length$ (create$)) crlf))"#
        ),
        "3|3|6|6|3|0\n"
    );
}

#[test]
fn random_single_argument_returns_a_draw_without_evaluating_the_operand() {
    for expression in [
        "(random (progn (printout t unexpected) 1))",
        "(random (expand$ (create$ 1)))",
    ] {
        let mut engine = Engine::with_rules(&format!(
            "(defrule check => (seed 1) (printout t {expression} crlf (random) crlf))"
        ))
        .unwrap();
        assert_eq!(
            engine.run(RunLimit::Unlimited).unwrap().halt_reason,
            HaltReason::AgendaEmpty
        );
        assert_eq!(engine.get_output("t"), Some("1804289383\n846930886\n"));
        assert_eq!(
            engine.get_output("werror"),
            Some("[MISCFUN2] Function random expected either 0 or 2 arguments\n")
        );
        assert!(engine.action_diagnostics().is_empty());
    }
}

#[test]
fn excessive_expanded_random_arguments_fail_before_consuming_a_draw() {
    let mut engine =
        Engine::with_rules("(defrule check => (seed 1) (random (expand$ (create$ 1 2 3))))")
            .unwrap();
    assert_eq!(
        engine.run(RunLimit::Unlimited).unwrap().halt_reason,
        HaltReason::ActionError
    );
    engine
        .load_str("(defrule after => (printout t (random) crlf))")
        .unwrap();
    assert_eq!(
        engine.run(RunLimit::Unlimited).unwrap().halt_reason,
        HaltReason::AgendaEmpty
    );
    assert_eq!(engine.get_output("t"), Some("1804289383\n"));
}

#[test]
fn funcall_evaluates_operands_before_short_circuiting_and_target_arity_checks() {
    assert_eq!(
        output(
            r#"(defrule check =>
              (printout t (funcall eq a b (progn (printout t eager) c)) "|"
                (funcall and FALSE (progn (printout t eager) TRUE)) crlf))"#
        ),
        "eagerFALSE|eagerFALSE\n"
    );
    let mut engine = Engine::with_rules(
        "(defrule check =>
          (funcall abs (progn (printout t left) 1) (progn (printout t right) 2)))",
    )
    .unwrap();
    assert_eq!(
        engine.run(RunLimit::Unlimited).unwrap().halt_reason,
        HaltReason::ActionError
    );
    assert_eq!(engine.get_output("t"), Some("leftright"));
}

#[test]
fn funcall_random_invalid_count_evaluates_operands_then_recovers_with_a_draw() {
    let mut engine = Engine::with_rules(
        "(defrule check => (seed 1)
          (printout t (funcall random (progn (printout t eager) 1) 2 3) crlf
            (random) crlf))",
    )
    .unwrap();
    assert_eq!(
        engine.run(RunLimit::Unlimited).unwrap().halt_reason,
        HaltReason::AgendaEmpty
    );
    assert_eq!(engine.get_output("t"), Some("eager1804289383\n846930886\n"));
    assert_eq!(
        engine.get_output("werror"),
        Some("[MISCFUN2] Function random expected either 0 or 2 arguments\n")
    );
    assert!(engine.action_diagnostics().is_empty());
}

#[test]
fn math_errors_stop_the_run_before_later_actions_and_rules() {
    for (expression, diagnostic) in [
        ("(sqrt -1)", "EMATHFUN1"),
        ("(acos 2)", "EMATHFUN1"),
        ("(asin 2)", "EMATHFUN1"),
        ("(acosh 0)", "EMATHFUN1"),
        ("(atanh 1)", "EMATHFUN1"),
        ("(atanh -1)", "EMATHFUN1"),
        ("(log -1)", "EMATHFUN1"),
        ("(** -8 0.5)", "EMATHFUN1"),
        ("(** 0 0)", "EMATHFUN1"),
        ("(** 0 -1)", "EMATHFUN1"),
        ("(log 0)", "EMATHFUN2"),
        ("(log10 0)", "EMATHFUN2"),
        ("(tan 1.5707963267948966)", "EMATHFUN3"),
    ] {
        let mut engine = Engine::with_rules(&format!(
            "(defrule fail (declare (salience 10)) => (printout t prefix) {expression} (assert (after)))
             (defrule later => (assert (later)))"
        )).unwrap();
        assert_eq!(
            engine.run(RunLimit::Unlimited).unwrap().halt_reason,
            HaltReason::ActionError,
            "{expression}"
        );
        assert_eq!(engine.fact_count(), 0, "{expression}");
        assert_eq!(engine.get_output("t"), Some("prefix"));
        assert!(
            engine
                .action_diagnostics()
                .iter()
                .any(|error| error.to_string().contains(diagnostic)),
            "{expression}: {:?}",
            engine.action_diagnostics()
        );
    }
}

#[test]
fn valid_math_boundaries_and_clips_infinities_continue_normally() {
    assert_eq!(
        output(
            r#"(defrule check => (printout t
      (sqrt 0) "|" (acos 1) "|" (asin 0) "|" (acosh 1) "|" (atanh 0) "|"
      (log 1) "|" (log10 1) "|" (** -8 3) "|" (exp 1000) "|" (sinh 1000) "|" (cosh 1000) crlf))"#
        ),
        "0.0|0.0|0.0|0.0|0.0|0.0|0.0|-512.0|inf.0|inf.0|inf.0\n"
    );
}

#[test]
fn concatenation_rejects_multifields_and_void_without_evaluating_later_operands() {
    for function in ["str-cat", "sym-cat"] {
        for value in ["(create$ x y)", "(printout t first)"] {
            let mut engine = Engine::with_rules(&format!(
                "(defrule fail => ({function} {value} (printout t later)) (assert (after)))"
            ))
            .unwrap();
            assert_eq!(
                engine.run(RunLimit::Unlimited).unwrap().halt_reason,
                HaltReason::ActionError
            );
            assert_eq!(engine.fact_count(), 0);
            assert_eq!(
                engine.get_output("t").unwrap_or_default(),
                if value.starts_with("(printout") {
                    "first"
                } else {
                    ""
                }
            );
            assert!(engine
                .action_diagnostics()
                .iter()
                .any(|error| error.to_string().contains("type error")));
        }
    }
}

#[test]
fn invalid_builtin_calls_do_not_replace_a_rule_or_prevent_later_rules_loading() {
    for (call, code) in [
        ("(abs 1 2)", "ARGACCES4"),
        ("(str-length)", "ARGACCES4"),
        ("(eq a)", "ARGACCES4"),
        ("(min 1 a)", "ARGACCES5"),
        ("(mod 1 wrong)", "ARGACCES5"),
        ("(length$ 1)", "ARGACCES5"),
        ("(sqrt wrong)", "ARGACCES5"),
        ("(str-cat)", "ARGACCES4"),
    ] {
        let mut engine = Engine::with_rules("(defrule keep => (assert (kept)))").unwrap();
        let errors = engine
            .load_str(&format!(
                "(defrule keep => {call}) (defrule good => (assert (good)))"
            ))
            .unwrap_err();
        assert!(
            errors.iter().any(|error| error.to_string().contains(code)),
            "{call}: {errors:?}"
        );
        assert_eq!(
            engine.run(RunLimit::Unlimited).unwrap().rules_fired,
            2,
            "{call}"
        );
        assert_eq!(engine.find_facts("kept").unwrap().len(), 1);
        assert_eq!(engine.find_facts("good").unwrap().len(), 1);
        assert!(engine.action_diagnostics().is_empty());
    }
}

#[test]
fn checks_cover_nested_callable_default_global_and_lhs_contexts() {
    for source in [
        "(deffunction bad () (abs 1 2))",
        "(defmethod bad () (abs 1 2))",
        "(defglobal ?*bad* = (if FALSE then (abs 1 2) else 1))",
        "(deftemplate bad (slot x (default-dynamic (abs 1 2))))",
        "(deffacts bad (item (abs 1 2)))",
        "(defrule bad (test (eq a)) =>)",
        "(defrule bad (item ?x&:(abs 1 2)) =>)",
        "(defrule bad (item =(abs 1 2)) =>)",
    ] {
        let mut engine = Engine::new(EngineConfig::default());
        let errors = engine.load_str(source).unwrap_err();
        assert!(
            errors
                .iter()
                .any(|error| error.to_string().contains("ARGACCES4")),
            "{source}: {errors:?}"
        );
    }
}

#[test]
fn fact_and_slot_data_heads_are_not_builtin_calls() {
    assert_eq!(
        output(
            r"(deftemplate box (slot abs))
      (defrule check => (assert (abs a b c)) (assert (box (abs 7))) (printout t ok crlf))"
        ),
        "ok\n"
    );
}

#[test]
fn unknown_expression_types_are_checked_when_evaluated() {
    let mut engine = Engine::with_rules(
        "(deffunction operand () bad)
         (defrule fail => (min 1 (operand)) (assert (after)))",
    )
    .unwrap();
    assert_eq!(
        engine.run(RunLimit::Unlimited).unwrap().halt_reason,
        HaltReason::ActionError
    );
    assert_eq!(engine.fact_count(), 0);
}

#[test]
fn invalid_callable_replacement_retains_previous_body_and_later_definitions() {
    let mut engine = Engine::with_rules("(deffunction keep () 7)").unwrap();
    let errors = engine
        .load_str(
            r#"(deffunction keep () (abs 1 2))
           (deffunction good () 8)
           (defrule report => (printout t (keep) "|" (good) crlf))"#,
        )
        .unwrap_err();
    assert!(errors
        .iter()
        .any(|error| error.to_string().contains("ARGACCES4")));
    assert_eq!(engine.run(RunLimit::Unlimited).unwrap().rules_fired, 1);
    assert_eq!(engine.get_output("t"), Some("7|8\n"));
    assert!(engine.action_diagnostics().is_empty());
}
