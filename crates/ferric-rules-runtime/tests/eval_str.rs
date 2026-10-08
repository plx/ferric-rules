use ferric_rules_core::Value;
use ferric_rules_runtime::{Engine, EngineConfig, EvalStrError};

#[test]
fn roots_have_fresh_locals_but_one_expression_can_bind_and_read() {
    let mut engine = Engine::new(EngineConfig::default());
    assert!(matches!(
        engine.eval_str("(+ 1 2)").unwrap(),
        Value::Integer(3)
    ));
    assert!(matches!(
        engine.eval_str("(bind ?x 3)").unwrap(),
        Value::Integer(3)
    ));
    assert!(matches!(
        engine.eval_str("?x"),
        Err(EvalStrError::Evaluation(_))
    ));
    assert!(matches!(
        engine.eval_str("(progn (bind ?x 3) (+ ?x 1))").unwrap(),
        Value::Integer(4)
    ));
    assert!(matches!(
        engine.eval_str("; comment\n 7 ; trailing\n").unwrap(),
        Value::Integer(7)
    ));
}

#[test]
fn multiple_forms_and_source_errors_do_not_evaluate_a_prefix() {
    let mut engine = Engine::new(EngineConfig::default());
    let error = engine
        .eval_str("(assert (unexpected))\n(+ 1 2)")
        .unwrap_err();
    assert!(matches!(error, EvalStrError::Source(_)));
    assert!(error.to_string().contains("line 2, column 1"));
    assert_eq!(engine.facts().unwrap().count(), 0);
    assert!(matches!(
        engine.eval_str("; empty"),
        Err(EvalStrError::Source(_))
    ));
    let error = engine.eval_str("\n(+ 1").unwrap_err();
    assert!(matches!(error, EvalStrError::Source(_)));
    let EvalStrError::Source(errors) = error else {
        unreachable!()
    };
    assert!(
        matches!(&errors[0], ferric_rules_runtime::LoadError::Parse(error) if error.span.start.line == 2)
    );
}

#[test]
fn runtime_errors_preserve_prior_output_and_effects() {
    let mut engine = Engine::new(EngineConfig::default());
    assert!(matches!(
        engine.eval_str("(progn (printout t kept) (assert (p 1)) (/ 1 0))"),
        Err(EvalStrError::Evaluation(_))
    ));
    assert_eq!(engine.get_output("t"), Some("kept"));
    assert_eq!(engine.facts().unwrap().count(), 1);
    assert!(matches!(
        engine.eval_str("(progn (printout t more) ?missing)"),
        Err(EvalStrError::Evaluation(_))
    ));
    assert_eq!(engine.get_output("t"), Some("keptmore"));
    assert!(matches!(
        engine.eval_str("(+ 2 3)").unwrap(),
        Value::Integer(5)
    ));
}

#[test]
fn root_clear_preserves_compiled_literals_output_and_input() {
    let mut engine = Engine::with_rules("(deffunction old () 9)").unwrap();
    engine.eval_str("(assert (p))").unwrap();
    engine.push_input("42");
    // The expression names no template or relation, so nothing is in use.
    assert!(matches!(engine.eval_str(
        r#"(progn (printout t before) (clear) (printout t after) (assert-string "(q)") (str-length "kept"))"#
    ).unwrap(), Value::Integer(4)));
    assert_eq!(engine.get_output("t"), Some("beforeafter"));
    assert!(matches!(
        engine.eval_str("(read)").unwrap(),
        Value::Integer(42)
    ));
    assert!(matches!(
        engine.eval_str("(old)"),
        Err(EvalStrError::Source(_))
    ));
    assert!(matches!(
        engine.eval_str("(fact-index (assert (r)))").unwrap(),
        Value::Integer(2)
    ));
}

