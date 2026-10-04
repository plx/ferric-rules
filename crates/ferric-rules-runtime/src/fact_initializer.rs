//! Compiled fact initializers shared by source assertions and dormant deffacts.

use std::collections::HashSet;

use ferric_rules_core::{Fact, OrderedFact, Symbol, TemplateFact, TemplateId, Value};
use ferric_rules_parser::{
    interpret_action_expr, ActionExpr, FactBody, FactValue, FunctionCall, SExpr, SlotType, Span,
};

use crate::engine::Engine;
use crate::evaluator::{self, EvalContext, RuntimeExpr};
use crate::loader::{LoadError, TemplateLookupError, TemplateResolver};
use crate::modules::ModuleId;

/// Fact identity is fixed when a definition is registered. Values are evaluated
/// against the current globals and callable definitions whenever it is asserted.
#[derive(Clone, Debug)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub(crate) enum PreparedFact {
    Ordered {
        relation: Symbol,
        fields: Vec<RuntimeExpr>,
    },
    Template {
        template_id: TemplateId,
        slots: Vec<(usize, Vec<RuntimeExpr>)>,
    },
}

impl PreparedFact {
    pub(crate) fn expressions(&self) -> impl Iterator<Item = &RuntimeExpr> {
        let (ordered, slots): (&[RuntimeExpr], &[(usize, Vec<RuntimeExpr>)]) = match self {
            Self::Ordered { fields, .. } => (fields, &[]),
            Self::Template { slots, .. } => (&[], slots),
        };
        ordered
            .iter()
            .chain(slots.iter().flat_map(|(_, fields)| fields))
    }

    pub(crate) fn all_expressions(&self) -> RuntimeExpressions<'_> {
        RuntimeExpressions {
            pending: self.expressions().collect(),
        }
    }
}

/// Iterative traversal also visits dormant compiled branches. No initializer is
/// executed while checking dependencies or unsupported snapshot values.
pub(crate) struct RuntimeExpressions<'a> {
    pending: Vec<&'a RuntimeExpr>,
}

impl<'a> RuntimeExpressions<'a> {
    #[cfg(feature = "serde")]
    pub(crate) fn new(root: &'a RuntimeExpr) -> Self {
        Self {
            pending: vec![root],
        }
    }
}

impl<'a> Iterator for RuntimeExpressions<'a> {
    type Item = &'a RuntimeExpr;

    fn next(&mut self) -> Option<Self::Item> {
        let expression = self.pending.pop()?;
        let mut branches = Vec::new();
        match expression {
            RuntimeExpr::Literal(_)
            | RuntimeExpr::BoundVar { .. }
            | RuntimeExpr::GlobalVar { .. } => {}
            RuntimeExpr::Call { args, .. } => self.pending.extend(args),
            RuntimeExpr::If {
                condition,
                then_branch,
                else_branch,
                ..
            } => {
                self.pending.push(condition);
                branches.extend([then_branch, else_branch]);
            }
            RuntimeExpr::While {
                condition, body, ..
            } => {
                self.pending.push(condition);
                branches.push(body);
            }
            RuntimeExpr::LoopForCount {
                start, end, body, ..
            } => {
                self.pending.extend([start.as_ref(), end.as_ref()]);
                branches.push(body);
            }
            RuntimeExpr::Progn {
                list_expr, body, ..
            } => {
                self.pending.push(list_expr);
                branches.push(body);
            }
            RuntimeExpr::QueryAction { query, body, .. } => {
                self.pending.push(query);
                branches.push(body);
            }
            RuntimeExpr::Switch {
                expr,
                cases,
                default,
                ..
            } => {
                self.pending.push(expr);
                for (value, body) in cases {
                    self.pending.push(value);
                    branches.push(body);
                }
                branches.extend(default.iter());
            }
        }
        for branch in branches {
            self.pending
                .extend(branch.iter().filter_map(|(_, runtime)| runtime.as_deref()));
        }
        Some(expression)
    }
}

fn invalid_at(span: Span, message: &str) -> LoadError {
    LoadError::Compile(format!(
        "{message} at line {}, column {}",
        span.start.line, span.start.column
    ))
}

fn expression_span(expression: &ActionExpr) -> Span {
    match expression {
        ActionExpr::Literal(literal) => literal.span,
        ActionExpr::FunctionCall(call) => call.span,
        ActionExpr::Variable(_, span)
        | ActionExpr::GlobalVariable(_, span)
        | ActionExpr::If { span, .. }
        | ActionExpr::While { span, .. }
        | ActionExpr::LoopForCount { span, .. }
        | ActionExpr::Progn { span, .. }
        | ActionExpr::QueryAction { span, .. }
        | ActionExpr::Switch { span, .. } => *span,
    }
}

