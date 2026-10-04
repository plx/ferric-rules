//! Source checks for lexical iterator bindings and loop control.

use ferric_rules_parser::{Action, ActionExpr, FunctionCall, Span};

type ValidationResult = Result<(), (Span, String)>;

/// Validate break placement in callable bodies and standalone expressions.
pub(crate) fn validate_breaks(body: &[ActionExpr]) -> ValidationResult {
    validate_breaks_with_templates(body, &|_| false)
}

/// Fact and slot heads are data; the owning module determines their shape.
pub(crate) fn validate_breaks_with_templates(
    body: &[ActionExpr],
    is_template: &dyn Fn(&str) -> bool,
) -> ValidationResult {
    validate_break_body(body, false, is_template)
}

/// Rule actions wrap structured forms in synthetic function calls.
pub(crate) fn validate_action_breaks_with_templates(
    actions: &[Action],
    is_template: &dyn Fn(&str) -> bool,
) -> ValidationResult {
    for action in actions {
        validate_break_call(&action.call, false, is_template)?;
    }
    Ok(())
}

fn validate_break_call(
    call: &FunctionCall,
    allowed: bool,
    is_template: &dyn Fn(&str) -> bool,
) -> ValidationResult {
    if call.name == "break" {
        if !allowed {
            return Err((
                call.span,
                "[PRCDRPSR2] The break function not valid in this context.".to_string(),
            ));
        }
        if !call.args.is_empty() {
            return Err((
                call.span,
                "[ARGACCES4] Function break expected exactly 0 arguments.".to_string(),
            ));
        }
    }
    match call.name.as_str() {
        "assert" => {
            for argument in &call.args {
                if let ActionExpr::FunctionCall(fact) = argument {
                    if is_template(&fact.name) {
                        validate_break_slots(&fact.args, is_template)?;
                    } else {
                        validate_break_body(&fact.args, false, is_template)?;
                    }
                } else {
                    validate_break_expression(argument, false, is_template)?;
                }
            }
            Ok(())
        }
        "modify" | "duplicate" => {
            if let Some((target, slots)) = call.args.split_first() {
                validate_break_expression(target, false, is_template)?;
                validate_break_slots(slots, is_template)?;
            }
            Ok(())
        }
        // A break is an action, never an ordinary call operand. A structured
        // loop inside an operand can establish its own valid body scope.
        _ => validate_break_body(&call.args, false, is_template),
    }
}

fn validate_break_slots(
    slots: &[ActionExpr],
    is_template: &dyn Fn(&str) -> bool,
) -> ValidationResult {
    for slot in slots {
        if let ActionExpr::FunctionCall(slot) = slot {
            validate_break_body(&slot.args, false, is_template)?;
        } else {
            validate_break_expression(slot, false, is_template)?;
        }
    }
    Ok(())
}

fn validate_break_body(
    body: &[ActionExpr],
    allowed: bool,
    is_template: &dyn Fn(&str) -> bool,
) -> ValidationResult {
    for expression in body {
        validate_break_expression(expression, allowed, is_template)?;
    }
    Ok(())
}

fn validate_break_expression(
    expression: &ActionExpr,
    allowed: bool,
    is_template: &dyn Fn(&str) -> bool,
) -> ValidationResult {
    match expression {
        ActionExpr::FunctionCall(call) => validate_break_call(call, allowed, is_template),
        ActionExpr::If {
            condition,
            then_actions,
            else_actions,
            ..
        } => {
            validate_break_expression(condition, false, is_template)?;
            validate_break_body(then_actions, allowed, is_template)?;
            validate_break_body(else_actions, allowed, is_template)
        }
        ActionExpr::While {
            condition, body, ..
        } => {
            validate_break_expression(condition, false, is_template)?;
            validate_break_body(body, true, is_template)
        }
        ActionExpr::LoopForCount {
            start, end, body, ..
        } => {
            validate_break_expression(start, false, is_template)?;
            validate_break_expression(end, false, is_template)?;
            validate_break_body(body, true, is_template)
        }
        ActionExpr::Progn {
            list_expr, body, ..
        } => {
            validate_break_expression(list_expr, false, is_template)?;
            validate_break_body(body, true, is_template)
        }
        ActionExpr::QueryAction {
            name, query, body, ..
        } => {
            validate_break_expression(query, false, is_template)?;
            validate_break_body(
                body,
                matches!(
                    name.as_str(),
                    "do-for-fact" | "do-for-all-facts" | "delayed-do-for-all-facts"
                ),
                is_template,
            )
        }
        ActionExpr::Switch {
            expr,
            cases,
            default,
            ..
        } => {
            validate_break_expression(expr, false, is_template)?;
            for (value, body) in cases {
                validate_break_expression(value, false, is_template)?;
                validate_break_body(body, allowed, is_template)?;
            }
            if let Some(body) = default {
                validate_break_body(body, allowed, is_template)?;
            }
            Ok(())
        }
        ActionExpr::Literal(..) | ActionExpr::Variable(..) | ActionExpr::GlobalVariable(..) => {
            Ok(())
        }
    }
}

