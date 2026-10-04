//! Validation of runtime metadata after the core graph has been checked.

use super::{Engine, SerializationError};
use crate::evaluator::RuntimeExpr;
use crate::fact_initializer::PreparedFact;
use ferric_rules_core::{Fact, SequenceSource, Value};
use ferric_rules_parser::{ActionExpr, SlotType};

fn ensure(condition: bool, message: &str) -> Result<(), String> {
    if condition {
        Ok(())
    } else {
        Err(message.to_owned())
    }
}

impl Engine {
    fn validate_declaration_names(
        &self,
        declarations: &[(crate::modules::ModuleId, String)],
    ) -> Result<rustc_hash::FxHashSet<(crate::modules::ModuleId, String)>, String> {
        let mut names = rustc_hash::FxHashSet::default();
        for (module, name) in declarations {
            ensure(
                self.module_registry.get(*module).is_some(),
                "construct declaration has a dangling module",
            )?;
            ensure(
                matches!(crate::qualified_name::parse_qualified_name(name),
                    Ok(crate::qualified_name::QualifiedName::Unqualified(local)) if !local.is_empty()),
                "construct declaration has an invalid local name",
            )?;
            ensure(
                names.insert((*module, name.clone())),
                "duplicate construct declaration",
            )?;
        }
        Ok(names)
    }

    pub(super) fn validate_restored_state(&self) -> Result<(), SerializationError> {
        self.validate_snapshot_metadata()
            .map_err(SerializationError::InvalidState)
    }

    fn validate_snapshot_value(&self, value: &Value) -> Result<(), String> {
        self.symbol_table.validate_snapshot_value(value)?;
        let mut pending = vec![value];
        while let Some(value) = pending.pop() {
            match value {
                Value::FactAddress(address) => self.fact_base.validate_snapshot_fact_address(
                    address,
                    self.fact_epoch,
                    self.initial_fact_id,
                    self.fact_index_starts_at_zero,
                )?,
                Value::Multifield(fields) => pending.extend(fields.iter()),
                _ => {}
            }
        }
        Ok(())
    }

