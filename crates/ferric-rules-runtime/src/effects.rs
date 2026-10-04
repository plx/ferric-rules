//! Engine effects shared by RHS actions and ordinary expression evaluation.

use ferric_rules_core::{Fact, FactId, OrderedFact, TemplateFact, Value};
use ferric_rules_parser::{ActionExpr, FunctionCall, SlotType};

use crate::engine::{Engine, FactAssertionResult};
use crate::evaluator::{self, EvalContext, EvalError, RuntimeExpr, SourceSpan};
use crate::fact_address::{live_fact_id, make_fact_address};
use crate::loader::TemplateLookupError;
use crate::modules::ModuleId;
use crate::templates::RegisteredTemplate;

pub(crate) fn is_effect(name: &str) -> bool {
    matches!(
        name,
        "assert" | "retract" | "modify" | "duplicate" | "halt" | "focus" | "reset" | "clear"
    )
}

fn failure(name: &str, reason: impl Into<String>, span: Option<&SourceSpan>) -> EvalError {
    EvalError::UnsupportedOperation {
        operation: name.into(),
        reason: reason.into(),
        span: span.cloned(),
    }
}

fn require_effects(
    ctx: &EvalContext<'_>,
    name: &str,
    span: Option<&SourceSpan>,
) -> Result<(), EvalError> {
    if ctx.allow_engine_effects {
        Ok(())
    } else {
        Err(failure(
            name,
            "engine mutation is unavailable while evaluating a match condition",
            span,
        ))
    }
}

fn boolean(ctx: &mut EvalContext<'_>, value: bool) -> Result<Value, EvalError> {
    ctx.engine
        .symbol_table
        .intern_symbol(
            if value { "TRUE" } else { "FALSE" },
            ctx.engine.config.string_encoding,
        )
        .map(Value::Symbol)
        .map_err(|error| failure("boolean", error.to_string(), None))
}

fn arity(
    name: &str,
    actual: usize,
    minimum: usize,
    exact: bool,
    span: Option<&SourceSpan>,
) -> Result<(), EvalError> {
    if actual >= minimum && (!exact || actual == minimum) {
        return Ok(());
    }
    Err(EvalError::ArityMismatch {
        name: name.into(),
        expected: if exact {
            minimum.to_string()
        } else {
            format!("at least {minimum}")
        },
        actual,
        span: span.cloned(),
    })
}

/// Syntax wrappers name facts and slots. Only these leaves are expressions.
/// Sharing this walk keeps predicate validation from treating data heads as calls.
pub(crate) fn evaluated_arguments<'a>(
    engine: &Engine,
    module: ModuleId,
    call: &'a FunctionCall,
) -> Vec<&'a ActionExpr> {
    fn slots<'a>(fields: &'a [ActionExpr], arguments: &mut Vec<&'a ActionExpr>) {
        for field in fields {
            match field {
                ActionExpr::FunctionCall(slot) => arguments.extend(&slot.args),
                other => arguments.push(other),
            }
        }
    }
    let mut arguments = Vec::new();
    match call.name.as_str() {
        "assert" => {
            // Each fact may be ordered or explicitly templated.
            for argument in &call.args {
                if let ActionExpr::FunctionCall(fact) = argument {
                    if engine.resolve_template_id(&fact.name, module).is_ok() {
                        slots(&fact.args, &mut arguments);
                    } else {
                        arguments.extend(&fact.args);
                    }
                } else {
                    arguments.push(argument);
                }
            }
        }
        "modify" | "duplicate" => {
            if let Some((target, overrides)) = call.args.split_first() {
                slots(overrides, &mut arguments);
                arguments.insert(0, target);
            }
        }
        _ => arguments.extend(&call.args),
    }
    arguments
}

pub(crate) fn eval_call(
    ctx: &mut EvalContext<'_>,
    name: &str,
    args: &[RuntimeExpr],
    span: Option<&SourceSpan>,
) -> Result<Value, EvalError> {
    require_effects(ctx, name, span)?;
    match name {
        "halt" | "reset" | "clear" => {
            arity(name, args.len(), 0, true, span)?;
            match name {
                "halt" => ctx.engine.halt(),
                "reset" => ctx
                    .engine
                    .reset_for_evaluation()
                    .map_err(|error| failure(name, error.to_string(), span))?,
                "clear" => ctx
                    .engine
                    .clear_for_evaluation()
                    .map_err(|error| failure(name, error.to_string(), span))?,
                _ => unreachable!(),
            }
            Ok(Value::Void)
        }
        "retract" => {
            arity(name, args.len(), 1, false, span)?;
            for expression in args {
                let value = evaluator::eval_inner(ctx, expression)?;
                if let Some(id) = resolve_target(ctx, name, &value, span)? {
                    retract(ctx.engine, id);
                }
            }
            Ok(Value::Void)
        }
        "focus" => {
            arity(name, args.len(), 1, false, span)?;
            // CLIPS evaluates and pushes right to left. A later failure keeps
            // earlier pushes and skips the remaining operand expressions.
            for expression in args.iter().rev() {
                let value = evaluator::eval_inner(ctx, expression)?;
                let Value::Symbol(symbol) = value else {
                    return Err(EvalError::TypeError {
                        function: name.into(),
                        expected: "SYMBOL module name".into(),
                        actual: value.type_name().into(),
                        span: span.cloned(),
                    });
                };
                let name = ctx
                    .engine
                    .symbol_table
                    .resolve_symbol_str(symbol)
                    .ok_or_else(|| failure("focus", "invalid module symbol", span))?;
                let Some(module) = ctx.engine.module_registry.get_by_name(name) else {
                    return boolean(ctx, false);
                };
                ctx.engine.module_registry.push_focus(module);
            }
            boolean(ctx, true)
        }
        _ => Err(failure(name, "unknown engine effect", span)),
    }
}

