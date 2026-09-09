//! Private #346 checks for actual return values and transient evaluator state.
//! Source-backed scanner/router behavior and Ferric framed/encoding policies
//! are labeled separately. No public state setter or byte-input API is needed.

use super::*;
use ferric_rules_core::InstanceName;

fn with_context(config: &EngineConfig, lines: &[&str], test: impl FnOnce(&mut EvalContext<'_>)) {
    let mut symbols = SymbolTable::new();
    let variables = VarMap::new();
    let bindings = BindingSet::new();
    let functions = FunctionEnv::new();
    let mut globals = GlobalStore::new();
    let generics = GenericRegistry::new();
    let modules = crate::modules::ModuleRegistry::new();
    let owners = rustc_hash::FxHashMap::default();
    let mut input = lines.iter().map(|line| (*line).to_owned()).collect();
    globals.set(modules.main_module_id(), "trace", Value::Integer(0));
    test(&mut EvalContext {
        bindings: &bindings,
        var_map: &variables,
        symbol_table: &mut symbols,
        config,
        functions: &functions,
        globals: &mut globals,
        generics: &generics,
        call_depth: 0,
        expression_depth: 0,
        current_module: modules.main_module_id(),
        module_registry: &modules,
        function_modules: &owners,
        global_modules: &owners,
        generic_modules: &owners,
        method_chain: None,
        input_buffer: Some(&mut input),
        fact_base: None,
        template_defs: None,
    });
}

fn call(name: &str, args: Vec<RuntimeExpr>) -> RuntimeExpr {
    RuntimeExpr::Call {
        name: name.into(),
        args,
        span: None,
    }
}

fn integer(value: i64) -> RuntimeExpr {
    RuntimeExpr::Literal(Value::Integer(value))
}

fn string(bytes: &[u8]) -> RuntimeExpr {
    RuntimeExpr::Literal(Value::String(
        FerricString::from_bytes(bytes, StringEncoding::Utf8).unwrap(),
    ))
}

fn lexeme(ctx: &mut EvalContext<'_>, kind: u8, bytes: &[u8]) -> RuntimeExpr {
    if kind == 0 {
        return string(bytes);
    }
    let symbol = ctx
        .symbol_table
        .intern_symbol_bytes(bytes, StringEncoding::Utf8)
        .unwrap();
    RuntimeExpr::Literal(if kind == 1 {
        Value::Symbol(symbol)
    } else {
        Value::InstanceName(InstanceName::from_symbol(symbol))
    })
}

fn bind(value: RuntimeExpr) -> RuntimeExpr {
    call(
        "bind",
        vec![
            RuntimeExpr::GlobalVar {
                name: "trace".into(),
                span: None,
            },
            value,
        ],
    )
}

fn text(value: &Value, expected: &[u8]) {
    assert!(matches!(value, Value::String(actual) if actual.as_bytes() == expected));
}

fn read_error(value: &Value) {
    text(value, b"*** READ ERROR ***");
}

fn remaining(ctx: &EvalContext<'_>, expected: &[&str]) {
    assert_eq!(
        ctx.input_buffer
            .as_deref()
            .unwrap()
            .iter()
            .map(String::as_str)
            .collect::<Vec<_>>(),
        expected
    );
}

fn flags(ctx: &EvalContext<'_>, error: bool, halt: bool) {
    assert_eq!(ctx.globals.evaluation_error(), error);
    assert_eq!(ctx.globals.evaluation_halted(), halt);
}

fn division() -> EvalError {
    EvalError::DivisionByZero {
        function: "/".into(),
        span: None,
    }
}

#[test]
fn read_arity_uses_actual_default_before_effects_or_consumption() {
    for name in ["read", "readline"] {
        with_context(&EngineConfig::utf8(), &["kept"], |ctx| {
            let expression = call(name, vec![bind(integer(1)), bind(integer(2))]);
            read_error(&eval_inner(ctx, &expression).unwrap());
            assert!(matches!(
                ctx.globals.get(ctx.current_module, "trace"),
                Some(Value::Integer(0))
            ));
            remaining(ctx, &["kept"]);
            flags(ctx, true, true);
            assert!(!ctx.globals.is_sort_recovery_active());
            assert!(matches!(ctx.globals.take_diagnostics().as_slice(),
                [EvalError::ArityMismatch { name: actual, actual: 2, .. }] if actual == name));
        });
    }
}

#[test]
fn exact_stdin_aliases_accept_all_logical_lexeme_types_and_c_prefixes() {
    for name in ["read", "readline"] {
        for kind in 0..3 {
            for bytes in [
                b"t".as_slice(),
                b"T",
                b"stdin",
                b"stdin\0\xffignored",
                b"T\0other",
            ] {
                with_context(&EngineConfig::utf8(), &["7 tail", "next"], |ctx| {
                    let router = lexeme(ctx, kind, bytes);
                    let value = eval_inner(ctx, &call(name, vec![router])).unwrap();
                    if name == "read" {
                        assert!(matches!(value, Value::Integer(7)));
                    } else {
                        text(&value, b"7 tail");
                    }
                    remaining(ctx, &["next"]);
                    flags(ctx, false, false);
                    assert!(ctx.globals.take_diagnostics().is_empty());
                    assert!(!ctx.globals.is_sort_recovery_active());
                });
            }
        }
    }
}

#[test]
fn unavailable_versus_illegal_router_types_are_fatal_without_consuming() {
    for name in ["read", "readline"] {
        for which in 0..8 {
            with_context(&EngineConfig::utf8(), &["kept"], |ctx| {
                let router = match which {
                    0 => string(b"missing"),
                    1 => lexeme(ctx, 1, b"STDIN"),
                    2 => lexeme(ctx, 2, b"stdin-other"),
                    3 => string(b"\0stdin"),
                    4 => integer(42),
                    5 => RuntimeExpr::Literal(Value::Float(42.0)),
                    6 => RuntimeExpr::Literal(Value::Multifield(Box::default())),
                    _ => RuntimeExpr::Literal(Value::Void),
                };
                read_error(&eval_inner(ctx, &call(name, vec![router])).unwrap());
                remaining(ctx, &["kept"]);
                flags(ctx, true, true);
                let diagnostics = ctx.globals.take_diagnostics();
                if which < 6 {
                    assert!(
                        matches!(diagnostics.as_slice(), [EvalError::UnsupportedOperation { operation, .. }] if operation == name)
                    );
                } else {
                    assert!(
                        matches!(diagnostics.as_slice(), [EvalError::TypeError { function, .. }] if function == name)
                    );
                }
                assert!(ctx.globals.take_printout_events().is_empty());
            });
        }
    }
}

#[test]
fn optional_router_keeps_direct_division_float_default_then_reports_unavailable_name() {
    for name in ["read", "readline"] {
        with_context(&EngineConfig::utf8(), &["kept"], |ctx| {
            let router = call("/", vec![integer(1), integer(0)]);
            read_error(&eval_inner(ctx, &call(name, vec![router])).unwrap());
            remaining(ctx, &["kept"]);
            flags(ctx, true, true);
            let diagnostics = ctx.globals.take_diagnostics();
            assert!(matches!(diagnostics.as_slice(),
                [EvalError::DivisionByZero { function, .. }, EvalError::UnsupportedOperation { operation, reason, .. }]
                if function == "/" && operation == name && reason.contains("FLOAT")));
            assert!(!ctx.globals.is_sort_recovery_active());
        });
    }
}

#[test]
fn framed_halt_policy_evaluates_valid_router_without_new_error_or_consumption() {
    // Ferric policy: whole frames are preserved. CLIPS getc-before-Halt's
    // one-byte consumption cannot be represented by this line queue.
    for name in ["read", "readline"] {
        for clear_error in [false, true] {
            with_context(&EngineConfig::utf8(), &["\"partial\\", "kept"], |ctx| {
                ctx.globals.push_halt_diagnostic(division());
                if clear_error {
                    ctx.globals.clear_evaluation_error();
                }
                let router = bind(string(b"stdin"));
                read_error(&eval_inner(ctx, &call(name, vec![router])).unwrap());
                assert!(
                    matches!(ctx.globals.get(ctx.current_module, "trace"), Some(Value::String(value)) if value.as_bytes() == b"stdin")
                );
                remaining(ctx, &["\"partial\\", "kept"]);
                flags(ctx, !clear_error, true);
                assert!(matches!(
                    ctx.globals.take_diagnostics().as_slice(),
                    [EvalError::DivisionByZero { .. }]
                ));
                assert!(ctx.globals.take_printout_events().is_empty());
                assert!(!ctx.globals.is_sort_recovery_active());
            });
        }
    }
}

#[test]
fn unknown_router_is_still_diagnosed_before_the_inherited_halt_guard() {
    for name in ["read", "readline"] {
        with_context(&EngineConfig::utf8(), &["kept"], |ctx| {
            ctx.globals.push_halt_diagnostic(division());
            ctx.globals.clear_evaluation_error();
            read_error(&eval_inner(ctx, &call(name, vec![string(b"missing")])).unwrap());
            flags(ctx, true, true);
            remaining(ctx, &["kept"]);
            assert!(matches!(ctx.globals.take_diagnostics().as_slice(),
                [EvalError::DivisionByZero { .. }, EvalError::UnsupportedOperation { operation, .. }] if operation == name));
        });
    }
}

#[test]
fn router_return_control_balances_recovery_and_preserves_outer_scope() {
    for name in ["read", "readline"] {
        for outer in [false, true] {
            with_context(&EngineConfig::utf8(), &["kept"], |ctx| {
                ctx.call_depth = 1;
                if outer {
                    ctx.globals.begin_sort_recovery();
                }
                let result = eval_inner(ctx, &call(name, vec![call("return", vec![integer(9)])]));
                assert!(matches!(
                    result,
                    Err(EvalError::ReturnControl {
                        value: Value::Integer(9),
                        ..
                    })
                ));
                remaining(ctx, &["kept"]);
                assert_eq!(ctx.globals.is_sort_recovery_active(), outer);
                flags(ctx, false, false);
                assert!(ctx.globals.take_diagnostics().is_empty());
                if outer {
                    ctx.globals.end_sort_recovery();
                }
            });
        }
    }
}

#[test]
fn read_skips_stop_frames_but_discards_only_the_selected_frames_tail() {
    with_context(
        &EngineConfig::utf8(),
        &["", "; comment", "\x0c", "\"two words\" 99", "42"],
        |ctx| {
            text(
                &eval_inner(ctx, &call("read", vec![])).unwrap(),
                b"two words",
            );
            remaining(ctx, &["42"]);
            assert!(matches!(
                eval_inner(ctx, &call("read", vec![])).unwrap(),
                Value::Integer(42)
            ));
            let eof = eval_inner(ctx, &call("read", vec![])).unwrap();
            assert!(
                matches!(eof, Value::Symbol(value) if ctx.symbol_table.resolve_symbol_str(value) == Some("EOF"))
            );
            flags(ctx, false, false);
            assert!(ctx.globals.take_diagnostics().is_empty());
        },
    );
}

#[test]
fn unknown_first_field_is_a_silent_nonfatal_read_error_string() {
    with_context(&EngineConfig::utf8(), &["\x0b 42", "next"], |ctx| {
        read_error(&eval_inner(ctx, &call("read", vec![])).unwrap());
        remaining(ctx, &["next"]);
        flags(ctx, false, false);
        assert!(ctx.globals.take_diagnostics().is_empty());
        assert!(ctx.globals.take_printout_events().is_empty());
    });
}

#[test]
fn scanner_notices_keep_partial_values_exact_bytes_and_nonfatal_metadata() {
    with_context(
        &EngineConfig::utf8(),
        &["9223372036854775808", "\"partial\\", "next"],
        |ctx| {
            assert!(matches!(
                eval_inner(ctx, &call("read", vec![])).unwrap(),
                Value::Integer(i64::MAX)
            ));
            let partial = eval_inner(ctx, &call("read", vec![])).unwrap();
            text(&partial, b"partial\xff");
            assert!(matches!(partial, Value::String(value) if value.as_str().is_err()));
            remaining(ctx, &["next"]);
            flags(ctx, false, false);
            let diagnostics = ctx.globals.take_diagnostics();
            let [EvalError::ScannerNotice(integer), EvalError::ScannerNotice(string)] =
                diagnostics.as_slice()
            else {
                panic!("two ordered scanner notices")
            };
            assert_eq!(
                (&*integer.function, &*integer.channel, integer.offset),
                ("read", "wwarning", 0)
            );
            assert_eq!(
                (&*string.function, &*string.channel, string.offset),
                ("read", "werror", 0)
            );
            let events = ctx.globals.take_printout_events();
            assert_eq!(
                events,
                vec![
                    (
                        "wwarning".into(),
                        b"[SCANNER1] WARNING: Over or underflow of long long integer.\n".to_vec()
                    ),
                    (
                        "werror".into(),
                        b"\n[SCANNER1] Encountered End-Of-File while scanning a string\n".to_vec()
                    ),
                ]
            );
        },
    );
}

#[test]
fn readline_preserves_empty_and_complete_frames_and_no_buffer_is_eof() {
    with_context(
        &EngineConfig::utf8(),
        &["", "  red\tblue  ", "a\nb"],
        |ctx| {
            for expected in [b"".as_slice(), b"  red\tblue  ", b"a\nb"] {
                text(
                    &eval_inner(ctx, &call("readline", vec![])).unwrap(),
                    expected,
                );
            }
            for name in ["read", "readline"] {
                let eof = eval_inner(ctx, &call(name, vec![])).unwrap();
                assert!(
                    matches!(eof, Value::Symbol(value) if ctx.symbol_table.resolve_symbol_str(value) == Some("EOF"))
                );
            }
            ctx.input_buffer = None;
            for name in ["read", "readline"] {
                let eof = eval_inner(ctx, &call(name, vec![])).unwrap();
                assert!(
                    matches!(eof, Value::Symbol(value) if ctx.symbol_table.resolve_symbol_str(value) == Some("EOF"))
                );
            }
            flags(ctx, false, false);
            assert!(ctx.globals.take_diagnostics().is_empty());
        },
    );
}

#[test]
fn strict_encoding_failures_keep_scanner_notices_but_return_actual_error_default() {
    // Ferric strict-ASCII configuration policy, not a CLIPS scanner mode.
    for (name, line, notice) in [("read", "\"partial\\", true), ("readline", "é", false)] {
        with_context(&EngineConfig::ascii(), &[line, "kept"], |ctx| {
            read_error(&eval_inner(ctx, &call(name, vec![])).unwrap());
            remaining(ctx, &["kept"]);
            flags(ctx, true, true);
            let diagnostics = ctx.globals.take_diagnostics();
            if notice {
                assert!(
                    matches!(diagnostics.as_slice(), [EvalError::ScannerNotice(_), EvalError::TypeError { function, .. }] if function == name)
                );
                assert_eq!(ctx.globals.take_printout_events().len(), 1);
            } else {
                assert!(
                    matches!(diagnostics.as_slice(), [EvalError::TypeError { function, .. }] if function == name)
                );
                assert!(ctx.globals.take_printout_events().is_empty());
            }
        });
    }
}
