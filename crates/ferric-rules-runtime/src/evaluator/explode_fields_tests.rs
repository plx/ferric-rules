//! Private #339 wrapper tests for arity and independent error/halt state.

use super::*;
use ferric_rules_core::{binding::BindingSet, binding::VarMap};

fn with_context(test: impl FnOnce(&mut EvalContext<'_>)) {
    let mut symbols = SymbolTable::new();
    let variables = VarMap::new();
    let bindings = BindingSet::new();
    let config = EngineConfig::utf8();
    let functions = FunctionEnv::new();
    let mut globals = GlobalStore::new();
    let generics = GenericRegistry::new();
    let modules = crate::modules::ModuleRegistry::new();
    let owners = rustc_hash::FxHashMap::default();
    test(&mut EvalContext {
        bindings: &bindings,
        var_map: &variables,
        symbol_table: &mut symbols,
        config: &config,
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
        input_buffer: None,
        fact_base: None,
        template_defs: None,
    });
}

fn literal(bytes: &[u8]) -> RuntimeExpr {
    RuntimeExpr::Literal(Value::String(
        FerricString::from_bytes(bytes, StringEncoding::Utf8).unwrap(),
    ))
}

fn call(name: &str, args: Vec<RuntimeExpr>) -> RuntimeExpr {
    RuntimeExpr::Call {
        name: name.into(),
        args,
        span: None,
    }
}

fn empty(value: &Value) {
    assert!(matches!(value, Value::Multifield(fields) if fields.is_empty()));
}

fn failure() -> EvalError {
    EvalError::DivisionByZero {
        function: "/".into(),
        span: None,
    }
}

#[test]
fn explode_arity_precedes_all_argument_effects_for_both_aliases() {
    for name in ["explode$", "str-explode"] {
        for count in [0, 2] {
            with_context(|ctx| {
                ctx.globals
                    .set(ctx.current_module, "trace", Value::Integer(0));
                let effect = call(
                    "bind",
                    vec![
                        RuntimeExpr::GlobalVar {
                            name: "trace".into(),
                            span: None,
                        },
                        RuntimeExpr::Literal(Value::Integer(1)),
                    ],
                );
                let result = eval_inner(ctx, &call(name, vec![effect; count])).unwrap();
                empty(&result);
                assert!(matches!(
                    ctx.globals.get(ctx.current_module, "trace"),
                    Some(Value::Integer(0))
                ));
                assert!(ctx.globals.evaluation_error());
                assert!(ctx.globals.evaluation_halted());
                assert!(
                    matches!(ctx.globals.take_diagnostics().as_slice(), [EvalError::ArityMismatch { name, actual, .. }] if name == "explode$" && *actual == count)
                );
            });
        }
    }
}

#[test]
fn explode_inherited_error_returns_empty_without_a_duplicate_diagnostic() {
    for name in ["explode$", "str-explode"] {
        with_context(|ctx| {
            ctx.globals.push_halt_diagnostic(failure());
            let result = eval_inner(ctx, &call(name, vec![literal(b"42")])).unwrap();
            empty(&result);
            assert!(ctx.globals.evaluation_error());
            assert!(ctx.globals.evaluation_halted());
            assert!(matches!(
                ctx.globals.take_diagnostics().as_slice(),
                [EvalError::DivisionByZero { .. }]
            ));
        });
    }
}

#[test]
fn explode_halt_alone_still_scans_and_does_not_clear_halt() {
    for name in ["explode$", "str-explode"] {
        with_context(|ctx| {
            ctx.globals.push_halt_diagnostic(failure());
            ctx.globals.clear_evaluation_error();
            let result = eval_inner(ctx, &call(name, vec![literal(b"42")])).unwrap();
            assert!(
                matches!(result, Value::Multifield(fields) if matches!(fields.as_slice(), [Value::Integer(42)]))
            );
            assert!(!ctx.globals.evaluation_error());
            assert!(ctx.globals.evaluation_halted());
            assert_eq!(ctx.globals.take_diagnostics().len(), 1);
        });
    }
}

#[test]
fn explode_notices_keep_scan_order_metadata_and_leave_error_halt_clear() {
    with_context(|ctx| {
        let result =
            builtin_explode_mf(ctx, &[literal(b"a 9223372036854775808 \"partial")], None).unwrap();
        assert!(matches!(result, Value::Multifield(fields) if fields.len() == 3));
        assert!(!ctx.globals.evaluation_error());
        assert!(!ctx.globals.evaluation_halted());
        let diagnostics = ctx.globals.take_diagnostics();
        let [EvalError::ScannerNotice(integer), EvalError::ScannerNotice(string)] =
            diagnostics.as_slice()
        else {
            panic!("ordered scanner notices")
        };
        assert_eq!(integer.function, "explode$");
        assert_eq!(integer.code, "SCANNER1");
        assert_eq!(integer.channel, "wwarning");
        assert_eq!(integer.offset, 2);
        assert_eq!(string.function, "explode$");
        assert_eq!(string.channel, "werror");
        assert_eq!(string.offset, 22);
        let events = ctx.globals.take_printout_events();
        assert_eq!(events.len(), 2);
        assert_eq!(events[0].0, "wwarning");
        assert_eq!(events[1].0, "werror");
    });
}

#[test]
fn shared_conversion_keeps_first_field_unknown_eof_and_ignored_suffix_policy() {
    with_context(|ctx| {
        let first = builtin_string_to_field(ctx, &[literal(b"\x0b 42")], None).unwrap();
        assert!(matches!(first, Value::String(text) if text.as_bytes() == b"*** ERROR ***"));
        let all = builtin_explode_mf(ctx, &[literal(b"\x0b 42")], None).unwrap();
        let Value::Multifield(fields) = all else {
            panic!("MULTIFIELD")
        };
        assert!(
            matches!(fields.as_slice(), [Value::String(text), Value::Integer(42)] if text.as_bytes() == b"<<<unprintable character>>>")
        );
        let first = builtin_string_to_field(ctx, &[literal(b"")], None).unwrap();
        assert!(
            matches!(first, Value::Symbol(symbol) if ctx.symbol_table.resolve_symbol_str(symbol) == Some("EOF"))
        );
        empty(&builtin_explode_mf(ctx, &[literal(b"")], None).unwrap());
        let first = builtin_string_to_field(ctx, &[literal(b"42 \"unterminated")], None).unwrap();
        assert!(matches!(first, Value::Integer(42)));
        assert!(ctx.globals.take_diagnostics().is_empty());
        assert!(ctx.globals.take_printout_events().is_empty());
    });
}
