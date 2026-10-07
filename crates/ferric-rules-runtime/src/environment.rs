//! Environment-owned builtin state and dynamic source evaluation.

use ferric_rules_core::Value;

use crate::evaluator::{self, EvalContext, EvalError, RuntimeExpr, SourceSpan};

pub(crate) fn is_builtin(name: &str) -> bool {
    matches!(
        name,
        "random" | "seed" | "time" | "eval" | "build" | "assert-string" | "str-assert"
    )
}

fn arity(name: &str, args: &[RuntimeExpr], expected: &str, span: Option<&SourceSpan>) -> EvalError {
    EvalError::ArityMismatch {
        name: name.to_owned(),
        expected: expected.to_owned(),
        actual: args.len(),
        span: span.cloned(),
    }
}

fn integer(
    ctx: &mut EvalContext<'_>,
    expression: &RuntimeExpr,
    name: &str,
    span: Option<&SourceSpan>,
) -> Result<i64, EvalError> {
    match evaluator::eval_inner(ctx, expression)? {
        Value::Integer(value) => Ok(value),
        value => Err(EvalError::TypeError {
            function: name.to_owned(),
            expected: "INTEGER".to_owned(),
            actual: value.type_name().to_owned(),
            span: span.cloned(),
        }),
    }
}

#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
pub(crate) fn eval(
    ctx: &mut EvalContext<'_>,
    name: &str,
    args: &[RuntimeExpr],
    span: Option<&SourceSpan>,
) -> Result<Value, EvalError> {
    match name {
        "eval" | "build" | "assert-string" | "str-assert" => dynamic_source(ctx, name, args, span),
        "seed" => {
            let [argument] = args else {
                return Err(arity(name, args, "1", span));
            };
            let seed = integer(ctx, argument, name, span)?;
            ctx.engine.globals.random = crate::random::RandomState::seeded(seed as u32);
            Ok(Value::Void)
        }
        "random" => {
            // The reference draws before evaluating its bounds: nested random
            // calls in argument expressions consume the following values.
            let draw = ctx.engine.globals.random.next();
            if args.is_empty() {
                return Ok(Value::Integer(draw));
            }
            let [minimum, maximum] = args else {
                ctx.engine.globals.push_printout_event(
                    "werror".to_owned(),
                    "[MISCFUN2] Function random expected either 0 or 2 arguments\n".to_owned(),
                );
                return Ok(Value::Integer(draw));
            };
            let minimum = integer(ctx, minimum, name, span)?;
            let maximum = integer(ctx, maximum, name, span)?;
            if maximum < minimum {
                ctx.engine.globals.push_printout_event(
                    "werror".to_owned(),
                    "[MISCFUN3] Function random expected argument #1 to be less than argument #2\n"
                        .to_owned(),
                );
                return Ok(Value::Integer(draw));
            }
            // i128 retains the inclusive full i64 range without overflow.
            let width = i128::from(maximum) - i128::from(minimum) + 1;
            let result = i128::from(minimum) + i128::from(draw) % width;
            Ok(Value::Integer(
                i64::try_from(result).expect("bounded random is within i64"),
            ))
        }
        "time" => {
            if !args.is_empty() {
                return Err(arity(name, args, "0", span));
            }
            let now = std::time::SystemTime::now();
            let seconds = match now.duration_since(std::time::UNIX_EPOCH) {
                Ok(duration) => duration.as_secs_f64(),
                Err(error) => -error.duration().as_secs_f64(),
            };
            Ok(Value::Float(seconds))
        }
        _ => Err(EvalError::UnknownFunction {
            name: name.to_owned(),
            span: span.cloned(),
        }),
    }
}

fn failure(name: &str, reason: &dyn std::fmt::Display, span: Option<&SourceSpan>) -> EvalError {
    EvalError::UnsupportedOperation {
        operation: name.to_owned(),
        reason: reason.to_string(),
        span: span.cloned(),
    }
}

