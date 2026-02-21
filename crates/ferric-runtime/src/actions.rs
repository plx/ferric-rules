//! RHS action execution for rule firings.
//!
//! ## Phase 3 scope
//!
//! - `GlobalVariable` reads and writes via `GlobalStore` (Pass 006).
//! - `modify`/`duplicate` support template-aware slot overrides (Pass 003).
//! - `printout` with per-channel output capture via `OutputRouter` (Pass 004).

use std::collections::{HashMap, VecDeque};
use std::fmt::Write as FmtWrite;

use ferric_core::beta::RuleId;
use ferric_core::binding::VarMap;
use ferric_core::token::Token;
use ferric_core::{Fact, FactBase, FactId, ReteNetwork, Symbol, SymbolTable, TemplateId, Value};
use ferric_parser::{Action, ActionExpr, FunctionCall, LiteralKind};

use crate::config::EngineConfig;
use crate::functions::{FunctionEnv, GenericRegistry, GlobalStore};
use crate::modules::ModuleRegistry;
use crate::router::OutputRouter;
use crate::templates::RegisteredTemplate;

type OrderedFields = smallvec::SmallVec<[Value; 8]>;

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
    pub salience: i32,
    /// Pre-translated test CE expressions, evaluated at firing time.
    pub test_conditions: Vec<crate::evaluator::RuntimeExpr>,
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
    Encoding(String),
    #[error("expression evaluation error: {0}")]
    EvalError(String),
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
#[allow(clippy::too_many_arguments)] // Context requires all these parameters
pub(crate) fn execute_actions(
    fact_base: &mut FactBase,
    rete: &mut ReteNetwork,
    symbol_table: &mut SymbolTable,
    halted: &mut bool,
    config: &EngineConfig,
    token: &Token,
    rule_info: &CompiledRuleInfo,
    template_defs: &HashMap<TemplateId, RegisteredTemplate>,
    router: &mut OutputRouter,
    functions: &FunctionEnv,
    globals: &mut GlobalStore,
    focus_requests: &mut Vec<String>,
    generics: &GenericRegistry,
    module_registry: &ModuleRegistry,
    current_module: crate::modules::ModuleId,
    function_modules: &HashMap<(crate::modules::ModuleId, String), crate::modules::ModuleId>,
    global_modules: &HashMap<(crate::modules::ModuleId, String), crate::modules::ModuleId>,
    generic_modules: &HashMap<(crate::modules::ModuleId, String), crate::modules::ModuleId>,
    input_buffer: &mut VecDeque<String>,
    all_rule_info: &HashMap<RuleId, CompiledRuleInfo>,
) -> (bool, bool, bool, Vec<ActionError>) {
    let mut errors = Vec::new();
    let mut reset_requested = false;
    let mut clear_requested = false;

    // Evaluate test conditions first — if any is falsy, skip all actions and
    // signal to the caller that the rule did NOT logically fire.
    for test_expr in &rule_info.test_conditions {
        let mut ctx = crate::evaluator::EvalContext {
            bindings: &token.bindings,
            var_map: &rule_info.var_map,
            symbol_table,
            config,
            functions,
            globals,
            generics,
            call_depth: 0,
            current_module,
            module_registry,
            function_modules,
            global_modules,
            generic_modules,
            method_chain: None,
            input_buffer: Some(input_buffer),
        };
        match crate::evaluator::eval(&mut ctx, test_expr) {
            Ok(value) => {
                if !crate::evaluator::is_truthy(&value, ctx.symbol_table) {
                    return (false, false, false, errors); // Test CE falsy — rule did not fire
                }
            }
            Err(e) => {
                errors.push(ActionError::EvalError(format!("{e}")));
                return (false, false, false, errors);
            }
        }
    }

    for action in &rule_info.actions {
        if let Err(e) = execute_single_action(
            fact_base,
            rete,
            symbol_table,
            halted,
            &mut reset_requested,
            &mut clear_requested,
            config,
            token,
            rule_info,
            &action.call,
            template_defs,
            router,
            functions,
            globals,
            focus_requests,
            generics,
            module_registry,
            current_module,
            function_modules,
            global_modules,
            generic_modules,
            input_buffer,
            all_rule_info,
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

#[allow(clippy::too_many_arguments, clippy::too_many_lines)] // Context requires all these parameters
fn execute_single_action(
    fact_base: &mut FactBase,
    rete: &mut ReteNetwork,
    symbol_table: &mut SymbolTable,
    halted: &mut bool,
    reset_requested: &mut bool,
    clear_requested: &mut bool,
    config: &EngineConfig,
    token: &Token,
    rule_info: &CompiledRuleInfo,
    call: &FunctionCall,
    template_defs: &HashMap<TemplateId, RegisteredTemplate>,
    router: &mut OutputRouter,
    functions: &FunctionEnv,
    globals: &mut GlobalStore,
    focus_requests: &mut Vec<String>,
    generics: &GenericRegistry,
    module_registry: &ModuleRegistry,
    current_module: crate::modules::ModuleId,
    function_modules: &HashMap<(crate::modules::ModuleId, String), crate::modules::ModuleId>,
    global_modules: &HashMap<(crate::modules::ModuleId, String), crate::modules::ModuleId>,
    generic_modules: &HashMap<(crate::modules::ModuleId, String), crate::modules::ModuleId>,
    input_buffer: &mut VecDeque<String>,
    all_rule_info: &HashMap<RuleId, CompiledRuleInfo>,
) -> Result<(), ActionError> {
    match call.name.as_str() {
        "assert" => execute_assert(
            fact_base,
            rete,
            symbol_table,
            config,
            token,
            rule_info,
            &call.args,
            functions,
            globals,
            generics,
            current_module,
            function_modules,
            global_modules,
            generic_modules,
            module_registry,
            input_buffer,
        ),
        "retract" => execute_retract(fact_base, rete, token, rule_info, &call.args),
        "modify" => execute_modify(
            fact_base,
            rete,
            symbol_table,
            config,
            token,
            rule_info,
            &call.args,
            template_defs,
            functions,
            globals,
            generics,
            current_module,
            function_modules,
            global_modules,
            generic_modules,
            module_registry,
            input_buffer,
        ),
        "duplicate" => execute_duplicate(
            fact_base,
            rete,
            symbol_table,
            config,
            token,
            rule_info,
            &call.args,
            template_defs,
            functions,
            globals,
            generics,
            current_module,
            function_modules,
            global_modules,
            generic_modules,
            module_registry,
            input_buffer,
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
        "printout" => execute_printout(
            symbol_table,
            config,
            token,
            rule_info,
            &call.args,
            router,
            functions,
            globals,
            generics,
            current_module,
            function_modules,
            global_modules,
            generic_modules,
            module_registry,
            input_buffer,
        ),
        "focus" => execute_focus(
            symbol_table,
            config,
            token,
            rule_info,
            &call.args,
            focus_requests,
            functions,
            globals,
            generics,
            module_registry,
            current_module,
            function_modules,
            global_modules,
            generic_modules,
            input_buffer,
        ),
        "list-focus-stack" => execute_list_focus_stack(router, module_registry),
        "agenda" => execute_agenda(rete, router, all_rule_info),
        "run" => {
            // (run) from within a rule RHS is a no-op — the engine is already running.
            // CLIPS allows this but it's unusual. We silently ignore it.
            Ok(())
        }
        // For any other call, try evaluating it as an expression (e.g., bind).
        _ => {
            let action_expr = ActionExpr::FunctionCall(call.clone());
            eval_expr(
                token,
                rule_info,
                symbol_table,
                config,
                &action_expr,
                functions,
                globals,
                generics,
                current_module,
                function_modules,
                global_modules,
                generic_modules,
                module_registry,
                input_buffer,
            )
            .map(|_| ())
            .map_err(|e| ActionError::UnknownAction(format!("{}: {e}", call.name)))
        }
    }
}

/// Execute a `focus` action: push module(s) onto the focus stack.
///
/// Arguments are evaluated to symbols and collected as focus requests.
/// They are applied by the engine after all actions complete, in reverse
/// order so the first argument becomes the top of the focus stack.
#[allow(clippy::too_many_arguments)]
fn execute_focus(
    symbol_table: &mut SymbolTable,
    config: &EngineConfig,
    token: &Token,
    rule_info: &CompiledRuleInfo,
    args: &[ActionExpr],
    focus_requests: &mut Vec<String>,
    functions: &FunctionEnv,
    globals: &mut GlobalStore,
    generics: &GenericRegistry,
    module_registry: &ModuleRegistry,
    current_module: crate::modules::ModuleId,
    function_modules: &HashMap<(crate::modules::ModuleId, String), crate::modules::ModuleId>,
    global_modules: &HashMap<(crate::modules::ModuleId, String), crate::modules::ModuleId>,
    generic_modules: &HashMap<(crate::modules::ModuleId, String), crate::modules::ModuleId>,
    input_buffer: &mut VecDeque<String>,
) -> Result<(), ActionError> {
    for arg in args {
        let value = eval_expr(
            token,
            rule_info,
            symbol_table,
            config,
            arg,
            functions,
            globals,
            generics,
            current_module,
            function_modules,
            global_modules,
            generic_modules,
            module_registry,
            input_buffer,
        )?;
        match value {
            Value::Symbol(sym) => {
                if let Some(name) = symbol_table.resolve_symbol_str(sym) {
                    if module_registry.get_by_name(name).is_none() {
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
    all_rule_info: &HashMap<RuleId, CompiledRuleInfo>,
) -> Result<(), ActionError> {
    let mut output = String::new();
    for activation in rete.agenda.iter_activations() {
        let rule_name = all_rule_info
            .get(&activation.rule)
            .map_or("???", |info| info.name.as_str());
        let _ = writeln!(output, "{} {rule_name}", activation.salience);
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
    symbol_table: &mut SymbolTable,
    config: &EngineConfig,
    token: &Token,
    rule_info: &CompiledRuleInfo,
    args: &[ActionExpr],
    router: &mut OutputRouter,
    functions: &FunctionEnv,
    globals: &mut GlobalStore,
    generics: &GenericRegistry,
    current_module: crate::modules::ModuleId,
    function_modules: &HashMap<(crate::modules::ModuleId, String), crate::modules::ModuleId>,
    global_modules: &HashMap<(crate::modules::ModuleId, String), crate::modules::ModuleId>,
    generic_modules: &HashMap<(crate::modules::ModuleId, String), crate::modules::ModuleId>,
    module_registry: &ModuleRegistry,
    input_buffer: &mut VecDeque<String>,
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
        let value = eval_expr(
            token,
            rule_info,
            symbol_table,
            config,
            arg,
            functions,
            globals,
            generics,
            current_module,
            function_modules,
            global_modules,
            generic_modules,
            module_registry,
            input_buffer,
        )?;
        format_printout_value(&value, symbol_table, &mut output);
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
    symbol_table: &mut SymbolTable,
    config: &EngineConfig,
    token: &Token,
    rule_info: &CompiledRuleInfo,
    args: &[ActionExpr],
    functions: &FunctionEnv,
    globals: &mut GlobalStore,
    generics: &GenericRegistry,
    current_module: crate::modules::ModuleId,
    function_modules: &HashMap<(crate::modules::ModuleId, String), crate::modules::ModuleId>,
    global_modules: &HashMap<(crate::modules::ModuleId, String), crate::modules::ModuleId>,
    generic_modules: &HashMap<(crate::modules::ModuleId, String), crate::modules::ModuleId>,
    module_registry: &ModuleRegistry,
    input_buffer: &mut VecDeque<String>,
) -> Result<(), ActionError> {
    // Each argument to assert should be a "function call" representing a fact pattern
    // e.g., (assert (relation val1 val2)) → args = [FunctionCall("relation", [val1, val2])]
    for arg in args {
        match arg {
            ActionExpr::FunctionCall(fact_pattern) => {
                let relation = &fact_pattern.name;
                let relation_sym = symbol_table
                    .intern_symbol(relation, config.string_encoding)
                    .map_err(|e| ActionError::Encoding(format!("{e}")))?;

                let mut fields = smallvec::SmallVec::new();
                for field_expr in &fact_pattern.args {
                    let value = eval_expr(
                        token,
                        rule_info,
                        symbol_table,
                        config,
                        field_expr,
                        functions,
                        globals,
                        generics,
                        current_module,
                        function_modules,
                        global_modules,
                        generic_modules,
                        module_registry,
                        input_buffer,
                    )?;
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
    symbol_table: &mut SymbolTable,
    config: &EngineConfig,
    token: &Token,
    rule_info: &CompiledRuleInfo,
    args: &[ActionExpr],
    template_defs: &HashMap<TemplateId, RegisteredTemplate>,
    functions: &FunctionEnv,
    globals: &mut GlobalStore,
    generics: &GenericRegistry,
    current_module: crate::modules::ModuleId,
    function_modules: &HashMap<(crate::modules::ModuleId, String), crate::modules::ModuleId>,
    global_modules: &HashMap<(crate::modules::ModuleId, String), crate::modules::ModuleId>,
    generic_modules: &HashMap<(crate::modules::ModuleId, String), crate::modules::ModuleId>,
    module_registry: &ModuleRegistry,
    input_buffer: &mut VecDeque<String>,
) -> Result<(), ActionError> {
    execute_fact_mutation(
        fact_base,
        rete,
        symbol_table,
        config,
        token,
        rule_info,
        args,
        template_defs,
        functions,
        globals,
        generics,
        FactMutationMode::Modify,
        current_module,
        function_modules,
        global_modules,
        generic_modules,
        module_registry,
        input_buffer,
    )
}

#[allow(clippy::too_many_arguments)] // Context requires all these parameters
fn execute_duplicate(
    fact_base: &mut FactBase,
    rete: &mut ReteNetwork,
    symbol_table: &mut SymbolTable,
    config: &EngineConfig,
    token: &Token,
    rule_info: &CompiledRuleInfo,
    args: &[ActionExpr],
    template_defs: &HashMap<TemplateId, RegisteredTemplate>,
    functions: &FunctionEnv,
    globals: &mut GlobalStore,
    generics: &GenericRegistry,
    current_module: crate::modules::ModuleId,
    function_modules: &HashMap<(crate::modules::ModuleId, String), crate::modules::ModuleId>,
    global_modules: &HashMap<(crate::modules::ModuleId, String), crate::modules::ModuleId>,
    generic_modules: &HashMap<(crate::modules::ModuleId, String), crate::modules::ModuleId>,
    module_registry: &ModuleRegistry,
    input_buffer: &mut VecDeque<String>,
) -> Result<(), ActionError> {
    execute_fact_mutation(
        fact_base,
        rete,
        symbol_table,
        config,
        token,
        rule_info,
        args,
        template_defs,
        functions,
        globals,
        generics,
        FactMutationMode::Duplicate,
        current_module,
        function_modules,
        global_modules,
        generic_modules,
        module_registry,
        input_buffer,
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
    symbol_table: &mut SymbolTable,
    config: &EngineConfig,
    token: &Token,
    rule_info: &CompiledRuleInfo,
    args: &[ActionExpr],
    template_defs: &HashMap<TemplateId, RegisteredTemplate>,
    functions: &FunctionEnv,
    globals: &mut GlobalStore,
    generics: &GenericRegistry,
    mode: FactMutationMode,
    current_module: crate::modules::ModuleId,
    function_modules: &HashMap<(crate::modules::ModuleId, String), crate::modules::ModuleId>,
    global_modules: &HashMap<(crate::modules::ModuleId, String), crate::modules::ModuleId>,
    generic_modules: &HashMap<(crate::modules::ModuleId, String), crate::modules::ModuleId>,
    module_registry: &ModuleRegistry,
    input_buffer: &mut VecDeque<String>,
) -> Result<(), ActionError> {
    let fact_id = resolve_target_fact_id(args, token, rule_info)?;
    let original_fact = get_fact_or_error(fact_base, fact_id)?;

    match &original_fact {
        Fact::Ordered(ordered) => {
            let relation = ordered.relation;
            let mut fields = ordered.fields.clone();
            apply_ordered_slot_overrides(
                &mut fields,
                &args[1..],
                token,
                rule_info,
                symbol_table,
                config,
                functions,
                globals,
                generics,
                current_module,
                function_modules,
                global_modules,
                generic_modules,
                module_registry,
                input_buffer,
            )?;
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
                symbol_table,
                config,
                functions,
                globals,
                generics,
                current_module,
                function_modules,
                global_modules,
                generic_modules,
                module_registry,
                input_buffer,
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
    symbol_table: &mut SymbolTable,
    config: &EngineConfig,
    functions: &FunctionEnv,
    globals: &mut GlobalStore,
    generics: &GenericRegistry,
    current_module: crate::modules::ModuleId,
    function_modules: &HashMap<(crate::modules::ModuleId, String), crate::modules::ModuleId>,
    global_modules: &HashMap<(crate::modules::ModuleId, String), crate::modules::ModuleId>,
    generic_modules: &HashMap<(crate::modules::ModuleId, String), crate::modules::ModuleId>,
    module_registry: &ModuleRegistry,
    input_buffer: &mut VecDeque<String>,
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
            fields[index] = eval_expr(
                token,
                rule_info,
                symbol_table,
                config,
                first_arg,
                functions,
                globals,
                generics,
                current_module,
                function_modules,
                global_modules,
                generic_modules,
                module_registry,
                input_buffer,
            )?;
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
    symbol_table: &mut SymbolTable,
    config: &EngineConfig,
    functions: &FunctionEnv,
    globals: &mut GlobalStore,
    generics: &GenericRegistry,
    current_module: crate::modules::ModuleId,
    function_modules: &HashMap<(crate::modules::ModuleId, String), crate::modules::ModuleId>,
    global_modules: &HashMap<(crate::modules::ModuleId, String), crate::modules::ModuleId>,
    generic_modules: &HashMap<(crate::modules::ModuleId, String), crate::modules::ModuleId>,
    module_registry: &ModuleRegistry,
    input_buffer: &mut VecDeque<String>,
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
            slots[slot_idx] = eval_expr(
                token,
                rule_info,
                symbol_table,
                config,
                first_arg,
                functions,
                globals,
                generics,
                current_module,
                function_modules,
                global_modules,
                generic_modules,
                module_registry,
                input_buffer,
            )?;
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

/// Evaluate an action expression to a Value via the evaluator pipeline.
///
/// Translates the `ActionExpr` to a `RuntimeExpr` first (which only requires
/// `&mut SymbolTable`), then evaluates it with a freshly constructed
/// `EvalContext`.  This two-step approach avoids the double-borrow that would
/// arise if we tried to hold an `EvalContext` (which mutably borrows
/// `symbol_table`) while also passing `symbol_table` to `from_action_expr`.
#[allow(clippy::too_many_arguments)] // Context requires all these parameters
fn eval_expr(
    token: &Token,
    rule_info: &CompiledRuleInfo,
    symbol_table: &mut SymbolTable,
    config: &EngineConfig,
    expr: &ActionExpr,
    functions: &FunctionEnv,
    globals: &mut GlobalStore,
    generics: &GenericRegistry,
    current_module: crate::modules::ModuleId,
    function_modules: &HashMap<(crate::modules::ModuleId, String), crate::modules::ModuleId>,
    global_modules: &HashMap<(crate::modules::ModuleId, String), crate::modules::ModuleId>,
    generic_modules: &HashMap<(crate::modules::ModuleId, String), crate::modules::ModuleId>,
    module_registry: &ModuleRegistry,
    input_buffer: &mut VecDeque<String>,
) -> Result<Value, ActionError> {
    let runtime_expr = crate::evaluator::from_action_expr(expr, symbol_table, config)
        .map_err(|e| ActionError::EvalError(format!("{e}")))?;
    let mut ctx = crate::evaluator::EvalContext {
        bindings: &token.bindings,
        var_map: &rule_info.var_map,
        symbol_table,
        config,
        functions,
        globals,
        generics,
        call_depth: 0,
        current_module,
        module_registry,
        function_modules,
        global_modules,
        generic_modules,
        method_chain: None,
        input_buffer: Some(input_buffer),
    };
    crate::evaluator::eval(&mut ctx, &runtime_expr)
        .map_err(|e| ActionError::EvalError(format!("{e}")))
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
