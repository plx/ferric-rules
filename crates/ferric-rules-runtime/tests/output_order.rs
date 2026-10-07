//! Output effects remain ordered across action and callable evaluation frames.

use ferric_rules_runtime::{Engine, EngineConfig, HaltReason, RunLimit, Value};

fn run(source: &str, expected_halt: HaltReason) -> Engine {
    let mut engine = Engine::with_rules_config(source, EngineConfig::utf8()).unwrap();
    let result = engine.run(RunLimit::Count(10)).unwrap();
    assert_eq!(result.rules_fired, 1);
    assert_eq!(result.halt_reason, expected_halt);
    if expected_halt == HaltReason::ActionError {
        assert_eq!(engine.action_diagnostics().len(), 1);
    } else {
        assert!(engine.action_diagnostics().is_empty());
    }
    engine
}

#[test]
fn format_routes_the_returned_string_without_crossing_channels() {
    let engine = run(
        r#"
        (defrule exercise =>
          (bind ?text (format stdout "n=%d%n" 42))
          (printout t "[" ?text "]" crlf)
          (format wwarning "warning=%s%n" expected)
          (format werror "error=%d%n" 7)
          (format wdisplay "display%n"))
        "#,
        HaltReason::AgendaEmpty,
    );
    assert_eq!(engine.get_output("t"), Some("[n=42\n]\n"));
    assert_eq!(engine.get_output("stdout"), Some("n=42\n"));
    assert_eq!(engine.get_output("wwarning"), Some("warning=expected\n"));
    assert_eq!(engine.get_output("werror"), Some("error=7\n"));
    assert_eq!(engine.get_output("wdisplay"), Some("display\n"));
}

#[test]
fn nested_output_stays_in_order_through_functions_methods_and_println() {
    let engine = run(
        r#"
        (deffunction leaf () (printout t "L") "v")
        (deffunction middle () (printout t "M[" (leaf) "]"))
        (defmethod method ((?x INTEGER)) (printout t "D[" (leaf) "]") ?x)
        (defrule exercise =>
          (printout t "R[" (middle) "]|" (method 7) crlf)
          (println "P[" (leaf) "]")
          (printout t "a " (format t "b%n") "c" crlf))
        "#,
        HaltReason::AgendaEmpty,
    );
    assert_eq!(
        engine.get_output("t"),
        Some("R[M[Lv]]|D[Lv]7\nP[Lv]\na b\nb\nc\n")
    );
}

#[test]
fn printout_and_println_preserve_completed_arguments_before_an_error() {
    for action in [
        r#"(printout t "prefix " (fail ?bad) "unreachable" crlf)"#,
        r#"(println "prefix " (fail ?bad) "unreachable")"#,
        "(outer ?bad)",
    ] {
        let engine = run(
            &format!(
                r#"
                (deffacts seed (bad abc))
                (deffunction fail (?x) (printout t "inner ") (+ 1 ?x))
                (deffunction outer (?x)
                  (printout t "prefix " (fail ?x) "unreachable" crlf))
                (defrule exercise (bad ?bad) => {action}
                  (printout t "unreachable-action" crlf))
                "#
            ),
            HaltReason::ActionError,
        );
        assert_eq!(engine.get_output("t"), Some("prefix inner "), "{action}");
    }
}

#[test]
fn a_failed_format_keeps_nested_effects_but_emits_no_partial_result() {
    let engine = run(
        r#"
        (deffunction emit (?x) (printout t "<" ?x ">") ?x)
        (defrule exercise =>
          (printout t "prefix "
            (format t "unwritten-%d-%s" (emit 7) (emit 9))
            "unreachable" crlf))
        "#,
        HaltReason::ActionError,
    );
    assert_eq!(engine.get_output("t"), Some("prefix <7><9>"));
}

#[test]
fn format_validates_all_directives_and_arity_before_evaluating_operands() {
    for format_call in [
        r#"(format t "unwritten-%d-%d" (emit 7))"#,
        r#"(format t "unwritten-%q" (emit 7))"#,
    ] {
        let engine = run(
            &format!(
                r#"
                (deffunction emit (?x) (printout t "unreachable") ?x)
                (defrule exercise => (printout t "prefix " {format_call}))
                "#
            ),
            HaltReason::ActionError,
        );
        assert_eq!(engine.get_output("t"), Some("prefix "), "{format_call}");
    }
}

#[test]
fn nil_format_evaluates_once_and_returns_text_without_writing_a_nil_channel() {
    let engine = run(
        r#"
        (defglobal ?*calls* = 0)
        (deffunction mark (?value)
          (bind ?*calls* (+ ?*calls* 1))
          (printout t "M")
          ?value)
        (defrule exercise =>
          (printout t "[" (format nil "%d" (mark 3)) "]" crlf)
          (printout t "[" (format "nil" "%d" (mark 4)) "]" crlf)
          (printout t "[" (format [nil] "%d" (mark 5)) "]" crlf))
        "#,
        HaltReason::AgendaEmpty,
    );
    assert_eq!(engine.get_output("t"), Some("[M3]\n[M4]\n[M5]\n"));
    assert!(matches!(
        engine.get_global("calls"),
        Some(Value::Integer(3))
    ));
    assert!(engine.get_output("nil").is_none());
}

#[test]
fn nil_printout_skips_operand_evaluation_in_actions_and_callables() {
    let engine = run(
        r#"
        (defglobal ?*calls* = 0)
        (deffunction mark ()
          (bind ?*calls* (+ ?*calls* 1))
          (printout t "unreachable")
          (/ 1 0))
        (deffunction quiet (?channel) (printout ?channel (mark)))
        (defrule exercise =>
          (printout nil (mark))
          (printout "nil" (mark))
          (printout [nil] (mark))
          (quiet nil)
          (quiet "nil")
          (quiet [nil])
          (printout t "done" crlf))
        "#,
        HaltReason::AgendaEmpty,
    );
    assert_eq!(engine.get_output("t"), Some("done\n"));
    assert!(matches!(
        engine.get_global("calls"),
        Some(Value::Integer(0))
    ));
    assert!(engine.get_output("nil").is_none());
}

#[test]
fn queued_output_in_nested_bodies_precedes_later_direct_writes() {
    for (source, expected) in [
        (
            r#"(defrule exercise => (if TRUE then (format t "a") (println)))"#,
            "a\n",
        ),
        (
            r#"(defrule exercise =>
                 (loop-for-count (?i 2) (format t "%d" ?i) (println)))"#,
            "1\n2\n",
        ),
        (
            r#"(deffunction g () (format t "cond%n") TRUE)
               (defrule exercise => (if (g) then (println)))"#,
            "cond\n\n",
        ),
        (
            r#"(deffunction f () (printout t "in-f" crlf))
               (defrule exercise => (if TRUE then (f) (list-focus-stack)))"#,
            "in-f\nMAIN\n",
        ),
    ] {
        let engine = run(source, HaltReason::AgendaEmpty);
        assert_eq!(engine.get_output("t"), Some(expected), "{source}");
    }
}
