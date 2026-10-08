//! Public inspection and transient shell tracing of live engine operations.

use std::fmt::Write as _;

use ferric_rules_core::{Activation, Fact, FactId, Timestamp, Value};

use crate::engine::{rule_index_get, Engine, EngineError};
use crate::host::FactHandle;
use crate::modules::ModuleId;

#[derive(Default)]
pub(crate) struct WatchState {
    pub(crate) facts: bool,
    pub(crate) rules: bool,
    pub(crate) firing_ordinal: u64,
}

/// One pending activation, in the engine's conflict-resolution order.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AgendaEntry {
    /// Unqualified rule name.
    pub rule_name: String,
    /// Module defining the rule.
    pub module_name: String,
    pub salience: i32,
    /// Outer conditional-element basis; `None` is an absent/dummy match (`*`).
    pub basis: Vec<Option<u64>>,
}

impl AgendaEntry {
    /// One CLIPS agenda row, without a trailing newline or module heading.
    #[must_use]
    pub fn format_line(&self) -> String {
        format!(
            "{:<6} {}: {}",
            self.salience,
            self.rule_name,
            self.format_basis()
        )
    }

    /// CLIPS fact basis, such as `f-1,f-2,*`.
    #[must_use]
    pub fn format_basis(&self) -> String {
        let mut text = String::new();
        for (index, fact) in self.basis.iter().enumerate() {
            if index != 0 {
                text.push(',');
            }
            if let Some(fact) = fact {
                let _ = write!(text, "f-{fact}");
            } else {
                text.push('*');
            }
        }
        text
    }
}

impl Engine {
    /// Return a live user fact's CLIPS index, independently of its opaque host handle.
    #[must_use]
    pub fn public_fact_index(&self, handle: FactHandle) -> Option<u64> {
        let id = self.host.resolve(handle)?;
        crate::fact_address::public_fact_index(
            &self.fact_base,
            self.initial_fact_id,
            self.fact_index_starts_at_zero,
            id,
        )
    }

    /// List every live fact with its CLIPS index, in index order.
    ///
    /// Unlike [`Self::facts`], this includes the protected `(initial-fact)` as
    /// `f-0`, as the CLIPS `facts` command does.
    #[must_use]
    pub fn fact_listing(&self) -> Vec<(u64, &Fact)> {
        let mut facts: Vec<_> = self
            .fact_base
            .iter()
            .filter_map(|(id, entry)| {
                crate::fact_address::public_fact_index(
                    &self.fact_base,
                    self.initial_fact_id,
                    self.fact_index_starts_at_zero,
                    id,
                )
                .map(|index| (index, &entry.fact))
            })
            .collect();
        facts.sort_unstable_by_key(|(index, _)| *index);
        facts
    }

    /// Unqualified names of the current module's rules, in definition order.
    #[must_use]
    pub fn current_module_rule_names(&self) -> Vec<&str> {
        let module = self.module_registry.current_module();
        self.rule_declarations
            .iter()
            .filter(|(owner, _)| *owner == module)
            .map(|(_, name)| name.as_str())
            .collect()
    }

    /// Render a value for a CLIPS prompt, quoting strings and retaining literal control symbols.
    #[must_use]
    pub fn format_value(&self, value: &Value) -> String {
        let mut text = String::new();
        crate::value_print::append_display_value(value, &self.symbol_table, &mut text);
        text
    }

    /// Render a fact with its real relation/template name and declared slot names.
    /// The fact's interned identities must belong to this engine.
    pub fn format_fact(&self, fact: &Fact) -> Result<String, EngineError> {
        self.render_fact(fact, false)
    }

    pub(crate) fn format_fact_for_save(&self, fact: &Fact) -> Result<String, EngineError> {
        self.render_fact(fact, true)
    }

    fn render_fact(&self, fact: &Fact, save: bool) -> Result<String, EngineError> {
        let mut text = String::from("(");
        let append = |value: &Value, text: &mut String| {
            if save {
                crate::value_print::append_save_value(value, &self.symbol_table, text);
            } else {
                crate::value_print::append_display_value(value, &self.symbol_table, text);
            }
        };
        let append_fields = |value: &Value, text: &mut String| {
            if let Value::Multifield(fields) = value {
                for field in fields.iter() {
                    text.push(' ');
                    append(field, text);
                }
            } else {
                text.push(' ');
                append(value, text);
            }
        };
        match fact {
            Fact::Ordered(fact) => {
                let relation = self.resolve_core_symbol(fact.relation).ok_or_else(|| {
                    EngineError::InvalidHostValue(
                        "fact relation is not interned in this engine".into(),
                    )
                })?;
                text.push_str(relation);
                for field in &fact.fields {
                    append_fields(field, &mut text);
                }
            }
            Fact::Template(fact) => {
                let template = self.template_defs.get(fact.template_id).ok_or_else(|| {
                    EngineError::InvalidHostValue(
                        "fact template is not registered in this engine".into(),
                    )
                })?;
                if fact.slots.len() != template.slot_names.len() {
                    return Err(EngineError::SlotCountMismatch {
                        names: template.slot_names.len(),
                        values: fact.slots.len(),
                    });
                }
                text.push_str(template.name.rsplit("::").next().unwrap_or(&template.name));
                for (name, value) in template.slot_names.iter().zip(&fact.slots) {
                    text.push_str(" (");
                    text.push_str(name);
                    append_fields(value, &mut text);
                    text.push(')');
                }
            }
        }
        text.push(')');
        Ok(text)
    }

