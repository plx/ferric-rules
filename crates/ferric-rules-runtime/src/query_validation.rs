//! Lexical variable checks limited to result-query predicates.

use std::collections::HashSet;

use ferric_rules_parser::{ActionExpr, LiteralKind, Span};

type ScopeResult = Result<(), (Span, String)>;

/// CLIPS knows all explicit local bind names when validating a callable, even
/// names bound later or in a branch. Loop and query member names remain lexical.
/// Expressions outside result-query predicates retain their existing policy.
pub(crate) fn validate_query_scopes<'a>(
    expressions: impl IntoIterator<Item = &'a ActionExpr>,
    mut ordinary: HashSet<String>,
    compact: &HashSet<String>,
) -> ScopeResult {
    let expressions: Vec<_> = expressions.into_iter().collect();
    let mut pending = expressions.clone();
    while let Some(expr) = pending.pop() {
        if let ActionExpr::FunctionCall(call) = expr {
            if call.name == "bind" {
                if let Some(ActionExpr::Variable(name, _)) = call.args.first() {
                    ordinary.insert(binding_name(name).into());
                }
            }
        }
        push_children(expr, &mut pending);
    }
    for expr in expressions {
        validate_expr(expr, &ordinary, compact, false)?;
    }
    Ok(())
}

fn binding_name(name: &str) -> &str {
    name.strip_prefix("$?").unwrap_or(name)
}

fn unbound(name: &str, span: Span) -> (Span, String) {
    (
        span,
        format!("[PRCCODE3] undefined variable ?{name} in fact-query predicate"),
    )
}

#[allow(clippy::too_many_lines)] // Each structured expression introduces its own lexical scope.
fn validate_expr(
    expr: &ActionExpr,
    ordinary: &HashSet<String>,
    compact: &HashSet<String>,
    in_predicate: bool,
) -> ScopeResult {
    match expr {
        ActionExpr::Variable(name, span) if in_predicate => {
            if !ordinary.contains(binding_name(name)) {
                return Err(unbound(binding_name(name), *span));
            }
        }
        ActionExpr::FunctionCall(call) => {
            // The parser preserves compact syntax as a call whose first
            // argument is the member name. Its ordinary colon-name alternative
            // is visible only when no lexical fact member has that name.
            if in_predicate && call.name == "__fact_slot_ref" {
                if let [ActionExpr::Variable(member, span), ActionExpr::Literal(slot)] =
                    call.args.as_slice()
                {
                    if let LiteralKind::Symbol(slot) = &slot.value {
                        let full_name = format!("{member}:{slot}");
                        if !compact.contains(member) && !ordinary.contains(&full_name) {
                            return Err(unbound(&full_name, *span));
                        }
                        return Ok(());
                    }
                }
            }
            for arg in &call.args {
                validate_expr(arg, ordinary, compact, in_predicate)?;
            }
        }
        ActionExpr::If {
            condition,
            then_actions,
            else_actions,
            ..
        } => {
            validate_expr(condition, ordinary, compact, in_predicate)?;
            for action in then_actions.iter().chain(else_actions) {
                validate_expr(action, ordinary, compact, in_predicate)?;
            }
        }
        ActionExpr::While {
            condition, body, ..
        } => {
            validate_expr(condition, ordinary, compact, in_predicate)?;
            for action in body {
                validate_expr(action, ordinary, compact, in_predicate)?;
            }
        }
        ActionExpr::LoopForCount {
            var_name,
            start,
            end,
            body,
            ..
        } => {
            validate_expr(start, ordinary, compact, in_predicate)?;
            validate_expr(end, ordinary, compact, in_predicate)?;
            let mut inner = ordinary.clone();
            inner.extend(var_name.iter().cloned());
            for action in body {
                validate_expr(action, &inner, compact, in_predicate)?;
            }
        }
        ActionExpr::Progn {
            var_name,
            list_expr,
            body,
            ..
        } => {
            validate_expr(list_expr, ordinary, compact, in_predicate)?;
            let mut inner = ordinary.clone();
            inner.insert(var_name.clone());
            inner.insert(format!("{var_name}-index"));
            for action in body {
                validate_expr(action, &inner, compact, in_predicate)?;
            }
        }
        ActionExpr::QueryAction {
            name,
            bindings,
            query,
            body,
            ..
        } => {
            let mut inner = ordinary.clone();
            let mut inner_compact = compact.clone();
            for (member, _) in bindings {
                inner.insert(member.clone());
                inner_compact.insert(member.clone());
            }
            let result_query =
                matches!(name.as_str(), "any-factp" | "find-fact" | "find-all-facts");
            validate_expr(query, &inner, &inner_compact, in_predicate || result_query)?;
            for action in body {
                validate_expr(action, &inner, &inner_compact, in_predicate)?;
            }
        }
        ActionExpr::Switch {
            expr,
            cases,
            default,
            ..
        } => {
            validate_expr(expr, ordinary, compact, in_predicate)?;
            for (value, actions) in cases {
                validate_expr(value, ordinary, compact, in_predicate)?;
                for action in actions {
                    validate_expr(action, ordinary, compact, in_predicate)?;
                }
            }
            if let Some(actions) = default {
                for action in actions {
                    validate_expr(action, ordinary, compact, in_predicate)?;
                }
            }
        }
        ActionExpr::Variable(..) | ActionExpr::Literal(..) | ActionExpr::GlobalVariable(..) => {}
    }
    Ok(())
}

fn push_children<'a>(expr: &'a ActionExpr, pending: &mut Vec<&'a ActionExpr>) {
    match expr {
        ActionExpr::FunctionCall(call) => pending.extend(&call.args),
        ActionExpr::If {
            condition,
            then_actions,
            else_actions,
            ..
        } => {
            pending.push(condition);
            pending.extend(then_actions);
            pending.extend(else_actions);
        }
        ActionExpr::While {
            condition, body, ..
        } => {
            pending.push(condition);
            pending.extend(body);
        }
        ActionExpr::LoopForCount {
            start, end, body, ..
        } => {
            pending.push(start);
            pending.push(end);
            pending.extend(body);
        }
        ActionExpr::Progn {
            list_expr, body, ..
        } => {
            pending.push(list_expr);
            pending.extend(body);
        }
        ActionExpr::QueryAction { query, body, .. } => {
            pending.push(query);
            pending.extend(body);
        }
        ActionExpr::Switch {
            expr,
            cases,
            default,
            ..
        } => {
            pending.push(expr);
            for (value, actions) in cases {
                pending.push(value);
                pending.extend(actions);
            }
            if let Some(actions) = default {
                pending.extend(actions);
            }
        }
        ActionExpr::Literal(..) | ActionExpr::Variable(..) | ActionExpr::GlobalVariable(..) => {}
    }
}
