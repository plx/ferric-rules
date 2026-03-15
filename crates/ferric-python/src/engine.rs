//! Python Engine wrapper.

use std::path::Path;

use pyo3::prelude::*;
use pyo3::types::{PyDict, PyList};

use ferric_core::FactId;
use ferric_runtime::config::EngineConfig;
use ferric_runtime::execution::RunLimit;
use ferric_runtime::Engine;
use slotmap::{Key, KeyData};

use crate::config::{Encoding, Strategy};
use crate::error::{engine_error_to_pyerr, init_error_to_pyerr, load_errors_to_pyerr};
use crate::fact::{fact_to_python, Fact};
use crate::result::{FiredRule, RunResult};
use crate::value::{python_to_value, value_to_python};

/// Build an `EngineConfig` from optional Python args.
fn make_config(strategy: Option<Strategy>, encoding: Option<Encoding>) -> EngineConfig {
    let mut config = EngineConfig::default();
    if let Some(s) = strategy {
        config.strategy = s.into();
    }
    if let Some(e) = encoding {
        config.string_encoding = e.into();
    }
    config
}

/// The Ferric rules engine.
///
/// Thread-affine: must be used only from the thread that created it.
#[pyclass(name = "Engine", unsendable)]
pub struct PyEngine {
    engine: Engine,
}

#[pymethods]
impl PyEngine {
    /// Create a new engine.
    ///
    /// # Arguments
    ///
    /// * `strategy` — Conflict resolution strategy (default: `Strategy.DEPTH`).
    /// * `encoding` — String encoding mode (default: `Encoding.UTF8`).
    #[new]
    #[pyo3(signature = (*, strategy=None, encoding=None))]
    fn new(strategy: Option<Strategy>, encoding: Option<Encoding>) -> Self {
        let config = make_config(strategy, encoding);
        Self {
            engine: Engine::new(config),
        }
    }

    /// Create an engine from CLIPS source, loading and resetting in one step.
    #[staticmethod]
    #[pyo3(signature = (source, *, strategy=None, encoding=None))]
    fn from_source(
        source: &str,
        strategy: Option<Strategy>,
        encoding: Option<Encoding>,
    ) -> PyResult<Self> {
        let config = make_config(strategy, encoding);
        let engine = Engine::with_rules_config(source, config).map_err(init_error_to_pyerr)?;
        Ok(Self { engine })
    }

    // -- Context manager --

    fn __enter__(slf: Py<Self>) -> Py<Self> {
        slf
    }

    #[pyo3(signature = (_exc_type=None, _exc_val=None, _exc_tb=None))]
    fn __exit__(
        &mut self,
        _exc_type: Option<&Bound<'_, PyAny>>,
        _exc_val: Option<&Bound<'_, PyAny>>,
        _exc_tb: Option<&Bound<'_, PyAny>>,
    ) -> bool {
        self.engine.clear();
        false // don't suppress exceptions
    }

    // -- Loading --

    /// Load CLIPS source into the engine.
    fn load(&mut self, source: &str) -> PyResult<()> {
        self.engine.load_str(source).map_err(load_errors_to_pyerr)?;
        Ok(())
    }

    /// Load CLIPS source from a file path.
    fn load_file(&mut self, path: &str) -> PyResult<()> {
        self.engine
            .load_file(Path::new(path))
            .map_err(load_errors_to_pyerr)?;
        Ok(())
    }

    // -- Fact operations --

    /// Assert a fact from a CLIPS syntax string like `"(color red)"`.
    fn assert_string(&mut self, source: &str) -> PyResult<u64> {
        let wrapped = format!("(assert {source})");
        let result = self
            .engine
            .load_str(&wrapped)
            .map_err(load_errors_to_pyerr)?;
        if let Some(fid) = result.asserted_facts.first() {
            Ok(fid.data().as_ffi())
        } else {
            Err(crate::error::FerricError::new_err(
                "assert_string did not produce a fact",
            ))
        }
    }

    /// Assert a structured ordered fact.
    ///
    /// # Arguments
    ///
    /// * `relation` — The fact relation name.
    /// * `args` — The field values.
    #[pyo3(signature = (relation, *args))]
    fn assert_fact(
        &mut self,
        py: Python<'_>,
        relation: &str,
        args: &Bound<'_, PyTuple>,
    ) -> PyResult<u64> {
        let mut values = Vec::with_capacity(args.len());
        for item in args.iter() {
            values.push(python_to_value(&item, &mut self.engine)?);
        }
        let fid = self
            .engine
            .assert_ordered(relation, values)
            .map_err(engine_error_to_pyerr)?;
        let _ = py;
        Ok(fid.data().as_ffi())
    }

