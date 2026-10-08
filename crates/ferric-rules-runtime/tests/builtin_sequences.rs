//! CLIPS argument expansion, sequence member operations, and progn control flow.
use ferric_rules_runtime::{Engine, EngineConfig, HaltReason, RunLimit};

fn run(source: &str) -> Engine {
    let mut engine = Engine::new(EngineConfig::utf8());
    engine.load_str(source).unwrap();
    engine.reset().unwrap();
    engine.run(RunLimit::Count(50)).unwrap();
    engine
}

fn output(source: &str) -> String {
    let engine = run(source);
    assert!(
        engine.action_diagnostics().is_empty(),
        "{:?}",
        engine.action_diagnostics()
    );
    engine.get_output("t").unwrap_or("").to_owned()
}

#[test]
fn expansion_runs_once_before_ordinary_arguments_and_preserves_lazy_operands() {
    assert_eq!(
        output(
            r"
      (deffunction mark (?tag ?value) (printout t ?tag) ?value)
      (defrule run =>
       (printout t (+ (mark A 1) (expand$ (mark B (create$ 2 3)))
         (mark C 4) (expand$ (mark D (create$ 5)))) crlf)
       (printout t (and FALSE (mark skipped TRUE) (expand$ (mark E (create$ TRUE)))) crlf))
    "
        ),
        "BDAC15\nEFALSE\n"
    );
}

#[test]
fn expanded_arguments_reach_fixed_variadic_generic_qualified_and_funcall_targets() {
    assert_eq!(
        output(
            r#"
      (defmodule A (export deffunction pair))
      (deffunction pair (?a ?b) (str-cat ?a ":" ?b))
      (defmodule MAIN (import A deffunction pair))
      (deffunction tail (?head $?rest) (create$ (length$ ?head) (length$ ?rest)))
      (defgeneric total)
      (defmethod total ((?a INTEGER) (?b INTEGER)) (+ ?a ?b))
      (defrule run =>
       (printout t (A::pair (expand$ (create$ a b))) ":"
         (funcall pair (expand$ (create$ c d))) ":"
         (total (expand$ (create$ 1 2))) ":"
         (funcall + (expand$ (create$ 3 4))) ":"
         (tail (create$ a b) (expand$ (create$ 1 2))) crlf))
    "#
        ),
        "a:b:c:d:3:7:(2 2)\n"
    );
}

#[test]
fn rhs_printout_and_bind_expand_and_nil_router_still_runs_expansions() {
    assert_eq!(
        output(
            r#"
      (deffunction mark () (printout t "expanded;") (create$ a b))
      (defrule run =>
       (printout nil (expand$ (mark)))
       (printout t (expand$ (create$ x y)) crlf)
       (bind ?values (expand$ (create$ a b)))
       (printout t ?values crlf))
    "#
        ),
        "expanded;xy\n(a b)\n"
    );
}

#[test]
fn println_prints_expanded_fields_without_a_literal_form() {
    assert_eq!(
        output(
            r#"
      (deffacts seed (a))
      (deffunction mark (?tag ?value) (printout t ?tag ";") ?value)
      (defrule run ?f <- (a) =>
       (println "g=" (expand$ (create$ ?f 1 x)))
       (println (mark A "a") (expand$ (mark B (create$ b))) (mark C "c"))
       (println "done"))
    "#
        ),
        "g=<Fact-1>1x\nA;aB;bC;c\ndone\n"
    );
}

#[test]
fn println_expansion_of_a_non_multifield_is_a_type_error() {
    let engine = run(r#"(defrule run => (println "before" (expand$ (+ 1 2)) "after"))"#);
    assert!(
        engine
            .action_diagnostics()
            .iter()
            .any(|diagnostic| diagnostic.to_string().contains("expand$")),
        "{:?}",
        engine.action_diagnostics()
    );
    assert_eq!(engine.get_output("t"), Some("before"));
}

#[test]
fn progn_keeps_bindings_and_propagates_return_and_break_to_their_owners() {
    assert_eq!(
        output(
            r#"
      (deffunction value () (progn (bind ?x 7) (return ?x) 99) 100)
      (defrule run =>
       (printout t (progn) ":" (progn 1 2 3) ":" (value) crlf)
       (loop-for-count (?i 1 3) do
         (progn (printout t ?i) (break) (printout t "bad")))
       (progn (bind ?x done) (printout t ?x crlf) (return 7) (printout t "bad"))
       (printout t "bad"))
    "#
        ),
        "FALSE:3:7\n1done\n"
    );
}

#[test]
fn member_search_uses_leftmost_match_then_pattern_order_and_distinct_boundaries() {
    assert_eq!(
        output(
            r#"
      (defrule run =>
       (printout t (delete-member$ (create$ a b a c b) a b) ":"
         (delete-member$ (create$ a b c) b (create$ a c)) ":"
         (replace-member$ (create$ a b c a b) X b (create$ a b)) ":"
         (replace-member$ (create$ a b c) X a (create$ a b)) ":"
         (replace-member$ (create$ a b c) X (create$ a b) a) ":"
         (replace-member$ (create$ a b c) (create$) b (create$ a c)) ":"
         (replace-member$ (create$ a a) (create$ a b) a) crlf))
    "#
        ),
        "(c):():(X c X):(X b c):(X c):(a c):(a b a b)\n"
    );
}

#[test]
fn member_equality_preserves_numeric_kinds_signed_zero_and_lexeme_types() {
    assert_eq!(
        output(
            r#"
      (defrule run =>
       (printout t (delete-member$ (create$ 1 1.0 0.0 -0.0 a "a") 1 0.0 a) ":"
         (delete-member$ (create$) (create$)) ":"
         (replace-member$ (create$) X (create$)) crlf))
    "#
        ),
        "(1.0 -0.0 \"a\"):():()\n"
    );
}

#[test]
fn dynamic_expansion_arity_type_and_empty_search_errors_stop_actions() {
    for expression in [
        "(+ (expand$ (create$)))",
        "(abs (expand$ (create$ 1 2)))",
        "(+ 1 (expand$ (bad)))",
        "(delete-member$ (create$ a b) (create$))",
        "(replace-member$ (create$ a b) X (create$))",
    ] {
        let source = format!(
            "(deffunction bad () 3) (defrule run => (printout t prefix {expression} after))"
        );
        let mut engine = Engine::with_rules(&source).unwrap();
        engine.reset().unwrap();
        assert_eq!(
            engine.run(RunLimit::Count(5)).unwrap().halt_reason,
            HaltReason::ActionError,
            "{expression}"
        );
        assert!(!engine.action_diagnostics().is_empty());
        assert_eq!(engine.get_output("t"), Some("prefix"), "{expression}");
    }
}

#[test]
fn invalid_expansion_placement_and_progn_operand_return_are_load_errors() {
    for source in [
        "(defrule r => (expand$ (create$ a b)))",
        "(defrule r => (progn (expand$ (create$ a b))))",
        "(deffunction f () (return (expand$ (create$ 7))))",
        "(defrule r => (if TRUE then (expand$ (create$ a b))))",
        "(defrule r => (assert (p (expand$ (create$ a b)))))",
        "(defrule r => (+ (expand$ (expand$ (create$ 1 2)))))",
        "(defrule r => (printout t (progn (return 7))))",
    ] {
        let mut engine = Engine::new(EngineConfig::utf8());
        assert!(engine.load_str(source).is_err(), "{source}");
    }
}
