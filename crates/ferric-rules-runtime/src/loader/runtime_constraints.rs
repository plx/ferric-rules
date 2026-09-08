//! Detached source-order plans for fact-local and outer-dependent constraints.

use super::{
    Atom, CompilableCondition, CompilablePattern, CompiledTestCondition, Constraint, Engine,
    HashMap, HashSet, LiteralKind, LoadError, Pattern, RuleConstruct, SExpr, SlotIndex,
};
use crate::evaluator::RuntimeExpr;
use ferric_rules_core::{CompilableRuntimePattern, RuntimeConditionRole, Symbol};

pub(super) struct RuleConstraintPlan {
    pub available: HashSet<String>,
    pub conditions: Vec<CompiledTestCondition>,
    reserved: HashSet<String>,
    seed: usize,
}

impl RuleConstraintPlan {
    pub fn new(rule: &RuleConstruct) -> Self {
        let mut reserved = HashSet::new();
        for pattern in &rule.patterns {
            Engine::collect_pattern_binding_variables(pattern, &mut reserved);
        }
        Self {
            available: HashSet::new(),
            conditions: Vec::new(),
            reserved,
            seed: 0,
        }
    }

    fn add(&mut self, condition: CompiledTestCondition) -> Result<u32, LoadError> {
        let index = u32::try_from(self.conditions.len())
            .map_err(|_| LoadError::Compile("too many runtime conditions in one rule".into()))?;
        self.conditions.push(condition);
        Ok(index)
    }

    fn slot_name(&mut self) -> String {
        loop {
            let name = format!("__ferric_constraint_slot_{}", self.seed);
            self.seed += 1;
            if self.reserved.insert(name.clone()) {
                return name;
            }
        }
    }
}

struct PatternConstraints {
    pattern: CompilablePattern,
    local_bindings: Vec<(SlotIndex, Symbol)>,
    outer: HashSet<String>,
    first_local: HashMap<String, SlotIndex>,
    local: Vec<RuntimeExpr>,
    join: Vec<RuntimeExpr>,
    in_disjunction: bool,
    force_join: bool,
}

impl PatternConstraints {
    fn push(&mut self, expr: RuntimeExpr, join: bool) {
        if join || self.force_join {
            self.join.push(expr);
        } else {
            self.local.push(expr);
        }
    }

    fn needs_join(&self, constraint: &Constraint) -> bool {
        let outer_only = self
            .outer
            .iter()
            .filter(|name| !self.first_local.contains_key(*name))
            .cloned()
            .collect();
        match constraint {
            Constraint::Variable(name, _) | Constraint::MultiVariable(name, _) => {
                self.outer.contains(name)
            }
            Constraint::Predicate(expr, _) | Constraint::ReturnValue(expr, _) => {
                Engine::sexpr_references_any_variable(expr, &outer_only)
            }
            Constraint::Not(inner, _) => self.needs_join(inner),
            Constraint::And(parts, _) | Constraint::Or(parts, _) => {
                parts.iter().any(|part| self.needs_join(part))
            }
            _ => false,
        }
    }
}

fn call(name: &str, args: Vec<RuntimeExpr>) -> RuntimeExpr {
    RuntimeExpr::Call {
        name: name.into(),
        args,
        span: None,
    }
}

fn bound(name: &str) -> RuntimeExpr {
    RuntimeExpr::BoundVar {
        name: name.into(),
        span: None,
    }
}

fn conjunction(mut expressions: Vec<RuntimeExpr>) -> RuntimeExpr {
    if expressions.len() == 1 {
        expressions.pop().unwrap()
    } else {
        call("and", expressions)
    }
}

impl Engine {
    /// A leading CLIPS field binding belongs to every connected alternative.
    /// Lift it out of the parser's Or(And(binding,predicate),literal) shape.
    pub(super) fn field_constraint(source: &Constraint) -> Constraint {
        if let Constraint::Or(parts, span) = source {
            let mut alternatives = parts.clone();
            let binding = match alternatives.first_mut() {
                Some(Constraint::And(terms, _))
                    if matches!(
                        terms.first(),
                        Some(Constraint::Variable(..) | Constraint::MultiVariable(..))
                    ) =>
                {
                    Some(terms.remove(0))
                }
                _ => None,
            };
            if let Some(binding) = binding {
                return Constraint::And(vec![binding, Constraint::Or(alternatives, *span)], *span);
            }
        }
        source.clone()
    }

