//! Prevent a later explicit template from changing a live ordered relation.
//!
//! Ordered relations have global identities in this engine. Reject replacement
//! while any fact or construct depends on that identity; do not reinterpret
//! previously loaded RHS forms using a newly installed template.

use crate::actions::CompiledTestCondition;
use crate::engine::Engine;
use crate::evaluator::RuntimeExpr;
use crate::fact_initializer::{PreparedFact, RuntimeExpressions};
use crate::modules::ModuleId;
use crate::query_targets::QueryTarget;
use crate::templates::DynamicSlotDefault;
use ferric_rules_core::{AlphaEntryType, Fact};
use ferric_rules_parser::{ActionExpr, FactBody, FactValue, FunctionCall, Pattern, RuleConstruct};

impl Engine {
    /// Whether compiled source running outside any construct (see
    /// `active_expressions`) names a template or ordered relation: a fact
    /// assertion or a fact query. Like CLIPS's installed top-level command,
    /// such an expression keeps those constructs in use, so `clear` refuses.
    pub(crate) fn active_expressions_name_facts(&self) -> bool {
        self.active_expressions.iter().any(|(module, expression)| {
            RuntimeExpressions::new(expression).any(|expression| match expression {
                RuntimeExpr::QueryAction { .. } => true,
                RuntimeExpr::EffectCall { call } => self.call_names_facts(call, *module),
                _ => false,
            })
        })
    }

    /// Whether compiled source running outside any construct calls a user
    /// deffunction or defgeneric visible from its module. CLIPS binds such a
    /// call when it parses the expression, so the callable stays in use and
    /// `clear` refuses rather than leave the call without a target.
    pub(crate) fn active_expressions_name_callables(&self) -> bool {
        self.active_expressions.iter().any(|(module, expression)| {
            RuntimeExpressions::new(expression).any(|expression| match expression {
                RuntimeExpr::Call { name, .. } => self.names_user_callable(name, *module),
                RuntimeExpr::EffectCall { call } => self.effect_names_callables(call, *module),
                _ => false,
            })
        })
    }

    fn effect_names_callables(&self, call: &FunctionCall, module: ModuleId) -> bool {
        self.names_user_callable(&call.name, module)
            || crate::effects::evaluated_arguments(self, module, call)
                .iter()
                .any(|expr| self.expr_names_callables(expr, module))
    }

    fn expr_names_callables(&self, expr: &ActionExpr, module: ModuleId) -> bool {
        if let ActionExpr::FunctionCall(call) = expr {
            return self.effect_names_callables(call, module);
        }
        let mut children = Vec::new();
        expr.push_children(&mut children);
        children
            .into_iter()
            .any(|child| self.expr_names_callables(child, module))
    }

    /// Whether `name`, called from `module`, names a registered deffunction or
    /// defgeneric there or one that module can see.
    fn names_user_callable(&self, name: &str, module: ModuleId) -> bool {
        if let Ok(crate::QualifiedName::Qualified { module, name }) =
            crate::parse_qualified_name(name)
        {
            return self
                .module_registry
                .get_by_name(&module)
                .is_some_and(|owner| {
                    self.functions.contains(owner, &name) || self.generics.contains(owner, &name)
                });
        }
        let visible = |owner: &ModuleId, construct_type| {
            self.module_registry
                .is_construct_visible(module, *owner, construct_type, name)
        };
        self.functions
            .modules_for_name(name)
            .iter()
            .any(|owner| visible(owner, "deffunction"))
            || self
                .generics
                .modules_for_name(name)
                .iter()
                .any(|owner| visible(owner, "defgeneric"))
    }

    fn call_names_facts(&self, call: &FunctionCall, module: ModuleId) -> bool {
        (call.name == "assert"
            && call
                .args
                .iter()
                .any(|expr| matches!(expr, ActionExpr::FunctionCall(_))))
            || crate::effects::evaluated_arguments(self, module, call)
                .iter()
                .any(|expr| self.expr_names_facts(expr, module))
    }

