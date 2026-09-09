//! #346 public value/diagnostic integration, complementary to the fixture and
//! snapshot matrix. Scanner probes are CLIPS 6.30; strict encoding and framed
//! preexisting-Halt behavior are explicitly Ferric policies. No test setters.

use ferric_rules_core::Value;
use ferric_rules_runtime::{Engine, EngineConfig, HaltReason, HostValue, RunLimit};

const READ_ERROR: &[u8] = b"*** READ ERROR ***";

fn load(engine: &mut Engine, source: &str) {
    engine
        .load_str(source)
        .unwrap_or_else(|errors| panic!("{errors:?}"));
}

fn engine(config: EngineConfig, body: &str) -> Engine {
    let mut engine = Engine::new(config);
    load(
        &mut engine,
        &format!(
            r"
        (defglobal ?*result* = pending ?*next* = pending ?*trace* = 0)
        (deffunction mark (?value) (bind ?*trace* (+ ?*trace* 1)) ?value)
        (defrule capture (channel ?name) => {body} (assert (after)))
    "
        ),
    );
    engine.reset().unwrap();
    engine
}

fn with_stdin(config: EngineConfig, expression: &str) -> Engine {
    let mut engine = engine(config, &format!("(bind ?*result* {expression})"));
    let stdin = engine.symbol_value("stdin").unwrap();
    engine.assert_ordered("channel", [stdin]).unwrap();
    engine
}

fn assert_string(engine: &Engine, name: &str, bytes: &[u8]) {
    let Some(Value::String(value)) = engine.get_global(name) else {
        panic!(
            "{name} must contain an actual STRING: {:?}",
            engine.get_global(name)
        );
    };
    assert_eq!(value.as_bytes(), bytes);
}

fn run_success(engine: &mut Engine) {
    let run = engine.run(RunLimit::Count(20)).unwrap();
    assert_eq!(run.rules_fired, 1);
    assert_eq!(run.halt_reason, HaltReason::AgendaEmpty);
    assert_eq!(engine.find_facts("after").unwrap().len(), 1);
    assert!(engine.action_diagnostics().is_empty());
}

fn run_failure(engine: &mut Engine) {
    let run = engine.run(RunLimit::Count(20)).unwrap();
    assert_eq!(run.rules_fired, 1);
    assert_eq!(run.halt_reason, HaltReason::ActionError);
    assert!(engine.find_facts("after").unwrap().is_empty());
    assert_string(engine, "result", READ_ERROR);
}

fn next_read(engine: &mut Engine, expected: &[u8]) {
    // A new activation proves the prior public boundary drained error state.
    // Do not reset or push replacement input before checking the unread frame.
    load(engine, "(defrule next-read => (bind ?*next* (read)))");
    let run = engine.run(RunLimit::Count(20)).unwrap();
    assert_eq!(run.rules_fired, 1);
    assert_eq!(run.halt_reason, HaltReason::AgendaEmpty);
    assert_string(engine, "next", expected);
    assert!(engine.action_diagnostics().is_empty());
}

#[test]
fn quoted_empty_line_and_eof_are_distinct_public_value_types() {
    let mut engine = with_stdin(EngineConfig::utf8(), "(read)");
    engine.push_input("\"two words\" ignored");
    engine.push_input("");
    run_success(&mut engine);
    assert_string(&engine, "result", b"two words");
    load(&mut engine, "(defrule line => (bind ?*next* (readline)))");
    assert_eq!(engine.run(RunLimit::Count(20)).unwrap().rules_fired, 1);
    assert_string(&engine, "next", b"");
    load(&mut engine, "(defrule eof => (bind ?*next* (read)))");
    assert_eq!(engine.run(RunLimit::Count(20)).unwrap().rules_fired, 1);
    let Some(Value::Symbol(symbol)) = engine.get_global("next") else {
        panic!("exhaustion must produce SYMBOL EOF");
    };
    assert_eq!(
        engine.resolve_core_symbol_bytes(*symbol),
        Some(b"EOF".as_slice())
    );
    assert!(engine.action_diagnostics().is_empty());
}

#[test]
fn dynamically_typed_stdin_names_are_evaluated_once_without_object_lookup() {
    // GetLogicalName admits names and uses first-NUL comparison. These host
    // value cases extend the sealed text aliases without a raw-input API.
    for (kind, bytes) in [
        (0, b"t".as_slice()),
        (0, b"T"),
        (1, b"stdin"),
        (1, b"t\0ignored"),
        (2, b"stdin"),
        (2, b"T\0ignored"),
    ] {
        for function in ["read", "readline"] {
            let mut engine = engine(
                EngineConfig::utf8(),
                &format!("(bind ?*result* ({function} (mark ?name)))"),
            );
            let name: HostValue = match kind {
                0 => engine.create_string_bytes(bytes).unwrap().into(),
                1 => engine.symbol_value_bytes(bytes).unwrap(),
                _ => engine.instance_name_value_bytes(bytes).unwrap(),
            };
            engine.assert_ordered("channel", [name]).unwrap();
            engine.push_input(if function == "read" {
                "\"input\" tail"
            } else {
                "input"
            });
            run_success(&mut engine);
            assert_string(&engine, "result", b"input");
            assert!(matches!(
                engine.get_global("trace"),
                Some(Value::Integer(1))
            ));
        }
    }
}

