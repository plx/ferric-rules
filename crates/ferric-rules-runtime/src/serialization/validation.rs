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
        self.symbol_table
            .validate_encoding(self.config.string_encoding)?;
        self.fact_base.validate_snapshot(&self.symbol_table)?;
        self.rete
            .validate_snapshot(&self.fact_base, &self.symbol_table)?;
        ensure(
            self.config.strategy == self.rete.agenda.strategy(),
            "configured strategy disagrees with restored agenda",
        )?;
        self.compiler
            .validate_snapshot(&self.rete, &self.symbol_table)?;
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
            for index in 0..info.var_map.len() {
                let id = ferric_rules_core::VarId(
                    u16::try_from(index).map_err(|_| "oversized variable map")?,
                );
                ensure(
                    self.symbol_table
                        .resolve_symbol_str(info.var_map.name(id))
                        .is_some(),
                    "runtime variable name must be UTF-8 text",
                )?;
            }
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
                self.validate_expression(&condition.expr)?;
            }
        }
        self.rete.validate_snapshot_rules(|id| {
            self.rule_info
                .get(id.0 as usize)
                .and_then(Option::as_ref)
                .map(|info| (info.salience, info.test_conditions.len()))
        })?;
        self.validate_runtime_condition_uses()?;
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
                self.validate_snapshot_runtime_value(&template.defaults[index])?;
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
        // Segment sources are physical template slots. Core validation checks
        // each plan's separate flattened logical selectors and capture widths.
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
            ensure(self.fact_base.get(id).is_some_and(|entry| matches!(&entry.fact, Fact::Ordered(fact) if fact.fields.is_empty() && self.symbol_table.resolve_symbol_bytes(fact.relation) == Some(b"initial-fact".as_slice()))), "invalid initial-fact identity")?;
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
                self.validate_snapshot_runtime_value(value)?;
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
            self.validate_snapshot_runtime_value(value)?;
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
                    "ordered relation identifier must be valid UTF-8 text",
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
            self.validate_snapshot_runtime_value(value)?;
        }
        Ok(())
    }

    fn validate_snapshot_runtime_value(&self, value: &Value) -> Result<(), String> {
        self.symbol_table.validate_snapshot_value(value)?;
        let mut pending = vec![value];
        while let Some(value) = pending.pop() {
            match value {
                Value::String(string) => {
                    ferric_rules_core::FerricString::from_bytes(
                        string.as_bytes(),
                        self.config.string_encoding,
                    )
                    .map_err(|error| {
                        format!("snapshot string violates configured encoding: {error}")
                    })?;
                }
                Value::Multifield(fields) => pending.extend(fields.iter()),
                _ => {}
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
                RuntimeExpr::Literal(value) => self.validate_snapshot_runtime_value(value)?,
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

    fn validate_runtime_condition_uses(&self) -> Result<(), String> {
        let mut used = rustc_hash::FxHashSet::default();
        let mut work = 10_000_000_usize;
        for usage in self.rete.snapshot_runtime_condition_uses() {
            let info = self
                .rule_info
                .get(usage.rule.0 as usize)
                .and_then(Option::as_ref)
                .ok_or("runtime condition lacks rule metadata")?;
            let condition = info
                .test_conditions
                .get(usage.condition_index as usize)
                .ok_or("runtime condition has dangling index")?;
            ensure(
                condition.role == usage.role,
                "runtime condition role disagrees with graph owner",
            )?;
            // These descriptors extract local physical fields only. Outer
            // sequence captures below contribute variable IDs, not slot indexes.
            for (slot, variable) in &usage.bindings {
                work = work
                    .checked_sub(1)
                    .ok_or("runtime condition validation work limit exceeded")?;
                ensure(
                    (variable.0 as usize) < info.var_map.len(),
                    "runtime condition binding exceeds rule variable map",
                )?;
                if let (
                    Some(ferric_rules_core::AlphaEntryType::Template(id)),
                    ferric_rules_core::SlotIndex::Template(index),
                ) = (&usage.entry_type, slot)
                {
                    let template = self
                        .template_defs
                        .get(*id)
                        .ok_or("runtime condition has dangling template")?;
                    ensure(
                        *index < template.slot_names.len(),
                        "runtime condition binding exceeds template slot count",
                    )?;
                }
            }
            let mut available = rustc_hash::FxHashSet::default();
            for variable in &usage.available_variables {
                work = work
                    .checked_sub(1)
                    .ok_or("runtime condition validation work limit exceeded")?;
                ensure(
                    (variable.0 as usize) < info.var_map.len(),
                    "runtime condition variable exceeds rule map",
                )?;
                let name = self
                    .symbol_table
                    .resolve_symbol_str(info.var_map.name(*variable))
                    .ok_or("runtime condition variable has dangling symbol")?;
                available.insert(name);
            }
            validate_condition_variables(&condition.expr, &available, &mut work)?;
            used.insert((usage.rule, usage.condition_index));
        }
        for (rule, info) in self.rule_info.iter().enumerate() {
            let Some(info) = info else {
                continue;
            };
            for index in 0..info.test_conditions.len() {
                work = work
                    .checked_sub(1)
                    .ok_or("runtime condition validation work limit exceeded")?;
                let rule = ferric_rules_core::RuleId(
                    u32::try_from(rule).map_err(|_| "oversized rule index")?,
                );
                let index = u32::try_from(index).map_err(|_| "oversized condition index")?;
                ensure(
                    used.contains(&(rule, index)),
                    "runtime condition lacks graph owner",
                )?;
            }
        }
        Ok(())
    }
}

/// Inspect lexical free variables without executing callbacks or rebuilding a
/// predicate decision against globals that may have changed since matching.
#[allow(clippy::too_many_lines)] // Scope-aware traversal of the two persisted expression forms.
fn validate_condition_variables(
    root: &RuntimeExpr,
    available: &rustc_hash::FxHashSet<&str>,
    work: &mut usize,
) -> Result<(), String> {
    enum Expression<'a> {
        Runtime(&'a RuntimeExpr),
        Source(&'a ActionExpr),
    }
    use Expression::{Runtime, Source};
    let mut pending = vec![(Runtime(root), Vec::<String>::new())];
    while let Some((expression, scope)) = pending.pop() {
        *work = work
            .checked_sub(scope.len() + 1)
            .ok_or("runtime condition validation work limit exceeded")?;
        let mut children = Vec::new();
        let mut scoped = Vec::new();
        let mut inner = scope.clone();
        let mut branches = Vec::new();
        let mut source_branches = Vec::new();
        let variable = match expression {
            Runtime(RuntimeExpr::BoundVar { name, .. }) | Source(ActionExpr::Variable(name, _)) => {
                Some(name)
            }
            Runtime(RuntimeExpr::Literal(_) | RuntimeExpr::GlobalVar { .. })
            | Source(ActionExpr::Literal(_) | ActionExpr::GlobalVariable(..)) => None,
            Runtime(RuntimeExpr::Call { args, .. }) => {
                children.extend(args.iter().map(Runtime));
                None
            }
            Source(ActionExpr::FunctionCall(call)) => {
                children.extend(call.args.iter().map(Source));
                None
            }
            Runtime(RuntimeExpr::If {
                condition,
                then_branch,
                else_branch,
                ..
            }) => {
                children.push(Runtime(condition));
                branches.extend([then_branch, else_branch]);
                None
            }
            Source(ActionExpr::If {
                condition,
                then_actions,
                else_actions,
                ..
            }) => {
                children.push(Source(condition));
                source_branches.extend([then_actions, else_actions]);
                None
            }
            Runtime(RuntimeExpr::While {
                condition, body, ..
            }) => {
                children.push(Runtime(condition));
                branches.push(body);
                None
            }
            Source(ActionExpr::While {
                condition, body, ..
            }) => {
                children.push(Source(condition));
                source_branches.push(body);
                None
            }
            Runtime(RuntimeExpr::LoopForCount {
                var_name,
                start,
                end,
                body,
                ..
            }) => {
                children.extend([Runtime(start), Runtime(end)]);
                inner.extend(var_name.iter().cloned());
                branches.push(body);
                None
            }
            Source(ActionExpr::LoopForCount {
                var_name,
                start,
                end,
                body,
                ..
            }) => {
                children.extend([Source(start), Source(end)]);
                inner.extend(var_name.iter().cloned());
                source_branches.push(body);
                None
            }
            Runtime(RuntimeExpr::Progn {
                var_name,
                list_expr,
                body,
                ..
            }) => {
                children.push(Runtime(list_expr));
                inner.extend([var_name.clone(), format!("{var_name}-index")]);
                branches.push(body);
                None
            }
            Source(ActionExpr::Progn {
                var_name,
                list_expr,
                body,
                ..
            }) => {
                children.push(Source(list_expr));
                inner.extend([var_name.clone(), format!("{var_name}-index")]);
                source_branches.push(body);
                None
            }
            Runtime(RuntimeExpr::QueryAction {
                bindings,
                query,
                body,
                ..
            }) => {
                inner.extend(bindings.iter().map(|(name, _)| name.clone()));
                scoped.push(Runtime(query));
                branches.push(body);
                None
            }
            Source(ActionExpr::QueryAction {
                bindings,
                query,
                body,
                ..
            }) => {
                inner.extend(bindings.iter().map(|(name, _)| name.clone()));
                scoped.push(Source(query));
                source_branches.push(body);
                None
            }
            Runtime(RuntimeExpr::Switch {
                expr,
                cases,
                default,
                ..
            }) => {
                children.push(Runtime(expr));
                for (value, body) in cases {
                    children.push(Runtime(value));
                    branches.push(body);
                }
                branches.extend(default.iter());
                None
            }
            Source(ActionExpr::Switch {
                expr,
                cases,
                default,
                ..
            }) => {
                children.push(Source(expr));
                for (value, body) in cases {
                    children.push(Source(value));
                    source_branches.push(body);
                }
                source_branches.extend(default.iter());
                None
            }
        };
        if let Some(name) = variable {
            let name = name.strip_prefix("$?").unwrap_or(name);
            ensure(
                available.contains(name)
                    || scope
                        .iter()
                        .any(|bound| bound.strip_prefix("$?").unwrap_or(bound) == name),
                "runtime condition references a variable outside its graph scope",
            )?;
        }
        for branch in branches {
            for (source, runtime) in branch {
                // Both forms are stored and can be selected by execution.
                scoped.push(Source(source));
                if let Some(runtime) = runtime {
                    scoped.push(Runtime(runtime));
                }
            }
        }
        for branch in source_branches {
            scoped.extend(branch.iter().map(Source));
        }
        pending.extend(children.into_iter().map(|child| (child, scope.clone())));
        pending.extend(scoped.into_iter().map(|child| (child, inner.clone())));
    }
    Ok(())
}

fn validate_action(root: &ActionExpr) -> Result<(), String> {
    crate::evaluator::validate_action_depth(root).map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn variable(name: &str) -> RuntimeExpr {
        RuntimeExpr::BoundVar {
            name: name.into(),
            span: None,
        }
    }

    fn body_variable(name: &str) -> (ActionExpr, Option<Box<RuntimeExpr>>) {
        let position = ferric_rules_parser::Position {
            offset: 0,
            line: 1,
            column: 1,
        };
        let span =
            ferric_rules_parser::Span::new(position, position, ferric_rules_parser::FileId(0));
        (
            ActionExpr::Variable(name.into(), span),
            Some(Box::new(variable(name))),
        )
    }

    #[test]
    fn restored_runtime_scope_accepts_an_outer_sequence_capture() {
        let expression = RuntimeExpr::Call {
            name: "length$".into(),
            args: vec![variable("$?prefix")],
            span: None,
        };
        let available = rustc_hash::FxHashSet::from_iter(["prefix"]);
        assert!(validate_condition_variables(&expression, &available, &mut 1000).is_ok());
        let absent = rustc_hash::FxHashSet::default();
        assert!(validate_condition_variables(&expression, &absent, &mut 1000).is_err());
    }

    #[test]
    fn restored_condition_variables_respect_nested_lexical_scopes() {
        let available = rustc_hash::FxHashSet::from_iter(["outer"]);
        let mut loop_expr = RuntimeExpr::LoopForCount {
            var_name: Some("i".into()),
            start: Box::new(variable("outer")),
            end: Box::new(RuntimeExpr::Literal(Value::Integer(2))),
            body: vec![body_variable("i")],
            span: None,
        };
        assert!(validate_condition_variables(&loop_expr, &available, &mut 1000).is_ok());
        if let RuntimeExpr::LoopForCount { start, .. } = &mut loop_expr {
            **start = variable("i");
        }
        assert!(validate_condition_variables(&loop_expr, &available, &mut 1000).is_err());
        let progn = RuntimeExpr::Progn {
            var_name: "field".into(),
            list_expr: Box::new(variable("outer")),
            body: vec![body_variable("field"), body_variable("field-index")],
            span: None,
        };
        assert!(validate_condition_variables(&progn, &available, &mut 1000).is_ok());
        let query = RuntimeExpr::QueryAction {
            name: "do-for-fact".into(),
            bindings: vec![("member".into(), "item".into())],
            query: Box::new(variable("member")),
            body: vec![body_variable("member")],
            span: None,
        };
        assert!(validate_condition_variables(&query, &available, &mut 1000).is_ok());
        for escaped in ["i", "field", "field-index", "member"] {
            assert!(
                validate_condition_variables(&variable(escaped), &available, &mut 1000).is_err()
            );
        }
        assert!(validate_condition_variables(&progn, &available, &mut 1).is_err());
    }
}