    fn expr_names_facts(&self, expr: &ActionExpr, module: ModuleId) -> bool {
        match expr {
            ActionExpr::FunctionCall(call) => self.call_names_facts(call, module),
            ActionExpr::QueryAction { .. } => true,
            _ => {
                let mut children = Vec::new();
                expr.push_children(&mut children);
                children
                    .into_iter()
                    .any(|child| self.expr_names_facts(child, module))
            }
        }
    }

    pub(crate) fn ordered_identity_is_live(&self, name: &str) -> bool {
        if name == "initial-fact" || self.active_query_targets.iter().any(|target| {
            matches!(target, QueryTarget::Ordered(symbol)
                if self.resolve_core_symbol(*symbol).is_some_and(|raw| Self::ordered_relation_name_is(raw, name)))
        }) {
            return true;
        }
        let is_relation = |relation| {
            self.resolve_core_symbol(relation)
                .is_some_and(|raw| Self::ordered_relation_name_is(raw, name))
        };
        if self
            .active_ordered_relations
            .iter()
            .any(|relation| is_relation(*relation))
        {
            return true;
        }
        let matches_fact = |fact: &Fact| matches!(fact, Fact::Ordered(fact) if self.resolve_core_symbol(fact.relation).is_some_and(|raw| Self::ordered_relation_name_is(raw, name)));
        if self.fact_base.iter().any(|(_, entry)| matches_fact(&entry.fact))
            || self.rete.alpha.entry_types().any(|entry| {
                matches!(entry, AlphaEntryType::OrderedRelation(symbol) if self.resolve_core_symbol(*symbol).is_some_and(|raw| Self::ordered_relation_name_is(raw, name)))
            })
            || self.registered_deffacts.iter().any(|seed| seed.facts.iter().any(|fact| {
                matches!(fact, PreparedFact::Ordered { relation, .. } if self.resolve_core_symbol(*relation).is_some_and(|raw| Self::ordered_relation_name_is(raw, name)))
                    || fact.all_expressions().any(|expression| self.runtime_expression_uses_ordered_name(expression, seed.module, name))
            }))
            || self.template_defs.values().any(|template| {
                template
                    .dynamic_defaults
                    .iter()
                    .flatten()
                    .any(|default| self.dynamic_default_uses_ordered_name(default, name))
            })
            || self.active_expressions.iter().any(|(module, expression)| {
                self.runtime_expressions_use_ordered_name(
                    std::slice::from_ref(expression.as_ref()),
                    *module,
                    name,
                )
            })
            // A rule that removed itself keeps running its actions.
            || self.active_rules.iter().any(|(module, info)| {
                info.actions
                    .iter()
                    .any(|action| self.call_uses_ordered_name(&action.call, *module, name))
            })
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
                    || info.test_conditions.iter().any(|condition| {
                        let CompiledTestCondition::Expr(expression) = condition;
                        RuntimeExpressions::new(expression).any(|expression| {
                            self.runtime_expression_uses_ordered_name(expression, *module, name)
                        })
                    })
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
                        .chain(method.parameter_queries.iter().flatten())
                        .chain(method.wildcard_query.as_ref())
                        .any(|expr| self.expr_uses_ordered_name(expr, module, name))
                })
            })
    }

    /// Whether a dynamic default asserts or queries `name` as an ordered relation.
    pub(crate) fn dynamic_default_uses_ordered_name(
        &self,
        default: &DynamicSlotDefault,
        name: &str,
    ) -> bool {
        self.runtime_expressions_use_ordered_name(&default.expressions, default.module, name)
    }

    /// Whether compiled expressions, including nested ones, assert or query
    /// `name` as an ordered relation.
    fn runtime_expressions_use_ordered_name(
        &self,
        expressions: &[RuntimeExpr],
        module: ModuleId,
        name: &str,
    ) -> bool {
        expressions.iter().any(|expression| {
            RuntimeExpressions::new(expression).any(|expression| {
                self.runtime_expression_uses_ordered_name(expression, module, name)
            })
        })
    }

    pub(crate) fn ordered_relation_name_is(raw: &str, name: &str) -> bool {
        crate::qualified_name::parse_qualified_name(raw)
            .is_ok_and(|parsed| parsed.local_name() == name)
    }

    pub(crate) fn ordered_name_is(&self, raw: &str, module: ModuleId, name: &str) -> bool {
        Self::ordered_relation_name_is(raw, name) && self.resolve_template_id(raw, module).is_err()
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
            || rule_lhs_expressions(rule).into_iter().any(|expression| {
                ferric_rules_parser::interpret_action_expr(expression)
                    .is_ok_and(|expression| self.expr_uses_ordered_name(&expression, module, name))
            })
    }

    /// A deffacts body still queued in the current load uses an ordered
    /// relation through its head or through a fact query in an initializer.
    pub(crate) fn fact_body_uses_ordered_name(
        &self,
        fact: &FactBody,
        module: ModuleId,
        name: &str,
    ) -> bool {
        let initializer_uses = |value: &FactValue| matches!(value, FactValue::Expression(expr) if self.expr_uses_ordered_name(expr, module, name));
        match fact {
            FactBody::Ordered(fact) => {
                self.ordered_name_is(&fact.relation, module, name)
                    || fact.values.iter().any(initializer_uses)
            }
            FactBody::Template(fact) => {
                self.ordered_name_is(&fact.template, module, name)
                    || fact.slot_values.iter().any(|slot| {
                        slot.values.iter().any(initializer_uses)
                            || slot
                                .ordered_expression
                                .as_deref()
                                .is_some_and(|expr| self.expr_uses_ordered_name(expr, module, name))
                    })
            }
        }
    }

    fn call_uses_ordered_name(&self, call: &FunctionCall, module: ModuleId, name: &str) -> bool {
        (call.name == "assert" && call.args.iter().any(|expr| {
            matches!(expr, ActionExpr::FunctionCall(fact) if self.ordered_name_is(&fact.name, module, name))
        })) || crate::effects::evaluated_arguments(self, module, call).iter().any(|expr| self.expr_uses_ordered_name(expr, module, name))
    }

    fn runtime_expression_uses_ordered_name(
        &self,
        expression: &RuntimeExpr,
        module: ModuleId,
        name: &str,
    ) -> bool {
        match expression {
            RuntimeExpr::QueryAction { bindings, .. } => bindings.iter()
                .flat_map(|binding| &binding.restrictions)
                .any(|restriction| matches!(restriction,
                    RuntimeExpr::Literal(ferric_rules_core::Value::Symbol(symbol))
                    if self.resolve_core_symbol(*symbol).is_some_and(|raw| self.ordered_name_is(raw, module, name)))),
            RuntimeExpr::EffectCall { call } => self.call_uses_ordered_name(call, module, name),
            _ => false,
        }
    }

    fn expr_uses_ordered_name(&self, expr: &ActionExpr, module: ModuleId, name: &str) -> bool {
        if let ActionExpr::FunctionCall(call) = expr {
            return self.call_uses_ordered_name(call, module, name);
        }
        if let ActionExpr::QueryAction { bindings, .. } = expr {
            if bindings
                .iter()
                .flat_map(|binding| &binding.restrictions)
                .any(|restriction| {
                    matches!(restriction, ActionExpr::Literal(literal)
                    if matches!(&literal.value, ferric_rules_parser::LiteralKind::Symbol(raw)
                        if self.ordered_name_is(raw, module, name)))
                })
            {
                return true;
            }
        }
        let mut children = Vec::new();
        expr.push_children(&mut children);
        children
            .into_iter()
            .any(|child| self.expr_uses_ordered_name(child, module, name))
    }
}

