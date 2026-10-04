//! Declare ordered relations and resolve literal query targets in source order.
//!
//! Parsing an ordered pattern/assertion makes its relation available before its
//! fields. A later declaration, including one in an unexecuted branch, must not
//! rescue an earlier query reference in the same construct.

use ferric_rules_parser::{
    interpret_action_expr, ActionExpr, Constraint, FactBody, FactValue, FunctionCall,
    InterpretError, LiteralKind, Pattern, RuleConstruct, SExpr, Span,
};

use crate::loader::LoadError;
use crate::modules::ModuleId;
use crate::Engine;

struct Event {
    span: Span,
    kind: EventKind,
}

enum EventKind {
    DeclareOrdered(String),
    QueryLiteral(String),
    InvalidExpression(InterpretError),
}

impl Engine {
    pub(crate) fn declare_rule_query_order(
        &mut self,
        rule: &RuleConstruct,
        module: ModuleId,
    ) -> Result<(), LoadError> {
        let mut events = Vec::new();
        let mut patterns: Vec<_> = rule.patterns.iter().collect();
        let mut constraints = Vec::new();
        while let Some(pattern) = patterns.pop() {
            match pattern {
                Pattern::Ordered(pattern) => {
                    events.push(Event {
                        span: pattern.span,
                        kind: EventKind::DeclareOrdered(pattern.relation.clone()),
                    });
                    constraints.extend(&pattern.constraints);
                }
                Pattern::Template(pattern) => constraints.extend(
                    pattern
                        .slot_constraints
                        .iter()
                        .flat_map(|slot| &slot.constraints),
                ),
                Pattern::Test(expression, _) => {
                    self.collect_raw_query_events(expression, module, &mut events);
                }
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
                    self.collect_raw_query_events(expression, module, &mut events);
                }
                Constraint::Not(inner, _) => constraints.push(inner),
                Constraint::And(children, _) | Constraint::Or(children, _) => {
                    constraints.extend(children);
                }
                _ => {}
            }
        }
        for action in &rule.actions {
            self.collect_call_query_events(&action.call, module, &mut events);
        }
        // Salience is checked/evaluated independently before the rule's LHS.
        self.apply_query_source_events(events, module)
    }

    pub(crate) fn declare_expression_query_order<'a>(
        &mut self,
        expressions: impl IntoIterator<Item = &'a ActionExpr>,
        module: ModuleId,
    ) -> Result<(), LoadError> {
        let mut events = Vec::new();
        for expression in expressions {
            self.collect_expression_query_events(expression, module, &mut events);
        }
        self.apply_query_source_events(events, module)
    }

    pub(crate) fn declare_facts_query_order(
        &mut self,
        facts: &[FactBody],
        module: ModuleId,
    ) -> Result<(), LoadError> {
        let mut events = Vec::new();
        for fact in facts {
            match fact {
                FactBody::Ordered(fact) => {
                    events.push(Event {
                        span: fact.span,
                        kind: EventKind::DeclareOrdered(fact.relation.clone()),
                    });
                    self.collect_fact_value_query_events(&fact.values, module, &mut events);
                }
                FactBody::Template(fact) => {
                    events.push(Event {
                        span: fact.span,
                        kind: EventKind::DeclareOrdered(fact.template.clone()),
                    });
                    let explicit = self.resolve_template_id(&fact.template, module).is_ok();
                    for slot in &fact.slot_values {
                        if explicit {
                            self.collect_fact_value_query_events(&slot.values, module, &mut events);
                        } else if let Some(expression) = &slot.ordered_expression {
                            self.collect_expression_query_events(expression, module, &mut events);
                        }
                    }
                }
            }
        }
        self.apply_query_source_events(events, module)
    }

    fn apply_query_source_events(
        &mut self,
        mut events: Vec<Event>,
        module: ModuleId,
    ) -> Result<(), LoadError> {
        events.sort_by_key(|event| event.span.start.offset);
        for event in events {
            match event.kind {
                EventKind::DeclareOrdered(name) => self.declare_implicit_template(&name, module),
                EventKind::QueryLiteral(name) => {
                    self.query_reference(&name, module)
                        .map_err(|message| Self::compile_error_at(&event.span, &message))?;
                }
                EventKind::InvalidExpression(error) => return Err(LoadError::Interpret(error)),
            }
        }
        Ok(())
    }

    fn collect_raw_query_events(&self, raw: &SExpr, module: ModuleId, events: &mut Vec<Event>) {
        match interpret_action_expr(raw) {
            Ok(expression) => self.collect_expression_query_events(&expression, module, events),
            Err(error) => events.push(Event {
                span: error.span,
                kind: EventKind::InvalidExpression(error),
            }),
        }
    }

    fn collect_expression_query_events(
        &self,
        expression: &ActionExpr,
        module: ModuleId,
        events: &mut Vec<Event>,
    ) {
        let mut pending = vec![expression];
        while let Some(expression) = pending.pop() {
            match expression {
                ActionExpr::FunctionCall(call) => {
                    Self::collect_call_declaration_events(call, events);
                    pending.extend(crate::effects::evaluated_arguments(self, module, call));
                }
                ActionExpr::QueryAction { bindings, .. } => {
                    for restriction in bindings.iter().flat_map(|binding| &binding.restrictions) {
                        if let ActionExpr::Literal(literal) = restriction {
                            if let LiteralKind::Symbol(name) = &literal.value {
                                events.push(Event {
                                    span: literal.span,
                                    kind: EventKind::QueryLiteral(name.clone()),
                                });
                            }
                        }
                    }
                    expression.push_children(&mut pending);
                }
                _ => expression.push_children(&mut pending),
            }
        }
    }

    fn collect_call_query_events(
        &self,
        call: &FunctionCall,
        module: ModuleId,
        events: &mut Vec<Event>,
    ) {
        Self::collect_call_declaration_events(call, events);
        for expression in crate::effects::evaluated_arguments(self, module, call) {
            self.collect_expression_query_events(expression, module, events);
        }
    }

    fn collect_call_declaration_events(call: &FunctionCall, events: &mut Vec<Event>) {
        if call.name == "assert" {
            for argument in &call.args {
                if let ActionExpr::FunctionCall(fact) = argument {
                    events.push(Event {
                        span: fact.span,
                        kind: EventKind::DeclareOrdered(fact.name.clone()),
                    });
                }
            }
        }
    }

    fn collect_fact_value_query_events(
        &self,
        values: &[FactValue],
        module: ModuleId,
        events: &mut Vec<Event>,
    ) {
        for value in values {
            if let FactValue::Expression(expression) = value {
                self.collect_expression_query_events(expression, module, events);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use ferric_rules_parser::{
        interpret_constructs, parse_sexprs, Construct, FileId, InterpreterConfig,
    };

    use super::*;
    use crate::EngineConfig;

    fn construct(source: &str) -> Construct {
        let parsed = parse_sexprs(source, FileId(0));
        assert!(parsed.errors.is_empty(), "{:?}", parsed.errors);
        let mut interpreted = interpret_constructs(&parsed.exprs, &InterpreterConfig::default());
        assert!(interpreted.errors.is_empty(), "{:?}", interpreted.errors);
        interpreted.constructs.remove(0)
    }

    #[test]
    fn literal_targets_observe_lexical_pattern_action_and_branch_order() {
        // Pinned CLIPS 6.30 source-order probes reject the first four forms and
        // accept their reversed variants, even an unexecuted asserting branch.
        for (source, valid) in [
            (
                "(defrule r (test (any-factp ((?f item)) TRUE)) (item ?) =>)",
                false,
            ),
            (
                "(defrule r => (any-factp ((?f item)) TRUE) (assert (item)))",
                false,
            ),
            (
                "(defrule r => (if TRUE then (any-factp ((?f item)) TRUE) else (assert (item))))",
                false,
            ),
            (
                "(defrule r (or (test (any-factp ((?f item)) TRUE)) (item)) =>)",
                false,
            ),
            (
                "(defrule r (item ?) (test (any-factp ((?f item)) TRUE)) =>)",
                true,
            ),
            (
                "(defrule r => (assert (item)) (any-factp ((?f item)) TRUE))",
                true,
            ),
            (
                "(defrule r => (if FALSE then (assert (item)) else (any-factp ((?f item)) TRUE)))",
                true,
            ),
            (
                "(defrule r (or (item) (test (any-factp ((?f item)) TRUE))) =>)",
                true,
            ),
        ] {
            let Construct::Rule(rule) = construct(source) else {
                panic!("expected rule")
            };
            let mut engine = Engine::new(EngineConfig::default());
            let module = engine.module_registry.main_module_id();
            let result = engine.declare_rule_query_order(&rule, module);
            assert_eq!(result.is_ok(), valid, "{source}: {result:?}");
        }
    }

    #[test]
    fn own_assert_relation_is_declared_before_initializer_queries() {
        let Construct::Function(function) =
            construct("(deffunction f () (assert (item (any-factp ((?f item)) TRUE))))")
        else {
            panic!("expected function")
        };
        let mut engine = Engine::new(EngineConfig::default());
        let module = engine.module_registry.main_module_id();
        engine
            .declare_expression_query_order(&function.body, module)
            .unwrap();
        assert!(engine.has_implicit_template("item", module));
    }

    #[test]
    fn nested_dynamic_symbols_are_not_load_time_references() {
        let Construct::Function(function) = construct(
            "(deffunction f () (any-factp ((?f (create$ later) (if TRUE then later else other))) TRUE))",
        ) else { panic!("expected function") };
        let mut engine = Engine::new(EngineConfig::default());
        let module = engine.module_registry.main_module_id();
        engine
            .declare_expression_query_order(&function.body, module)
            .unwrap();
        assert!(!engine.has_implicit_template("later", module));
        assert!(!engine.has_implicit_template("other", module));
    }

    #[test]
    fn nested_queries_in_restrictions_still_validate_literal_targets() {
        let Construct::Function(function) = construct(
            "(deffunction f () (any-factp ((?f (if (any-factp ((?g missing)) TRUE) then item else other))) TRUE))",
        ) else { panic!("expected function") };
        let mut engine = Engine::new(EngineConfig::default());
        let module = engine.module_registry.main_module_id();
        let error = engine
            .declare_expression_query_order(&function.body, module)
            .unwrap_err();
        assert!(error.to_string().contains("missing"));
    }

    #[test]
    fn failure_preserves_prior_declarations_and_reports_query_location() {
        let Construct::Rule(rule) = construct(
            "(defrule r\n (known)\n => (any-factp ((?f missing)) TRUE) (assert (later)))",
        ) else {
            panic!("expected rule")
        };
        let mut engine = Engine::new(EngineConfig::default());
        let module = engine.module_registry.main_module_id();
        let error = engine
            .declare_rule_query_order(&rule, module)
            .unwrap_err()
            .to_string();
        assert!(error.contains("missing"), "{error}");
        assert!(error.contains("line 3"), "{error}");
        assert!(engine.has_implicit_template("known", module));
        assert!(!engine.has_implicit_template("later", module));
    }

    #[test]
    fn deffacts_relations_precede_their_fields_but_not_earlier_facts() {
        for (source, valid) in [
            ("(deffacts d (item (any-factp ((?f item)) TRUE)))", true),
            (
                "(deffacts d (before (any-factp ((?f later)) TRUE)) (later))",
                false,
            ),
            (
                "(deffacts d (before) (later (any-factp ((?f before)) TRUE)))",
                true,
            ),
        ] {
            let Construct::Facts(facts) = construct(source) else {
                panic!("expected facts")
            };
            let mut engine = Engine::new(EngineConfig::default());
            let module = engine.module_registry.main_module_id();
            let result = engine.declare_facts_query_order(&facts.facts, module);
            assert_eq!(result.is_ok(), valid, "{source}: {result:?}");
        }
    }
}
