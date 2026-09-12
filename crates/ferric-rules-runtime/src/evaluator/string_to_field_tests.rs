//! Private tests for the first-field wrapper's evaluator-flag protocol.

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
        callable_locals: None,
        compact_fact_bindings: None,
        template_resolver: None,
        initial_fact_id: None,
    });
}

fn literal(bytes: &[u8]) -> RuntimeExpr {
    RuntimeExpr::Literal(Value::String(
        FerricString::from_bytes(bytes, StringEncoding::Utf8).unwrap(),
    ))
}

fn error_value(value: &Value) {
    let Value::String(text) = value else {
        panic!("wrapper errors must return a STRING default")
    };
    assert_eq!(text.as_bytes(), b"*** ERROR ***");
}

fn failure() -> EvalError {
    EvalError::DivisionByZero {
        function: "/".into(),
        span: None,
    }
}

#[test]
fn string_to_field_arity_precedes_all_argument_effects() {
    for count in [0, 2] {
        with_context(|ctx| {
            ctx.globals
                .set(ctx.current_module, "trace", Value::Integer(0));
            let effect = RuntimeExpr::Call {
                name: "bind".into(),
                args: vec![
                    RuntimeExpr::GlobalVar {
                        name: "trace".into(),
                        span: None,
                    },
                    RuntimeExpr::Literal(Value::Integer(1)),
                ],
                span: None,
            };
            let result = builtin_string_to_field(ctx, &vec![effect; count], None).unwrap();
            error_value(&result);
            assert!(matches!(
                ctx.globals.get(ctx.current_module, "trace"),
                Some(Value::Integer(0))
            ));
            assert!(ctx.globals.evaluation_error());
            assert!(ctx.globals.evaluation_halted());
            assert!(
                matches!(ctx.globals.take_diagnostics().as_slice(), [EvalError::ArityMismatch { actual, .. }] if *actual == count)
            );
        });
    }
}

#[test]
fn string_to_field_error_default_does_not_duplicate_an_inherited_error() {
    with_context(|ctx| {
        ctx.globals.push_halt_diagnostic(failure());
        let result = builtin_string_to_field(ctx, &[literal(b"42")], None).unwrap();
        error_value(&result);
        assert!(ctx.globals.evaluation_error());
        assert!(ctx.globals.evaluation_halted());
        assert!(matches!(
            ctx.globals.take_diagnostics().as_slice(),
            [EvalError::DivisionByZero { .. }]
        ));
    });
}

#[test]
fn string_to_field_halt_alone_does_not_skip_literal_conversion_or_get_cleared() {
    with_context(|ctx| {
        ctx.globals.push_halt_diagnostic(failure());
        ctx.globals.clear_evaluation_error();
        let result = builtin_string_to_field(ctx, &[literal(b"42")], None).unwrap();
        assert!(matches!(result, Value::Integer(42)));
        assert!(!ctx.globals.evaluation_error());
        assert!(ctx.globals.evaluation_halted());
        assert_eq!(ctx.globals.take_diagnostics().len(), 1);
    });
}

#[test]
fn string_to_field_notices_leave_error_and_halt_clear() {
    for (bytes, channel) in [
        (b"9223372036854775808".as_slice(), "wwarning"),
        (b"\"unfinished\\".as_slice(), "werror"),
    ] {
        with_context(|ctx| {
            let _ = builtin_string_to_field(ctx, &[literal(bytes)], None).unwrap();
            assert!(!ctx.globals.evaluation_error());
            assert!(!ctx.globals.evaluation_halted());
            assert!(
                matches!(ctx.globals.take_diagnostics().as_slice(), [EvalError::ScannerNotice(notice)] if notice.code == "SCANNER1" && notice.channel == channel)
            );
            let events = ctx.globals.take_printout_events();
            assert_eq!(events.len(), 1);
            assert_eq!(events[0].0, channel);
        });
    }
}

#[test]
fn scanner_notice_box_preserves_metadata_display_and_serde() {
    with_context(|ctx| {
        let span = SourceSpan {
            line: 12,
            column: 3,
        };
        let value = builtin_string_to_field(ctx, &[literal(b"   \"partial")], Some(&span)).unwrap();
        assert!(matches!(value, Value::String(text) if text.as_bytes() == b"partial"));
        let diagnostics = ctx.globals.take_diagnostics();
        let [error @ EvalError::ScannerNotice(notice)] = diagnostics.as_slice() else {
            panic!("one scanner notice")
        };
        assert_eq!(notice.function, "string-to-field");
        assert_eq!(notice.code, "SCANNER1");
        assert_eq!(notice.channel, "werror");
        assert_eq!(
            notice.message,
            "Encountered End-Of-File while scanning a string"
        );
        assert_eq!(notice.offset, 3);
        assert!(matches!(
            notice.span,
            Some(SourceSpan {
                line: 12,
                column: 3
            })
        ));
        let expected = "[SCANNER1] Encountered End-Of-File while scanning a string in `string-to-field` on werror at input byte 3 (line 12:3)";
        assert_eq!(error.to_string(), expected);
        assert_eq!(error.clone().to_string(), expected);

        #[cfg(feature = "serde")]
        {
            // A boxed newtype remains the same external variant and field map
            // as the former inline variant for direct EvalError serialization.
            let encoded = serde_json::to_value(error).unwrap();
            assert_eq!(
                encoded,
                serde_json::json!({
                    "ScannerNotice": {
                        "function": "string-to-field",
                        "code": "SCANNER1",
                        "channel": "werror",
                        "message": "Encountered End-Of-File while scanning a string",
                        "offset": 3,
                        "span": { "line": 12, "column": 3 }
                    }
                })
            );
            let restored: EvalError = serde_json::from_value(encoded).unwrap();
            assert_eq!(restored.to_string(), expected);
        }
    });
}
