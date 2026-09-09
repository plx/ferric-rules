//! #340: format result variants, operand ordering, diagnostic severity and bytes.
//!
//! CLIPS 6.30 iofun.c and the sealed original/extra format programs establish
//! these controls. Global capture exposes values hidden by halted printout;
//! host-byte controls extend the existing byte API, not source token grammar.
//! Unmeasured modifier/resource/float-to-integer domain policies are excluded.

use ferric_rules_core::Value;
use ferric_rules_runtime::evaluator::EvalError;
use ferric_rules_runtime::{ActionError, Engine, EngineConfig, HaltReason, HostValue, RunLimit};

const PREFIX: &str = r"
    (defglobal ?*result* = pending ?*trace* = 0 ?*after* = 0)
    (deffunction mark (?n ?value)
      (bind ?*trace* (+ (* ?*trace* 10) ?n)) ?value)
    (deffunction fail (?n)
      (bind ?*trace* (+ (* ?*trace* 10) ?n)) (/ 1 0))
";

fn expression_case(expression: &str, halt: HaltReason, trace: i64, expected: &[u8]) -> Engine {
    let source = format!(
        "{PREFIX}\n(defrule exercise =>\n (bind ?*result* {expression})\n (bind ?*after* 1))\n"
    );
    let mut engine = Engine::with_rules(&source).unwrap();
    let result = engine.run(RunLimit::Count(10)).unwrap();
    assert_eq!(result.rules_fired, 1, "{expression}");
    assert_eq!(result.halt_reason, halt, "{expression}");
    assert_string(&engine, expected);
    assert!(matches!(engine.get_global("trace"), Some(Value::Integer(value)) if *value == trace));
    let after = i64::from(halt == HaltReason::AgendaEmpty);
    assert!(matches!(engine.get_global("after"), Some(Value::Integer(value)) if *value == after));
    engine
}

fn assert_string(engine: &Engine, expected: &[u8]) {
    let Some(Value::String(value)) = engine.get_global("result") else {
        panic!("format must store an actual STRING, including its empty error default")
    };
    assert_eq!(value.as_bytes(), expected);
    if std::str::from_utf8(expected).is_ok() {
        assert_eq!(value.as_str().unwrap().as_bytes(), expected);
    } else {
        assert!(value.as_str().is_err());
    }
}

fn one_format_diagnostic(engine: &Engine) {
    assert_eq!(engine.action_diagnostics().len(), 1);
    assert!(engine.action_diagnostics()[0]
        .to_string()
        .contains("format"));
}

fn format_type_error(error: &ActionError) -> bool {
    matches!(error, ActionError::Evaluator(EvalError::TypeError { function, .. }) if function == "format")
}

fn division_error(error: &ActionError) -> bool {
    matches!(error, ActionError::Evaluator(EvalError::DivisionByZero { function, .. }) if function == "/")
}

