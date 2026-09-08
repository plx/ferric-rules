//! `create$` scalar VOID omission, verified independently of any formatter.
//! Six exact source programs match pinned CLIPS 6.30 observations of actual
//! field types, lengths, output effects, and continued rule execution.

use ferric_rules_core::Value;
use ferric_rules_runtime::{Engine, EngineConfig, HaltReason, RunLimit};

fn verify(source: &str, expected: &[(&str, &str)], output: &str, trace: i64) {
    let mut engine = Engine::new(EngineConfig::utf8());
    engine.load_str(source).unwrap();
    engine.reset().unwrap();
    let result = engine.run(RunLimit::Count(10)).unwrap();
    assert_eq!(result.rules_fired, 1);
    assert_eq!(result.halt_reason, HaltReason::AgendaEmpty);
    assert!(engine.action_diagnostics().is_empty());
    let Some(Value::Multifield(fields)) = engine.get_global("result") else {
        panic!("create$ must store an actual MULTIFIELD")
    };
    assert_eq!(fields.len(), expected.len());
    for (field, (tag, text)) in fields.iter().zip(expected) {
        match (field, *tag) {
            (Value::Symbol(symbol), "S") => {
                assert_eq!(engine.resolve_core_symbol(*symbol), Some(*text));
            }
            (Value::String(string), "T") => assert_eq!(string.as_bytes(), text.as_bytes()),
            (Value::Integer(integer), "I") => assert_eq!(*integer, text.parse::<i64>().unwrap()),
            _ => panic!("unexpected stored field {field:?}, expected {tag} {text:?}"),
        }
    }
    assert!(matches!(engine.get_global("trace"), Some(Value::Integer(value)) if *value == trace));
    assert!(matches!(
        engine.get_global("after"),
        Some(Value::Integer(999))
    ));
    assert_eq!(engine.get_output("t"), Some(output));
    assert_eq!(engine.run(RunLimit::Count(10)).unwrap().rules_fired, 0);
    assert_eq!(engine.get_output("t"), Some(output));
}

#[test]
fn direct_middle() {
    verify(
        r#"(defglobal ?*result* = (create$) ?*trace* = 0 ?*after* = 0)
(deffunction emit () (printout t "callable:" (bind ?*trace* (+ ?*trace* 1)) crlf))
(defrule probe => (bind ?*result* (create$ a (printout t "direct:" (bind ?*trace* (+ ?*trace* 1)) crlf) "b c")) (bind ?*after* 999))
"#,
        &[("S", "a"), ("T", "b c")],
        "direct:1\n",
        1,
    );
}

#[test]
fn callable_middle() {
    verify(
        r#"(defglobal ?*result* = (create$) ?*trace* = 0 ?*after* = 0)
(deffunction emit () (printout t "callable:" (bind ?*trace* (+ ?*trace* 1)) crlf))
(defrule probe => (bind ?*result* (create$ a (emit) "b c")) (bind ?*after* 999))
"#,
        &[("S", "a"), ("T", "b c")],
        "callable:1\n",
        1,
    );
}

#[test]
fn leading_trailing() {
    verify(
        r#"(defglobal ?*result* = (create$) ?*trace* = 0 ?*after* = 0)
(deffunction emit () (printout t "callable:" (bind ?*trace* (+ ?*trace* 1)) crlf))
(defrule probe => (bind ?*result* (create$ (printout t "direct:" (bind ?*trace* (+ ?*trace* 1)) crlf) a "b c" (emit))) (bind ?*after* 999))
"#,
        &[("S", "a"), ("T", "b c")],
        "direct:1\ncallable:2\n",
        2,
    );
}

#[test]
fn all_void() {
    verify(
        r#"(defglobal ?*result* = (create$) ?*trace* = 0 ?*after* = 0)
(deffunction emit () (printout t "callable:" (bind ?*trace* (+ ?*trace* 1)) crlf))
(defrule probe => (bind ?*result* (create$ (printout t "direct:" (bind ?*trace* (+ ?*trace* 1)) crlf) (emit))) (bind ?*after* 999))
"#,
        &[],
        "direct:1\ncallable:2\n",
        2,
    );
}

#[test]
fn nested_flattening() {
    verify(
        r#"(defglobal ?*result* = (create$) ?*trace* = 0 ?*after* = 0)
(deffunction emit () (printout t "callable:" (bind ?*trace* (+ ?*trace* 1)) crlf))
(defrule probe => (bind ?*result* (create$ before (create$ (printout t "direct:" (bind ?*trace* (+ ?*trace* 1)) crlf) "b c" (create$ (emit) 7)) (printout t "direct:" (bind ?*trace* (+ ?*trace* 1)) crlf) after)) (bind ?*after* 999))
"#,
        &[("S", "before"), ("T", "b c"), ("I", "7"), ("S", "after")],
        "direct:1\ncallable:2\ndirect:3\n",
        3,
    );
}

#[test]
fn void_vs_empty_string() {
    verify(
        r#"(defglobal ?*result* = (create$) ?*trace* = 0 ?*after* = 0)
(deffunction emit () (printout t "callable:" (bind ?*trace* (+ ?*trace* 1)) crlf))
(defrule probe => (bind ?*result* (create$ (emit) "" (create$ (printout t "direct:" (bind ?*trace* (+ ?*trace* 1)) crlf)) "")) (bind ?*after* 999))
"#,
        &[("T", ""), ("T", "")],
        "callable:1\ndirect:2\n",
        2,
    );
}
