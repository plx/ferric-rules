//! RHS action execution for rule firings.
//!
//! ## Phase 3 scope
//!
//! - `GlobalVariable` reads and writes via `GlobalStore` (Pass 006).
//! - `modify`/`duplicate` support template-aware slot overrides (Pass 003).
//! - `printout` with per-channel output capture via `OutputRouter` (Pass 004).

use std::collections::{HashMap, HashSet};
use std::fmt::Write as FmtWrite;
use std::io::Write as IoWrite;
use std::path::Path;
use std::sync::Arc;

use ferric_rules_core::beta::{RuleId, Salience};
use ferric_rules_core::binding::{BindingSet, ValueRef, VarMap};
use ferric_rules_core::token::Token;
use ferric_rules_core::{
    EncodingError, Fact, FactBase, FactId, OrderedFact, ReteNetwork, Symbol, SymbolTable,
    TemplateId, Value,
};
use ferric_rules_parser::{Action, ActionExpr, FunctionCall, LiteralKind};
use slotmap::Key as _;

use crate::evaluator::CompactFactBinding;
use crate::modules::ModuleRegistry;
use crate::qualified_name::{parse_qualified_name, QualifiedName};
use crate::router::OutputRouter;
use crate::templates::RegisteredTemplate;
use crate::tracing_support::{ferric_event, ferric_span};
use crate::Engine;

type OrderedFields = smallvec::SmallVec<[Value; 8]>;
type RuntimeBindingEnv = HashMap<String, Value>;

pub(crate) struct ActionExecutionContext<'a> {
    pub engine: &'a mut Engine,
    pub current_module: crate::modules::ModuleId,
}

#[derive(Default)]
struct ActionEvalEnv {
    /// Captured once per activation; remains available after supporting facts
    /// are retracted. Inner token bindings and explicit RHS locals override it.
    fact_bindings: RuntimeBindingEnv,
    /// Query membership has lexical scope independent of ordinary loop values.
    compact_facts: crate::evaluator::CompactFactBindings,
    runtime_bindings: RuntimeBindingEnv,
}

fn flush_deferred_printout(context: &mut ActionExecutionContext<'_>) {
    for (channel, text) in context.engine.globals.take_printout_events() {
        context.engine.router.write(&channel, &text);
    }
}

impl ActionEvalEnv {
    /// Mask outer RHS locals while query/loop bindings are in scope, then
    /// restore them even when the body returns an error or a rule return.
    fn with_local_scope<'a, T>(
        &mut self,
        names: impl IntoIterator<Item = &'a str>,
        execute: impl FnOnce(&mut Self) -> Result<T, ActionError>,
    ) -> Result<T, ActionError> {
        let mut saved = HashMap::new();
        for name in names {
            let name = name.strip_prefix("$?").unwrap_or(name);
            saved
                .entry(name.to_string())
                .or_insert_with(|| self.runtime_bindings.remove(name));
        }
        let result = execute(self);
        for (name, previous) in saved {
            if let Some(value) = previous {
                self.runtime_bindings.insert(name, value);
            } else {
                self.runtime_bindings.remove(&name);
            }
        }
        result
    }

    /// Compact references follow query membership even when a loop reuses the
    /// ordinary variable name. Nested queries temporarily replace that member.
    fn with_compact_scope<T>(
        &mut self,
        bindings: &[(String, CompactFactBinding)],
        execute: impl FnOnce(&mut Self) -> Result<T, ActionError>,
    ) -> Result<T, ActionError> {
        let mut saved = HashMap::new();
        for (name, fact_id) in bindings {
            let previous = self.compact_facts.insert(name.clone(), fact_id.clone());
            saved.entry(name.clone()).or_insert(previous);
        }
        let result = execute(self);
        for (name, previous) in saved {
            if let Some(fact_id) = previous {
                self.compact_facts.insert(name, fact_id);
            } else {
                self.compact_facts.remove(&name);
            }
        }
        result
    }

    fn make_eval_context<'ctx>(
        token: &'ctx Token,
        rule_info: &'ctx CompiledRuleInfo,
        context: &'ctx mut ActionExecutionContext<'_>,
        compact_facts: &'ctx crate::evaluator::CompactFactBindings,
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
            expression_depth: 0,
            callable_locals: None,
            current_module: context.current_module,
            module_registry: &engine.module_registry,
            function_modules: &engine.function_modules,
            global_modules: &engine.global_modules,
            generic_modules: &engine.generic_modules,
            method_chain: None,
            input_buffer: Some(&mut engine.input_buffer),
            fact_base: Some(&engine.fact_base),
            initial_fact_id: engine.initial_fact_id,
            template_defs: Some(&engine.template_defs),
            compact_fact_bindings: Some(compact_facts),
            template_resolver: Some(crate::loader::TemplateResolver {
                template_local_ids: &engine.template_local_ids,
                template_modules: &engine.template_modules,
                module_registry: &engine.module_registry,
            }),
        }
    }

    fn eval_runtime_expr(
        &mut self,
        token: &Token,
        rule_info: &CompiledRuleInfo,
        runtime_expr: &crate::evaluator::RuntimeExpr,
        context: &mut ActionExecutionContext<'_>,
    ) -> Result<Value, ActionError> {
        if !self.fact_bindings.is_empty() || !self.runtime_bindings.is_empty() {
            let (bindings, var_map) = build_runtime_eval_bindings(
                token,
                rule_info,
                &self.fact_bindings,
                &self.runtime_bindings,
                context,
            )?;
            return Self::eval_runtime_expr_with_bindings(
                runtime_expr,
                &bindings,
                &var_map,
                context,
                &self.compact_facts,
            );
        }

        let mut ctx = Self::make_eval_context(token, rule_info, context, &self.compact_facts);
        crate::evaluator::eval(&mut ctx, runtime_expr).map_err(ActionError::from)
    }

    fn eval_expr(
        &mut self,
        token: &Token,
        rule_info: &CompiledRuleInfo,
        expr: &ActionExpr,
        context: &mut ActionExecutionContext<'_>,
        _collected_facts: &[FactId],
    ) -> Result<Value, ActionError> {
        let runtime_expr = crate::evaluator::from_action_expr(
            expr,
            &mut context.engine.symbol_table,
            &context.engine.config,
        )
        .map_err(ActionError::from)?;
        self.eval_runtime_expr(token, rule_info, &runtime_expr, context)
    }

    fn eval_runtime_expr_with_bindings(
        runtime_expr: &crate::evaluator::RuntimeExpr,
        bindings: &BindingSet,
        var_map: &VarMap,
        context: &mut ActionExecutionContext<'_>,
        compact_facts: &crate::evaluator::CompactFactBindings,
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
            expression_depth: 0,
            callable_locals: None,
            current_module: context.current_module,
            module_registry: &engine.module_registry,
            function_modules: &engine.function_modules,
            global_modules: &engine.global_modules,
            generic_modules: &engine.generic_modules,
            method_chain: None,
            input_buffer: Some(&mut engine.input_buffer),
            fact_base: Some(&engine.fact_base),
            initial_fact_id: engine.initial_fact_id,
            template_defs: Some(&engine.template_defs),
            compact_fact_bindings: Some(compact_facts),
            template_resolver: Some(crate::loader::TemplateResolver {
                template_local_ids: &engine.template_local_ids,
                template_modules: &engine.template_modules,
                module_registry: &engine.module_registry,
            }),
        };
        crate::evaluator::eval(&mut ctx, runtime_expr).map_err(ActionError::from)
    }
}

/// A compiled condition evaluated when a partial match reaches its predicate node.
#[derive(Clone, Debug)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub(crate) enum CompiledTestCondition {
    Expr(crate::evaluator::RuntimeExpr),
}

/// Runtime hint for trailing ordered multi-variable captures (`$?var`).
///
/// The rete compiler currently approximates ordered multi-variable constraints
/// as single-slot bindings. This hint allows action-time evaluation to restore
/// CLIPS-style trailing multifield capture semantics for RHS expressions.
#[derive(Clone, Debug)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub(crate) struct MultifieldTailBindingHint {
    pub name: String,
    pub fact_index: usize,
    pub start_slot: usize,
}

/// Compiled rule metadata stored for action execution.
#[derive(Clone, Debug)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
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
    /// Maps fact-address variable names to their index in the collected facts list.
    /// e.g., "f" (for ?f <- pattern) → 0 means `collected_facts[0]` is the fact.
    pub fact_address_vars: HashMap<String, usize>,
    /// Rule salience (stored for informational purposes).
    #[allow(dead_code)] // May be used in future for debugging/logging
    pub salience: Salience,
    /// Pre-translated match conditions referenced by predicate-node indexes.
    pub test_conditions: Vec<CompiledTestCondition>,
    /// Pre-translated RHS action call expressions.
    pub runtime_actions: Vec<Option<crate::evaluator::RuntimeExpr>>,
    /// Trailing ordered multifield capture hints for action-time evaluation.
    pub multifield_tail_bindings: Vec<MultifieldTailBindingHint>,
}

