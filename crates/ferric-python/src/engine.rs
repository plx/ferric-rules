//! Python Engine wrapper.

use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::thread::ThreadId;

/// Global counter for assigning unique engine IDs.
static NEXT_ENGINE_ID: AtomicU64 = AtomicU64::new(1);

use pyo3::prelude::*;
use pyo3::types::{PyDict, PyList};

use ferric_core::FactId;
use ferric_runtime::config::EngineConfig;
use ferric_runtime::execution::RunLimit;
use ferric_runtime::Engine;
use slotmap::{Key, KeyData};

use crate::config::{Encoding, Strategy};
use crate::error::{
    engine_error_to_pyerr, init_error_to_pyerr, load_errors_to_pyerr, FerricRuntimeError,
};
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
/// Cross-thread access raises `FerricRuntimeError` (not a panic).
#[pyclass(name = "Engine", module = "ferric")]
pub struct PyEngine {
    engine: Engine,
    creator_thread: ThreadId,
    /// Unique identifier for this engine instance (used in Fact identity).
    engine_id: u64,
}

// SAFETY: We enforce thread affinity ourselves via `check_thread()` at every
// Python entry point, converting violations into `FerricRuntimeError` instead
// of the `PanicException` that PyO3's `unsendable` would produce.  The inner
// `Engine` is never actually accessed from a foreign thread.
unsafe impl Send for PyEngine {}
unsafe impl Sync for PyEngine {}

