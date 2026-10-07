//! RHS action execution for rule firings.
//!
//! - `GlobalVariable` reads and writes via `GlobalStore`.
//! - `modify`/`duplicate` support template-aware slot overrides.
//! - `printout` with per-channel output capture via `OutputRouter`.

use std::borrow::Cow;
use std::collections::{HashMap, HashSet};
use std::fmt::Write as FmtWrite;
use std::io::Write as IoWrite;
use std::path::Path;
use std::sync::{Arc, OnceLock};

use ferric_rules_core::beta::{RuleId, Salience};
use ferric_rules_core::binding::{BindingSet, ValueRef, VarId, VarMap};
use ferric_rules_core::token::Token;
use ferric_rules_core::{
    EncodingError, Fact, FactBase, FactId, ReteNetwork, SymbolTable, TemplateId, Value,
};
use ferric_rules_parser::{Action, ActionExpr, FunctionCall, LiteralKind};

use crate::evaluator::{CompactFactBinding, EvalError};
use crate::modules::ModuleRegistry;
use crate::qualified_name::{parse_qualified_name, QualifiedName};
use crate::query_cursor::{ActionQueryCursor, QueryCandidate};
use crate::router::OutputRouter;
use crate::tracing_support::{ferric_event, ferric_span};
use crate::Engine;

type RuntimeBindingEnv = HashMap<String, Value>;

pub(crate) struct ActionExecutionContext<'a> {
    pub engine: &'a mut Engine,
    pub current_module: crate::modules::ModuleId,
}

struct ActionEvalEnv {
    allow_engine_effects: bool,
    /// Query membership has lexical scope independent of ordinary loop values.
    compact_facts: crate::evaluator::CompactFactBindings,
    runtime_bindings: RuntimeBindingEnv,
    /// Reused storage for the merged frame built while RHS locals exist.
    merged_frame: RuntimeFrame,
}

impl Default for ActionEvalEnv {
    fn default() -> Self {
        Self {
            allow_engine_effects: true,
            compact_facts: HashMap::default(),
            runtime_bindings: HashMap::default(),
            merged_frame: Default::default(),
        }
    }
}

/// An evaluation frame: its bindings and the names they are keyed by.
type RuntimeFrame = (BindingSet, VarMap);

fn flush_deferred_printout(context: &mut ActionExecutionContext<'_>) {
    for (channel, text) in context.engine.globals.take_printout_events() {
        context.engine.router.write(&channel, &text);
    }
}

/// Write action output directly to the router, after any output that
/// evaluation has queued, so a direct write never overtakes earlier text.
fn write_output(context: &mut ActionExecutionContext<'_>, channel: &str, text: &str) {
    flush_deferred_printout(context);
    context.engine.router.write(channel, text);
}

impl ActionEvalEnv {
    /// Mask outer RHS locals while query/loop bindings are in scope, then
    /// restore them even when the body returns an error or a rule return.
    fn with_local_scope<'a, T>(
        &mut self,
        names: impl IntoIterator<Item = &'a str>,
        execute: impl FnOnce(&mut Self) -> Result<T, ActionError>,
    ) -> Result<T, ActionError> {
        // Scopes hold a few names and are entered once per query candidate,
        // so a small list beats a map here.
        let mut saved = smallvec::SmallVec::<[(&str, Option<Value>); 4]>::new();
        for name in names {
            let name = name.strip_prefix("$?").unwrap_or(name);
            if saved.iter().all(|(seen, _)| *seen != name) {
                saved.push((name, self.runtime_bindings.remove(name)));
            }
        }
        let result = execute(self);
        for (name, previous) in saved {
            if let Some(value) = previous {
                self.runtime_bindings.insert(name.to_string(), value);
            } else {
                self.runtime_bindings.remove(name);
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
        let mut saved = smallvec::SmallVec::<[(&str, Option<CompactFactBinding>); 4]>::new();
        for (name, binding) in bindings {
            let previous = if let Some(current) = self.compact_facts.get_mut(name.as_str()) {
                Some(std::mem::replace(current, binding.clone()))
            } else {
                self.compact_facts.insert(name.clone(), binding.clone());
                None
            };
            if saved.iter().all(|(seen, _)| *seen != name) {
                saved.push((name, previous));
            }
        }
        let result = execute(self);
        for (name, previous) in saved {
            if let Some(binding) = previous {
                self.compact_facts.insert(name.to_string(), binding);
            } else {
                self.compact_facts.remove(name);
            }
        }
        result
    }

    fn make_eval_context<'ctx>(
        token: &'ctx Token,
        rule_info: &'ctx CompiledRuleInfo,
        context: &'ctx mut ActionExecutionContext<'_>,
        compact_facts: &'ctx crate::evaluator::CompactFactBindings,
        allow_engine_effects: bool,
    ) -> crate::evaluator::EvalContext<'ctx> {
        let engine = &mut *context.engine;
        let (call_depth, expression_depth) = engine.eval_depth_floor;
        crate::evaluator::EvalContext {
            global_module: None,
            engine,
            bindings: &token.bindings,
            var_map: &rule_info.var_map,
            call_depth,
            expression_depth,
            callable_locals: None,
            current_module: context.current_module,
            method_chain: None,
            compact_fact_bindings: Some(compact_facts),
            allow_engine_effects,
        }
    }

    fn eval_runtime_expr(
        &mut self,
        token: &Token,
        rule_info: &CompiledRuleInfo,
        runtime_expr: &crate::evaluator::RuntimeExpr,
        context: &mut ActionExecutionContext<'_>,
    ) -> Result<Value, ActionError> {
        let has_locals = !self.runtime_bindings.is_empty();
        if has_locals {
            // Canonicalize activation aliases, but leave mutable RHS names in
            // CallableLocals alone: unbind must not resurrect a stale overlay.
            build_runtime_eval_bindings(
                token,
                rule_info,
                &RuntimeBindingEnv::new(),
                context,
                &mut self.merged_frame,
            )?;
        }
        // Expressions may bind RHS locals too. Restore the shared map on both
        // success and error so completed nested effects retain their bindings.
        let mut locals = crate::evaluator::CallableLocals::from_values(std::mem::take(
            &mut self.runtime_bindings,
        ));
        let result = if has_locals {
            let (bindings, var_map) = &self.merged_frame;
            Self::eval_runtime_expr_with_bindings(
                runtime_expr,
                bindings,
                var_map,
                context,
                &self.compact_facts,
                self.allow_engine_effects,
                &mut locals,
            )
        } else {
            let mut ctx = Self::make_eval_context(
                token,
                rule_info,
                context,
                &self.compact_facts,
                self.allow_engine_effects,
            );
            ctx.callable_locals = Some(&mut locals);
            crate::evaluator::eval_action_expression(&mut ctx, runtime_expr)
                .map_err(ActionError::from_action_evaluation)
        };
        self.runtime_bindings = locals.into_values();
        self.merged_frame.0.clear();
        result
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
        allow_engine_effects: bool,
        locals: &mut crate::evaluator::CallableLocals,
    ) -> Result<Value, ActionError> {
        let engine = &mut *context.engine;
        let (call_depth, expression_depth) = engine.eval_depth_floor;
        let mut ctx = crate::evaluator::EvalContext {
            global_module: None,
            engine,
            bindings,
            var_map,
            call_depth,
            expression_depth,
            callable_locals: Some(locals),
            current_module: context.current_module,
            method_chain: None,
            compact_fact_bindings: Some(compact_facts),
            allow_engine_effects,
        };
        crate::evaluator::eval_action_expression(&mut ctx, runtime_expr)
            .map_err(ActionError::from_action_evaluation)
    }
}

