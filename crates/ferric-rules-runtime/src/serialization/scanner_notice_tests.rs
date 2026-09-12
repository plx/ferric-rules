//! Typed wire checks use each actual codec before `into_engine` can normalize
//! diagnostics. Prior diagnostic representations are constructed inside the
//! current envelope, deliberately bypassing the new From boundary.

use super::*;
use crate::evaluator::{EvalError, ScannerNoticeDiagnostic};
use crate::{HaltReason, RunLimit};

fn notice(channel: &str) -> EvalError {
    EvalError::ScannerNotice(Box::new(ScannerNoticeDiagnostic {
        function: "string-to-field".into(),
        code: "SCANNER1".into(),
        channel: channel.into(),
        message: "historical scanner notice".into(),
        offset: 5,
        span: None,
    }))
}

fn wire_diagnostics(bytes: &[u8], format: SerializationFormat) -> Vec<ActionError> {
    assert_eq!(&bytes[8..10], &SCHEMA_VERSION.to_le_bytes());
    let snapshot: EngineSnapshotOwned =
        decode(open_envelope(bytes, format).unwrap(), format).unwrap();
    snapshot.action_diagnostics
}

fn assert_string_wire(diagnostic: &ActionError, payload: &str, format: SerializationFormat) {
    assert!(matches!(diagnostic, ActionError::EvalError(message) if message == payload));
    // Exact typed encoding, not a text search for an enum name or warning code.
    let bytes = encode(diagnostic, format).unwrap();
    assert_eq!(
        bytes,
        encode(&ActionError::EvalError(payload.into()), format).unwrap()
    );
    assert!(
        matches!(decode::<ActionError>(&bytes, format).unwrap(), ActionError::EvalError(message) if message == payload)
    );
}

#[test]
fn every_codec_writes_scanner_notices_as_the_existing_action_string_variant() {
    const SOURCE: &str = r"
        (defglobal ?*result* = pending)
        (defrule scan (input ?text) =>
          (bind ?*result* (string-to-field ?text)) (assert (after)))
        (defrule follower (after) => (assert (followed)))
    ";
    for &format in SerializationFormat::ALL {
        for (input, channel, expected_output) in [
            (
                b"9223372036854775808".as_slice(),
                "wwarning",
                b"[SCANNER1] WARNING: Over or underflow of long long integer.\n".as_slice(),
            ),
            (
                b"\"unfinished\\".as_slice(),
                "werror",
                b"\n[SCANNER1] Encountered End-Of-File while scanning a string\n".as_slice(),
            ),
        ] {
            let mut engine = Engine::with_rules(SOURCE).unwrap();
            let value = engine.create_string_bytes(input).unwrap();
            engine.assert_ordered("input", [value]).unwrap();
            let pending = engine.serialize(format).unwrap();
            assert!(wire_diagnostics(&pending, format).is_empty());
            let mut resumed = Engine::deserialize(&pending, format).unwrap();
            let run = resumed.run(RunLimit::Unlimited).unwrap();
            assert_eq!(run.halt_reason, HaltReason::AgendaEmpty);
            assert_eq!(run.rules_fired, 2);
            assert_eq!(resumed.find_facts("followed").unwrap().len(), 1);
            assert_eq!(resumed.get_output_bytes(channel), Some(expected_output));
            assert!(!resumed.globals.evaluation_halted());
            assert!(!resumed.globals.evaluation_error());
            let [ActionError::EvalError(payload)] = resumed.action_diagnostics() else {
                panic!("scanner notices must be normalized before persistence")
            };
            let payload = payload.clone();
            let public_message = resumed.action_diagnostics()[0].to_string();
            let completed = resumed.serialize(format).unwrap();
            let wire = wire_diagnostics(&completed, format);
            assert_eq!(wire.len(), 1);
            assert_string_wire(&wire[0], &payload, format);
            let mut restored = Engine::deserialize(&completed, format).unwrap();
            assert_eq!(restored.action_diagnostics()[0].to_string(), public_message);
            assert_eq!(restored.get_output_bytes(channel), Some(expected_output));
            if channel == "werror" {
                let Some(Value::String(value)) = restored.get_global("result") else {
                    panic!("partial scanner STRING must retain its raw byte")
                };
                assert_eq!(value.as_bytes(), b"unfinished\xff");
            } else {
                assert!(matches!(
                    restored.get_global("result"),
                    Some(Value::Integer(i64::MAX))
                ));
            }
            let second_wire = wire_diagnostics(&restored.serialize(format).unwrap(), format);
            assert_string_wire(&second_wire[0], &payload, format);
            assert_eq!(restored.run(RunLimit::Unlimited).unwrap().rules_fired, 0);
            assert!(restored.action_diagnostics().is_empty());
            assert_eq!(restored.get_output_bytes(channel), Some(expected_output));
        }
    }
}

