//! Slot evaluation shared by source assertions, dormant seeds, and host assertions.

use ferric_rules_core::binding::{BindingSet, VarMap};
use ferric_rules_core::Value;
use ferric_rules_parser::{ActionExpr, SlotType};

use crate::evaluator::{self, CallableLocals, EvalContext, EvalError, RuntimeExpr};
use crate::modules::ModuleId;
use crate::templates::{DynamicSlotDefault, RegisteredTemplate};
use crate::Engine;

/// Where one slot's value comes from. Callers list one source per slot in
/// declaration order, so explicit fields and omitted defaults interleave.
pub(crate) enum SlotSource<'a> {
    /// A host value the caller has already checked against the slot.
    Supplied(Value),
    /// Prepared runtime expressions, such as a deffacts slot.
    Exprs(&'a [RuntimeExpr]),
    /// Source fields of an RHS assertion, converted when their slot is reached.
    Actions(&'a [ActionExpr]),
    /// The template default: a dynamic default is evaluated, a static one copied.
    Default,
}

/// Each entry point reports a constraint or shape violation in its own form;
/// evaluation failures keep their original error.
pub(crate) enum SlotFailure {
    Invalid(String),
    Eval(EvalError),
}

impl From<EvalError> for SlotFailure {
    fn from(error: EvalError) -> Self {
        Self::Eval(error)
    }
}

pub(crate) struct SlotEvaluationError {
    pub index: usize,
    pub failure: SlotFailure,
}

/// How a void result (such as `printout`'s) is treated in a slot expression.
#[derive(Clone, Copy, PartialEq, Eq)]
enum VoidPolicy {
    /// Supplied slot fields: a void multislot element is omitted.
    Omit,
    /// Dynamic defaults: a void single-field value becomes `nil`, and a void
    /// multislot element is omitted.
    NilOrOmit,
    /// Static defaults: CLIPS 6.30 rejects any void result (CSTRNCHK1).
    Reject,
}

/// Evaluate one complete slot; multifield expressions splice into the aggregate.
/// Recursive calls retain the active evaluator depth and shared execution budget.
fn evaluate_expressions(
    ctx: &mut EvalContext<'_>,
    slot_type: SlotType,
    name: &str,
    expressions: &[RuntimeExpr],
    void_policy: VoidPolicy,
) -> Result<Value, SlotFailure> {
    if slot_type == SlotType::Single {
        let [expression] = expressions else {
            return Err(SlotFailure::Invalid(format!(
                "single-field slot `{name}` requires exactly one scalar expression"
            )));
        };
        let mut value = evaluator::eval_inner(ctx, expression)?;
        if void_policy == VoidPolicy::NilOrOmit && matches!(value, Value::Void) {
            value = Value::Symbol(
                ctx.engine
                    .symbol_table
                    .intern_symbol("nil", ctx.engine.config.string_encoding)
                    .map_err(|error| SlotFailure::Invalid(error.to_string()))?,
            );
        }
        if matches!(value, Value::Multifield(_) | Value::Void) {
            return Err(SlotFailure::Invalid(format!(
                "single-field slot `{name}` requires one scalar value"
            )));
        }
        return Ok(value);
    }
    let mut fields = ferric_rules_core::Multifield::new();
    let mut produced_void = false;
    for expression in expressions {
        match evaluator::eval_inner(ctx, expression)? {
            Value::Multifield(values) => fields.extend(values.iter().cloned()),
            Value::Void => produced_void = true,
            value => fields.push(value),
        }
    }
    // Like CLIPS, every element runs before the whole default is checked.
    if produced_void && void_policy == VoidPolicy::Reject {
        return Err(SlotFailure::Invalid(format!(
            "static default for multislot `{name}` produced no value"
        )));
    }
    Ok(Value::Multifield(Box::new(fields)))
}

fn evaluate_actions(
    ctx: &mut EvalContext<'_>,
    slot_type: SlotType,
    name: &str,
    fields: &[ActionExpr],
) -> Result<Value, SlotFailure> {
    let expressions = fields
        .iter()
        .map(|field| {
            evaluator::from_action_expr(field, &mut ctx.engine.symbol_table, &ctx.engine.config)
        })
        .collect::<Result<Vec<_>, _>>()?;
    evaluate_expressions(ctx, slot_type, name, &expressions, VoidPolicy::Omit)
}

fn evaluate_dynamic(
    ctx: &mut EvalContext<'_>,
    default: &DynamicSlotDefault,
    slot_type: SlotType,
    name: &str,
) -> Result<Value, SlotFailure> {
    let bindings = BindingSet::new();
    let variables = VarMap::new();
    let mut locals = CallableLocals::default();
    // CLIPS binds callable/template references at definition, but a direct
    // global reference observes the assertion caller. A called function gets
    // its own ordinary lexical global scope from execute_callable_body.
    let mut child = EvalContext {
        global_module: Some(ctx.global_module.unwrap_or(ctx.current_module)),
        engine: ctx.engine,
        bindings: &bindings,
        var_map: &variables,
        callable_locals: Some(&mut locals),
        call_depth: ctx.call_depth,
        expression_depth: ctx.expression_depth,
        current_module: default.module,
        method_chain: None,
        compact_fact_bindings: None,
        allow_engine_effects: ctx.allow_engine_effects,
    };
    // A default is not lexically inside the asserting callable or loop, so a
    // `return` or `break` reached through `funcall` must not escape into it.
    evaluate_expressions(
        &mut child,
        slot_type,
        name,
        &default.expressions,
        VoidPolicy::NilOrOmit,
    )
    .map_err(|failure| match failure {
        SlotFailure::Eval(error) => SlotFailure::Eval(evaluator::contain_control_signals(error)),
        invalid @ SlotFailure::Invalid(_) => invalid,
    })
}

/// One source per slot for a prepared fact, whose overrides may be stored out
/// of declaration order (older snapshots) and are checked for duplicates here.
pub(crate) fn prepared_sources<'a>(
    template: &RegisteredTemplate,
    overrides: &'a [(usize, Vec<RuntimeExpr>)],
) -> Result<Vec<SlotSource<'a>>, SlotEvaluationError> {
    let mut sources = default_sources(template);
    for (index, expressions) in overrides {
        let entry = sources.get_mut(*index).ok_or_else(|| SlotEvaluationError {
            index: *index,
            failure: SlotFailure::Invalid(format!(
                "unknown slot position {index} in template `{}`",
                template.name
            )),
        })?;
        if !matches!(
            std::mem::replace(entry, SlotSource::Exprs(expressions)),
            SlotSource::Default
        ) {
            return Err(SlotEvaluationError {
                index: *index,
                failure: SlotFailure::Invalid(format!(
                    "duplicate slot `{}` in template `{}`",
                    template.slot_names[*index], template.name
                )),
            });
        }
    }
    Ok(sources)
}

