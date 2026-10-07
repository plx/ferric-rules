//! Loop control stays local to its lexical loop and restores temporary bindings.

use ferric_rules_runtime::{Engine, EngineConfig, HaltReason, RunLimit};

fn run(source: &str, expected_halt: HaltReason) -> Engine {
    let mut engine = Engine::with_rules(source).unwrap_or_else(|error| {
        panic!("failed to load {source}: {error:?}");
    });
    let result = engine.run(RunLimit::Count(10)).unwrap();
    assert_eq!(result.rules_fired, 1, "{source}");
    assert_eq!(result.halt_reason, expected_halt, "{source}");
    if expected_halt == HaltReason::ActionError {
        assert_eq!(engine.action_diagnostics().len(), 1);
    } else {
        assert!(engine.action_diagnostics().is_empty());
    }
    engine.rete().validate_consistency().unwrap();
    engine
}

fn loop_forms(body: &str) -> Vec<String> {
    [
        format!("(while (< ?n 4) do {body})"),
        format!("(loop-for-count (?i 1 4) do {body})"),
        format!("(loop-for-count 4 do {body})"),
        format!("(loop-for-count ?end do {body})"),
        format!("(loop-for-count (+ 2 2) do {body})"),
        format!("(progn$ (?x (create$ a b c d)) {body})"),
        format!("(foreach ?x (create$ a b c d) {body})"),
    ]
    .into()
}

#[test]
fn action_loops_catch_break_inside_if_and_switch_and_continue_the_rule() {
    for loop_expression in loop_forms(
        r"(bind ?n (+ ?n 1))
           (printout t ?n)
           (if (= ?n 2) then (switch ?n (case 2 then (break))))",
    ) {
        let source = format!(
            r#"(defrule exercise =>
                 (bind ?n 0)
                 (bind ?end 4)
                 {loop_expression}
                 (printout t "|" ?n crlf))"#
        );
        let engine = run(&source, HaltReason::AgendaEmpty);
        assert_eq!(engine.get_output("t"), Some("12|2\n"), "{loop_expression}");
    }
}

#[test]
fn nested_break_exits_only_the_inner_loop_and_restores_shadowed_names() {
    let engine = run(
        r#"
        (defrule exercise =>
          (bind ?i outer-count)
          (bind ?x outer-value)
          (bind ?x-index 99)
          (loop-for-count (?i 1 2) do
            (foreach ?x (create$ a b c)
              (if (eq ?x b) then (break))
              (printout t ?i ?x ":" ?x-index " "))
            (printout t "after" ?i " "))
          (printout t ?i " " ?x " " ?x-index crlf))
        "#,
        HaltReason::AgendaEmpty,
    );
    assert_eq!(
        engine.get_output("t"),
        Some("1a:1 after1 2a:1 after2 outer-count outer-value 99\n")
    );
}

#[test]
fn each_query_action_catches_break_without_exiting_the_outer_loop() {
    for query in [
        "do-for-fact",
        "do-for-all-facts",
        "delayed-do-for-all-facts",
    ] {
        let source = format!(
            r#"
            (deftemplate item (slot x))
            (deffacts seed (item (x 1)) (item (x 2)) (item (x 3)))
            (defrule exercise =>
              (bind ?f outer)
              (loop-for-count (?i 1 2) do
                ({query} ((?f item) (?g item)) TRUE
                  (printout t ?i ":" ?f:x ?g:x)
                  (break)
                  (printout t "unreachable"))
                (printout t "|"))
              (printout t ?f crlf))
            "#
        );
        let engine = run(&source, HaltReason::AgendaEmpty);
        assert_eq!(engine.get_output("t"), Some("1:11|2:11|outer\n"), "{query}");
        assert_eq!(engine.fact_count(), 3);
    }
}

#[test]
fn query_break_restores_scope_after_retracting_the_current_fact() {
    for query in ["do-for-all-facts", "delayed-do-for-all-facts"] {
        let source = format!(
            r#"
            (deftemplate item (slot x))
            (deffacts seed (item (x 1)) (item (x 2)) (item (x 3)))
            (defrule exercise =>
              (bind ?f outer)
              ({query} ((?f item)) TRUE
                (retract ?f)
                (printout t ?f:x " ")
                (break))
              (printout t ?f " " (length$ (find-all-facts ((?f item)) TRUE)) crlf))
            "#
        );
        let engine = run(&source, HaltReason::AgendaEmpty);
        assert_eq!(engine.get_output("t"), Some("1 outer 2\n"), "{query}");
        assert_eq!(engine.fact_count(), 2);
    }
}

