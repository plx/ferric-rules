use ferric_rules_runtime::{Engine, RunLimit};

const INVALID: &[(&str, &str)] = &[
    ("(loop-for-count (?x 1 2) do (bind ?x 3))", "PRCDRPSR1"),
    (
        "(loop-for-count (?x 1 2) do (if FALSE then (bind ?x 3)))",
        "PRCDRPSR1",
    ),
    (
        "(loop-for-count (?x 1 2) do (progn$ (?y (create$ 1)) (bind ?x 3)))",
        "PRCDRPSR1",
    ),
    ("(progn$ (?x (create$ 1 2)) (bind ?x 3))", "MULTIFUN2"),
    ("(foreach ?x (create$ 1 2) (bind $?x 3))", "MULTIFUN2"),
];

#[test]
fn iterator_bind_rejection_preserves_existing_callable_definitions() {
    for definition in ["deffunction keep", "defmethod keep 1"] {
        for (body, diagnostic) in INVALID {
            let mut engine = Engine::with_rules(&format!(
                "({definition} () 7) (defrule probe => (printout t (keep) crlf))"
            ))
            .unwrap();
            let errors = engine
                .load_str(&format!("({definition} () {body})"))
                .unwrap_err();
            assert!(
                errors
                    .iter()
                    .any(|error| error.to_string().contains(diagnostic)),
                "{errors:?}"
            );
            engine.run(RunLimit::Unlimited).unwrap();
            assert!(
                engine.action_diagnostics().is_empty(),
                "{:?}",
                engine.action_diagnostics()
            );
            assert_eq!(engine.get_output("t"), Some("7\n"));
        }
    }
}

#[test]
fn invalid_iterator_bind_does_not_create_a_generic() {
    for (body, diagnostic) in INVALID {
        let mut engine = Engine::with_rules("").unwrap();
        let errors = engine
            .load_str(&format!("(defmethod keep () {body})"))
            .unwrap_err();
        assert!(
            errors
                .iter()
                .any(|error| error.to_string().contains(diagnostic)),
            "{errors:?}"
        );
        engine.load_str("(deffunction keep () 9)").unwrap();
    }
}

#[test]
fn iterator_bind_guard_does_not_follow_calls_or_scope_loop_operands() {
    let mut engine = Engine::with_rules(
        "(deffunction change (?x) (bind ?x (+ ?x 1)) ?x)
         (deffunction probe (?x)
             (loop-for-count (?x (bind ?x 1) 2) do (change ?x))
             (progn$ (?x (create$ (bind ?x 9))) (change ?x))
             ?x)
         (defrule check => (printout t (probe 7) crlf))",
    )
    .unwrap();
    engine.run(RunLimit::Unlimited).unwrap();
    assert!(
        engine.action_diagnostics().is_empty(),
        "{:?}",
        engine.action_diagnostics()
    );
    assert_eq!(engine.get_output("t"), Some("9\n"));
}
