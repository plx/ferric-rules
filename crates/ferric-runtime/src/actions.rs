//! RHS action execution for rule firings.

use std::collections::HashMap;

use ferric_core::binding::VarMap;
use ferric_core::token::Token;
use ferric_core::{Fact, FactBase, FactId, FerricString, ReteNetwork, SymbolTable, Value};
use ferric_parser::{Action, ActionExpr, FunctionCall, LiteralKind};

use crate::config::EngineConfig;

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
}

/// Execute actions for a fired rule.
///
/// This is called with all the data needed pre-extracted to avoid borrow issues.
/// Returns a list of action errors (non-fatal — execution continues through all actions).
pub(crate) fn execute_actions(
    fact_base: &mut FactBase,
    rete: &mut ReteNetwork,
    symbol_table: &mut SymbolTable,
    halted: &mut bool,
    config: &EngineConfig,
    token: &Token,
    rule_info: &CompiledRuleInfo,
) -> Vec<ActionError> {
    let mut errors = Vec::new();

    for action in &rule_info.actions {
        if let Err(e) = execute_single_action(
            fact_base,
            rete,
            symbol_table,
            halted,
            config,
            token,
            rule_info,
            &action.call,
        ) {
            errors.push(e);
        }
    }

    errors
}

#[allow(clippy::too_many_arguments)] // Context requires all these parameters
fn execute_single_action(
    fact_base: &mut FactBase,
    rete: &mut ReteNetwork,
    symbol_table: &mut SymbolTable,
    halted: &mut bool,
    config: &EngineConfig,
    token: &Token,
    rule_info: &CompiledRuleInfo,
    call: &FunctionCall,
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
        ),
        "duplicate" => execute_duplicate(
            fact_base,
            rete,
            symbol_table,
            config,
            token,
            rule_info,
            &call.args,
        ),
        "halt" => {
            *halted = true;
            Ok(())
        }
        "printout" => {
            // Phase 2: no-op stub for printout
            Ok(())
        }
        _ => Err(ActionError::UnknownAction(call.name.clone())),
    }
}

fn execute_assert(
    fact_base: &mut FactBase,
    rete: &mut ReteNetwork,
    symbol_table: &mut SymbolTable,
    config: &EngineConfig,
    token: &Token,
    rule_info: &CompiledRuleInfo,
    args: &[ActionExpr],
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
                    let value = eval_expr(token, rule_info, symbol_table, config, field_expr)?;
                    fields.push(value);
                }

                let fact_id = fact_base.assert_ordered(relation_sym, fields);
                let fact = fact_base.get(fact_id).unwrap().fact.clone();
                rete.assert_fact(fact_id, &fact, fact_base);
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
                if fact_base.get(fact_id).is_none() {
                    return Err(ActionError::FactNotFound(fact_id));
                }
                let fact = fact_base.get(fact_id).unwrap().fact.clone();
                rete.retract_fact(fact_id, &fact, fact_base);
                fact_base.retract(fact_id);
            }
            _ => return Err(ActionError::InvalidRetract),
        }
    }
    Ok(())
}

fn execute_modify(
    fact_base: &mut FactBase,
    rete: &mut ReteNetwork,
    symbol_table: &mut SymbolTable,
    config: &EngineConfig,
    token: &Token,
    rule_info: &CompiledRuleInfo,
    args: &[ActionExpr],
) -> Result<(), ActionError> {
    if args.is_empty() {
        return Err(ActionError::InvalidRetract);
    }

    // First arg is the fact address variable
    let fact_id = match &args[0] {
        ActionExpr::Variable(var_name, _) => resolve_fact_address(token, rule_info, var_name)?,
        _ => return Err(ActionError::InvalidRetract),
    };

    let entry = fact_base
        .get(fact_id)
        .ok_or(ActionError::FactNotFound(fact_id))?;
    let original_fact = entry.fact.clone();

    // Clone the original fact's fields for modification
    let mut fields: smallvec::SmallVec<[Value; 8]> = match &original_fact {
        Fact::Ordered(ordered) => ordered.fields.clone(),
        Fact::Template(_) => return Ok(()), // Template modify not yet supported
    };
    let relation = match &original_fact {
        Fact::Ordered(ordered) => ordered.relation,
        Fact::Template(_) => return Ok(()),
    };

    // Apply slot overrides (for ordered facts, these are positional)
    // In CLIPS, modify uses (slot-name value) syntax. For ordered facts in Phase 2,
    // we interpret FunctionCall args as positional overrides where the "name" is the index.
    // But the more common usage is with template facts, which we don't fully support yet.
    // For now, just apply any remaining arguments as additional field values if they're simple values.
    for slot_override in &args[1..] {
        if let ActionExpr::FunctionCall(fc) = slot_override {
            // Try to parse the function name as a field index for ordered fact positional override
            if let Ok(index) = fc.name.parse::<usize>() {
                if index < fields.len() {
                    if let Some(first_arg) = fc.args.first() {
                        fields[index] =
                            eval_expr(token, rule_info, symbol_table, config, first_arg)?;
                    }
                }
            }
        }
    }

    // Retract original
    rete.retract_fact(fact_id, &original_fact, fact_base);
    fact_base.retract(fact_id);

    // Assert modified
    let new_id = fact_base.assert_ordered(relation, fields);
    let new_fact = fact_base.get(new_id).unwrap().fact.clone();
    rete.assert_fact(new_id, &new_fact, fact_base);

    Ok(())
}

