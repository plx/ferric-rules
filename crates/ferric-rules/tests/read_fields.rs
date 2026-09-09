//! #346: CLIPS stdin field scanning through the existing queued-line API.
//!
//! All 30 reference triples are preserved. This harness executes 23 stdin
//! programs; router failures and load arity have separate controls below.
//! Named-file programs require a real input registry and `open` implementation.
//! Original CLIPS 6.30 receipts use digest
//! `sha256:b4b99ba2f08102f9c18743c6adaecb5ab82789ddfa8628693cabfeced461a929`.
//! No normal/late or snapshot replay was run while preparing this draft.

use ferric_rules::core::Value;
use ferric_rules::runtime::{Engine, EngineConfig, HaltReason, RunLimit};

struct Fixture {
    name: &'static str,
    source: &'static str,
    input: &'static [u8],
    output: &'static [u8],
}

macro_rules! fixture {
    ($name:literal) => {
        Fixture {
            name: $name,
            source: include_str!(concat!("fixtures/read/", $name, ".clp")),
            input: include_bytes!(concat!("fixtures/read/", $name, ".in")),
            output: include_bytes!(concat!("fixtures/read/", $name, ".out")),
        }
    };
}

const FIXTURES: &[Fixture] = &[
    fixture!("read-string"),
    fixture!("read-integer"),
    fixture!("read-symbol"),
    fixture!("read-eof"),
    fixture!("read-two-tokens"),
    fixture!("readline-whitespace"),
    fixture!("readline-eof"),
    fixture!("quoted-strings-and-escapes"),
    fixture!("numeric-token-types"),
    fixture!("numeric-looking-symbols"),
    fixture!("symbol-delimiters-and-special-tokens"),
    fixture!("blank-comment-lines-skip"),
    fixture!("physical-lines-and-readline"),
    fixture!("crlf-readline-state"),
    fixture!("no-final-newline"),
    fixture!("valid-first-ignores-malformed-tail"),
    fixture!("vertical-tab-and-formfeed"),
    fixture!("integer-overflow"),
    fixture!("unterminated-quote-per-line"),
    fixture!("unterminated-terminal-backslash"),
    fixture!("instance-name-token"),
    fixture!("explicit-stdin-channel"),
    fixture!("callable-and-nested-read"),
];

fn load(engine: &mut Engine, source: &str) {
    engine
        .load_str(source)
        .unwrap_or_else(|errors| panic!("{errors:?}"));
}

/// Fixture framing only: one CR OR LF ends one stdin physical line. Preserve
/// the empty LF frame after CR in CRLF. Do not fabricate an EOF trailing frame.
/// The public `push_input` API still takes one already-framed UTF8 line.
fn push_physical_lines(engine: &mut Engine, input: &[u8]) {
    let mut start = 0;
    for (offset, &byte) in input.iter().enumerate() {
        if matches!(byte, b'\r' | b'\n') {
            engine.push_input(std::str::from_utf8(&input[start..offset]).unwrap());
            start = offset + 1;
        }
    }
    if start < input.len() {
        engine.push_input(std::str::from_utf8(&input[start..]).unwrap());
    }
}

fn pending(fixture: &Fixture, late: bool) -> Engine {
    let mut engine = Engine::new(EngineConfig::utf8());
    if late {
        let (prefix, rules) = fixture.source.split_once("(defrule").unwrap();
        load(&mut engine, prefix);
        engine.reset().unwrap();
        push_physical_lines(&mut engine, fixture.input);
        load(&mut engine, &format!("(defrule{rules}"));
    } else {
        load(&mut engine, fixture.source);
        engine.reset().unwrap();
        push_physical_lines(&mut engine, fixture.input);
    }
    engine
}

const INTEGER_NOTICE: &[u8] = b"[SCANNER1] WARNING: Over or underflow of long long integer.\n";
const STRING_NOTICE: &[u8] = b"\n[SCANNER1] Encountered End-Of-File while scanning a string\n";

/// Reference stdout merges three routers. Separate only the exact known notice
/// records to compare the public per-channel buffers; retain the golden bytes.
/// This does not claim that Engine's independent buffers expose cross-channel
/// chronological ordering.
fn reference_channels(golden: &[u8]) -> (Vec<u8>, Vec<u8>, Vec<u8>, usize) {
    let mut text = Vec::new();
    let mut warning = Vec::new();
    let mut error = Vec::new();
    let mut count = 0;
    let mut offset = 0;
    while offset < golden.len() {
        if golden[offset..].starts_with(INTEGER_NOTICE) {
            warning.extend_from_slice(INTEGER_NOTICE);
            offset += INTEGER_NOTICE.len();
            count += 1;
        } else if golden[offset..].starts_with(STRING_NOTICE) {
            error.extend_from_slice(STRING_NOTICE);
            offset += STRING_NOTICE.len();
            count += 1;
        } else {
            text.push(golden[offset]);
            offset += 1;
        }
    }
    (text, warning, error, count)
}