/// Errors that can occur during action execution.
#[derive(Clone, Debug, thiserror::Error)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
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
    /// Internal non-error signal used to unwind the current rule RHS.
    #[doc(hidden)]
    #[error("internal rule return control escaped the action sequence")]
    RuleReturn,
}

/// Execute actions for a fired rule.
///
/// This is called with all the data needed pre-extracted to avoid borrow issues.
///
/// Returns `(fired, reset_requested, clear_requested, errors)` where:
/// - `fired` is `true` once the already-matched activation reaches execution.
/// - `reset_requested` is `true` if a `(reset)` action was executed.
/// - `clear_requested` is `true` if a `(clear)` action was executed.
/// - `errors` is a list of non-fatal action errors that occurred during execution.
#[allow(clippy::too_many_lines)] // Sequential action/test evaluation flow with explicit error branches.
pub(crate) fn execute_actions(
    token: &Token,
    rule_info: &CompiledRuleInfo,
    context: &mut ActionExecutionContext<'_>,
    collected_facts: &[FactId],
) -> (bool, bool, bool, Vec<ActionError>) {
    context.engine.config.begin_action_loop_budget();
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
    let mut eval_env = ActionEvalEnv::default();
    // Defensive: clear any stale deferred events that might have accumulated
    // in non-action evaluation contexts.
    let _ = context.engine.globals.take_printout_events();
    if let Err(error) = seed_fact_address_bindings(
        collected_facts,
        &rule_info.fact_address_vars,
        &mut eval_env.fact_bindings,
        &mut eval_env.compact_facts,
    ) {
        context.engine.config.end_action_loop_budget();
        return (true, false, false, vec![error]);
    }
    seed_multifield_tail_bindings(
        &context.engine.fact_base,
        collected_facts,
        &rule_info.multifield_tail_bindings,
        &mut eval_env.runtime_bindings,
    );

    for (index, action) in rule_info.actions.iter().enumerate() {
        let runtime_call = rule_info
            .runtime_actions
            .get(index)
            .and_then(Option::as_ref);
        let action_result = execute_single_action(
            &mut reset_requested,
            &mut clear_requested,
            token,
            rule_info,
            &action.call,
            runtime_call,
            context,
            &mut eval_env,
            collected_facts,
        );
        match action_result {
            Ok(()) => {}
            Err(ActionError::RuleReturn) => {
                ferric_event!(
                    debug,
                    rule = %rule_info.name,
                    action_index = index,
                    "rule_action_return"
                );
                flush_deferred_printout(context);
                break;
            }
            Err(e) => {
                ferric_event!(
                    warn,
                    rule = %rule_info.name,
                    action_index = index,
                    action_name = %action.call.name,
                    error = %e,
                    "rule_action_error"
                );
                errors.push(e);
                flush_deferred_printout(context);
                break;
            }
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
    context.engine.config.end_action_loop_budget();
    (true, reset_requested, clear_requested, errors)
}

/// Evaluate one rule-local predicate for an incoming partial match.
pub(crate) fn evaluate_test_condition(
    token: &Token,
    rule_info: &CompiledRuleInfo,
    test_condition: &CompiledTestCondition,
    collected_facts: &[FactId],
    context: &mut ActionExecutionContext<'_>,
) -> Result<bool, ActionError> {
    let mut eval_env = ActionEvalEnv::default();
    seed_multifield_tail_bindings(
        &context.engine.fact_base,
        collected_facts,
        &rule_info.multifield_tail_bindings,
        &mut eval_env.runtime_bindings,
    );

    let CompiledTestCondition::Expr(test_expr) = test_condition;
    let result = eval_env
        .eval_runtime_expr(token, rule_info, test_expr, context)
        .map(|value| crate::evaluator::is_truthy(&value, &context.engine.symbol_table));
    flush_deferred_printout(context);
    result
}

fn seed_fact_address_bindings(
    collected_facts: &[FactId],
    addresses: &HashMap<String, usize>,
    env: &mut RuntimeBindingEnv,
    compact_facts: &mut crate::evaluator::CompactFactBindings,
) -> Result<(), ActionError> {
    if addresses.len() > ferric_rules_core::compiler::MAX_RULE_CONDITIONS {
        return Err(ActionError::EvalError(
            "too many fact-address bindings for one rule".to_string(),
        ));
    }
    for (name, index) in addresses {
        let fact_id = collected_facts.get(*index).ok_or_else(|| {
            ActionError::EvalError(format!(
                "fact-address binding ?{name} references missing fact at position {index}"
            ))
        })?;
        let encoded = i64::from_ne_bytes(fact_id.data().as_ffi().to_ne_bytes());
        insert_runtime_binding(env, name, Value::Integer(encoded));
        compact_facts.insert(
            name.strip_prefix("$?").unwrap_or(name).to_string(),
            CompactFactBinding::live(*fact_id),
        );
    }
    Ok(())
}

fn insert_runtime_binding(env: &mut RuntimeBindingEnv, name: &str, value: Value) {
    // Both spellings share one current value, including after an RHS bind.
    env.insert(name.strip_prefix("$?").unwrap_or(name).to_string(), value);
}

fn seed_multifield_tail_bindings(
    fact_base: &FactBase,
    collected_facts: &[FactId],
    hints: &[MultifieldTailBindingHint],
    env: &mut RuntimeBindingEnv,
) {
    for hint in hints {
        let Some(&fact_id) = collected_facts.get(hint.fact_index) else {
            continue;
        };
        let Some(entry) = fact_base.get(fact_id) else {
            continue;
        };
        let Fact::Ordered(ordered) = &entry.fact else {
            continue;
        };

        let mut captured = ferric_rules_core::Multifield::new();
        captured.extend(ordered.fields.iter().skip(hint.start_slot).cloned());
        insert_runtime_binding(env, &hint.name, Value::Multifield(Box::new(captured)));
    }
}

fn build_runtime_eval_bindings(
    token: &Token,
    rule_info: &CompiledRuleInfo,
    fact_bindings: &RuntimeBindingEnv,
    runtime_bindings: &RuntimeBindingEnv,
    context: &mut ActionExecutionContext<'_>,
) -> Result<(BindingSet, VarMap), ActionError> {
    // ValueRef clones share strings/multifields. Adding an address must not
    // deep-copy all ordinary values for every expression in the RHS.
    let mut var_map = rule_info.var_map.clone();
    let mut bindings = token.bindings.clone();

    for (env, overwrite) in [(fact_bindings, false), (runtime_bindings, true)] {
        for (name, value) in env {
            let symbol = context
                .engine
                .symbol_table
                .intern_symbol(name, context.engine.config.string_encoding)
                .map_err(ActionError::Encoding)?;
            let var_id = var_map
                .get_or_create(symbol)
                .map_err(|e| ActionError::EvalError(e.to_string()))?;
            if overwrite || bindings.get(var_id).is_none() {
                bindings.set(var_id, ValueRef::new(value.clone()));
            }
        }
    }

    Ok((bindings, var_map))
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
    collected_facts: &[FactId],
) -> Result<(), ActionError> {
    match call.name.as_str() {
        "assert" => execute_assert(
            token,
            rule_info,
            &call.args,
            context,
            eval_env,
            collected_facts,
        ),
        "retract" => execute_retract(
            token,
            rule_info,
            &call.args,
            context,
            eval_env,
            collected_facts,
        ),
        "modify" => execute_modify(
            token,
            rule_info,
            &call.args,
            context,
            eval_env,
            collected_facts,
        ),
        "duplicate" => execute_duplicate(
            token,
            rule_info,
            &call.args,
            context,
            eval_env,
            collected_facts,
        ),
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
        "printout" => execute_printout(
            token,
            rule_info,
            &call.args,
            context,
            eval_env,
            collected_facts,
        ),
        "println" => execute_println(
            token,
            rule_info,
            &call.args,
            context,
            eval_env,
            collected_facts,
        ),
        "focus" => execute_focus(
            token,
            rule_info,
            &call.args,
            context,
            eval_env,
            collected_facts,
        ),
        "list-focus-stack" => {
            execute_list_focus_stack(&mut context.engine.router, &context.engine.module_registry)
        }
        "agenda" => execute_agenda(
            &context.engine.rete,
            &mut context.engine.router,
            &context.engine.rule_info,
        ),
        "rules" => execute_rules(
            token,
            rule_info,
            &call.args,
            context,
            eval_env,
            collected_facts,
        ),
        "undefrule" => execute_undefrule(
            token,
            rule_info,
            &call.args,
            context,
            eval_env,
            collected_facts,
        ),
        "undeffacts" => execute_undeffacts(
            token,
            rule_info,
            &call.args,
            context,
            eval_env,
            collected_facts,
        ),
        "ppdefrule" => execute_ppdefrule(
            token,
            rule_info,
            &call.args,
            context,
            eval_env,
            collected_facts,
        ),
        "load" => execute_load(
            token,
            rule_info,
            &call.args,
            context,
            eval_env,
            collected_facts,
        ),
        "load-facts" => execute_load_facts(
            token,
            rule_info,
            &call.args,
            context,
            eval_env,
            collected_facts,
        ),
        "save-facts" => execute_save_facts(
            token,
            rule_info,
            &call.args,
            context,
            eval_env,
            collected_facts,
        ),
        "run" => {
            // (run) from within a rule RHS is a no-op — the engine is already running.
            // CLIPS allows this but it's unusual. We silently ignore it.
            Ok(())
        }
        "return" => match call.args.as_slice() {
            [] => Err(ActionError::RuleReturn),
            [value_expr] => {
                let _ =
                    eval_env.eval_expr(token, rule_info, value_expr, context, collected_facts)?;
                Err(ActionError::RuleReturn)
            }
            _ => Err(ActionError::Evaluator(
                crate::evaluator::EvalError::ArityMismatch {
                    name: "return".to_string(),
                    expected: "0 or 1".to_string(),
                    actual: call.args.len(),
                    span: Some(crate::evaluator::SourceSpan {
                        line: call.span.start.line,
                        column: call.span.start.column,
                    }),
                },
            )),
        },
        "bind" => {
            if let [ActionExpr::Variable(name, _), value_expr] = call.args.as_slice() {
                let value =
                    eval_env.eval_expr(token, rule_info, value_expr, context, collected_facts)?;
                insert_runtime_binding(&mut eval_env.runtime_bindings, name, value);
                Ok(())
            } else {
                // Global bind (or malformed local bind) stays on evaluator semantics.
                let eval_result = if let Some(runtime_expr) = runtime_call {
                    eval_env.eval_runtime_expr(token, rule_info, runtime_expr, context)
                } else {
                    let action_expr = ActionExpr::FunctionCall(call.clone());
                    eval_env.eval_expr(token, rule_info, &action_expr, context, collected_facts)
                };
                eval_result.map(|_| ())
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
                let cond_value =
                    eval_env.eval_runtime_expr(token, rule_info, condition, context)?;
                let branch =
                    if crate::evaluator::is_truthy(&cond_value, &context.engine.symbol_table) {
                        then_branch
                    } else {
                        else_branch
                    };
                execute_loop_body(
                    reset_requested,
                    clear_requested,
                    token,
                    rule_info,
                    branch,
                    context,
                    eval_env,
                    collected_facts,
                )
            } else {
                // Fallback: evaluate as expression (no-op if void).
                if let Some(runtime_expr) = runtime_call {
                    eval_env
                        .eval_runtime_expr(token, rule_info, runtime_expr, context)
                        .map(|_| ())
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
                condition,
                body,
                span,
            }) = while_runtime
            {
                loop {
                    // Evaluate condition.
                    let cond_value =
                        eval_env.eval_runtime_expr(token, rule_info, condition, context)?;
                    if !crate::evaluator::is_truthy(&cond_value, &context.engine.symbol_table) {
                        break;
                    }
                    crate::evaluator::consume_action_loop_iteration(
                        &context.engine.config,
                        "while",
                        span.clone(),
                    )
                    .map_err(ActionError::from)?;
                    // Execute body items.
                    execute_loop_body(
                        reset_requested,
                        clear_requested,
                        token,
                        rule_info,
                        body,
                        context,
                        eval_env,
                        collected_facts,
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
                    let sv = eval_env.eval_runtime_expr(token, rule_info, start, context)?;
                    let ev = eval_env.eval_runtime_expr(token, rule_info, end, context)?;
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
                    crate::evaluator::consume_action_loop_iteration(
                        &context.engine.config,
                        "loop-for-count",
                        span.clone(),
                    )
                    .map_err(ActionError::from)?;
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

                    eval_env.with_local_scope(var_name.as_deref(), |eval_env| {
                        execute_loop_body(
                            reset_requested,
                            clear_requested,
                            &loop_token,
                            &loop_rule_info,
                            body,
                            context,
                            eval_env,
                            collected_facts,
                        )
                    })?;
                    if context.engine.is_halted() || *reset_requested || *clear_requested {
                        break;
                    }
                }
                Ok(())
            } else if let Some(runtime_expr) = runtime_call {
                eval_env
                    .eval_runtime_expr(token, rule_info, runtime_expr, context)
                    .map(|_| ())
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
                let disc_value = eval_env.eval_runtime_expr(token, rule_info, expr, context)?;

                // Find first matching case.
                let mut matched_body = None;
                for (test_val_expr, case_body) in cases {
                    let test_value =
                        eval_env.eval_runtime_expr(token, rule_info, test_val_expr, context)?;
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
                        collected_facts,
                    )?;
                }
                Ok(())
            } else if let Some(runtime_expr) = runtime_call {
                eval_env
                    .eval_runtime_expr(token, rule_info, runtime_expr, context)
                    .map(|_| ())
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
                    let list_val =
                        eval_env.eval_runtime_expr(token, rule_info, list_expr, context)?;
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

                    eval_env.with_local_scope(
                        [var_name.as_str(), index_var_name.as_str()],
                        |eval_env| {
                            execute_loop_body(
                                reset_requested,
                                clear_requested,
                                &loop_token,
                                &loop_rule_info,
                                body,
                                context,
                                eval_env,
                                collected_facts,
                            )
                        },
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

            // Result forms retain their evaluation and early-stop semantics
            // even when the enclosing action discards the returned value.
            if let Some(rt @ crate::evaluator::RuntimeExpr::QueryAction { name, .. }) =
                query_runtime
            {
                if matches!(name.as_str(), "any-factp" | "find-fact" | "find-all-facts") {
                    return eval_env
                        .eval_runtime_expr(token, rule_info, rt, context)
                        .map(|_| ());
                }
            }

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
                    collected_facts,
                )
            } else if let Some(runtime_expr) = runtime_call {
                eval_env
                    .eval_runtime_expr(token, rule_info, runtime_expr, context)
                    .map(|_| ())
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
                eval_env.eval_expr(token, rule_info, &action_expr, context, collected_facts)
            };
            eval_result.map(|_| ())
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
        ferric_rules_parser::ActionExpr,
        Option<Box<crate::evaluator::RuntimeExpr>>,
    )],
    context: &mut ActionExecutionContext<'_>,
    eval_env: &mut ActionEvalEnv,
    collected_facts: &[FactId],
) -> Result<(), ActionError> {
    for (action_expr, rt_expr) in body {
        let (branch_call, branch_runtime): (
            ferric_rules_parser::FunctionCall,
            Option<&crate::evaluator::RuntimeExpr>,
        ) = match action_expr {
            ActionExpr::FunctionCall(fc) => {
                let rt: Option<&crate::evaluator::RuntimeExpr> = rt_expr.as_deref();
                (fc.clone(), rt)
            }
            ActionExpr::If { span, .. } => {
                let synthetic = ferric_rules_parser::FunctionCall {
                    name: "if".to_string(),
                    args: vec![],
                    span: *span,
                };
                let rt: Option<&crate::evaluator::RuntimeExpr> = rt_expr.as_deref();
                (synthetic, rt)
            }
            ActionExpr::While { span, .. } => {
                let synthetic = ferric_rules_parser::FunctionCall {
                    name: "while".to_string(),
                    args: vec![],
                    span: *span,
                };
                let rt: Option<&crate::evaluator::RuntimeExpr> = rt_expr.as_deref();
                (synthetic, rt)
            }
            ActionExpr::LoopForCount { span, .. } => {
                let synthetic = ferric_rules_parser::FunctionCall {
                    name: "loop-for-count".to_string(),
                    args: vec![],
                    span: *span,
                };
                let rt: Option<&crate::evaluator::RuntimeExpr> = rt_expr.as_deref();
                (synthetic, rt)
            }
            ActionExpr::Progn { span, .. } => {
                let synthetic = ferric_rules_parser::FunctionCall {
                    name: "progn$".to_string(),
                    args: vec![],
                    span: *span,
                };
                let rt: Option<&crate::evaluator::RuntimeExpr> = rt_expr.as_deref();
                (synthetic, rt)
            }
            ActionExpr::QueryAction { name, span, .. } => {
                let synthetic = ferric_rules_parser::FunctionCall {
                    name: name.clone(),
                    args: vec![],
                    span: *span,
                };
                let rt: Option<&crate::evaluator::RuntimeExpr> = rt_expr.as_deref();
                (synthetic, rt)
            }
            ActionExpr::Switch { span, .. } => {
                let synthetic = ferric_rules_parser::FunctionCall {
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
                    eval_env.eval_runtime_expr(token, rule_info, rt, context)?;
                } else {
                    eval_env.eval_expr(token, rule_info, action_expr, context, collected_facts)?;
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
            collected_facts,
        )?;
        if context.engine.is_halted() || *reset_requested || *clear_requested {
            break;
        }
    }
    Ok(())
}

type QueryCandidate = Vec<(String, CompactFactBinding)>;

/// A live, iterative nested-loop cursor. Each level remembers chronology,
/// rather than an arena slot, so removal/reuse cannot revive an old member.
/// Outer members remain selected while their inner levels advance.
struct ActionQueryCursor {
    members: Vec<(String, TemplateId)>,
    after: Vec<Option<u64>>,
    current: Vec<Option<CompactFactBinding>>,
    retained: HashMap<FactId, Arc<Fact>>,
    level: usize,
    finished: bool,
}

impl ActionQueryCursor {
    fn new(
        bindings: &[(String, String)],
        context: &mut ActionExecutionContext<'_>,
    ) -> Result<Self, ActionError> {
        if bindings.is_empty() {
            return Err(ActionError::EvalError(
                "query requires at least one member".into(),
            ));
        }
        let resolver = crate::loader::TemplateResolver {
            template_local_ids: &context.engine.template_local_ids,
            template_modules: &context.engine.template_modules,
            module_registry: &context.engine.module_registry,
        };
        let mut names = HashSet::new();
        let mut members = Vec::with_capacity(bindings.len());
        for (name, template) in bindings {
            if !crate::evaluator::valid_query_member(name) || !names.insert(name) {
                return Err(ActionError::EvalError(
                    "invalid or duplicate query member".into(),
                ));
            }
            let template = resolver
                .resolve_query_reference(template, context.current_module)
                .map_err(ActionError::EvalError)?;
            members.push((name.clone(), template));
        }
        // Resolve every declaration before recognizing an empty product.
        let finished = members.iter().any(|(_, template)| {
            context
                .engine
                .fact_base
                .next_template_fact_after(*template, None)
                .is_none()
        });
        Ok(Self {
            after: vec![None; members.len()],
            current: vec![None; members.len()],
            members,
            retained: HashMap::new(),
            level: 0,
            finished,
        })
    }

    fn next(
        &mut self,
        context: &mut ActionExecutionContext<'_>,
        query_name: &str,
    ) -> Result<Option<QueryCandidate>, ActionError> {
        while !self.finished {
            let next = context
                .engine
                .fact_base
                .next_template_fact_after(self.members[self.level].1, self.after[self.level]);
            // Charge traversal as well as predicates: mutation can empty an
            // inner set while many outer prefixes remain to be visited.
            if let Some((fact_id, timestamp)) = next {
                crate::evaluator::consume_action_loop_iteration(
                    &context.engine.config,
                    query_name,
                    None,
                )
                .map_err(ActionError::from)?;
                self.after[self.level] = Some(timestamp);
                let retained = self.retained.entry(fact_id).or_insert_with(|| {
                    Arc::new(
                        context
                            .engine
                            .fact_base
                            .get(fact_id)
                            .expect("chronological index only yields live facts")
                            .fact
                            .clone(),
                    )
                });
                self.current[self.level] =
                    Some(CompactFactBinding::retained(fact_id, retained.clone()));
                if self.level + 1 == self.members.len() {
                    let candidate = self
                        .members
                        .iter()
                        .zip(&self.current)
                        .map(|((name, _), member)| {
                            (
                                name.clone(),
                                member.as_ref().expect("complete query tuple").clone(),
                            )
                        })
                        .collect();
                    return Ok(Some(candidate));
                }
                self.level += 1;
                self.after[self.level] = None;
            } else if self.level == 0 {
                self.finished = true;
            } else {
                self.current[self.level] = None;
                self.after[self.level] = None;
                self.level -= 1;
            }
        }
        Ok(None)
    }
}

/// Install both ordinary and compact member scopes once for a predicate/body.
/// Retained records follow compact members; ordinary aliases remain addresses.
fn with_query_candidate<T>(
    candidate: &QueryCandidate,
    token: &Token,
    rule_info: &CompiledRuleInfo,
    context: &mut ActionExecutionContext<'_>,
    eval_env: &mut ActionEvalEnv,
    execute: impl FnOnce(
        &Token,
        &CompiledRuleInfo,
        &mut ActionExecutionContext<'_>,
        &mut ActionEvalEnv,
    ) -> Result<T, ActionError>,
) -> Result<T, ActionError> {
    let mut aug_token = token.clone();
    let mut aug_rule_info = rule_info_clone_light(rule_info);
    for (name, member) in candidate {
        let value = Value::Integer(i64::from_ne_bytes(
            member.fact_id().data().as_ffi().to_ne_bytes(),
        ));
        let (next_token, next_rule) = augment_bindings_with_var(
            &aug_token,
            &aug_rule_info,
            name,
            value,
            &mut context.engine.symbol_table,
            &context.engine.config,
        )?;
        aug_token = next_token;
        aug_rule_info = next_rule;
    }
    eval_env.with_local_scope(
        candidate.iter().map(|(name, _)| name.as_str()),
        |eval_env| {
            eval_env.with_compact_scope(candidate, |eval_env| {
                execute(&aug_token, &aug_rule_info, context, eval_env)
            })
        },
    )
}

/// Delayed queries select every matching tuple before their first body.
/// Immediate queries resume their chronological cursor after each body.
#[allow(clippy::too_many_arguments)]
fn execute_query_action(
    reset_requested: &mut bool,
    clear_requested: &mut bool,
    token: &Token,
    rule_info: &CompiledRuleInfo,
    name: &str,
    bindings: &[(String, String)],
    query: &crate::evaluator::RuntimeExpr,
    body: &[(ActionExpr, Option<Box<crate::evaluator::RuntimeExpr>>)],
    context: &mut ActionExecutionContext<'_>,
    eval_env: &mut ActionEvalEnv,
    collected_facts: &[FactId],
) -> Result<(), ActionError> {
    let delayed = name == "delayed-do-for-all-facts";
    let stop_after_first = name == "do-for-fact";
    if !delayed && !stop_after_first && name != "do-for-all-facts" {
        return Err(ActionError::UnknownAction(name.into()));
    }
    crate::evaluator::validate_query_predicate(
        &mut ActionEvalEnv::make_eval_context(token, rule_info, context, &eval_env.compact_facts),
        query,
        name,
        None,
        0,
    )
    .map_err(ActionError::from)?;
    let mut cursor = ActionQueryCursor::new(bindings, context)?;
    let mut selected = Vec::new();
    while let Some(candidate) = cursor.next(context, name)? {
        let matched = with_query_candidate(
            &candidate,
            token,
            rule_info,
            context,
            eval_env,
            |token, rule_info, context, eval_env| {
                let value = eval_env.eval_runtime_expr(token, rule_info, query, context)?;
                let matched = crate::evaluator::is_truthy(&value, &context.engine.symbol_table);
                if matched && !delayed {
                    execute_loop_body(
                        reset_requested,
                        clear_requested,
                        token,
                        rule_info,
                        body,
                        context,
                        eval_env,
                        collected_facts,
                    )?;
                }
                Ok(matched)
            },
        )?;
        if matched && delayed {
            selected.push(candidate);
        }
        if context.engine.is_halted()
            || *reset_requested
            || *clear_requested
            || (matched && stop_after_first)
        {
            return Ok(());
        }
    }
    for candidate in selected {
        crate::evaluator::consume_action_loop_iteration(&context.engine.config, name, None)
            .map_err(ActionError::from)?;
        with_query_candidate(
            &candidate,
            token,
            rule_info,
            context,
            eval_env,
            |token, rule_info, context, eval_env| {
                execute_loop_body(
                    reset_requested,
                    clear_requested,
                    token,
                    rule_info,
                    body,
                    context,
                    eval_env,
                    collected_facts,
                )
            },
        )?;
        if context.engine.is_halted() || *reset_requested || *clear_requested {
            break;
        }
    }
    Ok(())
}

/// Execute a `focus` action: push module(s) onto the focus stack.
///
/// Resolve all arguments, then push in reverse order so the first argument
/// becomes the top of the stack before the next RHS action executes.
#[allow(clippy::too_many_arguments)]
fn execute_focus(
    token: &Token,
    rule_info: &CompiledRuleInfo,
    args: &[ActionExpr],
    context: &mut ActionExecutionContext<'_>,
    eval_env: &mut ActionEvalEnv,
    collected_facts: &[FactId],
) -> Result<(), ActionError> {
    let mut modules = Vec::with_capacity(args.len());
    for arg in args {
        let value = eval_env.eval_expr(token, rule_info, arg, context, collected_facts)?;
        match value {
            Value::Symbol(sym) => {
                let name = context
                    .engine
                    .symbol_table
                    .resolve_symbol_str(sym)
                    .ok_or_else(|| {
                        ActionError::EvalError("focus: invalid symbol argument".to_string())
                    })?;
                let module = context
                    .engine
                    .module_registry
                    .get_by_name(name)
                    .ok_or_else(|| {
                        ActionError::EvalError(format!("focus: unknown module `{name}`"))
                    })?;
                modules.push(module);
            }
            _ => {
                return Err(ActionError::EvalError(
                    "focus: expected symbol argument".to_string(),
                ));
            }
        }
    }
    for module in modules.into_iter().rev() {
        context.engine.module_registry.push_focus(module);
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
    all_rule_info: &crate::engine::RuleIndex<Arc<CompiledRuleInfo>>,
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
    collected_facts: &[FactId],
) -> Result<(), ActionError> {
    if args.len() > 1 {
        return Err(ActionError::EvalError(format!(
            "rules: expected 0 or 1 arguments, got {}",
            args.len()
        )));
    }

    if let Some(module_arg) = args.first() {
        let _ = eval_env.eval_expr(token, rule_info, module_arg, context, collected_facts)?;
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
    collected_facts: &[FactId],
) -> Result<(), ActionError> {
    if args.is_empty() {
        return Err(ActionError::EvalError(
            "undefrule: expected at least 1 argument".to_string(),
        ));
    }

    let selectors = evaluated_rule_selectors(
        "undefrule",
        token,
        rule_info,
        args,
        context,
        eval_env,
        collected_facts,
    )?;
    let selected = selected_rule_ids(
        &selectors,
        &context.engine.rule_info,
        &context.engine.rule_modules,
        &context.engine.module_registry,
        "undefrule",
    )?;

    context
        .engine
        .remove_compiled_rules(&selected.into_iter().collect::<Vec<_>>());

    Ok(())
}

fn execute_undeffacts(
    token: &Token,
    rule_info: &CompiledRuleInfo,
    args: &[ActionExpr],
    context: &mut ActionExecutionContext<'_>,
    eval_env: &mut ActionEvalEnv,
    collected_facts: &[FactId],
) -> Result<(), ActionError> {
    if args.len() != 1 {
        return Err(ActionError::EvalError(
            "undeffacts: expected one name or *".to_string(),
        ));
    }
    let selectors = evaluated_rule_selectors(
        "undeffacts",
        token,
        rule_info,
        args,
        context,
        eval_env,
        collected_facts,
    )?;
    if selectors[0] == "*" {
        context
            .engine
            .registered_deffacts
            .retain(|definition| definition.module != context.current_module);
        return Ok(());
    }
    let name = parse_qualified_name(&selectors[0])
        .map_err(|error| ActionError::EvalError(format!("undeffacts: {error}")))?;
    let module = match name.module_name() {
        Some(module) => context
            .engine
            .module_registry
            .get_by_name(module)
            .ok_or_else(|| {
                ActionError::EvalError(format!("undeffacts: unknown module `{module}`"))
            })?,
        None => context.current_module,
    };
    if module == context.engine.module_registry.main_module_id()
        && name.local_name() == "initial-fact"
    {
        return Err(ActionError::EvalError(
            "the built-in initial-fact definition is protected".to_string(),
        ));
    }
    let before = context.engine.registered_deffacts.len();
    context
        .engine
        .registered_deffacts
        .retain(|definition| definition.module != module || definition.name != name.local_name());
    if before == context.engine.registered_deffacts.len() {
        return Err(ActionError::EvalError(format!(
            "undeffacts: unknown definition `{}`",
            selectors[0]
        )));
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
    collected_facts: &[FactId],
) -> Result<(), ActionError> {
    if args.len() != 1 {
        return Err(ActionError::EvalError(format!(
            "ppdefrule: expected exactly 1 argument, got {}",
            args.len()
        )));
    }

    let selectors = evaluated_rule_selectors(
        "ppdefrule",
        token,
        rule_info,
        args,
        context,
        eval_env,
        collected_facts,
    )?;
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
    collected_facts: &[FactId],
) -> Result<(), ActionError> {
    if args.len() != 1 {
        return Err(ActionError::EvalError(format!(
            "load: expected exactly 1 argument, got {}",
            args.len()
        )));
    }

    let selector = eval_env.eval_expr(token, rule_info, &args[0], context, collected_facts)?;
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

/// Format a `Value` as it should appear inside a `.fct` file (CLIPS s-expression syntax).
///
/// Strings are quoted; symbols, integers, floats, and multifields render as CLIPS expects.
fn format_value_for_fct(value: &Value, symbol_table: &SymbolTable, output: &mut String) {
    match value {
        Value::Integer(n) => output.push_str(&n.to_string()),
        Value::Float(f) => {
            if f.fract() == 0.0 {
                let _ = write!(output, "{f:.1}");
            } else {
                output.push_str(&f.to_string());
            }
        }
        Value::Symbol(sym) => {
            if let Some(name) = symbol_table.resolve_symbol_str(*sym) {
                output.push_str(name);
            }
        }
        Value::String(s) => {
            output.push('"');
            for ch in s.as_str().chars() {
                match ch {
                    '\\' => output.push_str("\\\\"),
                    '"' => output.push_str("\\\""),
                    _ => output.push(ch),
                }
            }
            output.push('"');
        }
        Value::Multifield(mf) => {
            for (i, v) in mf.as_slice().iter().enumerate() {
                if i > 0 {
                    output.push(' ');
                }
                format_value_for_fct(v, symbol_table, output);
            }
        }
        Value::ExternalAddress(_) | Value::Void => {}
    }
}

/// Format a single `Fact` as a bare CLIPS s-expression suitable for a `.fct` file.
///
/// Ordered facts render as `(relation field1 field2 ...)`.
/// Template facts render as `(template-name (slot1 val1) (slot2 val2) ...)`.
fn format_fact_for_fct(
    fact: &Fact,
    symbol_table: &SymbolTable,
    template_defs: &slotmap::SlotMap<TemplateId, Arc<crate::templates::RegisteredTemplate>>,
) -> String {
    let mut out = String::new();
    match fact {
        Fact::Ordered(o) => {
            out.push('(');
            if let Some(rel) = symbol_table.resolve_symbol_str(o.relation) {
                out.push_str(rel);
            }
            for field in &o.fields {
                out.push(' ');
                format_value_for_fct(field, symbol_table, &mut out);
            }
            out.push(')');
        }
        Fact::Template(t) => {
            out.push('(');
            if let Some(reg) = template_defs.get(t.template_id) {
                out.push_str(&reg.name);
                // Use slot_names (declaration order) for deterministic output.
                for (slot_idx, slot_name) in reg.slot_names.iter().enumerate() {
                    if let Some(val) = t.slots.get(slot_idx) {
                        out.push(' ');
                        out.push('(');
                        out.push_str(slot_name);
                        out.push(' ');
                        format_value_for_fct(val, symbol_table, &mut out);
                        out.push(')');
                    }
                }
            }
            out.push(')');
        }
    }
    out
}

/// Evaluate the first argument of `load-facts` or `save-facts` as a filename string.
fn eval_filename_arg(
    token: &Token,
    rule_info: &CompiledRuleInfo,
    args: &[ActionExpr],
    command_name: &str,
    context: &mut ActionExecutionContext<'_>,
    eval_env: &mut ActionEvalEnv,
    collected_facts: &[FactId],
) -> Result<String, ActionError> {
    if args.len() != 1 {
        return Err(ActionError::EvalError(format!(
            "{command_name}: expected exactly 1 argument, got {}",
            args.len()
        )));
    }
    let val = eval_env.eval_expr(token, rule_info, &args[0], context, collected_facts)?;
    match val {
        Value::String(s) => Ok(s.as_str().to_string()),
        Value::Symbol(sym) => Ok(context
            .engine
            .symbol_table
            .resolve_symbol_str(sym)
            .unwrap_or("???")
            .to_string()),
        other => Err(ActionError::EvalError(format!(
            "{command_name}: expected STRING or SYMBOL filename, got {}",
            runtime_value_type_name(&other)
        ))),
    }
}

/// `(save-facts <filename>)` — write all current facts to a file in `.fct` format.
///
/// Each fact is written as a bare s-expression on its own line, readable by
/// `load-facts`. Returns TRUE on success, FALSE on I/O failure.
fn execute_save_facts(
    token: &Token,
    rule_info: &CompiledRuleInfo,
    args: &[ActionExpr],
    context: &mut ActionExecutionContext<'_>,
    eval_env: &mut ActionEvalEnv,
    collected_facts: &[FactId],
) -> Result<(), ActionError> {
    let filename = eval_filename_arg(
        token,
        rule_info,
        args,
        "save-facts",
        context,
        eval_env,
        collected_facts,
    )?;

    let result = do_save_facts(&filename, context);
    let return_val = match result {
        Ok(_count) => crate::evaluator::clips_true(
            &mut context.engine.symbol_table,
            context.engine.config.string_encoding,
        ),
        Err(_) => crate::evaluator::clips_false(
            &mut context.engine.symbol_table,
            context.engine.config.string_encoding,
        ),
    };
    // Write return value to global ?*result* if it exists; otherwise discard.
    // (CLIPS itself returns the value but RHS actions discard scalar returns —
    // the caller can capture it via (bind ?result (save-facts "file")).)
    let _ = return_val;
    Ok(())
}

/// Inner I/O body for save-facts, separated to avoid borrow-conflict with `symbol_table`.
fn do_save_facts(
    filename: &str,
    context: &mut ActionExecutionContext<'_>,
) -> Result<usize, std::io::Error> {
    let file = std::fs::File::create(filename)?;
    let mut writer = std::io::BufWriter::new(file);
    let mut count = 0usize;

    // Collect (id, cloned fact) pairs to avoid holding a borrow on fact_base
    // while also borrowing symbol_table and template_defs.
    let exclude_id = context.engine.initial_fact_id;
    let facts: Vec<(FactId, Fact)> = context
        .engine
        .fact_base
        .iter()
        .filter(|(id, _)| Some(*id) != exclude_id)
        .map(|(id, entry)| (id, entry.fact.clone()))
        .collect();

    for (_fact_id, fact) in facts {
        let line = format_fact_for_fct(
            &fact,
            &context.engine.symbol_table,
            &context.engine.template_defs,
        );
        writeln!(writer, "{line}")?;
        count += 1;
    }

    Ok(count)
}

/// `(load-facts <filename>)` — read a `.fct` file and assert each fact.
///
/// The file must contain bare fact s-expressions (not wrapped in `assert`),
/// one per top-level form.  Facts are asserted directly into working memory
/// and are **not** registered for re-assertion on reset.
/// Returns TRUE on success, FALSE on failure.
fn execute_load_facts(
    token: &Token,
    rule_info: &CompiledRuleInfo,
    args: &[ActionExpr],
    context: &mut ActionExecutionContext<'_>,
    eval_env: &mut ActionEvalEnv,
    collected_facts: &[FactId],
) -> Result<(), ActionError> {
    let filename = eval_filename_arg(
        token,
        rule_info,
        args,
        "load-facts",
        context,
        eval_env,
        collected_facts,
    )?;

    let contents = match crate::source_limits::read_source_file(std::path::Path::new(&filename)) {
        Ok(contents) => contents,
        // Preserve the existing I/O-failure behavior, but report resource limits.
        Err(crate::loader::LoadError::Io(_)) => return Ok(()),
        Err(error) => return Err(ActionError::EvalError(format!("load-facts: {error}"))),
    };

    // Loading facts must not mutate named reset seeds or collide with a source
    // definition. The shared fact builder validates and asserts file facts.
    context
        .engine
        .load_facts_str(&contents)
        .map_err(|error| ActionError::EvalError(format!("load-facts: {error}")))?;
    Ok(())
}

fn evaluated_rule_selectors(
    command_name: &str,
    token: &Token,
    rule_info: &CompiledRuleInfo,
    args: &[ActionExpr],
    context: &mut ActionExecutionContext<'_>,
    eval_env: &mut ActionEvalEnv,
    collected_facts: &[FactId],
) -> Result<Vec<String>, ActionError> {
    let mut selectors = Vec::with_capacity(args.len());
    for arg in args {
        let value = eval_env.eval_expr(token, rule_info, arg, context, collected_facts)?;
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
    all_rule_info: &crate::engine::RuleIndex<Arc<CompiledRuleInfo>>,
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
    collected_facts: &[FactId],
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
        let value = eval_env.eval_expr(token, rule_info, arg, context, collected_facts)?;
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
    collected_facts: &[FactId],
) -> Result<(), ActionError> {
    let mut output = String::new();
    for arg in args {
        let value = eval_env.eval_expr(token, rule_info, arg, context, collected_facts)?;
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
    collected_facts: &[FactId],
) -> Result<(), ActionError> {
    // Each argument to assert should be a "function call" representing a fact pattern
    // e.g., (assert (relation val1 val2)) → args = [FunctionCall("relation", [val1, val2])]
    for arg in args {
        match arg {
            ActionExpr::FunctionCall(fact_pattern) => {
                let relation = &fact_pattern.name;
                if let Ok(template_id) = context
                    .engine
                    .resolve_template_id(relation, context.current_module)
                {
                    let registered = context.engine.template_defs[template_id].clone();
                    let mut slots = registered.defaults.clone();
                    apply_template_slot_overrides(
                        &mut slots,
                        &fact_pattern.args,
                        &registered,
                        token,
                        rule_info,
                        context,
                        eval_env,
                        collected_facts,
                    )?;
                    registered
                        .validate_slots(&slots)
                        .map_err(ActionError::EvalError)?;
                    assert_template_and_propagate(
                        context.engine,
                        template_id,
                        slots.into_boxed_slice(),
                    )?;
                    continue;
                }
                let relation_sym = context
                    .engine
                    .symbol_table
                    .intern_symbol(relation, context.engine.config.string_encoding)
                    .map_err(ActionError::from)?;

                let mut fields = smallvec::SmallVec::new();
                for field_expr in &fact_pattern.args {
                    let value = eval_env.eval_expr(
                        token,
                        rule_info,
                        field_expr,
                        context,
                        collected_facts,
                    )?;
                    match value {
                        // CLIPS splices multifield values into ordered assertions.
                        Value::Multifield(mf) => fields.extend(mf.as_slice().iter().cloned()),
                        other => fields.push(other),
                    }
                }

                assert_ordered_and_propagate(context.engine, relation_sym, fields)?;
            }
            _ => return Err(ActionError::InvalidAssert),
        }
    }
    Ok(())
}

fn execute_retract(
    token: &Token,
    rule_info: &CompiledRuleInfo,
    args: &[ActionExpr],
    context: &mut ActionExecutionContext<'_>,
    eval_env: &mut ActionEvalEnv,
    collected_facts: &[FactId],
) -> Result<(), ActionError> {
    for arg in args {
        let fact_id = resolve_target_fact_id(
            "retract",
            arg,
            token,
            rule_info,
            context,
            eval_env,
            collected_facts,
        )?;
        if Some(fact_id) == context.engine.initial_fact_id {
            return Err(ActionError::EvalError(
                "the internal initial-fact is protected and cannot be retracted".to_string(),
            ));
        }
        // A retained query member may already have been retracted by an
        // earlier body. Repeated retraction of the same address is a no-op.
        if let Some(entry) = context.engine.fact_base.get(fact_id) {
            let fact = entry.fact.clone();
            context
                .engine
                .rete
                .retract_fact(fact_id, &fact, &context.engine.fact_base);
            context.engine.fact_base.retract(fact_id);
            // Each removal is a matching boundary. A later target expression
            // may change globals used by predicates unblocked by this fact.
            context.engine.drain_pending_predicate_matches();
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
    collected_facts: &[FactId],
) -> Result<(), ActionError> {
    execute_fact_mutation(
        token,
        rule_info,
        args,
        FactMutationMode::Modify,
        context,
        eval_env,
        collected_facts,
    )
}

#[allow(clippy::too_many_arguments)] // Context requires all these parameters
fn execute_duplicate(
    token: &Token,
    rule_info: &CompiledRuleInfo,
    args: &[ActionExpr],
    context: &mut ActionExecutionContext<'_>,
    eval_env: &mut ActionEvalEnv,
    collected_facts: &[FactId],
) -> Result<(), ActionError> {
    execute_fact_mutation(
        token,
        rule_info,
        args,
        FactMutationMode::Duplicate,
        context,
        eval_env,
        collected_facts,
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
    collected_facts: &[FactId],
) -> Result<(), ActionError> {
    let target = args.first().ok_or(ActionError::InvalidRetract)?;
    let fact_id = resolve_target_fact_id(
        if mode.retract_original() {
            "modify"
        } else {
            "duplicate"
        },
        target,
        token,
        rule_info,
        context,
        eval_env,
        collected_facts,
    )?;
    if Some(fact_id) == context.engine.initial_fact_id {
        return Err(ActionError::EvalError(
            "the internal initial-fact is protected and cannot be modified or duplicated"
                .to_string(),
        ));
    }
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
                collected_facts,
            )?;
            if mode.retract_original() {
                context
                    .engine
                    .fact_base
                    .ensure_assertion_capacity()
                    .map_err(|error| ActionError::EvalError(error.to_string()))?;
                retract_original_fact(
                    &mut context.engine.fact_base,
                    &mut context.engine.rete,
                    fact_id,
                    &original_fact,
                );
            }
            assert_ordered_and_propagate(context.engine, relation, fields)?;
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
                collected_facts,
            )?;
            registered
                .validate_slots(&slots)
                .map_err(ActionError::EvalError)?;
            if mode.retract_original() {
                context
                    .engine
                    .fact_base
                    .ensure_assertion_capacity()
                    .map_err(|error| ActionError::EvalError(error.to_string()))?;
                retract_original_fact(
                    &mut context.engine.fact_base,
                    &mut context.engine.rete,
                    fact_id,
                    &original_fact,
                );
            }
            assert_template_and_propagate(
                context.engine,
                template.template_id,
                slots.into_boxed_slice(),
            )?;
        }
    }

    Ok(())
}

fn assert_ordered_and_propagate(
    engine: &mut Engine,
    relation: Symbol,
    fields: OrderedFields,
) -> Result<crate::FactAssertionResult<FactId>, ActionError> {
    engine
        .assert_fact_internal(Fact::Ordered(OrderedFact { relation, fields }))
        .map_err(|error| ActionError::EvalError(error.to_string()))
}

fn assert_template_and_propagate(
    engine: &mut Engine,
    template_id: TemplateId,
    slots: Box<[Value]>,
) -> Result<crate::FactAssertionResult<FactId>, ActionError> {
    engine
        .assert_fact_internal(Fact::Template(ferric_rules_core::TemplateFact {
            template_id,
            slots,
        }))
        .map_err(|error| ActionError::EvalError(error.to_string()))
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

/// Fact actions use the current ordinary frame, including query members,
/// aliases, and inner loop shadowing. Compact slot bindings have a separate
/// lexical purpose and must not override these ordinary values.
fn resolve_target_fact_id(
    action: &str,
    target: &ActionExpr,
    token: &Token,
    rule_info: &CompiledRuleInfo,
    context: &mut ActionExecutionContext<'_>,
    eval_env: &mut ActionEvalEnv,
    collected_facts: &[FactId],
) -> Result<FactId, ActionError> {
    let value = eval_env.eval_expr(token, rule_info, target, context, collected_facts)?;
    crate::evaluator::checked_fact_address(&value)
        .ok_or_else(|| ActionError::EvalError(format!("{action}: target must be a fact-address")))
}

#[allow(clippy::too_many_arguments)] // Context requires all these parameters
fn apply_ordered_slot_overrides(
    fields: &mut OrderedFields,
    slot_overrides: &[ActionExpr],
    token: &Token,
    rule_info: &CompiledRuleInfo,
    context: &mut ActionExecutionContext<'_>,
    eval_env: &mut ActionEvalEnv,
    collected_facts: &[FactId],
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
            fields[index] =
                eval_env.eval_expr(token, rule_info, first_arg, context, collected_facts)?;
        }
    }

    Ok(())
}

/// Apply slot overrides to a mutable template slot vector.
///
/// Each override in `slot_overrides` is expected to be a `FunctionCall` whose
/// name is a slot name. Single-field slots require one scalar value; multifield
/// slots evaluate all expressions and splice their multifield results.
#[allow(clippy::too_many_arguments)] // Context requires all these parameters
fn apply_template_slot_overrides(
    slots: &mut [Value],
    slot_overrides: &[ActionExpr],
    registered: &RegisteredTemplate,
    token: &Token,
    rule_info: &CompiledRuleInfo,
    context: &mut ActionExecutionContext<'_>,
    eval_env: &mut ActionEvalEnv,
    collected_facts: &[FactId],
) -> Result<(), ActionError> {
    let overrides = registered
        .slot_overrides(slot_overrides)
        .map_err(ActionError::EvalError)?;
    for (slot_idx, call) in overrides {
        if slot_idx >= slots.len() {
            return Err(ActionError::EvalError(format!(
                "slot index {slot_idx} out of bounds for template `{}`",
                registered.name
            )));
        }

        slots[slot_idx] = match registered.slot_types[slot_idx] {
            ferric_rules_parser::SlotType::Single => {
                let value = eval_env.eval_expr(
                    token,
                    rule_info,
                    &call.args[0],
                    context,
                    collected_facts,
                )?;
                if matches!(value, Value::Multifield(_) | Value::Void) {
                    return Err(ActionError::EvalError(format!(
                        "single-field slot `{}` in template `{}` requires one scalar value",
                        call.name, registered.name
                    )));
                }
                value
            }
            ferric_rules_parser::SlotType::Multi => {
                let mut values = ferric_rules_core::Multifield::new();
                for expression in &call.args {
                    match eval_env.eval_expr(
                        token,
                        rule_info,
                        expression,
                        context,
                        collected_facts,
                    )? {
                        Value::Multifield(fields) => {
                            values.extend(fields.as_slice().iter().cloned());
                        }
                        // CLIPS omits expressions that return no value from
                        // multislot construction while retaining their effects.
                        Value::Void => {}
                        value => values.push(value),
                    }
                }
                Value::Multifield(Box::new(values))
            }
        };
    }

    Ok(())
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

    #[test]
    fn local_scope_restores_bindings_after_error_and_rule_return() {
        for error in [
            ActionError::EvalError("stopped".into()),
            ActionError::RuleReturn,
        ] {
            let mut env = ActionEvalEnv::default();
            insert_runtime_binding(&mut env.runtime_bindings, "f", Value::Integer(42));
            insert_runtime_binding(&mut env.runtime_bindings, "outside", Value::Integer(5));

            let result: Result<(), ActionError> =
                env.with_local_scope(["$?f", "f", "temporary"], |inner| {
                    assert!(!inner.runtime_bindings.contains_key("f"));
                    assert!(!inner.runtime_bindings.contains_key("temporary"));
                    insert_runtime_binding(&mut inner.runtime_bindings, "f", Value::Integer(7));
                    insert_runtime_binding(
                        &mut inner.runtime_bindings,
                        "temporary",
                        Value::Integer(8),
                    );
                    insert_runtime_binding(
                        &mut inner.runtime_bindings,
                        "outside",
                        Value::Integer(6),
                    );
                    Err(error.clone())
                });

            assert_eq!(result.unwrap_err().to_string(), error.to_string());
            assert!(matches!(
                env.runtime_bindings.get("f"),
                Some(Value::Integer(42))
            ));
            assert!(!env.runtime_bindings.contains_key("temporary"));
            assert!(matches!(
                env.runtime_bindings.get("outside"),
                Some(Value::Integer(6))
            ));
        }
    }

    #[test]
    fn compact_query_scope_restores_after_nested_error_and_rule_return() {
        let outer = FactId::from(slotmap::KeyData::from_ffi(0x0000_0001_0000_0001));
        let inner = FactId::from(slotmap::KeyData::from_ffi(0x0000_0001_0000_0002));
        let nested = FactId::from(slotmap::KeyData::from_ffi(0x0000_0001_0000_0003));
        for error in [
            ActionError::EvalError("stopped".into()),
            ActionError::RuleReturn,
        ] {
            let mut env = ActionEvalEnv::default();
            env.compact_facts
                .insert("f".into(), CompactFactBinding::live(outer));
            insert_runtime_binding(&mut env.runtime_bindings, "f", Value::Integer(42));
            let result: Result<(), ActionError> = env.with_compact_scope(
                &[
                    ("f".into(), CompactFactBinding::live(inner)),
                    ("temporary".into(), CompactFactBinding::live(inner)),
                ],
                |env| {
                    assert_eq!(
                        env.compact_facts.get("f").map(CompactFactBinding::fact_id),
                        Some(inner)
                    );
                    env.with_local_scope(["f"], |env| {
                        insert_runtime_binding(&mut env.runtime_bindings, "f", Value::Integer(7));
                        assert_eq!(
                            env.compact_facts.get("f").map(CompactFactBinding::fact_id),
                            Some(inner)
                        );
                        Ok(())
                    })?;
                    assert!(matches!(
                        env.runtime_bindings.get("f"),
                        Some(Value::Integer(42))
                    ));
                    let nested_result: Result<(), ActionError> = env.with_compact_scope(
                        &[("f".into(), CompactFactBinding::live(nested))],
                        |env| {
                            assert_eq!(
                                env.compact_facts.get("f").map(CompactFactBinding::fact_id),
                                Some(nested)
                            );
                            Err(error.clone())
                        },
                    );
                    assert_eq!(
                        env.compact_facts.get("f").map(CompactFactBinding::fact_id),
                        Some(inner)
                    );
                    nested_result
                },
            );
            assert_eq!(result.unwrap_err().to_string(), error.to_string());
            assert_eq!(
                env.compact_facts.get("f").map(CompactFactBinding::fact_id),
                Some(outer)
            );
            assert!(!env.compact_facts.contains_key("temporary"));
        }
    }

    #[test]
    fn compact_query_scope_does_not_leak_into_the_next_query() {
        let fact = FactId::from(slotmap::KeyData::from_ffi(0x0000_0001_0000_0001));
        let mut env = ActionEvalEnv::default();
        for name in ["f", "g"] {
            env.with_compact_scope(&[(name.into(), CompactFactBinding::live(fact))], |env| {
                assert_eq!(env.compact_facts.len(), 1);
                assert_eq!(
                    env.compact_facts.get(name).map(CompactFactBinding::fact_id),
                    Some(fact)
                );
                Ok(())
            })
            .unwrap();
            assert!(env.compact_facts.is_empty());
        }
    }

    #[test]
    fn runtime_eval_bindings_preserve_shared_values_and_precedence() {
        let mut engine = Engine::new(crate::EngineConfig::utf8());
        let mut rule_info = CompiledRuleInfo {
            name: "binding-test".into(),
            source_definition: None,
            actions: Vec::new(),
            var_map: VarMap::new(),
            fact_address_vars: HashMap::new(),
            salience: Salience::new(0),
            test_conditions: Vec::new(),
            runtime_actions: Vec::new(),
            multifield_tail_bindings: Vec::new(),
        };
        let mut multifield = ferric_rules_core::Multifield::new();
        multifield.push(Value::Integer(7));
        multifield.push(Value::Integer(8));
        let shared = Arc::new(Value::Multifield(Box::new(multifield)));
        let mut token = Token {
            fact: None,
            bindings: BindingSet::new(),
            parent: None,
            owner_node: ferric_rules_core::token::NodeId(0),
        };
        for (name, value) in [
            ("ordinary", ValueRef::Shared(Arc::clone(&shared))),
            ("shadow", ValueRef::new(Value::Integer(2))),
            ("local", ValueRef::new(Value::Integer(20))),
        ] {
            let symbol = engine
                .symbol_table
                .intern_symbol(name, engine.config.string_encoding)
                .unwrap();
            let id = rule_info.var_map.get_or_create(symbol).unwrap();
            token.bindings.set(id, value);
        }
        let fact_bindings = HashMap::from([
            ("shadow".into(), Value::Integer(1)),
            ("only_fact".into(), Value::Integer(3)),
            ("local".into(), Value::Integer(10)),
        ]);
        let runtime_bindings = HashMap::from([("local".into(), Value::Integer(30))]);
        let current_module = engine.module_registry.current_module();
        let (bindings, var_map) = build_runtime_eval_bindings(
            &token,
            &rule_info,
            &fact_bindings,
            &runtime_bindings,
            &mut ActionExecutionContext {
                engine: &mut engine,
                current_module,
            },
        )
        .unwrap();

        for (name, expected) in [("shadow", 2), ("only_fact", 3), ("local", 30)] {
            let symbol = engine
                .symbol_table
                .intern_symbol(name, engine.config.string_encoding)
                .unwrap();
            let id = var_map.lookup(symbol).unwrap();
            assert!(bindings
                .get(id)
                .unwrap()
                .structural_eq(&Value::Integer(expected)));
        }
        let symbol = engine
            .symbol_table
            .intern_symbol("ordinary", engine.config.string_encoding)
            .unwrap();
        let id = var_map.lookup(symbol).unwrap();
        let ValueRef::Shared(merged) = bindings.get(id).unwrap() else {
            panic!("ordinary multifield must retain shared storage");
        };
        assert!(Arc::ptr_eq(&shared, merged));
        assert_eq!(rule_info.var_map.len(), 3);
        assert_eq!(token.bindings.bound_count(), 3);
        let local_symbol = engine
            .symbol_table
            .intern_symbol("local", engine.config.string_encoding)
            .unwrap();
        let local_id = rule_info.var_map.lookup(local_symbol).unwrap();
        assert!(token
            .bindings
            .get(local_id)
            .unwrap()
            .structural_eq(&Value::Integer(20)));
    }

    #[test]
    fn fact_address_seed_rejects_oversized_mapping() {
        let addresses = (0..=ferric_rules_core::compiler::MAX_RULE_CONDITIONS)
            .map(|index| (format!("f{index}"), 0))
            .collect();
        let mut env = RuntimeBindingEnv::new();
        let error =
            seed_fact_address_bindings(&[], &addresses, &mut env, &mut HashMap::new()).unwrap_err();
        assert!(error.to_string().contains("too many fact-address bindings"));
        assert!(env.is_empty());
    }

    #[test]
    fn fact_address_seed_rejects_out_of_range_mapping_without_indexing() {
        let fact_id = FactId::from(slotmap::KeyData::from_ffi(0x0000_0001_0000_0001));
        let addresses = HashMap::from([("f".into(), usize::MAX)]);
        let mut env = RuntimeBindingEnv::new();
        let error =
            seed_fact_address_bindings(&[fact_id], &addresses, &mut env, &mut HashMap::new())
                .unwrap_err();
        assert!(error
            .to_string()
            .contains("references missing fact at position"));
        assert!(env.is_empty());
    }
}

#[cfg(test)]
mod action_query_validation_tests {
    use super::*;
    use crate::evaluator::RuntimeExpr;
    use ferric_rules_parser::{FileId, Position, Span};

    fn query(name: &str, bindings: Vec<(String, String)>, predicate: RuntimeExpr) -> RuntimeExpr {
        let span = Span::new(Position::new(), Position::new(), FileId(0));
        RuntimeExpr::QueryAction {
            name: name.into(),
            bindings,
            query: Box::new(predicate),
            body: vec![(
                ActionExpr::FunctionCall(FunctionCall {
                    name: "assert".into(),
                    args: vec![ActionExpr::FunctionCall(FunctionCall {
                        name: "leaked".into(),
                        args: Vec::new(),
                        span,
                    })],
                    span,
                }),
                None,
            )],
            span: None,
        }
    }

    fn run_empty_query(runtime: &RuntimeExpr) -> Result<(), ActionError> {
        let mut engine = Engine::new(crate::EngineConfig::utf8());
        engine
            .load_str("(deftemplate item (slot value)) (defglobal ?*hits* = 0)")
            .unwrap();
        engine.config.max_action_loop_iterations = 0;
        let initial_count = engine.fact_base.len();
        let token = Token {
            fact: None,
            bindings: BindingSet::new(),
            parent: None,
            owner_node: ferric_rules_core::token::NodeId(0),
        };
        let info = CompiledRuleInfo {
            name: "saved-query".into(),
            source_definition: None,
            actions: Vec::new(),
            var_map: VarMap::new(),
            fact_address_vars: HashMap::new(),
            salience: Salience::new(0),
            test_conditions: Vec::new(),
            runtime_actions: Vec::new(),
            multifield_tail_bindings: Vec::new(),
        };
        let mut env = ActionEvalEnv::default();
        insert_runtime_binding(&mut env.runtime_bindings, "outside", Value::Integer(7));
        let mut context = ActionExecutionContext {
            current_module: engine.module_registry.current_module(),
            engine: &mut engine,
        };
        let action = FunctionCall {
            name: "do-for-all-facts".into(),
            args: Vec::new(),
            span: Span::new(Position::new(), Position::new(), FileId(0)),
        };
        let mut reset = false;
        let mut clear = false;
        context.engine.config.begin_action_loop_budget();
        let result = execute_single_action(
            &mut reset,
            &mut clear,
            &token,
            &info,
            &action,
            Some(runtime),
            &mut context,
            &mut env,
            &[],
        );
        context.engine.config.end_action_loop_budget();
        assert!(!reset && !clear);
        assert_eq!(context.engine.fact_base.len(), initial_count);
        assert!(matches!(
            context.engine.globals.get(context.current_module, "hits"),
            Some(Value::Integer(0))
        ));
        assert!(context.engine.get_output("t").is_none());
        assert!(env.compact_facts.is_empty());
        assert_eq!(env.runtime_bindings.len(), 1);
        assert!(matches!(env.runtime_bindings["outside"], Value::Integer(7)));
        result
    }

    #[test]
    fn restored_empty_action_queries_reject_malformed_headers_before_effects() {
        let mut bindings = vec![
            Vec::new(),
            vec![("f".into(), "item".into()), ("f".into(), "item".into())],
            vec![("f".into(), "item".into()), ("g".into(), "missing".into())],
        ];
        for member in ["", "?f", "$?f", "f:slot", "bad member"] {
            bindings.push(vec![(member.into(), "item".into())]);
        }
        for members in bindings {
            let expr = query(
                "do-for-all-facts",
                members,
                RuntimeExpr::Literal(Value::Integer(1)),
            );
            assert!(run_empty_query(&expr).is_err(), "{expr:?}");
        }
        let unknown = query(
            "forged-query",
            vec![("f".into(), "item".into())],
            RuntimeExpr::Literal(Value::Integer(1)),
        );
        assert!(matches!(
            run_empty_query(&unknown),
            Err(ActionError::UnknownAction(_))
        ));
    }

    #[test]
    fn restored_empty_action_queries_reject_invalid_predicates_before_effects() {
        let predicates = [
            RuntimeExpr::Call {
                name: "bind".into(),
                args: vec![
                    RuntimeExpr::BoundVar {
                        name: "temporary".into(),
                        span: None,
                    },
                    RuntimeExpr::Literal(Value::Integer(1)),
                ],
                span: None,
            },
            RuntimeExpr::Call {
                name: "and".into(),
                args: vec![
                    RuntimeExpr::Call {
                        name: "bind".into(),
                        args: vec![
                            RuntimeExpr::GlobalVar {
                                name: "hits".into(),
                                span: None,
                            },
                            RuntimeExpr::Literal(Value::Integer(1)),
                        ],
                        span: None,
                    },
                    RuntimeExpr::Call {
                        name: "absent-predicate".into(),
                        args: Vec::new(),
                        span: None,
                    },
                ],
                span: None,
            },
        ];
        for predicate in predicates {
            let expr = query(
                "delayed-do-for-all-facts",
                vec![("f".into(), "item".into())],
                predicate,
            );
            let error = run_empty_query(&expr).unwrap_err();
            assert!(
                error.to_string().contains("FACTQPSR2")
                    || error.to_string().contains("absent-predicate")
            );
        }
    }

    #[test]
    fn valid_empty_action_queries_need_no_iteration_budget_or_body_effects() {
        for name in [
            "do-for-fact",
            "do-for-all-facts",
            "delayed-do-for-all-facts",
        ] {
            let expr = query(
                name,
                vec![("f".into(), "item".into())],
                RuntimeExpr::Literal(Value::Integer(1)),
            );
            run_empty_query(&expr).unwrap();
        }
    }
}