/// Raw match expressions also carry literal query dependencies before a rule is compiled.
pub(crate) fn rule_lhs_expressions(rule: &RuleConstruct) -> Vec<&ferric_rules_parser::SExpr> {
    use ferric_rules_parser::Constraint;
    let mut expressions = Vec::new();
    let mut patterns: Vec<_> = rule.patterns.iter().collect();
    let mut constraints = Vec::new();
    while let Some(pattern) = patterns.pop() {
        match pattern {
            Pattern::Ordered(pattern) => constraints.extend(&pattern.constraints),
            Pattern::Template(pattern) => constraints.extend(
                pattern
                    .slot_constraints
                    .iter()
                    .flat_map(|slot| &slot.constraints),
            ),
            Pattern::Test(expression, _) => expressions.push(expression),
            Pattern::Not(inner, _) | Pattern::Assigned { pattern: inner, .. } => {
                patterns.push(inner);
            }
            Pattern::And(children, _)
            | Pattern::Or(children, _)
            | Pattern::Exists(children, _)
            | Pattern::Forall(children, _)
            | Pattern::Logical(children, _) => patterns.extend(children),
        }
    }
    while let Some(constraint) = constraints.pop() {
        match constraint {
            Constraint::Predicate(expression, _) | Constraint::ReturnValue(expression, _) => {
                expressions.push(expression);
            }
            Constraint::Not(inner, _) => constraints.push(inner),
            Constraint::And(children, _) | Constraint::Or(children, _) => {
                constraints.extend(children);
            }
            _ => {}
        }
    }
    expressions
}