    pub(super) fn pattern_needs_runtime_disjunction(pattern: &Pattern) -> bool {
        fn expression(constraint: &Constraint) -> bool {
            match constraint {
                Constraint::Predicate(..) | Constraint::ReturnValue(..) => true,
                Constraint::And(parts, _) | Constraint::Or(parts, _) => {
                    parts.iter().any(expression)
                }
                Constraint::Not(inner, _) => expression(inner),
                _ => false,
            }
        }
        fn disjunction(constraint: &Constraint) -> bool {
            match constraint {
                Constraint::Or(..) => true,
                Constraint::And(parts, _) => parts.iter().any(disjunction),
                Constraint::Not(inner, _) => disjunction(inner),
                _ => false,
            }
        }
        let fields: Vec<_> = match pattern {
            Pattern::Ordered(pattern) => {
                if pattern
                    .constraints
                    .iter()
                    .any(Self::constraint_has_multifield)
                {
                    return false;
                }
                pattern.constraints.iter().collect()
            }
            Pattern::Template(pattern) => pattern
                .slot_constraints
                .iter()
                .map(|slot| &slot.constraint)
                .collect(),
            Pattern::Assigned { pattern, .. } => {
                return Self::pattern_needs_runtime_disjunction(pattern)
            }
            _ => return false,
        };
        fields.iter().any(|field| expression(field))
            && fields.iter().any(|field| disjunction(field))
    }

    pub(super) fn lower_lhs_condition(
        &mut self,
        source: &Pattern,
        plan: &mut RuleConstraintPlan,
        out: &mut Vec<CompilableCondition>,
    ) -> Result<(), LoadError> {
        match source {
            Pattern::Ordered(_) | Pattern::Template(_) => {
                self.lower_fact_constraints(source, false, &[], plan, out)?;
                Self::collect_pattern_binding_variables(source, &mut plan.available);
            }
            Pattern::Assigned {
                variable, pattern, ..
            } => {
                self.lower_lhs_condition(pattern, plan, out)?;
                if Self::is_positive_fact_pattern(pattern) {
                    plan.available.insert(variable.clone());
                }
            }
            Pattern::And(children, _) => {
                for child in children {
                    self.lower_lhs_condition(child, plan, out)?;
                }
            }
            Pattern::Test(expr, _) => {
                let expr = self.translate_lhs_expr(expr)?;
                let condition_index = plan.add(CompiledTestCondition::join(expr))?;
                out.push(CompilableCondition::Predicate { condition_index });
            }
            Pattern::Not(inner, _) => self.lower_negated_condition(inner, plan, out)?,
            Pattern::Exists(children, span) => {
                self.lower_exists_condition(children, span, plan, out)?;
            }
            Pattern::Forall(children, span) => {
                if children.len() != 2
                    || children
                        .iter()
                        .any(|child| !Self::is_positive_fact_pattern(child))
                {
                    return Err(Self::unsupported_pattern(
                        "forall",
                        span,
                        "forall requires exactly two simple fact patterns",
                    ));
                }
                let saved = plan.available.clone();
                let mut nested = Vec::new();
                self.lower_lhs_condition(&children[0], plan, &mut nested)?;
                self.lower_fact_constraints(&children[1], true, &[], plan, &mut nested)?;
                plan.available = saved;
                out.push(CompilableCondition::Ncc(nested));
            }
            Pattern::Logical(_, span) => {
                return Err(Self::unsupported_pattern(
                    "logical",
                    span,
                    "truth maintenance is not implemented",
                ))
            }
            Pattern::Or(_, span) => {
                return Err(Self::unsupported_pattern(
                    "or",
                    span,
                    "or CE must be expanded before compilation",
                ))
            }
        }
        Ok(())
    }

