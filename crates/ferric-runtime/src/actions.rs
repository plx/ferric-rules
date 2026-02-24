//! RHS action execution for rule firings.
//!
//! ## Phase 3 scope
//!
//! - `GlobalVariable` reads and writes via `GlobalStore` (Pass 006).
//! - `modify`/`duplicate` support template-aware slot overrides (Pass 003).
//! - `printout` with per-channel output capture via `OutputRouter` (Pass 004).

use std::collections::{HashMap, VecDeque};
use std::fmt::Write as FmtWrite;
use std::rc::Rc;

use ferric_core::beta::{RuleId, Salience};
use ferric_core::binding::VarMap;
use ferric_core::token::Token;
use ferric_core::{
    EncodingError, Fact, FactBase, FactId, ReteNetwork, Symbol, SymbolTable, TemplateId, Value,
};
use ferric_parser::{Action, ActionExpr, FunctionCall, LiteralKind};
use slotmap::Key as _;

use crate::config::EngineConfig;
use crate::functions::{FunctionEnv, GenericRegistry, GlobalStore};
use crate::modules::ModuleRegistry;
use crate::router::OutputRouter;
use crate::templates::RegisteredTemplate;

type OrderedFields = smallvec::SmallVec<[Value; 8]>;
type ModuleLookup = HashMap<(crate::modules::ModuleId, String), crate::modules::ModuleId>;

/// Maximum iterations for action-level loops (while, loop-for-count, progn$).
const MAX_ACTION_LOOP_ITERATIONS: usize = 1_000_000;

pub(crate) struct ActionExecutionContext<'a> {
    pub fact_base: &'a mut FactBase,
    pub rete: &'a mut ReteNetwork,
    pub halted: &'a mut bool,
    pub symbol_table: &'a mut SymbolTable,
    pub config: &'a EngineConfig,
    pub template_defs: &'a HashMap<TemplateId, RegisteredTemplate>,
    /// Template name → `TemplateId` lookup, used by fact-query macros.
    pub template_ids: &'a HashMap<String, TemplateId>,
    pub router: &'a mut OutputRouter,
    pub functions: &'a FunctionEnv,
    pub globals: &'a mut GlobalStore,
    pub focus_requests: &'a mut Vec<String>,
    pub generics: &'a GenericRegistry,
    pub module_registry: &'a ModuleRegistry,
    pub current_module: crate::modules::ModuleId,
    pub function_modules: &'a ModuleLookup,
    pub global_modules: &'a ModuleLookup,
    pub generic_modules: &'a ModuleLookup,
    pub input_buffer: &'a mut VecDeque<String>,
    pub all_rule_info: &'a HashMap<RuleId, Rc<CompiledRuleInfo>>,
}

struct ActionEvalEnv<'a> {
    symbol_table: &'a mut SymbolTable,
    config: &'a EngineConfig,
    functions: &'a FunctionEnv,
    globals: &'a mut GlobalStore,
    generics: &'a GenericRegistry,
    module_registry: &'a ModuleRegistry,
    current_module: crate::modules::ModuleId,
    function_modules: &'a ModuleLookup,
    global_modules: &'a ModuleLookup,
    generic_modules: &'a ModuleLookup,
    input_buffer: &'a mut VecDeque<String>,
}

impl ActionEvalEnv<'_> {
    fn make_eval_context<'ctx>(
        &'ctx mut self,
        token: &'ctx Token,
        rule_info: &'ctx CompiledRuleInfo,
    ) -> crate::evaluator::EvalContext<'ctx> {
        crate::evaluator::EvalContext {
            bindings: &token.bindings,
            var_map: &rule_info.var_map,
            symbol_table: self.symbol_table,
            config: self.config,
            functions: self.functions,
            globals: self.globals,
            generics: self.generics,
            call_depth: 0,
            current_module: self.current_module,
            module_registry: self.module_registry,
            function_modules: self.function_modules,
            global_modules: self.global_modules,
            generic_modules: self.generic_modules,
            method_chain: None,
            input_buffer: Some(self.input_buffer),
        }
    }

    fn eval_runtime_expr(
        &mut self,
        token: &Token,
        rule_info: &CompiledRuleInfo,
        runtime_expr: &crate::evaluator::RuntimeExpr,
    ) -> Result<Value, ActionError> {
        let mut ctx = self.make_eval_context(token, rule_info);
        crate::evaluator::eval(&mut ctx, runtime_expr).map_err(ActionError::from)
    }

    fn eval_expr(
        &mut self,
        token: &Token,
        rule_info: &CompiledRuleInfo,
        expr: &ActionExpr,
    ) -> Result<Value, ActionError> {
        let runtime_expr = crate::evaluator::from_action_expr(expr, self.symbol_table, self.config)
            .map_err(ActionError::from)?;
        self.eval_runtime_expr(token, rule_info, &runtime_expr)
    }
}

