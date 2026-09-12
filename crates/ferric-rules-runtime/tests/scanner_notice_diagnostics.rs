//! Scanner notices retain their public message and nonfatal behavior while
//! crossing into the existing string action-diagnostic representation.

use std::error::Error;

use ferric_rules_runtime::evaluator::{EvalError, ScannerNoticeDiagnostic};
use ferric_rules_runtime::{ActionError, Engine, EngineConfig, HaltReason, RunLimit};

const OVERFLOW: &[u8] = b"[SCANNER1] WARNING: Over or underflow of long long integer.\n";

#[test]
fn scanner_conversion_preserves_display_in_existing_string_variant() {
    for channel in ["wwarning", "werror"] {
        let error = EvalError::ScannerNotice(Box::new(ScannerNoticeDiagnostic {
            function: "string-to-field".into(),
            code: "SCANNER1".into(),
            channel: channel.into(),
            message: "notice with exact punctuation: [] and Unicode é".into(),
            offset: 7,
            span: None,
        }));
        let expected_payload = error.to_string();
        let expected_display = ActionError::Evaluator(error.clone()).to_string();
        let converted = ActionError::from(error);
        assert!(
            matches!(&converted, ActionError::EvalError(message) if message == &expected_payload)
        );
        assert_eq!(converted.to_string(), expected_display);
    }
}

#[test]
fn other_evaluator_diagnostics_keep_their_typed_payload_and_error_source() {
    for error in [
        EvalError::UnknownFunction {
            name: "missing".into(),
            span: None,
        },
        EvalError::TypeError {
            function: "string-to-field".into(),
            expected: "lexeme".into(),
            actual: "INTEGER".into(),
            span: None,
        },
        EvalError::DivisionByZero {
            function: "/".into(),
            span: None,
        },
    ] {
        let expected_debug = format!("{error:?}");
        let expected_source = error.to_string();
        let expected_display = ActionError::Evaluator(error.clone()).to_string();
        let converted = ActionError::from(error);
        let ActionError::Evaluator(retained) = &converted else {
            panic!("non-scanner diagnostic must remain typed")
        };
        assert_eq!(format!("{retained:?}"), expected_debug);
        assert_eq!(converted.to_string(), expected_display);
        assert_eq!(converted.source().unwrap().to_string(), expected_source);
    }
}

fn assert_notice(engine: &Engine) {
    let [ActionError::EvalError(message)] = engine.action_diagnostics() else {
        panic!(
            "scanner notice must use the existing string diagnostic variant: {:?}",
            engine.action_diagnostics()
        )
    };
    assert!(message.contains("SCANNER1"));
    assert!(message.contains("string-to-field"));
    assert_eq!(engine.get_output_bytes("wwarning"), Some(OVERFLOW));
}

#[test]
fn load_and_matching_drains_project_scanner_notices_without_losing_router_bytes() {
    let mut loaded = Engine::new(EngineConfig::utf8());
    loaded
        .load_str(r#"(defglobal ?*result* = (string-to-field "9223372036854775808"))"#)
        .unwrap();
    assert_notice(&loaded);

    let mut matched = Engine::with_rules(
        r"(defrule match (input ?text)
          (test (= (string-to-field ?text) 9223372036854775807)) => (assert (after)))",
    )
    .unwrap();
    let value = matched.create_string_bytes(b"9223372036854775808").unwrap();
    matched.assert_ordered("input", [value]).unwrap();
    assert_notice(&matched);
    assert_eq!(matched.agenda_len(), 1);
    let run = matched.run(RunLimit::Unlimited).unwrap();
    assert_eq!(run.halt_reason, HaltReason::AgendaEmpty);
    assert_eq!(matched.find_facts("after").unwrap().len(), 1);
}

#[test]
fn action_step_and_continued_run_keep_notices_nonfatal_and_do_not_duplicate_events() {
    const SOURCE: &str = r#"
      (defglobal ?*result* = pending)
      (defrule scan =>
        (bind ?*result* (string-to-field "9223372036854775808"))
        (assert (after)))
      (defrule follower (after) => (assert (followed)))
    "#;
    let mut stepped = Engine::with_rules(SOURCE).unwrap();
    assert!(stepped.step().unwrap().is_some());
    assert_notice(&stepped);
    assert_eq!(stepped.find_facts("after").unwrap().len(), 1);
    assert!(stepped.step().unwrap().is_some());
    assert_eq!(stepped.find_facts("followed").unwrap().len(), 1);
    assert!(stepped.action_diagnostics().is_empty());
    assert_eq!(stepped.get_output_bytes("wwarning"), Some(OVERFLOW));

    let mut continued = Engine::with_rules(SOURCE).unwrap();
    assert_eq!(
        continued.run(RunLimit::Count(1)).unwrap().halt_reason,
        HaltReason::LimitReached
    );
    assert_notice(&continued);
    assert_eq!(
        continued
            .continue_run(RunLimit::Unlimited)
            .unwrap()
            .halt_reason,
        HaltReason::AgendaEmpty
    );
    assert_notice(&continued);
    assert_eq!(continued.find_facts("followed").unwrap().len(), 1);
}