    /// Assert a structured template fact.
    ///
    /// # Arguments
    ///
    /// * `template_name` — The deftemplate name.
    /// * `kwargs` — Slot name/value pairs.
    ///
    /// # Example
    ///
    /// ```python
    /// engine.assert_template("person", name="Alice", age=30)
    /// ```
    #[pyo3(signature = (template_name, **kwargs))]
    fn assert_template(
        &mut self,
        template_name: &str,
        kwargs: Option<&Bound<'_, PyDict>>,
    ) -> PyResult<u64> {
        let (names, values) = match kwargs {
            Some(dict) => {
                let mut names = Vec::with_capacity(dict.len());
                let mut values = Vec::with_capacity(dict.len());
                for (key, val) in dict.iter() {
                    let name: String = key.extract()?;
                    names.push(name);
                    values.push(python_to_value(&val, &mut self.engine)?);
                }
                (names, values)
            }
            None => (Vec::new(), Vec::new()),
        };

        let name_refs: Vec<&str> = names.iter().map(String::as_str).collect();
        let fid = self
            .engine
            .assert_template(template_name, &name_refs, values)
            .map_err(engine_error_to_pyerr)?;
        Ok(fid.data().as_ffi())
    }

    /// Retract a fact by its ID.
    fn retract(&mut self, fact_id: u64) -> PyResult<()> {
        let fid = FactId::from(KeyData::from_ffi(fact_id));
        self.engine.retract(fid).map_err(engine_error_to_pyerr)
    }

    /// Get a fact by its ID, or `None` if it does not exist.
    fn get_fact(&self, py: Python<'_>, fact_id: u64) -> PyResult<Option<Fact>> {
        let fid = FactId::from(KeyData::from_ffi(fact_id));
        let fact = self.engine.get_fact(fid).map_err(engine_error_to_pyerr)?;
        match fact {
            Some(f) => Ok(Some(fact_to_python(py, fid, f, &self.engine)?)),
            None => Ok(None),
        }
    }

    /// Return all facts currently in working memory.
    fn facts(&self, py: Python<'_>) -> PyResult<Vec<Fact>> {
        let iter = self.engine.facts().map_err(engine_error_to_pyerr)?;
        let mut result = Vec::new();
        for (fid, fact) in iter {
            result.push(fact_to_python(py, fid, fact, &self.engine)?);
        }
        Ok(result)
    }

    /// Find facts by relation name.
    fn find_facts(&self, py: Python<'_>, relation: &str) -> PyResult<Vec<Fact>> {
        let facts = self
            .engine
            .find_facts(relation)
            .map_err(engine_error_to_pyerr)?;
        let mut result = Vec::new();
        for (fid, fact) in facts {
            result.push(fact_to_python(py, fid, fact, &self.engine)?);
        }
        Ok(result)
    }

    // -- Execution --

    /// Run the engine.
    ///
    /// # Arguments
    ///
    /// * `limit` — Maximum number of rule firings (default: unlimited).
    #[pyo3(signature = (*, limit=None))]
    fn run(&mut self, limit: Option<usize>) -> PyResult<RunResult> {
        let run_limit = match limit {
            Some(n) => RunLimit::Count(n),
            None => RunLimit::Unlimited,
        };
        let result = self.engine.run(run_limit).map_err(engine_error_to_pyerr)?;
        Ok(result.into())
    }

    /// Fire a single rule activation. Returns `FiredRule` or `None`.
    fn step(&mut self) -> PyResult<Option<FiredRule>> {
        let result = self.engine.step().map_err(engine_error_to_pyerr)?;
        Ok(result.map(|fr| {
            let name = self
                .engine
                .rule_name(fr.rule_id)
                .unwrap_or("<unknown>")
                .to_string();
            FiredRule { rule_name: name }
        }))
    }

    /// Request the engine to halt.
    fn halt(&mut self) {
        self.engine.halt();
    }

    /// Reset the engine: clear facts and re-assert deffacts.
    fn reset(&mut self) -> PyResult<()> {
        self.engine.reset().map_err(engine_error_to_pyerr)
    }

    /// Clear the engine: remove all rules, facts, templates, etc.
    fn clear(&mut self) {
        self.engine.clear();
    }

    // -- Properties --

    /// Number of user-visible facts.
    #[getter]
    fn fact_count(&self) -> PyResult<usize> {
        let count = self.engine.facts().map_err(engine_error_to_pyerr)?.count();
        Ok(count)
    }

    /// Whether the engine is currently halted.
    #[getter]
    fn is_halted(&self) -> bool {
        self.engine.is_halted()
    }

    /// Number of pending activations on the agenda.
    #[getter]
    fn agenda_size(&self) -> usize {
        self.engine.agenda_len()
    }