fn eval_source(ctx: &mut EvalContext<'_>, expression: &ActionExpr) -> Result<Value, EvalError> {
    let expression =
        evaluator::from_action_expr(expression, &mut ctx.engine.symbol_table, &ctx.engine.config)?;
    evaluator::eval_inner(ctx, &expression)
}

fn eval_fields(
    ctx: &mut EvalContext<'_>,
    expressions: &[ActionExpr],
) -> Result<Vec<Value>, EvalError> {
    let mut values = Vec::new();
    for expression in expressions {
        match eval_source(ctx, expression)? {
            Value::Multifield(fields) => values.extend(fields.iter().cloned()),
            Value::Void => {}
            value => values.push(value),
        }
    }
    Ok(values)
}

fn apply_slots(
    ctx: &mut EvalContext<'_>,
    name: &str,
    template: &RegisteredTemplate,
    slots: &mut [Value],
    overrides: &[ActionExpr],
    span: Option<&SourceSpan>,
) -> Result<(), EvalError> {
    let overrides = template
        .slot_overrides(overrides, &ctx.engine.symbol_table)
        .map_err(|error| failure(name, error, span))?;
    for (index, call) in overrides {
        slots[index] = match template.slot_types[index] {
            SlotType::Single => {
                let value = eval_source(ctx, &call.args[0])?;
                if matches!(value, Value::Multifield(_) | Value::Void) {
                    return Err(failure(
                        name,
                        format!(
                            "single-field slot `{}` requires one scalar value",
                            call.name
                        ),
                        span,
                    ));
                }
                value
            }
            SlotType::Multi => Value::Multifield(Box::new(
                eval_fields(ctx, &call.args)?.into_iter().collect(),
            )),
        };
    }
    template
        .validate_slots(slots)
        .map_err(|error| failure(name, error, span))
}

fn assert_result(
    ctx: &mut EvalContext<'_>,
    name: &str,
    fact: Fact,
    span: Option<&SourceSpan>,
) -> Result<Value, EvalError> {
    match ctx
        .engine
        .assert_fact_internal(fact)
        .map_err(|error| failure(name, error.to_string(), span))?
    {
        FactAssertionResult::Duplicate(_) => boolean(ctx, false),
        FactAssertionResult::Asserted(id) => make_fact_address(
            &ctx.engine.fact_base,
            ctx.engine.initial_fact_id,
            ctx.engine.fact_epoch,
            ctx.engine.fact_index_starts_at_zero,
            id,
        )
        .map(Value::FactAddress)
        .ok_or_else(|| failure(name, "asserted fact disappeared during propagation", span)),
    }
}

