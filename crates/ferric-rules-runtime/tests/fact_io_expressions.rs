//! Native CLIPS 6.30 fact-file return values, ordering, and partial failures.

use std::path::Path;

use ferric_rules_core::{Fact, Value};
use ferric_rules_runtime::{Engine, EngineConfig, FactHandle, HaltReason, RunLimit};

fn quoted(path: &Path) -> String {
    format!("\"{}\"", path.display())
}

fn boolean(engine: &mut Engine, expression: &str) -> String {
    let value = engine.eval_str(expression).unwrap();
    engine.format_value(&value)
}

fn template_facts(engine: &Engine, name: &str) -> Vec<FactHandle> {
    engine
        .facts()
        .unwrap()
        .filter_map(|(id, fact)| {
            let Fact::Template(fact) = fact else {
                return None;
            };
            (engine.template_name_by_id(fact.template_id) == Some(name)).then_some(id)
        })
        .collect()
}

#[test]
fn expression_and_action_save_roundtrip_literals_in_fact_chronology() {
    let directory = tempfile::tempdir().unwrap();
    let expression_path = directory.path().join("expression.fct");
    let action_path = directory.path().join("action.fct");
    let definitions = "(deftemplate item (slot text) (slot number) (multislot values))";
    let mut engine = Engine::with_rules(definitions).unwrap();
    engine
        .eval_str(r#"(assert (item (text "a\"b\\c") (number 2.0) (values a 3 "d")))"#)
        .unwrap();
    engine.eval_str("(bind ?temporary (assert (old)))").unwrap();
    let old = engine.find_facts("old").unwrap()[0].0;
    engine.retract(old).unwrap();
    engine.eval_str("(assert (later 7))").unwrap();
    assert_eq!(
        boolean(
            &mut engine,
            &format!("(save-facts {})", quoted(&expression_path))
        ),
        "TRUE"
    );
    engine
        .load_str(&format!(
            "(defrule save => (save-facts {}))",
            quoted(&action_path)
        ))
        .unwrap();
    let result = engine.run(RunLimit::Count(10)).unwrap();
    assert_eq!(result.halt_reason, HaltReason::AgendaEmpty);
    assert!(engine.action_diagnostics().is_empty());
    let saved = std::fs::read_to_string(&expression_path).unwrap();
    assert_eq!(saved, std::fs::read_to_string(&action_path).unwrap());
    assert!(saved.starts_with("(initial-fact)\n"));
    assert!(saved.contains("(text \"a\\\"b\\\\c\")"));
    assert!(saved.contains("(number 2.0)"));
    assert!(saved.ends_with("(later 7)\n"));
    let mut restored = Engine::with_rules(definitions).unwrap();
    assert_eq!(
        boolean(
            &mut restored,
            &format!("(load-facts {})", quoted(&expression_path))
        ),
        "TRUE"
    );
    let item = template_facts(&restored, "item")[0];
    assert!(
        matches!(restored.get_fact_slot_by_name(item, "text").unwrap(), Value::String(text) if text.as_str() == "a\"b\\c")
    );
    assert!(
        matches!(restored.get_fact_slot_by_name(item, "number").unwrap(), Value::Float(value) if value.to_bits() == 2.0_f64.to_bits())
    );
    assert_eq!(restored.find_facts("later").unwrap().len(), 1);
}

#[test]
fn save_modes_and_selectors_use_module_visibility_and_fact_order() {
    let directory = tempfile::tempdir().unwrap();
    let local = directory.path().join("local.fct");
    let visible = directory.path().join("visible.fct");
    let invalid = directory.path().join("invalid.fct");
    let mut engine = Engine::new(EngineConfig::default());
    engine
        .load_str(
            "(defmodule A (export deftemplate p)) (deftemplate p (slot x)) (assert (p (x 1)))
         (defmodule B (import A deftemplate p)) (deftemplate q (slot x)) (assert (q (x 2)))",
        )
        .unwrap();
    assert_eq!(
        boolean(&mut engine, &format!("(save-facts {})", quoted(&local))),
        "TRUE"
    );
    assert_eq!(std::fs::read_to_string(local).unwrap(), "(q (x 2))\n");
    assert_eq!(
        boolean(
            &mut engine,
            &format!("(save-facts {} visible q p)", quoted(&visible))
        ),
        "TRUE"
    );
    assert_eq!(
        std::fs::read_to_string(visible).unwrap(),
        "(p (x 1))\n(q (x 2))\n"
    );
    assert_eq!(
        boolean(
            &mut engine,
            &format!("(save-facts {} local p)", quoted(&invalid))
        ),
        "FALSE"
    );
    assert_eq!(std::fs::read_to_string(invalid).unwrap(), "");
    assert!(engine.get_output("werror").unwrap().contains("ARGACCES5"));
}

#[test]
fn save_evaluates_filename_mode_open_and_selectors_in_reference_order() {
    let directory = tempfile::tempdir().unwrap();
    let absent = directory.path().join("absent/facts.fct");
    let wrong_mode = directory.path().join("wrong-mode.fct");
    let wrong_selector = directory.path().join("wrong-selector.fct");
    let mut engine = Engine::with_rules("(deftemplate p)").unwrap();
    assert_eq!(boolean(&mut engine, &format!(
        "(save-facts (progn (printout t FILE) {}) (progn (printout t MODE) local) (progn (printout t SKIP) p))", quoted(&absent)
    )), "FALSE");
    assert_eq!(engine.get_output("t"), Some("FILEMODE"));
    assert_eq!(
        boolean(
            &mut engine,
            &format!(
                "(save-facts {} (progn (printout t MODE) wrong) (progn (printout t SKIP) p))",
                quoted(&wrong_mode)
            )
        ),
        "FALSE"
    );
    assert!(!wrong_mode.exists());
    assert_eq!(boolean(&mut engine, &format!(
        "(save-facts {} local (progn (printout t ONE) missing) (progn (printout t SKIP) p))", quoted(&wrong_selector)
    )), "FALSE");
    assert_eq!(std::fs::read_to_string(wrong_selector).unwrap(), "");
    assert_eq!(engine.get_output("t"), Some("FILEMODEMODEONE"));
}

#[test]
fn save_mode_values_that_are_not_symbols_halt_but_selector_values_continue() {
    let directory = tempfile::tempdir().unwrap();
    let mode = directory.path().join("mode.fct");
    let selector = directory.path().join("selector.fct");
    let mut engine = Engine::with_rules(&format!(
        "(deftemplate p)
         (deffunction text-mode () \"local\")
         (deffunction number () 7)
         (defrule mode => (printout t A (save-facts {} (text-mode)) crlf) (printout t B crlf))
         (defrule selector (declare (salience 1)) => (printout t C (save-facts {} local p (number)) crlf)
             (printout t D crlf))",
        quoted(&mode),
        quoted(&selector)
    ))
    .unwrap();
    // CLIPS 6.30: a run-time STRING mode prints ARGACCES5 "of type symbol" and
    // PRCCODE4, halting the rule; a non-symbol selector prints ARGACCES5 and
    // returns FALSE while the RHS continues.
    let result = engine.run(RunLimit::Unlimited).unwrap();
    assert_eq!(result.halt_reason, HaltReason::ActionError);
    assert_eq!(engine.get_output("t"), Some("CFALSE\nD\nA"));
    assert!(!mode.exists());
    assert_eq!(std::fs::read_to_string(&selector).unwrap(), "");
    assert!(engine
        .get_output("werror")
        .unwrap()
        .contains("[ARGACCES5] Function save-facts expected argument #4 to be of type symbol\n"));
    let error = engine
        .eval_str(&format!("(save-facts {} (number))", quoted(&mode)))
        .unwrap_err();
    assert!(error.to_string().contains("SYMBOL mode"), "{error}");
}

#[test]
fn load_content_errors_stop_the_evaluation_and_retain_only_the_prefix() {
    let directory = tempfile::tempdir().unwrap();
    for (name, source) in [
        ("syntax", "(p (x 1))\n(p (x 2)"),
        ("constraint", "(p (x 1))\n(p (x wrong))\n(p (x 3))"),
        ("expression", "(p (x 1))\n(p (x (+ 1 1)))\n(p (x 3))"),
    ] {
        let path = directory.path().join(name);
        std::fs::write(&path, source).unwrap();
        let mut engine = Engine::with_rules("(deftemplate p (slot x (type INTEGER)))").unwrap();
        // CLIPS 6.30 prints the error, "Function load-facts encountered an
        // error", and stops the enclosing evaluation.
        let error = engine
            .eval_str(&format!(
                "(progn (load-facts {}) (printout t continued) 5)",
                quoted(&path)
            ))
            .unwrap_err()
            .to_string();
        assert!(
            error.contains("Function load-facts encountered an error"),
            "{name}: {error}"
        );
        assert_eq!(engine.get_output("t"), None, "{name}");
        let facts = template_facts(&engine, "p");
        assert_eq!(facts.len(), 1, "{name}");
        assert!(matches!(
            engine.get_fact_slot_by_name(facts[0], "x").unwrap(),
            Value::Integer(1)
        ));
    }
}

#[test]
fn load_evaluates_defaults_and_io_failures_are_recoverable() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("defaults.fct");
    std::fs::write(&path, "(p)\n(p (x 8))\n").unwrap();
    let mut engine = Engine::with_rules(
        "(defglobal ?*calls* = 0) (deftemplate p (slot x (default-dynamic (bind ?*calls* (+ ?*calls* 1)))))"
    ).unwrap();
    assert_eq!(
        boolean(&mut engine, &format!("(load-facts {})", quoted(&path))),
        "TRUE"
    );
    assert!(matches!(
        engine.eval_str("?*calls*").unwrap(),
        Value::Integer(1)
    ));
    assert_eq!(template_facts(&engine, "p").len(), 2);
    let absent = directory.path().join("absent.fct");
    assert_eq!(
        boolean(&mut engine, &format!("(load-facts {})", quoted(&absent))),
        "FALSE"
    );
    assert!(engine.get_output("werror").unwrap().contains("ARGACCES2"));
    assert!(engine.eval_str("(load-facts 7)").is_err());
    assert!(engine.eval_str("(load-facts x y)").is_err());
    let forbidden = directory.path().join("match-time.fct");
    engine
        .load_str(&format!(
            "(defrule forbidden (test (save-facts {})) =>)",
            quoted(&forbidden)
        ))
        .unwrap();
    assert!(
        !forbidden.exists(),
        "match conditions cannot perform file I/O"
    );
    assert_eq!(engine.run(RunLimit::Count(10)).unwrap().rules_fired, 0);
}
