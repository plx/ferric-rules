//! Python Engine wrapper.
//!
//! Engine instances live in a thread-local registry on their creator thread.
//! `PyEngine` is a lightweight handle (engine ID + thread ID) that is naturally
//! `Send + Sync` — no `unsafe` required.

use std::cell::RefCell;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::thread::{self, ThreadId};

use pyo3::prelude::*;
use pyo3::types::{PyDict, PyList, PyTuple};

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

/// Global counter for assigning unique engine IDs.
static NEXT_ENGINE_ID: AtomicU64 = AtomicU64::new(1);

/// Live engine instance count (testing instrumentation only).
#[cfg(feature = "testing")]
static ENGINE_INSTANCE_COUNT: AtomicU64 = AtomicU64::new(0);

thread_local! {
    /// Thread-local registry of engines owned by this thread.
    static ENGINES: RefCell<HashMap<u64, Engine>> = RefCell::new(HashMap::new());
}

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

/// Register a newly-created engine in the thread-local registry.
fn register_engine(engine: Engine) -> (u64, ThreadId) {
    let engine_id = NEXT_ENGINE_ID.fetch_add(1, Ordering::Relaxed);
    ENGINES.with(|engines| {
        engines.borrow_mut().insert(engine_id, engine);
    });
    #[cfg(feature = "testing")]
    ENGINE_INSTANCE_COUNT.fetch_add(1, Ordering::Relaxed);
    (engine_id, thread::current().id())
}

/// The Ferric rules engine.
///
/// Thread-affine: must be used only from the thread that created it.
/// Cross-thread access raises `FerricRuntimeError` (not a panic).
///
/// The actual engine data lives in a thread-local registry on the creator
/// thread.  This struct is a lightweight handle that is naturally `Send + Sync`.
#[pyclass(name = "Engine", module = "ferric")]
pub struct PyEngine {
    engine_id: u64,
    creator_thread: ThreadId,
}

impl PyEngine {
    /// Check thread + look up engine in TLS; run closure with `&mut Engine`.
    fn with_engine<F, R>(&self, f: F) -> PyResult<R>
    where
        F: FnOnce(&mut Engine) -> PyResult<R>,
    {
        let current = thread::current().id();
        if current != self.creator_thread {
            return Err(FerricRuntimeError::new_err(format!(
                "engine called from wrong thread (created on {:?}, called from {:?})",
                self.creator_thread, current,
            )));
        }
        ENGINES.with(|engines| {
            let mut map = engines.borrow_mut();
            let engine = map
                .get_mut(&self.engine_id)
                .ok_or_else(|| FerricRuntimeError::new_err("engine has been closed"))?;
            f(engine)
        })
    }

    /// Remove engine from registry.  Returns `true` if it was present.
    fn remove_engine(&self) -> bool {
        if thread::current().id() != self.creator_thread {
            return false;
        }
        let removed =
            ENGINES.with(|engines| engines.borrow_mut().remove(&self.engine_id).is_some());
        #[cfg(feature = "testing")]
        if removed {
            ENGINE_INSTANCE_COUNT.fetch_sub(1, Ordering::Relaxed);
        }
        removed
    }
}

impl Drop for PyEngine {
    fn drop(&mut self) {
        if thread::current().id() == self.creator_thread {
            // Try to remove from TLS.  `try_with` handles TLS-already-destroyed
            // during interpreter shutdown.
            let _removed = ENGINES
                .try_with(|engines| engines.borrow_mut().remove(&self.engine_id).is_some())
                .unwrap_or(false);
            #[cfg(feature = "testing")]
            if _removed {
                ENGINE_INSTANCE_COUNT.fetch_sub(1, Ordering::Relaxed);
            }
        }
        // Foreign thread: no-op.  Engine stays in creator thread's TLS and is
        // cleaned up when that thread exits (TLS destructors run).
    }
}