fn fact_expression(value: &FactValue, literal_only: bool) -> Result<ActionExpr, LoadError> {
    match value {
        FactValue::Literal(literal) => Ok(ActionExpr::Literal(literal.clone())),
        FactValue::Variable(name, span) if !literal_only => {
            Ok(ActionExpr::Variable(name.clone(), *span))
        }
        FactValue::GlobalVariable(name, span) if !literal_only => {
            Ok(ActionExpr::GlobalVariable(name.clone(), *span))
        }
        FactValue::Expression(expression) if !literal_only => Ok((**expression).clone()),
        FactValue::EmptyMultifield(span) => Ok(ActionExpr::FunctionCall(FunctionCall {
            name: "create$".to_owned(),
            args: Vec::new(),
            span: *span,
        })),
        FactValue::Variable(_, span) | FactValue::GlobalVariable(_, span) => Err(invalid_at(
            *span,
            "load-facts requires literal field values",
        )),
        FactValue::Expression(expression) => Err(invalid_at(
            expression_span(expression),
            "load-facts requires literal field values",
        )),
    }
}

impl Engine {
    pub(crate) fn prepare_fact_body(
        &mut self,
        body: &FactBody,
        literal_only: bool,
    ) -> Result<PreparedFact, LoadError> {
        let module = self.module_registry.current_module();
        let mut locals = HashSet::new();
        match body {
            FactBody::Ordered(fact) => {
                if let Some(template_id) = self.initializer_template(&fact.relation, module)? {
                    if !fact.values.is_empty() {
                        return Err(invalid_at(fact.span, "template facts require named slots"));
                    }
                    return self.prepare_template_initializer(
                        template_id,
                        &[],
                        module,
                        &mut locals,
                        false,
                    );
                }
                let fields = fact
                    .values
                    .iter()
                    .map(|value| fact_expression(value, literal_only))
                    .collect::<Result<Vec<_>, _>>()?;
                self.prepare_ordered_initializer(
                    &fact.relation,
                    &fields,
                    module,
                    &mut locals,
                    false,
                )
            }
            FactBody::Template(fact) => {
                if let Some(template_id) = self.initializer_template(&fact.template, module)? {
                    let slots = fact
                        .slot_values
                        .iter()
                        .map(|slot| {
                            Ok(FunctionCall {
                                name: slot.name.clone(),
                                args: slot
                                    .values
                                    .iter()
                                    .map(|value| fact_expression(value, literal_only))
                                    .collect::<Result<Vec<_>, LoadError>>()?,
                                span: slot.span,
                            })
                        })
                        .collect::<Result<Vec<_>, LoadError>>()?;
                    self.prepare_template_initializer(
                        template_id,
                        &slots,
                        module,
                        &mut locals,
                        false,
                    )
                } else {
                    let fields = fact
                        .slot_values
                        .iter()
                        .map(|slot| {
                            if literal_only {
                                return Err(invalid_at(
                                    slot.span,
                                    "load-facts requires literal field values",
                                ));
                            }
                            slot.ordered_expression.as_deref().cloned().ok_or_else(|| {
                                invalid_at(
                                    slot.span,
                                    "invalid expression in ordered fact initializer",
                                )
                            })
                        })
                        .collect::<Result<Vec<_>, _>>()?;
                    self.prepare_ordered_initializer(
                        &fact.template,
                        &fields,
                        module,
                        &mut locals,
                        false,
                    )
                }
            }
        }
    }

    /// Source assertions retain raw slot wrappers so slots named `if`, for
    /// example, do not pass through special-form expression interpretation.
    pub(crate) fn prepare_assertion(
        &mut self,
        expression: &SExpr,
        locals: &mut HashSet<String>,
    ) -> Result<PreparedFact, LoadError> {
        let fields = expression
            .as_list()
            .filter(|fields| !fields.is_empty())
            .ok_or_else(|| LoadError::InvalidAssert("expected nonempty fact list".to_owned()))?;
        let name = fields[0]
            .as_symbol()
            .ok_or_else(|| LoadError::InvalidAssert("fact relation must be a symbol".to_owned()))?;
        let module = self.module_registry.current_module();
        if let Some(template_id) = self.initializer_template(name, module)? {
            let slots = fields[1..]
                .iter()
                .map(|slot| {
                    let values = slot
                        .as_list()
                        .filter(|values| !values.is_empty())
                        .ok_or_else(|| invalid_at(slot.span(), "expected named slot list"))?;
                    let name = values[0].as_symbol().ok_or_else(|| {
                        invalid_at(values[0].span(), "slot name must be a symbol")
                    })?;
                    Ok(FunctionCall {
                        name: name.to_owned(),
                        args: values[1..]
                            .iter()
                            .map(interpret_action_expr)
                            .collect::<Result<Vec<_>, _>>()
                            .map_err(LoadError::Interpret)?,
                        span: slot.span(),
                    })
                })
                .collect::<Result<Vec<_>, LoadError>>()?;
            self.prepare_template_initializer(template_id, &slots, module, locals, true)
        } else {
            let values = fields[1..]
                .iter()
                .map(interpret_action_expr)
                .collect::<Result<Vec<_>, _>>()
                .map_err(LoadError::Interpret)?;
            self.prepare_ordered_initializer(name, &values, module, locals, true)
        }
    }

