//! Python bindings for the Ferric rules engine.

use pyo3::prelude::*;

pub mod config;
pub mod engine;
pub mod error;
pub mod fact;
pub mod result;
pub mod value;

/// The `ferric` Python module.
#[pymodule]
fn ferric(m: &Bound<'_, PyModule>) -> PyResult<()> {
    // Engine
    m.add_class::<engine::PyEngine>()?;

    // Fact types
    m.add_class::<fact::Fact>()?;
    m.add_class::<fact::FactType>()?;

    // Value types (symbol vs string distinction)
    m.add_class::<value::Symbol>()?;
    m.add_class::<value::ClipsString>()?;

    // Config enums
    m.add_class::<config::Strategy>()?;
    m.add_class::<config::Encoding>()?;
    #[cfg(feature = "serde")]
    m.add_class::<config::Format>()?;

    // Result types
    m.add_class::<result::RunResult>()?;
    m.add_class::<result::HaltReason>()?;
    m.add_class::<result::FiredRule>()?;

    // Exception hierarchy
    error::register_exceptions(m)?;

    Ok(())
}