    /// Name of the current module.
    #[getter]
    fn current_module(&self) -> &str {
        self.engine.current_module()
    }

    /// Top of the focus stack, or `None`.
    #[getter]
    fn focus(&self) -> Option<&str> {
        self.engine.get_focus()
    }

    /// Full focus stack as a list of module names (bottom to top).
    #[getter]
    fn focus_stack(&self) -> Vec<String> {
        self.engine
            .get_focus_stack()
            .into_iter()
            .map(String::from)
            .collect()
    }

    /// Non-fatal action diagnostics from the most recent run/step.
    #[getter]
    fn diagnostics(&self) -> Vec<String> {
        self.engine
            .action_diagnostics()
            .iter()
            .map(ToString::to_string)
            .collect()
    }

    // -- Introspection --

    /// Return a list of `(name, salience)` tuples for all rules.
    fn rules(&self, py: Python<'_>) -> PyResult<PyObject> {
        let rules = self.engine.rules();
        let list = PyList::empty(py);
        for (name, salience) in rules {
            let tuple = pyo3::types::PyTuple::new(
                py,
                [
                    name.into_pyobject(py)?.into_any(),
                    salience.into_pyobject(py)?.into_any(),
                ],
            )?;
            list.append(tuple)?;
        }
        Ok(list.into_any().unbind())
    }

    /// Return a list of template names.
    fn templates(&self) -> Vec<String> {
        self.engine
            .templates()
            .into_iter()
            .map(String::from)
            .collect()
    }

    /// Get the value of a global variable, or `None`.
    fn get_global(&self, py: Python<'_>, name: &str) -> Option<PyObject> {
        self.engine
            .get_global(name)
            .map(|v| value_to_python(py, v, &self.engine))
    }

    // -- I/O --

    /// Get captured output for a channel (e.g. "stdout").
    fn get_output(&self, channel: &str) -> Option<String> {
        self.engine.get_output(channel).map(String::from)
    }

    /// Clear captured output for a channel.
    fn clear_output(&mut self, channel: &str) {
        self.engine.clear_output_channel(channel);
    }

    /// Push a line of input for `read`/`readline`.
    fn push_input(&mut self, line: &str) {
        self.engine.push_input(line);
    }

    // -- Serialization --

    /// Serialize the engine state to bytes in the given format.
    ///
    /// # Arguments
    ///
    /// * `format` — Serialization format (default: `Format.BINCODE`).
    ///
    /// Returns `bytes` containing the serialized engine state.
    #[cfg(feature = "serde")]
    #[pyo3(signature = (format=None))]
    fn serialize<'py>(
        &self,
        py: Python<'py>,
        format: Option<crate::config::Format>,
    ) -> PyResult<Bound<'py, pyo3::types::PyBytes>> {
        let fmt = format.unwrap_or(crate::config::Format::Bincode).into();
        let bytes = self
            .engine
            .serialize(fmt)
            .map_err(|e| crate::error::FerricError::new_err(e.to_string()))?;
        Ok(pyo3::types::PyBytes::new(py, &bytes))
    }

    /// Create an engine by deserializing a snapshot.
    ///
    /// # Arguments
    ///
    /// * `data` — Serialized engine state (bytes).
    /// * `format` — Serialization format (default: `Format.BINCODE`).
    /// * `strategy` — Conflict resolution strategy override (unused for snapshots).
    /// * `encoding` — String encoding mode override (unused for snapshots).
    #[staticmethod]
    #[cfg(feature = "serde")]
    #[pyo3(signature = (data, *, format=None))]
    fn from_snapshot(data: &[u8], format: Option<crate::config::Format>) -> PyResult<Self> {
        let fmt = format.unwrap_or(crate::config::Format::Bincode).into();
        let engine = Engine::deserialize(data, fmt)
            .map_err(|e| crate::error::FerricError::new_err(e.to_string()))?;
        Ok(Self { engine })
    }

    // -- Python protocols --

    fn __repr__(&self) -> PyResult<String> {
        let fact_count = self.engine.facts().map_err(engine_error_to_pyerr)?.count();
        let rule_count = self.engine.rules().len();
        let halted = self.engine.is_halted();
        Ok(format!(
            "Engine(facts={fact_count}, rules={rule_count}, halted={halted})"
        ))
    }

    fn __len__(&self) -> PyResult<usize> {
        let count = self.engine.facts().map_err(engine_error_to_pyerr)?.count();
        Ok(count)
    }

    fn __contains__(&self, fact_id: u64) -> PyResult<bool> {
        let fid = FactId::from(KeyData::from_ffi(fact_id));
        let fact = self.engine.get_fact(fid).map_err(engine_error_to_pyerr)?;
        Ok(fact.is_some())
    }
}

use pyo3::types::PyTuple;
