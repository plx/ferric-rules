//! Source checks for local binds that would overwrite lexical iterators.

use ferric_rules_parser::{ActionExpr, Span};

type ValidationResult = Result<(), (Span, String)>;

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
