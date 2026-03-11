//! RHS action execution for rule firings.
//!
//! ## Phase 3 scope
//!
//! - `GlobalVariable` reads and writes via `GlobalStore` (Pass 006).
//! - `modify`/`duplicate` support template-aware slot overrides (Pass 003).
//! - `printout` with per-channel output capture via `OutputRouter` (Pass 004).

use std::collections::{HashMap, HashSet};
use std::fmt::Write as FmtWrite;
use std::path::Path;
use std::rc::Rc;

use ferric_core::beta::{RuleId, Salience};
use ferric_core::binding::{BindingSet, ValueRef, VarId, VarMap};
use ferric_core::token::Token;
use ferric_core::{
    AtomKey, EncodingError, Fact, FactBase, FactId, OrderedFact, ReteNetwork, Symbol, SymbolTable,
    TemplateId, Value,
};
use ferric_parser::{Action, ActionExpr, Constraint, FunctionCall, LiteralKind};
use slotmap::Key as _;

use crate::modules::ModuleRegistry;
use crate::qualified_name::{parse_qualified_name, QualifiedName};
use crate::router::OutputRouter;
use crate::templates::RegisteredTemplate;
use crate::tracing_support::{ferric_event, ferric_span};
use crate::Engine;

type OrderedFields = smallvec::SmallVec<[Value; 8]>;
type RuntimeBindingEnv = HashMap<String, Value>;

/// Maximum iterations for action-level loops (while, loop-for-count, progn$).
const MAX_ACTION_LOOP_ITERATIONS: usize = 1_000_000;
/// Internal callable emitted by parser for compact fact-slot references (`?f:slot`).
const FACT_SLOT_REF_FN: &str = "__fact_slot_ref";

pub(crate) struct ActionExecutionContext<'a> {
    pub engine: &'a mut Engine,
    pub focus_requests: &'a mut Vec<String>,
    pub current_module: crate::modules::ModuleId,
}

struct ActionEvalEnv {
    runtime_bindings: RuntimeBindingEnv,
}

fn flush_deferred_printout(context: &mut ActionExecutionContext<'_>) {
    for (channel, text) in context.engine.globals.take_printout_events() {
        context.engine.router.write(&channel, &text);
    }
}

impl ActionEvalEnv {
    fn make_eval_context<'ctx>(
        token: &'ctx Token,
        rule_info: &'ctx CompiledRuleInfo,
        context: &'ctx mut ActionExecutionContext<'_>,
    ) -> crate::evaluator::EvalContext<'ctx> {
        let engine = &mut *context.engine;
        crate::evaluator::EvalContext {
            bindings: &token.bindings,
            var_map: &rule_info.var_map,
            symbol_table: &mut engine.symbol_table,
            config: &engine.config,
            functions: &engine.functions,
            globals: &mut engine.globals,
            generics: &engine.generics,
            call_depth: 0,
            current_module: context.current_module,
            module_registry: &engine.module_registry,
            function_modules: &engine.function_modules,
            global_modules: &engine.global_modules,
            generic_modules: &engine.generic_modules,
            method_chain: None,
            input_buffer: Some(&mut engine.input_buffer),
        }
    }

    fn eval_runtime_expr(
        &mut self,
        token: &Token,
        rule_info: &CompiledRuleInfo,
        runtime_expr: &crate::evaluator::RuntimeExpr,
        context: &mut ActionExecutionContext<'_>,
    ) -> Result<Value, ActionError> {
        if !self.runtime_bindings.is_empty() {
            let mut merged_env =
                collect_outer_runtime_bindings(token, rule_info, &context.engine.symbol_table);
            for (name, value) in &self.runtime_bindings {
                insert_runtime_binding(&mut merged_env, name, value.clone());
            }
            let (bindings, var_map) = build_runtime_eval_bindings(&merged_env, context)?;
            return Self::eval_runtime_expr_with_bindings(
                runtime_expr,
                &bindings,
                &var_map,
                context,
            );
        }

        let mut ctx = Self::make_eval_context(token, rule_info, context);
        crate::evaluator::eval(&mut ctx, runtime_expr).map_err(ActionError::from)
    }

    fn eval_expr(
        &mut self,
        token: &Token,
        rule_info: &CompiledRuleInfo,
        expr: &ActionExpr,
        context: &mut ActionExecutionContext<'_>,
    ) -> Result<Value, ActionError> {
        if matches!(expr, ActionExpr::FunctionCall(_)) && action_expr_contains_fact_slot_ref(expr) {
            return self.eval_expr_with_fact_slot_refs(token, rule_info, expr, context);
        }
        self.eval_expr_base(token, rule_info, expr, context)
    }

    fn eval_expr_base(
        &mut self,
        token: &Token,
        rule_info: &CompiledRuleInfo,
        expr: &ActionExpr,
        context: &mut ActionExecutionContext<'_>,
    ) -> Result<Value, ActionError> {
        let runtime_expr = crate::evaluator::from_action_expr(
            expr,
            &mut context.engine.symbol_table,
            &context.engine.config,
        )
        .map_err(ActionError::from)?;
        self.eval_runtime_expr(token, rule_info, &runtime_expr, context)
    }

    fn eval_expr_with_fact_slot_refs(
        &mut self,
        token: &Token,
        rule_info: &CompiledRuleInfo,
        expr: &ActionExpr,
        context: &mut ActionExecutionContext<'_>,
    ) -> Result<Value, ActionError> {
        match expr {
            ActionExpr::FunctionCall(call) if call.name == FACT_SLOT_REF_FN => {
                eval_fact_slot_ref_call(token, rule_info, call, context)
            }
            ActionExpr::FunctionCall(call) => {
                let mut arg_values = Vec::with_capacity(call.args.len());
                for arg in &call.args {
                    let value = self.eval_expr(token, rule_info, arg, context)?;
                    arg_values.push(crate::evaluator::RuntimeExpr::Literal(value));
                }
                let runtime_expr = crate::evaluator::RuntimeExpr::Call {
                    name: call.name.clone(),
                    args: arg_values,
                    span: Some(crate::evaluator::SourceSpan {
                        line: call.span.start.line,
                        column: call.span.start.column,
                    }),
                };
                self.eval_runtime_expr(token, rule_info, &runtime_expr, context)
            }
            _ => self.eval_expr_base(token, rule_info, expr, context),
        }
    }

    fn eval_runtime_expr_with_bindings(
        runtime_expr: &crate::evaluator::RuntimeExpr,
        bindings: &BindingSet,
        var_map: &VarMap,
        context: &mut ActionExecutionContext<'_>,
    ) -> Result<Value, ActionError> {
        let engine = &mut *context.engine;
        let mut ctx = crate::evaluator::EvalContext {
            bindings,
            var_map,
            symbol_table: &mut engine.symbol_table,
            config: &engine.config,
            functions: &engine.functions,
            globals: &mut engine.globals,
            generics: &engine.generics,
            call_depth: 0,
            current_module: context.current_module,
            module_registry: &engine.module_registry,
            function_modules: &engine.function_modules,
            global_modules: &engine.global_modules,
            generic_modules: &engine.generic_modules,
            method_chain: None,
            input_buffer: Some(&mut engine.input_buffer),
        };
        crate::evaluator::eval(&mut ctx, runtime_expr).map_err(ActionError::from)
    }
}

fn action_expr_contains_fact_slot_ref(expr: &ActionExpr) -> bool {
    match expr {
        ActionExpr::FunctionCall(call) => {
            call.name == FACT_SLOT_REF_FN
                || call.args.iter().any(action_expr_contains_fact_slot_ref)
        }
        ActionExpr::If {
            condition,
            then_actions,
            else_actions,
            ..
        } => {
            action_expr_contains_fact_slot_ref(condition)
                || then_actions.iter().any(action_expr_contains_fact_slot_ref)
                || else_actions.iter().any(action_expr_contains_fact_slot_ref)
        }
        ActionExpr::While {
            condition, body, ..
        } => {
            action_expr_contains_fact_slot_ref(condition)
                || body.iter().any(action_expr_contains_fact_slot_ref)
        }
        ActionExpr::LoopForCount {
            start, end, body, ..
        } => {
            action_expr_contains_fact_slot_ref(start)
                || action_expr_contains_fact_slot_ref(end)
                || body.iter().any(action_expr_contains_fact_slot_ref)
        }
        ActionExpr::Progn {
            list_expr, body, ..
        } => {
            action_expr_contains_fact_slot_ref(list_expr)
                || body.iter().any(action_expr_contains_fact_slot_ref)
        }
        ActionExpr::QueryAction { query, body, .. } => {
            action_expr_contains_fact_slot_ref(query)
                || body.iter().any(action_expr_contains_fact_slot_ref)
        }
        ActionExpr::Switch {
            expr,
            cases,
            default,
            ..
        } => {
            action_expr_contains_fact_slot_ref(expr)
                || cases.iter().any(|(case_expr, actions)| {
                    action_expr_contains_fact_slot_ref(case_expr)
                        || actions.iter().any(action_expr_contains_fact_slot_ref)
                })
                || default
                    .as_ref()
                    .is_some_and(|actions| actions.iter().any(action_expr_contains_fact_slot_ref))
        }
        ActionExpr::Literal(..) | ActionExpr::Variable(..) | ActionExpr::GlobalVariable(..) => {
            false
        }
    }
}

fn eval_fact_slot_ref_call(
    token: &Token,
    rule_info: &CompiledRuleInfo,
    call: &FunctionCall,
    context: &mut ActionExecutionContext<'_>,
) -> Result<Value, ActionError> {
    if call.args.len() != 2 {
        return Err(ActionError::EvalError(format!(
            "{FACT_SLOT_REF_FN}: expected 2 arguments"
        )));
    }

    let var_name = match &call.args[0] {
        ActionExpr::Variable(name, _) => name.as_str(),
        _ => {
            return Err(ActionError::EvalError(format!(
                "{FACT_SLOT_REF_FN}: first argument must be a fact-address variable"
            )))
        }
    };

    let slot_name = match &call.args[1] {
        ActionExpr::Literal(lit) => match &lit.value {
            LiteralKind::Symbol(s) => s.as_str(),
            _ => {
                return Err(ActionError::EvalError(format!(
                    "{FACT_SLOT_REF_FN}: second argument must be a slot symbol"
                )))
            }
        },
        _ => {
            return Err(ActionError::EvalError(format!(
                "{FACT_SLOT_REF_FN}: second argument must be a slot symbol"
            )))
        }
    };

    let fact_id = resolve_fact_address(token, rule_info, var_name)?;
    let fact = get_fact_or_error(&context.engine.fact_base, fact_id)?;
    match fact {
        Fact::Template(template_fact) => {
            let registered = context
                .engine
                .template_defs
                .get(template_fact.template_id)
                .ok_or_else(|| {
                    ActionError::EvalError(format!(
                        "{FACT_SLOT_REF_FN}: unknown template id {}",
                        template_fact.template_id.data().as_ffi()
                    ))
                })?;

            let slot_idx = registered
                .slot_index
                .get(slot_name)
                .copied()
                .ok_or_else(|| {
                    ActionError::EvalError(format!(
                        "unknown slot `{slot_name}` in template `{}`",
                        registered.name
                    ))
                })?;

            template_fact.slots.get(slot_idx).cloned().ok_or_else(|| {
                ActionError::EvalError(format!(
                    "{FACT_SLOT_REF_FN}: slot index {slot_idx} out of bounds for template `{}`",
                    registered.name
                ))
            })
        }
        Fact::Ordered(_) => Err(ActionError::EvalError(format!(
            "fact-slot access `{var_name}:{slot_name}` requires a template fact"
        ))),
    }
}