#[test]
fn callable_breaks_do_not_interrupt_the_callers_loop() {
    let engine = run(
        r#"
        (deffunction stop-inner ()
          (loop-for-count (?j 1 3) do
            (if (= ?j 2) then (break))
            (printout t ?j))
          done)
        (defmethod stop-method ((?x INTEGER))
          (while TRUE do (break))
          ?x)
        (defrule exercise =>
          (loop-for-count (?i 1 2) do
            (printout t (stop-inner) ":" (stop-method ?i) " "))
          (printout t "after" crlf))
        "#,
        HaltReason::AgendaEmpty,
    );
    assert_eq!(engine.get_output("t"), Some("1done:1 1done:2 after\n"));
}

#[test]
fn return_still_exits_the_rule_or_callable_instead_of_only_the_loop() {
    for loop_expression in loop_forms(r#"(printout t "before") (return)"#) {
        let source = format!(
            r#"(defrule exercise =>
                 (bind ?n 0) (bind ?end 4)
                 {loop_expression}
                 (printout t "unreachable"))"#
        );
        let engine = run(&source, HaltReason::AgendaEmpty);
        assert_eq!(engine.get_output("t"), Some("before"), "{loop_expression}");
    }
    let engine = run(
        r#"
        (deffunction early ()
          (while TRUE do (return 7))
          99)
        (defrule exercise => (printout t (early) " after" crlf))
        "#,
        HaltReason::AgendaEmpty,
    );
    assert_eq!(engine.get_output("t"), Some("7 after\n"));
}

#[test]
fn ordinary_loop_errors_still_stop_the_rule() {
    for loop_expression in loop_forms(r#"(printout t "before") (/ 1 0)"#) {
        let source = format!(
            r#"(defrule exercise =>
                 (bind ?n 0) (bind ?end 4)
                 {loop_expression}
                 (printout t "unreachable"))"#
        );
        let engine = run(&source, HaltReason::ActionError);
        assert_eq!(engine.get_output("t"), Some("before"), "{loop_expression}");
    }
    for query in [
        "do-for-fact",
        "do-for-all-facts",
        "delayed-do-for-all-facts",
    ] {
        let source = format!(
            r#"
            (deftemplate item (slot x))
            (deffacts seed (item (x 1)))
            (defrule exercise =>
              ({query} ((?f item)) TRUE (printout t "before") (/ 1 0))
              (printout t "unreachable"))
            "#
        );
        let engine = run(&source, HaltReason::ActionError);
        assert_eq!(engine.get_output("t"), Some("before"), "{query}");
    }
}

#[test]
fn break_is_rejected_outside_lexical_loop_action_positions() {
    for body in [
        "(break)",
        "(if FALSE then (break))",
        "(loop-for-count (?i 1 2) do (printout t (break)))",
        "(loop-for-count (?i 1 2) do (bind ?x (break)))",
        "(loop-for-count (?i 1 2) do (if (break) then TRUE))",
        "(loop-for-count (?i 1 2) do (switch (break) (case a then TRUE)))",
        "(loop-for-count (?i 1 2) do (switch a (case (break) then TRUE)))",
        "(loop-for-count (?i 1 2) do (while (break) do TRUE))",
        "(loop-for-count (?i 1 2) do (loop-for-count (?j (break)) do TRUE))",
        "(loop-for-count (?i 1 2) do (progn$ (?x (break)) TRUE))",
        "(loop-for-count (?i 1 2) do (do-for-all-facts ((?f item)) (break) TRUE))",
    ] {
        for definition in [
            format!("(defrule rejected => {body})"),
            format!("(deffunction rejected () {body})"),
            format!("(defmethod rejected ((?x INTEGER)) {body})"),
        ] {
            let mut engine = Engine::new(EngineConfig::default());
            let errors = engine
                .load_str(&format!("(deftemplate item (slot x)) {definition}"))
                .expect_err(&definition);
            assert!(
                errors
                    .iter()
                    .any(|error| error.to_string().contains("[PRCDRPSR2]")),
                "{definition}: {errors:?}"
            );
        }
    }
}

#[test]
fn nested_loops_can_establish_break_scope_inside_expression_operands() {
    let engine = run(
        r#"
        (deffunction stopped () (while TRUE do (break)))
        (defrule exercise =>
          (printout t (loop-for-count (?i 1 2) do (break)) " ")
          (if (while TRUE do (break)) then
            (printout t "unreachable")
           else
            (printout t (stopped) " after" crlf)))
        "#,
        HaltReason::AgendaEmpty,
    );
    assert_eq!(engine.get_output("t"), Some("FALSE FALSE after\n"));
}