fn execute_duplicate(
    fact_base: &mut FactBase,
    rete: &mut ReteNetwork,
    symbol_table: &mut SymbolTable,
    config: &EngineConfig,
    token: &Token,
    rule_info: &CompiledRuleInfo,
    args: &[ActionExpr],
) -> Result<(), ActionError> {
    if args.is_empty() {
        return Err(ActionError::InvalidRetract);
    }

    // First arg is the fact address variable
    let fact_id = match &args[0] {
        ActionExpr::Variable(var_name, _) => resolve_fact_address(token, rule_info, var_name)?,
        _ => return Err(ActionError::InvalidRetract),
    };

    let entry = fact_base
        .get(fact_id)
        .ok_or(ActionError::FactNotFound(fact_id))?;
    let original_fact = entry.fact.clone();

    let mut fields: smallvec::SmallVec<[Value; 8]> = match &original_fact {
        Fact::Ordered(ordered) => ordered.fields.clone(),
        Fact::Template(_) => return Ok(()),
    };
    let relation = match &original_fact {
        Fact::Ordered(ordered) => ordered.relation,
        Fact::Template(_) => return Ok(()),
    };

    // Apply slot overrides (same as modify)
    for slot_override in &args[1..] {
        if let ActionExpr::FunctionCall(fc) = slot_override {
            if let Ok(index) = fc.name.parse::<usize>() {
                if index < fields.len() {
                    if let Some(first_arg) = fc.args.first() {
                        fields[index] =
                            eval_expr(token, rule_info, symbol_table, config, first_arg)?;
                    }
                }
            }
        }
    }

    // Assert duplicate (original stays)
    let new_id = fact_base.assert_ordered(relation, fields);
    let new_fact = fact_base.get(new_id).unwrap().fact.clone();
    rete.assert_fact(new_id, &new_fact, fact_base);

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

/// Evaluate an action expression to a Value.
fn eval_expr(
    token: &Token,
    rule_info: &CompiledRuleInfo,
    symbol_table: &mut SymbolTable,
    config: &EngineConfig,
    expr: &ActionExpr,
) -> Result<Value, ActionError> {
    match expr {
        ActionExpr::Literal(lit) => literal_to_value(symbol_table, config, &lit.value),
        ActionExpr::Variable(name, _) => {
            // Try to resolve as a bound variable
            let sym = symbol_table
                .intern_symbol(name, config.string_encoding)
                .map_err(|e| ActionError::Encoding(format!("{e}")))?;
            if let Some(var_id) = rule_info.var_map.lookup(sym) {
                token
                    .bindings
                    .get(var_id)
                    .map(|v| (**v).clone())
                    .ok_or_else(|| ActionError::UnboundVariable(name.clone()))
            } else {
                Err(ActionError::UnboundVariable(name.clone()))
            }
        }
        ActionExpr::GlobalVariable(name, _) => {
            // Global variables not supported in Phase 2
            Err(ActionError::UnboundVariable(format!("*{name}*")))
        }
        ActionExpr::FunctionCall(_) => {
            // Nested function calls not evaluated in Phase 2
            // Return Void as a placeholder
            Ok(Value::Void)
        }
    }
}

/// Convert a literal kind to a Value.
fn literal_to_value(
    symbol_table: &mut SymbolTable,
    config: &EngineConfig,
    kind: &LiteralKind,
) -> Result<Value, ActionError> {
    match kind {
        LiteralKind::Integer(n) => Ok(Value::Integer(*n)),
        LiteralKind::Float(f) => Ok(Value::Float(*f)),
        LiteralKind::String(s) => {
            let fs = FerricString::new(s, config.string_encoding)
                .map_err(|e| ActionError::Encoding(format!("{e}")))?;
            Ok(Value::String(fs))
        }
        LiteralKind::Symbol(s) => {
            let sym = symbol_table
                .intern_symbol(s, config.string_encoding)
                .map_err(|e| ActionError::Encoding(format!("{e}")))?;
            Ok(Value::Symbol(sym))
        }
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
