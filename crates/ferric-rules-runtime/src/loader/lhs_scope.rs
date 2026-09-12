//! Validate source-position bindings before constructing any matching nodes.

use super::{Atom, Constraint, Engine, HashSet, LoadError, Pattern, RuleConstruct, SExpr, Span};
use crate::modules::ModuleId;
use crate::qualified_name::{parse_qualified_name, QualifiedName};

impl Engine {
    pub(super) fn validate_rule_lhs_scope(&self, rule: &RuleConstruct) -> Result<(), LoadError> {
        let mut available = HashSet::new();
        for pattern in &rule.patterns {
            self.validate_lhs_pattern(&rule.name, pattern, &mut available)?;
        }
        Ok(())
    }

    fn validate_lhs_pattern(
        &self,
        rule_name: &str,
        pattern: &Pattern,
        available: &mut HashSet<String>,
    ) -> Result<(), LoadError> {
        match pattern {
            Pattern::Ordered(ordered) => {
                for constraint in &ordered.constraints {
                    self.validate_lhs_constraint(
                        rule_name,
                        &Self::field_constraint(constraint),
                        available,
                        true,
                    )?;
                }
            }
            Pattern::Template(template) => {
                for slot in &template.slot_constraints {
                    for constraint in &slot.constraints {
                        self.validate_lhs_constraint(
                            rule_name,
                            &Self::field_constraint(constraint),
                            available,
                            true,
                        )?;
                    }
                }
            }
            Pattern::Assigned {
                variable, pattern, ..
            } => {
                available.insert(variable.clone());
                self.validate_lhs_pattern(rule_name, pattern, available)?;
            }
            Pattern::Test(expr, _) => self.validate_lhs_expression(rule_name, expr, available)?,
            Pattern::And(children, _) | Pattern::Logical(children, _) => {
                for child in children {
                    self.validate_lhs_pattern(rule_name, child, available)?;
                }
            }
            Pattern::Not(inner, _) => {
                self.validate_lhs_pattern(rule_name, inner, &mut available.clone())?;
            }
            Pattern::Exists(children, _) | Pattern::Forall(children, _) => {
                let mut inner = available.clone();
                for child in children {
                    self.validate_lhs_pattern(rule_name, child, &mut inner)?;
                }
            }
            Pattern::Or(children, _) => {
                let mut common: Option<HashSet<String>> = None;
                for child in children {
                    let mut branch = available.clone();
                    self.validate_lhs_pattern(rule_name, child, &mut branch)?;
                    common = Some(match common {
                        None => branch,
                        Some(previous) => previous.intersection(&branch).cloned().collect(),
                    });
                }
                if let Some(common) = common {
                    available.extend(common);
                }
            }
        }
        Ok(())
    }

    fn validate_lhs_constraint(
        &self,
        rule_name: &str,
        constraint: &Constraint,
        available: &mut HashSet<String>,
        allow_binding: bool,
    ) -> Result<(), LoadError> {
        match constraint {
            Constraint::Variable(name, span) | Constraint::MultiVariable(name, span) => {
                if allow_binding {
                    available.insert(name.clone());
                } else {
                    Self::validate_lhs_variable(rule_name, name, span, available)?;
                }
            }
            Constraint::And(parts, _) => {
                for (index, part) in parts.iter().enumerate() {
                    self.validate_lhs_constraint(
                        rule_name,
                        part,
                        available,
                        allow_binding && index == 0,
                    )?;
                }
            }
            Constraint::Or(parts, _) => {
                let mut common: Option<HashSet<String>> = None;
                for part in parts {
                    let mut branch = available.clone();
                    self.validate_lhs_constraint(rule_name, part, &mut branch, false)?;
                    common = Some(match common {
                        None => branch,
                        Some(previous) => previous.intersection(&branch).cloned().collect(),
                    });
                }
                if let Some(common) = common {
                    available.extend(common);
                }
            }
            Constraint::Predicate(expr, _) | Constraint::ReturnValue(expr, _) => {
                self.validate_lhs_expression(rule_name, expr, available)?;
            }
            Constraint::Not(inner, _) => match inner.as_ref() {
                Constraint::Variable(name, span) | Constraint::MultiVariable(name, span) => {
                    Self::validate_lhs_variable(rule_name, name, span, available)?;
                }
                other => {
                    self.validate_lhs_constraint(rule_name, other, &mut available.clone(), false)?;
                }
            },
            Constraint::Literal(_) | Constraint::Wildcard(_) | Constraint::MultiWildcard(_) => {}
        }
        Ok(())
    }

