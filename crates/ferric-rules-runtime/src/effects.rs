//! Engine effects shared by RHS actions and ordinary expression evaluation.

use ferric_rules_core::{Fact, FactId, OrderedFact, TemplateFact, Value};
use ferric_rules_parser::{ActionExpr, FunctionCall, SlotType};

use crate::engine::{Engine, FactAssertionResult, FactIdentity};
use crate::evaluator::{self, EvalContext, EvalError, RuntimeExpr, SourceSpan};
use crate::fact_address::{live_fact_id, make_fact_address};
use crate::loader::TemplateLookupError;
use crate::modules::ModuleId;
use crate::template_defaults;
use crate::templates::RegisteredTemplate;

pub(crate) fn is_effect(name: &str) -> bool {
    matches!(
        name,
        "assert"
            | "retract"
            | "modify"
            | "duplicate"
            | "halt"
            | "focus"
            | "reset"
            | "clear"
            | "load-facts"
            | "save-facts"
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

/// Run an engine effect with the evaluator depth of its call site as the
/// floor for every evaluation root the effect opens. A match condition,
/// deffacts or defglobal initializer evaluated by the effect then counts
/// against the same call and expression limits instead of starting at zero,
/// which keeps the native stack bounded however effects nest. `build` uses it
/// for the initializers and match conditions its loading evaluates.
pub(crate) fn with_depth_floor<T>(
    ctx: &mut EvalContext<'_>,
    effect: impl FnOnce(&mut EvalContext<'_>) -> T,
) -> T {
    let floor = (ctx.call_depth, ctx.expression_depth);
    let previous = std::mem::replace(&mut ctx.engine.eval_depth_floor, floor);
    let result = effect(ctx);
    ctx.engine.eval_depth_floor = previous;
    result
}

/// Whether `ctx` evaluates a top-level expression (or `eval` source it runs)
/// rather than a rule, callable body or fact initializer.
fn is_root_context(ctx: &EvalContext<'_>) -> bool {
    ctx.call_depth == 0
        && ctx.engine.active_rules.is_empty()
        && ctx.engine.active_fact_initializers == 0
        && ctx.engine.active_expression_scopes != 0
}

/// The module that dynamic source (`eval`, `assert-string`, `build`, fact
/// files) and run-time name lookups resolve in. A root expression follows the
/// engine's current module, as CLIPS's top-level commands follow its global
/// one: a root `reset` or `clear` selects MAIN and a built `defmodule` selects
/// itself, while the expression's own parsed references stay bound to the
/// module it was parsed in.
pub(crate) fn dynamic_module(ctx: &EvalContext<'_>) -> ModuleId {
    if is_root_context(ctx) {
        ctx.engine.module_registry.current_module()
    } else {
        ctx.current_module
    }
}

/// The module `bind` resolves an unqualified defglobal in. CLIPS binds the
/// global a command names when it parses the command, so this is the
/// expression's own module, unless a root `clear` has deleted that module.
pub(crate) fn bind_module(ctx: &EvalContext<'_>) -> ModuleId {
    if ctx.engine.root_cleared && is_root_context(ctx) {
        ctx.engine.module_registry.current_module()
    } else {
        ctx.current_module
    }
}

pub(crate) fn eval_call(
    ctx: &mut EvalContext<'_>,
    name: &str,
    args: &[RuntimeExpr],
    span: Option<&SourceSpan>,
) -> Result<Value, EvalError> {
    with_depth_floor(ctx, |ctx| eval_call_inner(ctx, name, args, span))
}

fn eval_call_inner(
    ctx: &mut EvalContext<'_>,
    name: &str,
    args: &[RuntimeExpr],
    span: Option<&SourceSpan>,
) -> Result<Value, EvalError> {
    require_effects(ctx, name, span)?;
    match name {
        "load-facts" | "save-facts" => crate::fact_io::eval_call(ctx, name, args, span),
        "halt" | "reset" | "clear" => {
            arity(name, args.len(), 0, true, span)?;
            match name {
                "halt" => ctx.engine.halt(),
                "reset" => {
                    ctx.engine
                        .reset_for_evaluation()
                        .map_err(|error| failure(name, error.to_string(), span))?;
                }
                "clear" => {
                    if ctx
                        .engine
                        .clear_for_evaluation()
                        .map_err(|error| failure(name, error.to_string(), span))?
                    {
                        let main = ctx.engine.module_registry.current_module();
                        // Enclosing contexts of the expression still hold the
                        // deleted module; see `bind_module`.
                        if is_root_context(ctx) {
                            ctx.engine.root_cleared = true;
                        }
                        // The clear deleted the expression's module, and module
                        // ids restart. It refused if the expression names a
                        // template, fact or user callable, so nothing left in
                        // it is bound to the old module.
                        ctx.current_module = main;
                        ctx.global_module = None;
                    }
                }
                _ => unreachable!(),
            }
            Ok(Value::Void)
        }
        "retract" => {
            arity(name, args.len(), 1, false, span)?;
            // CLIPS 6.30's `retract` skips a missing fact, stops at a negative
            // index without evaluating later targets (execution continues),
            // and reports a wrong-type target only after retracting the
            // remaining targets.
            let mut wrong_type = None;
            for expression in args {
                // Once a wrong-type target has set CLIPS's halt flag, a later
                // deffunction or generic target returns FALSE without running.
                // Builtins, variables and literals still evaluate. Only a
                // top-level call is checked: a user callable nested inside a
                // builtin target or called through `funcall` still runs.
                if wrong_type.is_some() && calls_user_callable(ctx, expression) {
                    continue;
                }
                let value = evaluator::eval_inner(ctx, expression)?;
                match resolve_target(ctx, name, &value, span)? {
                    FactTarget::Live(id) => retract(ctx.engine, id),
                    FactTarget::MissingIndex | FactTarget::StaleAddress => {}
                    FactTarget::NegativeIndex => break,
                    FactTarget::WrongType => {
                        wrong_type.get_or_insert_with(|| wrong_target_type(name, &value, span));
                    }
                }
            }
            wrong_type.map_or(Ok(Value::Void), Err)
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
    // The target's template is held in use while its slots are evaluated,
    // so the layouts agree; report rather than index a mismatched fact.
    if slots.len() != template.slot_types.len() {
        return Err(failure(
            name,
            "target fact does not match its template's slots",
            span,
        ));
    }
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

/// Hold a fact's template or ordered relation in use from the moment the
/// fact is captured until it is published (see [`Engine::with_active_fact`]).
fn with_active_fact<T>(
    ctx: &mut EvalContext<'_>,
    identity: FactIdentity,
    assemble: impl FnOnce(&mut EvalContext<'_>) -> Result<T, EvalError>,
) -> Result<T, EvalError> {
    let templates = ctx.engine.active_templates.len();
    let relations = ctx.engine.active_ordered_relations.len();
    match identity {
        FactIdentity::Template(id) => ctx.engine.active_templates.push(id),
        FactIdentity::Ordered(relation) => ctx.engine.active_ordered_relations.push(relation),
    }
    let result = assemble(ctx);
    ctx.engine.active_templates.truncate(templates);
    ctx.engine.active_ordered_relations.truncate(relations);
    result
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
    with_depth_floor(ctx, |ctx| {
        ctx.engine.active_fact_initializers += 1;
        let result = eval_syntax_inner(ctx, call);
        ctx.engine.active_fact_initializers -= 1;
        result
    })
}

fn eval_syntax_inner(ctx: &mut EvalContext<'_>, call: &FunctionCall) -> Result<Value, EvalError> {
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
            // CLIPS 6.30 reports a missing index and continues without
            // evaluating or applying the slot overrides; other unresolved
            // targets stop execution.
            let id = match resolve_target(ctx, name, &target, span)? {
                FactTarget::Live(id) => id,
                FactTarget::MissingIndex => return boolean(ctx, false),
                FactTarget::StaleAddress => {
                    return Err(failure(name, "target fact does not exist", span))
                }
                FactTarget::NegativeIndex => {
                    return Err(failure(name, "fact index must not be negative", span))
                }
                FactTarget::WrongType => return Err(wrong_target_type(name, &target, span)),
            };
            let address = make_fact_address(
                &ctx.engine.fact_base,
                ctx.engine.initial_fact_id,
                ctx.engine.fact_epoch,
                ctx.engine.fact_index_starts_at_zero,
                id,
            )
            .expect("resolved target is live");
            let fact = ctx
                .engine
                .fact_base
                .get(id)
                .expect("resolved target is live")
                .fact
                .clone();
            // A slot expression may retract the original first, so the live
            // fact alone does not keep its template or relation in use until
            // publication.
            with_active_fact(ctx, FactIdentity::of(&fact), |ctx| {
                replace_fact(ctx, name, call, fact, &address, span)
            })
        }
        _ => Err(failure(name, "unknown syntax effect", span)),
    }
}

/// Apply `modify`/`duplicate` slot overrides to a copy of the target and
/// publish it, retracting the original for `modify`.
fn replace_fact(
    ctx: &mut EvalContext<'_>,
    name: &str,
    call: &FunctionCall,
    mut fact: Fact,
    address: &ferric_rules_core::FactAddress,
    span: Option<&SourceSpan>,
) -> Result<Value, EvalError> {
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
        if let Some(id) = live_fact_id(&ctx.engine.fact_base, ctx.engine.fact_epoch, address) {
            retract(ctx.engine, id);
        }
    }
    assert_result(ctx, name, fact, span)
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
        result = match ctx
            .engine
            .resolve_template_id(&pattern.name, ctx.current_module)
        {
            Ok(id) => with_active_fact(ctx, FactIdentity::Template(id), |ctx| {
                let definition = ctx.engine.template_defs[id].clone();
                let validated = definition
                    .slot_overrides(&pattern.args, &ctx.engine.symbol_table)
                    .map_err(|error| failure(name, error, span))?;
                let mut sources = template_defaults::default_sources(&definition);
                for (index, slot) in validated {
                    sources[index] = template_defaults::SlotSource::Actions(&slot.args);
                }
                let slots = template_defaults::evaluate_slots(ctx, &definition, sources).map_err(
                    |error| match error.failure {
                        template_defaults::SlotFailure::Invalid(reason) => {
                            failure(name, reason, span)
                        }
                        template_defaults::SlotFailure::Eval(error) => error,
                    },
                )?;
                let fact = Fact::Template(TemplateFact {
                    template_id: id,
                    slots: slots.into_boxed_slice(),
                });
                assert_result(ctx, name, fact, span)
            })?,
            Err(TemplateLookupError::Unknown) => {
                let relation = ctx
                    .engine
                    .symbol_table
                    .intern_symbol(&pattern.name, ctx.engine.config.string_encoding)
                    .map_err(|error| failure(name, error.to_string(), span))?;
                with_active_fact(ctx, FactIdentity::Ordered(relation), |ctx| {
                    let fact = Fact::Ordered(OrderedFact {
                        relation,
                        fields: eval_fields(ctx, &pattern.args)?.into(),
                    });
                    assert_result(ctx, name, fact, span)
                })?
            }
            Err(error) => {
                return Err(failure(
                    name,
                    format!("template `{}` is unavailable: {error:?}", pattern.name),
                    span,
                ))
            }
        };
    }
    Ok(result)
}

/// What a fact-effect target designates. Each effect decides which of the
/// non-live outcomes are recoverable, following CLIPS 6.30.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum FactTarget {
    Live(FactId),
    /// A non-negative INTEGER index that names no live fact.
    MissingIndex,
    /// A fact address whose fact was retracted (or a dummy address).
    StaleAddress,
    NegativeIndex,
    /// Neither a fact address nor an INTEGER index.
    WrongType,
}

fn wrong_target_type(name: &str, value: &Value, span: Option<&SourceSpan>) -> EvalError {
    EvalError::TypeError {
        function: name.into(),
        expected: "fact-address or INTEGER fact index".into(),
        actual: value.type_name().into(),
        span: span.cloned(),
    }
}

fn resolve_target(
    ctx: &EvalContext<'_>,
    name: &str,
    value: &Value,
    span: Option<&SourceSpan>,
) -> Result<FactTarget, EvalError> {
    let missing = match value {
        Value::FactAddress(_) => FactTarget::StaleAddress,
        Value::Integer(index) if *index < 0 => return Ok(FactTarget::NegativeIndex),
        Value::Integer(_) => FactTarget::MissingIndex,
        _ => return Ok(FactTarget::WrongType),
    };
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
    Ok(id.map_or(missing, FactTarget::Live))
}

/// Whether `expression` is a top-level call that runs a deffunction or
/// defgeneric rather than a builtin.
fn calls_user_callable(ctx: &EvalContext<'_>, expression: &RuntimeExpr) -> bool {
    matches!(expression, RuntimeExpr::Call { name, .. }
        if evaluator::call_names_user_callable(ctx, name))
}

fn retract(engine: &mut Engine, id: FactId) {
    engine.trace_fact(id, false);
    if let Some(entry) = engine.fact_base.get(id) {
        engine.rete.retract_fact(id, &entry.fact, &engine.fact_base);
        engine.fact_base.retract(id);
        engine.host.remove(id);
        engine.drain_network_events();
    }
}