    #[allow(clippy::too_many_lines)]
    fn validate_snapshot_metadata(&self) -> Result<(), String> {
        ensure(
            !self.fact_index_starts_at_zero || self.initial_fact_id.is_none(),
            "zero-based cleared fact chronology has an initial fact",
        )?;
        self.symbol_table.validate_snapshot()?;
        self.globals.random.validate_snapshot()?;
        self.fact_base.validate_snapshot(&self.symbol_table)?;
        self.rete
            .validate_snapshot(&self.fact_base, &self.symbol_table)?;
        ensure(
            self.config.strategy == self.rete.agenda.strategy(),
            "configured strategy disagrees with restored agenda",
        )?;
        self.rete
            .validate_snapshot_binding_values(|value| self.validate_snapshot_value(value))?;
        self.compiler.validate_snapshot(&self.rete)?;
        // Installation allocates sequential IDs and reuses removed slots. The
        // index retains its capacity after removal; only a new engine is empty.
        // A forged counter must not make the next load allocate a sparse Vec.
        ensure(
            self.compiler.snapshot_next_rule_id() as usize == self.rule_info.len().max(1),
            "compiler rule allocator disagrees with runtime rule capacity",
        )?;
        let modules = &self.module_registry;
        modules.validate_snapshot()?;
        let template_names = self.validate_declaration_names(&self.template_declarations)?;
        let rule_names = self.validate_declaration_names(&self.rule_declarations)?;
        ensure(
            self.template_declarations.first()
                == Some(&(modules.main_module_id(), "initial-fact".to_owned())),
            "template declaration order lacks the initial fact",
        )?;
        // Requested call depth is application configuration; the evaluator
        // always applies its fixed effective ceiling, including after restore.
        ensure(
            self.rule_info.len() == self.rule_modules.len(),
            "inconsistent rule/module index length",
        )?;
        let terminal_rules: rustc_hash::FxHashSet<_> = self.rete.snapshot_rule_ids().collect();
        ensure(
            terminal_rules.len() == self.rete.snapshot_rule_ids().count(),
            "multiple terminals share an executable rule ID",
        )?;
        let mut live_rules = 0;
        let mut live_rule_names = rustc_hash::FxHashSet::default();
        for (index, info) in self.rule_info.iter().enumerate() {
            let Some(info) = info else {
                ensure(
                    self.rule_modules[index].is_none(),
                    "module assigned to removed rule",
                )?;
                continue;
            };
            live_rules += 1;
            let rule = ferric_rules_core::RuleId(
                u32::try_from(index).map_err(|_| "oversized rule index")?,
            );
            ensure(
                terminal_rules.contains(&rule),
                "runtime rule has no terminal",
            )?;
            let module = self.rule_modules[index].ok_or("rule has no module")?;
            ensure(modules.get(module).is_some(), "rule has dangling module")?;
            let name = crate::qualified_name::parse_qualified_name(&info.name)?;
            live_rule_names.insert((module, name.local_name().to_owned()));
            // One source rule with `or` conditions lowers to several executable
            // rules. Their public names may coincide; the unique slot/terminal
            // association above is their executable identity.
            info.var_map.validate_snapshot(&self.symbol_table)?;
            ensure(
                info.actions.len() == info.runtime_actions.len(),
                "inconsistent compiled action index",
            )?;
            crate::callable_validation::validate_action_breaks_with_templates(
                &info.actions,
                &|name| self.resolve_template_id(name, module).is_ok(),
            )
            .map_err(|(_, message)| message)?;
            for action in &info.actions {
                for argument in &action.call.args {
                    validate_action(argument)?;
                }
            }
            for expr in info.runtime_actions.iter().flatten() {
                self.validate_expression(expr)?;
            }
            for condition in &info.test_conditions {
                let crate::actions::CompiledTestCondition::Expr(expr) = condition;
                self.validate_expression(expr)?;
            }
        }
        self.rete.validate_snapshot_rules(|id| {
            self.rule_info
                .get(id.0 as usize)
                .and_then(Option::as_ref)
                .map(|info| (info.salience, info.test_conditions.len()))
        })?;
        ensure(
            live_rules == terminal_rules.len(),
            "terminal lacks runtime rule metadata",
        )?;
        ensure(
            live_rule_names == rule_names,
            "rule declaration order disagrees with active rules",
        )?;
        ensure(
            self.template_defs.len() == self.template_ids.len(),
            "inconsistent template name index",
        )?;
        ensure(
            self.template_defs.len() == self.template_modules.len(),
            "inconsistent template module index",
        )?;
        for (id, template) in &self.template_defs {
            ensure(
                self.template_ids.get(template.name.as_str()) == Some(&id),
                "inconsistent template identity",
            )?;
            ensure(
                self.template_modules
                    .get(id)
                    .is_some_and(|module| modules.get(*module).is_some()),
                "template has dangling module",
            )?;
            let module = self.template_modules[id];
            let name = crate::qualified_name::parse_qualified_name(&template.name)?;
            ensure(
                template_names.contains(&(module, name.local_name().to_owned())),
                "template declaration order omits an explicit template",
            )?;
            let count = template.slot_names.len();
            ensure(
                count == template.slot_types.len()
                    && count == template.allowed_types.len()
                    && count == template.constraints.len()
                    && count == template.dynamic_defaults.len()
                    && count == template.defaults.len()
                    && count == template.slot_index.len(),
                "inconsistent template slot vectors",
            )?;
            for (index, name) in template.slot_names.iter().enumerate() {
                ensure(
                    template.slot_index.get(name) == Some(&index),
                    "inconsistent template slot index",
                )?;
                if let Some(types) = &template.allowed_types[index] {
                    ensure(
                        !types.is_empty() && types.windows(2).all(|pair| pair[0] < pair[1]),
                        "noncanonical template type union",
                    )?;
                }
                template.constraints[index].validate_metadata(
                    template.allowed_types[index].as_deref(),
                    template.slot_types[index],
                    &self.symbol_table,
                )?;
                self.validate_snapshot_value(&template.defaults[index])?;
                if let Some(default) = &template.dynamic_defaults[index] {
                    ensure(
                        self.template_modules.get(id) == Some(&default.module),
                        "dynamic default has inconsistent owner module",
                    )?;
                    ensure(
                        matches!(template.defaults[index], Value::Void),
                        "dynamic default has a static default value",
                    )?;
                    for expression in &default.expressions {
                        self.validate_expression(expression)?;
                        self.validate_snapshot_default_control(expression, default.module)?;
                    }
                    Self::validate_snapshot_slot_expressions(
                        template,
                        index,
                        &default.expressions,
                    )?;
                } else if !matches!(template.defaults[index], Value::Void) {
                    template.validate_slot(index, &template.defaults[index])?;
                }
            }
        }
        for id in self.rete.snapshot_template_ids() {
            ensure(
                self.template_defs.contains_key(id),
                "alpha graph has dangling template",
            )?;
        }
        for (template_id, plan) in self.rete.snapshot_template_sequence_patterns()? {
            let template = self
                .template_defs
                .get(template_id)
                .ok_or("sequence plan has a dangling template")?;
            for segment in &plan.segments {
                let (index, expected) = match segment.source {
                    SequenceSource::TemplateSlot(index) => (index, SlotType::Multi),
                    SequenceSource::TemplateScalar(index) => (index, SlotType::Single),
                    SequenceSource::Ordered => {
                        return Err("template sequence plan contains an ordered source".to_owned())
                    }
                };
                let kind = template
                    .slot_types
                    .get(index)
                    .ok_or("sequence plan references an invalid physical template slot")?;
                ensure(
                    *kind == expected,
                    "template sequence source does not match its slot kind",
                )?;
            }
        }
        for (_, entry) in self.fact_base.iter() {
            self.validate_snapshot_fact(&entry.fact)?;
        }
        let mut seed_names = rustc_hash::FxHashSet::default();
        for definition in &self.registered_deffacts {
            ensure(
                modules.get(definition.module).is_some(),
                "deffacts has dangling module",
            )?;
            ensure(
                !definition.name.is_empty() && !definition.name.contains("::"),
                "invalid local deffacts name",
            )?;
            ensure(
                seed_names.insert((definition.module, &definition.name)),
                "duplicate named deffacts definition",
            )?;
            for fact in &definition.facts {
                self.validate_snapshot_initializer(fact)?;
            }
        }
        if let Some(id) = self.initial_fact_id {
            ensure(self.fact_base.get(id).is_some_and(|entry| matches!(&entry.fact, Fact::Ordered(fact) if fact.fields.is_empty() && self.symbol_table.resolve_symbol_str(fact.relation) == Some("initial-fact"))), "invalid initial-fact identity")?;
        }
        for (module, values) in &self.globals.values {
            ensure(modules.get(*module).is_some(), "global has dangling module")?;
            for (name, value) in values {
                ensure(!name.is_empty(), "empty global name")?;
                ensure(
                    self.global_modules
                        .get(module)
                        .and_then(|entries| entries.get(name))
                        .copied()
                        == Some(*module),
                    "global missing from owner index",
                )?;
                self.validate_snapshot_value(value)?;
            }
        }
        ensure(self.globals.gensym_counter >= 1, "invalid gensym counter")?;
        let mut global_names = rustc_hash::FxHashSet::default();
        for (module, name, value) in &self.registered_globals {
            ensure(
                modules.get(*module).is_some(),
                "registered global has dangling module",
            )?;
            ensure(
                global_names.insert((*module, name.as_str())),
                "duplicate registered global definition",
            )?;
            ensure(
                self.globals.contains(*module, name)
                    && self
                        .global_modules
                        .get(module)
                        .and_then(|entries| entries.get(name.as_str()))
                        .copied()
                        == Some(*module),
                "registered global missing from runtime or owner index",
            )?;
            self.validate_snapshot_value(value)?;
        }
        for (module, functions) in &self.functions.functions {
            ensure(
                modules.get(*module).is_some(),
                "function has dangling module",
            )?;
            for (name, function) in functions {
                ensure(name.as_ref() == function.name, "inconsistent function name")?;
                ensure(
                    self.function_modules
                        .get(module)
                        .and_then(|entries| entries.get(name))
                        .copied()
                        == Some(*module),
                    "function missing from owner index",
                )?;
                for expr in &function.body {
                    validate_action(expr)?;
                }
                crate::callable_validation::validate_breaks_with_templates(
                    &function.body,
                    &|name| self.resolve_template_id(name, *module).is_ok(),
                )
                .map_err(|(_, message)| message)?;
                crate::callable_validation::validate_iterator_binds_with_templates(
                    &function.body,
                    &|name| self.resolve_template_id(name, *module).is_ok(),
                )
                .map_err(|(_, message)| message)?;
            }
        }
        for (module, generics) in &self.generics.generics {
            ensure(
                modules.get(*module).is_some(),
                "generic has dangling module",
            )?;
            for (name, generic) in generics {
                ensure(name.as_ref() == generic.name, "inconsistent generic name")?;
                ensure(
                    self.generic_modules
                        .get(module)
                        .and_then(|entries| entries.get(name))
                        .copied()
                        == Some(*module),
                    "generic missing from owner index",
                )?;
                ensure(
                    generic.next_index > 0 && generic.next_index < i32::MAX,
                    "invalid next method index",
                )?;
                let mut previous = 0;
                for method in &generic.methods {
                    ensure(
                        method.index > previous && method.index < generic.next_index,
                        "inconsistent method order/index",
                    )?;
                    previous = method.index;
                    ensure(
                        method.parameters.len() == method.type_restrictions.len(),
                        "inconsistent method restrictions",
                    )?;
                    ensure(
                        method.parameters.len() == method.parameter_queries.len(),
                        "inconsistent method queries",
                    )?;
                    ensure(
                        method.wildcard_parameter.is_some()
                            || (method.wildcard_type_restrictions.is_empty()
                                && method.wildcard_query.is_none()),
                        "wildcard restrictions without a wildcard parameter",
                    )?;
                    for expr in method
                        .body
                        .iter()
                        .chain(method.parameter_queries.iter().flatten())
                        .chain(method.wildcard_query.as_ref())
                    {
                        validate_action(expr)?;
                    }
                    self.validate_method_queries(
                        &method
                            .parameters
                            .iter()
                            .cloned()
                            .chain(method.wildcard_parameter.iter().cloned())
                            .collect(),
                        method
                            .parameter_queries
                            .iter()
                            .flatten()
                            .chain(method.wildcard_query.as_ref()),
                        *module,
                        name,
                    )
                    .map_err(|error| error.to_string())?;
                    crate::callable_validation::validate_breaks_with_templates(
                        &method.body,
                        &|name| self.resolve_template_id(name, *module).is_ok(),
                    )
                    .map_err(|(_, message)| message)?;
                    crate::callable_validation::validate_iterator_binds_with_templates(
                        &method.body,
                        &|name| self.resolve_template_id(name, *module).is_ok(),
                    )
                    .map_err(|(_, message)| message)?;
                }
            }
        }
        for (mapping, kind) in [
            (&self.function_modules, "function"),
            (&self.global_modules, "global"),
            (&self.generic_modules, "generic"),
        ] {
            for (module, entries) in mapping {
                ensure(
                    modules.get(*module).is_some(),
                    "construct map has dangling module",
                )?;
                for (name, owner) in entries {
                    ensure(module == owner, "inconsistent construct owner")?;
                    let exists = match kind {
                        "function" => self.functions.contains(*module, name),
                        "global" => self.globals.contains(*module, name),
                        _ => self.generics.contains(*module, name),
                    };
                    ensure(exists, "construct owner map has dangling entry")?;
                }
            }
        }
        Ok(())
    }

