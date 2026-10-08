//! Lexical variable checks limited to result-query predicates.

use std::collections::HashSet;

use ferric_rules_parser::{ActionExpr, LiteralKind, Span};

use crate::modules::ModuleId;
use crate::Engine;

struct ScopeContext<'a> {
    engine: &'a Engine,
    module: ModuleId,
    lhs_queries: bool,
}

type ScopeResult = Result<(), (Span, String)>;

/// Query members have no fact address while their target expressions run.
/// CLIPS 6.30 dereferences invalid query state for such reads. Reject them while
/// preserving real inner lexical shadows and ordinary bind destinations.
pub(crate) fn validate_restriction_members(
    expression: &ActionExpr,
    bindings: &[ferric_rules_parser::QueryBinding],
    engine: &Engine,
    module: ModuleId,
) -> ScopeResult {
    let forbidden: HashSet<_> = bindings
        .iter()
        .map(|binding| binding.variable.as_str())
        .collect();
    restriction_reads(
        expression,
        &forbidden,
        &HashSet::new(),
        &ScopeContext {
            engine,
            module,
            lhs_queries: false,
        },
    )
}

fn restriction_reads(
    expression: &ActionExpr,
    forbidden: &HashSet<&str>,
    lexical: &HashSet<String>,
    context: &ScopeContext<'_>,
) -> ScopeResult {
    match expression {
        ActionExpr::Variable(name, span) => {
            let name = binding_name(name);
            if forbidden.contains(name) && !lexical.contains(name) {
                return Err((*span, format!("query member ?{name} cannot be read in its restriction before a fact is selected")));
            }
        }
        ActionExpr::FunctionCall(call) => {
            for (index, argument) in
                crate::effects::evaluated_arguments(context.engine, context.module, call)
                    .into_iter()
                    .enumerate()
            {
                if call.name == "bind" && index == 0 {
                    continue;
                }
                restriction_reads(argument, forbidden, lexical, context)?;
            }
        }
        ActionExpr::LoopForCount {
            var_name,
            start,
            end,
            body,
            ..
        } => {
            restriction_reads(start, forbidden, lexical, context)?;
            restriction_reads(end, forbidden, lexical, context)?;
            let mut inner = lexical.clone();
            inner.extend(var_name.iter().cloned());
            for action in body {
                restriction_reads(action, forbidden, &inner, context)?;
            }
        }
        ActionExpr::Progn {
            var_name,
            list_expr,
            body,
            ..
        } => {
            restriction_reads(list_expr, forbidden, lexical, context)?;
            let mut inner = lexical.clone();
            inner.extend([var_name.clone(), format!("{var_name}-index")]);
            for action in body {
                restriction_reads(action, forbidden, &inner, context)?;
            }
        }
        ActionExpr::QueryAction {
            bindings,
            query,
            body,
            ..
        } => {
            for restriction in bindings.iter().flat_map(|binding| &binding.restrictions) {
                restriction_reads(restriction, forbidden, lexical, context)?;
            }
            let mut inner = lexical.clone();
            inner.extend(bindings.iter().map(|binding| binding.variable.clone()));
            restriction_reads(query, forbidden, &inner, context)?;
            for action in body {
                restriction_reads(action, forbidden, &inner, context)?;
            }
        }
        _ => {
            let mut children = Vec::new();
            expression.push_children(&mut children);
            for child in children {
                restriction_reads(child, forbidden, lexical, context)?;
            }
        }
    }
    Ok(())
}

/// CLIPS knows all explicit local bind names when validating a callable, even
/// names bound later or in a branch. Loop and query member names remain lexical.
/// Expressions outside result-query predicates retain their existing policy.
pub(crate) fn validate_query_scopes<'a>(
    expressions: impl IntoIterator<Item = &'a ActionExpr>,
    ordinary: HashSet<String>,
    compact: &HashSet<String>,
    engine: &Engine,
    module: ModuleId,
) -> ScopeResult {
    validate_scopes(expressions, ordinary, compact, engine, module, false)
}

