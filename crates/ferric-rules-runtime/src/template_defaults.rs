//! Slot evaluation shared by source assertions, dormant seeds, and host assertions.

use ferric_rules_core::binding::{BindingSet, VarMap};
use ferric_rules_core::Value;
use ferric_rules_parser::SlotType;

use crate::evaluator::{self, CallableLocals, EvalContext, EvalError, RuntimeExpr};
use crate::modules::ModuleId;
use crate::templates::{DynamicSlotDefault, RegisteredTemplate};
use crate::Engine;

pub(crate) struct SlotEvaluationError {
    pub index: usize,
    pub error: EvalError,
}

fn invalid_slot(name: &str, reason: impl Into<String>) -> EvalError {
    EvalError::UnsupportedOperation {
        operation: format!("template slot `{name}`"),
        reason: reason.into(),
        span: None,
    }
}

/// Evaluate one complete slot; multifield expressions splice into the aggregate.
/// Recursive calls retain the active evaluator depth and shared execution budget.
fn evaluate_expressions(
    ctx: &mut EvalContext<'_>,
    slot_type: SlotType,
    name: &str,
    expressions: &[RuntimeExpr],
    void_as_nil: bool,
) -> Result<Value, EvalError> {
    if slot_type == SlotType::Single {
        let [expression] = expressions else {
            return Err(invalid_slot(name, "requires exactly one scalar expression"));
        };
        let mut value = evaluator::eval_inner(ctx, expression)?;
        if void_as_nil && matches!(value, Value::Void) {
            value = Value::Symbol(
                ctx.engine
                    .symbol_table
                    .intern_symbol("nil", ctx.engine.config.string_encoding)
                    .map_err(|error| invalid_slot(name, error.to_string()))?,
            );
        }
        if matches!(value, Value::Multifield(_) | Value::Void) {
            return Err(invalid_slot(name, "requires one scalar value"));
        }
        return Ok(value);
    }
    let mut fields = ferric_rules_core::Multifield::new();
    for expression in expressions {
        match evaluator::eval_inner(ctx, expression)? {
            Value::Multifield(values) => fields.extend(values.iter().cloned()),
            Value::Void => {}
            value => fields.push(value),
        }
    }
    Ok(Value::Multifield(Box::new(fields)))
}

fn evaluate_dynamic(
    ctx: &mut EvalContext<'_>,
    default: &DynamicSlotDefault,
    slot_type: SlotType,
    name: &str,
) -> Result<Value, EvalError> {
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
    evaluate_expressions(&mut child, slot_type, name, &default.expressions, true)
}

/// Explicit fields and missing defaults interleave in slot declaration order.
/// Every caller supplies indexed overrides already checked for duplicate slots.
pub(crate) fn evaluate_slots(
    ctx: &mut EvalContext<'_>,
    template: &RegisteredTemplate,
    overrides: &[(usize, Vec<RuntimeExpr>)],
) -> Result<Vec<Value>, SlotEvaluationError> {
    let owns_budget = ctx.engine.config.begin_action_loop_budget_if_inactive();
    let result = evaluate_slots_inner(ctx, template, overrides);
    if owns_budget {
        ctx.engine.config.end_action_loop_budget();
    }
    result
}

fn evaluate_slots_inner(
    ctx: &mut EvalContext<'_>,
    template: &RegisteredTemplate,
    overrides: &[(usize, Vec<RuntimeExpr>)],
) -> Result<Vec<Value>, SlotEvaluationError> {
    let mut by_index = vec![None; template.slot_names.len()];
    for (index, expressions) in overrides {
        let entry = by_index
            .get_mut(*index)
            .ok_or_else(|| SlotEvaluationError {
                index: *index,
                error: invalid_slot(&template.name, "unknown slot position"),
            })?;
        if entry.replace(expressions.as_slice()).is_some() {
            return Err(SlotEvaluationError {
                index: *index,
                error: invalid_slot(&template.slot_names[*index], "duplicate slot"),
            });
        }
    }
    let mut values = Vec::with_capacity(by_index.len());
    for (index, explicit) in by_index.into_iter().enumerate() {
        let name = &template.slot_names[index];
        let result = if let Some(expressions) = explicit {
            evaluate_expressions(ctx, template.slot_types[index], name, expressions, false)
        } else if let Some(default) = &template.dynamic_defaults[index] {
            evaluate_dynamic(ctx, default, template.slot_types[index], name)
        } else {
            Ok(template.defaults[index].clone())
        };
        let value = result.map_err(|error| SlotEvaluationError { index, error })?;
        template
            .validate_slot(index, &value)
            .map_err(|reason| SlotEvaluationError {
                index,
                error: invalid_slot(name, reason),
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
        overrides: &[(usize, Vec<RuntimeExpr>)],
        module: ModuleId,
    ) -> Result<Vec<Value>, SlotEvaluationError> {
        self.with_default_context(module, |ctx| evaluate_slots(ctx, template, overrides))
    }

    pub(crate) fn evaluate_static_default(
        &mut self,
        slot_type: SlotType,
        name: &str,
        expressions: &[RuntimeExpr],
        module: ModuleId,
    ) -> Result<Value, EvalError> {
        self.with_default_context(module, |ctx| {
            evaluate_expressions(ctx, slot_type, name, expressions, false)
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