fn check_fixture_output(engine: &Engine, fixture: &Fixture) {
    let (text, warning, error, _) = reference_channels(fixture.output);
    assert_eq!(
        engine.get_output_bytes("t").unwrap_or(b""),
        text,
        "{}",
        fixture.name
    );
    assert_eq!(
        engine.get_output_bytes("wwarning").unwrap_or(b""),
        warning,
        "{}",
        fixture.name
    );
    assert_eq!(
        engine.get_output_bytes("werror").unwrap_or(b""),
        error,
        "{}",
        fixture.name
    );
    if std::str::from_utf8(&text).is_err() {
        assert!(engine.get_output("t").is_err());
    }
}

fn check_fixture(engine: &Engine, fixture: &Fixture) {
    check_fixture_output(engine, fixture);
    let (_, _, _, count) = reference_channels(fixture.output);
    assert_eq!(engine.action_diagnostics().len(), count, "{}", fixture.name);
    for diagnostic in engine.action_diagnostics() {
        assert!(
            diagnostic.to_string().contains("SCANNER1"),
            "{}: {diagnostic}",
            fixture.name
        );
    }
}

fn assert_no_refiring(engine: &mut Engine, fixture: &Fixture) {
    assert_eq!(engine.run(RunLimit::Count(100)).unwrap().rules_fired, 0);
    // Each new run clears prior action diagnostics, while router bytes remain.
    assert!(engine.action_diagnostics().is_empty());
    check_fixture_output(engine, fixture);
}

fn run_fixture(engine: &mut Engine, fixture: &Fixture) {
    let run = engine.run(RunLimit::Count(100)).unwrap();
    assert_eq!(run.halt_reason, HaltReason::AgendaEmpty, "{}", fixture.name);
    assert_eq!(run.rules_fired, 1, "{}", fixture.name);
    check_fixture(engine, fixture);
}

#[test]
fn preserved_stdin_triples_match_types_and_exact_router_bytes() {
    for fixture in FIXTURES {
        let mut engine = pending(fixture, false);
        run_fixture(&mut engine, fixture);
        assert_no_refiring(&mut engine, fixture);
    }
}

#[test]
fn queued_input_survives_late_rule_installation() {
    for fixture in FIXTURES {
        let mut engine = pending(fixture, true);
        run_fixture(&mut engine, fixture);
        assert_no_refiring(&mut engine, fixture);
    }
}

#[cfg(feature = "serde")]
#[test]
fn pending_completed_and_late_queues_round_trip_all_five_codecs() {
    use ferric_rules::runtime::SerializationFormat;
    for fixture in FIXTURES {
        for &format in SerializationFormat::ALL {
            let engine = pending(fixture, false);
            let mut restored =
                Engine::deserialize(&engine.serialize(format).unwrap(), format).unwrap();
            run_fixture(&mut restored, fixture);
            let mut completed =
                Engine::deserialize(&restored.serialize(format).unwrap(), format).unwrap();
            check_fixture(&completed, fixture);
            assert_no_refiring(&mut completed, fixture);

            // Serialize before installing the rule, while the original queue is
            // unread. No extra reset after installing the saved rule suffix.
            let (prefix, rules) = fixture.source.split_once("(defrule").unwrap();
            let mut before = Engine::new(EngineConfig::utf8());
            load(&mut before, prefix);
            before.reset().unwrap();
            push_physical_lines(&mut before, fixture.input);
            let mut late = Engine::deserialize(&before.serialize(format).unwrap(), format).unwrap();
            load(&mut late, &format!("(defrule{rules}"));
            run_fixture(&mut late, fixture);
            assert_no_refiring(&mut late, fixture);
        }
    }
}