/// Compiled rule metadata stored for action execution.
#[derive(Clone, Debug)]
pub(crate) struct CompiledRuleInfo {
    /// The rule name.
    #[allow(dead_code)] // May be used in future for debugging/logging
    pub name: String,
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
    /// Pre-translated test CE expressions, evaluated at firing time.
    pub test_conditions: Vec<crate::evaluator::RuntimeExpr>,
    /// Pre-translated RHS action call expressions.
    pub runtime_actions: Vec<Option<crate::evaluator::RuntimeExpr>>,
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
pub(crate) fn execute_actions(
    token: &Token,
    rule_info: &CompiledRuleInfo,
    context: &mut ActionExecutionContext<'_>,
) -> (bool, bool, bool, Vec<ActionError>) {
    let fact_base = &mut *context.fact_base;
    let rete = &mut *context.rete;
    let halted = &mut *context.halted;
    let symbol_table = &mut *context.symbol_table;
    let globals = &mut *context.globals;
    let router = &mut *context.router;
    let focus_requests = &mut *context.focus_requests;
    let input_buffer = &mut *context.input_buffer;
    let config = context.config;
    let template_defs = context.template_defs;
    let template_ids = context.template_ids;
    let functions = context.functions;
    let generics = context.generics;
    let module_registry = context.module_registry;
    let current_module = context.current_module;
    let function_modules = context.function_modules;
    let global_modules = context.global_modules;
    let generic_modules = context.generic_modules;
    let all_rule_info = context.all_rule_info;

    let mut errors = Vec::new();
    let mut reset_requested = false;
    let mut clear_requested = false;
    let mut eval_env = ActionEvalEnv {
        symbol_table,
        config,
        functions,
        globals,
        generics,
        module_registry,
        current_module,
        function_modules,
        global_modules,
        generic_modules,
        input_buffer,
    };

    // Evaluate test conditions first — if any is falsy, skip all actions and
    // signal to the caller that the rule did NOT logically fire.
    for test_expr in &rule_info.test_conditions {
        match eval_env.eval_runtime_expr(token, rule_info, test_expr) {
            Ok(value) => {
                if !crate::evaluator::is_truthy(&value, eval_env.symbol_table) {
                    return (false, false, false, errors); // Test CE falsy — rule did not fire
                }
            }
            Err(e) => {
                errors.push(e);
                return (false, false, false, errors);
            }
        }
    }

    for (index, action) in rule_info.actions.iter().enumerate() {
        let runtime_call = rule_info
            .runtime_actions
            .get(index)
            .and_then(Option::as_ref);
        if let Err(e) = execute_single_action(
            fact_base,
            rete,
            halted,
            &mut reset_requested,
            &mut clear_requested,
            token,
            rule_info,
            &action.call,
            runtime_call,
            template_defs,
            template_ids,
            router,
            focus_requests,
            all_rule_info,
            &mut eval_env,
        ) {
            errors.push(e);
        }
        // Stop executing further actions if clear/reset was requested.
        if clear_requested || reset_requested {
            break;
        }
    }

    (true, reset_requested, clear_requested, errors)
}

#[allow(clippy::too_many_arguments)] // Action dispatch needs full mutable engine/action context.
#[allow(clippy::too_many_lines)] // if-branch execution adds necessary verbosity
fn execute_single_action(
    fact_base: &mut FactBase,
    rete: &mut ReteNetwork,
    halted: &mut bool,
    reset_requested: &mut bool,
    clear_requested: &mut bool,
    token: &Token,
    rule_info: &CompiledRuleInfo,
    call: &FunctionCall,
    runtime_call: Option<&crate::evaluator::RuntimeExpr>,
    template_defs: &HashMap<TemplateId, RegisteredTemplate>,
    template_ids: &HashMap<String, TemplateId>,
    router: &mut OutputRouter,
    focus_requests: &mut Vec<String>,
    all_rule_info: &HashMap<RuleId, Rc<CompiledRuleInfo>>,
    eval_env: &mut ActionEvalEnv<'_>,
) -> Result<(), ActionError> {
    match call.name.as_str() {
        "assert" => execute_assert(fact_base, rete, token, rule_info, &call.args, eval_env),
        "retract" => execute_retract(fact_base, rete, token, rule_info, &call.args),
        "modify" => execute_modify(
            fact_base,
            rete,
            token,
            rule_info,
            &call.args,
            template_defs,
            eval_env,
        ),
        "duplicate" => execute_duplicate(
            fact_base,
            rete,
            token,
            rule_info,
            &call.args,
            template_defs,
            eval_env,
        ),
        "halt" => {
            *halted = true;
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
        "printout" => execute_printout(token, rule_info, &call.args, router, eval_env),
        "focus" => execute_focus(token, rule_info, &call.args, focus_requests, eval_env),
        "list-focus-stack" => execute_list_focus_stack(router, eval_env.module_registry),
        "agenda" => execute_agenda(rete, router, all_rule_info),
        "run" => {
            // (run) from within a rule RHS is a no-op — the engine is already running.
            // CLIPS allows this but it's unusual. We silently ignore it.
            Ok(())
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
                    let mut ctx = eval_env.make_eval_context(token, rule_info);
                    crate::evaluator::eval(&mut ctx, condition).map_err(ActionError::from)?
                };
                let branch = if crate::evaluator::is_truthy(&cond_value, eval_env.symbol_table) {
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
                        _ => {
                            // Literal/variable — evaluate as expression; not an
                            // action but valid in CLIPS (result is discarded).
                            if let Some(rt) = rt_expr {
                                let _ = eval_env.eval_runtime_expr(token, rule_info, rt);
                            } else {
                                let _ = eval_env.eval_expr(token, rule_info, action_expr);
                            }
                            continue;
                        }
                    };
                    execute_single_action(
                        fact_base,
                        rete,
                        halted,
                        reset_requested,
                        clear_requested,
                        token,
                        rule_info,
                        &branch_call,
                        branch_runtime,
                        template_defs,
                        template_ids,
                        router,
                        focus_requests,
                        all_rule_info,
                        eval_env,
                    )?;
                    // Propagate early-exit flags.
                    if *halted || *reset_requested || *clear_requested {
                        break;
                    }
                }
                Ok(())
            } else {
                // Fallback: evaluate as expression (no-op if void).
                if let Some(runtime_expr) = runtime_call {
                    eval_env
                        .eval_runtime_expr(token, rule_info, runtime_expr)
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
                        let mut ctx = eval_env.make_eval_context(token, rule_info);
                        crate::evaluator::eval(&mut ctx, condition).map_err(ActionError::from)?
                    };
                    if !crate::evaluator::is_truthy(&cond_value, eval_env.symbol_table) {
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
                        fact_base,
                        rete,
                        halted,
                        reset_requested,
                        clear_requested,
                        token,
                        rule_info,
                        body,
                        template_defs,
                        template_ids,
                        router,
                        focus_requests,
                        all_rule_info,
                        eval_env,
                    )?;
                    if *halted || *reset_requested || *clear_requested {
                        break;
                    }
                }
                Ok(())
            } else if let Some(runtime_expr) = runtime_call {
                eval_env
                    .eval_runtime_expr(token, rule_info, runtime_expr)
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
                    let mut ctx = eval_env.make_eval_context(token, rule_info);
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
                            eval_env.symbol_table,
                            eval_env.config,
                        )?
                    } else {
                        (token.clone(), rule_info_clone_light(rule_info))
                    };

                    execute_loop_body(
                        fact_base,
                        rete,
                        halted,
                        reset_requested,
                        clear_requested,
                        &loop_token,
                        &loop_rule_info,
                        body,
                        template_defs,
                        template_ids,
                        router,
                        focus_requests,
                        all_rule_info,
                        eval_env,
                    )?;
                    if *halted || *reset_requested || *clear_requested {
                        break;
                    }
                }
                let _ = span; // suppress unused variable warning
                Ok(())
            } else if let Some(runtime_expr) = runtime_call {
                eval_env
                    .eval_runtime_expr(token, rule_info, runtime_expr)
                    .map(|_| ())
                    .map_err(|e| ActionError::UnknownAction(format!("loop-for-count: {e}")))
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
                    let mut ctx = eval_env.make_eval_context(token, rule_info);
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
                        eval_env.symbol_table,
                        eval_env.config,
                    )?;
                    let (loop_token, loop_rule_info) = augment_bindings_with_var(
                        &token1,
                        &rule_info1,
                        &index_var_name,
                        Value::Integer(one_based),
                        eval_env.symbol_table,
                        eval_env.config,
                    )?;

                    execute_loop_body(
                        fact_base,
                        rete,
                        halted,
                        reset_requested,
                        clear_requested,
                        &loop_token,
                        &loop_rule_info,
                        body,
                        template_defs,
                        template_ids,
                        router,
                        focus_requests,
                        all_rule_info,
                        eval_env,
                    )?;
                    if *halted || *reset_requested || *clear_requested {
                        break;
                    }
                }
                Ok(())
            } else if let Some(runtime_expr) = runtime_call {
                eval_env
                    .eval_runtime_expr(token, rule_info, runtime_expr)
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
                    fact_base,
                    rete,
                    halted,
                    reset_requested,
                    clear_requested,
                    token,
                    rule_info,
                    name,
                    bindings,
                    query,
                    body,
                    template_defs,
                    template_ids,
                    router,
                    focus_requests,
                    all_rule_info,
                    eval_env,
                )
            } else if let Some(runtime_expr) = runtime_call {
                eval_env
                    .eval_runtime_expr(token, rule_info, runtime_expr)
                    .map(|_| ())
                    .map_err(|e| ActionError::UnknownAction(format!("{}: {e}", call.name)))
            } else {
                Ok(())
            }
        }

        // For any other call, try evaluating it as an expression (e.g., bind).
        _ => {
            let eval_result = if let Some(runtime_expr) = runtime_call {
                eval_env.eval_runtime_expr(token, rule_info, runtime_expr)
            } else {
                let action_expr = ActionExpr::FunctionCall(call.clone());
                eval_env.eval_expr(token, rule_info, &action_expr)
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
        actions: Vec::new(),
        var_map: rule_info.var_map.clone(),
        fact_address_vars: rule_info.fact_address_vars.clone(),
        salience: rule_info.salience,
        test_conditions: Vec::new(),
        runtime_actions: Vec::new(),
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
    new_token.bindings.set(var_id, std::rc::Rc::new(value));

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
    fact_base: &mut FactBase,
    rete: &mut ReteNetwork,
    halted: &mut bool,
    reset_requested: &mut bool,
    clear_requested: &mut bool,
    token: &Token,
    rule_info: &CompiledRuleInfo,
    body: &[(
        ferric_parser::ActionExpr,
        Option<Box<crate::evaluator::RuntimeExpr>>,
    )],
    template_defs: &HashMap<TemplateId, RegisteredTemplate>,
    template_ids: &HashMap<String, TemplateId>,
    router: &mut OutputRouter,
    focus_requests: &mut Vec<String>,
    all_rule_info: &HashMap<RuleId, Rc<CompiledRuleInfo>>,
    eval_env: &mut ActionEvalEnv<'_>,
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
            _ => {
                // Literal/variable — evaluate as expression; result is discarded.
                if let Some(rt) = rt_expr {
                    let _ = eval_env.eval_runtime_expr(token, rule_info, rt);
                } else {
                    let _ = eval_env.eval_expr(token, rule_info, action_expr);
                }
                continue;
            }
        };
        execute_single_action(
            fact_base,
            rete,
            halted,
            reset_requested,
            clear_requested,
            token,
            rule_info,
            &branch_call,
            branch_runtime,
            template_defs,
            template_ids,
            router,
            focus_requests,
            all_rule_info,
            eval_env,
        )?;
        if *halted || *reset_requested || *clear_requested {
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
    fact_base: &mut FactBase,
    rete: &mut ReteNetwork,
    halted: &mut bool,
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
    template_defs: &HashMap<TemplateId, RegisteredTemplate>,
    template_ids: &HashMap<String, TemplateId>,
    router: &mut OutputRouter,
    focus_requests: &mut Vec<String>,
    all_rule_info: &HashMap<RuleId, Rc<CompiledRuleInfo>>,
    eval_env: &mut ActionEvalEnv<'_>,
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
        let Some(&tid) = template_ids.get(template_name.as_str()) else {
            // Unknown template — no facts match; result is empty / FALSE.
            return Ok(());
        };
        let ids: Vec<FactId> = fact_base.facts_by_template(tid).collect();
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
                eval_env.symbol_table,
                eval_env.config,
            )?;
            aug_token = new_token;
            aug_rule_info = new_rule_info;
        }

        // Evaluate the query expression.
        let query_val = {
            let mut ctx = eval_env.make_eval_context(&aug_token, &aug_rule_info);
            crate::evaluator::eval(&mut ctx, query).map_err(ActionError::from)?
        };

        if !crate::evaluator::is_truthy(&query_val, eval_env.symbol_table) {
            continue;
        }

        if is_action_form {
            execute_loop_body(
                fact_base,
                rete,
                halted,
                reset_requested,
                clear_requested,
                &aug_token,
                &aug_rule_info,
                body,
                template_defs,
                template_ids,
                router,
                focus_requests,
                all_rule_info,
                eval_env,
            )?;
            if *halted || *reset_requested || *clear_requested {
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
    focus_requests: &mut Vec<String>,
    eval_env: &mut ActionEvalEnv<'_>,
) -> Result<(), ActionError> {
    for arg in args {
        let value = eval_env.eval_expr(token, rule_info, arg)?;
        match value {
            Value::Symbol(sym) => {
                if let Some(name) = eval_env.symbol_table.resolve_symbol_str(sym) {
                    if eval_env.module_registry.get_by_name(name).is_none() {
                        return Err(ActionError::EvalError(format!(
                            "focus: unknown module `{name}`"
                        )));
                    }
                    focus_requests.push(name.to_string());
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
    all_rule_info: &HashMap<RuleId, Rc<CompiledRuleInfo>>,
) -> Result<(), ActionError> {
    let mut output = String::new();
    for activation in rete.agenda.iter_activations() {
        let rule_name = all_rule_info
            .get(&activation.rule)
            .map_or("???", |info| info.name.as_str());
        let _ = writeln!(output, "{} {rule_name}", activation.salience.get());
    }
    if output.is_empty() {
        output.push_str("(no activations)\n");
    }
    router.write("t", &output);
    Ok(())
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
    router: &mut OutputRouter,
    eval_env: &mut ActionEvalEnv<'_>,
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
        let value = eval_env.eval_expr(token, rule_info, arg)?;
        format_printout_value(&value, eval_env.symbol_table, &mut output);
    }

    router.write(&channel, &output);
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
    fact_base: &mut FactBase,
    rete: &mut ReteNetwork,
    token: &Token,
    rule_info: &CompiledRuleInfo,
    args: &[ActionExpr],
    eval_env: &mut ActionEvalEnv<'_>,
) -> Result<(), ActionError> {
    // Each argument to assert should be a "function call" representing a fact pattern
    // e.g., (assert (relation val1 val2)) → args = [FunctionCall("relation", [val1, val2])]
    for arg in args {
        match arg {
            ActionExpr::FunctionCall(fact_pattern) => {
                let relation = &fact_pattern.name;
                let relation_sym = eval_env
                    .symbol_table
                    .intern_symbol(relation, eval_env.config.string_encoding)
                    .map_err(ActionError::from)?;

                let mut fields = smallvec::SmallVec::new();
                for field_expr in &fact_pattern.args {
                    let value = eval_env.eval_expr(token, rule_info, field_expr)?;
                    fields.push(value);
                }

                assert_ordered_and_propagate(fact_base, rete, relation_sym, fields);
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
    fact_base: &mut FactBase,
    rete: &mut ReteNetwork,
    token: &Token,
    rule_info: &CompiledRuleInfo,
    args: &[ActionExpr],
    template_defs: &HashMap<TemplateId, RegisteredTemplate>,
    eval_env: &mut ActionEvalEnv<'_>,
) -> Result<(), ActionError> {
    execute_fact_mutation(
        fact_base,
        rete,
        token,
        rule_info,
        args,
        template_defs,
        FactMutationMode::Modify,
        eval_env,
    )
}

#[allow(clippy::too_many_arguments)] // Context requires all these parameters
fn execute_duplicate(
    fact_base: &mut FactBase,
    rete: &mut ReteNetwork,
    token: &Token,
    rule_info: &CompiledRuleInfo,
    args: &[ActionExpr],
    template_defs: &HashMap<TemplateId, RegisteredTemplate>,
    eval_env: &mut ActionEvalEnv<'_>,
) -> Result<(), ActionError> {
    execute_fact_mutation(
        fact_base,
        rete,
        token,
        rule_info,
        args,
        template_defs,
        FactMutationMode::Duplicate,
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
    fact_base: &mut FactBase,
    rete: &mut ReteNetwork,
    token: &Token,
    rule_info: &CompiledRuleInfo,
    args: &[ActionExpr],
    template_defs: &HashMap<TemplateId, RegisteredTemplate>,
    mode: FactMutationMode,
    eval_env: &mut ActionEvalEnv<'_>,
) -> Result<(), ActionError> {
    let fact_id = resolve_target_fact_id(args, token, rule_info)?;
    let original_fact = get_fact_or_error(fact_base, fact_id)?;

    match &original_fact {
        Fact::Ordered(ordered) => {
            let relation = ordered.relation;
            let mut fields = ordered.fields.clone();
            apply_ordered_slot_overrides(&mut fields, &args[1..], token, rule_info, eval_env)?;
            if mode.retract_original() {
                retract_original_fact(fact_base, rete, fact_id, &original_fact);
            }
            assert_ordered_and_propagate(fact_base, rete, relation, fields);
        }
        Fact::Template(template) => {
            let registered = template_defs.get(&template.template_id).ok_or_else(|| {
                ActionError::UnknownAction(format!(
                    "template ID {:?} not found in registry",
                    template.template_id
                ))
            })?;
            let mut slots = template.slots.to_vec();
            apply_template_slot_overrides(
                &mut slots,
                &args[1..],
                registered,
                token,
                rule_info,
                eval_env,
            )?;
            if mode.retract_original() {
                retract_original_fact(fact_base, rete, fact_id, &original_fact);
            }
            assert_template_and_propagate(
                fact_base,
                rete,
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
    let fact = fact_base
        .get(fact_id)
        .expect("asserted fact should be present in fact base")
        .fact
        .clone();
    rete.assert_fact(fact_id, &fact, fact_base);
    fact_id
}

fn assert_template_and_propagate(
    fact_base: &mut FactBase,
    rete: &mut ReteNetwork,
    template_id: TemplateId,
    slots: Box<[Value]>,
) -> FactId {
    let fact_id = fact_base.assert_template(template_id, slots);
    let fact = fact_base
        .get(fact_id)
        .expect("asserted fact should be present in fact base")
        .fact
        .clone();
    rete.assert_fact(fact_id, &fact, fact_base);
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
    eval_env: &mut ActionEvalEnv<'_>,
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
            fields[index] = eval_env.eval_expr(token, rule_info, first_arg)?;
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
    eval_env: &mut ActionEvalEnv<'_>,
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
            slots[slot_idx] = eval_env.eval_expr(token, rule_info, first_arg)?;
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
