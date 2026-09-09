use ferric_rules_runtime::{Engine, RunLimit};

#[test]
fn invalid_callable_query_scope_cannot_replace_an_existing_definition() {
    let invalid = [
        ("(any-factp ((?f item)) (> ?missing 0))", "missing"),
        (
            "(find-fact ((?f item)) (> ?missing:value 0))",
            "missing:value",
        ),
        (
            "(bind ?other 3) (find-all-facts ((?f item)) (> ?other:value 0))",
            "other:value",
        ),
        (
            "(loop-for-count (?minimum 1 2) do TRUE) (any-factp ((?f item)) (> ?minimum 0))",
            "minimum",
        ),
        (
            "(progn$ (?minimum (create$ 1 2)) TRUE) (any-factp ((?f item)) (> ?minimum 0))",
            "minimum",
        ),
        (
            "(any-factp ((?f item)) TRUE) (any-factp ((?g item)) (> ?f:value 0))",
            "f:value",
        ),
        (
            "(any-factp ((?f item)) (absent-predicate ?f:value))",
            "absent-predicate",
        ),
    ];
    for method in [false, true] {
        for (body, expected) in invalid {
            let definition = if method {
                "defmethod keep 1"
            } else {
                "deffunction keep"
            };
            let mut engine = Engine::with_rules(&format!(
                "(deftemplate item (slot value))
                 ({definition} () 7)
                 (defrule probe => (printout t (keep) crlf))"
            ))
            .unwrap();
            let errors = engine
                .load_str(&format!("({definition} () {body})"))
                .unwrap_err();
            assert!(
                errors
                    .iter()
                    .any(|error| error.to_string().contains(expected)),
                "{errors:?}"
            );
            engine.run(RunLimit::Unlimited).unwrap();
            assert!(
                engine.action_diagnostics().is_empty(),
                "{:?}",
                engine.action_diagnostics()
            );
            assert_eq!(engine.get_output("t").unwrap(), Some("7\n"));
        }
    }
}

#[test]
fn invalid_method_query_does_not_create_a_generic() {
    let mut engine = Engine::with_rules("(deftemplate item (slot value))").unwrap();
    assert!(engine
        .load_str("(defmethod candidate () (any-factp ((?f item)) ?missing))")
        .is_err());
    engine.load_str("(deffunction candidate () 7)").unwrap();
}

#[test]
fn callable_query_scope_accepts_bind_names_independently_of_execution_order() {
    // Declarations are source-visible before execution, but an untaken or
    // later bind does not initialize its local. Execution is tested separately.
    for body in [
        "(any-factp ((?f item)) (> ?minimum 0)) (bind ?minimum 5)",
        "(if FALSE then (bind ?minimum 5)) (any-factp ((?f item)) (> ?minimum 0))",
        "(loop-for-count (?minimum 1 2) do (any-factp ((?f item)) (> ?f:value ?minimum)))",
        "(progn$ (?minimum (create$ 1 2)) (any-factp ((?f item)) (> ?f:value ?minimum-index)))",
        "(bind ?minimum 5) (loop-for-count (?minimum 1 2) do TRUE) (any-factp ((?f item)) (> ?minimum 0))",
        "(bind ?other:value 3) (any-factp ((?f item)) (> ?other:value 0))",
        "(any-factp ((?f item)) (any-factp ((?g item)) (> ?f:value ?g:value)))",
        "(any-factp ((?f item)) (probe))",
    ] {
        for definition in ["deffunction probe", "defmethod probe"] {
            let mut engine = Engine::with_rules("(deftemplate item (slot value))").unwrap();
            engine.load_str(&format!("({definition} () {body})")).unwrap();
        }
    }
}