#[test]
fn every_codec_normalizes_historical_structured_notices_before_exposure_and_rewrite() {
    for &format in SerializationFormat::ALL {
        let mut historical = Engine::with_rules("(defrule ready => (assert (after)))").unwrap();
        let warning = notice("wwarning");
        let error_channel_notice = notice("werror");
        let expected_payloads = [warning.to_string(), error_channel_notice.to_string()];
        // Direct construction recreates the pre-fix schema-6 diagnostic vector,
        // bypassing the production From conversion. No fixture is regenerated.
        historical.action_diagnostics = vec![
            ActionError::Evaluator(warning),
            ActionError::Evaluator(EvalError::TypeError {
                function: "retained".into(),
                expected: "STRING".into(),
                actual: "INTEGER".into(),
                span: None,
            }),
            ActionError::EvalError("preexisting unstructured diagnostic".into()),
            ActionError::Evaluator(error_channel_notice),
        ];
        let expected_messages: Vec<_> = historical
            .action_diagnostics()
            .iter()
            .map(ToString::to_string)
            .collect();
        let unchanged_middle = encode(&&historical.action_diagnostics()[1..3], format).unwrap();
        historical.router.write("wwarning", b"historical warning\n");
        historical
            .router
            .write("werror", b"historical raw notice\0\xff\n");
        let bytes = historical.serialize(format).unwrap();
        let old_wire = wire_diagnostics(&bytes, format);
        assert!(matches!(
            &old_wire[0],
            ActionError::Evaluator(EvalError::ScannerNotice(_))
        ));
        assert!(matches!(
            &old_wire[3],
            ActionError::Evaluator(EvalError::ScannerNotice(_))
        ));
        let mut restored = Engine::deserialize(&bytes, format).unwrap();
        assert_eq!(
            restored
                .action_diagnostics()
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>(),
            expected_messages
        );
        assert_eq!(restored.action_diagnostics().len(), 4);
        assert_string_wire(
            &restored.action_diagnostics()[0],
            &expected_payloads[0],
            format,
        );
        assert_string_wire(
            &restored.action_diagnostics()[3],
            &expected_payloads[1],
            format,
        );
        assert_eq!(
            encode(&&restored.action_diagnostics()[1..3], format).unwrap(),
            unchanged_middle
        );
        assert!(
            matches!(&restored.action_diagnostics()[1], ActionError::Evaluator(EvalError::TypeError { function, .. }) if function == "retained")
        );
        assert!(!restored.globals.evaluation_halted());
        assert!(!restored.globals.evaluation_error());
        assert_eq!(
            restored.get_output_bytes("wwarning"),
            Some(b"historical warning\n".as_slice())
        );
        assert_eq!(
            restored.get_output_bytes("werror"),
            Some(b"historical raw notice\0\xff\n".as_slice())
        );
        assert!(restored.get_output("werror").is_err());
        let rewritten = restored.serialize(format).unwrap();
        let new_wire = wire_diagnostics(&rewritten, format);
        assert_string_wire(&new_wire[0], &expected_payloads[0], format);
        assert_string_wire(&new_wire[3], &expected_payloads[1], format);
        assert_eq!(encode(&&new_wire[1..3], format).unwrap(), unchanged_middle);
        let again = Engine::deserialize(&rewritten, format).unwrap();
        let again_wire = wire_diagnostics(&again.serialize(format).unwrap(), format);
        assert_eq!(
            encode(&again_wire, format).unwrap(),
            encode(&new_wire, format).unwrap(),
            "diagnostic normalization is idempotent"
        );
        let run = restored.run(RunLimit::Unlimited).unwrap();
        assert_eq!(run.halt_reason, HaltReason::AgendaEmpty);
        assert_eq!(run.rules_fired, 1);
        assert_eq!(restored.find_facts("after").unwrap().len(), 1);
        assert!(restored.action_diagnostics().is_empty());
        assert_eq!(
            restored.get_output_bytes("werror"),
            Some(b"historical raw notice\0\xff\n".as_slice())
        );
    }
}
