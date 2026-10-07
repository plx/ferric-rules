//! Engine effects share their values, ordering, and lifecycle across expression roots.

use ferric_rules_runtime::{Engine, EngineConfig, HaltReason, RunLimit};

fn run(source: &str) -> Engine {
    let mut engine = Engine::new(EngineConfig::utf8());
    engine.load_str(source).unwrap();
    engine.reset().unwrap();
    let result = engine.run(RunLimit::Count(30)).unwrap();
    assert_eq!(
        result.halt_reason,
        HaltReason::AgendaEmpty,
        "{:?}",
        engine.action_diagnostics()
    );
    assert!(
        engine.action_diagnostics().is_empty(),
        "{:?}",
        engine.action_diagnostics()
    );
    engine
}

#[test]
fn expression_assert_modify_and_duplicate_return_live_addresses() {
    let engine = run(r#"
      (deftemplate p (slot n) (slot v))
      (defrule probe =>
        (bind ?f (assert (p (n a) (v 1))))
        (if (assert (p (n b) (v 2))) then (printout t "if ok" crlf))
        (bind ?g (modify ?f (v 10)))
        (bind ?h (duplicate ?g (n c)))
        (printout t (fact-index ?f) " " (fact-index ?g) " " (fact-index ?h) crlf)
        (do-for-all-facts ((?p p)) TRUE (printout t ?p:n ":" ?p:v "|")))
    "#);
    assert_eq!(
        engine.get_output("t"),
        Some("if ok\n-1 3 4\nb:2|a:10|c:10|")
    );
    assert_eq!(engine.fact_count(), 3);
}

#[test]
fn callable_functions_methods_and_query_bodies_share_effects() {
    let engine = run(r#"
      (deftemplate p (slot v))
      (deffunction mk (?v) (assert (p (v ?v))))
      (defgeneric mkg)
      (defmethod mkg ((?v INTEGER)) (assert (p (v ?v))))
      (deffunction kill () (do-for-all-facts ((?f p)) TRUE (retract ?f)))
      (defrule probe =>
        (printout t "mk " (fact-slot-value (mk 5) v) crlf)
        (mkg 6)
        (kill)
        (printout t "done" crlf))
    "#);
    assert_eq!(engine.get_output("t"), Some("mk 5\ndone\n"));
    assert_eq!(engine.fact_count(), 0);
}

#[test]
fn duplicate_suppression_and_missing_mutation_targets_return_false() {
    let engine = run(r#"
      (deftemplate p (slot v))
      (defrule probe =>
        (bind ?a (assert (p (v 1))))
        (bind ?b (assert (p (v 2))))
        (printout t (assert (p (v 1))) " " (duplicate ?a) " ")
        (printout t (modify ?b (v 1)) " " (fact-existp ?b) crlf)
        (printout t (modify 99 (v 3)) " " (duplicate 98 (v 4)) " continued" crlf)
        (bind ?c (modify ?a (v 1)))
        (printout t (fact-index ?a) " " (fact-index ?c) crlf))
    "#);
    assert_eq!(
        engine.get_output("t"),
        Some("FALSE FALSE FALSE FALSE\nFALSE FALSE continued\n-1 3\n")
    );
    assert_eq!(engine.fact_count(), 1);
}

#[test]
fn negative_and_stale_mutation_targets_stop_expression_evaluation() {
    // CLIPS 6.30 halts the rule for a negative index after earlier operands
    // have printed. Ferric also stops for a stale source address, where CLIPS
    // copies the retracted fact's data (a documented boundary).
    for (target, expected) in [
        ("(bind ?t -1)", "FALSE "),
        ("(bind ?t (assert (p (v 9)))) (retract ?t)", "FALSE "),
    ] {
        let mut engine = Engine::new(EngineConfig::utf8());
        engine
            .load_str(&format!(
                "(deftemplate p (slot v))
                 (defrule probe =>
                   {target}
                   (bind ?m 99)
                   (printout t (modify ?m (v 3)) \" \" (duplicate ?t (v 4)) \" continued\" crlf)
                   (printout t unexpected crlf))"
            ))
            .unwrap();
        engine.reset().unwrap();
        assert_eq!(
            engine.run(RunLimit::Count(10)).unwrap().halt_reason,
            HaltReason::ActionError,
            "{target}"
        );
        assert_eq!(engine.get_output("t"), Some(expected), "{target}");
        assert_eq!(engine.fact_count(), 0, "{target}");
    }
}

#[test]
fn nested_reset_preserves_callable_locals_output_and_remaining_rhs() {
    let mut engine = Engine::with_rules(
        r#"
      (deftemplate item (slot value))
      (deffacts seed (item (value 10)))
      (deffunction restart (?value)
        (bind ?local (+ ?value 1))
        (printout t callable-before "|")
        (reset)
        (printout t callable-after ":" ?value ":" ?local "|")
        ?local)
      (defrule probe =>
        (bind ?outer 40)
        (printout t before "|" (restart 7) "|" ?outer "|")
        (printout t (fact-slot-value 1 value) crlf)
        (halt)
        (printout t after-halt crlf))
    "#,
    )
    .unwrap();
    let result = engine.run(RunLimit::Count(5)).unwrap();
    assert_eq!(result.halt_reason, HaltReason::HaltRequested);
    assert_eq!(result.rules_fired, 1);
    assert!(engine.action_diagnostics().is_empty());
    assert_eq!(
        engine.get_output("t"),
        Some("before|callable-before|callable-after:7:8|8|40|10\nafter-halt\n")
    );
}

#[test]
fn delayed_query_retains_original_members_across_one_reset_in_each_context() {
    for callable in [false, true] {
        let body = r#"
          (bind ?changed FALSE)
          (delayed-do-for-all-facts ((?f item)) TRUE
            (if (not ?changed) then (reset) (bind ?changed TRUE))
            (printout t ?f:value ":" (fact-index ?f) "|"))
          (printout t after crlf)
          (halt)
        "#;
        let probe = if callable {
            format!("(deffunction probe () {body}) (defrule driver => (probe))")
        } else {
            format!("(defrule driver => {body})")
        };
        let mut engine = Engine::with_rules(&format!(
            "(deftemplate item (slot value))
             (deffacts seed (item (value 10)) (item (value 20))) {probe}"
        ))
        .unwrap();
        let result = engine.run(RunLimit::Count(5)).unwrap();
        assert_eq!(
            result.halt_reason,
            HaltReason::HaltRequested,
            "callable={callable}"
        );
        assert!(
            engine.action_diagnostics().is_empty(),
            "{:?}",
            engine.action_diagnostics()
        );
        assert_eq!(
            engine.get_output("t"),
            Some("10:-1|20:-1|after\n"),
            "callable={callable}"
        );
        assert_eq!(engine.fact_count(), 2);
    }
}

#[test]
fn refused_clear_restarts_numbering_and_preserves_pending_unconditional_rules() {
    let mut engine = run(r#"
      (deffacts seed (old))
      (deffunction empty-facts () (clear))
      (defrule high (declare (salience 10)) =>
        (printout t before "|")
        (empty-facts)
        (printout t (assert (fresh)) "|"))
      (defrule low => (printout t (assert (later)) crlf))
    "#);
    assert_eq!(engine.get_output("t"), Some("before|<Fact-0>|<Fact-1>\n"));
    assert_eq!(engine.fact_count(), 2);
    assert_eq!(engine.rules().len(), 2);
    assert_eq!(engine.run(RunLimit::Count(5)).unwrap().rules_fired, 0);
}

#[test]
fn action_query_expressions_return_last_value_empty_false_and_break_void() {
    let engine = run(r#"
      (deftemplate item (slot value))
      (deffacts seed (item (value 10)) (item (value 20)))
      (defrule probe =>
        (printout t (do-for-fact ((?f item)) TRUE ?f:value) "|")
        (printout t (do-for-all-facts ((?f item)) TRUE ?f:value) "|")
        (printout t (delayed-do-for-all-facts ((?f item)) TRUE ?f:value) "|")
        (printout t (do-for-all-facts ((?f item)) FALSE ?f:value) "|")
        (printout t (do-for-all-facts ((?f item)) TRUE (break)) crlf))
    "#);
    assert_eq!(engine.get_output("t"), Some("10|20|20|FALSE|\n"));
}

#[test]
fn nested_effect_errors_keep_completed_arguments_and_stop_later_arguments() {
    let mut engine = Engine::with_rules(
        r#"
      (defrule probe =>
        (printout t prefix "|" (assert (kept 1) (bad (/ 1 0)) (later 3)))
        (assert (after)))
    "#,
    )
    .unwrap();
    let result = engine.run(RunLimit::Count(5)).unwrap();
    assert_eq!(result.halt_reason, HaltReason::ActionError);
    assert_eq!(engine.get_output("t"), Some("prefix|"));
    assert_eq!(engine.find_facts("kept").unwrap().len(), 1);
    for name in ["bad", "later", "after"] {
        assert!(engine.find_facts(name).unwrap().is_empty());
    }
}

#[test]
fn nested_rhs_local_assignments_survive_effect_evaluation_and_query_scopes() {
    let engine = run(r"
      (deftemplate item (slot value))
      (deffacts seed (item (value 10)))
      (defrule probe =>
        (bind ?local 1)
        (assert (first (bind ?local (+ ?local 1))))
        (do-for-all-facts ((?f item)) TRUE
          (assert (second (bind ?local (+ ?local ?f:value)))))
        (printout t ?local crlf))
    ");
    assert_eq!(engine.get_output("t"), Some("12\n"));
    assert_eq!(engine.find_facts("first").unwrap().len(), 1);
    assert_eq!(engine.find_facts("second").unwrap().len(), 1);
}

#[test]
fn reset_inside_loops_continues_the_current_lexical_iterator() {
    let mut engine = Engine::with_rules(
        r#"
      (defrule probe =>
        (bind ?local 10)
        (loop-for-count (?i 1 2) do
          (reset)
          (bind ?local (+ ?local ?i))
          (printout t ?i ":" ?local "|"))
        (printout t done crlf)
        (halt))
    "#,
    )
    .unwrap();
    let result = engine.run(RunLimit::Count(5)).unwrap();
    assert_eq!(result.halt_reason, HaltReason::HaltRequested);
    assert_eq!(result.rules_fired, 1);
    assert!(engine.action_diagnostics().is_empty());
    assert_eq!(engine.get_output("t"), Some("1:11|2:13|done\n"));
}

#[test]
fn resets_do_not_restart_the_shared_loop_budget() {
    let mut config = EngineConfig::utf8();
    config.max_action_loop_iterations = 4;
    let mut engine = Engine::new(config);
    engine
        .load_str("(defrule probe => (loop-for-count (?i 1 10) do (reset)))")
        .unwrap();
    engine.reset().unwrap();
    assert_eq!(
        engine.run(RunLimit::Count(1)).unwrap().halt_reason,
        HaltReason::ActionError
    );
    assert!(engine
        .action_diagnostics()
        .iter()
        .any(|error| error.to_string().contains("iteration")));
}

#[test]
fn matching_predicates_cannot_mutate_the_engine_through_callables() {
    let mut engine = Engine::with_rules(
        r"
      (deffunction touch () (assert (leaked)) TRUE)
      (defrule unsafe-predicate (test (touch)) => (assert (fired)))
    ",
    )
    .unwrap();
    let result = engine.run(RunLimit::Count(5)).unwrap();
    assert_eq!(result.rules_fired, 0);
    assert!(engine.find_facts("leaked").unwrap().is_empty());
    assert!(engine.find_facts("fired").unwrap().is_empty());
}

#[test]
fn nested_rhs_unbind_does_not_restore_a_stale_local_overlay() {
    let mut engine = Engine::with_rules(
        r"
      (defrule probe =>
        (bind ?local 9)
        (assert (kept (bind ?local)) (missing ?local))
        (assert (after)))
    ",
    )
    .unwrap();
    let result = engine.run(RunLimit::Count(5)).unwrap();
    assert_eq!(result.halt_reason, HaltReason::ActionError);
    assert_eq!(engine.find_facts("kept").unwrap().len(), 1);
    assert!(engine.find_facts("missing").unwrap().is_empty());
    assert!(engine.find_facts("after").unwrap().is_empty());
    assert!(engine
        .action_diagnostics()
        .iter()
        .any(|error| error.to_string().contains("local")));
}

#[test]
fn dormant_effect_initializers_retain_their_nested_template_dependencies() {
    let mut engine = Engine::new(EngineConfig::utf8());
    engine
        .load_str(
            r"
      (deftemplate record (slot original))
      (deftemplate envelope (slot address (type FACT-ADDRESS)))
      (deffacts seed (envelope (address (assert (record (original 8))))))
    ",
        )
        .unwrap();
    let errors = engine
        .load_str("(deftemplate record (slot changed))")
        .unwrap_err();
    assert!(
        errors
            .iter()
            .any(|error| error.to_string().contains("CSTRCPSR4")),
        "{errors:?}"
    );
    assert_eq!(engine.template_slot_names("record"), Some(vec!["original"]));
    engine.reset().unwrap();
    engine
        .load_str("(defrule inspect (record (original ?v)) => (printout t ?v crlf))")
        .unwrap();
    assert_eq!(engine.run(RunLimit::Count(5)).unwrap().rules_fired, 1);
    assert_eq!(engine.get_output("t"), Some("8\n"));
    assert_eq!(engine.fact_count(), 2);
}

#[test]
fn effect_fact_and_slot_heads_are_data_inside_callable_iterators() {
    let engine = run(r#"
      (deftemplate item (slot bind) (slot break))
      (deffunction create-items ()
        (loop-for-count (?i 1 2) do
          (assert (bind ?i))
          (bind ?f (assert (item (bind ?i) (break 10))))
          (bind ?f (modify ?f (bind ?i) (break 20)))
          (duplicate ?f (bind ?i) (break 30))))
      (defgeneric create-more)
      (defmethod create-more ()
        (foreach ?i (create$ 3)
          (assert (item (bind ?i) (break 40)))))
      (defrule probe =>
        (create-items)
        (create-more)
        (do-for-all-facts ((?f item)) TRUE (printout t ?f:bind ":" ?f:break "|")))
    "#);
    assert_eq!(engine.get_output("t"), Some("1:20|1:30|2:20|2:30|3:40|"));
    assert_eq!(engine.find_facts("bind").unwrap().len(), 2);
}

#[test]
fn genuine_iterator_rebinding_inside_effect_fields_remains_rejected() {
    for field in ["(bind ?i 3)", "(break)"] {
        let mut engine = Engine::new(EngineConfig::utf8());
        let source = format!(
            "(deftemplate item (slot bind) (slot break))
             (deffunction invalid ()
               (loop-for-count (?i 1 2) do
                 (assert (item (bind {field}) (break 4)))))"
        );
        let errors = engine.load_str(&source).unwrap_err();
        assert!(
            errors
                .iter()
                .any(|error| error.to_string().contains("PRCDRPSR")),
            "{source}: {errors:?}"
        );
    }
}

#[test]
fn dormant_nested_assertions_allow_control_word_slot_names() {
    let mut engine = Engine::new(EngineConfig::utf8());
    engine
        .load_str(
            r"
      (deftemplate item (slot bind) (slot break))
      (deffacts seed (outer (assert (item (bind 1) (break 2)))))
      (defrule probe (item (bind ?b) (break ?v)) => (printout t ?b ?v crlf))
    ",
        )
        .unwrap();
    engine.reset().unwrap();
    assert_eq!(engine.run(RunLimit::Count(5)).unwrap().rules_fired, 1);
    assert_eq!(engine.get_output("t"), Some("12\n"));
}

#[test]
fn method_query_effect_slot_heads_do_not_declare_local_bindings() {
    let engine = run(r"
      (deftemplate item (slot bind) (slot break))
      (defgeneric choose)
      (defmethod choose ((?v INTEGER (assert (item (bind ?v) (break 7))))) ?v)
      (defrule probe => (printout t (choose 3) crlf))
    ");
    assert_eq!(engine.get_output("t"), Some("3\n"));
    assert_eq!(engine.fact_count(), 1);
}

#[test]
fn bind_slot_names_do_not_create_phantom_rhs_variables() {
    let mut engine = Engine::new(EngineConfig::utf8());
    engine.load_str("(deftemplate item (slot bind))").unwrap();
    let errors = engine
        .load_str(
            r"
      (defrule invalid (exists (secret ?hidden)) =>
        (assert (item (bind ?hidden))))
    ",
        )
        .unwrap_err();
    assert!(
        errors
            .iter()
            .any(|error| error.to_string().contains("hidden")),
        "{errors:?}"
    );
    assert!(engine.rules().is_empty());
}

#[test]
fn syntax_slots_do_not_declare_phantom_callable_query_variables() {
    for body in [
        "(assert (box (bind ?ghost)))",
        "(assert (bind ?ghost))",
        "(modify 1 (bind ?ghost))",
        "(duplicate 1 (bind ?ghost))",
    ] {
        for definition in [
            format!("(deffunction invalid () {body} (any-factp ((?q item)) (= ?ghost 1)))"),
            format!("(defmethod invalid () {body} (any-factp ((?q item)) (= ?ghost 1)))"),
        ] {
            let mut engine = Engine::new(EngineConfig::utf8());
            engine
                .load_str("(deftemplate box (slot bind)) (deftemplate item)")
                .unwrap();
            let errors = engine.load_str(&definition).unwrap_err();
            assert!(
                errors
                    .iter()
                    .any(|error| error.to_string().contains("undefined variable ?ghost")),
                "{definition}: {errors:?}"
            );
        }
    }
}