/// LHS query targets can use bindings established at their source position.
/// Other expression operands retain the ordinary match-expression policy.
pub(crate) fn validate_lhs_query_scopes(
    expression: &ActionExpr,
    ordinary: HashSet<String>,
    compact: &HashSet<String>,
    engine: &Engine,
    module: ModuleId,
) -> ScopeResult {
    validate_scopes(
        std::iter::once(expression),
        ordinary,
        compact,
        engine,
        module,
        true,
    )
}

fn validate_scopes<'a>(
    expressions: impl IntoIterator<Item = &'a ActionExpr>,
    mut ordinary: HashSet<String>,
    compact: &HashSet<String>,
    engine: &Engine,
    module: ModuleId,
    lhs_queries: bool,
) -> ScopeResult {
    let context = ScopeContext {
        engine,
        module,
        lhs_queries,
    };
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
        if let ActionExpr::FunctionCall(call) = expr {
            pending.extend(crate::effects::evaluated_arguments(engine, module, call));
        } else {
            expr.push_children(&mut pending);
        }
    }
    for expr in expressions {
        validate_expr(expr, &ordinary, compact, false, &context)?;
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
    context: &ScopeContext<'_>,
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
            for arg in crate::effects::evaluated_arguments(context.engine, context.module, call) {
                validate_expr(arg, ordinary, compact, in_predicate, context)?;
            }
        }
        ActionExpr::If {
            condition,
            then_actions,
            else_actions,
            ..
        } => {
            validate_expr(condition, ordinary, compact, in_predicate, context)?;
            for action in then_actions.iter().chain(else_actions) {
                validate_expr(action, ordinary, compact, in_predicate, context)?;
            }
        }
        ActionExpr::While {
            condition, body, ..
        } => {
            validate_expr(condition, ordinary, compact, in_predicate, context)?;
            for action in body {
                validate_expr(action, ordinary, compact, in_predicate, context)?;
            }
        }
        ActionExpr::LoopForCount {
            var_name,
            start,
            end,
            body,
            ..
        } => {
            validate_expr(start, ordinary, compact, in_predicate, context)?;
            validate_expr(end, ordinary, compact, in_predicate, context)?;
            let mut inner = ordinary.clone();
            inner.extend(var_name.iter().cloned());
            for action in body {
                validate_expr(action, &inner, compact, in_predicate, context)?;
            }
        }
        ActionExpr::Progn {
            var_name,
            list_expr,
            body,
            ..
        } => {
            validate_expr(list_expr, ordinary, compact, in_predicate, context)?;
            let mut inner = ordinary.clone();
            inner.insert(var_name.clone());
            inner.insert(format!("{var_name}-index"));
            for action in body {
                validate_expr(action, &inner, compact, in_predicate, context)?;
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
            for binding in bindings {
                for restriction in &binding.restrictions {
                    validate_expr(
                        restriction,
                        ordinary,
                        compact,
                        in_predicate || context.lhs_queries,
                        context,
                    )?;
                }
                inner.insert(binding.variable.clone());
                inner_compact.insert(binding.variable.clone());
            }
            let result_query =
                matches!(name.as_str(), "any-factp" | "find-fact" | "find-all-facts");
            validate_expr(
                query,
                &inner,
                &inner_compact,
                in_predicate || result_query || context.lhs_queries,
                context,
            )?;
            for action in body {
                validate_expr(
                    action,
                    &inner,
                    &inner_compact,
                    in_predicate || context.lhs_queries,
                    context,
                )?;
            }
        }
        ActionExpr::Switch {
            expr,
            cases,
            default,
            ..
        } => {
            validate_expr(expr, ordinary, compact, in_predicate, context)?;
            for (value, actions) in cases {
                validate_expr(value, ordinary, compact, in_predicate, context)?;
                for action in actions {
                    validate_expr(action, ordinary, compact, in_predicate, context)?;
                }
            }
            if let Some(actions) = default {
                for action in actions {
                    validate_expr(action, ordinary, compact, in_predicate, context)?;
                }
            }
        }
        ActionExpr::Variable(..) | ActionExpr::Literal(..) | ActionExpr::GlobalVariable(..) => {}
    }
    Ok(())
}