/// A compiled condition evaluated when a partial match reaches its predicate node.
#[derive(Clone, Debug)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub(crate) enum CompiledTestCondition {
    Expr(crate::evaluator::RuntimeExpr),
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
    /// Push the owning module whenever a new activation is created.
    pub auto_focus: bool,
    /// Pre-translated match conditions referenced by predicate-node indexes.
    pub test_conditions: Vec<CompiledTestCondition>,
    /// Pre-translated RHS action call expressions.
    pub runtime_actions: Vec<Option<crate::evaluator::RuntimeExpr>>,
    /// Derived from the fields above on first activation; never serialized.
    #[cfg_attr(feature = "serde", serde(skip))]
    pub activation_layout: OnceLock<ActivationLayout>,
}

/// Per-rule facts about activations that would otherwise be recomputed from
/// names on every firing.
#[derive(Clone, Debug)]
pub(crate) struct ActivationLayout {
    /// Frame slot and collected-fact index for each pattern fact address.
    fact_address_slots: Vec<(VarId, usize)>,
    /// Whether any RHS expression uses the compact `?fact:slot` form.
    compact_slot_refs: bool,
}

impl CompiledRuleInfo {
    pub(crate) fn activation_layout(
        &self,
        symbol_table: &SymbolTable,
        encoding: ferric_rules_core::StringEncoding,
    ) -> &ActivationLayout {
        self.activation_layout.get_or_init(|| ActivationLayout {
            fact_address_slots: self
                .fact_address_vars
                .iter()
                .filter_map(|(name, &index)| {
                    let symbol = symbol_table.find_symbol(name, encoding)?;
                    Some((self.var_map.lookup(symbol)?, index))
                })
                .collect(),
            compact_slot_refs: self
                .actions
                .iter()
                .any(|action| call_uses_compact_slot_refs(&action.call)),
        })
    }
}

fn call_uses_compact_slot_refs(call: &FunctionCall) -> bool {
    call.name == crate::evaluator::COMPACT_FACT_SLOT_REF
        || call.args.iter().any(expr_uses_compact_slot_refs)
}

fn expr_uses_compact_slot_refs(expr: &ActionExpr) -> bool {
    let mut pending = vec![expr];
    while let Some(expr) = pending.pop() {
        if matches!(expr, ActionExpr::FunctionCall(call) if call.name == crate::evaluator::COMPACT_FACT_SLOT_REF)
        {
            return true;
        }
        expr.push_children(&mut pending);
    }
    false
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
    /// Internal non-error signal consumed by the nearest action loop.
    #[doc(hidden)]
    #[error("internal loop break control escaped the action sequence")]
    LoopBreak,
}

impl ActionError {
    fn from_action_evaluation(error: crate::evaluator::EvalError) -> Self {
        match error {
            crate::evaluator::EvalError::BreakControl { .. } => Self::LoopBreak,
            error => Self::Evaluator(error),
        }
    }
}