    fn lower_exists_condition(
        &mut self,
        children: &[Pattern],
        span: &super::Span,
        plan: &mut RuleConstraintPlan,
        out: &mut Vec<CompilableCondition>,
    ) -> Result<(), LoadError> {
        if children.is_empty() {
            return Err(Self::unsupported_pattern(
                "exists",
                span,
                "exists requires at least one inner pattern",
            ));
        }
        if children.len() == 1 && matches!(children[0], Pattern::Test(..)) {
            return self.lower_lhs_condition(&children[0], plan, out);
        }
        // The support-counted primitive path remains available when no
        // expression needs a runtime-owned decision.
        if children.len() == 1 && Self::is_positive_fact_pattern(&children[0]) {
            let mut residual = Vec::new();
            let mut simple =
                self.translate_pattern(&children[0], &mut residual, &mut plan.seed, false)?;
            if residual.is_empty() && !Self::pattern_needs_runtime_disjunction(&children[0]) {
                simple.exists = true;
                out.push(CompilableCondition::Pattern(simple));
                return Ok(());
            }
            // Invert one lazy negative node. Its selected support and
            // successor search have the same lifecycle as a blocker,
            // with the existential site's distinct error semantics.
            let mut nested = Vec::new();
            self.lower_fact_constraints(&children[0], true, &[], plan, &mut nested)?;
            for condition in &mut nested {
                if let CompilableCondition::RuntimePattern(pattern) = condition {
                    pattern.join_role = RuntimeConditionRole::ExistsJoin;
                    if let Some(index) = pattern.negative_condition {
                        plan.conditions[index as usize].role = RuntimeConditionRole::ExistsJoin;
                    }
                }
            }
            out.push(CompilableCondition::Ncc(nested));
            return Ok(());
        }
        let saved = plan.available.clone();
        let mut nested = Vec::new();
        for child in children {
            self.lower_lhs_condition(child, plan, &mut nested)?;
        }
        plan.available = saved;
        out.push(CompilableCondition::Ncc(vec![CompilableCondition::Ncc(
            nested,
        )]));
        Ok(())
    }

    fn lower_negated_condition(
        &mut self,
        inner: &Pattern,
        plan: &mut RuleConstraintPlan,
        out: &mut Vec<CompilableCondition>,
    ) -> Result<(), LoadError> {
        if Self::is_positive_fact_pattern(inner) {
            return self.lower_fact_constraints(inner, true, &[], plan, out);
        }
        match inner {
            Pattern::Test(expr, _) => {
                let expr = call("not", vec![self.translate_lhs_expr(expr)?]);
                let condition_index = plan.add(CompiledTestCondition::join(expr))?;
                out.push(CompilableCondition::Predicate { condition_index });
            }
            Pattern::Not(inner, span) => {
                return self.lower_lhs_condition(
                    &Pattern::Exists(vec![*inner.clone()], *span),
                    plan,
                    out,
                );
            }
            Pattern::Exists(children, span) => {
                // Absence of an existential tuple is absence of the tuple
                // itself; avoid nesting the primitive exists node in an NCC.
                return self.lower_negated_condition(
                    &Pattern::And(children.clone(), *span),
                    plan,
                    out,
                );
            }
            Pattern::And(children, span) => {
                if children.is_empty() {
                    return Err(Self::unsupported_pattern(
                        "not/and",
                        span,
                        "not(and ...) requires at least one inner pattern",
                    ));
                }
                if children
                    .iter()
                    .all(|child| matches!(child, Pattern::Test(..)))
                {
                    let expressions = children
                        .iter()
                        .filter_map(|child| {
                            if let Pattern::Test(expr, _) = child {
                                Some(expr)
                            } else {
                                None
                            }
                        })
                        .map(|expr| self.translate_lhs_expr(expr))
                        .collect::<Result<Vec<_>, _>>()?;
                    let expression = call("not", vec![conjunction(expressions)]);
                    let condition_index = plan.add(CompiledTestCondition::join(expression))?;
                    out.push(CompilableCondition::Predicate { condition_index });
                    return Ok(());
                }
                if Self::is_positive_fact_pattern(&children[0])
                    && children[1..]
                        .iter()
                        .all(|child| matches!(child, Pattern::Test(..)))
                {
                    let tests: Vec<_> = children[1..]
                        .iter()
                        .filter_map(|child| {
                            if let Pattern::Test(expr, _) = child {
                                Some(expr)
                            } else {
                                None
                            }
                        })
                        .collect();
                    return self.lower_fact_constraints(&children[0], true, &tests, plan, out);
                }
                let saved = plan.available.clone();
                let mut nested = Vec::new();
                for child in children {
                    self.lower_lhs_condition(child, plan, &mut nested)?;
                }
                plan.available = saved;
                out.push(CompilableCondition::Ncc(nested));
            }
            _ => {
                let saved = plan.available.clone();
                let mut nested = Vec::new();
                self.lower_lhs_condition(inner, plan, &mut nested)?;
                plan.available = saved;
                out.push(CompilableCondition::Ncc(nested));
            }
        }
        Ok(())
    }

