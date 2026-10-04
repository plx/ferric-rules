//! Template redefinition checks against live facts and construct references.
//!
//! These checks run only during loading. References are read from existing
//! state instead of maintaining another serialized reference-count registry.

use ferric_rules_core::{AlphaEntryType, RuleId, TemplateId};
use ferric_rules_parser::{ActionExpr, FunctionCall, Pattern, RuleConstruct};

use crate::actions::CompiledTestCondition;
use crate::engine::{rule_index_get, Engine};
use crate::evaluator::RuntimeExpr;
use crate::fact_initializer::{PreparedFact, RuntimeExpressions};
use crate::modules::ModuleId;
use crate::query_targets::QueryTarget;

impl Engine {
    pub(crate) fn template_is_in_use(&self, id: TemplateId) -> bool {
        if self
            .active_query_targets
            .contains(&QueryTarget::Template(id))
            || self.fact_base.facts_by_template(id).next().is_some()
            || self
                .rete
                .alpha
                .contains_entry(&AlphaEntryType::Template(id))
            || self.registered_deffacts.iter().any(|definition| {
                definition.facts.iter().any(|fact| {
                    matches!(fact, PreparedFact::Template { template_id, .. } if *template_id == id)
                        || fact.all_expressions().any(|expression| {
                            self.runtime_expression_uses_template(expression, definition.module, id)
                        })
                })
            })
            || self.template_defs.values().any(|template| {
                template.dynamic_defaults.iter().flatten().any(|default| {
                    default.expressions.iter().any(|expression| {
                        RuntimeExpressions::new(expression).any(|expression| {
                            self.runtime_expression_uses_template(expression, default.module, id)
                        })
                    })
                })
            })
        {
            return true;
        }
        self.rule_info.iter().enumerate().any(|(index, info)| {
            let Some(info) = info else { return false };
            let rule = RuleId(u32::try_from(index).expect("rule index fits its ID"));
            let module = *rule_index_get(&self.rule_modules, rule)
                .expect("compiled rule has an owning module");
            info.actions
                .iter()
                .any(|action| self.call_uses_template(&action.call, module, id))
                || info.test_conditions.iter().any(|condition| {
                    let CompiledTestCondition::Expr(expression) = condition;
                    RuntimeExpressions::new(expression).any(|expression| {
                        self.runtime_expression_uses_template(expression, module, id)
                    })
                })
        }) || self.functions.functions.iter().any(|(&module, functions)| {
            functions.values().any(|function| {
                function
                    .body
                    .iter()
                    .any(|expr| self.expr_uses_template(expr, module, id))
            })
        }) || self.generics.iter().any(|(module, generic)| {
            generic.methods.iter().any(|method| {
                method
                    .body
                    .iter()
                    .chain(method.parameter_queries.iter().flatten())
                    .chain(method.wildcard_query.as_ref())
                    .any(|expr| self.expr_uses_template(expr, module, id))
            })
        })
    }

    pub(crate) fn template_name_is(&self, name: &str, module: ModuleId, id: TemplateId) -> bool {
        self.resolve_template_id(name, module).ok() == Some(id)
    }

    fn runtime_expression_uses_template(
        &self,
        expression: &RuntimeExpr,
        module: ModuleId,
        id: TemplateId,
    ) -> bool {
        match expression {
            RuntimeExpr::QueryAction { bindings, .. } => bindings.iter()
                .flat_map(|binding| &binding.restrictions)
                .any(|restriction| matches!(restriction,
                    RuntimeExpr::Literal(ferric_rules_core::Value::Symbol(symbol))
                    if self.resolve_core_symbol(*symbol).is_some_and(|name| self.template_name_is(name, module, id)))),
            RuntimeExpr::EffectCall { call } => self.call_uses_template(call, module, id),
            _ => false,
        }
    }

    pub(crate) fn rule_uses_template(
        &self,
        rule: &RuleConstruct,
        module: ModuleId,
        id: TemplateId,
    ) -> bool {
        let mut patterns: Vec<_> = rule.patterns.iter().collect();
        while let Some(pattern) = patterns.pop() {
            match pattern {
                Pattern::Ordered(pattern)
                    if self.template_name_is(&pattern.relation, module, id) =>
                {
                    return true
                }
                Pattern::Template(pattern)
                    if self.template_name_is(&pattern.template, module, id) =>
                {
                    return true
                }
                Pattern::Not(pattern, _) | Pattern::Assigned { pattern, .. } => {
                    patterns.push(pattern);
                }
                Pattern::And(children, _)
                | Pattern::Or(children, _)
                | Pattern::Exists(children, _)
                | Pattern::Forall(children, _)
                | Pattern::Logical(children, _) => patterns.extend(children),
                _ => {}
            }
        }
        rule.actions
            .iter()
            .any(|action| self.call_uses_template(&action.call, module, id))
            || crate::template_identity::rule_lhs_expressions(rule)
                .into_iter()
                .any(|expression| {
                    ferric_rules_parser::interpret_action_expr(expression)
                        .is_ok_and(|expression| self.expr_uses_template(&expression, module, id))
                })
    }

    fn call_uses_template(&self, call: &FunctionCall, module: ModuleId, id: TemplateId) -> bool {
        (call.name == "assert" && call.args.iter().any(|expr| {
            matches!(expr, ActionExpr::FunctionCall(fact) if self.template_name_is(&fact.name, module, id))
        })) || crate::effects::evaluated_arguments(self, module, call).iter().any(|expr| self.expr_uses_template(expr, module, id))
    }

    fn expr_uses_template(&self, expr: &ActionExpr, module: ModuleId, id: TemplateId) -> bool {
        if let ActionExpr::FunctionCall(call) = expr {
            return self.call_uses_template(call, module, id);
        }
        if let ActionExpr::QueryAction { bindings, .. } = expr {
            if bindings
                .iter()
                .flat_map(|binding| &binding.restrictions)
                .any(|restriction| {
                    matches!(restriction, ActionExpr::Literal(literal)
                    if matches!(&literal.value, ferric_rules_parser::LiteralKind::Symbol(name)
                        if self.template_name_is(name, module, id)))
                })
            {
                return true;
            }
        }
        let mut children = Vec::new();
        expr.push_children(&mut children);
        children
            .into_iter()
            .any(|child| self.expr_uses_template(child, module, id))
    }
}