#[test]
fn router_control_and_reached_data_execute_once_in_order() {
    for (expression, trace, expected) in [
        (
            r#"(format (mark 1 nil) (mark 2 "%04d:%s") (mark 3 7) (mark 4 red))"#,
            1234,
            b"0007:red".as_slice(),
        ),
        (r#"(format (mark 1 nil) (mark 2 ""))"#, 12, b""),
        (r#"(format (mark 1 nil) (mark 2 "abc%"))"#, 12, b"abc%"),
    ] {
        let engine = expression_case(expression, HaltReason::AgendaEmpty, trace, expected);
        assert!(engine.action_diagnostics().is_empty());
    }
}

#[test]
fn whole_format_rejects_invalid_flags_before_any_data_or_count_error() {
    for expression in [
        r#"(format (mark 1 nil) (mark 2 "%q") (fail 3))"#,
        r#"(format (mark 1 nil) (mark 2 "%d:%q") (mark 3 red) (fail 4))"#,
        // This also has a count mismatch: invalid flag validation wins first.
        r#"(format (mark 1 nil) (mark 2 "%d:%q"))"#,
        r#"(format (mark 1 nil) (mark 2 "%*d") (mark 3 4) (fail 4))"#,
    ] {
        let engine = expression_case(expression, HaltReason::ActionError, 12, b"");
        one_format_diagnostic(&engine);
        assert!(!matches!(
            engine.action_diagnostics()[0],
            ActionError::Evaluator(EvalError::ArityMismatch { .. })
        ));
        assert!(!engine.action_diagnostics().iter().any(division_error));
    }
}

#[test]
fn exact_conversion_count_precedes_all_data_effects() {
    for expression in [
        r#"(format (mark 1 nil) (mark 2 "%04d") (mark 3 7) (fail 4))"#,
        r#"(format (mark 1 nil) (mark 2 "%04d:%d") (fail 3))"#,
        r#"(format (mark 1 nil) (mark 2 "%%:%n:%r:%t:%v") (fail 3))"#,
    ] {
        let engine = expression_case(expression, HaltReason::ActionError, 12, b"");
        one_format_diagnostic(&engine);
        assert!(!engine.action_diagnostics().iter().any(division_error));
    }
    let source = format!(
        "(format (mark 1 nil) (mark 2 \"%{}d\") (mark 3 7))",
        "0".repeat(74)
    );
    let engine = expression_case(&source, HaltReason::ActionError, 12, b"");
    one_format_diagnostic(&engine);
}

#[test]
fn malformed_control_type_is_checked_before_data() {
    let engine = expression_case(
        "(format (mark 1 nil) (mark 2 42) (fail 3))",
        HaltReason::ActionError,
        12,
        b"",
    );
    one_format_diagnostic(&engine);
    assert!(format_type_error(&engine.action_diagnostics()[0]));
}

#[test]
fn each_numeric_and_lexeme_conversion_validates_before_the_next_expression() {
    // All numeric conversions share EnvArgTypeCheck(INTEGER_OR_FLOAT). `%s`
    // uses SYMBOL_OR_STRING, including INSTANCE-NAME under OBJECT_SYSTEM.
    for (conversion, invalid) in [
        ('d', "red"),
        ('o', "red"),
        ('x', "red"),
        ('u', "red"),
        ('f', "red"),
        ('e', "red"),
        ('g', "red"),
        ('s', "42"),
    ] {
        let expression = format!(
            "(format (mark 1 nil) (mark 2 \"%{conversion}:%d\") (mark 3 {invalid}) (fail 4))"
        );
        let engine = expression_case(&expression, HaltReason::ActionError, 123, b"");
        one_format_diagnostic(&engine);
        assert!(format_type_error(&engine.action_diagnostics()[0]));
    }
}

#[test]
fn later_type_failure_discards_formatted_prefix_without_rewinding_effects() {
    let engine = expression_case(
        r#"(format (mark 1 nil) (mark 2 "prefix=%04d:%d:%d") (mark 3 7) (mark 4 red) (fail 5))"#,
        HaltReason::ActionError,
        1234,
        b"",
    );
    one_format_diagnostic(&engine);
    assert!(format_type_error(&engine.action_diagnostics()[0]));
}

#[test]
fn reached_child_failure_returns_empty_string_without_secondary_format_error() {
    let engine = expression_case(
        r#"(format (mark 1 nil) (mark 2 "%04d:%d") (fail 3) (mark 4 9))"#,
        HaltReason::ActionError,
        123,
        b"",
    );
    assert_eq!(engine.action_diagnostics().len(), 1);
    assert!(division_error(&engine.action_diagnostics()[0]));
}

#[test]
fn character_wrong_types_are_nonfatal_and_skip_remaining_format_data() {
    // c-float and character-instance-name-rejection both continue after format.
    for invalid in ["65.9", "[red]"] {
        let expression =
            format!("(format (mark 1 nil) (mark 2 \"%c:%d\") (mark 3 {invalid}) (fail 4))");
        let engine = expression_case(&expression, HaltReason::AgendaEmpty, 123, b"");
        one_format_diagnostic(&engine);
        assert!(format_type_error(&engine.action_diagnostics()[0]));
    }
    let engine = host_engine(b"%c", HostArgument::Name(b"n\xff\0ignored"));
    assert_string(&engine, b"");
    one_format_diagnostic(&engine);
    assert!(format_type_error(&engine.action_diagnostics()[0]));
}

#[test]
fn inherited_error_suppresses_numeric_lexeme_and_control_secondary_diagnostics() {
    for expression in [
        r#"(format nil "%d%c" (length$ (sort > (/ 1 0) 2)) (bind ?*trace* 9))"#,
        r#"(format nil "%s%c" (length$ (sort > (/ 1 0) 2)) (bind ?*trace* 9))"#,
        "(format nil (str-cat (length$ (sort > (/ 1 0) 2))) (bind ?*trace* 9))",
    ] {
        let engine = expression_case(expression, HaltReason::ActionError, 0, b"");
        assert_eq!(engine.action_diagnostics().len(), 1);
        assert!(division_error(&engine.action_diagnostics()[0]));
    }
}

#[test]
fn character_recovered_integer_consumes_later_direct_operand_despite_error() {
    // The pinned program establishes trace 9 and terminal halt. Actual bytes
    // follow source: sort returns two fields, length$ returns 2, and `%c` uses
    // EnvRtnUnknown without an Error gate. The second direct bind yields TAB.
    // This is a successful local format result under an inherited fatal error,
    // not the empty default used when format's own conversion fails.
    let engine = expression_case(
        r#"(format nil "%c%c" (length$ (sort > (/ 1 0) 2)) (bind ?*trace* 9))"#,
        HaltReason::ActionError,
        9,
        b"\x02\t",
    );
    assert_eq!(engine.action_diagnostics().len(), 1);
    assert!(division_error(&engine.action_diagnostics()[0]));
}

#[test]
fn character_float_error_default_adds_its_nonfatal_type_diagnostic() {
    let engine = expression_case(
        r#"(format nil "%c:%d" (/ 1 0) (bind ?*trace* 9))"#,
        HaltReason::ActionError,
        0,
        b"",
    );
    // / has FLOAT 1.0 as its error return. `%c` still inspects that value,
    // unlike `%d`/`%s`, and reports its own type error without clearing halt.
    assert_eq!(engine.action_diagnostics().len(), 2);
    assert!(division_error(&engine.action_diagnostics()[0]));
    assert!(format_type_error(&engine.action_diagnostics()[1]));
}

#[derive(Clone, Copy)]
enum HostArgument<'a> {
    String(&'a [u8]),
    Symbol(&'a [u8]),
    Name(&'a [u8]),
    Integer(i64),
}

fn host_engine(control: &[u8], argument: HostArgument<'_>) -> Engine {
    let mut engine = Engine::new(EngineConfig::utf8());
    engine
        .load_str(
            r"
        (defglobal ?*result* = pending ?*after* = 0)
        (defrule exercise (input ?control ?value) =>
          (bind ?*result* (format nil ?control ?value))
          (bind ?*after* 1))
        ",
        )
        .unwrap();
    engine.reset().unwrap();
    let control: HostValue = engine.create_string_bytes(control).unwrap().into();
    let argument: HostValue = match argument {
        HostArgument::String(bytes) => engine.create_string_bytes(bytes).unwrap().into(),
        HostArgument::Symbol(bytes) => engine.symbol_value_bytes(bytes).unwrap(),
        HostArgument::Name(bytes) => engine.instance_name_value_bytes(bytes).unwrap(),
        HostArgument::Integer(value) => value.into(),
    };
    engine.assert_ordered("input", [control, argument]).unwrap();
    let result = engine.run(RunLimit::Count(10)).unwrap();
    assert_eq!(result.rules_fired, 1);
    assert_eq!(result.halt_reason, HaltReason::AgendaEmpty);
    assert!(matches!(
        engine.get_global("after"),
        Some(Value::Integer(1))
    ));
    engine
}

fn host_case(control: &[u8], argument: HostArgument<'_>, expected: &[u8]) {
    let engine = host_engine(control, argument);
    assert_string(&engine, expected);
    assert!(engine.action_diagnostics().is_empty());
}

#[test]
fn string_conversion_accepts_raw_lexemes_and_unbracketed_names_with_byte_limits() {
    for argument in [
        HostArgument::String(b"a\xff\0ignored"),
        HostArgument::Symbol(b"a\xff\0ignored"),
        HostArgument::Name(b"a\xff\0ignored"),
    ] {
        host_case(b"%s", argument, b"a\xff");
        host_case(b"%6.2s", argument, b"    a\xff");
        host_case(b"%-6.1s", argument, b"a     ");
    }
    // The actual clipped C3 byte is pinned by string-name-and-byte-precision.
    host_case(b"%.1s", HostArgument::String("é".as_bytes()), b"\xc3");
    host_case(b"%s", HostArgument::Name(b"widget"), b"widget");
}

#[test]
fn character_conversion_uses_low_byte_and_fragment_nul_termination() {
    for argument in [
        HostArgument::String("é".as_bytes()),
        HostArgument::Symbol("é".as_bytes()),
    ] {
        host_case(b"%c", argument, b"\xc3");
        host_case(b"%3c", argument, b"  \xc3");
    }
    for value in [-1, 255] {
        host_case(b"%c", HostArgument::Integer(value), b"\xff");
    }
    for argument in [
        HostArgument::Integer(0),
        HostArgument::Integer(256),
        HostArgument::String(b""),
        HostArgument::Symbol(b""),
        HostArgument::String(b"\0ignored"),
    ] {
        host_case(b"[%4c]:end", argument, b"[   ]:end");
        host_case(b"[%-4c]:end", argument, b"[]:end");
    }
    host_case(b"%4.0c", HostArgument::Integer(65), b"   A");
    host_case(b"%-4.3c", HostArgument::Integer(65), b"A   ");
}

#[test]
fn raw_control_bytes_stop_at_nul_before_flag_and_count_validation() {
    // Source-derived C-string boundary over the public byte STRING API. The
    // unsuccessful literal-NUL batch attempt is not used as an output oracle.
    host_case(
        b"\xff:%.1s\0%q:%d",
        HostArgument::String("é".as_bytes()),
        b"\xff:\xc3",
    );
    host_case(b"[%s]\0%d", HostArgument::Name(b"n\xff"), b"[n\xff]");
}