pub(crate) fn eval_syntax(
    ctx: &mut EvalContext<'_>,
    call: &FunctionCall,
) -> Result<Value, EvalError> {
    let span = SourceSpan {
        line: call.span.start.line,
        column: call.span.start.column,
    };
    let span = Some(&span);
    let name = call.name.as_str();
    require_effects(ctx, name, span)?;
    arity(name, call.args.len(), 1, false, span)?;
    match name {
        "assert" => eval_assert(ctx, call, span),
        "modify" | "duplicate" => {
            let target = eval_source(ctx, &call.args[0])?;
            let Some(id) = resolve_target(ctx, name, &target, span)? else {
                return boolean(ctx, false);
            };
            let address = make_fact_address(
                &ctx.engine.fact_base,
                ctx.engine.initial_fact_id,
                ctx.engine.fact_epoch,
                ctx.engine.fact_index_starts_at_zero,
                id,
            )
            .expect("resolved target is live");
            let mut fact = ctx
                .engine
                .fact_base
                .get(id)
                .expect("resolved target is live")
                .fact
                .clone();
            match &mut fact {
                Fact::Template(fact) => {
                    let template = ctx
                        .engine
                        .template_defs
                        .get(fact.template_id)
                        .cloned()
                        .ok_or_else(|| failure(name, "target has unknown template", span))?;
                    apply_slots(ctx, name, &template, &mut fact.slots, &call.args[1..], span)?;
                }
                Fact::Ordered(fact) => {
                    // Preserve Ferric's existing positional ordered-fact overrides.
                    for override_expression in &call.args[1..] {
                        if let ActionExpr::FunctionCall(field) = override_expression {
                            if let (Ok(index), Some(value)) =
                                (field.name.parse::<usize>(), field.args.first())
                            {
                                if index < fact.fields.len() {
                                    fact.fields[index] = eval_source(ctx, value)?;
                                }
                            }
                        }
                    }
                }
            }
            if name == "modify" {
                ctx.engine
                    .fact_base
                    .ensure_assertion_capacity()
                    .map_err(|error| failure(name, error.to_string(), span))?;
                // A field expression may reset/retract the target. Never apply
                // its old slot key to a replacement fact in the new epoch.
                if let Some(id) =
                    live_fact_id(&ctx.engine.fact_base, ctx.engine.fact_epoch, &address)
                {
                    retract(ctx.engine, id);
                }
            }
            assert_result(ctx, name, fact, span)
        }
        _ => Err(failure(name, "unknown syntax effect", span)),
    }
}

fn eval_assert(
    ctx: &mut EvalContext<'_>,
    call: &FunctionCall,
    span: Option<&SourceSpan>,
) -> Result<Value, EvalError> {
    let name = "assert";
    let mut result = boolean(ctx, false)?;
    for argument in &call.args {
        let ActionExpr::FunctionCall(pattern) = argument else {
            return Err(failure(name, "expected a fact pattern", span));
        };
        let fact = match ctx
            .engine
            .resolve_template_id(&pattern.name, ctx.current_module)
        {
            Ok(id) => {
                let definition = ctx.engine.template_defs[id].clone();
                let validated = definition
                    .slot_overrides(&pattern.args, &ctx.engine.symbol_table)
                    .map_err(|error| failure(name, error, span))?;
                let overrides = validated
                    .into_iter()
                    .map(|(index, slot)| {
                        let fields = slot
                            .args
                            .iter()
                            .map(|field| {
                                evaluator::from_action_expr(
                                    field,
                                    &mut ctx.engine.symbol_table,
                                    &ctx.engine.config,
                                )
                            })
                            .collect::<Result<Vec<_>, _>>()?;
                        Ok((index, fields))
                    })
                    .collect::<Result<Vec<_>, EvalError>>()?;
                let slots = crate::template_defaults::evaluate_slots(ctx, &definition, &overrides)
                    .map_err(|error| error.error)?;
                Fact::Template(TemplateFact {
                    template_id: id,
                    slots: slots.into_boxed_slice(),
                })
            }
            Err(TemplateLookupError::Unknown) => {
                let relation = ctx
                    .engine
                    .symbol_table
                    .intern_symbol(&pattern.name, ctx.engine.config.string_encoding)
                    .map_err(|error| failure(name, error.to_string(), span))?;
                Fact::Ordered(OrderedFact {
                    relation,
                    fields: eval_fields(ctx, &pattern.args)?.into(),
                })
            }
            Err(error) => {
                return Err(failure(
                    name,
                    format!("template `{}` is unavailable: {error:?}", pattern.name),
                    span,
                ))
            }
        };
        result = assert_result(ctx, name, fact, span)?;
    }
    Ok(result)
}

fn resolve_target(
    ctx: &EvalContext<'_>,
    name: &str,
    value: &Value,
    span: Option<&SourceSpan>,
) -> Result<Option<FactId>, EvalError> {
    if !matches!(value, Value::Integer(_) | Value::FactAddress(_)) {
        return Err(EvalError::TypeError {
            function: name.into(),
            expected: "fact-address or INTEGER fact index".into(),
            actual: value.type_name().into(),
            span: span.cloned(),
        });
    }
    let id = evaluator::designated_fact(
        &ctx.engine.fact_base,
        ctx.engine.initial_fact_id,
        ctx.engine.fact_index_starts_at_zero,
        ctx.engine.fact_epoch,
        value,
    );
    if id.is_some() && id == ctx.engine.initial_fact_id {
        return Err(failure(
            name,
            "the internal initial-fact is protected",
            span,
        ));
    }
    Ok(id)
}

fn retract(engine: &mut Engine, id: FactId) {
    if let Some(entry) = engine.fact_base.get(id) {
        engine.rete.retract_fact(id, &entry.fact, &engine.fact_base);
        engine.fact_base.retract(id);
        engine.host.remove(id);
        engine.drain_network_events();
    }
}
