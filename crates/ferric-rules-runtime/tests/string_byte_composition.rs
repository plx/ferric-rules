//! Composition controls for issues 335–337 and the byte-lexeme foundation.
//!
//! Raw payload cases extend the public byte contract; they are not new CLIPS
//! oracle claims. Recovered-error controls combine the established sort flags
//! with strngfun.c's sequential `EnvArgTypeCheck` boundaries.

use ferric_rules_core::Value;
use ferric_rules_runtime::{Engine, HaltReason, HostValue, RunLimit};

fn lexeme(engine: &mut Engine, kind: &str, bytes: &[u8]) -> HostValue {
    match kind {
        "STRING" => engine.create_string_bytes(bytes).unwrap().into(),
        "SYMBOL" => engine.symbol_value_bytes(bytes).unwrap(),
        "INSTANCE-NAME" => engine.instance_name_value_bytes(bytes).unwrap(),
        _ => unreachable!("known kind"),
    }
}

#[test]
fn string_and_symbol_lengths_and_clipped_parts_share_whole_payload_position_units() {
    for kind in ["STRING", "SYMBOL"] {
        for (bytes, start, end, length, expected) in [
            ("éz".as_bytes(), 0, 1, 2, "é".as_bytes()),
            (b"\xc3\xa9\xff".as_slice(), 0, 2, 3, "é".as_bytes()),
            (b"\xc3\xa9\xff".as_slice(), 2, 3, 3, b"\xa9\xff".as_slice()),
            (
                b"a\0\xffz".as_slice(),
                2,
                i64::MAX,
                4,
                b"\0\xffz".as_slice(),
            ),
        ] {
            let mut engine = Engine::with_rules(&format!(
                "(defglobal ?*length* = 0 ?*part* = pending)
                 (defrule compute (input ?text) =>
                   (bind ?*length* (str-length ?text))
                   (bind ?*part* (sub-string {start} {end} ?text)))"
            ))
            .unwrap();
            let input = lexeme(&mut engine, kind, bytes);
            engine.assert_ordered("input", [input]).unwrap();
            assert_eq!(
                engine.run(RunLimit::Unlimited).unwrap().halt_reason,
                HaltReason::AgendaEmpty
            );
            assert!(engine.action_diagnostics().is_empty());
            assert!(
                matches!(engine.get_global("length"), Some(Value::Integer(actual)) if *actual == length)
            );
            let Some(Value::String(part)) = engine.get_global("part") else {
                panic!("sub-string must return STRING for {kind}")
            };
            assert_eq!(part.as_bytes(), expected, "{kind}: {bytes:?}");
        }
    }
}

#[test]
fn sequential_string_checks_preserve_error_defaults_without_treating_halt_alone_as_error() {
    for count in [2, 3] {
        for search in [false, true] {
            let arguments = if count == 2 { "3 1" } else { "3 1 2" };
            let substring = format!(
                "(bind ?*part* (sub-string (length$ (sort broken {arguments}))
                   (bind ?*end* 3) \"abcd\"))"
            );
            let expression = if search {
                format!("(str-index {substring} (bind ?*next* \"abcd\"))")
            } else {
                format!("(str-length {substring})")
            };
            let mut engine = Engine::with_rules(&format!(
                "(defglobal ?*result* = pending ?*part* = pending ?*end* = 0 ?*next* = 0)
                 (deffunction broken (?a ?b) (/ 1 0))
                 (defrule compute => (bind ?*result* {expression}) (assert (after)))"
            ))
            .unwrap();
            assert_eq!(
                engine.run(RunLimit::Unlimited).unwrap().halt_reason,
                HaltReason::ActionError
            );
            assert_eq!(engine.action_diagnostics().len(), 1);
            assert!(engine.action_diagnostics()[0]
                .to_string()
                .contains("division by zero"));
            assert!(engine.find_facts("after").unwrap().is_empty());
            let Some(Value::String(part)) = engine.get_global("part") else {
                panic!("the enclosing bind preserves the returned STRING")
            };
            assert_eq!(
                part.as_bytes(),
                if count == 2 {
                    b"".as_slice()
                } else {
                    b"c".as_slice()
                }
            );
            assert!(
                matches!(engine.get_global("end"), Some(Value::Integer(value)) if *value == if count == 2 { 0 } else { 3 })
            );
            if count == 2 && search {
                let Some(Value::Symbol(value)) = engine.get_global("result") else {
                    panic!("str-index's actual default is FALSE")
                };
                assert_eq!(engine.resolve_core_symbol(*value), Some("FALSE"));
            } else {
                let expected = if count == 2 {
                    -1
                } else if search {
                    3
                } else {
                    1
                };
                assert!(
                    matches!(engine.get_global("result"), Some(Value::Integer(value)) if *value == expected)
                );
            }
            if count == 3 && search {
                let Some(Value::String(next)) = engine.get_global("next") else {
                    panic!("a direct bind still executes with Halt alone")
                };
                assert_eq!(next.as_bytes(), b"abcd");
            } else {
                assert!(matches!(engine.get_global("next"), Some(Value::Integer(0))));
            }
        }
    }
}

#[cfg(feature = "serde")]
#[test]
fn every_codec_resumes_empty_needle_endpoints_in_text_and_raw_modes() {
    use ferric_rules_runtime::SerializationFormat;

    for kind in ["STRING", "SYMBOL", "INSTANCE-NAME"] {
        for (bytes, expected) in [("é".as_bytes(), 2), (b"\xc3\xa9\xff".as_slice(), 4)] {
            let mut pending = Engine::with_rules(
                "(defglobal ?*result* = pending)
                 (defrule compute (input ?needle ?text) =>
                   (bind ?*result* (str-index ?needle ?text)))",
            )
            .unwrap();
            let needle = lexeme(&mut pending, kind, b"");
            let text = lexeme(&mut pending, kind, bytes);
            pending.assert_ordered("input", [needle, text]).unwrap();
            for &format in SerializationFormat::ALL {
                let mut restored =
                    Engine::deserialize(&pending.serialize(format).unwrap(), format).unwrap();
                assert_eq!(restored.run(RunLimit::Unlimited).unwrap().rules_fired, 1);
                assert!(restored.action_diagnostics().is_empty());
                assert!(
                    matches!(restored.get_global("result"), Some(Value::Integer(value)) if *value == expected)
                );
                let mut completed =
                    Engine::deserialize(&restored.serialize(format).unwrap(), format).unwrap();
                assert_eq!(completed.run(RunLimit::Unlimited).unwrap().rules_fired, 0);
                assert!(
                    matches!(completed.get_global("result"), Some(Value::Integer(value)) if *value == expected)
                );
            }
        }
    }
}