    fn validate_snapshot_fact(&self, fact: &Fact) -> Result<(), String> {
        let values = match fact {
            Fact::Ordered(fact) => {
                ensure(
                    self.symbol_table
                        .resolve_symbol_str(fact.relation)
                        .is_some(),
                    "dangling deffacts relation",
                )?;
                fact.fields.as_slice()
            }
            Fact::Template(fact) => {
                let template = self
                    .template_defs
                    .get(fact.template_id)
                    .ok_or("fact has dangling template")?;
                ensure(
                    fact.slots.len() == template.slot_names.len(),
                    "fact/template slot count mismatch",
                )?;
                for (kind, value) in template.slot_types.iter().zip(fact.slots.iter()) {
                    ensure(
                        matches!(value, Value::Multifield(_)) == (*kind == SlotType::Multi),
                        "fact/template slot cardinality mismatch",
                    )?;
                }
                template.validate_slots(&fact.slots)?;
                fact.slots.as_ref()
            }
        };
        for value in values {
            self.validate_snapshot_value(value)?;
        }
        Ok(())
    }

    fn validate_snapshot_initializer(&self, fact: &PreparedFact) -> Result<(), String> {
        match fact {
            PreparedFact::Ordered { relation, .. } => {
                self.validate_snapshot_value(&Value::Symbol(*relation))?;
            }
            PreparedFact::Template { template_id, slots } => {
                let template = self
                    .template_defs
                    .get(*template_id)
                    .ok_or("fact initializer has dangling template")?;
                let mut seen = rustc_hash::FxHashSet::default();
                for (index, expressions) in slots {
                    ensure(
                        *index < template.slot_types.len(),
                        "fact initializer has invalid slot index",
                    )?;
                    ensure(seen.insert(*index), "fact initializer has duplicate slot")?;
                    Self::validate_snapshot_slot_expressions(template, *index, expressions)?;
                }
                for (index, default) in template.defaults.iter().enumerate() {
                    if !seen.contains(&index) && template.dynamic_defaults[index].is_none() {
                        template.validate_slot(index, default)?;
                    }
                }
            }
        }
        for expression in fact.expressions() {
            self.validate_expression(expression)?;
        }
        Ok(())
    }

