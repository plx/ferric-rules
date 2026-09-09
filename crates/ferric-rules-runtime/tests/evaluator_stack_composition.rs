//! Cross-repair controls: typed multifield operations and scoped query callbacks
//! retain the existing sort error/return boundaries. These are composition tests,
//! not new reference-oracle programs.

use ferric_rules_core::Value;
use ferric_rules_runtime::{Engine, HaltReason, RunLimit};

fn failing_engine(body: &str) -> Engine {
    Engine::with_rules(&format!(
        "(defglobal ?*result* = pending ?*trace* = 0)
         (deffunction broken (?left ?right) (/ 1 0))
         (deftemplate item (slot value))
         (deffacts seed (item (value 7)))
         (defrule compute => {body} (assert (after)))"
    ))
    .unwrap()
}

fn assert_halted(engine: &mut Engine) {
    let result = engine.run(RunLimit::Count(10)).unwrap();
    assert_eq!(result.halt_reason, HaltReason::ActionError);
    assert_eq!(result.rules_fired, 1);
    assert!(!engine.action_diagnostics().is_empty());
    assert!(engine.find_facts("after").unwrap().is_empty());
}

#[test]
fn nth_and_member_preserve_their_distinct_operand_error_gates() {
    for alias in ["nth", "nth$"] {
        let mut engine = failing_engine(&format!(
            "(bind ?*result* ({alias} (length$ (sort broken 3 1))
               (create$ (bind ?*trace* 9))))"
        ));
        assert_halted(&mut engine);
        let Some(Value::Symbol(result)) = engine.get_global("result") else {
            panic!("nth must retain its actual nil error value")
        };
        assert_eq!(engine.resolve_core_symbol(*result), Some("nil"));
        assert!(matches!(
            engine.get_global("trace"),
            Some(Value::Integer(0))
        ));
    }
    for alias in ["member", "member$"] {
        let mut engine = failing_engine(&format!(
            "(bind ?*result* ({alias} (sort broken 3 1) (bind ?*trace* 9)))"
        ));
        assert_halted(&mut engine);
        let Some(Value::Symbol(result)) = engine.get_global("result") else {
            panic!("member must retain its actual FALSE error value")
        };
        assert_eq!(engine.resolve_core_symbol(*result), Some("FALSE"));
        assert!(matches!(
            engine.get_global("trace"),
            Some(Value::Integer(9))
        ));
        // The inherited Error gate suppresses a secondary haystack type error.
        assert_eq!(engine.action_diagnostics().len(), 1);
    }
}

#[test]
fn failed_query_predicates_stop_before_body_or_next_candidate() {
    for query in [
        "do-for-fact",
        "do-for-all-facts",
        "delayed-do-for-all-facts",
    ] {
        let mut engine = failing_engine(&format!(
            "({query} ((?item item)) (sort broken 3 1) (bind ?*trace* 9))"
        ));
        assert_halted(&mut engine);
        assert!(matches!(
            engine.get_global("trace"),
            Some(Value::Integer(0))
        ));
        assert_eq!(engine.action_diagnostics().len(), 1);
    }
    for query in ["any-factp", "find-fact", "find-all-facts"] {
        let mut engine = failing_engine(&format!(
            "(bind ?*result* ({query} ((?item item)) (sort broken 3 1)))"
        ));
        assert_halted(&mut engine);
        match engine.get_global("result") {
            Some(Value::Symbol(result)) if query == "any-factp" => {
                assert_eq!(engine.resolve_core_symbol(*result), Some("FALSE"));
            }
            Some(Value::Multifield(result)) if query != "any-factp" => assert!(result.is_empty()),
            other => panic!("{query}: unexpected failed query value {other:?}"),
        }
        assert_eq!(engine.action_diagnostics().len(), 1);
    }
}
