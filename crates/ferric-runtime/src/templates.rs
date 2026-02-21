//! Runtime template registry types.
//!
//! This module defines the `RegisteredTemplate` type, which holds the runtime
//! metadata for a `deftemplate` construct after it has been registered with the
//! engine. Both `loader.rs` and `actions.rs` need access to this type.

use std::collections::HashMap;

use ferric_core::Value;

/// Runtime representation of a registered template.
///
/// Stores the slot names, their positional indices, and default values so that
/// fact assertions, pattern compilation, and `modify`/`duplicate` actions can
/// resolve slot names to positions and fill in defaults.
#[derive(Clone, Debug)]
pub(crate) struct RegisteredTemplate {
    /// The template name (e.g. `"person"`).
    pub name: String,
    /// Slot names in declaration order.
    ///
    /// Retained for future diagnostic use (e.g. "valid slots are: …").
    #[allow(dead_code)]
    pub slot_names: Vec<String>,
    /// Slot name → positional index mapping.
    pub slot_index: HashMap<String, usize>,
    /// Default values for each slot position (`Value::Void` if no default is
    /// declared or the default is `?NONE` / `?DERIVE`).
    pub defaults: Vec<Value>,
}