    /// Validate literal elements independently, then a known complete aggregate length.
    fn validate_snapshot_slot_expressions(
        template: &crate::templates::RegisteredTemplate,
        index: usize,
        expressions: &[RuntimeExpr],
    ) -> Result<(), String> {
        if template.slot_types[index] == SlotType::Single {
            ensure(
                expressions.len() == 1,
                "single-field initializer requires exactly one expression",
            )?;
            match &expressions[0] {
                RuntimeExpr::Literal(value) => template.validate_slot(index, value)?,
                RuntimeExpr::Call { name, .. } if name == "create$" => {
                    return Err("single-field initializer requires one scalar value".to_owned());
                }
                _ => {}
            }
            return Ok(());
        }
        let mut pending: Vec<_> = expressions.iter().collect();
        let mut length = 0_usize;
        let mut complete = true;
        while let Some(expression) = pending.pop() {
            match expression {
                RuntimeExpr::Literal(Value::Void) => {}
                RuntimeExpr::Literal(Value::Multifield(values)) => {
                    for value in values.iter() {
                        template.validate_field(index, value)?;
                    }
                    length = length
                        .checked_add(values.len())
                        .ok_or("initializer length overflow")?;
                }
                RuntimeExpr::Literal(value) => {
                    template.validate_field(index, value)?;
                    length = length.checked_add(1).ok_or("initializer length overflow")?;
                }
                RuntimeExpr::Call { name, args, .. } if name == "create$" => pending.extend(args),
                _ => complete = false,
            }
        }
        if complete {
            template.constraints[index].validate_cardinality(length)?;
        }
        Ok(())
    }