impl Engine {
    pub(crate) fn declare_implicit_template(&mut self, raw: &str, module: ModuleId) {
        let Ok(parsed) = crate::qualified_name::parse_qualified_name(raw) else {
            return;
        };
        let local = parsed.local_name();
        // Until ordered relations gain module-local identities, all callers share
        // the first declaration's owner, matching their shared fact identity.
        if self.template_declaration_names.contains(local)
            || self.resolve_template_id(raw, module).is_ok()
        {
            return;
        }
        let owner = parsed
            .module_name()
            .and_then(|name| self.module_registry.get_by_name(name))
            .unwrap_or(module);
        self.template_declaration_names.insert(local.to_owned());
        self.template_declarations.push((owner, local.to_owned()));
    }

    pub(crate) fn declare_explicit_template(&mut self, raw: &str, module: ModuleId) {
        let local = raw.rsplit("::").next().unwrap_or(raw);
        if !self.template_declaration_names.insert(local.to_owned()) {
            self.template_declarations
                .retain(|(owner, name)| *owner != module || name != local);
        }
        self.template_declarations.push((module, local.to_owned()));
    }

    pub(crate) fn has_implicit_template(&self, raw: &str, module: ModuleId) -> bool {
        let Ok(parsed) = crate::qualified_name::parse_qualified_name(raw) else {
            return false;
        };
        self.template_declarations.iter().any(|(owner, name)| {
            name == parsed.local_name()
                && parsed.module_name().map_or(true, |wanted| {
                    self.module_registry.module_name(*owner) == Some(wanted)
                })
                && (parsed.module_name().is_some()
                    || self.module_registry.is_construct_visible(
                        module,
                        *owner,
                        "deftemplate",
                        name,
                    ))
                && !self.template_defs.iter().any(|(id, template)| {
                    self.template_modules.get(id) == Some(owner)
                        && template.name.rsplit("::").next() == Some(name.as_str())
                })
        })
    }

    pub(crate) fn declare_expression_templates(
        &mut self,
        expression: &ActionExpr,
        module: ModuleId,
    ) {
        let mut declarations = Vec::new();
        self.collect_expression_templates(expression, module, &mut declarations);
        declarations.sort_by_key(|(offset, _)| *offset);
        for (_, name) in declarations {
            self.declare_implicit_template(&name, module);
        }
    }