impl PyEngine {
    /// Check that the caller is on the thread that created this engine.
    fn check_thread(&self) -> PyResult<()> {
        let current = std::thread::current().id();
        if current != self.creator_thread {
            return Err(FerricRuntimeError::new_err(format!(
                "engine called from wrong thread (created on {:?}, called from {:?})",
                self.creator_thread, current,
            )));
        }
        Ok(())
    }
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
            creator_thread: std::thread::current().id(),
            engine_id: NEXT_ENGINE_ID.fetch_add(1, Ordering::Relaxed),
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
        Ok(Self {
            engine,
            creator_thread: std::thread::current().id(),
            engine_id: NEXT_ENGINE_ID.fetch_add(1, Ordering::Relaxed),
        })
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
    ) -> PyResult<bool> {
        self.check_thread()?;
        self.engine.clear();
        Ok(false) // don't suppress exceptions
    }

    // -- Loading --

    /// Load CLIPS source into the engine.
    fn load(&mut self, source: &str) -> PyResult<()> {
        self.check_thread()?;
        self.engine.load_str(source).map_err(load_errors_to_pyerr)?;
        Ok(())
    }

    /// Load CLIPS source from a file path (str or os.PathLike).
    fn load_file(&mut self, path: PathBuf) -> PyResult<()> {
        self.check_thread()?;
        self.engine
            .load_file(&path)
            .map_err(load_errors_to_pyerr)?;
        Ok(())
    }

    // -- Fact operations --

    /// Assert one or more facts from CLIPS syntax, e.g. `"(color red)"`.
    ///
    /// Returns a list of fact IDs for all asserted facts.
    ///
    /// # Example
    ///
    /// ```python
    /// ids = engine.assert_string("(color red) (color blue)")
    /// assert len(ids) == 2
    /// ```
    fn assert_string(&mut self, source: &str) -> PyResult<Vec<u64>> {
        self.check_thread()?;
        let wrapped = format!("(assert {source})");
        let result = self
            .engine
            .load_str(&wrapped)
            .map_err(load_errors_to_pyerr)?;
        if result.asserted_facts.is_empty() {
            return Err(crate::error::FerricError::new_err(
                "assert_string did not produce any facts",
            ));
        }
        Ok(result
            .asserted_facts
            .iter()
            .map(|fid| fid.data().as_ffi())
            .collect())
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
        self.check_thread()?;
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
        self.check_thread()?;
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
        self.check_thread()?;
        let fid = FactId::from(KeyData::from_ffi(fact_id));
        self.engine.retract(fid).map_err(engine_error_to_pyerr)
    }

    /// Get a fact by its ID, or `None` if it does not exist.
    fn get_fact(&self, py: Python<'_>, fact_id: u64) -> PyResult<Option<Fact>> {
        self.check_thread()?;
        let fid = FactId::from(KeyData::from_ffi(fact_id));
        let fact = self.engine.get_fact(fid).map_err(engine_error_to_pyerr)?;
        match fact {
            Some(f) => Ok(Some(fact_to_python(py, fid, f, &self.engine, self.engine_id)?)),
            None => Ok(None),
        }
    }

    /// Return all facts currently in working memory.
    fn facts(&self, py: Python<'_>) -> PyResult<Vec<Fact>> {
        self.check_thread()?;
        let iter = self.engine.facts().map_err(engine_error_to_pyerr)?;
        let mut result = Vec::new();
        for (fid, fact) in iter {
            result.push(fact_to_python(py, fid, fact, &self.engine, self.engine_id)?);
        }
        Ok(result)
    }

    /// Find facts by relation name.
    fn find_facts(&self, py: Python<'_>, relation: &str) -> PyResult<Vec<Fact>> {
        self.check_thread()?;
        let facts = self
            .engine
            .find_facts(relation)
            .map_err(engine_error_to_pyerr)?;
        let mut result = Vec::new();
        for (fid, fact) in facts {
            result.push(fact_to_python(py, fid, fact, &self.engine, self.engine_id)?);
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
        self.check_thread()?;
        let run_limit = match limit {
            Some(n) => RunLimit::Count(n),
            None => RunLimit::Unlimited,
        };
        let result = self.engine.run(run_limit).map_err(engine_error_to_pyerr)?;
        Ok(result.into())
    }

    /// Fire a single rule activation. Returns `FiredRule` or `None`.
    fn step(&mut self) -> PyResult<Option<FiredRule>> {
        self.check_thread()?;
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
    fn halt(&mut self) -> PyResult<()> {
        self.check_thread()?;
        self.engine.halt();
        Ok(())
    }

    /// Reset the engine: clear facts and re-assert deffacts.
    fn reset(&mut self) -> PyResult<()> {
        self.check_thread()?;
        self.engine.reset().map_err(engine_error_to_pyerr)
    }

    /// Clear the engine: remove all rules, facts, templates, etc.
    fn clear(&mut self) -> PyResult<()> {
        self.check_thread()?;
        self.engine.clear();
        Ok(())
    }

    // -- Properties --

    /// Number of user-visible facts.
    #[getter]
    fn fact_count(&self) -> PyResult<usize> {
        self.check_thread()?;
        let count = self.engine.facts().map_err(engine_error_to_pyerr)?.count();
        Ok(count)
    }

    /// Whether the engine is currently halted.
    #[getter]
    fn is_halted(&self) -> PyResult<bool> {
        self.check_thread()?;
        Ok(self.engine.is_halted())
    }

    /// Number of pending activations on the agenda.
    #[getter]
    fn agenda_size(&self) -> PyResult<usize> {
        self.check_thread()?;
        Ok(self.engine.agenda_len())
    }

    /// Name of the current module.
    #[getter]
    fn current_module(&self) -> PyResult<String> {
        self.check_thread()?;
        Ok(self.engine.current_module().to_owned())
    }

    /// Top of the focus stack, or `None`.
    #[getter]
    fn focus(&self) -> PyResult<Option<String>> {
        self.check_thread()?;
        Ok(self.engine.get_focus().map(str::to_owned))
    }

    /// Full focus stack as a list of module names (bottom to top).
    #[getter]
    fn focus_stack(&self) -> PyResult<Vec<String>> {
        self.check_thread()?;
        Ok(self
            .engine
            .get_focus_stack()
            .into_iter()
            .map(String::from)
            .collect())
    }

    /// Non-fatal action diagnostics from the most recent run/step.
    #[getter]
    fn diagnostics(&self) -> PyResult<Vec<String>> {
        self.check_thread()?;
        Ok(self
            .engine
            .action_diagnostics()
            .iter()
            .map(ToString::to_string)
            .collect())
    }

    /// Set focus to exactly one module, replacing the previous focus stack.
    fn set_focus(&mut self, module_name: &str) -> PyResult<()> {
        self.check_thread()?;
        self.engine
            .set_focus(module_name)
            .map_err(engine_error_to_pyerr)
    }

    /// Push a module onto the focus stack.
    fn push_focus(&mut self, module_name: &str) -> PyResult<()> {
        self.check_thread()?;
        self.engine
            .push_focus(module_name)
            .map_err(engine_error_to_pyerr)
    }

    /// Return a list of registered module names.
    fn modules(&self) -> PyResult<Vec<String>> {
        self.check_thread()?;
        Ok(self
            .engine
            .modules()
            .into_iter()
            .map(String::from)
            .collect())
    }

    /// Clear accumulated action diagnostics.
    fn clear_diagnostics(&mut self) -> PyResult<()> {
        self.check_thread()?;
        self.engine.clear_action_diagnostics();
        Ok(())
    }

    /// Get the value of a template fact slot by name.
    fn get_fact_slot(&self, py: Python<'_>, fact_id: u64, slot_name: &str) -> PyResult<PyObject> {
        self.check_thread()?;
        let fid = FactId::from(KeyData::from_ffi(fact_id));
        let val = self
            .engine
            .get_fact_slot_by_name(fid, slot_name)
            .map_err(engine_error_to_pyerr)?;
        Ok(value_to_python(py, val, &self.engine))
    }

    // -- Introspection --

    /// Return a list of `(name, salience)` tuples for all rules.
    fn rules(&self, py: Python<'_>) -> PyResult<PyObject> {
        self.check_thread()?;
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
    fn templates(&self) -> PyResult<Vec<String>> {
        self.check_thread()?;
        Ok(self
            .engine
            .templates()
            .into_iter()
            .map(String::from)
            .collect())
    }

    /// Get the value of a global variable, or `None`.
    fn get_global(&self, py: Python<'_>, name: &str) -> PyResult<Option<PyObject>> {
        self.check_thread()?;
        Ok(self
            .engine
            .get_global(name)
            .map(|v| value_to_python(py, v, &self.engine)))
    }

    // -- I/O --

    /// Get captured output for a channel (e.g. "stdout").
    fn get_output(&self, channel: &str) -> PyResult<Option<String>> {
        self.check_thread()?;
        Ok(self.engine.get_output(channel).map(String::from))
    }

    /// Clear captured output for a channel.
    fn clear_output(&mut self, channel: &str) -> PyResult<()> {
        self.check_thread()?;
        self.engine.clear_output_channel(channel);
        Ok(())
    }

    /// Push a line of input for `read`/`readline`.
    fn push_input(&mut self, line: &str) -> PyResult<()> {
        self.check_thread()?;
        self.engine.push_input(line);
        Ok(())
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
        self.check_thread()?;
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
        Ok(Self {
            engine,
            creator_thread: std::thread::current().id(),
            engine_id: NEXT_ENGINE_ID.fetch_add(1, Ordering::Relaxed),
        })
    }

    /// Save a serialized engine snapshot to a file.
    ///
    /// # Arguments
    ///
    /// * `path` — File path (str or os.PathLike).
    /// * `format` — Serialization format (default: `Format.BINCODE`).
    #[cfg(feature = "serde")]
    #[pyo3(signature = (path, *, format=None))]
    fn save_snapshot(&self, path: PathBuf, format: Option<crate::config::Format>) -> PyResult<()> {
        self.check_thread()?;
        let fmt = format.unwrap_or(crate::config::Format::Bincode).into();
        let bytes = self
            .engine
            .serialize(fmt)
            .map_err(|e| crate::error::FerricError::new_err(e.to_string()))?;
        std::fs::write(&path, &bytes)
            .map_err(|e| pyo3::exceptions::PyIOError::new_err(e.to_string()))?;
        Ok(())
    }

    /// Create an engine by deserializing a snapshot from a file.
    ///
    /// # Arguments
    ///
    /// * `path` — File path (str or os.PathLike).
    /// * `format` — Serialization format (default: `Format.BINCODE`).
    #[staticmethod]
    #[cfg(feature = "serde")]
    #[pyo3(signature = (path, *, format=None))]
    fn from_snapshot_file(path: PathBuf, format: Option<crate::config::Format>) -> PyResult<Self> {
        let data = std::fs::read(&path)
            .map_err(|e| pyo3::exceptions::PyIOError::new_err(e.to_string()))?;
        let fmt = format.unwrap_or(crate::config::Format::Bincode).into();
        let engine = Engine::deserialize(&data, fmt)
            .map_err(|e| crate::error::FerricError::new_err(e.to_string()))?;
        Ok(Self {
            engine,
            creator_thread: std::thread::current().id(),
            engine_id: NEXT_ENGINE_ID.fetch_add(1, Ordering::Relaxed),
        })
    }

    // -- Python protocols --

    fn __repr__(&self) -> PyResult<String> {
        self.check_thread()?;
        let fact_count = self.engine.facts().map_err(engine_error_to_pyerr)?.count();
        let rule_count = self.engine.rules().len();
        let halted = self.engine.is_halted();
        Ok(format!(
            "Engine(facts={fact_count}, rules={rule_count}, halted={halted})"
        ))
    }

    fn __len__(&self) -> PyResult<usize> {
        self.check_thread()?;
        let count = self.engine.facts().map_err(engine_error_to_pyerr)?.count();
        Ok(count)
    }

    fn __contains__(&self, fact_id: u64) -> PyResult<bool> {
        self.check_thread()?;
        let fid = FactId::from(KeyData::from_ffi(fact_id));
        let fact = self.engine.get_fact(fid).map_err(engine_error_to_pyerr)?;
        Ok(fact.is_some())
    }
}

use pyo3::types::PyTuple;