    fn validate_snapshot_default_control(
        &self,
        root: &RuntimeExpr,
        module: crate::modules::ModuleId,
    ) -> Result<(), String> {
        let mut actions = Vec::new();
        for expression in crate::fact_initializer::RuntimeExpressions::new(root) {
            let mut branches = Vec::new();
            match expression {
                RuntimeExpr::Call { name, .. } if name == "return" => {
                    return Err("return is not valid in a template default".to_owned())
                }
                RuntimeExpr::EffectCall { call } => {
                    actions.extend(crate::effects::evaluated_arguments(self, module, call));
                }
                RuntimeExpr::If {
                    then_branch,
                    else_branch,
                    ..
                } => branches.extend([then_branch, else_branch]),
                RuntimeExpr::While { body, .. }
                | RuntimeExpr::LoopForCount { body, .. }
                | RuntimeExpr::Progn { body, .. }
                | RuntimeExpr::QueryAction { body, .. } => branches.push(body),
                RuntimeExpr::Switch { cases, default, .. } => {
                    branches.extend(cases.iter().map(|(_, body)| body));
                    branches.extend(default.iter());
                }
                _ => {}
            }
            for branch in branches {
                actions.extend(branch.iter().map(|(action, _)| action));
            }
        }
        while let Some(expression) = actions.pop() {
            match expression {
                ActionExpr::FunctionCall(call) => {
                    ensure(
                        call.name != "return",
                        "return is not valid in a template default",
                    )?;
                    actions.extend(crate::effects::evaluated_arguments(self, module, call));
                }
                expression => expression.push_children(&mut actions),
            }
        }
        Ok(())
    }