#[pymethods]
impl PyEngine {
    /// Create a new engine.
    ///
    /// # Arguments
    ///
    /// * `strategy` -- Conflict resolution strategy (default: `Strategy.DEPTH`).
    /// * `encoding` -- String encoding mode (default: `Encoding.UTF8`).
    #[new]
    #[pyo3(signature = (*, strategy=None, encoding=None))]
    fn new(strategy: Option<Strategy>, encoding: Option<Encoding>) -> Self {
        let config = make_config(strategy, encoding);
        let (engine_id, creator_thread) = register_engine(Engine::new(config));
        Self {
            engine_id,
            creator_thread,
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
        let (engine_id, creator_thread) = register_engine(engine);
        Ok(Self {
            engine_id,
            creator_thread,
        })
    }

    // -- Context manager --

    fn __enter__(slf: Py<Self>) -> Py<Self> {
        slf
    }

    #[pyo3(signature = (_exc_type=None, _exc_val=None, _exc_tb=None))]
    fn __exit__(
        &self,
        _exc_type: Option<&Bound<'_, PyAny>>,
        _exc_val: Option<&Bound<'_, PyAny>>,
        _exc_tb: Option<&Bound<'_, PyAny>>,
    ) -> PyResult<bool> {
        self.close()?;
        Ok(false) // don't suppress exceptions
    }

    /// Explicitly close and destroy this engine.
    ///
    /// After calling `close()`, any further method calls will raise
    /// `FerricRuntimeError`.  This is idempotent.
    fn close(&self) -> PyResult<()> {
        let current = thread::current().id();
        if current != self.creator_thread {
            return Err(FerricRuntimeError::new_err(format!(
                "engine called from wrong thread (created on {:?}, called from {:?})",
                self.creator_thread, current,
            )));
        }
        self.remove_engine();
        Ok(())
    }

    // -- Loading --

    /// Load CLIPS source into the engine.
    fn load(&self, source: &str) -> PyResult<()> {
        self.with_engine(|engine| {
            engine.load_str(source).map_err(load_errors_to_pyerr)?;
            Ok(())
        })
    }

    /// Load CLIPS source from a file path (str or os.PathLike).
    fn load_file(&self, path: PathBuf) -> PyResult<()> {
        self.with_engine(|engine| {
            engine.load_file(&path).map_err(load_errors_to_pyerr)?;
            Ok(())
        })
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
    fn assert_string(&self, source: &str) -> PyResult<Vec<u64>> {
        self.with_engine(|engine| {
            let wrapped = format!("(assert {source})");
            let result = engine.load_str(&wrapped).map_err(load_errors_to_pyerr)?;
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
        })
    }

    /// Assert a structured ordered fact.
    ///
    /// # Arguments
    ///
    /// * `relation` -- The fact relation name.
    /// * `args` -- The field values.
    #[pyo3(signature = (relation, *args))]
    fn assert_fact(
        &self,
        py: Python<'_>,
        relation: &str,
        args: &Bound<'_, PyTuple>,
    ) -> PyResult<u64> {
        let _ = py;
        self.with_engine(|engine| {
            let mut values = Vec::with_capacity(args.len());
            for item in args.iter() {
                values.push(python_to_value(&item, engine)?);
            }
            let fid = engine
                .assert_ordered(relation, values)
                .map_err(engine_error_to_pyerr)?;
            Ok(fid.data().as_ffi())
        })
    }

    /// Assert a structured template fact.
    ///
    /// # Arguments
    ///
    /// * `template_name` -- The deftemplate name.
    /// * `kwargs` -- Slot name/value pairs.
    ///
    /// # Example
    ///
    /// ```python
    /// engine.assert_template("person", name="Alice", age=30)
    /// ```
    #[pyo3(signature = (template_name, **kwargs))]
    fn assert_template(
        &self,
        template_name: &str,
        kwargs: Option<&Bound<'_, PyDict>>,
    ) -> PyResult<u64> {
        self.with_engine(|engine| {
            let (names, values) = match kwargs {
                Some(dict) => {
                    let mut names = Vec::with_capacity(dict.len());
                    let mut values = Vec::with_capacity(dict.len());
                    for (key, val) in dict.iter() {
                        let name: String = key.extract()?;
                        names.push(name);
                        values.push(python_to_value(&val, engine)?);
                    }
                    (names, values)
                }
                None => (Vec::new(), Vec::new()),
            };

            let name_refs: Vec<&str> = names.iter().map(String::as_str).collect();
            let fid = engine
                .assert_template(template_name, &name_refs, values)
                .map_err(engine_error_to_pyerr)?;
            Ok(fid.data().as_ffi())
        })
    }

    /// Retract a fact by its ID.
    fn retract(&self, fact_id: u64) -> PyResult<()> {
        self.with_engine(|engine| {
            let fid = FactId::from(KeyData::from_ffi(fact_id));
            engine.retract(fid).map_err(engine_error_to_pyerr)
        })
    }

    /// Get a fact by its ID, or `None` if it does not exist.
    fn get_fact(&self, py: Python<'_>, fact_id: u64) -> PyResult<Option<Fact>> {
        let eid = self.engine_id;
        self.with_engine(|engine| {
            let fid = FactId::from(KeyData::from_ffi(fact_id));
            let fact = engine.get_fact(fid).map_err(engine_error_to_pyerr)?;
            match fact {
                Some(f) => Ok(Some(fact_to_python(py, fid, f, engine, eid)?)),
                None => Ok(None),
            }
        })
    }

    /// Return all facts currently in working memory.
    fn facts(&self, py: Python<'_>) -> PyResult<Vec<Fact>> {
        let eid = self.engine_id;
        self.with_engine(|engine| {
            let iter = engine.facts().map_err(engine_error_to_pyerr)?;
            let mut result = Vec::new();
            for (fid, fact) in iter {
                result.push(fact_to_python(py, fid, fact, engine, eid)?);
            }
            Ok(result)
        })
    }

    /// Find facts by relation name.
    fn find_facts(&self, py: Python<'_>, relation: &str) -> PyResult<Vec<Fact>> {
        let eid = self.engine_id;
        self.with_engine(|engine| {
            let facts = engine.find_facts(relation).map_err(engine_error_to_pyerr)?;
            let mut result = Vec::new();
            for (fid, fact) in facts {
                result.push(fact_to_python(py, fid, fact, engine, eid)?);
            }
            Ok(result)
        })
    }

    // -- Execution --

    /// Run the engine.
    ///
    /// # Arguments
    ///
    /// * `limit` -- Maximum number of rule firings (default: unlimited).
    #[pyo3(signature = (*, limit=None))]
    fn run(&self, limit: Option<usize>) -> PyResult<RunResult> {
        self.with_engine(|engine| {
            let run_limit = match limit {
                Some(n) => RunLimit::Count(n),
                None => RunLimit::Unlimited,
            };
            let result = engine.run(run_limit).map_err(engine_error_to_pyerr)?;
            Ok(result.into())
        })
    }

    /// Fire a single rule activation. Returns `FiredRule` or `None`.
    fn step(&self) -> PyResult<Option<FiredRule>> {
        self.with_engine(|engine| {
            let result = engine.step().map_err(engine_error_to_pyerr)?;
            Ok(result.map(|fr| {
                let name = engine
                    .rule_name(fr.rule_id)
                    .unwrap_or("<unknown>")
                    .to_string();
                FiredRule { rule_name: name }
            }))
        })
    }

    /// Request the engine to halt.
    fn halt(&self) -> PyResult<()> {
        self.with_engine(|engine| {
            engine.halt();
            Ok(())
        })
    }

    /// Reset the engine: clear facts and re-assert deffacts.
    fn reset(&self) -> PyResult<()> {
        self.with_engine(|engine| engine.reset().map_err(engine_error_to_pyerr))
    }

    /// Clear the engine: remove all rules, facts, templates, etc.
    fn clear(&self) -> PyResult<()> {
        self.with_engine(|engine| {
            engine.clear();
            Ok(())
        })
    }

    // -- Properties --

    /// Number of user-visible facts.
    #[getter]
    fn fact_count(&self) -> PyResult<usize> {
        self.with_engine(|engine| {
            let count = engine.facts().map_err(engine_error_to_pyerr)?.count();
            Ok(count)
        })
    }

    /// Whether the engine is currently halted.
    #[getter]
    fn is_halted(&self) -> PyResult<bool> {
        self.with_engine(|engine| Ok(engine.is_halted()))
    }

    /// Number of pending activations on the agenda.
    #[getter]
    fn agenda_size(&self) -> PyResult<usize> {
        self.with_engine(|engine| Ok(engine.agenda_len()))
    }

    /// Name of the current module.
    #[getter]
    fn current_module(&self) -> PyResult<String> {
        self.with_engine(|engine| Ok(engine.current_module().to_owned()))
    }

    /// Top of the focus stack, or `None`.
    #[getter]
    fn focus(&self) -> PyResult<Option<String>> {
        self.with_engine(|engine| Ok(engine.get_focus().map(str::to_owned)))
    }

    /// Full focus stack as a list of module names (bottom to top).
    #[getter]
    fn focus_stack(&self) -> PyResult<Vec<String>> {
        self.with_engine(|engine| {
            Ok(engine
                .get_focus_stack()
                .into_iter()
                .map(String::from)
                .collect())
        })
    }

    /// Non-fatal action diagnostics from the most recent run/step.
    #[getter]
    fn diagnostics(&self) -> PyResult<Vec<String>> {
        self.with_engine(|engine| {
            Ok(engine
                .action_diagnostics()
                .iter()
                .map(ToString::to_string)
                .collect())
        })
    }

    /// Set focus to exactly one module, replacing the previous focus stack.
    fn set_focus(&self, module_name: &str) -> PyResult<()> {
        self.with_engine(|engine| engine.set_focus(module_name).map_err(engine_error_to_pyerr))
    }

    /// Push a module onto the focus stack.
    fn push_focus(&self, module_name: &str) -> PyResult<()> {
        self.with_engine(|engine| {
            engine
                .push_focus(module_name)
                .map_err(engine_error_to_pyerr)
        })
    }

    /// Return a list of registered module names.
    fn modules(&self) -> PyResult<Vec<String>> {
        self.with_engine(|engine| Ok(engine.modules().into_iter().map(String::from).collect()))
    }

    /// Clear accumulated action diagnostics.
    fn clear_diagnostics(&self) -> PyResult<()> {
        self.with_engine(|engine| {
            engine.clear_action_diagnostics();
            Ok(())
        })
    }

    /// Get the value of a template fact slot by name.
    fn get_fact_slot(&self, py: Python<'_>, fact_id: u64, slot_name: &str) -> PyResult<PyObject> {
        self.with_engine(|engine| {
            let fid = FactId::from(KeyData::from_ffi(fact_id));
            let val = engine
                .get_fact_slot_by_name(fid, slot_name)
                .map_err(engine_error_to_pyerr)?;
            value_to_python(py, val, engine)
        })
    }

    // -- Introspection --

    /// Return a list of `(name, salience)` tuples for all rules.
    fn rules(&self, py: Python<'_>) -> PyResult<PyObject> {
        self.with_engine(|engine| {
            let rules = engine.rules();
            let list = PyList::empty(py);
            for (name, salience) in rules {
                let tuple = PyTuple::new(
                    py,
                    [
                        name.into_pyobject(py)?.into_any(),
                        salience.into_pyobject(py)?.into_any(),
                    ],
                )?;
                list.append(tuple)?;
            }
            Ok(list.into_any().unbind())
        })
    }

    /// Return a list of template names.
    fn templates(&self) -> PyResult<Vec<String>> {
        self.with_engine(|engine| Ok(engine.templates().into_iter().map(String::from).collect()))
    }

    /// Get the value of a global variable, or `None`.
    fn get_global(&self, py: Python<'_>, name: &str) -> PyResult<Option<PyObject>> {
        self.with_engine(|engine| {
            engine
                .get_global(name)
                .map(|v| value_to_python(py, v, engine))
                .transpose()
        })
    }

    // -- I/O --

    /// Get captured output for a channel (e.g. "stdout").
    fn get_output(&self, channel: &str) -> PyResult<Option<String>> {
        self.with_engine(|engine| Ok(engine.get_output(channel).map(String::from)))
    }

    /// Clear captured output for a channel.
    fn clear_output(&self, channel: &str) -> PyResult<()> {
        self.with_engine(|engine| {
            engine.clear_output_channel(channel);
            Ok(())
        })
    }

    /// Push a line of input for `read`/`readline`.
    fn push_input(&self, line: &str) -> PyResult<()> {
        self.with_engine(|engine| {
            engine.push_input(line);
            Ok(())
        })
    }

    // -- Serialization --

    /// Serialize the engine state to bytes in the given format.
    ///
    /// # Arguments
    ///
    /// * `format` -- Serialization format (default: `Format.BINCODE`).
    ///
    /// Returns `bytes` containing the serialized engine state.
    #[cfg(feature = "serde")]
    #[pyo3(signature = (format=None))]
    fn serialize<'py>(
        &self,
        py: Python<'py>,
        format: Option<crate::config::Format>,
    ) -> PyResult<Bound<'py, pyo3::types::PyBytes>> {
        self.with_engine(|engine| {
            let fmt = format.unwrap_or(crate::config::Format::Bincode).into();
            let bytes = engine
                .serialize(fmt)
                .map_err(|e| crate::error::FerricError::new_err(e.to_string()))?;
            Ok(pyo3::types::PyBytes::new(py, &bytes))
        })
    }

