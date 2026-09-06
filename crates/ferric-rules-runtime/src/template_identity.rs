//! Prevent a later explicit template from changing a live ordered relation.
//!
//! Ordered relations have global identities in this engine. Reject replacement
//! while any fact or construct depends on that identity; do not reinterpret
//! previously loaded RHS forms using a newly installed template.

use crate::engine::Engine;
use crate::modules::ModuleId;
use ferric_rules_core::{AlphaEntryType, Fact};
use ferric_rules_parser::{ActionExpr, FunctionCall, Pattern, RuleConstruct};

impl Engine {
    pub(crate) fn ordered_identity_is_live(&self, name: &str) -> bool {
        if name == "initial-fact" {
            return true;
        }
        let matches_fact = |fact: &Fact| matches!(fact, Fact::Ordered(fact) if self.resolve_symbol(fact.relation).is_some_and(|raw| Self::ordered_relation_name_is(raw, name)));
        if self.fact_base.iter().any(|(_, entry)| matches_fact(&entry.fact))
            || self.rete.alpha.entry_types().any(|entry| {
                matches!(entry, AlphaEntryType::OrderedRelation(symbol) if self.resolve_symbol(*symbol).is_some_and(|raw| Self::ordered_relation_name_is(raw, name)))
            })
            || self.registered_deffacts.iter().flatten().any(matches_fact)
        {
            return true;
        }
        self.rule_info
            .iter()
            .zip(&self.rule_modules)
            .any(|(info, module)| {
                let (Some(info), Some(module)) = (info, module) else {
                    return false;
                };
                info.actions
                    .iter()
                    .any(|action| self.call_uses_ordered_name(&action.call, *module, name))
            })
            || self.functions.functions.iter().any(|(&module, functions)| {
                functions.values().any(|function| {
                    function
                        .body
                        .iter()
                        .any(|expr| self.expr_uses_ordered_name(expr, module, name))
                })
            })
            || self.generics.iter().any(|(module, generic)| {
                generic.methods.iter().any(|method| {
                    method
                        .body
                        .iter()
                        .any(|expr| self.expr_uses_ordered_name(expr, module, name))
                })
            })
    }

    pub(crate) fn ordered_relation_name_is(raw: &str, name: &str) -> bool {
        crate::qualified_name::parse_qualified_name(raw)
            .is_ok_and(|parsed| parsed.local_name() == name)
    }

    pub(crate) fn ordered_name_is(&self, raw: &str, module: ModuleId, name: &str) -> bool {
        Self::ordered_relation_name_is(raw, name)
            && self.resolve_template_reference(raw, module).is_err()
    }

    pub(crate) fn rule_uses_ordered_name(
        &self,
        rule: &RuleConstruct,
        module: ModuleId,
        name: &str,
    ) -> bool {
        let mut patterns: Vec<_> = rule.patterns.iter().collect();
        while let Some(pattern) = patterns.pop() {
            match pattern {
                Pattern::Ordered(pattern)
                    if self.ordered_name_is(&pattern.relation, module, name) =>
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
            .any(|action| self.call_uses_ordered_name(&action.call, module, name))
    }

    fn call_uses_ordered_name(&self, call: &FunctionCall, module: ModuleId, name: &str) -> bool {
        (call.name == "assert" && call.args.iter().any(|expr| {
            matches!(expr, ActionExpr::FunctionCall(fact) if self.ordered_name_is(&fact.name, module, name))
        })) || call.args.iter().any(|expr| self.expr_uses_ordered_name(expr, module, name))
    }

    fn expr_uses_ordered_name(&self, expr: &ActionExpr, module: ModuleId, name: &str) -> bool {
        let uses = |expr| self.expr_uses_ordered_name(expr, module, name);
        match expr {
            ActionExpr::FunctionCall(call) => self.call_uses_ordered_name(call, module, name),
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
                    .any(|(_, raw)| self.ordered_name_is(raw, module, name))
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