const BREAK_DATA_SOURCE: &str = r#"
    (deftemplate item (slot break))
    (deffacts seed (item (break 1)))
    (deffunction data () (assert (break 1) (item (break 2))))
    (defmethod data-method ((?x INTEGER)) (assert (item (break ?x))))
    (defrule exercise ?f <- (item (break 1)) =>
      (assert (break 1) (item (break 2)))
      (duplicate ?f (break 4))
      (modify ?f (break 3))
      (printout t
        (any-factp ((?f item)) (= ?f:break 2)) " "
        (any-factp ((?f item)) (= ?f:break 3)) " "
        (any-factp ((?f item)) (= ?f:break 4)) crlf))
"#;

#[test]
fn break_named_facts_and_slots_are_data_in_assert_modify_and_duplicate() {
    let engine = run(BREAK_DATA_SOURCE, HaltReason::AgendaEmpty);
    assert_eq!(engine.get_output("t"), Some("TRUE TRUE TRUE\n"));
    assert_eq!(engine.fact_count(), 4);
    let ordered = engine.find_facts("break").unwrap();
    assert_eq!(ordered.len(), 1);
    let ferric_rules_core::Fact::Ordered(fact) = ordered[0].1 else {
        panic!("expected ordered fact named break");
    };
    assert!(matches!(
        fact.fields.as_slice(),
        [ferric_rules_runtime::Value::Integer(1)]
    ));
}

#[test]
fn actual_break_calls_inside_fact_and_slot_values_are_rejected() {
    for action in [
        "(assert (ordered (break)))",
        "(assert (item (break (break))))",
        "(modify ?f (break (break)))",
        "(duplicate ?f (break (break)))",
        "(modify (break) (break 1))",
    ] {
        let mut engine = Engine::new(EngineConfig::default());
        let source = format!(
            "(deftemplate item (slot break))
             (defrule rejected ?f <- (item) => {action})"
        );
        let errors = engine.load_str(&source).expect_err(action);
        assert!(
            errors
                .iter()
                .any(|error| error.to_string().contains("[PRCDRPSR2]")),
            "{action}: {errors:?}"
        );
    }
}

#[cfg(feature = "serde")]
#[test]
fn snapshot_validation_distinguishes_break_control_from_fact_data() {
    use ferric_rules_runtime::SerializationFormat;

    let original = Engine::with_rules(BREAK_DATA_SOURCE).unwrap();
    for &format in SerializationFormat::ALL {
        let bytes = original.serialize(format).unwrap();
        let mut engine = Engine::deserialize(&bytes, format).unwrap();
        let result = engine.run(RunLimit::Count(10)).unwrap();
        assert_eq!(result.halt_reason, HaltReason::AgendaEmpty);
        assert_eq!(result.rules_fired, 1);
        assert_eq!(engine.get_output("t"), Some("TRUE TRUE TRUE\n"));
        assert_eq!(engine.fact_count(), 4);
    }
}

#[cfg(feature = "serde")]
#[test]
fn snapshot_roundtrips_keep_rule_and_callable_break_scopes() {
    use ferric_rules_runtime::SerializationFormat;

    let original = Engine::with_rules(
        r#"
        (deffunction first () (foreach ?x (create$ a b) (break)) done)
        (defrule exercise =>
          (loop-for-count (?i 1 2) do (printout t (first)) (break))
          (printout t " after" crlf))
        "#,
    )
    .unwrap();
    for &format in SerializationFormat::ALL {
        let bytes = original.serialize(format).unwrap();
        let mut engine = Engine::deserialize(&bytes, format).unwrap();
        let result = engine.run(RunLimit::Count(10)).unwrap();
        assert_eq!(result.halt_reason, HaltReason::AgendaEmpty);
        assert_eq!(result.rules_fired, 1);
        assert!(engine.action_diagnostics().is_empty());
        assert_eq!(engine.get_output("t"), Some("done after\n"));
    }
}

#[test]
fn break_in_lhs_expressions_is_rejected_at_load() {
    for (name, pattern) in [
        ("test-ce", "(test (break))"),
        ("predicate", "(a ?x&:(break))"),
        ("return-value", "(a ?x) (b =(break))"),
        ("negated-test", "(not (test (break)))"),
        ("nested-predicate", "(not (a ?x&:(> (break) 0)))"),
    ] {
        let mut engine = Engine::new(EngineConfig::default());
        let source = format!(
            "(defrule {name} {pattern} => (printout t hi crlf))\n(defrule kept => (printout t ok crlf))"
        );
        let errors = engine.load_str(&source).expect_err(&source);
        assert!(
            errors
                .iter()
                .any(|error| error.to_string().contains("[PRCDRPSR2]")),
            "{source}: {errors:?}"
        );
        let rules: Vec<_> = engine.rules().into_iter().map(|(rule, _)| rule).collect();
        assert_eq!(rules, ["kept"], "{source}");
    }
}