    fn initializer_template(
        &self,
        name: &str,
        module: ModuleId,
    ) -> Result<Option<TemplateId>, LoadError> {
        match self.resolve_template_id(name, module) {
            Ok(id) => Ok(Some(id)),
            Err(TemplateLookupError::Unknown) => Ok(None),
            Err(_) => Err(LoadError::Compile(
                self.resolve_template_reference(name, module).unwrap_err(),
            )),
        }
    }

    fn prepare_field(
        &mut self,
        expression: &ActionExpr,
        module: ModuleId,
        locals: &mut HashSet<String>,
        allow_local_reads: bool,
    ) -> Result<RuntimeExpr, LoadError> {
        Self::validate_fact_initializer_bindings(expression, locals, allow_local_reads)?;
        crate::callable_validation::validate_iterator_binds(std::slice::from_ref(expression))
            .map_err(|(span, message)| invalid_at(span, &message))?;
        crate::callable_validation::validate_breaks(std::slice::from_ref(expression))
            .map_err(|(span, message)| invalid_at(span, &message))?;
        self.validate_expression_query_declarations(expression, module, None)?;
        self.validate_action_expr_as_expression(
            expression,
            module,
            "fact initializer",
            &HashSet::new(),
        )?;
        evaluator::from_action_expr(expression, &mut self.symbol_table, &self.config)
            .map_err(|error| LoadError::Compile(format!("fact initializer: {error}")))
    }

    fn prepare_ordered_initializer(
        &mut self,
        name: &str,
        fields: &[ActionExpr],
        module: ModuleId,
        locals: &mut HashSet<String>,
        allow_local_reads: bool,
    ) -> Result<PreparedFact, LoadError> {
        let relation = self
            .symbol_table
            .intern_symbol(name, self.config.string_encoding)
            .map_err(|error| LoadError::Engine(error.into()))?;
        let fields = fields
            .iter()
            .map(|expression| self.prepare_field(expression, module, locals, allow_local_reads))
            .collect::<Result<Vec<_>, _>>()?;
        Ok(PreparedFact::Ordered { relation, fields })
    }

    fn prepare_template_initializer(
        &mut self,
        template_id: TemplateId,
        slots: &[FunctionCall],
        module: ModuleId,
        locals: &mut HashSet<String>,
        allow_local_reads: bool,
    ) -> Result<PreparedFact, LoadError> {
        let template = self.template_defs[template_id].clone();
        let overrides: Vec<_> = slots
            .iter()
            .cloned()
            .map(ActionExpr::FunctionCall)
            .collect();
        let validated = template
            .slot_overrides(&overrides)
            .map_err(LoadError::Compile)?;
        let assigned: HashSet<_> = validated.iter().map(|(index, _)| *index).collect();
        for (index, default) in template.defaults.iter().enumerate() {
            if !assigned.contains(&index) {
                template
                    .validate_slot(index, default)
                    .map_err(LoadError::Compile)?;
            }
        }
        let slots = validated
            .into_iter()
            .map(|(index, slot)| {
                if template.slot_types[index] == SlotType::Single
                    && matches!(&slot.args[0], ActionExpr::FunctionCall(call) if call.name == "create$") {
                    return Err(invalid_at(slot.span, &format!(
                        "single-field slot `{}` in template `{}` requires one scalar value",
                        slot.name, template.name)));
                }
                let fields = slot
                    .args
                    .iter()
                    .map(|expression| self.prepare_field(expression, module, locals, allow_local_reads))
                    .collect::<Result<Vec<_>, _>>()?;
                Ok((index, fields))
            })
            .collect::<Result<Vec<_>, LoadError>>()?;
        Ok(PreparedFact::Template { template_id, slots })
    }