    /// Create an engine by deserializing a snapshot.
    ///
    /// # Arguments
    ///
    /// * `data` -- Serialized engine state (bytes).
    /// * `format` -- Serialization format (default: `Format.BINCODE`).
    #[staticmethod]
    #[cfg(feature = "serde")]
    #[pyo3(signature = (data, *, format=None))]
    fn from_snapshot(data: &[u8], format: Option<crate::config::Format>) -> PyResult<Self> {
        let fmt = format.unwrap_or(crate::config::Format::Bincode).into();
        let engine = Engine::deserialize(data, fmt)
            .map_err(|e| crate::error::FerricError::new_err(e.to_string()))?;
        let (engine_id, creator_thread) = register_engine(engine);
        Ok(Self {
            engine_id,
            creator_thread,
        })
    }

    /// Save a serialized engine snapshot to a file.
    ///
    /// # Arguments
    ///
    /// * `path` -- File path (str or os.PathLike).
    /// * `format` -- Serialization format (default: `Format.BINCODE`).
    #[cfg(feature = "serde")]
    #[pyo3(signature = (path, *, format=None))]
    fn save_snapshot(&self, path: PathBuf, format: Option<crate::config::Format>) -> PyResult<()> {
        self.with_engine(|engine| {
            let fmt = format.unwrap_or(crate::config::Format::Bincode).into();
            let bytes = engine
                .serialize(fmt)
                .map_err(|e| crate::error::FerricError::new_err(e.to_string()))?;
            std::fs::write(&path, &bytes)
                .map_err(|e| pyo3::exceptions::PyIOError::new_err(e.to_string()))?;
            Ok(())
        })
    }

