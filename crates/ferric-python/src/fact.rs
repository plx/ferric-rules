//! Fact representation for Python bindings.

use pyo3::prelude::*;
use pyo3::types::PyDict;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

use ferric_core::{Fact as CoreFact, FactId};
use ferric_runtime::Engine;
use slotmap::Key;

use crate::value::value_to_python;

/// Fact type discriminator.
#[pyclass(eq, eq_int)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FactType {
    #[pyo3(name = "ORDERED")]
    Ordered = 0,
    #[pyo3(name = "TEMPLATE")]
    Template = 1,
}

/// A snapshot of a fact from the engine.
///
/// This is a value copy — it does not hold a reference to the engine.
#[pyclass]
pub struct Fact {
    /// Fact ID (as u64 from slotmap `KeyData::as_ffi()`).
    #[pyo3(get)]
    pub id: u64,
    /// Engine instance ID (used for cross-engine equality/hash).
    #[pyo3(get)]
    pub engine_id: u64,
    /// Whether this is an ordered or template fact.
    #[pyo3(get)]
    pub fact_type: FactType,
    /// Ordered fact relation name.
    #[pyo3(get)]
    pub relation: Option<String>,
    /// Template fact template name.
    #[pyo3(get)]
    pub template_name: Option<String>,
    /// Field values (ordered: positional fields; template: slot values).
    #[pyo3(get)]
    pub fields: PyObject,
    /// Slot name→value mapping (template facts only).
    #[pyo3(get)]
    pub slots: Option<PyObject>,
}

#[pymethods]
impl Fact {
    fn __repr__(&self, py: Python<'_>) -> PyResult<String> {
        match self.fact_type {
            FactType::Ordered => {
                let fields_repr: String = self.fields.bind(py).repr()?.extract()?;
                Ok(format!(
                    "Fact(id={}, type=ORDERED, relation={:?}, fields={})",
                    self.id,
                    self.relation.as_deref().unwrap_or("?"),
                    fields_repr
                ))
            }
            FactType::Template => {
                let slots_repr = if let Some(ref slots) = self.slots {
                    let r: String = slots.bind(py).repr()?.extract()?;
                    r
                } else {
                    "{}".to_string()
                };
                Ok(format!(
                    "Fact(id={}, type=TEMPLATE, template={:?}, slots={})",
                    self.id,
                    self.template_name.as_deref().unwrap_or("?"),
                    slots_repr
                ))
            }
        }
    }

    fn __eq__(&self, other: &Self) -> bool {
        self.engine_id == other.engine_id && self.id == other.id
    }

    fn __hash__(&self) -> u64 {
        let mut hasher = DefaultHasher::new();
        self.engine_id.hash(&mut hasher);
        self.id.hash(&mut hasher);
        hasher.finish()
    }
}

/// Create a Python `Fact` snapshot from a Rust fact.
pub fn fact_to_python(
    py: Python<'_>,
    fact_id: FactId,
    fact: &CoreFact,
    engine: &Engine,
    engine_id: u64,
) -> PyResult<Fact> {
    let id = fact_id.data().as_ffi();

    match fact {
        CoreFact::Ordered(ordered) => {
            let relation = engine
                .resolve_symbol(ordered.relation)
                .unwrap_or("<unknown>")
                .to_string();
            let fields: Vec<PyObject> = ordered
                .fields
                .iter()
                .map(|v| value_to_python(py, v, engine))
                .collect();
            let fields_list = pyo3::types::PyList::new(py, fields)?.into_any().unbind();

            Ok(Fact {
                id,
                engine_id,
                fact_type: FactType::Ordered,
                relation: Some(relation),
                template_name: None,
                fields: fields_list,
                slots: None,
            })
        }
        CoreFact::Template(template) => {
            let tmpl_name = engine
                .template_name_by_id(template.template_id)
                .unwrap_or("<unknown>")
                .to_string();
            let fields: Vec<PyObject> = template
                .slots
                .iter()
                .map(|v| value_to_python(py, v, engine))
                .collect();
            let fields_list = pyo3::types::PyList::new(py, &fields)?.into_any().unbind();

            // Build slots dict if we can resolve slot names (by ID to avoid
            // name-collision mismatches).
            let slots =
                if let Some(slot_names) = engine.template_slot_names_by_id(template.template_id) {
                    let dict = PyDict::new(py);
                    for (name, val) in slot_names.iter().zip(fields.iter()) {
                        dict.set_item(name, val)?;
                    }
                    Some(dict.into_any().unbind())
                } else {
                    None
                };

            Ok(Fact {
                id,
                engine_id,
                fact_type: FactType::Template,
                relation: None,
                template_name: Some(tmpl_name),
                fields: fields_list,
                slots,
            })
        }
    }
}
