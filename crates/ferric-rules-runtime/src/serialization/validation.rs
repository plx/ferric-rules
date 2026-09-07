//! Validation of runtime metadata after the core graph has been checked.

use super::{Engine, SerializationError};
use crate::evaluator::RuntimeExpr;
use ferric_rules_core::{Fact, SequenceField, SequenceSource, Value};
use ferric_rules_parser::{ActionExpr, SlotType};

fn ensure(condition: bool, message: &str) -> Result<(), String> {
    if condition {
        Ok(())
    } else {
        Err(message.to_owned())
    }
}

impl Engine {
    pub(super) fn validate_restored_state(&self) -> Result<(), SerializationError> {
        self.validate_snapshot_metadata()
            .map_err(SerializationError::InvalidState)
    }

    #[allow(clippy::too_many_lines)]
    fn validate_snapshot_metadata(&self) -> Result<(), String> {
        self.symbol_table.validate_snapshot()?;
        self.fact_base.validate_snapshot(&self.symbol_table)?;
        self.rete
            .validate_snapshot(&self.fact_base, &self.symbol_table)?;
        ensure(
            self.config.strategy == self.rete.agenda.strategy(),
            "configured strategy disagrees with restored agenda",
        )?;
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
            // One source rule with `or` conditions lowers to several executable
            // rules. Their public names may coincide; the unique slot/terminal
            // association above is their executable identity.
            info.var_map.validate_snapshot(&self.symbol_table)?;
            ensure(
                info.actions.len() == info.runtime_actions.len(),
                "inconsistent compiled action index",
            )?;
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
            let count = template.slot_names.len();
            ensure(
                count == template.slot_types.len()
                    && count == template.allowed_types.len()
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
                self.symbol_table
                    .validate_snapshot_value(&template.defaults[index])?;
                if !matches!(template.defaults[index], Value::Void) {
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
                let SequenceSource::TemplateSlot(index) = segment.source else {
                    return Err("template sequence plan contains an ordered source".to_owned());
                };
                let kind = template
                    .slot_types
                    .get(index)
                    .ok_or("sequence plan references an invalid physical template slot")?;
                ensure(
                    *kind == SlotType::Multi
                        || segment.fields.as_slice() == [SequenceField::Single],
                    "scalar template sequence source must consume exactly one single field",
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
                self.validate_snapshot_fact(fact)?;
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
                self.symbol_table.validate_snapshot_value(value)?;
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
            self.symbol_table.validate_snapshot_value(value)?;
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
                    for expr in &method.body {
                        validate_action(expr)?;
                    }
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
            self.symbol_table.validate_snapshot_value(value)?;
        }
        Ok(())
    }

    fn validate_expression(&self, root: &RuntimeExpr) -> Result<(), String> {
        let mut pending = vec![(root, 0)];
        while let Some((expr, depth)) = pending.pop() {
            ensure(depth < 16, "snapshot expression-depth limit is 16")?;
            let mut branches = Vec::new();
            match expr {
                RuntimeExpr::Literal(value) => self.symbol_table.validate_snapshot_value(value)?,
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
                    validate_action(action)?;
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
    crate::evaluator::validate_action_depth(root).map_err(|error| error.to_string())
}
