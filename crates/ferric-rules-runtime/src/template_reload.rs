//! Template redefinition checks against live facts and construct references.
//!
//! These checks run only during loading. References are read from existing
//! state instead of maintaining another serialized reference-count registry.

use ferric_rules_core::{AlphaEntryType, Fact, RuleId, TemplateId};
use ferric_rules_parser::{ActionExpr, FunctionCall, Pattern, RuleConstruct};

use crate::engine::{rule_index_get, Engine};
use crate::modules::ModuleId;

impl Engine {
    pub(crate) fn template_is_in_use(&self, id: TemplateId) -> bool {
        if self.fact_base.facts_by_template(id).next().is_some()
            || self
                .rete
                .alpha
                .contains_entry(&AlphaEntryType::Template(id))
            || self
                .registered_deffacts
                .iter()
                .flat_map(|definition| &definition.facts)
                .any(|fact| matches!(fact, Fact::Template(template) if template.template_id == id))
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
                    .any(|expr| self.expr_uses_template(expr, module, id))
            })
        })
    }

    pub(crate) fn template_name_is(&self, name: &str, module: ModuleId, id: TemplateId) -> bool {
        self.resolve_template_id(name, module).ok() == Some(id)
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
    }

    fn call_uses_template(&self, call: &FunctionCall, module: ModuleId, id: TemplateId) -> bool {
        (call.name == "assert" && call.args.iter().any(|expr| {
            matches!(expr, ActionExpr::FunctionCall(fact) if self.template_name_is(&fact.name, module, id))
        })) || call.args.iter().any(|expr| self.expr_uses_template(expr, module, id))
    }

    fn expr_uses_template(&self, expr: &ActionExpr, module: ModuleId, id: TemplateId) -> bool {
        let uses = |expr| self.expr_uses_template(expr, module, id);
        match expr {
            ActionExpr::FunctionCall(call) => self.call_uses_template(call, module, id),
            ActionExpr::If {
                condition,
                then_actions,
                else_actions,
                ..
            } => uses(condition) || then_actions.iter().chain(else_actions).any(uses),
            ActionExpr::While {
                condition, body, ..
            } => uses(condition) || body.iter().any(uses),
            ActionExpr::LoopForCount {
                start, end, body, ..
            } => uses(start) || uses(end) || body.iter().any(uses),
            ActionExpr::Progn {
                list_expr, body, ..
            } => uses(list_expr) || body.iter().any(uses),
            ActionExpr::QueryAction {
                bindings,
                query,
                body,
                ..
            } => {
                bindings
                    .iter()
                    .any(|(_, name)| self.template_name_is(name, module, id))
                    || uses(query)
                    || body.iter().any(uses)
            }
            ActionExpr::Switch {
                expr,
                cases,
                default,
                ..
            } => {
                uses(expr)
                    || cases
                        .iter()
                        .any(|(condition, body)| uses(condition) || body.iter().any(uses))
                    || default.iter().flatten().any(uses)
            }
            ActionExpr::Literal(_) | ActionExpr::Variable(..) | ActionExpr::GlobalVariable(..) => {
                false
            }
        }
    }
}
