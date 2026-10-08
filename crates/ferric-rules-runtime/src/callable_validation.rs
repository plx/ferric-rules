//! Source checks for lexical iterator bindings and loop control.

use ferric_rules_parser::{Action, ActionExpr, FunctionCall, Span};

type ValidationResult = Result<(), (Span, String)>;

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
        "progn" => validate_break_body(&call.args, allowed, is_template),
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
            name,
            bindings,
            query,
            body,
            ..
        } => {
            for expression in bindings.iter().flat_map(|binding| &binding.restrictions) {
                validate_break_expression(expression, false, is_template)?;
            }
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

pub(crate) fn validate_iterator_binds_with_templates(
    body: &[ActionExpr],
    is_template: &dyn Fn(&str) -> bool,
) -> ValidationResult {
    validate_body(body, &mut Vec::new(), is_template)
}

fn binding_name(name: &str) -> &str {
    name.strip_prefix("$?").unwrap_or(name)
}

fn validate_body(
    body: &[ActionExpr],
    protected: &mut Vec<(String, &'static str)>,
    is_template: &dyn Fn(&str) -> bool,
) -> ValidationResult {
    for expression in body {
        validate(expression, protected, is_template)?;
    }
    Ok(())
}

fn validate_iteration(
    body: &[ActionExpr],
    name: Option<&str>,
    diagnostic: &'static str,
    protected: &mut Vec<(String, &'static str)>,
    is_template: &dyn Fn(&str) -> bool,
) -> ValidationResult {
    let outer_len = protected.len();
    if let Some(name) = name {
        protected.push((binding_name(name).to_string(), diagnostic));
    }
    let result = validate_body(body, protected, is_template);
    protected.truncate(outer_len);
    result
}

fn validate_binding_slots(
    slots: &[ActionExpr],
    protected: &mut Vec<(String, &'static str)>,
    is_template: &dyn Fn(&str) -> bool,
) -> ValidationResult {
    for slot in slots {
        if let ActionExpr::FunctionCall(slot) = slot {
            validate_body(&slot.args, protected, is_template)?;
        } else {
            validate(slot, protected, is_template)?;
        }
    }
    Ok(())
}

fn validate_binding_call(
    call: &FunctionCall,
    protected: &mut Vec<(String, &'static str)>,
    is_template: &dyn Fn(&str) -> bool,
) -> ValidationResult {
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
    match call.name.as_str() {
        "assert" => {
            for argument in &call.args {
                if let ActionExpr::FunctionCall(fact) = argument {
                    if is_template(&fact.name) {
                        validate_binding_slots(&fact.args, protected, is_template)?;
                    } else {
                        validate_body(&fact.args, protected, is_template)?;
                    }
                } else {
                    validate(argument, protected, is_template)?;
                }
            }
            Ok(())
        }
        "modify" | "duplicate" => {
            if let Some((target, slots)) = call.args.split_first() {
                validate(target, protected, is_template)?;
                validate_binding_slots(slots, protected, is_template)?;
            }
            Ok(())
        }
        _ => validate_body(&call.args, protected, is_template),
    }
}

fn validate(
    expression: &ActionExpr,
    protected: &mut Vec<(String, &'static str)>,
    is_template: &dyn Fn(&str) -> bool,
) -> ValidationResult {
    match expression {
        ActionExpr::FunctionCall(call) => validate_binding_call(call, protected, is_template),
        ActionExpr::If {
            condition,
            then_actions,
            else_actions,
            ..
        } => {
            validate(condition, protected, is_template)?;
            validate_body(then_actions, protected, is_template)?;
            validate_body(else_actions, protected, is_template)
        }
        ActionExpr::While {
            condition, body, ..
        } => {
            validate(condition, protected, is_template)?;
            validate_body(body, protected, is_template)
        }
        ActionExpr::LoopForCount {
            var_name,
            start,
            end,
            body,
            ..
        } => {
            validate(start, protected, is_template)?;
            validate(end, protected, is_template)?;
            validate_iteration(
                body,
                var_name.as_deref(),
                "PRCDRPSR1",
                protected,
                is_template,
            )
        }
        ActionExpr::Progn {
            var_name,
            list_expr,
            body,
            ..
        } => {
            validate(list_expr, protected, is_template)?;
            // The generated -index name is an ordinary writable local. Only
            // the actual element variable is protected by CLIPS syntax.
            validate_iteration(body, Some(var_name), "MULTIFUN2", protected, is_template)
        }
        ActionExpr::QueryAction {
            bindings,
            query,
            body,
            ..
        } => {
            for expression in bindings.iter().flat_map(|binding| &binding.restrictions) {
                validate(expression, protected, is_template)?;
            }
            validate(query, protected, is_template)?;
            validate_body(body, protected, is_template)
        }
        ActionExpr::Switch {
            expr,
            cases,
            default,
            ..
        } => {
            validate(expr, protected, is_template)?;
            for (value, body) in cases {
                validate(value, protected, is_template)?;
                validate_body(body, protected, is_template)?;
            }
            if let Some(body) = default {
                validate_body(body, protected, is_template)?;
            }
            Ok(())
        }
        ActionExpr::Literal(..) | ActionExpr::Variable(..) | ActionExpr::GlobalVariable(..) => {
            Ok(())
        }
    }
}