    fn translate_lhs_expr(&mut self, source: &SExpr) -> Result<RuntimeExpr, LoadError> {
        crate::evaluator::from_sexpr(source, &mut self.symbol_table, &self.config)
            .map_err(|error| LoadError::Compile(format!("LHS expression translation: {error}")))
    }

    fn lower_fact_constraints(
        &mut self,
        source: &Pattern,
        negated: bool,
        trailing_tests: &[&SExpr],
        plan: &mut RuleConstraintPlan,
        out: &mut Vec<CompilableCondition>,
    ) -> Result<(), LoadError> {
        // Primitive translation produces an explicit residual list. It cannot
        // mutate the graph, and an actual translation failure remains an error.
        let mut residual = Vec::new();
        let mut pattern = self.translate_pattern(source, &mut residual, &mut plan.seed, negated)?;
        pattern.negated = negated;
        if residual.is_empty()
            && trailing_tests.is_empty()
            && !Self::pattern_needs_runtime_disjunction(source)
        {
            out.push(CompilableCondition::Pattern(pattern));
            return Ok(());
        }
        // Preserve the existing positive ordered-tail path until sequence
        // matching supplies physical selectors for runtime pattern filters.
        // Negated runtime constraints have no such supported legacy path.
        if !negated && Self::is_ordered_multifield_pattern(source) {
            out.push(CompilableCondition::Pattern(pattern));
            for expression in residual {
                let condition_index = plan.add(CompiledTestCondition::join(expression))?;
                out.push(CompilableCondition::Predicate { condition_index });
            }
            return Ok(());
        }
        // Once runtime evaluation is needed, retain source order within each
        // network. Moving later constants ahead of callbacks would hide effects
        // and errors, so the complete local/join groups own those decisions.
        pattern.constant_tests.clear();
        pattern.variable_slots.clear();
        pattern.negated_variable_slots.clear();
        let mut constraints = PatternConstraints {
            pattern,
            local_bindings: Vec::new(),
            outer: plan.available.clone(),
            first_local: HashMap::new(),
            local: Vec::new(),
            join: Vec::new(),
            in_disjunction: false,
            force_join: false,
        };
        let fields = self.runtime_pattern_fields(source)?;
        for (slot, constraint) in fields {
            let name = plan.slot_name();
            let symbol = self.compile_symbol(&name)?;
            constraints.pattern.variable_slots.push((slot, symbol));
            constraints.local_bindings.push((slot, symbol));
            self.lower_runtime_constraint(&constraint, slot, &name, &mut constraints)?;
        }
        for test in trailing_tests {
            constraints.join.push(self.translate_lhs_expr(test)?);
        }
        let test_count = constraints.local.len() + constraints.join.len();
        if test_count > ferric_rules_core::compiler::MAX_ALPHA_TESTS {
            return Err(LoadError::Compile(format!(
                "runtime pattern requires {test_count} tests, exceeding the supported limit of {}",
                ferric_rules_core::compiler::MAX_ALPHA_TESTS
            )));
        }
        let local_condition = if constraints.local.is_empty() {
            None
        } else {
            Some(plan.add(CompiledTestCondition::pattern(conjunction(
                constraints.local,
            )))?)
        };
        let join_expr = (!constraints.join.is_empty()).then(|| conjunction(constraints.join));
        let negative_condition = if negated {
            join_expr
                .clone()
                .map(|expr| plan.add(CompiledTestCondition::negative_join(expr)))
                .transpose()?
        } else {
            None
        };
        out.push(CompilableCondition::RuntimePattern(
            CompilableRuntimePattern {
                pattern: constraints.pattern,
                local_condition,
                local_bindings: if local_condition.is_some() {
                    constraints.local_bindings
                } else {
                    Vec::new()
                },
                negative_condition,
                join_role: RuntimeConditionRole::NegativeJoin,
            },
        ));
        if !negated {
            if let Some(expr) = join_expr {
                let condition_index = plan.add(CompiledTestCondition::join(expr))?;
                out.push(CompilableCondition::Predicate { condition_index });
            }
        }
        Ok(())
    }