    fn validate_expression(&self, root: &RuntimeExpr) -> Result<(), String> {
        let mut pending = vec![(root, 0)];
        while let Some((expr, depth)) = pending.pop() {
            ensure(depth < 16, "snapshot expression-depth limit is 16")?;
            let mut branches = Vec::new();
            match expr {
                RuntimeExpr::EffectCall { call } => {
                    ensure(
                        matches!(call.name.as_str(), "assert" | "modify" | "duplicate"),
                        "invalid syntax effect",
                    )?;
                    for argument in &call.args {
                        validate_action_at_depth(argument, depth + 1)?;
                    }
                }
                RuntimeExpr::Literal(value) => self.validate_snapshot_value(value)?,
                RuntimeExpr::BoundVar { .. } | RuntimeExpr::GlobalVar { .. } => {}
                RuntimeExpr::Call { args, .. } => {
                    pending.extend(args.iter().map(|expr| (expr, depth + 1)));
                }
                RuntimeExpr::If {
                    condition,
                    then_branch,
                    else_branch,
                    ..
                } => {
                    pending.push((condition, depth + 1));
                    branches.extend([then_branch, else_branch]);
                }
                RuntimeExpr::While {
                    condition, body, ..
                } => {
                    pending.push((condition, depth + 1));
                    branches.push(body);
                }
                RuntimeExpr::LoopForCount {
                    start, end, body, ..
                } => {
                    pending.extend([(start.as_ref(), depth + 1), (end.as_ref(), depth + 1)]);
                    branches.push(body);
                }
                RuntimeExpr::Progn {
                    list_expr, body, ..
                } => {
                    pending.push((list_expr, depth + 1));
                    branches.push(body);
                }
                RuntimeExpr::QueryAction { query, body, .. } => {
                    pending.push((query, depth + 1));
                    branches.push(body);
                }
                RuntimeExpr::Switch {
                    expr,
                    cases,
                    default,
                    ..
                } => {
                    pending.push((expr, depth + 1));
                    for (case, body) in cases {
                        pending.push((case, depth + 1));
                        branches.push(body);
                    }
                    branches.extend(default.iter());
                }
            }
            for branch in branches {
                for (action, runtime) in branch {
                    validate_action_at_depth(action, depth + 1)?;
                    if let Some(runtime) = runtime {
                        pending.push((runtime, depth + 1));
                    }
                }
            }
        }
        Ok(())
    }
}

fn validate_action(root: &ActionExpr) -> Result<(), String> {
    validate_action_at_depth(root, 0)
}

fn validate_action_at_depth(root: &ActionExpr, initial_depth: usize) -> Result<(), String> {
    let mut pending = vec![(root, initial_depth)];
    let mut children = Vec::new();
    while let Some((expression, depth)) = pending.pop() {
        ensure(depth < 16, "snapshot expression-depth limit is 16")?;
        expression.push_children(&mut children);
        pending.extend(children.drain(..).map(|child| (child, depth + 1)));
    }
    Ok(())
}