#[test]
fn ordinary_callable_parameters_remain_visible_to_query_predicates() {
    for definition in [
        "deffunction probe (?minimum)",
        "defmethod probe ((?minimum INTEGER))",
    ] {
        let mut engine = Engine::with_rules(&format!(
            "(deftemplate item (slot value))
             (deffacts seed (item (value 3)))
             ({definition} (any-factp ((?f item)) (> ?f:value ?minimum)))
             (defrule check => (printout t (probe 2) \":\" (probe 4) crlf))"
        ))
        .unwrap();
        engine.run(RunLimit::Unlimited).unwrap();
        assert!(
            engine.action_diagnostics().is_empty(),
            "{:?}",
            engine.action_diagnostics()
        );
        assert_eq!(engine.get_output("t").unwrap(), Some("TRUE:FALSE\n"));
    }
}

#[test]
fn colon_named_rhs_local_is_used_only_without_an_active_fact_member() {
    let mut engine = Engine::with_rules(
        "(deftemplate item (slot value))
         (deffacts seed (item (value -1)))
         (defrule probe =>
           (bind ?other:value 3)
           (printout t
             (any-factp ((?f item)) (> ?other:value 0)) \":\"
             (any-factp ((?other item)) (> ?other:value 0)) \":\"
             ?other:value crlf))",
    )
    .unwrap();
    engine.run(RunLimit::Unlimited).unwrap();
    assert!(
        engine.action_diagnostics().is_empty(),
        "{:?}",
        engine.action_diagnostics()
    );
    assert_eq!(engine.get_output("t").unwrap(), Some("TRUE:FALSE:3\n"));
}

#[test]
fn callable_local_values_remain_visible_through_query_and_iterator_scopes() {
    for definition in [
        "deffunction probe (?minimum)",
        "defmethod probe ((?minimum INTEGER))",
    ] {
        let mut engine = Engine::with_rules(&format!(
            "(deftemplate item (slot value))
             (deffacts seed (item (value 3)))
             ({definition}
                (bind ?minimum (+ ?minimum 1))
                (bind ?total 0)
                (loop-for-count (?minimum 1 2) do
                    (if (any-factp ((?f item)) (> ?f:value ?minimum))
                        then (bind ?total (+ ?total 1))))
                (create$ ?total ?minimum
                    (any-factp ((?f item)) (> ?f:value ?minimum))))
             (defrule check => (printout t (probe 2) \":\" (probe 4) crlf))"
        ))
        .unwrap();
        engine.run(RunLimit::Unlimited).unwrap();
        assert!(
            engine.action_diagnostics().is_empty(),
            "{:?}",
            engine.action_diagnostics()
        );
        assert_eq!(
            engine.get_output("t").unwrap(),
            Some("(2 3 FALSE):(2 5 FALSE)\n")
        );
    }
}

#[test]
fn callable_query_members_mask_and_restore_rebound_and_colon_named_locals() {
    for definition in ["deffunction probe (?f)", "defmethod probe ((?f INTEGER))"] {
        let mut engine = Engine::with_rules(&format!(
            "(deftemplate item (slot value))
             (deffacts seed (item (value 3)))
             ({definition}
                (bind ?f 91)
                (bind ?f:value 92)
                (bind ?matched
                    (any-factp ((?f item))
                        (and (fact-existp ?f) (= ?f:value 3))))
                (create$ ?matched ?f ?f:value))
             (defrule check => (printout t (probe 90) \":\" (probe 80) crlf))"
        ))
        .unwrap();
        engine.run(RunLimit::Unlimited).unwrap();
        assert!(
            engine.action_diagnostics().is_empty(),
            "{:?}",
            engine.action_diagnostics()
        );
        assert_eq!(
            engine.get_output("t").unwrap(),
            Some("(TRUE 91 92):(TRUE 91 92)\n")
        );
    }
}

#[test]
fn callable_local_binding_does_not_relax_the_query_predicate_bind_prohibition() {
    for definition in ["deffunction probe", "defmethod probe"] {
        let mut engine = Engine::with_rules("(deftemplate item (slot value))").unwrap();
        let errors = engine
            .load_str(&format!(
                "({definition} ()
                (bind ?minimum 2)
                (any-factp ((?f item)) (bind ?minimum 3)))"
            ))
            .unwrap_err();
        assert!(
            errors
                .iter()
                .any(|error| error.to_string().contains("FACTQPSR2")),
            "{errors:?}"
        );
    }
}