    fn runtime_pattern_fields(
        &self,
        source: &Pattern,
    ) -> Result<Vec<(SlotIndex, Constraint)>, LoadError> {
        match source {
            Pattern::Assigned { pattern, .. } => self.runtime_pattern_fields(pattern),
            Pattern::Ordered(pattern) => {
                if pattern
                    .constraints
                    .iter()
                    .any(Self::constraint_has_multifield)
                {
                    return Err(Self::unsupported_pattern(
                        "multifield",
                        &pattern.span,
                        "runtime ordered constraints require fixed-width fields",
                    ));
                }
                Ok(pattern
                    .constraints
                    .iter()
                    .enumerate()
                    .map(|(index, constraint)| {
                        (
                            SlotIndex::Ordered(index),
                            Self::field_constraint(constraint),
                        )
                    })
                    .collect())
            }
            Pattern::Template(pattern) => {
                let template_id = self
                    .resolve_template_reference(
                        &pattern.template,
                        self.module_registry.current_module(),
                    )
                    .map_err(|message| Self::compile_error_at(&pattern.span, &message))?;
                let registered = &self.template_defs[template_id];
                pattern
                    .slot_constraints
                    .iter()
                    .map(|slot| {
                        let index = registered.slot_index(&slot.slot_name).ok_or_else(|| {
                            Self::compile_error_at(&slot.span, "unknown template slot")
                        })?;
                        if registered.slot_types[index] == ferric_rules_parser::SlotType::Multi
                            && !Self::is_whole_multislot_constraint(&slot.constraint)
                        {
                            return Err(Self::unsupported_constraint(
                                "multislot",
                                &slot.span,
                                "runtime multislot constraints require a whole-slot multifield capture and predicates; scalar and sequence constraints require sequence matching",
                            ));
                        }
                        Ok((
                            SlotIndex::Template(index),
                            Self::field_constraint(&slot.constraint),
                        ))
                    })
                    .collect()
            }
            _ => unreachable!("only simple fact patterns have runtime fields"),
        }
    }

    fn is_whole_multislot_constraint(source: &Constraint) -> bool {
        fn whole_terms(constraint: &Constraint) -> bool {
            match constraint {
                Constraint::MultiVariable(..)
                | Constraint::MultiWildcard(_)
                | Constraint::Predicate(..) => true,
                Constraint::And(parts, _) | Constraint::Or(parts, _) => {
                    parts.iter().all(whole_terms)
                }
                Constraint::Not(inner, _) => {
                    matches!(inner.as_ref(), Constraint::MultiVariable(..))
                }
                _ => false,
            }
        }
        let constraint = Self::field_constraint(source);
        let first = match &constraint {
            Constraint::And(parts, _) => parts.first(),
            other => Some(other),
        };
        matches!(
            first,
            Some(Constraint::MultiVariable(..) | Constraint::MultiWildcard(_))
        ) && whole_terms(&constraint)
    }

    fn constraint_has_multifield(constraint: &Constraint) -> bool {
        match constraint {
            Constraint::MultiVariable(..) | Constraint::MultiWildcard(_) => true,
            Constraint::And(parts, _) | Constraint::Or(parts, _) => {
                parts.iter().any(Self::constraint_has_multifield)
            }
            Constraint::Not(inner, _) => Self::constraint_has_multifield(inner),
            _ => false,
        }
    }

    fn is_ordered_multifield_pattern(pattern: &Pattern) -> bool {
        match pattern {
            Pattern::Assigned { pattern, .. } => Self::is_ordered_multifield_pattern(pattern),
            Pattern::Ordered(pattern) => pattern
                .constraints
                .iter()
                .any(Self::constraint_has_multifield),
            _ => false,
        }
    }

