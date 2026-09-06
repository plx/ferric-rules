//! Runtime template registry types.
//!
//! This module defines the `RegisteredTemplate` type, which holds the runtime
//! metadata for a `deftemplate` construct after it has been registered with the
//! engine. Both `loader.rs` and `actions.rs` need access to this type.

use ferric_rules_core::Value;
use ferric_rules_parser::{ActionExpr, FunctionCall, LiteralKind, SlotType, SlotValueType};
use rustc_hash::FxHashMap as HashMap;

/// Runtime representation of a registered template.
///
/// Stores the slot names, their positional indices, and default values so that
/// fact assertions, pattern compilation, and `modify`/`duplicate` actions can
/// resolve slot names to positions and fill in defaults.
#[derive(Clone, Debug)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub(crate) struct RegisteredTemplate {
    /// The template name (e.g. `"person"`).
    pub name: String,
    /// Slot names in declaration order.
    pub slot_names: Vec<String>,
    /// Single-field versus multifield cardinality for each slot position.
    pub slot_types: Vec<SlotType>,
    /// Canonical primitive type union for each slot; None is unconstrained.
    pub allowed_types: Vec<Option<Vec<SlotValueType>>>,
    /// Slot name → positional index mapping.
    #[cfg_attr(
        feature = "serde",
        serde(with = "ferric_rules_core::serde_helpers::fx_hash_map")
    )]
    pub slot_index: HashMap<String, usize>,
    /// Default values for each slot position (`Value::Void` marks `?NONE`).
    pub defaults: Vec<Value>,
}

impl RegisteredTemplate {
    /// Whether an already complete owned fact still has the same template
    /// identity and slot meaning after a definition replacement.
    pub fn same_shape(&self, other: &Self) -> bool {
        self.name == other.name
            && self.slot_names == other.slot_names
            && self.slot_types == other.slot_types
    }

    #[must_use]
    pub fn slot_index(&self, name: &str) -> Option<usize> {
        self.slot_index.get(name).copied()
    }

