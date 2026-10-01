//! Ferric's own output actions next to the #345 multifield printout fix:
//! the `println` convenience action and the file written by `save-facts`,
//! neither of which the corpus can observe.

use ferric_rules_runtime::{Engine, EngineConfig, HaltReason, RunLimit};

fn run(source: &str) -> Engine {
    let mut engine = Engine::new(EngineConfig::utf8());
    engine.load_str(source).unwrap();
    engine.reset().unwrap();
    let result = engine.run(RunLimit::Count(10)).unwrap();
    assert_eq!(result.halt_reason, HaltReason::AgendaEmpty);
    assert_eq!(result.rules_fired, 1);
    assert!(engine.action_diagnostics().is_empty());
    engine
}

#[test]
fn println_quotes_multifield_strings_like_printout() {
    let engine = run(r#"(defrule exercise => (println "raw " (create$ "two words")))"#);
    assert_eq!(engine.get_output("t"), Some("raw (\"two words\")\n"));
}

#[test]
fn save_facts_escapes_string_fields_that_printout_writes_raw() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("quoted.fct");
    let path = path.to_str().unwrap();
    let path_literal = format!("\"{}\"", path.replace('\\', "\\\\").replace('"', "\\\""));
    let engine = run(&format!(
        r#"(deffacts seed (row "a\"b" "a\\b" "two words" 3.5))
           (defrule exercise =>
             (printout t (create$ "a\"b" "a\\b" "two words" 3.5) crlf)
             (save-facts {path_literal}))"#
    ));
    assert_eq!(
        engine.get_output("t"),
        Some("(\"a\"b\" \"a\\b\" \"two words\" 3.5)\n")
    );
    let saved = std::fs::read_to_string(path).unwrap();
    assert!(
        saved
            .lines()
            .any(|line| line == r#"(row "a\"b" "a\\b" "two words" 3.5)"#),
        "{saved:?}"
    );
}