pub(crate) fn validate_iterator_binds(body: &[ActionExpr]) -> ValidationResult {
    let mut protected = Vec::new();
    for expression in body {
        validate(expression, &mut protected)?;
    }
    Ok(())
}

fn binding_name(name: &str) -> &str {
    name.strip_prefix("$?").unwrap_or(name)
}

fn validate_body(
    body: &[ActionExpr],
    protected: &mut Vec<(String, &'static str)>,
) -> ValidationResult {
    for expression in body {
        validate(expression, protected)?;
    }
    Ok(())
}

fn validate_iteration(
    body: &[ActionExpr],
    name: Option<&str>,
    diagnostic: &'static str,
    protected: &mut Vec<(String, &'static str)>,
) -> ValidationResult {
    let outer_len = protected.len();
    if let Some(name) = name {
        protected.push((binding_name(name).to_string(), diagnostic));
    }
    let result = validate_body(body, protected);
    protected.truncate(outer_len);
    result
}

fn validate(
    expression: &ActionExpr,
    protected: &mut Vec<(String, &'static str)>,
) -> ValidationResult {
    match expression {
        ActionExpr::FunctionCall(call) => {
            if call.name == "bind" {
                if let Some(ActionExpr::Variable(name, span)) = call.args.first() {
                    if let Some((_, diagnostic)) = protected
                        .iter()
                        .rev()
                        .find(|(iterator, _)| iterator == binding_name(name))
                    {
                        return Err((
                            *span,
                            format!("[{diagnostic}] cannot rebind iteration variable ?{name}"),
                        ));
                    }
                }
            }
            validate_body(&call.args, protected)
        }
        ActionExpr::If {
            condition,
            then_actions,
            else_actions,
            ..
        } => {
            validate(condition, protected)?;
            validate_body(then_actions, protected)?;
            validate_body(else_actions, protected)
        }
        ActionExpr::While {
            condition, body, ..
        } => {
            validate(condition, protected)?;
            validate_body(body, protected)
        }
        ActionExpr::LoopForCount {
            var_name,
            start,
            end,
            body,
            ..
        } => {
            validate(start, protected)?;
            validate(end, protected)?;
            validate_iteration(body, var_name.as_deref(), "PRCDRPSR1", protected)
        }
        ActionExpr::Progn {
            var_name,
            list_expr,
            body,
            ..
        } => {
            validate(list_expr, protected)?;
            // The generated -index name is an ordinary writable local. Only
            // the actual element variable is protected by CLIPS syntax.
            validate_iteration(body, Some(var_name), "MULTIFUN2", protected)
        }
        ActionExpr::QueryAction { query, body, .. } => {
            validate(query, protected)?;
            validate_body(body, protected)
        }
        ActionExpr::Switch {
            expr,
            cases,
            default,
            ..
        } => {
            validate(expr, protected)?;
            for (value, body) in cases {
                validate(value, protected)?;
                validate_body(body, protected)?;
            }
            if let Some(body) = default {
                validate_body(body, protected)?;
            }
            Ok(())
        }
        ActionExpr::Literal(..) | ActionExpr::Variable(..) | ActionExpr::GlobalVariable(..) => {
            Ok(())
        }
    }
}