/// Execute actions for a fired rule.
///
/// This is called with all the data needed pre-extracted to avoid borrow issues.
///
/// Returns `(fired, errors)` where:
/// - `fired` is `true` once the already-matched activation reaches execution.
/// - `errors` is a list of non-fatal action errors that occurred during execution.
#[allow(clippy::too_many_lines)] // Sequential action/test evaluation flow with explicit error branches.
pub(crate) fn execute_actions(
    token: &Token,
    rule_info: &CompiledRuleInfo,
    context: &mut ActionExecutionContext<'_>,
    collected_facts: &[FactId],
) -> (bool, Vec<ActionError>) {
    context.engine.config.begin_action_loop_budget();
    ferric_span!(
        debug_span,
        "execute_actions",
        rule = %rule_info.name,
        action_count = rule_info.actions.len(),
        test_count = rule_info.test_conditions.len()
    );
    let mut errors = Vec::new();
    let mut eval_env = ActionEvalEnv::default();
    // Defensive: clear any stale deferred events that might have accumulated
    // in non-action evaluation contexts.
    let _ = context.engine.globals.take_printout_events();
    let layout = rule_info.activation_layout(
        &context.engine.symbol_table,
        context.engine.config.string_encoding,
    );
    if layout.compact_slot_refs {
        seed_compact_fact_addresses(
            collected_facts,
            &rule_info.fact_address_vars,
            &context.engine.fact_base,
            context.engine.initial_fact_id,
            context.engine.fact_epoch,
            context.engine.fact_index_starts_at_zero,
            &mut eval_env.compact_facts,
        );
    }

    for (index, action) in rule_info.actions.iter().enumerate() {
        let runtime_call = rule_info
            .runtime_actions
            .get(index)
            .and_then(Option::as_ref);
        let action_result = execute_single_action(
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
    }

    ferric_event!(
        debug,
        rule = %rule_info.name,
        error_count = errors.len(),
        "execute_actions_complete"
    );
    context.engine.config.end_action_loop_budget();
    (true, errors)
}

/// Evaluate one rule-local predicate for an incoming partial match.
pub(crate) fn evaluate_test_condition(
    token: &Token,
    rule_info: &CompiledRuleInfo,
    test_condition: &CompiledTestCondition,
    context: &mut ActionExecutionContext<'_>,
) -> Result<bool, ActionError> {
    let mut eval_env = ActionEvalEnv {
        allow_engine_effects: false,
        ..Default::default()
    };

    let CompiledTestCondition::Expr(test_expr) = test_condition;
    let result = eval_env
        .eval_runtime_expr(token, rule_info, test_expr, context)
        .map(|value| crate::evaluator::is_truthy(&value, &context.engine.symbol_table));
    flush_deferred_printout(context);
    result
}

/// Bind pattern fact addresses (`?f <- (...)`) into an activation's own token
/// copy, so RHS expressions read them through the ordinary binding frame.
/// A same-named pattern variable keeps precedence.
#[allow(clippy::too_many_arguments)] // Captures engine identity alongside activation bindings.
pub(crate) fn bind_fact_addresses(
    token: &mut Token,
    rule_info: &CompiledRuleInfo,
    collected_facts: &[FactId],
    symbol_table: &SymbolTable,
    encoding: ferric_rules_core::StringEncoding,
    fact_base: &FactBase,
    initial_fact_id: Option<FactId>,
    fact_epoch: u64,
    fact_index_starts_at_zero: bool,
) {
    let layout = rule_info.activation_layout(symbol_table, encoding);
    for &(id, index) in &layout.fact_address_slots {
        let Some(fact_id) = collected_facts.get(index) else {
            continue;
        };
        if token.bindings.get(id).is_none() {
            if let Some(address) = crate::fact_address::make_fact_address(
                fact_base,
                initial_fact_id,
                fact_epoch,
                fact_index_starts_at_zero,
                *fact_id,
            ) {
                token
                    .bindings
                    .set(id, ValueRef::new(Value::FactAddress(address)));
            }
        }
    }
}

/// Compact `?f:slot` references read the pattern fact even when a loop or
/// query later reuses the ordinary variable name.
fn seed_compact_fact_addresses(
    collected_facts: &[FactId],
    addresses: &HashMap<String, usize>,
    fact_base: &FactBase,
    initial_fact_id: Option<FactId>,
    fact_epoch: u64,
    fact_index_starts_at_zero: bool,
    compact_facts: &mut crate::evaluator::CompactFactBindings,
) {
    for (name, &index) in addresses {
        if let Some(address) = collected_facts.get(index).and_then(|fact_id| {
            crate::fact_address::make_fact_address(
                fact_base,
                initial_fact_id,
                fact_epoch,
                fact_index_starts_at_zero,
                *fact_id,
            )
        }) {
            compact_facts.insert(
                name.strip_prefix("$?").unwrap_or(name).to_string(),
                CompactFactBinding::live(address),
            );
        }
    }
}

fn insert_runtime_binding(env: &mut RuntimeBindingEnv, name: &str, value: Value) {
    // Both spellings share one current value, including after an RHS bind.
    env.insert(name.strip_prefix("$?").unwrap_or(name).to_string(), value);
}

/// Build the frame that RHS expressions see while RHS locals exist, reusing
/// `frame`'s storage.
fn build_runtime_eval_bindings(
    token: &Token,
    rule_info: &CompiledRuleInfo,
    env: &RuntimeBindingEnv,
    context: &mut ActionExecutionContext<'_>,
    frame: &mut RuntimeFrame,
) -> Result<(), ActionError> {
    let (bindings, var_map) = frame;
    bindings.clear();
    var_map.clear();

    // Preserve outer VarId order: when both spellings exist, the later
    // `$?name`/`name` alias wins, just as in the former merged map.
    for index in 0..rule_info.var_map.len() {
        #[allow(clippy::cast_possible_truncation)] // VarMap limits IDs to u16.
        let outer_id = VarId(index as u16);
        let Some(value) = token.bindings.get(outer_id) else {
            continue;
        };
        let original = rule_info.var_map.name(outer_id);
        let Some(name) = context.engine.symbol_table.resolve_symbol_str(original) else {
            continue;
        };
        let name = name.strip_prefix("$?").unwrap_or(name);
        // Reuse the interned canonical name when present, while retaining the
        // current encoding's lookup/intern behavior for aliases and mixed tables.
        let symbol = if let Some(symbol) = context
            .engine
            .symbol_table
            .find_symbol(name, context.engine.config.string_encoding)
        {
            symbol
        } else {
            let name = name.to_owned();
            context
                .engine
                .symbol_table
                .intern_symbol(&name, context.engine.config.string_encoding)
                .map_err(ActionError::Encoding)?
        };
        let id = var_map
            .get_or_create(symbol)
            .map_err(|error| ActionError::EvalError(error.to_string()))?;
        bindings.set(id, value.clone());
    }
    // Runtime locals override outer aliases. Clone their values once, directly
    // into the evaluation frame, without an intermediate owned-value map.
    for (name, value) in env {
        let name = name.strip_prefix("$?").unwrap_or(name);
        let symbol = context
            .engine
            .symbol_table
            .intern_symbol(name, context.engine.config.string_encoding)
            .map_err(ActionError::Encoding)?;
        let id = var_map
            .get_or_create(symbol)
            .map_err(|error| ActionError::EvalError(error.to_string()))?;
        bindings.set(id, ValueRef::new(value.clone()));
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)] // Action dispatch needs full mutable engine/action context.
#[allow(clippy::too_many_lines)] // if-branch execution adds necessary verbosity
fn execute_single_action(
    token: &Token,
    rule_info: &CompiledRuleInfo,
    call: &FunctionCall,
    runtime_call: Option<&crate::evaluator::RuntimeExpr>,
    context: &mut ActionExecutionContext<'_>,
    eval_env: &mut ActionEvalEnv,
    collected_facts: &[FactId],
) -> Result<(), ActionError> {
    if takes_expanded_operands(call) {
        let call =
            expand_action_operands(token, rule_info, call, context, eval_env, collected_facts)?;
        return execute_single_action(
            token,
            rule_info,
            &call,
            None,
            context,
            eval_env,
            collected_facts,
        );
    }
    match call.name.as_str() {
        "assert" | "retract" | "modify" | "duplicate" | "halt" | "focus" | "reset" | "clear" => {
            if let Some(runtime_expr) = runtime_call {
                eval_env.eval_runtime_expr(token, rule_info, runtime_expr, context)
            } else {
                eval_env.eval_expr(
                    token,
                    rule_info,
                    &ActionExpr::FunctionCall(call.clone()),
                    context,
                    collected_facts,
                )
            }
            .map(|_| ())
        }
        "printout" | "bind"
            if call.args.iter().any(
                |arg| matches!(arg, ActionExpr::FunctionCall(expansion) if expansion.name == "expand$"),
            ) =>
        {
            if let Some(runtime_expr) = runtime_call {
                eval_env.eval_runtime_expr(token, rule_info, runtime_expr, context)
            } else {
                eval_env.eval_expr(
                    token,
                    rule_info,
                    &ActionExpr::FunctionCall(call.clone()),
                    context,
                    collected_facts,
                )
            }
            .map(|_| ())
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
        "list-focus-stack" => {
            flush_deferred_printout(context);
            execute_list_focus_stack(&mut context.engine.router, &context.engine.module_registry)
        }
        "agenda" => {
            flush_deferred_printout(context);
            execute_agenda(
                &context.engine.rete,
                &mut context.engine.router,
                &context.engine.rule_info,
            )
        }
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
        "break" => {
            if call.args.is_empty() {
                Err(ActionError::LoopBreak)
            } else {
                Err(ActionError::Evaluator(
                    crate::evaluator::EvalError::ArityMismatch {
                        name: "break".to_string(),
                        expected: "0".to_string(),
                        actual: call.args.len(),
                        span: Some(crate::evaluator::SourceSpan {
                            line: call.span.start.line,
                            column: call.span.start.column,
                        }),
                    },
                ))
            }
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
        "progn" => {
            let compiled_args = match runtime_call {
                Some(crate::evaluator::RuntimeExpr::Call { args, .. }) => Some(args),
                _ => None,
            };
            let body: Vec<_> = call
                .args
                .iter()
                .enumerate()
                .map(|(index, source)| {
                    let compiled = compiled_args
                        .and_then(|args| args.get(index))
                        .cloned()
                        .map(Box::new);
                    (source.clone(), compiled)
                })
                .collect();
            execute_loop_body(token, rule_info, &body, context, eval_env, collected_facts)
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
                execute_loop_body(token, rule_info, branch, context, eval_env, collected_facts)
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
                    match execute_loop_body(
                        token,
                        rule_info,
                        body,
                        context,
                        eval_env,
                        collected_facts,
                    ) {
                        Ok(()) => {}
                        Err(ActionError::LoopBreak) => break,
                        Err(error) => return Err(error),
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

                // A loop owns one lexical scope; RHS overrides survive its iterations.
                eval_env.with_local_scope(var_name.as_deref(), |eval_env| {
                    let mut loop_frame: Option<(Token, CompiledRuleInfo, VarId)> = None;
                    for counter in start_int..=end_int {
                        crate::evaluator::consume_action_loop_iteration(
                            &context.engine.config,
                            "loop-for-count",
                            span.clone(),
                        )
                        .map_err(ActionError::from)?;
                        // Initialize after the first budget check: empty ranges and
                        // exhausted budgets must not introduce a variable early.
                        let (loop_token, loop_rule_info) = if let Some(var) = var_name {
                            if let Some((frame_token, _, id)) = &mut loop_frame {
                                frame_token
                                    .bindings
                                    .set(*id, ValueRef::new(Value::Integer(counter)));
                            } else {
                                loop_frame = Some(augment_bindings_with_var(
                                    token,
                                    rule_info,
                                    var,
                                    Value::Integer(counter),
                                    &mut context.engine.symbol_table,
                                    &context.engine.config,
                                )?);
                            }
                            let (frame_token, frame_info, _) = loop_frame.as_ref().unwrap();
                            (frame_token, frame_info)
                        } else {
                            (token, rule_info)
                        };

                        match execute_loop_body(
                            loop_token,
                            loop_rule_info,
                            body,
                            context,
                            eval_env,
                            collected_facts,
                        ) {
                            Ok(()) => {}
                            Err(ActionError::LoopBreak) => break,
                            Err(error) => return Err(error),
                        }
                    }
                    Ok(())
                })
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
                    execute_loop_body(token, rule_info, body, context, eval_env, collected_facts)?;
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
                // Keep the evaluated list owned locally while iterating borrowed fields.
                let list_value =
                    eval_env.eval_runtime_expr(token, rule_info, list_expr, context)?;
                let elements = match &list_value {
                    Value::Multifield(values) => values.as_slice(),
                    other => std::slice::from_ref(other),
                };

                let index_var_name = format!("{var_name}-index");
                eval_env.with_local_scope(
                    [var_name.as_str(), index_var_name.as_str()],
                    |eval_env| {
                        let mut loop_frame: Option<(Token, CompiledRuleInfo, VarId, VarId)> = None;
                        for (idx, element) in elements.iter().enumerate() {
                            #[allow(clippy::cast_possible_wrap)]
                            // usize→i64: element counts can't exceed i64
                            let one_based = idx as i64 + 1;

                            if let Some((frame_token, _, value_id, index_id)) = &mut loop_frame {
                                frame_token
                                    .bindings
                                    .set(*value_id, ValueRef::new(element.clone()));
                                frame_token
                                    .bindings
                                    .set(*index_id, ValueRef::new(Value::Integer(one_based)));
                            } else {
                                let (token1, info1, value_id) = augment_bindings_with_var(
                                    token,
                                    rule_info,
                                    var_name,
                                    element.clone(),
                                    &mut context.engine.symbol_table,
                                    &context.engine.config,
                                )?;
                                let (frame_token, frame_info, index_id) =
                                    augment_bindings_with_var(
                                        &token1,
                                        &info1,
                                        &index_var_name,
                                        Value::Integer(one_based),
                                        &mut context.engine.symbol_table,
                                        &context.engine.config,
                                    )?;
                                loop_frame = Some((frame_token, frame_info, value_id, index_id));
                            }
                            let (loop_token, loop_rule_info, _, _) = loop_frame.as_ref().unwrap();

                            match execute_loop_body(
                                loop_token,
                                loop_rule_info,
                                body,
                                context,
                                eval_env,
                                collected_facts,
                            ) {
                                Ok(()) => {}
                                Err(ActionError::LoopBreak) => break,
                                Err(error) => return Err(error),
                            }
                        }
                        Ok(())
                    },
                )
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

/// Whether `call` is a dedicated action handler, which evaluates its own
/// operands, given an explicit `expand$` operand. The evaluator expands
/// `printout`, `bind` and ordinary function calls itself, and `println`
/// prints expanded fields directly, so fields without a literal form (such
/// as fact addresses) still print.
fn takes_expanded_operands(call: &FunctionCall) -> bool {
    matches!(
        call.name.as_str(),
        "list-focus-stack"
            | "agenda"
            | "rules"
            | "undefrule"
            | "undeffacts"
            | "ppdefrule"
            | "load"
            | "load-facts"
            | "save-facts"
    ) && call.args.iter().any(is_expansion)
}

fn is_expansion(argument: &ActionExpr) -> bool {
    matches!(argument, ActionExpr::FunctionCall(expansion) if expansion.name == "expand$")
}

/// Replace each `expand$` operand with literals of its fields, evaluating the
/// expansions first, in source order, as the evaluator does for function
/// calls. Other operands stay expressions. The expanded count is rechecked.
fn expand_action_operands(
    token: &Token,
    rule_info: &CompiledRuleInfo,
    call: &FunctionCall,
    context: &mut ActionExecutionContext<'_>,
    eval_env: &mut ActionEvalEnv,
    collected_facts: &[FactId],
) -> Result<FunctionCall, ActionError> {
    let span_of = |call: &FunctionCall| crate::evaluator::SourceSpan {
        line: call.span.start.line,
        column: call.span.start.column,
    };
    let mut args = Vec::with_capacity(call.args.len());
    for argument in &call.args {
        let ActionExpr::FunctionCall(expansion) = argument else {
            args.push(argument.clone());
            continue;
        };
        if expansion.name != "expand$" {
            args.push(argument.clone());
            continue;
        }
        let [operand] = expansion.args.as_slice() else {
            return Err(ActionError::Evaluator(EvalError::ArityMismatch {
                name: "expand$".into(),
                expected: "1".into(),
                actual: expansion.args.len(),
                span: Some(span_of(expansion)),
            }));
        };
        let value = eval_env.eval_expr(token, rule_info, operand, context, collected_facts)?;
        let Value::Multifield(fields) = value else {
            return Err(ActionError::Evaluator(EvalError::TypeError {
                function: "expand$".into(),
                expected: "MULTIFIELD".into(),
                actual: runtime_value_type_name(&value).into(),
                span: Some(span_of(expansion)),
            }));
        };
        for field in fields.iter() {
            let symbols = &context.engine.symbol_table;
            let text = |symbol| symbols.resolve_symbol_str(symbol).map(str::to_owned);
            let literal = match field {
                Value::Integer(value) => Some(LiteralKind::Integer(*value)),
                Value::Float(value) => Some(LiteralKind::Float(*value)),
                Value::Symbol(symbol) => text(*symbol).map(LiteralKind::Symbol),
                Value::String(value) => Some(LiteralKind::String(value.as_str().to_owned())),
                Value::InstanceName(name) => text(name.as_symbol()).map(LiteralKind::InstanceName),
                _ => None,
            };
            let Some(value) = literal else {
                return Err(ActionError::Evaluator(EvalError::TypeError {
                    function: call.name.clone(),
                    expected: "a number, SYMBOL, STRING, or INSTANCE-NAME".into(),
                    actual: runtime_value_type_name(field).into(),
                    span: Some(span_of(call)),
                }));
            };
            args.push(ActionExpr::Literal(ferric_rules_parser::LiteralValue {
                value,
                span: expansion.span,
            }));
        }
    }
    crate::builtin_validation::validate_runtime_arity(&call.name, args.len()).map_err(
        |expected| {
            ActionError::Evaluator(EvalError::ArityMismatch {
                name: call.name.clone(),
                expected,
                actual: args.len(),
                span: Some(span_of(call)),
            })
        },
    )?;
    Ok(FunctionCall {
        name: call.name.clone(),
        args,
        span: call.span,
    })
}

/// Clone the metadata used by temporary action binding frames.
///
/// Clones the parts needed for loop body execution.  `runtime_actions` is
/// intentionally left empty because the loop body items are dispatched via
/// `execute_loop_body`, which constructs `runtime_call` from the `RuntimeExpr`
/// body entries directly rather than from a `runtime_actions` index.
fn rule_info_clone_light(rule_info: &CompiledRuleInfo) -> CompiledRuleInfo {
    CompiledRuleInfo {
        name: rule_info.name.clone(),
        // Introspection reads the registered rule in Engine, not this frame.
        source_definition: None,
        actions: Vec::new(),
        var_map: rule_info.var_map.clone(),
        fact_address_vars: rule_info.fact_address_vars.clone(),
        salience: rule_info.salience,
        auto_focus: rule_info.auto_focus,
        test_conditions: Vec::new(),
        runtime_actions: Vec::new(),
        // Loop bodies never start an activation.
        activation_layout: OnceLock::new(),
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
) -> Result<(Token, CompiledRuleInfo, VarId), ActionError> {
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

    Ok((new_token, new_rule_info, var_id))
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
            Cow<'_, ferric_rules_parser::FunctionCall>,
            Option<&crate::evaluator::RuntimeExpr>,
        ) = match action_expr {
            ActionExpr::FunctionCall(fc) => {
                let rt: Option<&crate::evaluator::RuntimeExpr> = rt_expr.as_deref();
                (Cow::Borrowed(fc), rt)
            }
            ActionExpr::If { span, .. } => {
                let synthetic = ferric_rules_parser::FunctionCall {
                    name: "if".to_string(),
                    args: vec![],
                    span: *span,
                };
                let rt: Option<&crate::evaluator::RuntimeExpr> = rt_expr.as_deref();
                (Cow::Owned(synthetic), rt)
            }
            ActionExpr::While { span, .. } => {
                let synthetic = ferric_rules_parser::FunctionCall {
                    name: "while".to_string(),
                    args: vec![],
                    span: *span,
                };
                let rt: Option<&crate::evaluator::RuntimeExpr> = rt_expr.as_deref();
                (Cow::Owned(synthetic), rt)
            }
            ActionExpr::LoopForCount { span, .. } => {
                let synthetic = ferric_rules_parser::FunctionCall {
                    name: "loop-for-count".to_string(),
                    args: vec![],
                    span: *span,
                };
                let rt: Option<&crate::evaluator::RuntimeExpr> = rt_expr.as_deref();
                (Cow::Owned(synthetic), rt)
            }
            ActionExpr::Progn { span, .. } => {
                let synthetic = ferric_rules_parser::FunctionCall {
                    name: "progn$".to_string(),
                    args: vec![],
                    span: *span,
                };
                let rt: Option<&crate::evaluator::RuntimeExpr> = rt_expr.as_deref();
                (Cow::Owned(synthetic), rt)
            }
            ActionExpr::QueryAction { name, span, .. } => {
                let synthetic = ferric_rules_parser::FunctionCall {
                    name: name.clone(),
                    args: vec![],
                    span: *span,
                };
                let rt: Option<&crate::evaluator::RuntimeExpr> = rt_expr.as_deref();
                (Cow::Owned(synthetic), rt)
            }
            ActionExpr::Switch { span, .. } => {
                let synthetic = ferric_rules_parser::FunctionCall {
                    name: "switch".to_string(),
                    args: vec![],
                    span: *span,
                };
                let rt: Option<&crate::evaluator::RuntimeExpr> = rt_expr.as_deref();
                (Cow::Owned(synthetic), rt)
            }
            _ => {
                // Literal/variable — evaluate as expression; result is discarded.
                if let Some(rt) = rt_expr {
                    eval_env.eval_runtime_expr(token, rule_info, rt, context)?;
                } else {
                    eval_env.eval_expr(token, rule_info, action_expr, context, collected_facts)?;
                }
                flush_deferred_printout(context);
                continue;
            }
        };
        execute_single_action(
            token,
            rule_info,
            &branch_call,
            branch_runtime,
            context,
            eval_env,
            collected_facts,
        )?;
        // Queued output belongs to this item, not the enclosing action.
        flush_deferred_printout(context);
    }
    Ok(())
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
    // One copy of the frame per candidate, extended in place for each member.
    let mut aug_token = token.clone();
    let mut aug_rule_info = rule_info_clone_light(rule_info);
    for (name, member) in candidate {
        let symbol = context
            .engine
            .symbol_table
            .intern_symbol(name, context.engine.config.string_encoding)
            .map_err(ActionError::from)?;
        let var_id = aug_rule_info
            .var_map
            .get_or_create(symbol)
            .map_err(|_| ActionError::EvalError(format!("query: too many variables for {name}")))?;
        aug_token.bindings.set(
            var_id,
            ValueRef::new(Value::FactAddress(member.address().clone())),
        );
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
        &mut ActionEvalEnv::make_eval_context(
            token,
            rule_info,
            context,
            &eval_env.compact_facts,
            eval_env.allow_engine_effects,
        ),
        query,
        name,
        None,
        0,
    )
    .map_err(ActionError::from)?;
    let mut cursor = ActionQueryCursor::new(bindings, context.engine, context.current_module)?;
    let mut selected = Vec::new();
    while let Some(candidate) = cursor.next(context.engine, name)? {
        let result = with_query_candidate(
            &candidate,
            token,
            rule_info,
            context,
            eval_env,
            |token, rule_info, context, eval_env| {
                let value = eval_env.eval_runtime_expr(token, rule_info, query, context)?;
                let matched = crate::evaluator::is_truthy(&value, &context.engine.symbol_table);
                if matched && !delayed {
                    execute_loop_body(token, rule_info, body, context, eval_env, collected_facts)?;
                }
                Ok(matched)
            },
        );
        let matched = match result {
            Ok(matched) => matched,
            Err(ActionError::LoopBreak) => return Ok(()),
            Err(error) => return Err(error),
        };
        if matched && delayed {
            selected.push(candidate);
        }
        if matched && stop_after_first {
            return Ok(());
        }
    }
    for candidate in selected {
        crate::evaluator::consume_action_loop_iteration(&context.engine.config, name, None)
            .map_err(ActionError::from)?;
        let result = with_query_candidate(
            &candidate,
            token,
            rule_info,
            context,
            eval_env,
            |token, rule_info, context, eval_env| {
                execute_loop_body(token, rule_info, body, context, eval_env, collected_facts)
            },
        );
        match result {
            Ok(()) => {}
            Err(ActionError::LoopBreak) => break,
            Err(error) => return Err(error),
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
    write_output(context, "t", &output);
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
        write_output(context, "t", &output);
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
        Value::InstanceName(name) => {
            if let Some(name) = symbol_table.resolve_symbol_str(name.as_symbol()) {
                let _ = write!(output, "[{name}]");
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
        Value::FactAddress(_) => {
            // CLIPS saves the address spelling as a string, not a reusable identity.
            output.push('"');
            crate::value_print::append_printout_value(value, symbol_table, output);
            output.push('"');
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
        Value::InstanceName(_) => "INSTANCE-NAME",
        Value::Multifield(_) => "MULTIFIELD",
        Value::ExternalAddress(_) => "EXTERNAL-ADDRESS",
        Value::FactAddress(_) => "FACT-ADDRESS",
        Value::Void => "VOID",
    }
}

/// Execute a `printout` action.
///
/// The first argument is the channel name (typically `t`) and must be a literal.
/// Remaining arguments are evaluated and formatted, with the special symbols
/// `crlf`, `tab`, `vtab`, and `ff` producing `\n`, `\t`, `\x0B`, and `\x0C` respectively.
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
            LiteralKind::Symbol(s) | LiteralKind::String(s) | LiteralKind::InstanceName(s) => {
                s.clone()
            }
            LiteralKind::Integer(n) => n.to_string(),
            LiteralKind::Float(f) => f.to_string(),
        },
        _ => {
            return Err(ActionError::EvalError(
                "printout: channel must be a literal symbol or string".to_string(),
            ))
        }
    };

    // CLIPS suppresses both output and operand evaluation for the nil router.
    if channel == "nil" {
        return Ok(());
    }

    // Write each argument before evaluating the next, so nested output and
    // errors retain the output order and any successfully written prefix.
    let mut output = String::new();
    for arg in &args[1..] {
        let value = eval_env.eval_expr(token, rule_info, arg, context, collected_facts)?;
        crate::value_print::append_printout_value(
            &value,
            &context.engine.symbol_table,
            &mut output,
        );
        write_output(context, &channel, &output);
        output.clear();
    }
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
        let value = match arg {
            ActionExpr::FunctionCall(expansion) if expansion.name == "expand$" => {
                let span = Some(crate::evaluator::SourceSpan {
                    line: expansion.span.start.line,
                    column: expansion.span.start.column,
                });
                let [operand] = expansion.args.as_slice() else {
                    return Err(ActionError::Evaluator(EvalError::ArityMismatch {
                        name: "expand$".into(),
                        expected: "1".into(),
                        actual: expansion.args.len(),
                        span,
                    }));
                };
                let value =
                    eval_env.eval_expr(token, rule_info, operand, context, collected_facts)?;
                let Value::Multifield(fields) = value else {
                    return Err(ActionError::Evaluator(EvalError::TypeError {
                        function: "expand$".into(),
                        expected: "MULTIFIELD".into(),
                        actual: runtime_value_type_name(&value).into(),
                        span,
                    }));
                };
                // Each expanded field is its own println argument.
                for field in fields.iter() {
                    crate::value_print::append_printout_value(
                        field,
                        &context.engine.symbol_table,
                        &mut output,
                    );
                    write_output(context, "t", &output);
                    output.clear();
                }
                continue;
            }
            _ => eval_env.eval_expr(token, rule_info, arg, context, collected_facts)?,
        };
        crate::value_print::append_printout_value(
            &value,
            &context.engine.symbol_table,
            &mut output,
        );
        write_output(context, "t", &output);
        output.clear();
    }
    write_output(context, "t", "\n");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    use proptest::prelude::*;

    proptest! {
        #[test]
        fn temporary_bindings_preserve_alias_precedence_and_values(
            first in any::<i64>(),
            second in any::<i64>(),
            local in prop::option::of(any::<i64>()),
            reverse in any::<bool>(),
            mixed_encoding in any::<bool>(),
            fields in prop::collection::vec(any::<i64>(), 0..32),
        ) {
            let mut engine = Engine::new(crate::EngineConfig::utf8());
            engine.load_str("(defrule frame =>)").unwrap();
            let mut info = rule_info_clone_light(engine.rule_info.iter().flatten().next().unwrap());
            let mut token = Token {
                fact: None,
                parent: None,
                owner_node: engine.rete.beta.root_id(),
                bindings: BindingSet::new(),
            };
            let encoding = if mixed_encoding {
                ferric_rules_core::StringEncoding::Ascii
            } else {
                ferric_rules_core::StringEncoding::Utf8
            };
            let names = if reverse { ["$?x", "x"] } else { ["x", "$?x"] };
            for (name, value) in names.into_iter().zip([first, second]) {
                let symbol = engine.symbol_table.intern_symbol(name, encoding).unwrap();
                let id = info.var_map.get_or_create(symbol).unwrap();
                token.bindings.set(id, ValueRef::new(Value::Integer(value)));
            }
            let mut multifield = ferric_rules_core::Multifield::new();
            multifield.extend(fields.into_iter().map(Value::Integer));
            let expected_tail = Value::Multifield(Box::new(multifield));
            let symbol = engine.symbol_table.intern_symbol("$?tail", encoding).unwrap();
            let id = info.var_map.get_or_create(symbol).unwrap();
            token.bindings.set(id, ValueRef::new(expected_tail.clone()));
            let symbol = engine.symbol_table.intern_symbol("unbound", encoding).unwrap();
            info.var_map.get_or_create(symbol).unwrap();

            let mut locals = RuntimeBindingEnv::from([("local".to_owned(), Value::Integer(0))]);
            if let Some(value) = local {
                locals.insert("$?x".to_owned(), Value::Integer(value));
            }
            let module = engine.module_registry.current_module();
            let mut frame = RuntimeFrame::default();
            build_runtime_eval_bindings(
                &token, &info, &locals,
                &mut ActionExecutionContext { engine: &mut engine, current_module: module },
                &mut frame,
            ).unwrap();
            let (bindings, names) = frame;
            prop_assert_eq!(names.len(), 3);
            for (name, expected) in [
                ("x", Value::Integer(local.unwrap_or(second))),
                ("tail", expected_tail),
                ("local", Value::Integer(0)),
            ] {
                let symbol = engine.symbol_table.find_symbol(name, engine.config.string_encoding).unwrap();
                let id = names.lookup(symbol).unwrap();
                prop_assert!(bindings.get(id).unwrap().structural_eq(&expected));
            }
        }
    }

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
            env.compact_facts.insert(
                "f".into(),
                CompactFactBinding::live(ferric_rules_core::FactAddress::new(
                    outer,
                    1,
                    ferric_rules_core::Timestamp::ZERO,
                    1,
                )),
            );
            insert_runtime_binding(&mut env.runtime_bindings, "f", Value::Integer(42));
            let result: Result<(), ActionError> = env.with_compact_scope(
                &[
                    (
                        "f".into(),
                        CompactFactBinding::live(ferric_rules_core::FactAddress::new(
                            inner,
                            1,
                            ferric_rules_core::Timestamp::ZERO,
                            1,
                        )),
                    ),
                    (
                        "temporary".into(),
                        CompactFactBinding::live(ferric_rules_core::FactAddress::new(
                            inner,
                            1,
                            ferric_rules_core::Timestamp::ZERO,
                            1,
                        )),
                    ),
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
                        &[(
                            "f".into(),
                            CompactFactBinding::live(ferric_rules_core::FactAddress::new(
                                nested,
                                1,
                                ferric_rules_core::Timestamp::ZERO,
                                1,
                            )),
                        )],
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
            env.with_compact_scope(
                &[(
                    name.into(),
                    CompactFactBinding::live(ferric_rules_core::FactAddress::new(
                        fact,
                        1,
                        ferric_rules_core::Timestamp::ZERO,
                        1,
                    )),
                )],
                |env| {
                    assert_eq!(env.compact_facts.len(), 1);
                    assert_eq!(
                        env.compact_facts.get(name).map(CompactFactBinding::fact_id),
                        Some(fact)
                    );
                    Ok(())
                },
            )
            .unwrap();
            assert!(env.compact_facts.is_empty());
        }
    }

    #[test]
    fn runtime_eval_bindings_share_values_and_let_locals_win() {
        let mut engine = Engine::new(crate::EngineConfig::utf8());
        let mut rule_info = CompiledRuleInfo {
            name: "binding-test".into(),
            source_definition: None,
            actions: Vec::new(),
            var_map: VarMap::new(),
            fact_address_vars: HashMap::new(),
            salience: Salience::new(0),
            auto_focus: false,
            test_conditions: Vec::new(),
            runtime_actions: Vec::new(),
            activation_layout: OnceLock::new(),
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
            ("local", ValueRef::new(Value::Integer(20))),
        ] {
            let symbol = engine
                .symbol_table
                .intern_symbol(name, engine.config.string_encoding)
                .unwrap();
            let id = rule_info.var_map.get_or_create(symbol).unwrap();
            token.bindings.set(id, value);
        }
        let runtime_bindings = HashMap::from([("local".into(), Value::Integer(30))]);
        let current_module = engine.module_registry.current_module();
        let mut frame = RuntimeFrame::default();
        build_runtime_eval_bindings(
            &token,
            &rule_info,
            &runtime_bindings,
            &mut ActionExecutionContext {
                engine: &mut engine,
                current_module,
            },
            &mut frame,
        )
        .unwrap();
        let (bindings, var_map) = frame;

        let lookup = |engine: &mut Engine, name: &str| {
            let symbol = engine
                .symbol_table
                .intern_symbol(name, engine.config.string_encoding)
                .unwrap();
            var_map.lookup(symbol).unwrap()
        };
        let local = lookup(&mut engine, "local");
        assert!(bindings
            .get(local)
            .unwrap()
            .structural_eq(&Value::Integer(30)));
        let ordinary = lookup(&mut engine, "ordinary");
        let ValueRef::Shared(merged) = bindings.get(ordinary).unwrap() else {
            panic!("ordinary multifield must retain shared storage");
        };
        assert!(Arc::ptr_eq(&shared, merged));
        // The activation frame itself is unchanged.
        assert_eq!(token.bindings.bound_count(), 2);
    }

    #[test]
    fn fact_addresses_bind_into_the_activation_token() {
        let mut engine = Engine::new(crate::EngineConfig::utf8());
        engine
            .load_str("(defrule r ?f <- (a ?x) ?g <- (b ?x) => (printout t (fact-index ?f) crlf))")
            .unwrap();
        let info = rule_info_clone_light(engine.rule_info.iter().flatten().next().unwrap());
        let mut token = Token {
            fact: None,
            bindings: BindingSet::new(),
            parent: None,
            owner_node: engine.rete.beta.root_id(),
        };
        let lookup = |engine: &Engine, name: &str| {
            let symbol = engine
                .symbol_table
                .find_symbol(name, engine.config.string_encoding)
                .unwrap();
            info.var_map.lookup(symbol).unwrap()
        };
        let (f, g) = (lookup(&engine, "f"), lookup(&engine, "g"));
        // A pattern binding of the same name keeps precedence.
        token.bindings.set(g, ValueRef::new(Value::Integer(-1)));
        let relation = engine
            .symbol_table
            .intern_symbol("a", engine.config.string_encoding)
            .unwrap();
        let fact_id = engine
            .fact_base
            .assert_ordered(relation, smallvec::smallvec![Value::Integer(1)]);
        bind_fact_addresses(
            &mut token,
            &info,
            &[fact_id, fact_id],
            &engine.symbol_table,
            engine.config.string_encoding,
            &engine.fact_base,
            engine.initial_fact_id,
            engine.fact_epoch,
            engine.fact_index_starts_at_zero,
        );
        let address = crate::fact_address::make_fact_address(
            &engine.fact_base,
            engine.initial_fact_id,
            engine.fact_epoch,
            engine.fact_index_starts_at_zero,
            fact_id,
        )
        .unwrap();
        assert!(token
            .bindings
            .get(f)
            .unwrap()
            .structural_eq(&Value::FactAddress(address)));
        assert!(token
            .bindings
            .get(g)
            .unwrap()
            .structural_eq(&Value::Integer(-1)));

        // Missing collected facts leave the variable unbound.
        let mut empty = Token {
            bindings: BindingSet::new(),
            ..token
        };
        bind_fact_addresses(
            &mut empty,
            &info,
            &[],
            &engine.symbol_table,
            engine.config.string_encoding,
            &engine.fact_base,
            engine.initial_fact_id,
            engine.fact_epoch,
            engine.fact_index_starts_at_zero,
        );
        assert_eq!(empty.bindings.bound_count(), 0);
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
            auto_focus: false,
            test_conditions: Vec::new(),
            runtime_actions: Vec::new(),
            activation_layout: OnceLock::new(),
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
        context.engine.config.begin_action_loop_budget();
        let result = execute_single_action(
            &token,
            &info,
            &action,
            Some(runtime),
            &mut context,
            &mut env,
            &[],
        );
        context.engine.config.end_action_loop_budget();
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