    fn collect_expression_templates(
        &self,
        expression: &ActionExpr,
        module: ModuleId,
        names: &mut Vec<(usize, String)>,
    ) {
        match expression {
            ActionExpr::FunctionCall(call) => {
                if call.name == "assert" {
                    for expression in &call.args {
                        if let ActionExpr::FunctionCall(fact) = expression {
                            if self.resolve_template_id(&fact.name, module).is_err() {
                                names.push((fact.span.start.offset, fact.name.clone()));
                            }
                        }
                    }
                }
                for expression in crate::effects::evaluated_arguments(self, module, call) {
                    self.collect_expression_templates(expression, module, names);
                }
            }
            ActionExpr::If {
                condition,
                then_actions,
                else_actions,
                ..
            } => {
                self.collect_expression_templates(condition, module, names);
                for expression in then_actions.iter().chain(else_actions) {
                    self.collect_expression_templates(expression, module, names);
                }
            }
            ActionExpr::While {
                condition, body, ..
            } => {
                self.collect_expression_templates(condition, module, names);
                for expression in body {
                    self.collect_expression_templates(expression, module, names);
                }
            }
            ActionExpr::LoopForCount {
                start, end, body, ..
            } => {
                self.collect_expression_templates(start, module, names);
                self.collect_expression_templates(end, module, names);
                for expression in body {
                    self.collect_expression_templates(expression, module, names);
                }
            }
            ActionExpr::Progn {
                list_expr, body, ..
            } => {
                self.collect_expression_templates(list_expr, module, names);
                for expression in body {
                    self.collect_expression_templates(expression, module, names);
                }
            }
            ActionExpr::QueryAction { query, body, .. } => {
                self.collect_expression_templates(query, module, names);
                for expression in body {
                    self.collect_expression_templates(expression, module, names);
                }
            }
            ActionExpr::Switch {
                expr,
                cases,
                default,
                ..
            } => {
                self.collect_expression_templates(expr, module, names);
                for (condition, body) in cases {
                    self.collect_expression_templates(condition, module, names);
                    for expression in body {
                        self.collect_expression_templates(expression, module, names);
                    }
                }
                for expression in default.iter().flatten() {
                    self.collect_expression_templates(expression, module, names);
                }
            }
            ActionExpr::Literal(_) | ActionExpr::Variable(..) | ActionExpr::GlobalVariable(..) => {}
        }
    }

    pub(crate) fn declare_fact_templates(&mut self, fact: &FactBody, module: ModuleId) {
        let mut expressions = Vec::new();
        match fact {
            FactBody::Ordered(fact) => {
                self.declare_implicit_template(&fact.relation, module);
                expressions.extend(fact.values.iter().filter_map(|value| match value {
                    FactValue::Expression(expression) => Some(expression.as_ref()),
                    _ => None,
                }));
            }
            FactBody::Template(fact) => {
                let explicit = self.resolve_template_id(&fact.template, module).is_ok();
                self.declare_implicit_template(&fact.template, module);
                for slot in &fact.slot_values {
                    if explicit {
                        expressions.extend(slot.values.iter().filter_map(|value| match value {
                            FactValue::Expression(expression) => Some(expression.as_ref()),
                            _ => None,
                        }));
                    } else if let Some(expression) = &slot.ordered_expression {
                        expressions.push(expression);
                    }
                }
            }
        }
        for expression in expressions {
            self.declare_expression_templates(expression, module);
        }
    }

    pub(crate) fn declare_rule_templates(&mut self, rule: &RuleConstruct, module: ModuleId) {
        let mut patterns: Vec<_> = rule.patterns.iter().rev().collect();
        while let Some(pattern) = patterns.pop() {
            match pattern {
                Pattern::Ordered(pattern) => {
                    self.declare_implicit_template(&pattern.relation, module);
                }
                Pattern::Not(pattern, _) | Pattern::Assigned { pattern, .. } => {
                    patterns.push(pattern);
                }
                Pattern::And(children, _)
                | Pattern::Or(children, _)
                | Pattern::Exists(children, _)
                | Pattern::Forall(children, _)
                | Pattern::Logical(children, _) => patterns.extend(children.iter().rev()),
                Pattern::Template(_) | Pattern::Test(..) => {}
            }
        }
        for action in &rule.actions {
            self.declare_expression_templates(
                &ActionExpr::FunctionCall(action.call.clone()),
                module,
            );
        }
    }
}