    /// Create an engine by deserializing a snapshot from a file.
    ///
    /// # Arguments
    ///
    /// * `path` -- File path (str or os.PathLike).
    /// * `format` -- Serialization format (default: `Format.BINCODE`).
    #[staticmethod]
    #[cfg(feature = "serde")]
    #[pyo3(signature = (path, *, format=None))]
    fn from_snapshot_file(path: PathBuf, format: Option<crate::config::Format>) -> PyResult<Self> {
        let data = std::fs::read(&path)
            .map_err(|e| pyo3::exceptions::PyIOError::new_err(e.to_string()))?;
        let fmt = format.unwrap_or(crate::config::Format::Bincode).into();
        let engine = Engine::deserialize(&data, fmt)
            .map_err(|e| crate::error::FerricError::new_err(e.to_string()))?;
        let (engine_id, creator_thread) = register_engine(engine);
        Ok(Self {
            engine_id,
            creator_thread,
        })
    }

    // -- Python protocols --

    fn __repr__(&self) -> PyResult<String> {
        self.with_engine(|engine| {
            let fact_count = engine.facts().map_err(engine_error_to_pyerr)?.count();
            let rule_count = engine.rules().len();
            let halted = engine.is_halted();
            Ok(format!(
                "Engine(facts={fact_count}, rules={rule_count}, halted={halted})"
            ))
        })
    }

    fn __len__(&self) -> PyResult<usize> {
        self.with_engine(|engine| {
            let count = engine.facts().map_err(engine_error_to_pyerr)?.count();
            Ok(count)
        })
    }

    fn __contains__(&self, fact_id: u64) -> PyResult<bool> {
        self.with_engine(|engine| {
            let fid = FactId::from(KeyData::from_ffi(fact_id));
            let fact = engine.get_fact(fid).map_err(engine_error_to_pyerr)?;
            Ok(fact.is_some())
        })
    }
}

/// Return the number of live engine instances (testing instrumentation).
#[cfg(feature = "testing")]
#[pyfunction]
pub fn engine_instance_count() -> u64 {
    ENGINE_INSTANCE_COUNT.load(Ordering::Relaxed)
}