/// A compiled test condition evaluated before RHS actions.
#[derive(Clone, Debug)]
pub(crate) enum CompiledTestCondition {
    Expr(crate::evaluator::RuntimeExpr),
    NegatedPatternRuntimeCheck(NegatedPatternRuntimeCheck),
}

/// Runtime fallback for negated ordered patterns with complex constraints.
///
/// This is used when compile-time lowering to join/alpha tests is not possible.
#[derive(Clone, Debug)]
pub(crate) struct NegatedPatternRuntimeCheck {
    pub relation: String,
    pub constraints: Vec<Constraint>,
}

/// Runtime hint for trailing ordered multi-variable captures (`$?var`).
///
/// The rete compiler currently approximates ordered multi-variable constraints
/// as single-slot bindings. This hint allows action-time evaluation to restore
/// CLIPS-style trailing multifield capture semantics for RHS expressions.
#[derive(Clone, Debug)]
pub(crate) struct MultifieldTailBindingHint {
    pub name: String,
    pub fact_index: usize,
    pub start_slot: usize,
}

/// Compiled rule metadata stored for action execution.
#[derive(Clone, Debug)]
pub(crate) struct CompiledRuleInfo {
    /// The rule name.
    #[allow(dead_code)] // May be used in future for debugging/logging
    pub name: String,
    /// Source-level rule definition captured at load time (when available).
    pub source_definition: Option<String>,
    /// The RHS actions to execute when the rule fires.
    pub actions: Vec<Action>,
    /// Variable name → `VarId` mapping from compilation.
    pub var_map: VarMap,
    /// Maps fact-address variable names to their index in token.facts.
    /// e.g., "f" (for ?f <- pattern) → 0 means token.facts[0] is the fact.
    pub fact_address_vars: HashMap<String, usize>,
    /// Rule salience (stored for informational purposes).
    #[allow(dead_code)] // May be used in future for debugging/logging
    pub salience: Salience,
    /// Pre-translated test conditions, evaluated at firing time.
    pub test_conditions: Vec<CompiledTestCondition>,
    /// Pre-translated RHS action call expressions.
    pub runtime_actions: Vec<Option<crate::evaluator::RuntimeExpr>>,
    /// Trailing ordered multifield capture hints for action-time evaluation.
    pub multifield_tail_bindings: Vec<MultifieldTailBindingHint>,
}