fn dynamic_source(
    ctx: &mut EvalContext<'_>,
    name: &str,
    args: &[RuntimeExpr],
    span: Option<&SourceSpan>,
) -> Result<Value, EvalError> {
    use ferric_rules_parser::{interpret_action_expr, parse_first_sexpr, Atom, FileId, SExpr};
    let [argument] = args else {
        return Err(arity(name, args, "1", span));
    };
    let source = match evaluator::eval_inner(ctx, argument)? {
        Value::String(value) => value.as_str().to_owned(),
        Value::Symbol(symbol) if matches!(name, "eval" | "build") => ctx
            .engine
            .symbol_table
            .resolve_symbol_str(symbol)
            .unwrap_or("")
            .to_owned(),
        value => {
            return Err(EvalError::TypeError {
                function: name.to_owned(),
                expected: if matches!(name, "eval" | "build") {
                    "SYMBOL or STRING"
                } else {
                    "STRING"
                }
                .to_owned(),
                actual: value.type_name().to_owned(),
                span: span.cloned(),
            })
        }
    };
    crate::source_limits::check_source_size(source.len())
        .map_err(|error| failure(name, &error, span))?;
    let source = source
        .split_once('\0')
        .map_or(source.as_str(), |(first, _)| first);
    let mut parsed = parse_first_sexpr(source, FileId(0));
    if !parsed.errors.is_empty() || parsed.exprs.is_empty() {
        let reason = parsed
            .errors
            .first()
            .map_or_else(|| "expected an expression".to_owned(), ToString::to_string);
        if name == "build" {
            ctx.engine
                .globals
                .push_printout_event("werror".to_owned(), format!("{reason}\n"));
            return Ok(evaluator::clips_false(
                &mut ctx.engine.symbol_table,
                ctx.engine.config.string_encoding,
            ));
        }
        return Err(failure(name, &reason, span));
    }
    let first = parsed.exprs.remove(0);
    if name == "build" {
        return build(ctx, source, &first, span);
    }
    let expression = if name == "eval" {
        interpret_action_expr(&first)
    } else {
        let first_span = first.span();
        let wrapped = SExpr::List(
            vec![
                SExpr::Atom(Atom::Symbol("assert".to_owned()), first_span),
                first,
            ],
            first_span,
        );
        interpret_action_expr(&wrapped)
    }
    .map_err(|error| failure(name, &error, span))?;
    let expression = ctx
        .engine
        .prepare_eval_expression(&expression, ctx.current_module)
        .map_err(|error| failure(name, &error, span))?;
    // Dynamic source cannot see the surrounding rule/callable's local variables.
    let bindings = ferric_rules_core::binding::BindingSet::new();
    let variables = ferric_rules_core::binding::VarMap::new();
    let mut locals = evaluator::CallableLocals::default();
    let mut child = EvalContext {
        engine: ctx.engine,
        bindings: &bindings,
        var_map: &variables,
        callable_locals: Some(&mut locals),
        call_depth: ctx.call_depth,
        expression_depth: ctx.expression_depth,
        current_module: ctx.current_module,
        global_module: ctx.global_module,
        method_chain: None,
        compact_fact_bindings: None,
        allow_engine_effects: ctx.allow_engine_effects,
    };
    evaluator::eval_inner(&mut child, &expression)
}

fn build(
    ctx: &mut EvalContext<'_>,
    source: &str,
    first: &ferric_rules_parser::SExpr,
    span: Option<&SourceSpan>,
) -> Result<Value, EvalError> {
    if !ctx.allow_engine_effects {
        return Err(failure(
            "build",
            &"engine mutation is unavailable while evaluating a match condition",
            span,
        ));
    }
    let recognized = first
        .as_list()
        .and_then(|items| items.first())
        .and_then(ferric_rules_parser::SExpr::as_symbol)
        .is_some_and(|name| {
            matches!(
                name,
                "defrule"
                    | "deftemplate"
                    | "deffacts"
                    | "deffunction"
                    | "defglobal"
                    | "defgeneric"
                    | "defmethod"
                    | "defmodule"
            )
        });
    if !recognized {
        return Ok(evaluator::clips_false(
            &mut ctx.engine.symbol_table,
            ctx.engine.config.string_encoding,
        ));
    }
    // Reentrant construct loading can invalidate the outer loader's provisional
    // definitions. The reference itself crashes for these initializer calls;
    // reject them explicitly rather than publishing inconsistent metadata.
    if ctx.engine.source_load_depth != 0 {
        return Err(failure(
            "build",
            &"building a construct during source loading is unsupported",
            span,
        ));
    }
    if let Some(reason) = active_definition_error(ctx, first) {
        ctx.engine
            .globals
            .push_printout_event("werror".to_owned(), format!("{reason}\n"));
        return Ok(evaluator::clips_false(
            &mut ctx.engine.symbol_table,
            ctx.engine.config.string_encoding,
        ));
    }
    ctx.engine
        .module_registry
        .set_current_module(ctx.current_module);
    // Defglobal initializers, rule-priming conditions, template defaults and
    // fact initializers evaluated by this load continue the caller's depth.
    let result = crate::effects::with_depth_floor(ctx, |ctx| {
        ctx.engine.load_str(&source[..first.span().end.offset])
    });
    match result {
        Ok(_) => Ok(evaluator::clips_true(
            &mut ctx.engine.symbol_table,
            ctx.engine.config.string_encoding,
        )),
        Err(errors) => {
            for error in errors {
                ctx.engine
                    .globals
                    .push_printout_event("werror".to_owned(), format!("{error}\n"));
            }
            Ok(evaluator::clips_false(
                &mut ctx.engine.symbol_table,
                ctx.engine.config.string_encoding,
            ))
        }
    }
}

fn active_definition_error(
    ctx: &EvalContext<'_>,
    first: &ferric_rules_parser::SExpr,
) -> Option<String> {
    let items = first.as_list()?;
    let kind = items.first()?.as_symbol()?;
    if !matches!(kind, "deffunction" | "defgeneric" | "defmethod" | "defrule") {
        return None;
    }
    let parsed = crate::qualified_name::parse_qualified_name(items.get(1)?.as_symbol()?).ok()?;
    let module = match parsed.module_name() {
        Some(name) => ctx.engine.module_registry.get_by_name(name)?,
        None => ctx.current_module,
    };
    let name = parsed.local_name();
    let active = if kind == "defrule" {
        ctx.engine
            .active_rules
            .iter()
            .any(|(owner, info)| *owner == module && info.name.rsplit("::").next() == Some(name))
    } else {
        ctx.engine
            .active_callables
            .iter()
            .any(|(owner, local)| *owner == module && local == name)
    };
    if !active {
        return None;
    }
    Some(match kind {
        "deffunction" => {
            format!("[DFNXPSR4] Deffunction {name} may not be redefined while it is executing.")
        }
        "defrule" => {
            format!("[CSTRCPSR4] Cannot redefine defrule {name} while it is in use.")
        }
        _ => format!(
            "[GENRCFUN1] Defgeneric {name} cannot be modified while one of its methods is executing."
        ),
    })
}