    fn validate_lhs_variable(
        rule_name: &str,
        name: &str,
        span: &Span,
        available: &HashSet<String>,
    ) -> Result<(), LoadError> {
        if available.contains(name) {
            Ok(())
        } else {
            Err(Self::compile_error_at(span, &format!(
                "rule `{rule_name}` variable ?{name} is an unbound LHS variable at this source position"
            )))
        }
    }

    fn validate_lhs_expression(
        &self,
        rule_name: &str,
        expr: &SExpr,
        available: &HashSet<String>,
    ) -> Result<(), LoadError> {
        match expr {
            SExpr::Atom(Atom::SingleVar(name) | Atom::MultiVar(name), span) => {
                Self::validate_lhs_variable(rule_name, name, span, available)
            }
            SExpr::Atom(Atom::GlobalVar(name), span) => {
                self.validate_lhs_construct_reference(name, "defglobal", span)
            }
            SExpr::List(items, span) => {
                if let Some(SExpr::Atom(Atom::Symbol(name), _)) = items.first() {
                    if !crate::evaluator::is_builtin_callable(name) {
                        self.validate_lhs_construct_reference(name, "callable", span)?;
                    }
                }
                for argument in items.iter().skip(1) {
                    self.validate_lhs_expression(rule_name, argument, available)?;
                }
                Ok(())
            }
            SExpr::Atom(_, _) => Ok(()),
        }
    }

    fn lhs_construct_owners(&self, name: &str, kind: &str) -> Vec<(ModuleId, &'static str)> {
        if kind == "defglobal" {
            self.globals
                .modules_for_name(name)
                .into_iter()
                .map(|id| (id, "defglobal"))
                .collect()
        } else {
            self.functions
                .modules_for_name(name)
                .into_iter()
                .map(|id| (id, "deffunction"))
                .chain(
                    self.generics
                        .modules_for_name(name)
                        .into_iter()
                        .map(|id| (id, "defgeneric")),
                )
                .collect()
        }
    }

    fn validate_lhs_construct_reference(
        &self,
        raw_name: &str,
        kind: &str,
        span: &Span,
    ) -> Result<(), LoadError> {
        let qualified = parse_qualified_name(raw_name)
            .map_err(|message| Self::compile_error_at(span, &message))?;
        let (module, name) = match &qualified {
            QualifiedName::Unqualified(name) => (None, name.as_str()),
            QualifiedName::Qualified { module, name } => {
                let id = self.module_registry.get_by_name(module).ok_or_else(|| {
                    Self::compile_error_at(
                        span,
                        &format!("unknown module `{module}` in LHS expression"),
                    )
                })?;
                (Some(id), name.as_str())
            }
        };
        let current = self.module_registry.current_module();
        let owners = self.lhs_construct_owners(name, kind);
        let local = module.is_none() && owners.iter().any(|(id, _)| *id == current);
        let visible = owners
            .iter()
            .filter(|(id, construct)| {
                module.map_or(!local || *id == current, |requested| requested == *id)
                    && self
                        .module_registry
                        .is_construct_visible(current, *id, construct, name)
            })
            .count();
        if visible == 1 {
            Ok(())
        } else {
            let reason = if visible == 0 {
                "undefined or inaccessible"
            } else {
                "ambiguous"
            };
            Err(Self::compile_error_at(
                span,
                &format!("{reason} {kind} `{raw_name}` in LHS expression"),
            ))
        }
    }
}