    /// Flush evaluation output before returning either a value or an error.
    /// A failed initializer never publishes a partially assembled fact.
    pub(crate) fn evaluate_prepared_fact(
        &mut self,
        fact: &PreparedFact,
        module: ModuleId,
    ) -> Result<Fact, String> {
        self.evaluate_prepared_fact_with_locals(
            fact,
            module,
            &mut evaluator::CallableLocals::default(),
        )
    }

    pub(crate) fn evaluate_prepared_fact_with_locals(
        &mut self,
        fact: &PreparedFact,
        module: ModuleId,
        locals: &mut evaluator::CallableLocals,
    ) -> Result<Fact, String> {
        let result = self.evaluate_prepared_fact_inner(fact, module, locals);
        for (channel, text) in self.globals.take_printout_events() {
            self.router.write(&channel, &text);
        }
        result
    }

    fn evaluate_prepared_fact_inner(
        &mut self,
        fact: &PreparedFact,
        module: ModuleId,
        locals: &mut evaluator::CallableLocals,
    ) -> Result<Fact, String> {
        let template = match fact {
            PreparedFact::Template { template_id, .. } => Some(
                self.template_defs
                    .get(*template_id)
                    .cloned()
                    .ok_or_else(|| "fact initializer references an unknown template".to_owned())?,
            ),
            PreparedFact::Ordered { .. } => None,
        };
        let bindings = ferric_rules_core::binding::BindingSet::new();
        let var_map = ferric_rules_core::binding::VarMap::new();
        let mut ctx = EvalContext {
            bindings: &bindings,
            var_map: &var_map,
            callable_locals: Some(locals),
            symbol_table: &mut self.symbol_table,
            config: &self.config,
            functions: &self.functions,
            globals: &mut self.globals,
            generics: &self.generics,
            call_depth: 0,
            expression_depth: 0,
            current_module: module,
            module_registry: &self.module_registry,
            function_modules: &self.function_modules,
            global_modules: &self.global_modules,
            generic_modules: &self.generic_modules,
            method_chain: None,
            input_buffer: Some(&mut self.input_buffer),
            fact_base: Some(&self.fact_base),
            initial_fact_id: self.initial_fact_id,
            template_defs: Some(&self.template_defs),
            compact_fact_bindings: None,
            template_resolver: Some(TemplateResolver {
                template_local_ids: &self.template_local_ids,
                template_modules: &self.template_modules,
                module_registry: &self.module_registry,
            }),
        };
        let evaluate_fields = |ctx: &mut EvalContext<'_>, expressions: &[RuntimeExpr]| {
            let mut fields = Vec::new();
            for expression in expressions {
                match evaluator::eval(ctx, expression).map_err(|error| error.to_string())? {
                    Value::Multifield(values) => fields.extend(values.as_slice().iter().cloned()),
                    Value::Void => {}
                    value => fields.push(value),
                }
            }
            Ok::<_, String>(fields)
        };
        match fact {
            PreparedFact::Ordered { relation, fields } => Ok(Fact::Ordered(OrderedFact {
                relation: *relation,
                fields: evaluate_fields(&mut ctx, fields)?.into(),
            })),
            PreparedFact::Template { template_id, slots } => {
                let template = template.expect("template resolved above");
                let mut values = template.defaults.clone();
                for (index, fields) in slots {
                    values[*index] = match template.slot_types[*index] {
                        SlotType::Single => evaluator::eval(&mut ctx, &fields[0])
                            .map_err(|error| error.to_string())?,
                        SlotType::Multi => Value::Multifield(Box::new(
                            evaluate_fields(&mut ctx, fields)?.into_iter().collect(),
                        )),
                    };
                    template.validate_slot(*index, &values[*index])?;
                }
                template.validate_slots(&values)?;
                Ok(Fact::Template(TemplateFact {
                    template_id: *template_id,
                    slots: values.into_boxed_slice(),
                }))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::{Engine, EngineConfig};

    #[test]
    fn dormant_queries_keep_their_template_dependencies_live() {
        let mut engine = Engine::new(EngineConfig::default());
        engine
            .load_str(
                "(deftemplate item (slot n))
             (deffacts seed (probe ready (if FALSE then (any-factp ((?f item)) TRUE) else FALSE)))",
            )
            .unwrap();
        assert!(engine
            .load_str("(deftemplate item (slot replacement))")
            .is_err());
        assert!(engine.find_facts("probe").unwrap().is_empty());
        engine.reset().unwrap();
        assert_eq!(engine.find_facts("probe").unwrap().len(), 1);
    }

    #[test]
    fn empty_assertions_are_errors() {
        let mut engine = Engine::new(EngineConfig::default());
        assert!(engine.load_str("(assert)").is_err());
        assert_eq!(engine.facts().unwrap().count(), 0);
    }
}