/// CLIPS 6.30 keeps the templates and relations a top-level command names in
/// use while it runs: its `clear` refuses without removing facts, and its
/// `build` cannot redefine them. Dynamic `eval` source is held the same way.
#[test]
fn root_clear_refuses_while_the_expression_names_a_template_or_relation() {
    let mut engine = Engine::with_rules("(deftemplate foo (slot x))").unwrap();
    engine
        .eval_str("(progn (assert (foo (x 1))) (clear) (assert (foo (x 2))))")
        .unwrap();
    assert_eq!(engine.facts().unwrap().count(), 2);
    assert!(engine.get_output("werror").unwrap().contains("CONSTRCT1"));
    engine.clear_output_channel("werror");
    engine.eval_str("(progn (clear) (assert (bar)))").unwrap();
    engine
        .eval_str("(progn (find-all-facts ((?f foo)) TRUE) (clear))")
        .unwrap();
    engine
        .eval_str(r#"(eval "(progn (assert (foo (x 3))) (clear))")"#)
        .unwrap();
    assert_eq!(engine.facts().unwrap().count(), 4);
    assert_eq!(
        engine
            .get_output("werror")
            .unwrap()
            .matches("CONSTRCT1")
            .count(),
        3
    );
    assert!(matches!(
        engine
            .eval_str(r#"(progn (build "(deftemplate foo (slot y))") (assert (foo (x 9))) 1)"#)
            .unwrap(),
        Value::Integer(1)
    ));
    assert_eq!(engine.facts().unwrap().count(), 5);
    engine.eval_str(r#"(eval "(clear)")"#).unwrap();
    assert_eq!(engine.facts().unwrap().count(), 0);
    assert!(engine.eval_str("(assert (foo (x 1)))").is_err());
}

/// CLIPS 6.30 refuses a clear from a deffunction that a top-level command
/// calls, directly or through `funcall`, and keeps the facts: after
/// `(assert (p))` both `(keep)` and `(funcall keep)` print CONSTRCT1 and 9,
/// and `(facts)` still lists f-1 (p).
#[test]
fn root_called_callable_clear_keeps_the_callable_and_facts() {
    let mut engine = Engine::with_rules("(deffunction keep () (clear) 9)").unwrap();
    engine.eval_str("(assert (p))").unwrap();
    for source in ["(keep)", "(funcall keep)", r#"(eval "(keep)")"#] {
        assert!(matches!(
            engine.eval_str(source).unwrap(),
            Value::Integer(9)
        ));
        assert_eq!(engine.facts().unwrap().count(), 1, "{source}");
    }
    assert_eq!(
        engine
            .get_output("werror")
            .unwrap()
            .matches("CONSTRCT1")
            .count(),
        3
    );
}

/// CLIPS binds the callables a top-level command names when it parses it, so
/// a clear inside the same expression refuses: with `(deffunction f () 7)`,
/// `(progn (clear) (f))` and `(eval "(progn (clear) (f))")` print CONSTRCT1
/// and return 7, while `(eval "(progn (clear) (+ 1 2))")` clears and gives 3.
#[test]
fn root_clear_refuses_while_the_expression_calls_a_user_callable() {
    let mut engine =
        Engine::with_rules("(deffunction f () 7) (defgeneric g) (defmethod g () 8)").unwrap();
    engine.eval_str("(assert (p))").unwrap();
    for (source, expected) in [
        ("(progn (clear) (f))", 7),
        (r#"(eval "(progn (clear) (f))")"#, 7),
        ("(progn (clear) (g))", 8),
        ("(progn (clear) (MAIN::f))", 7),
    ] {
        assert!(
            matches!(engine.eval_str(source).unwrap(), Value::Integer(value) if value == expected),
            "{source}"
        );
        assert_eq!(engine.facts().unwrap().count(), 1, "{source}");
    }
    assert_eq!(
        engine
            .get_output("werror")
            .unwrap()
            .matches("CONSTRCT1")
            .count(),
        4
    );
    assert!(matches!(
        engine
            .eval_str(r#"(eval "(progn (clear) (+ 1 2))")"#)
            .unwrap(),
        Value::Integer(3)
    ));
    assert!(engine.eval_str("(f)").is_err());
}

#[test]
fn root_clear_resets_the_current_module_for_remaining_operands() {
    let mut engine = Engine::with_rules("(defmodule A)").unwrap();
    let value = engine
        .eval_str(r#"(progn (clear) (build "(deffunction after () 7)") (eval "(after)"))"#)
        .unwrap();
    assert!(matches!(value, Value::Integer(7)));
}

/// An engine whose current module is `A` (`with_rules` would reset to MAIN).
fn engine_in_module_a() -> Engine {
    let mut engine = Engine::new(EngineConfig::default());
    engine.load_str("(defmodule A)").unwrap();
    assert_eq!(engine.current_module(), "A");
    engine
}

fn rule_list(engine: &mut Engine) -> String {
    let value = engine.eval_str("(get-defrule-list *)").unwrap();
    let Value::Multifield(items) = value else {
        panic!("expected a multifield, got {value:?}");
    };
    items
        .iter()
        .map(|item| match item {
            Value::Symbol(symbol) => engine.resolve_core_symbol(*symbol).unwrap().to_owned(),
            other => panic!("expected a symbol, got {other:?}"),
        })
        .collect::<Vec<_>>()
        .join(" ")
}

#[test]
fn clear_inside_eval_moves_the_enclosing_expression_to_main() {
    // CLIPS 6.30 lists (MAIN::r) and prints `fired` after reset/run.
    let mut engine = engine_in_module_a();
    engine
        .eval_str(r#"(progn (eval "(clear)") (build "(defrule r => (printout t fired crlf))"))"#)
        .unwrap();
    assert_eq!(rule_list(&mut engine), "MAIN::r");
    engine.reset().unwrap();
    engine
        .run(ferric_rules_runtime::RunLimit::Unlimited)
        .unwrap();
    assert_eq!(engine.get_output("t"), Some("fired\n"));
    // Nested sources carry the change out one level per return.
    let mut engine = engine_in_module_a();
    engine
        .eval_str(r#"(progn (eval "(eval \"(clear)\")") (build "(defrule r =>)"))"#)
        .unwrap();
    assert_eq!(rule_list(&mut engine), "MAIN::r");
}

#[test]
fn reset_inside_eval_moves_the_enclosing_expression_to_main() {
    // CLIPS 6.30 lists (MAIN::r).
    let mut engine = engine_in_module_a();
    engine
        .eval_str(r#"(progn (eval "(reset)") (build "(defrule r => (printout t fired crlf))"))"#)
        .unwrap();
    assert_eq!(rule_list(&mut engine), "MAIN::r");
}

/// CLIPS binds a top-level command's callable and template references when it
/// parses the command, so a root `reset` that selects MAIN does not move
/// them: with module A current and `f` and `t` defined in A,
/// `(progn (reset) (f))`, `(eval "(progn (reset) (f))")` and
/// `(progn (reset) (assert (t (x 1))))` succeed. Dynamic source evaluated
/// after the reset resolves in MAIN, so `(progn (reset) (eval "(f)"))` fails.
#[test]
fn root_reset_keeps_the_expression_bound_to_its_module() {
    let engine_in_a = || {
        let mut engine = Engine::new(EngineConfig::default());
        engine
            .load_str(
                "(defmodule MAIN (export ?ALL)) (defmodule A (import MAIN ?ALL))
                 (deffunction f () 7) (deftemplate t (slot x))",
            )
            .unwrap();
        assert_eq!(engine.current_module(), "A");
        engine
    };
    for source in ["(progn (reset) (f))", r#"(eval "(progn (reset) (f))")"#] {
        let mut engine = engine_in_a();
        assert!(
            matches!(engine.eval_str(source).unwrap(), Value::Integer(7)),
            "{source}"
        );
        assert_eq!(engine.current_module(), "MAIN");
    }
    let mut engine = engine_in_a();
    assert!(matches!(
        engine
            .eval_str("(progn (reset) (assert (t (x 1))))")
            .unwrap(),
        Value::FactAddress(_)
    ));
    let mut engine = engine_in_a();
    assert!(engine.eval_str(r#"(progn (reset) (eval "(f)"))"#).is_err());
    // The selection ends with the expression: the next root resolves in MAIN.
    assert!(engine.eval_str("(f)").is_err());
    // Runtime construct lookups follow the new current module, as in CLIPS,
    // which lists (m) here.
    let mut engine = Engine::new(EngineConfig::default());
    engine
        .load_str("(defrule MAIN::m =>) (defmodule A) (defrule A::r =>)")
        .unwrap();
    let Value::Multifield(rules) = engine
        .eval_str("(progn (reset) (get-defrule-list))")
        .unwrap()
    else {
        panic!("expected a multifield");
    };
    assert!(
        matches!(rules.as_slice(), [Value::Symbol(name)] if engine.resolve_core_symbol(*name) == Some("m")),
        "{rules:?}"
    );
}

/// Names an expression looks up while it runs resolve in the module a root
/// reset selects. With MAIN's `gt` and generic `g` and module A current,
/// CLIPS 6.30 gives `(1 2 3)` for `(progn (reset) (sort gt 1 3 2))`, TRUE for
/// `(progn (reset) (funcall gt 2 1))`, 8 for
/// `(progn (reset) (call-specific-method g 1 4))` and accepts
/// `(progn (reset) (watch deffunctions gt))`. With A's own `f` and A current,
/// `(progn (reset) (funcall f))` prints `[ARGACCES5] Function funcall expected
/// argument #1 to be of type function, deffunction, or generic function name`
/// and returns FALSE.
#[test]
fn root_reset_moves_runtime_name_lookups_to_main() {
    let engine_in_a = || {
        let mut engine = Engine::new(EngineConfig::default());
        engine
            .load_str(
                "(deffunction gt (?a ?b) (> ?a ?b))
                 (defgeneric g) (defmethod g ((?x INTEGER)) (* ?x 2))
                 (defmodule A) (deffunction f () 7)",
            )
            .unwrap();
        assert_eq!(engine.current_module(), "A");
        engine
    };
    let mut engine = engine_in_a();
    let Value::Multifield(sorted) = engine.eval_str("(progn (reset) (sort gt 1 3 2))").unwrap()
    else {
        panic!("expected a multifield");
    };
    assert!(
        matches!(
            sorted.as_slice(),
            [Value::Integer(1), Value::Integer(2), Value::Integer(3)]
        ),
        "{sorted:?}"
    );
    let mut engine = engine_in_a();
    let value = engine.eval_str("(progn (reset) (funcall gt 2 1))").unwrap();
    assert!(
        matches!(value, Value::Symbol(symbol) if engine.resolve_core_symbol(symbol) == Some("TRUE"))
    );
    let mut engine = engine_in_a();
    assert!(matches!(
        engine
            .eval_str("(progn (reset) (call-specific-method g 1 4))")
            .unwrap(),
        Value::Integer(8)
    ));
    let mut engine = engine_in_a();
    engine
        .eval_str("(progn (reset) (watch deffunctions gt))")
        .unwrap();
    let mut engine = engine_in_a();
    let value = engine.eval_str("(progn (reset) (funcall f))").unwrap();
    assert!(
        matches!(value, Value::Symbol(symbol) if engine.resolve_core_symbol(symbol) == Some("FALSE"))
    );
    assert_eq!(
        engine.get_output("werror"),
        Some("[ARGACCES5] Function funcall expected argument #1 to be of type function, deffunction, or generic function name\n")
    );
}

/// CLIPS 6.30's `funcall` never resolves a module-qualified name, even one
/// that names a visible deffunction: with MAIN's `gt`,
/// `(funcall MAIN::gt 2 (progn (printout t "operand" crlf) 1))` and
/// `(funcall "MAIN::gt" 2 1)` each print `[ARGACCES5] Function funcall expected
/// argument #1 to be of type function, deffunction, or generic function name`
/// and return FALSE without evaluating the operands.
#[test]
fn funcall_reports_every_module_qualified_name() {
    let mut engine = Engine::new(EngineConfig::default());
    engine
        .load_str("(deffunction gt (?a ?b) (> ?a ?b))")
        .unwrap();
    for source in [
        r#"(funcall MAIN::gt 2 (progn (printout t "operand" crlf) 1))"#,
        r#"(funcall "MAIN::gt" 2 1)"#,
        "(funcall MAIN::nosuch)",
    ] {
        engine.clear_output_channel("werror");
        let value = engine.eval_str(source).unwrap();
        assert!(
            matches!(value, Value::Symbol(symbol) if engine.resolve_core_symbol(symbol) == Some("FALSE")),
            "{source}: {value:?}"
        );
        assert_eq!(
            engine.get_output("werror"),
            Some("[ARGACCES5] Function funcall expected argument #1 to be of type function, deffunction, or generic function name\n"),
            "{source}"
        );
    }
    assert_eq!(engine.get_output("t"), None);
}

/// A root clear deletes the expression's module, so later lookups resolve in
/// the new MAIN: after `(defmodule A)`, CLIPS 6.30 returns 5 for
/// `(progn (clear) (build "(deffunction h () 5)") (funcall h))`.
#[test]
fn root_clear_moves_runtime_name_lookups_to_the_new_main() {
    let mut engine = engine_in_module_a();
    let value = engine
        .eval_str(r#"(progn (clear) (build "(deffunction h () 5)") (funcall h))"#)
        .unwrap();
    assert!(matches!(value, Value::Integer(5)), "{value:?}");
    assert_eq!(engine.current_module(), "MAIN");
}

/// `save-facts` checks each selector and saves in the module current once the
/// selectors have run. With `MAIN::p` and `A::p` deffacts and A current, CLIPS
/// 6.30's `(save-facts F local (progn (reset) p))` returns TRUE and writes
/// only `(p (x main))`.
#[test]
fn save_facts_selectors_follow_a_root_reset_to_main() {
    let mut engine = Engine::new(EngineConfig::default());
    engine
        .load_str(
            "(defmodule MAIN (export ?ALL))
             (deftemplate MAIN::p (slot x)) (deffacts MAIN::d (p (x main)))
             (defmodule A)
             (deftemplate A::p (slot x)) (deffacts A::da (p (x a)))",
        )
        .unwrap();
    assert_eq!(engine.current_module(), "A");
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("saved.fct");
    let value = engine
        .eval_str(&format!(
            r#"(save-facts "{}" local (progn (reset) p))"#,
            path.display()
        ))
        .unwrap();
    assert!(
        matches!(value, Value::Symbol(symbol) if engine.resolve_core_symbol(symbol) == Some("TRUE"))
    );
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "(p (x main))\n");
}

/// A fact query binds its template only through a literal restriction. CLIPS
/// 6.30 clears for `(progn (clear) (find-all-facts ((?f (sym-cat p))) TRUE))`
/// and then reports `[PRNTUTIL1] Unable to find deftemplate p`, while
/// `(progn (clear) (find-all-facts ((?f p)) TRUE))` prints CONSTRCT1 and
/// returns both facts.
#[test]
fn root_clear_ignores_a_computed_query_restriction() {
    let engine_with_facts = || {
        let mut engine =
            Engine::with_rules("(deftemplate p (slot x)) (deffacts d (p (x 1)) (p (x 2)))")
                .unwrap();
        engine.reset().unwrap();
        assert_eq!(engine.facts().unwrap().count(), 2);
        engine
    };
    let mut engine = engine_with_facts();
    assert!(engine
        .eval_str("(progn (clear) (find-all-facts ((?f (sym-cat p))) TRUE))")
        .is_err());
    assert_eq!(engine.facts().unwrap().count(), 0);
    assert!(engine.eval_str("(assert (p (x 3)))").is_err());
    let mut engine = engine_with_facts();
    let Value::Multifield(found) = engine
        .eval_str("(progn (clear) (find-all-facts ((?f p)) TRUE))")
        .unwrap()
    else {
        panic!("expected a multifield");
    };
    assert_eq!(found.len(), 2);
    assert_eq!(engine.facts().unwrap().count(), 2);
    assert!(engine.get_output("werror").unwrap().contains("CONSTRCT1"));
}

/// Inside a rule, CLIPS 6.30 refuses the clear in
/// `(eval "(progn (clear) (find-all-facts ((?f (sym-cat p))) TRUE))")` but
/// still removes the facts, so the query returns `()`.
#[test]
fn rule_eval_clear_with_a_computed_query_restriction_removes_facts() {
    let mut engine = Engine::with_rules(
        r#"(deftemplate p (slot x)) (deffacts d (p (x 1)) (p (x 2)))
           (defrule go =>
             (bind ?found (eval "(progn (clear) (find-all-facts ((?f (sym-cat p))) TRUE))"))
             (printout t (length$ ?found) " " (length$ (find-all-facts ((?f p)) TRUE)) crlf))"#,
    )
    .unwrap();
    engine.reset().unwrap();
    engine
        .run(ferric_rules_runtime::RunLimit::Unlimited)
        .unwrap();
    assert_eq!(engine.get_output("t"), Some("0 0\n"));
    assert_eq!(engine.facts().unwrap().count(), 0);
    assert!(engine.get_output("werror").unwrap().contains("CONSTRCT1"));
}

#[test]
fn clear_during_fact_initialization_keeps_old_facts_and_releases_guard_on_errors() {
    let mut engine = Engine::with_rules(
        "(deftemplate p (slot x (default-dynamic (progn (printout t default) (clear)))))",
    )
    .unwrap();
    engine.eval_str("(assert (old))").unwrap();
    engine.eval_str("(assert (p))").unwrap();
    engine.eval_str("(assert (ordered (clear)))").unwrap();
    assert_eq!(engine.facts().unwrap().count(), 3);
    assert_eq!(engine.get_output("t"), Some("default"));
    assert!(engine.get_output("werror").unwrap().contains("CONSTRCT1"));
    assert!(engine.eval_str("(assert (failed (/ 1 0)))").is_err());
    engine.eval_str("(clear)").unwrap();
    assert_eq!(engine.facts().unwrap().count(), 0);
    assert!(engine.eval_str("(assert (p (x 7)))").is_err());
}

#[test]
fn source_reset_selects_main_for_the_shell_and_remaining_root_operands() {
    let mut engine = Engine::new(EngineConfig::default());
    engine
        .load_str("(defrule main =>) (defmodule A) (defrule other =>)")
        .unwrap();
    assert_eq!(engine.current_module(), "A");
    engine.reset().unwrap();
    assert_eq!(engine.current_module(), "MAIN");
    engine.load_str("(defmodule A)").unwrap();
    engine
        .eval_str(r#"(progn (reset) (build "(deffunction after () 7)"))"#)
        .unwrap();
    assert_eq!(engine.current_module(), "MAIN");
    assert!(engine
        .agenda_entries()
        .iter()
        .all(|entry| entry.module_name == "MAIN"));
    assert!(matches!(
        engine.eval_str("(after)").unwrap(),
        Value::Integer(7)
    ));
}

#[test]
fn excerpt_entry_points_locate_diagnostics_in_the_enclosing_source() {
    let mut engine = Engine::new(EngineConfig::default());
    let error = engine.eval_str_at("(missing-function)", 3, 3).unwrap_err();
    assert!(error.to_string().contains("line 3, column 3"), "{error}");
    let error = engine.eval_str_at("(+ 1\n (", 8, 4).unwrap_err();
    assert!(matches!(
        &error,
        EvalStrError::Source(errors) if matches!(&errors[0],
            ferric_rules_runtime::LoadError::Parse(error) if error.span.start.line == 9)
    ));
    let errors = engine
        .load_str_at("(defrule r\n  =>\n  (missing-function))", 20, 5)
        .unwrap_err();
    let text = errors[0].to_string();
    assert!(text.contains("line 22, column 3"), "{text}");
    assert!(engine.load_str_at("(defrule ok =>)", 40, 1).is_ok());
    assert!(matches!(
        engine.eval_str_at("(+ 1 2)", 7, 9).unwrap(),
        Value::Integer(3)
    ));
}