#[derive(Debug)]
enum Expected<'a> {
    Integer(i64),
    Float(f64),
    String(&'a [u8]),
    Symbol(&'a [u8]),
    Name(&'a [u8]),
}

fn expect_value(engine: &Engine, value: &Value, expected: &Expected<'_>) {
    match (value, expected) {
        (Value::Integer(actual), Expected::Integer(expected)) => assert_eq!(actual, expected),
        (Value::Float(actual), Expected::Float(expected)) => {
            assert_eq!(actual.to_bits(), expected.to_bits());
        }
        (Value::String(actual), Expected::String(expected)) => {
            assert_eq!(actual.as_bytes(), *expected);
        }
        (Value::Symbol(actual), Expected::Symbol(expected)) => {
            assert_eq!(engine.resolve_core_symbol_bytes(*actual), Some(*expected));
        }
        (Value::InstanceName(actual), Expected::Name(expected)) => assert_eq!(
            engine.resolve_core_symbol_bytes(actual.as_symbol()),
            Some(*expected)
        ),
        _ => panic!("expected {expected:?}, got {value:?}"),
    }
}

fn capture_engine(expression: &str, input: &[u8]) -> Engine {
    let mut engine = Engine::new(EngineConfig::utf8());
    load(&mut engine, &format!("(defglobal ?*value* = pending ?*after* = 0) (defrule capture => (bind ?*value* {expression}) (bind ?*after* 1))"));
    engine.reset().unwrap();
    push_physical_lines(&mut engine, input);
    engine
}

fn captured(engine: &Engine) -> &Value {
    engine.get_global("value").unwrap()
}

#[test]
fn scanned_values_retain_exact_variants_bytes_and_signed_zero() {
    for (input, expected) in [
        (
            b"\"two words\" tail\n".as_slice(),
            Expected::String(b"two words"),
        ),
        (b"\"\"\n", Expected::String(b"")),
        (b"\"a\\n\\q\\\"\\\\\"\n", Expected::String(b"anq\"\\")),
        (b"9223372036854775807\n", Expected::Integer(i64::MAX)),
        (b"-9223372036854775808\n", Expected::Integer(i64::MIN)),
        (b"2e3\n", Expected::Float(2000.0)),
        (b"-0.0\n", Expected::Float(-0.0)),
        (b"NaN\n", Expected::Symbol(b"NaN")),
        (b"42abc\n", Expected::Symbol(b"42abc")),
        (b"[widget]\n", Expected::Name(b"widget")),
        (b"?x\n", Expected::String(b"?x")),
        (b"(\n", Expected::String(b"(")),
        (b"\x0bignored\n", Expected::String(b"*** READ ERROR ***")),
        (b"", Expected::Symbol(b"EOF")),
    ] {
        let mut engine = capture_engine("(read)", input);
        assert_eq!(
            engine.run(RunLimit::Count(10)).unwrap().halt_reason,
            HaltReason::AgendaEmpty
        );
        expect_value(&engine, captured(&engine), &expected);
        assert!(engine.action_diagnostics().is_empty());
        assert!(matches!(
            engine.get_global("after"),
            Some(Value::Integer(1))
        ));
    }
}

#[test]
fn scanner_notices_keep_the_returned_value_and_continue() {
    for (input, expected, channel, notice) in [
        (
            b"9223372036854775808\n".as_slice(),
            Expected::Integer(i64::MAX),
            "wwarning",
            INTEGER_NOTICE,
        ),
        (
            b"-9223372036854775809\n",
            Expected::Integer(i64::MIN),
            "wwarning",
            INTEGER_NOTICE,
        ),
        (
            b"\"two\n",
            Expected::String(b"two"),
            "werror",
            STRING_NOTICE,
        ),
        (
            b"\"unfinished\\",
            Expected::String(b"unfinished\xff"),
            "werror",
            STRING_NOTICE,
        ),
    ] {
        let mut engine = capture_engine("(read)", input);
        assert_eq!(
            engine.run(RunLimit::Count(10)).unwrap().halt_reason,
            HaltReason::AgendaEmpty
        );
        expect_value(&engine, captured(&engine), &expected);
        assert_eq!(engine.get_output_bytes(channel), Some(notice));
        assert_eq!(engine.action_diagnostics().len(), 1);
        assert!(engine.action_diagnostics()[0]
            .to_string()
            .contains("SCANNER1"));
        assert!(matches!(
            engine.get_global("after"),
            Some(Value::Integer(1))
        ));
    }
}

const CONSUMER: &str =
    "(defglobal ?*value* = pending) (defrule consume (ticket ?id) => (bind ?*value* (read)))";

fn next_capture(engine: &mut Engine, ticket: i64, expected: &Expected<'_>) {
    engine.assert_ordered("ticket", [ticket]).unwrap();
    assert_eq!(engine.run(RunLimit::Count(10)).unwrap().rules_fired, 1);
    expect_value(engine, captured(engine), expected);
    assert!(engine.action_diagnostics().is_empty());
}

#[test]
fn partial_consumption_and_reset_clear_keep_the_line_queue_contract() {
    let mut engine = Engine::new(EngineConfig::utf8());
    load(&mut engine, CONSUMER);
    engine.reset().unwrap();
    push_physical_lines(
        &mut engine,
        b"\n;comment\n\"first value\" ignored\n42\n[widget]\n",
    );
    next_capture(&mut engine, 1, &Expected::String(b"first value"));
    next_capture(&mut engine, 2, &Expected::Integer(42));
    next_capture(&mut engine, 3, &Expected::Name(b"widget"));
    next_capture(&mut engine, 4, &Expected::Symbol(b"EOF"));
    engine.push_input("\"survives reset\"");
    engine.reset().unwrap();
    next_capture(&mut engine, 5, &Expected::String(b"survives reset"));
    engine.push_input("discarded by clear");
    engine.clear();
    load(&mut engine, CONSUMER);
    engine.reset().unwrap();
    next_capture(&mut engine, 6, &Expected::Symbol(b"EOF"));
}

#[cfg(feature = "serde")]
#[test]
fn partial_consumption_restore_new_input_and_reset_clear_keep_queue_contract() {
    use ferric_rules::runtime::SerializationFormat;
    for &format in SerializationFormat::ALL {
        let mut engine = Engine::new(EngineConfig::utf8());
        load(&mut engine, CONSUMER);
        engine.reset().unwrap();
        push_physical_lines(
            &mut engine,
            b"\n;comment\n\"first value\" ignored\n42\n[widget]\n",
        );
        next_capture(&mut engine, 1, &Expected::String(b"first value"));
        let mut restored = Engine::deserialize(&engine.serialize(format).unwrap(), format).unwrap();
        assert_eq!(restored.run(RunLimit::Count(10)).unwrap().rules_fired, 0);
        next_capture(&mut restored, 2, &Expected::Integer(42));
        let mut middle = Engine::deserialize(&restored.serialize(format).unwrap(), format).unwrap();
        next_capture(&mut middle, 3, &Expected::Name(b"widget"));
        next_capture(&mut middle, 4, &Expected::Symbol(b"EOF"));
        let mut completed =
            Engine::deserialize(&middle.serialize(format).unwrap(), format).unwrap();
        completed.push_input("\"new value\"");
        next_capture(&mut completed, 5, &Expected::String(b"new value"));
        completed.push_input("\"survives reset\"");
        completed.reset().unwrap();
        next_capture(&mut completed, 6, &Expected::String(b"survives reset"));
        completed.push_input("discarded by clear");
        completed.clear();
        load(&mut completed, CONSUMER);
        completed.reset().unwrap();
        next_capture(&mut completed, 7, &Expected::Symbol(b"EOF"));
    }
}

#[test]
fn router_errors_capture_read_error_and_do_not_consume_input() {
    // The preserved reference triples retain exact reference diagnostics. Public
    // diagnostics here assert semantic error/default/continuation and ordering;
    // Ferric's enclosing rule/callable message text is not declared byte-equal.
    for (source, child_error) in [
        (include_str!("fixtures/read/unknown-router.clp"), false),
        (include_str!("fixtures/read/invalid-router-type.clp"), false),
        (include_str!("fixtures/read/numeric-router.clp"), false),
        (
            include_str!("fixtures/read/channel-expression-error.clp"),
            true,
        ),
    ] {
        let mut engine = Engine::new(EngineConfig::utf8());
        load(&mut engine, source);
        engine.reset().unwrap();
        push_physical_lines(&mut engine, b"\"input remains\"\n");
        let result = engine.run(RunLimit::Count(10)).unwrap();
        assert_eq!(result.halt_reason, HaltReason::ActionError);
        assert_eq!(result.rules_fired, 1);
        expect_value(
            &engine,
            engine.get_global("result").unwrap(),
            &Expected::String(b"*** READ ERROR ***"),
        );
        assert!(matches!(
            engine.get_global("trace"),
            Some(Value::Integer(1))
        ));
        assert_eq!(engine.get_output_bytes("t").unwrap_or(b""), b"");
        let messages: Vec<_> = engine
            .action_diagnostics()
            .iter()
            .map(ToString::to_string)
            .collect();
        let read_position = messages
            .iter()
            .position(|message| {
                message.contains("read")
                    || message.contains("router")
                    || message.contains("logical name")
            })
            .expect("router diagnosis");
        if child_error {
            let division = messages
                .iter()
                .position(|message| message.contains("zero"))
                .expect("original division diagnosis");
            assert!(division < read_position);
        }
        // A new rule executes after the boundary clears transient Error/Halt.
        // No reset may erase/refill the queue or create an apparent preservation.
        engine.clear_action_diagnostics();
        load(
            &mut engine,
            "(defglobal ?*remaining* = pending) (defrule remaining => (bind ?*remaining* (read)))",
        );
        assert_eq!(
            engine.run(RunLimit::Count(10)).unwrap().halt_reason,
            HaltReason::AgendaEmpty
        );
        expect_value(
            &engine,
            engine.get_global("remaining").unwrap(),
            &Expected::String(b"input remains"),
        );
        assert!(engine.action_diagnostics().is_empty());
    }
}

#[test]
fn two_argument_read_is_rejected_at_source_load() {
    let mut engine = Engine::new(EngineConfig::utf8());
    let errors = engine
        .load_str(include_str!("fixtures/read/arity-rejected-at-load.clp"))
        .expect_err("CLIPS rejects two arguments during source parsing");
    assert!(!errors.is_empty());
}
