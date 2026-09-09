//! Private #340 checks for actual values and the independent Error/Halt flags.
//! Include from evaluator.rs without adding any public testing API.

use super::*;
use ferric_rules_core::{binding::BindingSet, binding::VarMap, InstanceName};

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
    globals.set(modules.main_module_id(), "trace", Value::Integer(0));
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

fn string(bytes: &[u8]) -> RuntimeExpr {
    RuntimeExpr::Literal(Value::String(
        FerricString::from_bytes(bytes, StringEncoding::Utf8).unwrap(),
    ))
}

fn integer(value: i64) -> RuntimeExpr {
    RuntimeExpr::Literal(Value::Integer(value))
}

fn call(name: &str, args: Vec<RuntimeExpr>) -> RuntimeExpr {
    RuntimeExpr::Call {
        name: name.into(),
        args,
        span: None,
    }
}

fn nil(ctx: &mut EvalContext<'_>) -> RuntimeExpr {
    RuntimeExpr::Literal(Value::Symbol(
        ctx.symbol_table
            .intern_symbol("nil", StringEncoding::Utf8)
            .unwrap(),
    ))
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

fn format_call(
    ctx: &mut EvalContext<'_>,
    control: RuntimeExpr,
    mut values: Vec<RuntimeExpr>,
) -> RuntimeExpr {
    let mut args = vec![nil(ctx), control];
    args.append(&mut values);
    call("format", args)
}

fn expect_string(value: &Value, bytes: &[u8]) {
    let Value::String(text) = value else {
        panic!("format returns an actual STRING")
    };
    assert_eq!(text.as_bytes(), bytes);
}

fn division() -> EvalError {
    EvalError::DivisionByZero {
        function: "/".into(),
        span: None,
    }
}

fn trace(ctx: &EvalContext<'_>, expected: i64) {
    assert!(
        matches!(ctx.globals.get(ctx.current_module, "trace"), Some(Value::Integer(actual)) if *actual == expected)
    );
}

#[test]
fn format_minimum_arity_is_checked_before_any_router_effect() {
    // Runtime-only construction avoids changing source-time arity diagnostics.
    for args in [vec![], vec![bind(integer(1))]] {
        with_context(|ctx| {
            let actual = args.len();
            let result = eval_inner(ctx, &call("format", args)).unwrap();
            expect_string(&result, b"");
            trace(ctx, 0);
            assert!(ctx.globals.evaluation_error());
            assert!(ctx.globals.evaluation_halted());
            assert!(matches!(ctx.globals.take_diagnostics().as_slice(),
                [EvalError::ArityMismatch { name, actual: count, .. }] if name == "format" && *count == actual));
        });
    }
}

#[test]
fn control_expression_runs_under_inherited_error_without_an_extra_diagnostic() {
    with_context(|ctx| {
        ctx.globals.push_halt_diagnostic(division());
        let expression = format_call(ctx, bind(string(b"%d")), vec![bind(integer(9))]);
        let result = eval_inner(ctx, &expression).unwrap();
        expect_string(&result, b"");
        // The control is evaluated by EnvArgTypeCheck before it notices Error.
        let Some(Value::String(control)) = ctx.globals.get(ctx.current_module, "trace") else {
            panic!("direct control bind must occur even though inherited Error is set")
        };
        assert_eq!(control.as_bytes(), b"%d");
        assert!(ctx.globals.evaluation_error());
        assert!(ctx.globals.evaluation_halted());
        assert!(matches!(
            ctx.globals.take_diagnostics().as_slice(),
            [EvalError::DivisionByZero { .. }]
        ));
    });
}

#[test]
fn halt_without_error_does_not_reject_a_valid_format_or_clear_halt() {
    with_context(|ctx| {
        ctx.globals.push_halt_diagnostic(division());
        ctx.globals.clear_evaluation_error();
        let expression = format_call(ctx, string(b"%04d"), vec![integer(7)]);
        let result = eval_inner(ctx, &expression).unwrap();
        expect_string(&result, b"0007");
        assert!(!ctx.globals.evaluation_error());
        assert!(ctx.globals.evaluation_halted());
        assert!(matches!(
            ctx.globals.take_diagnostics().as_slice(),
            [EvalError::DivisionByZero { .. }]
        ));
    });
}

#[test]
fn character_type_diagnostics_leave_both_flags_clear_and_stop_later_data() {
    for which in 0..4 {
        with_context(|ctx| {
            let value = match which {
                0 => Value::Float(65.9),
                1 => Value::Multifield(Box::default()),
                2 => Value::Void,
                _ => Value::InstanceName(InstanceName::from_symbol(
                    ctx.symbol_table
                        .intern_symbol("red", StringEncoding::Utf8)
                        .unwrap(),
                )),
            };
            let expression = format_call(
                ctx,
                string(b"%c:%d"),
                vec![RuntimeExpr::Literal(value), bind(integer(9))],
            );
            let result = eval_inner(ctx, &expression).unwrap();
            expect_string(&result, b"");
            trace(ctx, 0);
            assert!(!ctx.globals.evaluation_error());
            assert!(!ctx.globals.evaluation_halted());
            assert!(matches!(ctx.globals.take_diagnostics().as_slice(),
                [EvalError::TypeError { function, .. }] if function == "format"));
        });
    }
}

#[test]
fn numeric_and_string_type_diagnostics_set_both_flags_without_later_effects() {
    for (control, value) in [
        (b"%d:%d".as_slice(), string(b"bad")),
        (b"%s:%d", integer(7)),
        (b"%s:%d", RuntimeExpr::Literal(Value::Void)),
        (
            b"%s:%d",
            RuntimeExpr::Literal(Value::Multifield(Box::default())),
        ),
    ] {
        with_context(|ctx| {
            let expression = format_call(ctx, string(control), vec![value, bind(integer(9))]);
            let result = eval_inner(ctx, &expression).unwrap();
            expect_string(&result, b"");
            trace(ctx, 0);
            assert!(ctx.globals.evaluation_error());
            assert!(ctx.globals.evaluation_halted());
            assert!(matches!(ctx.globals.take_diagnostics().as_slice(),
                [EvalError::TypeError { function, .. }] if function == "format"));
        });
    }
}

#[test]
fn recovered_character_data_keeps_error_halt_and_later_builtin_effects() {
    with_context(|ctx| {
        let comparator = RuntimeExpr::Literal(Value::Symbol(
            ctx.symbol_table
                .intern_symbol(">", StringEncoding::Utf8)
                .unwrap(),
        ));
        let length = call(
            "length$",
            vec![call(
                "sort",
                vec![
                    comparator,
                    call("/", vec![integer(1), integer(0)]),
                    integer(2),
                ],
            )],
        );
        let expression = format_call(ctx, string(b"%c%c"), vec![length, bind(integer(9))]);
        let result = eval_inner(ctx, &expression).unwrap();
        expect_string(&result, b"\x02\t");
        trace(ctx, 9);
        assert!(ctx.globals.evaluation_error());
        assert!(ctx.globals.evaluation_halted());
        assert!(matches!(
            ctx.globals.take_diagnostics().as_slice(),
            [EvalError::DivisionByZero { .. }]
        ));
    });
}

// These tests establish Ferric's resource and undefined-C-behavior policies;
// they do not treat unsafe printf allocation/cast behavior as a CLIPS oracle.
fn limited(
    ctx: &mut EvalContext<'_>,
    control: &[u8],
    mut values: Vec<RuntimeExpr>,
    allowed: usize,
) -> Result<Value, EvalError> {
    let mut args = vec![nil(ctx), string(control)];
    args.append(&mut values);
    super::format::builtin_format_with_limit(ctx, &args, None, allowed)
}

fn resource_failure(ctx: &mut EvalContext<'_>, result: &Value, expected_trace: i64) {
    expect_string(result, b"");
    trace(ctx, expected_trace);
    assert!(ctx.globals.evaluation_error());
    assert!(ctx.globals.evaluation_halted());
    assert!(matches!(ctx.globals.take_diagnostics().as_slice(),
        [EvalError::UnsupportedOperation { operation, .. }] if operation == "format"));
    assert!(!ctx.globals.is_sort_recovery_active());
}

#[test]
fn output_limit_rejects_expansion_before_later_operand_effects() {
    with_context(|ctx| {
        let result = limited(ctx, b"%10d%d", vec![bind(integer(7)), bind(integer(9))], 9).unwrap();
        resource_failure(ctx, &result, 7);
    });
}

#[test]
fn aggregate_limit_discards_prefix_and_keeps_reached_operand_effects() {
    with_context(|ctx| {
        let result = limited(ctx, b"%5d%5d", vec![bind(integer(7)), bind(integer(8))], 9).unwrap();
        resource_failure(ctx, &result, 8);
    });
}

#[test]
fn control_prefix_limit_is_checked_before_data_and_ignores_nul_suffix() {
    with_context(|ctx| {
        let result = limited(ctx, b"123456789", vec![bind(integer(7))], 8).unwrap();
        resource_failure(ctx, &result, 0);
    });
    with_context(|ctx| {
        let result = limited(ctx, b"%8s\0ignored%q", vec![string(b"abc\0tail")], 8).unwrap();
        expect_string(&result, b"     abc");
        assert!(!ctx.globals.evaluation_error());
        assert!(!ctx.globals.evaluation_halted());
        assert!(ctx.globals.take_diagnostics().is_empty());
    });
    with_context(|ctx| {
        let result = limited(ctx, b"12345678", vec![], 8).unwrap();
        expect_string(&result, b"12345678");
        assert!(ctx.globals.take_diagnostics().is_empty());
    });
}

#[test]
fn overflowing_parameter_is_a_resource_failure_after_its_operand_is_reached() {
    with_context(|ctx| {
        let control = format!("%{}9d%c", usize::MAX);
        let result = limited(
            ctx,
            control.as_bytes(),
            vec![bind(integer(7)), bind(integer(9))],
            64,
        )
        .unwrap();
        resource_failure(ctx, &result, 7);
    });
}

#[test]
fn large_fixed_float_is_bounded_without_using_the_unsafe_reference_buffer() {
    with_context(|ctx| {
        let result = limited(
            ctx,
            b"%f%d",
            vec![
                RuntimeExpr::Literal(Value::Float(f64::MAX)),
                bind(integer(9)),
            ],
            32,
        )
        .unwrap();
        resource_failure(ctx, &result, 0);
    });
}

#[test]
fn noncanonical_fallback_still_requires_admission_and_contributes_to_total_limit() {
    with_context(|ctx| {
        let result = limited(ctx, b"%..d", vec![bind(integer(7))], 7).unwrap();
        expect_string(&result, b"%.0.lld");
        trace(ctx, 7);
        assert!(ctx.globals.take_diagnostics().is_empty());
    });
    with_context(|ctx| {
        let result = limited(ctx, b"%..d", vec![bind(integer(7))], 6).unwrap();
        resource_failure(ctx, &result, 7);
    });
    with_context(|ctx| {
        let result = limited(ctx, b"%..d", vec![string(b"bad")], 64).unwrap();
        expect_string(&result, b"");
        assert!(matches!(ctx.globals.take_diagnostics().as_slice(),
            [EvalError::TypeError { function, .. }] if function == "format"));
        assert!(ctx.globals.evaluation_error());
        assert!(ctx.globals.evaluation_halted());
    });
}

#[test]
fn integer_formats_keep_the_existing_saturating_rust_cast_policy() {
    for (value, expected) in [
        (f64::NAN, b"0".as_slice()),
        (f64::INFINITY, b"9223372036854775807"),
        (f64::NEG_INFINITY, b"-9223372036854775808"),
        (9_223_372_036_854_775_808.0, b"9223372036854775807"),
        (-9_223_372_036_854_777_856.0, b"-9223372036854775808"),
    ] {
        with_context(|ctx| {
            let result = limited(
                ctx,
                b"%d",
                vec![RuntimeExpr::Literal(Value::Float(value))],
                64,
            )
            .unwrap();
            expect_string(&result, expected);
            assert!(ctx.globals.take_diagnostics().is_empty());
            assert!(!ctx.globals.evaluation_error());
        });
    }
}

#[test]
fn nonlocal_return_balances_operand_recovery_and_preserves_an_outer_scope() {
    for outer in [false, true] {
        with_context(|ctx| {
            ctx.call_depth = 1;
            if outer {
                ctx.globals.begin_sort_recovery();
            }
            let result = limited(ctx, b"%d", vec![call("return", vec![integer(9)])], 64);
            assert!(matches!(
                result,
                Err(EvalError::ReturnControl {
                    value: Value::Integer(9),
                    ..
                })
            ));
            assert_eq!(ctx.globals.is_sort_recovery_active(), outer);
            assert!(!ctx.globals.evaluation_error());
            assert!(ctx.globals.take_diagnostics().is_empty());
            if outer {
                ctx.globals.end_sort_recovery();
            }
        });
    }
}