    /// Validate named slot syntax before any value expression is evaluated.
    pub fn slot_overrides<'a>(
        &self,
        overrides: &'a [ActionExpr],
    ) -> Result<Vec<(usize, &'a FunctionCall)>, String> {
        let mut seen = vec![false; self.slot_names.len()];
        overrides
            .iter()
            .map(|expression| {
                let ActionExpr::FunctionCall(call) = expression else {
                    return Err(format!(
                        "expected named slot list in template `{}`",
                        self.name
                    ));
                };
                let index = self.slot_index(&call.name).ok_or_else(|| {
                    format!("unknown slot `{}` in template `{}`", call.name, self.name)
                })?;
                if std::mem::replace(&mut seen[index], true) {
                    return Err(format!(
                        "duplicate slot `{}` in template `{}`",
                        call.name, self.name
                    ));
                }
                if self.slot_types[index] == SlotType::Single && call.args.len() != 1 {
                    return Err(format!(
                        "single-field slot `{}` in template `{}` requires exactly one value",
                        call.name, self.name
                    ));
                }
                for arg in &call.args {
                    self.validate_literal_expression(index, arg)?;
                }
                Ok((index, call))
            })
            .collect()
    }

    /// Definition identity relevant to an already complete, owned fact.
    #[allow(dead_code)] // Used by the coordinated owned-host-fact boundary.
    pub fn same_shape(&self, other: &Self) -> bool {
        self.name == other.name
            && self.slot_names == other.slot_names
            && self.slot_types == other.slot_types
            && self.allowed_types == other.allowed_types
    }

    /// Check only provably literal fields; expression results are checked at run time.
    pub fn validate_literal_expression(
        &self,
        index: usize,
        expression: &ActionExpr,
    ) -> Result<(), String> {
        match expression {
            ActionExpr::Literal(literal) => self.validate_literal(index, &literal.value),
            ActionExpr::FunctionCall(call) if call.name == "create$" => {
                for arg in &call.args {
                    self.validate_literal_expression(index, arg)?;
                }
                Ok(())
            }
            _ => Ok(()),
        }
    }

    pub fn validate_literal(&self, index: usize, literal: &LiteralKind) -> Result<(), String> {
        let kind = match literal {
            LiteralKind::Symbol(_) => SlotValueType::Symbol,
            LiteralKind::String(_) => SlotValueType::String,
            LiteralKind::Integer(_) => SlotValueType::Integer,
            LiteralKind::Float(_) => SlotValueType::Float,
        };
        self.validate_kind(index, kind)
    }

    fn validate_kind(&self, index: usize, kind: SlotValueType) -> Result<(), String> {
        if self.allowed_types[index]
            .as_ref()
            .is_some_and(|allowed| !allowed.contains(&kind))
        {
            return Err(format!(
                "value does not match allowed types for slot `{}` in template `{}`",
                self.slot_names[index], self.name
            ));
        }
        Ok(())
    }

    pub fn validate_slot(&self, index: usize, value: &Value) -> Result<(), String> {
        if matches!(value, Value::Void) {
            return Err(format!(
                "slot `{}` in template `{}` requires a value because of its (default ?NONE) attribute",
                self.slot_names[index], self.name
            ));
        }
        let fields = match (self.slot_types[index], value) {
            (SlotType::Single, Value::Multifield(_)) => {
                return Err(format!(
                    "single-field slot `{}` in template `{}` requires one scalar value",
                    self.slot_names[index], self.name
                ))
            }
            (SlotType::Single, value) => std::slice::from_ref(value),
            (SlotType::Multi, Value::Multifield(fields)) => fields.as_slice(),
            (SlotType::Multi, _) => {
                return Err(format!(
                    "multifield slot `{}` in template `{}` requires a multifield value",
                    self.slot_names[index], self.name
                ))
            }
        };
        for field in fields {
            let kind = match field {
                Value::Symbol(_) => SlotValueType::Symbol,
                Value::String(_) => SlotValueType::String,
                Value::Integer(_) => SlotValueType::Integer,
                Value::Float(_) => SlotValueType::Float,
                Value::ExternalAddress(_) => SlotValueType::ExternalAddress,
                Value::Multifield(_) if self.allowed_types[index].is_none() => continue,
                Value::Multifield(_) | Value::Void => {
                    return Err(format!(
                        "value does not match allowed types for slot `{}` in template `{}`",
                        self.slot_names[index], self.name
                    ))
                }
            };
            self.validate_kind(index, kind)?;
        }
        Ok(())
    }

    pub fn validate_slots(&self, slots: &[Value]) -> Result<(), String> {
        if slots.len() != self.slot_names.len() {
            return Err(format!(
                "wrong number of slots for template `{}`",
                self.name
            ));
        }
        for (index, value) in slots.iter().enumerate() {
            self.validate_slot(index, value)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    /// Build a well-formed `RegisteredTemplate` from a list of unique slot names.
    fn make_template(name: &str, slot_names: Vec<String>) -> RegisteredTemplate {
        let slot_index: HashMap<String, usize> = slot_names
            .iter()
            .enumerate()
            .map(|(i, n)| (n.clone(), i))
            .collect();
        let defaults = vec![Value::Void; slot_names.len()];
        RegisteredTemplate {
            name: name.to_string(),
            slot_types: vec![SlotType::Single; slot_names.len()],
            allowed_types: vec![None; slot_names.len()],
            slot_names,
            slot_index,
            defaults,
        }
    }

    proptest! {
        /// `slot_index` keys always match `slot_names` entries.
        #[test]
        fn slot_index_keys_match_names(
            slots in proptest::collection::hash_set("[a-z][a-z0-9]{0,8}", 1..8)
        ) {
            let names: Vec<String> = slots.into_iter().collect();
            let tmpl = make_template("test", names.clone());
            for name in &names {
                prop_assert!(
                    tmpl.slot_index.contains_key(name),
                    "slot_index missing key: {}",
                    name
                );
            }
            prop_assert_eq!(tmpl.slot_index.len(), names.len());
        }

        /// All `slot_index` values are valid indices into `defaults`.
        #[test]
        fn slot_index_values_within_bounds(
            slots in proptest::collection::hash_set("[a-z][a-z0-9]{0,8}", 1..8)
        ) {
            let names: Vec<String> = slots.into_iter().collect();
            let tmpl = make_template("test", names);
            for (slot_name, &idx) in &tmpl.slot_index {
                prop_assert!(
                    idx < tmpl.defaults.len(),
                    "slot {} index {} out of bounds (defaults len {})",
                    slot_name, idx, tmpl.defaults.len()
                );
            }
        }

        /// `defaults` length matches `slot_names` length.
        #[test]
        fn defaults_len_matches_slots(
            slots in proptest::collection::hash_set("[a-z][a-z0-9]{0,8}", 0..10)
        ) {
            let names: Vec<String> = slots.into_iter().collect();
            let tmpl = make_template("test", names.clone());
            prop_assert_eq!(tmpl.defaults.len(), names.len());
        }

        /// `slot_index` maps each name to a unique position, and the inverse
        /// mapping back to `slot_names` is consistent.
        #[test]
        fn slot_index_inverse_consistent(
            slots in proptest::collection::hash_set("[a-z][a-z0-9]{0,8}", 1..8)
        ) {
            let names: Vec<String> = slots.into_iter().collect();
            let tmpl = make_template("test", names.clone());
            for (name, &idx) in &tmpl.slot_index {
                prop_assert_eq!(
                    &tmpl.slot_names[idx],
                    name,
                    "inverse mapping broken: slot_names[{}] = '{}', expected '{}'",
                    idx, tmpl.slot_names[idx], name
                );
            }
        }
    }
}