#[test]
fn an_existing_output_channel_is_not_a_registered_input_source() {
    for function in ["read", "readline"] {
        let mut engine = engine(
            EngineConfig::utf8(),
            &format!(
                r#"
            (printout log "existing")
            (bind ?*result* ({function} (mark ?name)))
        "#
            ),
        );
        let log = engine.symbol_value("log").unwrap();
        engine.assert_ordered("channel", [log]).unwrap();
        engine.push_input("\"still queued\"");
        run_failure(&mut engine);
        assert_eq!(engine.get_output_bytes("log"), Some(b"existing".as_slice()));
        assert!(matches!(
            engine.get_global("trace"),
            Some(Value::Integer(1))
        ));
        assert_eq!(engine.action_diagnostics().len(), 1);
        assert!(engine.action_diagnostics()[0].to_string().contains("input"));
        next_read(&mut engine, b"still queued");
    }
}

#[test]
fn configured_encoding_is_checked_after_the_selected_line_is_consumed() {
    // This is Ferric's existing encoding policy, not a CLIPS differential
    // assertion. Quoted UTF8 remains allowed with ASCII-only symbol names.
    for (config, expression, input) in [
        (EngineConfig::ascii(), "(read)", "\"é\""),
        (EngineConfig::ascii(), "(readline)", "é"),
        (EngineConfig::ascii_symbols_utf8_strings(), "(read)", "é"),
        (EngineConfig::ascii_symbols_utf8_strings(), "(read)", "[é]"),
    ] {
        let mut engine = with_stdin(config, expression);
        engine.push_input(input);
        engine.push_input("\"remaining\"");
        run_failure(&mut engine);
        assert_eq!(engine.action_diagnostics().len(), 1);
        assert!(engine.action_diagnostics()[0]
            .to_string()
            .contains("encoding"));
        next_read(&mut engine, b"remaining");
    }
    let mut engine = with_stdin(EngineConfig::ascii_symbols_utf8_strings(), "(read)");
    engine.push_input("\"é\"");
    run_success(&mut engine);
    assert_string(&engine, "result", "é".as_bytes());
}

#[test]
fn an_inherited_halt_preserves_frames_and_adds_no_read_diagnostic() {
    // Two sort fields leave Error+Halt, so create$ stops before the read.
    // With three fields, a later skipped callback clears Error but retains
    // Halt: the read is reached and returns READ ERROR without consuming a
    // frame. Direct private tests also cover both flags at the read boundary.
    for values in ["3 1", "3 1 2"] {
        for function in ["read", "readline"] {
            let mut engine = engine(
                EngineConfig::utf8(),
                &format!(
                    r"
                (create$ (sort broken {values})
                  (bind ?*result* ({function} ?name)))
            "
                ),
            );
            load(&mut engine, "(deffunction broken (?left ?right) (/ 1 0))");
            let stdin = engine.symbol_value("stdin").unwrap();
            engine.assert_ordered("channel", [stdin]).unwrap();
            engine.push_input("\"not consumed\"");
            if values == "3 1" {
                let run = engine.run(RunLimit::Count(20)).unwrap();
                assert_eq!(run.rules_fired, 1);
                assert_eq!(run.halt_reason, HaltReason::ActionError);
                assert!(engine.find_facts("after").unwrap().is_empty());
                assert!(
                    matches!(engine.get_global("result"), Some(Value::Symbol(value))
                    if engine.resolve_core_symbol(*value) == Some("pending"))
                );
            } else {
                run_failure(&mut engine);
            }
            assert_eq!(engine.action_diagnostics().len(), 1);
            let diagnostic = engine.action_diagnostics()[0].to_string();
            assert!(diagnostic.contains("zero"), "{diagnostic}");
            assert!(!diagnostic.contains("read"), "{diagnostic}");
            next_read(&mut engine, b"not consumed");
        }
    }
}

#[test]
fn stdin_quoted_nul_reports_the_c_string_boundary_and_discards_the_frame() {
    // CLIPS 6.30 ReadTokenFromStdin uses OpenStringSource, not the named
    // router's direct GetToken path. Its first NUL ends the scanner input,
    // even when the already-consumed physical line contains a closing quote.
    let mut engine = engine(
        EngineConfig::utf8(),
        "(bind ?*result* (read)) (bind ?*next* (readline))",
    );
    let stdin = engine.symbol_value("stdin").unwrap();
    engine.assert_ordered("channel", [stdin]).unwrap();
    engine.push_input("\"ab\0ignored\" tail");
    engine.push_input("second frame");

    let run = engine.run(RunLimit::Count(20)).unwrap();
    assert_eq!(run.rules_fired, 1);
    assert_eq!(run.halt_reason, HaltReason::AgendaEmpty);
    assert_string(&engine, "result", b"ab");
    assert_string(&engine, "next", b"second frame");
    assert_eq!(engine.find_facts("after").unwrap().len(), 1);
    assert_eq!(engine.action_diagnostics().len(), 1);
    let diagnostic = engine.action_diagnostics()[0].to_string();
    assert!(diagnostic.contains("SCANNER1"), "{diagnostic}");
    assert!(diagnostic.contains("read"), "{diagnostic}");
    assert_eq!(
        engine.get_output_bytes("werror"),
        Some(b"\n[SCANNER1] Encountered End-Of-File while scanning a string\n".as_slice())
    );
    assert!(engine
        .get_output_bytes("wwarning")
        .unwrap_or_default()
        .is_empty());
}