    fn lower_runtime_constraint(
        &mut self,
        constraint: &Constraint,
        slot: SlotIndex,
        slot_name: &str,
        plan: &mut PatternConstraints,
    ) -> Result<(), LoadError> {
        match constraint {
            Constraint::And(parts, _) => {
                for part in parts {
                    self.lower_runtime_constraint(part, slot, slot_name, plan)?;
                }
            }
            Constraint::Variable(name, _) | Constraint::MultiVariable(name, _) => {
                if plan.outer.contains(name) && plan.in_disjunction {
                    plan.push(call("eq", vec![bound(slot_name), bound(name)]), true);
                } else if plan.outer.contains(name) {
                    // Equality joins also supply CLIPS candidate hashing, even
                    // when the equality is written after a runtime callback.
                    let symbol = self.compile_symbol(name)?;
                    plan.pattern.variable_slots.push((slot, symbol));
                    if !plan.first_local.contains_key(name) {
                        plan.first_local.insert(name.clone(), slot);
                        plan.local_bindings.push((slot, symbol));
                    }
                } else if plan.first_local.contains_key(name) {
                    plan.push(call("eq", vec![bound(slot_name), bound(name)]), false);
                } else {
                    plan.first_local.insert(name.clone(), slot);
                    let symbol = self.compile_symbol(name)?;
                    plan.pattern.variable_slots.push((slot, symbol));
                    plan.local_bindings.push((slot, symbol));
                }
            }
            Constraint::Predicate(expr, _) | Constraint::ReturnValue(expr, _) => {
                let mut translated = self.translate_lhs_expr(expr)?;
                if matches!(constraint, Constraint::ReturnValue(..)) {
                    translated = call("eq", vec![bound(slot_name), translated]);
                }
                let outer_only = plan
                    .outer
                    .iter()
                    .filter(|name| !plan.first_local.contains_key(*name))
                    .cloned()
                    .collect();
                plan.push(
                    translated,
                    Self::sexpr_references_any_variable(expr, &outer_only),
                );
            }
            Constraint::Literal(literal) => {
                let expr = self.translate_lhs_literal(literal)?;
                plan.push(call("eq", vec![bound(slot_name), expr]), false);
            }
            Constraint::Not(inner, span) => match inner.as_ref() {
                Constraint::Variable(name, _) | Constraint::MultiVariable(name, _) => {
                    let expr = call("neq", vec![bound(slot_name), bound(name)]);
                    if plan.outer.contains(name) && !plan.first_local.contains_key(name) {
                        plan.push(expr, true);
                    } else {
                        plan.push(expr, false);
                    }
                }
                Constraint::Literal(_) => {
                    self.lower_runtime_constraint(inner, slot, slot_name, plan)?;
                    let expressions = if plan.force_join {
                        &mut plan.join
                    } else {
                        &mut plan.local
                    };
                    let expression = expressions.pop().expect("literal generates one test");
                    expressions.push(call("not", vec![expression]));
                }
                Constraint::Wildcard(_) | Constraint::MultiWildcard(_) => {}
                _ => {
                    return Err(Self::unsupported_constraint(
                        "not",
                        span,
                        "only negated literals and variables are supported",
                    ))
                }
            },
            Constraint::Or(parts, _) => {
                let previous_local = std::mem::take(&mut plan.local);
                let previous_join = std::mem::take(&mut plan.join);
                let previous_disjunction = std::mem::replace(&mut plan.in_disjunction, true);
                let mut alternatives = Vec::new();
                let needs_join = plan.force_join || parts.iter().any(|part| plan.needs_join(part));
                let previous_force_join = std::mem::replace(&mut plan.force_join, needs_join);
                for part in parts {
                    self.lower_runtime_constraint(part, slot, slot_name, plan)?;
                    let mut expressions = std::mem::take(&mut plan.local);
                    expressions.extend(std::mem::take(&mut plan.join));
                    alternatives.push(conjunction(expressions));
                }
                plan.local = previous_local;
                plan.join = previous_join;
                plan.in_disjunction = previous_disjunction;
                plan.force_join = previous_force_join;
                let expr = call("or", alternatives);
                if needs_join {
                    plan.push(expr, true);
                } else {
                    plan.push(expr, false);
                }
            }
            Constraint::Wildcard(_) | Constraint::MultiWildcard(_) => {}
        }
        Ok(())
    }

    fn translate_lhs_literal(
        &mut self,
        literal: &ferric_rules_parser::LiteralValue,
    ) -> Result<RuntimeExpr, LoadError> {
        let atom = match &literal.value {
            LiteralKind::Integer(value) => Atom::Integer(*value),
            LiteralKind::Float(value) => Atom::Float(*value),
            LiteralKind::String(value) => Atom::String(value.clone()),
            LiteralKind::Symbol(value) => Atom::Symbol(value.clone()),
        };
        self.translate_lhs_expr(&SExpr::Atom(atom, literal.span))
    }
}