    /// Inspect the current module's agenda without changing focus or consuming activations.
    #[must_use]
    pub fn agenda_entries(&self) -> Vec<AgendaEntry> {
        self.collect_agenda_entries(Some(self.module_registry.current_module()))
    }

    /// Inspect a named module's agenda, or all modules for `"*"`.
    pub fn agenda_entries_in_module(&self, module: &str) -> Result<Vec<AgendaEntry>, EngineError> {
        if module == "*" {
            let mut entries = self.collect_agenda_entries(None);
            // CLIPS groups agendas by module creation order. Stable sorting
            // retains conflict-resolution order within each module.
            entries.sort_by_key(|entry| {
                self.module_registry
                    .get_by_name(&entry.module_name)
                    .map(|module| module.0)
            });
            Ok(entries)
        } else {
            let module = self
                .module_registry
                .get_by_name(module)
                .ok_or_else(|| EngineError::ModuleNotFound(module.to_owned()))?;
            Ok(self.collect_agenda_entries(Some(module)))
        }
    }

    fn collect_agenda_entries(&self, module: Option<ModuleId>) -> Vec<AgendaEntry> {
        self.rete
            .agenda
            .iter_ordered()
            .filter_map(|activation| {
                let owner = rule_index_get(&self.rule_modules, activation.rule)?;
                if module.is_some_and(|module| module != *owner) {
                    return None;
                }
                self.describe_activation(activation)
            })
            .collect()
    }

    fn public_index_for_timestamp(&self, timestamp: Timestamp) -> u64 {
        let initial = self.initial_fact_id.and_then(|id| self.fact_base.get(id));
        if initial.is_some_and(|initial| initial.timestamp == timestamp) {
            0
        } else if self.fact_index_starts_at_zero
            || initial.is_some_and(|initial| initial.timestamp < timestamp)
        {
            timestamp.get()
        } else {
            // The fact store never assigns u64::MAX.
            timestamp.get() + 1
        }
    }

    fn describe_activation(&self, activation: &Activation) -> Option<AgendaEntry> {
        let info = rule_index_get(&self.rule_info, activation.rule)?;
        let module = *rule_index_get(&self.rule_modules, activation.rule)?;
        Some(AgendaEntry {
            rule_name: info
                .name
                .rsplit("::")
                .next()
                .unwrap_or(&info.name)
                .to_owned(),
            module_name: self.module_registry.module_name(module)?.to_owned(),
            salience: activation.salience.get(),
            basis: activation
                .recency
                .iter()
                .map(|tag| {
                    tag.timestamp()
                        .map(|timestamp| self.public_index_for_timestamp(timestamp))
                })
                .collect(),
        })
    }

    /// Enable/disable fact tracing, returning the previous setting.
    pub fn set_watch_facts(&mut self, enabled: bool) -> bool {
        std::mem::replace(&mut self.watch.facts, enabled)
    }

    /// Enable/disable rule firing tracing, returning the previous setting.
    pub fn set_watch_rules(&mut self, enabled: bool) -> bool {
        std::mem::replace(&mut self.watch.rules, enabled)
    }

    #[must_use]
    pub fn watch_facts(&self) -> bool {
        self.watch.facts
    }

    #[must_use]
    pub fn watch_rules(&self) -> bool {
        self.watch.rules
    }

    pub(crate) fn trace_fact(&mut self, id: FactId, asserted: bool) {
        if !self.watch.facts {
            return;
        }
        let Some(entry) = self.fact_base.get(id) else {
            return;
        };
        let Some(index) = crate::fact_address::public_fact_index(
            &self.fact_base,
            self.initial_fact_id,
            self.fact_index_starts_at_zero,
            id,
        ) else {
            return;
        };
        let Ok(fact) = self.format_fact(&entry.fact) else {
            return;
        };
        let marker = if asserted { "==>" } else { "<==" };
        self.flush_expression_output();
        self.router
            .write("wtrace", &format!("{marker} f-{index:<5} {fact}\n"));
    }

    pub(crate) fn trace_fact_removals(&mut self) {
        if !self.watch.facts {
            return;
        }
        let mut facts: Vec<_> = self
            .fact_base
            .iter()
            .map(|(id, entry)| (entry.timestamp, id))
            .collect();
        facts.sort_unstable_by_key(|(timestamp, _)| *timestamp);
        for (_, id) in facts {
            self.trace_fact(id, false);
        }
    }

    pub(crate) fn trace_rule_firing(&mut self, activation: &Activation) {
        self.watch.firing_ordinal = self.watch.firing_ordinal.saturating_add(1);
        if !self.watch.rules {
            return;
        }
        let Some(entry) = self.describe_activation(activation) else {
            return;
        };
        let text = format!(
            "FIRE {:4} {}: {}\n",
            self.watch.firing_ordinal,
            entry.rule_name,
            entry.format_basis()
        );
        self.flush_expression_output();
        self.router.write("wtrace", &text);
    }
}
