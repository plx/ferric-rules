//! Result types for Python bindings.

use pyo3::prelude::*;

/// Why execution stopped.
#[pyclass(eq, eq_int)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HaltReason {
    /// The agenda was empty.
    #[pyo3(name = "AGENDA_EMPTY")]
    AgendaEmpty = 0,
    /// The run limit was reached.
    #[pyo3(name = "LIMIT_REACHED")]
    LimitReached = 1,
    /// A halt was requested.
    #[pyo3(name = "HALT_REQUESTED")]
    HaltRequested = 2,
}

impl From<ferric_runtime::HaltReason> for HaltReason {
    fn from(hr: ferric_runtime::HaltReason) -> Self {
        match hr {
            ferric_runtime::HaltReason::AgendaEmpty => Self::AgendaEmpty,
            ferric_runtime::HaltReason::LimitReached => Self::LimitReached,
            ferric_runtime::HaltReason::HaltRequested => Self::HaltRequested,
        }
    }
}

/// Result of an execution run.
#[pyclass]
#[derive(Clone, Debug)]
pub struct RunResult {
    /// Number of rules fired.
    #[pyo3(get)]
    pub rules_fired: usize,
    /// Why execution stopped.
    #[pyo3(get)]
    pub halt_reason: HaltReason,
}

#[pymethods]
impl RunResult {
    fn __repr__(&self) -> String {
        format!(
            "RunResult(rules_fired={}, halt_reason={:?})",
            self.rules_fired, self.halt_reason
        )
    }
}

impl From<ferric_runtime::RunResult> for RunResult {
    fn from(rr: ferric_runtime::RunResult) -> Self {
        Self {
            rules_fired: rr.rules_fired,
            halt_reason: rr.halt_reason.into(),
        }
    }
}

/// Information about a fired rule.
#[pyclass]
#[derive(Clone, Debug)]
pub struct FiredRule {
    /// Name of the rule that fired.
    #[pyo3(get)]
    pub rule_name: String,
}

#[pymethods]
impl FiredRule {
    fn __repr__(&self) -> String {
        format!("FiredRule(rule_name={:?})", self.rule_name)
    }
}