/// Errors that can occur during action execution.
#[derive(Clone, Debug, thiserror::Error)]
pub enum ActionError {
    #[error("unknown action: {0}")]
    UnknownAction(String),
    #[error("unbound variable: ?{0}")]
    UnboundVariable(String),
    #[error("fact not found: {0:?}")]
    FactNotFound(FactId),
    #[error("invalid assert: expected fact pattern argument")]
    InvalidAssert,
    #[error("invalid retract: expected variable argument")]
    InvalidRetract,
    #[error("encoding error: {0}")]
    Encoding(#[from] EncodingError),
    #[error("expression evaluation error: {0}")]
    EvalError(String),
    #[error("expression evaluation error: {0}")]
    Evaluator(#[from] crate::evaluator::EvalError),
}

/// Execute actions for a fired rule.
///
/// This is called with all the data needed pre-extracted to avoid borrow issues.
///
/// Returns `(fired, reset_requested, clear_requested, errors)` where:
/// - `fired` is `true` if test CE conditions all passed and actions were executed,
///   `false` if a test CE was falsy (actions are skipped and the rule is not
///   counted as having fired).
/// - `reset_requested` is `true` if a `(reset)` action was executed.
/// - `clear_requested` is `true` if a `(clear)` action was executed.
/// - `errors` is a list of non-fatal action errors that occurred during execution.
#[allow(clippy::too_many_lines)] // Sequential action/test evaluation flow with explicit error branches.
pub(crate) fn execute_actions(
    token: &Token,
    rule_info: &CompiledRuleInfo,
    context: &mut ActionExecutionContext<'_>,
) -> (bool, bool, bool, Vec<ActionError>) {
    ferric_span!(
        debug_span,
        "execute_actions",
        rule = %rule_info.name,
        action_count = rule_info.actions.len(),
        test_count = rule_info.test_conditions.len()
    );
    let mut errors = Vec::new();
    let mut reset_requested = false;
    let mut clear_requested = false;
    let mut eval_env = ActionEvalEnv {
        runtime_bindings: RuntimeBindingEnv::new(),
    };
    // Defensive: clear any stale deferred events that might have accumulated
    // in non-action evaluation contexts.
    let _ = context.engine.globals.take_printout_events();
    seed_multifield_tail_bindings(
        &context.engine.fact_base,
        token,
        &rule_info.multifield_tail_bindings,
        &mut eval_env.runtime_bindings,
    );

    // Evaluate test conditions first — if any is falsy, skip all actions and
    // signal to the caller that the rule did NOT logically fire.
    for test_condition in &rule_info.test_conditions {
        let condition_passed = match test_condition {
            CompiledTestCondition::Expr(test_expr) => {
                match eval_env.eval_runtime_expr(token, rule_info, test_expr, context) {
                    Ok(value) => crate::evaluator::is_truthy(&value, &context.engine.symbol_table),
                    Err(e) => {
                        ferric_event!(
                            warn,
                            rule = %rule_info.name,
                            error = %e,
                            "test_condition_eval_error"
                        );
                        flush_deferred_printout(context);
                        errors.push(e);
                        return (false, false, false, errors);
                    }
                }
            }
            CompiledTestCondition::NegatedPatternRuntimeCheck(check) => {
                match evaluate_negated_pattern_runtime_check(
                    token,
                    rule_info,
                    check,
                    &mut eval_env,
                    context,
                ) {
                    Ok(passed) => passed,
                    Err(e) => {
                        ferric_event!(
                            warn,
                            rule = %rule_info.name,
                            error = %e,
                            "test_condition_eval_error"
                        );
                        flush_deferred_printout(context);
                        errors.push(e);
                        return (false, false, false, errors);
                    }
                }
            }
        };
        flush_deferred_printout(context);

        if !condition_passed {
            ferric_event!(debug, rule = %rule_info.name, "rule_skipped_by_test_condition");
            return (false, false, false, errors); // Test CE falsy — rule did not fire
        }
    }

    for (index, action) in rule_info.actions.iter().enumerate() {
        let runtime_call = rule_info
            .runtime_actions
            .get(index)
            .and_then(Option::as_ref);
        if let Err(e) = execute_single_action(
            &mut reset_requested,
            &mut clear_requested,
            token,
            rule_info,
            &action.call,
            runtime_call,
            context,
            &mut eval_env,
        ) {
            ferric_event!(
                warn,
                rule = %rule_info.name,
                action_index = index,
                action_name = %action.call.name,
                error = %e,
                "rule_action_error"
            );
            errors.push(e);
        }
        flush_deferred_printout(context);
        // Stop executing further actions if clear/reset was requested.
        if clear_requested || reset_requested {
            ferric_event!(
                debug,
                rule = %rule_info.name,
                action_index = index,
                reset_requested,
                clear_requested,
                "rule_action_short_circuit"
            );
            break;
        }
    }

    ferric_event!(
        debug,
        rule = %rule_info.name,
        error_count = errors.len(),
        reset_requested,
        clear_requested,
        "execute_actions_complete"
    );
    (true, reset_requested, clear_requested, errors)
}

fn evaluate_negated_pattern_runtime_check(
    token: &Token,
    rule_info: &CompiledRuleInfo,
    check: &NegatedPatternRuntimeCheck,
    eval_env: &mut ActionEvalEnv,
    context: &mut ActionExecutionContext<'_>,
) -> Result<bool, ActionError> {
    let Ok(relation) = context
        .engine
        .symbol_table
        .intern_symbol(&check.relation, context.engine.config.string_encoding)
    else {
        return Ok(true);
    };

    let base_env = collect_outer_runtime_bindings(token, rule_info, &context.engine.symbol_table);

    let candidate_ids: Vec<_> = context
        .engine
        .fact_base
        .facts_by_relation(relation)
        .collect();

    for fact_id in candidate_ids {
        let Some(entry) = context.engine.fact_base.get(fact_id) else {
            continue;
        };
        let Fact::Ordered(ordered) = &entry.fact else {
            continue;
        };
        let ordered = ordered.clone();

        if ordered_fact_matches_runtime_constraints(
            &ordered,
            &check.constraints,
            &base_env,
            eval_env,
            context,
        )? {
            // Matching fact exists, so the negated condition fails.
            return Ok(false);
        }
    }

    Ok(true)
}

fn collect_outer_runtime_bindings(
    token: &Token,
    rule_info: &CompiledRuleInfo,
    symbol_table: &SymbolTable,
) -> RuntimeBindingEnv {
    let mut env = RuntimeBindingEnv::new();
    for index in 0..rule_info.var_map.len() {
        #[allow(clippy::cast_possible_truncation)]
        let var_id = VarId(index as u16);
        let Some(value_ref) = token.bindings.get(var_id) else {
            continue;
        };
        let symbol = rule_info.var_map.name(var_id);
        let Some(name) = symbol_table.resolve_symbol_str(symbol) else {
            continue;
        };
        insert_runtime_binding(&mut env, name, Value::clone(value_ref));
    }
    env
}

fn insert_runtime_binding(env: &mut RuntimeBindingEnv, name: &str, value: Value) {
    env.insert(name.to_string(), value.clone());
    if !name.starts_with("$?") {
        env.entry(format!("$?{name}")).or_insert(value);
    }
}

fn seed_multifield_tail_bindings(
    fact_base: &FactBase,
    token: &Token,
    hints: &[MultifieldTailBindingHint],
    env: &mut RuntimeBindingEnv,
) {
    for hint in hints {
        let Some(&fact_id) = token.facts.get(hint.fact_index) else {
            continue;
        };
        let Some(entry) = fact_base.get(fact_id) else {
            continue;
        };
        let Fact::Ordered(ordered) = &entry.fact else {
            continue;
        };

        let mut captured = ferric_core::Multifield::new();
        captured.extend(ordered.fields.iter().skip(hint.start_slot).cloned());
        insert_runtime_binding(env, &hint.name, Value::Multifield(Box::new(captured)));
    }
}

fn ordered_fact_matches_runtime_constraints(
    fact: &OrderedFact,
    constraints: &[Constraint],
    base_env: &RuntimeBindingEnv,
    eval_env: &mut ActionEvalEnv,
    context: &mut ActionExecutionContext<'_>,
) -> Result<bool, ActionError> {
    if fact.fields.len() < constraints.len() {
        return Ok(false);
    }

    let mut envs = vec![base_env.clone()];
    for (slot_index, constraint) in constraints.iter().enumerate() {
        let Some(slot_value) = fact.fields.get(slot_index) else {
            return Ok(false);
        };

        let mut next_envs = Vec::new();
        for env in &envs {
            let mut matched =
                runtime_constraint_matches_envs(constraint, slot_value, env, eval_env, context)?;
            next_envs.append(&mut matched);
        }

        if next_envs.is_empty() {
            return Ok(false);
        }

        envs = next_envs;
    }

    Ok(!envs.is_empty())
}

fn runtime_constraint_matches_envs(
    constraint: &Constraint,
    slot_value: &Value,
    env: &RuntimeBindingEnv,
    eval_env: &mut ActionEvalEnv,
    context: &mut ActionExecutionContext<'_>,
) -> Result<Vec<RuntimeBindingEnv>, ActionError> {
    match constraint {
        Constraint::Literal(lit) => {
            if value_equals_literal(slot_value, &lit.value, &context.engine.symbol_table) {
                Ok(vec![env.clone()])
            } else {
                Ok(Vec::new())
            }
        }
        Constraint::Variable(name, _) | Constraint::MultiVariable(name, _) => {
            if let Some(existing) = env.get(name) {
                if values_equal(existing, slot_value) {
                    Ok(vec![env.clone()])
                } else {
                    Ok(Vec::new())
                }
            } else {
                let mut bound = env.clone();
                insert_runtime_binding(&mut bound, name, slot_value.clone());
                Ok(vec![bound])
            }
        }
        Constraint::Wildcard(_) | Constraint::MultiWildcard(_) => Ok(vec![env.clone()]),
        Constraint::Not(inner, _) => match inner.as_ref() {
            Constraint::Literal(lit) => {
                if value_equals_literal(slot_value, &lit.value, &context.engine.symbol_table) {
                    Ok(Vec::new())
                } else {
                    Ok(vec![env.clone()])
                }
            }
            Constraint::Variable(name, _) | Constraint::MultiVariable(name, _) => {
                let Some(existing) = env.get(name) else {
                    return Ok(Vec::new());
                };
                if values_equal(existing, slot_value) {
                    Ok(Vec::new())
                } else {
                    Ok(vec![env.clone()])
                }
            }
            Constraint::Wildcard(_) | Constraint::MultiWildcard(_) => Ok(vec![env.clone()]),
            other => {
                let inner =
                    runtime_constraint_matches_envs(other, slot_value, env, eval_env, context)?;
                if inner.is_empty() {
                    Ok(vec![env.clone()])
                } else {
                    Ok(Vec::new())
                }
            }
        },
        Constraint::And(parts, _) => {
            let mut envs = vec![env.clone()];
            for part in parts {
                let mut next = Vec::new();
                for candidate in &envs {
                    let mut matched = runtime_constraint_matches_envs(
                        part, slot_value, candidate, eval_env, context,
                    )?;
                    next.append(&mut matched);
                }
                if next.is_empty() {
                    return Ok(Vec::new());
                }
                envs = next;
            }
            Ok(envs)
        }
        Constraint::Or(parts, _) => {
            let mut results = Vec::new();
            for part in parts {
                let mut matched =
                    runtime_constraint_matches_envs(part, slot_value, env, eval_env, context)?;
                results.append(&mut matched);
            }
            Ok(results)
        }
        Constraint::Predicate(expr, _) => {
            let Some(value) = runtime_constraint_expr_value(expr, env, eval_env, context)? else {
                return Ok(Vec::new());
            };
            if crate::evaluator::is_truthy(&value, &context.engine.symbol_table) {
                Ok(vec![env.clone()])
            } else {
                Ok(Vec::new())
            }
        }
        Constraint::ReturnValue(expr, _) => {
            let Some(value) = runtime_constraint_expr_value(expr, env, eval_env, context)? else {
                return Ok(Vec::new());
            };
            if values_equal(&value, slot_value) {
                Ok(vec![env.clone()])
            } else {
                Ok(Vec::new())
            }
        }
    }
}

fn runtime_constraint_expr_value(
    expr: &ferric_parser::SExpr,
    env: &RuntimeBindingEnv,
    _eval_env: &mut ActionEvalEnv,
    context: &mut ActionExecutionContext<'_>,
) -> Result<Option<Value>, ActionError> {
    let Ok(runtime_expr) = crate::evaluator::from_sexpr(
        expr,
        &mut context.engine.symbol_table,
        &context.engine.config,
    ) else {
        return Ok(None);
    };

    let (bindings, var_map) = build_runtime_eval_bindings(env, context)?;
    match ActionEvalEnv::eval_runtime_expr_with_bindings(
        &runtime_expr,
        &bindings,
        &var_map,
        context,
    ) {
        Ok(value) => Ok(Some(value)),
        Err(ActionError::Evaluator(_) | ActionError::EvalError(_)) => Ok(None),
        Err(other) => Err(other),
    }
}

fn build_runtime_eval_bindings(
    env: &RuntimeBindingEnv,
    context: &mut ActionExecutionContext<'_>,
) -> Result<(BindingSet, VarMap), ActionError> {
    let mut var_map = VarMap::new();
    let mut bindings = BindingSet::new();

    for (name, value) in env {
        let symbol = context
            .engine
            .symbol_table
            .intern_symbol(name, context.engine.config.string_encoding)
            .map_err(ActionError::Encoding)?;
        let var_id = var_map
            .get_or_create(symbol)
            .map_err(|e| ActionError::EvalError(e.to_string()))?;
        bindings.set(var_id, ValueRef::new(value.clone()));
    }

    Ok((bindings, var_map))
}

fn value_equals_literal(value: &Value, literal: &LiteralKind, symbol_table: &SymbolTable) -> bool {
    match literal {
        LiteralKind::Integer(expected) => {
            matches!(value, Value::Integer(actual) if actual == expected)
        }
        LiteralKind::Float(expected) => {
            matches!(value, Value::Float(actual) if actual.to_bits() == expected.to_bits())
        }
        LiteralKind::String(expected) => {
            matches!(value, Value::String(actual) if actual.as_str() == expected)
        }
        LiteralKind::Symbol(expected) => match value {
            Value::Symbol(symbol) => {
                symbol_table.resolve_symbol_str(*symbol) == Some(expected.as_str())
            }
            _ => false,
        },
    }
}

fn values_equal(lhs: &Value, rhs: &Value) -> bool {
    match (AtomKey::from_value(lhs), AtomKey::from_value(rhs)) {
        (Some(lhs_key), Some(rhs_key)) => lhs_key == rhs_key,
        _ => lhs.structural_eq(rhs),
    }
}

#[allow(clippy::too_many_arguments)] // Action dispatch needs full mutable engine/action context.
#[allow(clippy::too_many_lines)] // if-branch execution adds necessary verbosity
fn execute_single_action(
    reset_requested: &mut bool,
    clear_requested: &mut bool,
    token: &Token,
    rule_info: &CompiledRuleInfo,
    call: &FunctionCall,
    runtime_call: Option<&crate::evaluator::RuntimeExpr>,
    context: &mut ActionExecutionContext<'_>,
    eval_env: &mut ActionEvalEnv,
) -> Result<(), ActionError> {
    match call.name.as_str() {
        "assert" => execute_assert(token, rule_info, &call.args, context, eval_env),
        "retract" => execute_retract(
            &mut context.engine.fact_base,
            &mut context.engine.rete,
            token,
            rule_info,
            &call.args,
        ),
        "modify" => execute_modify(token, rule_info, &call.args, context, eval_env),
        "duplicate" => execute_duplicate(token, rule_info, &call.args, context, eval_env),
        "halt" => {
            context.engine.halt();
            Ok(())
        }
        "reset" => {
            *reset_requested = true;
            Ok(())
        }
        "clear" => {
            *clear_requested = true;
            Ok(())
        }
        "printout" => execute_printout(token, rule_info, &call.args, context, eval_env),
        "println" => execute_println(token, rule_info, &call.args, context, eval_env),
        "focus" => execute_focus(token, rule_info, &call.args, context, eval_env),
        "list-focus-stack" => {
            execute_list_focus_stack(&mut context.engine.router, &context.engine.module_registry)
        }
        "agenda" => execute_agenda(
            &context.engine.rete,
            &mut context.engine.router,
            &context.engine.rule_info,
        ),
        "rules" => execute_rules(token, rule_info, &call.args, context, eval_env),
        "undefrule" => execute_undefrule(token, rule_info, &call.args, context, eval_env),
        "ppdefrule" => execute_ppdefrule(token, rule_info, &call.args, context, eval_env),
        "load" => execute_load(token, rule_info, &call.args, context, eval_env),
        "run" => {
            // (run) from within a rule RHS is a no-op — the engine is already running.
            // CLIPS allows this but it's unusual. We silently ignore it.
            Ok(())
        }
        "bind" => {
            if let [ActionExpr::Variable(name, _), value_expr] = call.args.as_slice() {
                let value = eval_env.eval_expr(token, rule_info, value_expr, context)?;
                insert_runtime_binding(&mut eval_env.runtime_bindings, name, value);
                Ok(())
            } else {
                // Global bind (or malformed local bind) stays on evaluator semantics.
                let eval_result = if let Some(runtime_expr) = runtime_call {
                    eval_env.eval_runtime_expr(token, rule_info, runtime_expr, context)
                } else {
                    let action_expr = ActionExpr::FunctionCall(call.clone());
                    eval_env.eval_expr(token, rule_info, &action_expr, context)
                };
                eval_result
                    .map(|_| ())
                    .map_err(|e| ActionError::UnknownAction(format!("bind: {e}")))
            }
        }
        "if" => {
            // `(if <cond> then <action>* [else <action>*])` special form.
            //
            // Two cases for `runtime_call`:
            //
            // 1. Top-level `if` action: the loader wraps the `if` as the sole
            //    arg of a `RuntimeExpr::Call { name: "if", args: [RuntimeExpr::If{...}] }`.
            //    We unwrap one level to get the `RuntimeExpr::If`.
            //
            // 2. Nested `if` in a branch: recursive `execute_single_action` is
            //    called with `runtime_call = Some(RuntimeExpr::If{...})` directly
            //    (since the branch item's `rt_expr` is already the `RuntimeExpr::If`).
            let if_runtime: Option<&crate::evaluator::RuntimeExpr> = match runtime_call {
                Some(crate::evaluator::RuntimeExpr::Call { args, .. }) => {
                    // Top-level path: unwrap the wrapper call.
                    args.first().map(|a| a as &crate::evaluator::RuntimeExpr)
                }
                Some(rt @ crate::evaluator::RuntimeExpr::If { .. }) => {
                    // Nested path: already the RuntimeExpr::If directly.
                    Some(rt)
                }
                _ => None,
            };

            if let Some(crate::evaluator::RuntimeExpr::If {
                condition,
                then_branch,
                else_branch,
                ..
            }) = if_runtime
            {
                let cond_value = {
                    let mut ctx = ActionEvalEnv::make_eval_context(token, rule_info, context);
                    crate::evaluator::eval(&mut ctx, condition).map_err(ActionError::from)?
                };
                let branch =
                    if crate::evaluator::is_truthy(&cond_value, &context.engine.symbol_table) {
                        then_branch
                    } else {
                        else_branch
                    };
                for (action_expr, rt_expr) in branch {
                    // Reconstruct a FunctionCall from the ActionExpr so we can
                    // route through execute_single_action normally.
                    let (branch_call, branch_runtime): (
                        FunctionCall,
                        Option<&crate::evaluator::RuntimeExpr>,
                    ) = match action_expr {
                        ActionExpr::FunctionCall(fc) => {
                            let rt: Option<&crate::evaluator::RuntimeExpr> = rt_expr.as_deref();
                            (fc.clone(), rt)
                        }
                        ActionExpr::If { span, .. } => {
                            // Nested if: create a synthetic call with name "if"
                            // so the recursive execute_single_action picks it up.
                            let synthetic = FunctionCall {
                                name: "if".to_string(),
                                args: vec![],
                                span: *span,
                            };
                            let rt: Option<&crate::evaluator::RuntimeExpr> = rt_expr.as_deref();
                            (synthetic, rt)
                        }
                        ActionExpr::Switch { span, .. } => {
                            let synthetic = FunctionCall {
                                name: "switch".to_string(),
                                args: vec![],
                                span: *span,
                            };
                            let rt: Option<&crate::evaluator::RuntimeExpr> = rt_expr.as_deref();
                            (synthetic, rt)
                        }
                        ActionExpr::While { span, .. } => {
                            let synthetic = FunctionCall {
                                name: "while".to_string(),
                                args: vec![],
                                span: *span,
                            };
                            let rt: Option<&crate::evaluator::RuntimeExpr> = rt_expr.as_deref();
                            (synthetic, rt)
                        }
                        ActionExpr::LoopForCount { span, .. } => {
                            let synthetic = FunctionCall {
                                name: "loop-for-count".to_string(),
                                args: vec![],
                                span: *span,
                            };
                            let rt: Option<&crate::evaluator::RuntimeExpr> = rt_expr.as_deref();
                            (synthetic, rt)
                        }
                        ActionExpr::Progn { span, .. } => {
                            let synthetic = FunctionCall {
                                name: "progn$".to_string(),
                                args: vec![],
                                span: *span,
                            };
                            let rt: Option<&crate::evaluator::RuntimeExpr> = rt_expr.as_deref();
                            (synthetic, rt)
                        }
                        _ => {
                            // Literal/variable — evaluate as expression; not an
                            // action but valid in CLIPS (result is discarded).
                            if let Some(rt) = rt_expr {
                                let _ = eval_env.eval_runtime_expr(token, rule_info, rt, context);
                            } else {
                                let _ = eval_env.eval_expr(token, rule_info, action_expr, context);
                            }
                            continue;
                        }
                    };
                    execute_single_action(
                        reset_requested,
                        clear_requested,
                        token,
                        rule_info,
                        &branch_call,
                        branch_runtime,
                        context,
                        eval_env,
                    )?;
                    // Propagate early-exit flags.
                    if context.engine.is_halted() || *reset_requested || *clear_requested {
                        break;
                    }
                }
                Ok(())
            } else {
                // Fallback: evaluate as expression (no-op if void).
                if let Some(runtime_expr) = runtime_call {
                    eval_env
                        .eval_runtime_expr(token, rule_info, runtime_expr, context)
                        .map(|_| ())
                        .map_err(|e| ActionError::UnknownAction(format!("if: {e}")))
                } else {
                    Ok(())
                }
            }
        }
        "while" => {
            // `(while <cond> do <action>*)` loop form.
            let while_runtime: Option<&crate::evaluator::RuntimeExpr> = match runtime_call {
                Some(crate::evaluator::RuntimeExpr::Call { args, .. }) => {
                    args.first().map(|a| a as &crate::evaluator::RuntimeExpr)
                }
                Some(rt @ crate::evaluator::RuntimeExpr::While { .. }) => Some(rt),
                _ => None,
            };

            if let Some(crate::evaluator::RuntimeExpr::While {
                condition, body, ..
            }) = while_runtime
            {
                let mut iterations = 0usize;
                loop {
                    // Evaluate condition.
                    let cond_value = {
                        let mut ctx = ActionEvalEnv::make_eval_context(token, rule_info, context);
                        crate::evaluator::eval(&mut ctx, condition).map_err(ActionError::from)?
                    };
                    if !crate::evaluator::is_truthy(&cond_value, &context.engine.symbol_table) {
                        break;
                    }
                    iterations += 1;
                    if iterations > MAX_ACTION_LOOP_ITERATIONS {
                        return Err(ActionError::EvalError(format!(
                            "while loop exceeded maximum iterations ({MAX_ACTION_LOOP_ITERATIONS})"
                        )));
                    }
                    // Execute body items.
                    execute_loop_body(
                        reset_requested,
                        clear_requested,
                        token,
                        rule_info,
                        body,
                        context,
                        eval_env,
                    )?;
                    if context.engine.is_halted() || *reset_requested || *clear_requested {
                        break;
                    }
                }
                Ok(())
            } else if let Some(runtime_expr) = runtime_call {
                eval_env
                    .eval_runtime_expr(token, rule_info, runtime_expr, context)
                    .map(|_| ())
                    .map_err(|e| ActionError::UnknownAction(format!("while: {e}")))
            } else {
                Ok(())
            }
        }

        "loop-for-count" => {
            // `(loop-for-count (?var start end) do <action>*)` loop form.
            let lfc_runtime: Option<&crate::evaluator::RuntimeExpr> = match runtime_call {
                Some(crate::evaluator::RuntimeExpr::Call { args, .. }) => {
                    args.first().map(|a| a as &crate::evaluator::RuntimeExpr)
                }
                Some(rt @ crate::evaluator::RuntimeExpr::LoopForCount { .. }) => Some(rt),
                _ => None,
            };

            if let Some(crate::evaluator::RuntimeExpr::LoopForCount {
                var_name,
                start,
                end,
                body,
                span,
            }) = lfc_runtime
            {
                let (start_int, end_int) = {
                    let mut ctx = ActionEvalEnv::make_eval_context(token, rule_info, context);
                    let sv = crate::evaluator::eval(&mut ctx, start).map_err(ActionError::from)?;
                    let ev = crate::evaluator::eval(&mut ctx, end).map_err(ActionError::from)?;
                    let si = match &sv {
                        Value::Integer(n) => *n,
                        #[allow(clippy::cast_possible_truncation)]
                        Value::Float(f) => *f as i64,
                        _ => {
                            return Err(ActionError::EvalError(
                                "loop-for-count: start value must be an integer".to_string(),
                            ))
                        }
                    };
                    let ei = match &ev {
                        Value::Integer(n) => *n,
                        #[allow(clippy::cast_possible_truncation)]
                        Value::Float(f) => *f as i64,
                        _ => {
                            return Err(ActionError::EvalError(
                                "loop-for-count: end value must be an integer".to_string(),
                            ))
                        }
                    };
                    (si, ei)
                };

                for counter in start_int..=end_int {
                    // Build an augmented token and rule_info with the loop variable bound.
                    let (loop_token, loop_rule_info) = if let Some(var) = var_name {
                        augment_bindings_with_var(
                            token,
                            rule_info,
                            var,
                            Value::Integer(counter),
                            &mut context.engine.symbol_table,
                            &context.engine.config,
                        )?
                    } else {
                        (token.clone(), rule_info_clone_light(rule_info))
                    };

                    execute_loop_body(
                        reset_requested,
                        clear_requested,
                        &loop_token,
                        &loop_rule_info,
                        body,
                        context,
                        eval_env,
                    )?;
                    if context.engine.is_halted() || *reset_requested || *clear_requested {
                        break;
                    }
                }
                let _ = span; // suppress unused variable warning
                Ok(())
            } else if let Some(runtime_expr) = runtime_call {
                eval_env
                    .eval_runtime_expr(token, rule_info, runtime_expr, context)
                    .map(|_| ())
                    .map_err(|e| ActionError::UnknownAction(format!("loop-for-count: {e}")))
            } else {
                Ok(())
            }
        }

        "switch" => {
            // `(switch <expr> (case <val> then <action>*) ... [(default <action>*)])` form.
            let switch_runtime: Option<&crate::evaluator::RuntimeExpr> = match runtime_call {
                Some(crate::evaluator::RuntimeExpr::Call { args, .. }) => {
                    args.first().map(|a| a as &crate::evaluator::RuntimeExpr)
                }
                Some(rt @ crate::evaluator::RuntimeExpr::Switch { .. }) => Some(rt),
                _ => None,
            };

            if let Some(crate::evaluator::RuntimeExpr::Switch {
                expr,
                cases,
                default,
                ..
            }) = switch_runtime
            {
                // Evaluate the discriminant expression.
                let disc_value = {
                    let mut ctx = ActionEvalEnv::make_eval_context(token, rule_info, context);
                    crate::evaluator::eval(&mut ctx, expr).map_err(ActionError::from)?
                };

                // Find first matching case.
                let mut matched_body = None;
                for (test_val_expr, case_body) in cases {
                    let test_value = {
                        let mut ctx = ActionEvalEnv::make_eval_context(token, rule_info, context);
                        crate::evaluator::eval(&mut ctx, test_val_expr)
                            .map_err(ActionError::from)?
                    };
                    if disc_value.structural_eq(&test_value) {
                        matched_body = Some(case_body);
                        break;
                    }
                }

                // Fall to default if no case matched.
                if matched_body.is_none() {
                    if let Some(default_body) = default {
                        matched_body = Some(default_body);
                    }
                }

                // Execute matched body.
                if let Some(body) = matched_body {
                    execute_loop_body(
                        reset_requested,
                        clear_requested,
                        token,
                        rule_info,
                        body,
                        context,
                        eval_env,
                    )?;
                }
                Ok(())
            } else if let Some(runtime_expr) = runtime_call {
                eval_env
                    .eval_runtime_expr(token, rule_info, runtime_expr, context)
                    .map(|_| ())
                    .map_err(|e| ActionError::UnknownAction(format!("switch: {e}")))
            } else {
                Ok(())
            }
        }

        "progn$" | "foreach" => {
            // `(progn$ (?var <expr>) <action>*)` / `(foreach ?var <expr> do <action>*)` loop.
            let progn_runtime: Option<&crate::evaluator::RuntimeExpr> = match runtime_call {
                Some(crate::evaluator::RuntimeExpr::Call { args, .. }) => {
                    args.first().map(|a| a as &crate::evaluator::RuntimeExpr)
                }
                Some(rt @ crate::evaluator::RuntimeExpr::Progn { .. }) => Some(rt),
                _ => None,
            };

            if let Some(crate::evaluator::RuntimeExpr::Progn {
                var_name,
                list_expr,
                body,
                ..
            }) = progn_runtime
            {
                let elements: Vec<Value> = {
                    let mut ctx = ActionEvalEnv::make_eval_context(token, rule_info, context);
                    let list_val =
                        crate::evaluator::eval(&mut ctx, list_expr).map_err(ActionError::from)?;
                    match list_val {
                        Value::Multifield(mf) => mf.as_slice().to_vec(),
                        other => vec![other],
                    }
                };

                let index_var_name = format!("{var_name}-index");
                for (idx, element) in elements.iter().enumerate() {
                    #[allow(clippy::cast_possible_wrap)]
                    // usize→i64: element counts can't exceed i64
                    let one_based = idx as i64 + 1;

                    // Bind the element variable and the index variable.
                    let (token1, rule_info1) = augment_bindings_with_var(
                        token,
                        rule_info,
                        var_name,
                        element.clone(),
                        &mut context.engine.symbol_table,
                        &context.engine.config,
                    )?;
                    let (loop_token, loop_rule_info) = augment_bindings_with_var(
                        &token1,
                        &rule_info1,
                        &index_var_name,
                        Value::Integer(one_based),
                        &mut context.engine.symbol_table,
                        &context.engine.config,
                    )?;

                    execute_loop_body(
                        reset_requested,
                        clear_requested,
                        &loop_token,
                        &loop_rule_info,
                        body,
                        context,
                        eval_env,
                    )?;
                    if context.engine.is_halted() || *reset_requested || *clear_requested {
                        break;
                    }
                }
                Ok(())
            } else if let Some(runtime_expr) = runtime_call {
                eval_env
                    .eval_runtime_expr(token, rule_info, runtime_expr, context)
                    .map(|_| ())
                    .map_err(|e| ActionError::UnknownAction(format!("progn$: {e}")))
            } else {
                Ok(())
            }
        }

        // Fact-query macro forms.
        "do-for-fact"
        | "do-for-all-facts"
        | "delayed-do-for-all-facts"
        | "any-factp"
        | "find-fact"
        | "find-all-facts" => {
            // Unwrap the pre-compiled `RuntimeExpr::QueryAction` from the
            // wrapper call that the loader places around it.
            let query_runtime: Option<&crate::evaluator::RuntimeExpr> = match runtime_call {
                Some(crate::evaluator::RuntimeExpr::Call { args, .. }) => {
                    args.first().map(|a| a as &crate::evaluator::RuntimeExpr)
                }
                Some(rt @ crate::evaluator::RuntimeExpr::QueryAction { .. }) => Some(rt),
                _ => None,
            };

            if let Some(crate::evaluator::RuntimeExpr::QueryAction {
                name,
                bindings,
                query,
                body,
                ..
            }) = query_runtime
            {
                execute_query_action(
                    reset_requested,
                    clear_requested,
                    token,
                    rule_info,
                    name,
                    bindings,
                    query,
                    body,
                    context,
                    eval_env,
                )
            } else if let Some(runtime_expr) = runtime_call {
                eval_env
                    .eval_runtime_expr(token, rule_info, runtime_expr, context)
                    .map(|_| ())
                    .map_err(|e| ActionError::UnknownAction(format!("{}: {e}", call.name)))
            } else {
                Ok(())
            }
        }

        // For any other call, try evaluating it as an expression (e.g., bind).
        _ => {
            let eval_result = if let Some(runtime_expr) = runtime_call {
                eval_env.eval_runtime_expr(token, rule_info, runtime_expr, context)
            } else {
                let action_expr = ActionExpr::FunctionCall(call.clone());
                eval_env.eval_expr(token, rule_info, &action_expr, context)
            };
            eval_result
                .map(|_| ())
                .map_err(|e| ActionError::UnknownAction(format!("{}: {e}", call.name)))
        }
    }
}

/// Create a lightweight clone of `CompiledRuleInfo` sharing only the `var_map`.
///
/// Clones the parts needed for loop body execution.  `runtime_actions` is
/// intentionally left empty because the loop body items are dispatched via
/// `execute_loop_body`, which constructs `runtime_call` from the `RuntimeExpr`
/// body entries directly rather than from a `runtime_actions` index.
fn rule_info_clone_light(rule_info: &CompiledRuleInfo) -> CompiledRuleInfo {
    CompiledRuleInfo {
        name: rule_info.name.clone(),
        source_definition: rule_info.source_definition.clone(),
        actions: Vec::new(),
        var_map: rule_info.var_map.clone(),
        fact_address_vars: rule_info.fact_address_vars.clone(),
        salience: rule_info.salience,
        test_conditions: Vec::new(),
        runtime_actions: Vec::new(),
        multifield_tail_bindings: rule_info.multifield_tail_bindings.clone(),
    }
}

/// Clone a `Token` and `CompiledRuleInfo` and augment them with an additional
/// loop variable binding.
///
/// This allows loop body items to reference the loop variable via the normal
/// `eval_expr(token, rule_info, ...)` path without modifying the original
/// token or `rule_info`.
fn augment_bindings_with_var(
    token: &Token,
    rule_info: &CompiledRuleInfo,
    var_name: &str,
    value: Value,
    symbol_table: &mut SymbolTable,
    config: &crate::config::EngineConfig,
) -> Result<(Token, CompiledRuleInfo), ActionError> {
    let sym = symbol_table
        .intern_symbol(var_name, config.string_encoding)
        .map_err(ActionError::from)?;

    let mut new_rule_info = rule_info_clone_light(rule_info);
    let var_id = new_rule_info
        .var_map
        .get_or_create(sym)
        .map_err(|_| ActionError::EvalError(format!("loop: too many variables for {var_name}")))?;

    let mut new_token = token.clone();
    new_token.bindings.set(var_id, ValueRef::new(value));

    Ok((new_token, new_rule_info))
}

/// Execute a slice of loop body items.
///
/// Each body item is a `(ActionExpr, Option<RuntimeExpr>)` pair.  Items with
/// pre-compiled `RuntimeExpr`s use them directly; others are dispatched via
/// the standard action executor path.
///
/// This mirrors the branch-execution logic in the `if` handler but is factored
/// out so the three loop forms can share it.
#[allow(clippy::too_many_arguments)]
fn execute_loop_body(
    reset_requested: &mut bool,
    clear_requested: &mut bool,
    token: &Token,
    rule_info: &CompiledRuleInfo,
    body: &[(
        ferric_parser::ActionExpr,
        Option<Box<crate::evaluator::RuntimeExpr>>,
    )],
    context: &mut ActionExecutionContext<'_>,
    eval_env: &mut ActionEvalEnv,
) -> Result<(), ActionError> {
    use ferric_parser::ActionExpr;
    for (action_expr, rt_expr) in body {
        let (branch_call, branch_runtime): (
            ferric_parser::FunctionCall,
            Option<&crate::evaluator::RuntimeExpr>,
        ) = match action_expr {
            ActionExpr::FunctionCall(fc) => {
                let rt: Option<&crate::evaluator::RuntimeExpr> = rt_expr.as_deref();
                (fc.clone(), rt)
            }
            ActionExpr::If { span, .. } => {
                let synthetic = ferric_parser::FunctionCall {
                    name: "if".to_string(),
                    args: vec![],
                    span: *span,
                };
                let rt: Option<&crate::evaluator::RuntimeExpr> = rt_expr.as_deref();
                (synthetic, rt)
            }
            ActionExpr::While { span, .. } => {
                let synthetic = ferric_parser::FunctionCall {
                    name: "while".to_string(),
                    args: vec![],
                    span: *span,
                };
                let rt: Option<&crate::evaluator::RuntimeExpr> = rt_expr.as_deref();
                (synthetic, rt)
            }
            ActionExpr::LoopForCount { span, .. } => {
                let synthetic = ferric_parser::FunctionCall {
                    name: "loop-for-count".to_string(),
                    args: vec![],
                    span: *span,
                };
                let rt: Option<&crate::evaluator::RuntimeExpr> = rt_expr.as_deref();
                (synthetic, rt)
            }
            ActionExpr::Progn { span, .. } => {
                let synthetic = ferric_parser::FunctionCall {
                    name: "progn$".to_string(),
                    args: vec![],
                    span: *span,
                };
                let rt: Option<&crate::evaluator::RuntimeExpr> = rt_expr.as_deref();
                (synthetic, rt)
            }
            ActionExpr::QueryAction { name, span, .. } => {
                let synthetic = ferric_parser::FunctionCall {
                    name: name.clone(),
                    args: vec![],
                    span: *span,
                };
                let rt: Option<&crate::evaluator::RuntimeExpr> = rt_expr.as_deref();
                (synthetic, rt)
            }
            ActionExpr::Switch { span, .. } => {
                let synthetic = ferric_parser::FunctionCall {
                    name: "switch".to_string(),
                    args: vec![],
                    span: *span,
                };
                let rt: Option<&crate::evaluator::RuntimeExpr> = rt_expr.as_deref();
                (synthetic, rt)
            }
            _ => {
                // Literal/variable — evaluate as expression; result is discarded.
                if let Some(rt) = rt_expr {
                    let _ = eval_env.eval_runtime_expr(token, rule_info, rt, context);
                } else {
                    let _ = eval_env.eval_expr(token, rule_info, action_expr, context);
                }
                continue;
            }
        };
        execute_single_action(
            reset_requested,
            clear_requested,
            token,
            rule_info,
            &branch_call,
            branch_runtime,
            context,
            eval_env,
        )?;
        if context.engine.is_halted() || *reset_requested || *clear_requested {
            break;
        }
    }
    Ok(())
}

/// Execute a fact-query macro action.
///
/// Handles `do-for-fact`, `do-for-all-facts`, `delayed-do-for-all-facts`,
/// `any-factp`, `find-fact`, and `find-all-facts`.
///
/// For action forms (`do-for-*`), executes `body` for each matching fact and
/// returns `Ok(())`. For expression forms (`any-factp`, `find-*`), the return
/// value cannot be propagated here; call-sites that need it should go through
/// `eval()` instead (which returns a default value since it lacks fact-base
/// access — see `RuntimeExpr::QueryAction` eval arm).
#[allow(clippy::too_many_arguments)]
fn execute_query_action(
    reset_requested: &mut bool,
    clear_requested: &mut bool,
    token: &Token,
    rule_info: &CompiledRuleInfo,
    name: &str,
    bindings: &[(String, String)],
    query: &crate::evaluator::RuntimeExpr,
    body: &[(
        ferric_parser::ActionExpr,
        Option<Box<crate::evaluator::RuntimeExpr>>,
    )],
    context: &mut ActionExecutionContext<'_>,
    eval_env: &mut ActionEvalEnv,
) -> Result<(), ActionError> {
    // Collect matching fact IDs.
    //
    // For multi-binding forms (e.g. `(do-for-all-facts ((?a T1) (?b T2)) ...)`),
    // we iterate the cross-product of each binding's fact set.  For the common
    // single-binding case this degenerates to a simple loop.
    //
    // We collect all IDs first to avoid borrow issues when executing the body
    // (body actions may assert/retract facts, which would mutate the fact base
    // while we're iterating it).  This also serves as the snapshot required by
    // `delayed-do-for-all-facts`.

    // Resolve each binding to (variable_name, Vec<FactId>).
    let mut binding_fact_ids: Vec<(String, Vec<FactId>)> = Vec::with_capacity(bindings.len());
    for (var_name, template_name) in bindings {
        let Some(&tid) = context.engine.template_ids.get(template_name.as_str()) else {
            // Unknown template — no facts match; result is empty / FALSE.
            return Ok(());
        };
        let ids: Vec<FactId> = context.engine.fact_base.facts_by_template(tid).collect();
        binding_fact_ids.push((var_name.clone(), ids));
    }

    if binding_fact_ids.is_empty() {
        return Ok(());
    }

    // For simplicity, implement the cross-product as a recursive-style iteration
    // over the bindings list. We build candidate tuples iteratively.
    // A "candidate" is a Vec of (var_name, FactId) assignments — one per binding.

    // Seed with single-element tuples for the first binding.
    let (first_var, first_ids) = &binding_fact_ids[0];
    let mut candidates: Vec<Vec<(String, FactId)>> = first_ids
        .iter()
        .map(|&id| vec![(first_var.clone(), id)])
        .collect();

    // Extend with remaining bindings.
    for (var_name, ids) in &binding_fact_ids[1..] {
        let mut next = Vec::with_capacity(candidates.len() * ids.len());
        for candidate in &candidates {
            for &id in ids {
                let mut extended = candidate.clone();
                extended.push((var_name.clone(), id));
                next.push(extended);
            }
        }
        candidates = next;
    }

    // Now iterate candidates, filter by query, execute body as appropriate.
    let stop_after_first = name == "do-for-fact";
    let is_action_form = matches!(
        name,
        "do-for-fact" | "do-for-all-facts" | "delayed-do-for-all-facts"
    );

    for candidate in &candidates {
        // Build augmented (token, rule_info) with all binding variables set.
        let mut aug_token = token.clone();
        let mut aug_rule_info = rule_info_clone_light(rule_info);

        for (var_name, fact_id) in candidate {
            // Represent the FactId as an integer value.
            #[allow(clippy::cast_possible_wrap)] // FactId ffi is u64; wrap is acceptable here
            let fact_val = Value::Integer(fact_id.data().as_ffi() as i64);
            let (new_token, new_rule_info) = augment_bindings_with_var(
                &aug_token,
                &aug_rule_info,
                var_name,
                fact_val,
                &mut context.engine.symbol_table,
                &context.engine.config,
            )?;
            aug_token = new_token;
            aug_rule_info = new_rule_info;
        }

        // Evaluate the query expression.
        let query_val = {
            let mut ctx = ActionEvalEnv::make_eval_context(&aug_token, &aug_rule_info, context);
            crate::evaluator::eval(&mut ctx, query).map_err(ActionError::from)?
        };

        if !crate::evaluator::is_truthy(&query_val, &context.engine.symbol_table) {
            continue;
        }

        if is_action_form {
            execute_loop_body(
                reset_requested,
                clear_requested,
                &aug_token,
                &aug_rule_info,
                body,
                context,
                eval_env,
            )?;
            if context.engine.is_halted() || *reset_requested || *clear_requested {
                break;
            }
            if stop_after_first {
                break;
            }
        }
        // For expression forms (any-factp, find-fact, find-all-facts) we
        // can't return the value from this action-execution path; callers
        // that need a return value should use the eval() path instead.
    }

    Ok(())
}

/// Execute a `focus` action: push module(s) onto the focus stack.
///
/// Arguments are evaluated to symbols and collected as focus requests.
/// They are applied by the engine after all actions complete, in reverse
/// order so the first argument becomes the top of the focus stack.
#[allow(clippy::too_many_arguments)]
fn execute_focus(
    token: &Token,
    rule_info: &CompiledRuleInfo,
    args: &[ActionExpr],
    context: &mut ActionExecutionContext<'_>,
    eval_env: &mut ActionEvalEnv,
) -> Result<(), ActionError> {
    for arg in args {
        let value = eval_env.eval_expr(token, rule_info, arg, context)?;
        match value {
            Value::Symbol(sym) => {
                if let Some(name) = context.engine.symbol_table.resolve_symbol_str(sym) {
                    if context.engine.module_registry.get_by_name(name).is_none() {
                        return Err(ActionError::EvalError(format!(
                            "focus: unknown module `{name}`"
                        )));
                    }
                    context.focus_requests.push(name.to_string());
                }
            }
            _ => {
                return Err(ActionError::EvalError(
                    "focus: expected symbol argument".to_string(),
                ));
            }
        }
    }
    Ok(())
}

/// Execute a `list-focus-stack` action: print the focus stack to the `t` channel.
///
/// Prints each module name on its own line, top of stack first.
#[allow(clippy::unnecessary_wraps)] // Consistent with other action-handler return type
fn execute_list_focus_stack(
    router: &mut OutputRouter,
    module_registry: &ModuleRegistry,
) -> Result<(), ActionError> {
    let stack = module_registry.focus_stack();
    let mut output = String::new();
    // Print top-first (reverse of internal order)
    for &module_id in stack.iter().rev() {
        let name = module_registry.module_name(module_id).unwrap_or("???");
        output.push_str(name);
        output.push('\n');
    }
    router.write("t", &output);
    Ok(())
}

/// Execute an `agenda` action: print the current agenda to the `t` channel.
///
/// Format: one line per activation showing `salience rule-name`.
/// When the agenda is empty, prints `(no activations)`.
#[allow(clippy::unnecessary_wraps)] // Consistent with other action-handler return type
fn execute_agenda(
    rete: &ReteNetwork,
    router: &mut OutputRouter,
    all_rule_info: &crate::engine::RuleIndex<Rc<CompiledRuleInfo>>,
) -> Result<(), ActionError> {
    let mut output = String::new();
    for activation in rete.agenda.iter_activations() {
        let rule_name = crate::engine::rule_index_get(all_rule_info, activation.rule)
            .map_or("???", |info| info.name.as_str());
        let _ = writeln!(output, "{} {rule_name}", activation.salience.get());
    }
    if output.is_empty() {
        output.push_str("(no activations)\n");
    }
    router.write("t", &output);
    Ok(())
}

/// Execute a `rules` action: print known rule names to the `t` channel.
#[allow(clippy::too_many_arguments)] // Keep call-site symmetry with other action handlers.
fn execute_rules(
    token: &Token,
    rule_info: &CompiledRuleInfo,
    args: &[ActionExpr],
    context: &mut ActionExecutionContext<'_>,
    eval_env: &mut ActionEvalEnv,
) -> Result<(), ActionError> {
    if args.len() > 1 {
        return Err(ActionError::EvalError(format!(
            "rules: expected 0 or 1 arguments, got {}",
            args.len()
        )));
    }

    if let Some(module_arg) = args.first() {
        let _ = eval_env.eval_expr(token, rule_info, module_arg, context)?;
    }

    let mut names: Vec<&str> = context
        .engine
        .rule_info
        .iter()
        .flatten()
        .map(|info| info.name.as_str())
        .collect();
    names.sort_unstable();
    names.dedup();

    let mut output = String::new();
    for name in names {
        output.push_str(name);
        output.push('\n');
    }
    if output.is_empty() {
        output.push_str("(no rules)\n");
    }
    context.engine.router.write("t", &output);
    Ok(())
}

#[allow(clippy::too_many_arguments)] // Keep call-site symmetry with other action handlers.
fn execute_undefrule(
    token: &Token,
    rule_info: &CompiledRuleInfo,
    args: &[ActionExpr],
    context: &mut ActionExecutionContext<'_>,
    eval_env: &mut ActionEvalEnv,
) -> Result<(), ActionError> {
    if args.is_empty() {
        return Err(ActionError::EvalError(
            "undefrule: expected at least 1 argument".to_string(),
        ));
    }

    let selectors =
        evaluated_rule_selectors("undefrule", token, rule_info, args, context, eval_env)?;
    let selected = selected_rule_ids(
        &selectors,
        &context.engine.rule_info,
        &context.engine.rule_modules,
        &context.engine.module_registry,
        "undefrule",
    )?;

    for rule_id in selected {
        if let Some(slot) = context.engine.rule_info.get_mut(rule_id.0 as usize) {
            *slot = None;
        }
        if let Some(slot) = context.engine.rule_modules.get_mut(rule_id.0 as usize) {
            *slot = None;
        }
        context.engine.rete.disable_rule(rule_id);
    }

    Ok(())
}

#[allow(clippy::too_many_arguments)] // Keep call-site symmetry with other action handlers.
fn execute_ppdefrule(
    token: &Token,
    rule_info: &CompiledRuleInfo,
    args: &[ActionExpr],
    context: &mut ActionExecutionContext<'_>,
    eval_env: &mut ActionEvalEnv,
) -> Result<(), ActionError> {
    if args.len() != 1 {
        return Err(ActionError::EvalError(format!(
            "ppdefrule: expected exactly 1 argument, got {}",
            args.len()
        )));
    }

    let selectors =
        evaluated_rule_selectors("ppdefrule", token, rule_info, args, context, eval_env)?;
    let selected = selected_rule_ids(
        &selectors,
        &context.engine.rule_info,
        &context.engine.rule_modules,
        &context.engine.module_registry,
        "ppdefrule",
    )?;

    if selected.is_empty() {
        return Ok(());
    }

    let mut ordered_ids: Vec<_> = selected.into_iter().collect();
    ordered_ids.sort_by(|left, right| {
        let left_name = crate::engine::rule_index_get(&context.engine.rule_info, *left)
            .map_or("???", |info| info.name.as_str())
            .to_ascii_lowercase();
        let right_name = crate::engine::rule_index_get(&context.engine.rule_info, *right)
            .map_or("???", |info| info.name.as_str())
            .to_ascii_lowercase();

        let left_module = crate::engine::rule_index_get(&context.engine.rule_modules, *left)
            .and_then(|module_id| context.engine.module_registry.module_name(*module_id))
            .unwrap_or("")
            .to_ascii_lowercase();
        let right_module = crate::engine::rule_index_get(&context.engine.rule_modules, *right)
            .and_then(|module_id| context.engine.module_registry.module_name(*module_id))
            .unwrap_or("")
            .to_ascii_lowercase();

        (left_name, left_module, left.0).cmp(&(right_name, right_module, right.0))
    });

    let mut output = String::new();
    for rule_id in ordered_ids {
        let Some(compiled) = crate::engine::rule_index_get(&context.engine.rule_info, rule_id)
        else {
            continue;
        };

        if let Some(source) = compiled.source_definition.as_deref() {
            output.push_str(source);
            output.push('\n');
            continue;
        }

        let qualified_name = if let Some(module_id) =
            crate::engine::rule_index_get(&context.engine.rule_modules, rule_id)
        {
            if let Some(module_name) = context.engine.module_registry.module_name(*module_id) {
                format!("{module_name}::{}", compiled.name)
            } else {
                compiled.name.clone()
            }
        } else {
            compiled.name.clone()
        };
        let _ = writeln!(output, "(defrule {qualified_name} ...)");
    }

    if !output.is_empty() {
        context.engine.router.write("t", &output);
    }

    Ok(())
}

fn execute_load(
    token: &Token,
    rule_info: &CompiledRuleInfo,
    args: &[ActionExpr],
    context: &mut ActionExecutionContext<'_>,
    eval_env: &mut ActionEvalEnv,
) -> Result<(), ActionError> {
    if args.len() != 1 {
        return Err(ActionError::EvalError(format!(
            "load: expected exactly 1 argument, got {}",
            args.len()
        )));
    }

    let selector = eval_env.eval_expr(token, rule_info, &args[0], context)?;
    let path_text = match selector {
        Value::String(s) => s.as_str().to_string(),
        Value::Symbol(sym) => context
            .engine
            .symbol_table
            .resolve_symbol_str(sym)
            .unwrap_or("???")
            .to_string(),
        other => {
            return Err(ActionError::EvalError(format!(
                "load: expected STRING or SYMBOL, got {}",
                runtime_value_type_name(&other)
            )))
        }
    };

    let path = Path::new(&path_text);
    let saved_module = context.engine.module_registry.current_module();
    let load_result = context.engine.load_file(path);
    context
        .engine
        .module_registry
        .set_current_module(saved_module);

    match load_result {
        Ok(_) => Ok(()),
        Err(errors) => {
            let message = errors
                .into_iter()
                .map(|error| error.to_string())
                .collect::<Vec<_>>()
                .join("; ");
            Err(ActionError::EvalError(format!(
                "load failed for `{path_text}`: {message}"
            )))
        }
    }
}

fn evaluated_rule_selectors(
    command_name: &str,
    token: &Token,
    rule_info: &CompiledRuleInfo,
    args: &[ActionExpr],
    context: &mut ActionExecutionContext<'_>,
    eval_env: &mut ActionEvalEnv,
) -> Result<Vec<String>, ActionError> {
    let mut selectors = Vec::with_capacity(args.len());
    for arg in args {
        let value = eval_env.eval_expr(token, rule_info, arg, context)?;
        let selector = match value {
            Value::Symbol(symbol) => context
                .engine
                .symbol_table
                .resolve_symbol_str(symbol)
                .unwrap_or("???")
                .to_string(),
            Value::String(s) => s.as_str().to_string(),
            other => {
                return Err(ActionError::EvalError(format!(
                    "{command_name}: expected SYMBOL or STRING, got {}",
                    runtime_value_type_name(&other)
                )))
            }
        };
        selectors.push(selector);
    }
    Ok(selectors)
}

fn selected_rule_ids(
    selectors: &[String],
    all_rule_info: &crate::engine::RuleIndex<Rc<CompiledRuleInfo>>,
    rule_modules: &crate::engine::RuleIndex<crate::modules::ModuleId>,
    module_registry: &ModuleRegistry,
    command_name: &str,
) -> Result<HashSet<RuleId>, ActionError> {
    let mut selected = HashSet::new();
    for selector in selectors {
        if selector == "*" {
            selected.extend(
                all_rule_info
                    .iter()
                    .enumerate()
                    .filter_map(|(index, compiled)| {
                        #[allow(clippy::cast_possible_truncation)]
                        compiled.as_ref().map(|_| RuleId(index as u32))
                    }),
            );
            continue;
        }

        if let Some(module_name) = selector.strip_suffix("::") {
            if module_name.is_empty() {
                return Err(ActionError::EvalError(format!(
                    "{command_name}: empty module name in selector"
                )));
            }
            if let Some(module_id) = module_registry.get_by_name(module_name) {
                for (index, owner_module) in rule_modules.iter().enumerate() {
                    let Some(owner_module) = owner_module else {
                        continue;
                    };
                    #[allow(clippy::cast_possible_truncation)]
                    let rule_id = RuleId(index as u32);
                    if *owner_module == module_id
                        && crate::engine::rule_index_get(all_rule_info, rule_id).is_some()
                    {
                        selected.insert(rule_id);
                    }
                }
            }
            continue;
        }

        let parsed = parse_qualified_name(selector).map_err(|err| {
            ActionError::EvalError(format!("{command_name}: invalid selector: {err}"))
        })?;

        match parsed {
            QualifiedName::Unqualified(name) => {
                for (index, compiled) in all_rule_info.iter().enumerate() {
                    let Some(compiled) = compiled.as_ref() else {
                        continue;
                    };
                    if compiled.name == name {
                        #[allow(clippy::cast_possible_truncation)]
                        selected.insert(RuleId(index as u32));
                    }
                }
            }
            QualifiedName::Qualified { module, name } => {
                let Some(module_id) = module_registry.get_by_name(&module) else {
                    continue;
                };
                for (index, compiled) in all_rule_info.iter().enumerate() {
                    let Some(compiled) = compiled.as_ref() else {
                        continue;
                    };
                    if compiled.name != name {
                        continue;
                    }
                    #[allow(clippy::cast_possible_truncation)]
                    let rule_id = RuleId(index as u32);
                    if crate::engine::rule_index_get(rule_modules, rule_id).copied()
                        == Some(module_id)
                    {
                        selected.insert(rule_id);
                    }
                }
            }
        }
    }

    Ok(selected)
}

fn runtime_value_type_name(value: &Value) -> &'static str {
    match value {
        Value::Integer(_) => "INTEGER",
        Value::Float(_) => "FLOAT",
        Value::Symbol(_) => "SYMBOL",
        Value::String(_) => "STRING",
        Value::Multifield(_) => "MULTIFIELD",
        Value::ExternalAddress(_) => "EXTERNAL-ADDRESS",
        Value::Void => "VOID",
    }
}

/// Execute a `printout` action.
///
/// The first argument is the channel name (typically `t`) and must be a literal.
/// Remaining arguments are evaluated and formatted, with the special symbols
/// `crlf`, `tab`, and `ff` producing `\n`, `\t`, and `\x0C` respectively.
#[allow(clippy::too_many_arguments)] // Context requires all these parameters
fn execute_printout(
    token: &Token,
    rule_info: &CompiledRuleInfo,
    args: &[ActionExpr],
    context: &mut ActionExecutionContext<'_>,
    eval_env: &mut ActionEvalEnv,
) -> Result<(), ActionError> {
    if args.is_empty() {
        return Err(ActionError::EvalError(
            "printout requires at least a channel argument".to_string(),
        ));
    }

    // First argument is the channel name and must be a literal token.
    let channel = match &args[0] {
        ActionExpr::Literal(lit) => match &lit.value {
            LiteralKind::Symbol(s) | LiteralKind::String(s) => s.clone(),
            LiteralKind::Integer(n) => n.to_string(),
            LiteralKind::Float(f) => f.to_string(),
        },
        _ => {
            return Err(ActionError::EvalError(
                "printout: channel must be a literal symbol or string".to_string(),
            ))
        }
    };

    // Evaluate and format remaining arguments.
    let mut output = String::new();
    for arg in &args[1..] {
        let value = eval_env.eval_expr(token, rule_info, arg, context)?;
        flush_deferred_printout(context);
        format_printout_value(&value, &context.engine.symbol_table, &mut output);
    }

    context.engine.router.write(&channel, &output);
    Ok(())
}

/// Execute a `println` action.
///
/// Behaves like `(printout t <args> crlf)`: arguments are evaluated and
/// formatted using the same rules as `printout`, output is written to `t`,
/// and a trailing newline is appended.
#[allow(clippy::too_many_arguments)] // Context requires all these parameters
fn execute_println(
    token: &Token,
    rule_info: &CompiledRuleInfo,
    args: &[ActionExpr],
    context: &mut ActionExecutionContext<'_>,
    eval_env: &mut ActionEvalEnv,
) -> Result<(), ActionError> {
    let mut output = String::new();
    for arg in args {
        let value = eval_env.eval_expr(token, rule_info, arg, context)?;
        flush_deferred_printout(context);
        format_printout_value(&value, &context.engine.symbol_table, &mut output);
    }
    output.push('\n');
    context.engine.router.write("t", &output);
    Ok(())
}

/// Format a `Value` for `printout` output.
///
/// Special symbols `crlf`, `tab`, and `ff` are expanded to their control
/// characters. All other values are formatted as their display string.
/// Strings are written without surrounding quotes.
fn format_printout_value(value: &Value, symbol_table: &SymbolTable, output: &mut String) {
    match value {
        Value::Integer(n) => output.push_str(&n.to_string()),
        Value::Float(f) => {
            // CLIPS always shows a decimal point: 3.0 not 3.
            if f.fract() == 0.0 {
                // Use write! to avoid the intermediate String allocation
                // that clippy::format_push_string warns about.
                let _ = write!(output, "{f:.1}");
            } else {
                output.push_str(&f.to_string());
            }
        }
        Value::Symbol(sym) => {
            if let Some(name) = symbol_table.resolve_symbol_str(*sym) {
                match name {
                    "crlf" => output.push('\n'),
                    "tab" => output.push('\t'),
                    "ff" => output.push('\x0C'),
                    other => output.push_str(other),
                }
            }
        }
        Value::String(s) => output.push_str(s.as_str()),
        Value::Void => {}
        Value::ExternalAddress(_) => output.push_str("<ExternalAddress>"),
        Value::Multifield(mf) => {
            output.push('(');
            for (i, v) in mf.as_slice().iter().enumerate() {
                if i > 0 {
                    output.push(' ');
                }
                format_printout_value(v, symbol_table, output);
            }
            output.push(')');
        }
    }
}

#[allow(clippy::too_many_arguments)] // Context requires all these parameters
fn execute_assert(
    token: &Token,
    rule_info: &CompiledRuleInfo,
    args: &[ActionExpr],
    context: &mut ActionExecutionContext<'_>,
    eval_env: &mut ActionEvalEnv,
) -> Result<(), ActionError> {
    // Each argument to assert should be a "function call" representing a fact pattern
    // e.g., (assert (relation val1 val2)) → args = [FunctionCall("relation", [val1, val2])]
    for arg in args {
        match arg {
            ActionExpr::FunctionCall(fact_pattern) => {
                let relation = &fact_pattern.name;
                let relation_sym = context
                    .engine
                    .symbol_table
                    .intern_symbol(relation, context.engine.config.string_encoding)
                    .map_err(ActionError::from)?;

                let mut fields = smallvec::SmallVec::new();
                for field_expr in &fact_pattern.args {
                    let value = eval_env.eval_expr(token, rule_info, field_expr, context)?;
                    match value {
                        // CLIPS splices multifield values into ordered assertions.
                        Value::Multifield(mf) => fields.extend(mf.as_slice().iter().cloned()),
                        other => fields.push(other),
                    }
                }

                assert_ordered_and_propagate(
                    &mut context.engine.fact_base,
                    &mut context.engine.rete,
                    relation_sym,
                    fields,
                );
            }
            _ => return Err(ActionError::InvalidAssert),
        }
    }
    Ok(())
}

fn execute_retract(
    fact_base: &mut FactBase,
    rete: &mut ReteNetwork,
    token: &Token,
    rule_info: &CompiledRuleInfo,
    args: &[ActionExpr],
) -> Result<(), ActionError> {
    for arg in args {
        match arg {
            ActionExpr::Variable(var_name, _) => {
                let fact_id = resolve_fact_address(token, rule_info, var_name)?;
                let fact = get_fact_or_error(fact_base, fact_id)?;
                rete.retract_fact(fact_id, &fact, fact_base);
                fact_base.retract(fact_id);
            }
            _ => return Err(ActionError::InvalidRetract),
        }
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)] // Context requires all these parameters
fn execute_modify(
    token: &Token,
    rule_info: &CompiledRuleInfo,
    args: &[ActionExpr],
    context: &mut ActionExecutionContext<'_>,
    eval_env: &mut ActionEvalEnv,
) -> Result<(), ActionError> {
    execute_fact_mutation(
        token,
        rule_info,
        args,
        FactMutationMode::Modify,
        context,
        eval_env,
    )
}

#[allow(clippy::too_many_arguments)] // Context requires all these parameters
fn execute_duplicate(
    token: &Token,
    rule_info: &CompiledRuleInfo,
    args: &[ActionExpr],
    context: &mut ActionExecutionContext<'_>,
    eval_env: &mut ActionEvalEnv,
) -> Result<(), ActionError> {
    execute_fact_mutation(
        token,
        rule_info,
        args,
        FactMutationMode::Duplicate,
        context,
        eval_env,
    )
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum FactMutationMode {
    Modify,
    Duplicate,
}

impl FactMutationMode {
    fn retract_original(self) -> bool {
        matches!(self, Self::Modify)
    }
}

#[allow(clippy::too_many_arguments)] // Context requires all these parameters
fn execute_fact_mutation(
    token: &Token,
    rule_info: &CompiledRuleInfo,
    args: &[ActionExpr],
    mode: FactMutationMode,
    context: &mut ActionExecutionContext<'_>,
    eval_env: &mut ActionEvalEnv,
) -> Result<(), ActionError> {
    let fact_id = resolve_target_fact_id(args, token, rule_info)?;
    let original_fact = get_fact_or_error(&context.engine.fact_base, fact_id)?;

    match &original_fact {
        Fact::Ordered(ordered) => {
            let relation = ordered.relation;
            let mut fields = ordered.fields.clone();
            apply_ordered_slot_overrides(
                &mut fields,
                &args[1..],
                token,
                rule_info,
                context,
                eval_env,
            )?;
            if mode.retract_original() {
                retract_original_fact(
                    &mut context.engine.fact_base,
                    &mut context.engine.rete,
                    fact_id,
                    &original_fact,
                );
            }
            assert_ordered_and_propagate(
                &mut context.engine.fact_base,
                &mut context.engine.rete,
                relation,
                fields,
            );
        }
        Fact::Template(template) => {
            let registered = context
                .engine
                .template_defs
                .get(template.template_id)
                .cloned()
                .ok_or_else(|| {
                    ActionError::UnknownAction(format!(
                        "template ID {:?} not found in registry",
                        template.template_id
                    ))
                })?;
            let mut slots = template.slots.to_vec();
            apply_template_slot_overrides(
                &mut slots,
                &args[1..],
                &registered,
                token,
                rule_info,
                context,
                eval_env,
            )?;
            if mode.retract_original() {
                retract_original_fact(
                    &mut context.engine.fact_base,
                    &mut context.engine.rete,
                    fact_id,
                    &original_fact,
                );
            }
            assert_template_and_propagate(
                &mut context.engine.fact_base,
                &mut context.engine.rete,
                template.template_id,
                slots.into_boxed_slice(),
            );
        }
    }

    Ok(())
}

fn assert_ordered_and_propagate(
    fact_base: &mut FactBase,
    rete: &mut ReteNetwork,
    relation: Symbol,
    fields: OrderedFields,
) -> FactId {
    let fact_id = fact_base.assert_ordered(relation, fields);
    let fact = &fact_base
        .get(fact_id)
        .expect("asserted fact should be present in fact base")
        .fact;
    rete.assert_fact(fact_id, fact, fact_base);
    fact_id
}

fn assert_template_and_propagate(
    fact_base: &mut FactBase,
    rete: &mut ReteNetwork,
    template_id: TemplateId,
    slots: Box<[Value]>,
) -> FactId {
    let fact_id = fact_base.assert_template(template_id, slots);
    let fact = &fact_base
        .get(fact_id)
        .expect("asserted fact should be present in fact base")
        .fact;
    rete.assert_fact(fact_id, fact, fact_base);
    fact_id
}

fn retract_original_fact(
    fact_base: &mut FactBase,
    rete: &mut ReteNetwork,
    fact_id: FactId,
    fact: &Fact,
) {
    rete.retract_fact(fact_id, fact, fact_base);
    fact_base.retract(fact_id);
}

fn get_fact_or_error(fact_base: &FactBase, fact_id: FactId) -> Result<Fact, ActionError> {
    fact_base
        .get(fact_id)
        .map(|entry| entry.fact.clone())
        .ok_or(ActionError::FactNotFound(fact_id))
}

fn resolve_target_fact_id(
    args: &[ActionExpr],
    token: &Token,
    rule_info: &CompiledRuleInfo,
) -> Result<FactId, ActionError> {
    if args.is_empty() {
        return Err(ActionError::InvalidRetract);
    }

    match &args[0] {
        ActionExpr::Variable(var_name, _) => resolve_fact_address(token, rule_info, var_name),
        _ => Err(ActionError::InvalidRetract),
    }
}

#[allow(clippy::too_many_arguments)] // Context requires all these parameters
fn apply_ordered_slot_overrides(
    fields: &mut OrderedFields,
    slot_overrides: &[ActionExpr],
    token: &Token,
    rule_info: &CompiledRuleInfo,
    context: &mut ActionExecutionContext<'_>,
    eval_env: &mut ActionEvalEnv,
) -> Result<(), ActionError> {
    // In CLIPS, modify uses (slot-name value) syntax. For ordered facts in Phase 2,
    // we interpret FunctionCall args as positional overrides where the "name" is the index.
    // But the more common usage is with template facts, which we don't fully support yet.
    for slot_override in slot_overrides {
        let ActionExpr::FunctionCall(fc) = slot_override else {
            continue;
        };

        let Ok(index) = fc.name.parse::<usize>() else {
            continue;
        };
        if index >= fields.len() {
            continue;
        }

        if let Some(first_arg) = fc.args.first() {
            fields[index] = eval_env.eval_expr(token, rule_info, first_arg, context)?;
        }
    }

    Ok(())
}

/// Apply slot overrides to a mutable template slot vector.
///
/// Each override in `slot_overrides` is expected to be a `FunctionCall` whose
/// name is a slot name and whose first argument is the new value.  Unknown slot
/// names return an `EvalError`.
#[allow(clippy::too_many_arguments)] // Context requires all these parameters
fn apply_template_slot_overrides(
    slots: &mut [Value],
    slot_overrides: &[ActionExpr],
    registered: &RegisteredTemplate,
    token: &Token,
    rule_info: &CompiledRuleInfo,
    context: &mut ActionExecutionContext<'_>,
    eval_env: &mut ActionEvalEnv,
) -> Result<(), ActionError> {
    for slot_override in slot_overrides {
        let ActionExpr::FunctionCall(fc) = slot_override else {
            continue;
        };

        let slot_idx = registered
            .slot_index
            .get(&fc.name)
            .copied()
            .ok_or_else(|| {
                ActionError::EvalError(format!(
                    "unknown slot `{}` in template `{}`",
                    fc.name, registered.name
                ))
            })?;

        if slot_idx >= slots.len() {
            return Err(ActionError::EvalError(format!(
                "slot index {slot_idx} out of bounds for template `{}`",
                registered.name
            )));
        }

        if let Some(first_arg) = fc.args.first() {
            slots[slot_idx] = eval_env.eval_expr(token, rule_info, first_arg, context)?;
        }
    }

    Ok(())
}

/// Resolve a fact-address variable to a `FactId`.
fn resolve_fact_address(
    token: &Token,
    rule_info: &CompiledRuleInfo,
    var_name: &str,
) -> Result<FactId, ActionError> {
    if let Some(&fact_index) = rule_info.fact_address_vars.get(var_name) {
        token
            .facts
            .get(fact_index)
            .copied()
            .ok_or_else(|| ActionError::UnboundVariable(var_name.to_string()))
    } else {
        Err(ActionError::UnboundVariable(var_name.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn action_error_display() {
        let err = ActionError::UnknownAction("foo".to_string());
        assert!(format!("{err}").contains("foo"));
    }

    #[test]
    fn action_error_unbound_variable() {
        let err = ActionError::UnboundVariable("x".to_string());
        assert!(format!("{err}").contains('x'));
    }
}