/// Every slot of `template` taking its default.
pub(crate) fn default_sources<'a>(template: &RegisteredTemplate) -> Vec<SlotSource<'a>> {
    std::iter::repeat_with(|| SlotSource::Default)
        .take(template.slot_names.len())
        .collect()
}

/// Evaluate every slot in declaration order and validate each computed value.
/// Supplied values were checked by the caller and are moved into place as-is.
pub(crate) fn evaluate_slots(
    ctx: &mut EvalContext<'_>,
    template: &RegisteredTemplate,
    sources: Vec<SlotSource<'_>>,
) -> Result<Vec<Value>, SlotEvaluationError> {
    let owns_budget = ctx.engine.config.begin_action_loop_budget_if_inactive();
    let result = evaluate_slots_inner(ctx, template, sources);
    if owns_budget {
        ctx.engine.config.end_action_loop_budget();
    }
    result
}

fn evaluate_slots_inner(
    ctx: &mut EvalContext<'_>,
    template: &RegisteredTemplate,
    sources: Vec<SlotSource<'_>>,
) -> Result<Vec<Value>, SlotEvaluationError> {
    debug_assert_eq!(sources.len(), template.slot_names.len());
    let mut values = Vec::with_capacity(sources.len());
    for (index, source) in sources.into_iter().enumerate() {
        let name = &template.slot_names[index];
        let slot_type = template.slot_types[index];
        let result = match source {
            SlotSource::Supplied(value) => {
                values.push(value);
                continue;
            }
            SlotSource::Exprs(expressions) => {
                evaluate_expressions(ctx, slot_type, name, expressions, VoidPolicy::Omit)
            }
            SlotSource::Actions(fields) => evaluate_actions(ctx, slot_type, name, fields),
            SlotSource::Default => match &template.dynamic_defaults[index] {
                Some(default) => evaluate_dynamic(ctx, default, slot_type, name),
                None => Ok(template.defaults[index].clone()),
            },
        };
        let value = result.map_err(|failure| SlotEvaluationError { index, failure })?;
        template
            .validate_slot(index, &value)
            .map_err(|reason| SlotEvaluationError {
                index,
                failure: SlotFailure::Invalid(reason),
            })?;
        values.push(value);
    }
    Ok(values)
}

impl Engine {
    /// A root evaluation frame for host assertions and definition-time defaults.
    pub(crate) fn evaluate_template_defaults(
        &mut self,
        template: &RegisteredTemplate,
        sources: Vec<SlotSource<'_>>,
        module: ModuleId,
    ) -> Result<Vec<Value>, SlotEvaluationError> {
        self.with_default_context(module, |ctx| evaluate_slots(ctx, template, sources))
    }

    pub(crate) fn evaluate_static_default(
        &mut self,
        slot_type: SlotType,
        name: &str,
        expressions: &[RuntimeExpr],
        module: ModuleId,
    ) -> Result<Value, String> {
        self.with_default_context(module, |ctx| {
            evaluate_expressions(ctx, slot_type, name, expressions, VoidPolicy::Reject)
        })
        .map_err(|failure| match failure {
            SlotFailure::Invalid(reason) => reason,
            // Static defaults evaluate at a root: contain escaped control signals.
            SlotFailure::Eval(error) => evaluator::contain_control_signals(error).to_string(),
        })
    }

    fn with_default_context<T>(
        &mut self,
        module: ModuleId,
        evaluate: impl FnOnce(&mut EvalContext<'_>) -> T,
    ) -> T {
        let owns_budget = self.config.begin_action_loop_budget_if_inactive();
        let bindings = BindingSet::new();
        let variables = VarMap::new();
        let mut locals = CallableLocals::default();
        // A template defined or asserted by an engine effect counts against
        // the effect's evaluator depth instead of starting a fresh root.
        let (call_depth, expression_depth) = self.eval_depth_floor;
        let mut ctx = EvalContext {
            global_module: None,
            engine: self,
            bindings: &bindings,
            var_map: &variables,
            callable_locals: Some(&mut locals),
            call_depth,
            expression_depth,
            current_module: module,
            method_chain: None,
            compact_fact_bindings: None,
            allow_engine_effects: true,
        };
        let result = evaluate(&mut ctx);
        if owns_budget {
            self.config.end_action_loop_budget();
        }
        self.flush_expression_output();
        result
    }
}
