//! Python Engine wrapper.
//!
//! Engine operations remain pinned to the creator thread. `PyEngine` owns the
//! engine behind a mutex so explicit close or Python's final-reference cleanup
//! can destroy it synchronously on another thread without transferring runtime
//! access.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread::{self, ThreadId};

use pyo3::prelude::*;
use pyo3::types::{PyDict, PyList, PyTuple};

use ferric_rules_core::FactId;
use ferric_rules_runtime::config::EngineConfig;
use ferric_rules_runtime::execution::RunLimit;
use ferric_rules_runtime::{
    Engine, EngineError, HaltReason as NativeHaltReason, RunResult as NativeRunResult,
};
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

/// Maximum native rule firings between external cancellation checks.
const RUN_CANCEL_CHUNK_SIZE: usize = 64;

/// Live engine instance count (testing instrumentation only).
#[cfg(feature = "testing")]
static ENGINE_INSTANCE_COUNT: AtomicU64 = AtomicU64::new(0);

/// Exclusive ownership of a thread-affine engine which may be sent solely so
/// the whole engine can be destroyed on the thread that releases Python's
/// final reference.
struct OwnedThreadAffineEngine {
    // Rust drops struct fields in declaration order. Keep the native engine
    // before the testing guard so the live count changes only after Engine's
    // destructor has completed.
    engine: Box<Engine>,
    #[cfg(feature = "testing")]
    _instance_count: EngineInstanceCount,
}

#[cfg(feature = "testing")]
struct EngineInstanceCount;

#[cfg(feature = "testing")]
impl EngineInstanceCount {
    fn new() -> Self {
        ENGINE_INSTANCE_COUNT.fetch_add(1, Ordering::Relaxed);
        Self
    }
}

#[cfg(feature = "testing")]
impl Drop for EngineInstanceCount {
    fn drop(&mut self) {
        ENGINE_INSTANCE_COUNT.fetch_sub(1, Ordering::Relaxed);
    }
}

impl OwnedThreadAffineEngine {
    fn new(engine: Engine) -> Self {
        Self {
            engine: Box::new(engine),
            #[cfg(feature = "testing")]
            _instance_count: EngineInstanceCount::new(),
        }
    }

    fn get_mut(&mut self) -> &mut Engine {
        &mut self.engine
    }
}

// SAFETY: `OwnedThreadAffineEngine` is private and is only stored behind
// `PyEngine`'s mutex. Runtime access is granted only after checking that the
// current thread is the engine's creator, and the mutex prevents that access
// from overlapping destruction. Actual cross-thread movement is used only to
// drop the whole, exclusively owned engine, matching the destruction-only
// exception documented by `ferric_engine_free_unchecked`. Construction also
// returns this wrapper through `Python::allow_threads`, whose closure executes
// synchronously on the invoking OS thread; that round trip does not transfer
// runtime access to another thread.
unsafe impl Send for OwnedThreadAffineEngine {}

/// Exclusive, creator-thread engine access for one GIL-released native phase.
///
/// `PyO3` 0.23 models `Ungil` as `Send` on stable Rust. `Engine` is deliberately
/// `!Send`, so this narrow lease supplies only that conservative marker bound.
/// It is never exposed or stored and is consumed directly by
/// `Python::allow_threads`.
struct GilReleasedEngineOperation<'a> {
    engine: &'a mut Engine,
    invoking_thread: ThreadId,
}

impl<'a> GilReleasedEngineOperation<'a> {
    fn new(engine: &'a mut Engine) -> Self {
        Self {
            engine,
            invoking_thread: thread::current().id(),
        }
    }

    fn run<F, R>(self, operation: F) -> R
    where
        F: FnOnce(&mut Engine) -> R,
    {
        debug_assert_eq!(thread::current().id(), self.invoking_thread);
        operation(self.engine)
    }
}

// SAFETY: PyO3 documents that `Python::allow_threads` releases the GIL around
// a synchronous call on the invoking OS thread; it does not launch a thread.
// `PyEngine` creates this private lease only after the creator-thread check and
// while holding the engine mutex, moves it directly into that call, and keeps
// the mutex reserved until the call returns. No API can move or retain it.
unsafe impl Send for GilReleasedEngineOperation<'_> {}

/// A per-run cancellation token published only while a run is active.
#[derive(Default)]
struct ActiveRunControl {
    active: Mutex<Option<Arc<AtomicBool>>>,
}

impl ActiveRunControl {
    fn publish<'a>(&'a self, closing: &AtomicBool) -> ActiveRunGuard<'a> {
        let token = Arc::new(AtomicBool::new(false));
        let mut active = lock_unpoisoned(&self.active);
        *active = Some(Arc::clone(&token));

        // Close stores `closing` before taking this mutex. Publishing under
        // the same mutex and then rechecking closes the opposite race: either
        // close observes this token or the run observes closing.
        if closing.load(Ordering::Acquire) {
            token.store(true, Ordering::Release);
        }
        drop(active);

        ActiveRunGuard {
            control: self,
            token,
        }
    }

    fn cancel_active(&self) {
        if let Some(token) = lock_unpoisoned(&self.active).as_ref() {
            token.store(true, Ordering::Release);
        }
    }
}

struct ActiveRunGuard<'a> {
    control: &'a ActiveRunControl,
    token: Arc<AtomicBool>,
}

impl ActiveRunGuard<'_> {
    fn token(&self) -> Arc<AtomicBool> {
        Arc::clone(&self.token)
    }
}

impl Drop for ActiveRunGuard<'_> {
    fn drop(&mut self) {
        let mut active = lock_unpoisoned(&self.control.active);
        if active
            .as_ref()
            .is_some_and(|token| Arc::ptr_eq(token, &self.token))
        {
            active.take();
        }
    }
}

#[cfg(feature = "serde")]
enum SnapshotFileError {
    Io(std::io::Error),
    Serialization(ferric_rules_runtime::SerializationError),
}

fn lock_unpoisoned<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
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

fn run_with_external_cancel(
    engine: &mut Engine,
    limit: RunLimit,
    cancel: &AtomicBool,
    closing: &AtomicBool,
) -> Result<NativeRunResult, EngineError> {
    // Every accepted logical run enters the native fresh-run path exactly
    // once, even when its limit is zero or external control is already set.
    // Count(0) fires nothing while clearing prior halt and diagnostic state.
    let entry_result = engine.run(RunLimit::Count(0))?;
    let mut total = 0usize;
    let mut ran_positive_chunk = false;
    let mut remaining = match limit {
        RunLimit::Unlimited => usize::MAX,
        RunLimit::Count(count) => count,
    };

    loop {
        // Cancellation precedes every positive firing chunk and a finite-limit
        // result. A terminal result from an already-started positive chunk is
        // handled below before control reaches this check again.
        if cancel.load(Ordering::Acquire) || closing.load(Ordering::Acquire) {
            return Ok(NativeRunResult {
                rules_fired: total,
                halt_reason: NativeHaltReason::HaltRequested,
            });
        }
        if remaining == 0 {
            return if ran_positive_chunk {
                Ok(NativeRunResult {
                    rules_fired: total,
                    halt_reason: NativeHaltReason::LimitReached,
                })
            } else {
                Ok(entry_result)
            };
        }

        let chunk = remaining.min(RUN_CANCEL_CHUNK_SIZE);
        ran_positive_chunk = true;
        let result = engine.continue_run(RunLimit::Count(chunk))?;
        total = total.saturating_add(result.rules_fired);
        remaining = remaining.saturating_sub(result.rules_fired);

        // Preserve every native terminal from the chunk which already
        // started. Only an inner LimitReached can need repair: when its
        // limit-th firing requested a rule-side halt, the engine's flag is the
        // stronger terminal before another chunk can clear it on entry.
        if matches!(
            result.halt_reason,
            NativeHaltReason::AgendaEmpty
                | NativeHaltReason::HaltRequested
                | NativeHaltReason::ActionError
        ) {
            return Ok(NativeRunResult {
                rules_fired: total,
                halt_reason: result.halt_reason,
            });
        }
        if result.halt_reason == NativeHaltReason::LimitReached && engine.is_halted() {
            return Ok(NativeRunResult {
                rules_fired: total,
                halt_reason: NativeHaltReason::HaltRequested,
            });
        }
    }
}

#[cfg(feature = "serde")]
fn snapshot_file_error_to_pyerr(error: SnapshotFileError) -> PyErr {
    match error {
        SnapshotFileError::Io(error) => pyo3::exceptions::PyIOError::new_err(error.to_string()),
        SnapshotFileError::Serialization(error) => {
            crate::error::FerricError::new_err(error.to_string())
        }
    }
}

/// The Ferric rules engine.
///
/// Thread-affine: must be used only from the thread that created it.
/// Cross-thread access raises `FerricRuntimeError` (not a panic).
///
/// The actual engine data remains creator-thread-only for runtime access, but
/// the handle owns it directly so the final Python reference can destroy it on
/// any supported Python thread.
#[pyclass(name = "Engine", module = "ferric")]
pub struct PyEngine {
    engine_id: u64,
    creator_thread: ThreadId,
    closing: AtomicBool,
    active_run: ActiveRunControl,
    engine: Mutex<Option<OwnedThreadAffineEngine>>,
}

impl PyEngine {
    fn from_engine(engine: Engine) -> Self {
        Self::from_owned_engine(OwnedThreadAffineEngine::new(engine))
    }

    fn from_owned_engine(engine: OwnedThreadAffineEngine) -> Self {
        Self {
            engine_id: NEXT_ENGINE_ID.fetch_add(1, Ordering::Relaxed),
            creator_thread: thread::current().id(),
            closing: AtomicBool::new(false),
            active_run: ActiveRunControl::default(),
            engine: Mutex::new(Some(engine)),
        }
    }

    fn ensure_creator_thread(&self) -> PyResult<()> {
        let current = thread::current().id();
        if current != self.creator_thread {
            return Err(FerricRuntimeError::new_err(format!(
                "engine called from wrong thread (created on {:?}, called from {:?})",
                self.creator_thread, current,
            )));
        }
        Ok(())
    }

    fn closed_error() -> PyErr {
        FerricRuntimeError::new_err("engine has been closed")
    }

    /// Check thread, lock the live engine, and run a closure with mutable access.
    fn with_engine<F, R>(&self, f: F) -> PyResult<R>
    where
        F: FnOnce(&mut Engine) -> PyResult<R>,
    {
        self.ensure_creator_thread()?;
        let mut state = lock_unpoisoned(&self.engine);
        if self.closing.load(Ordering::Acquire) {
            return Err(Self::closed_error());
        }
        let engine = state.as_mut().ok_or_else(Self::closed_error)?;
        f(engine.get_mut())
    }

    /// Reserve the live engine, release the GIL for one native phase, and
    /// return its owned result after reacquiring the GIL and releasing the
    /// engine reservation.
    fn with_engine_allow_threads<F, R>(&self, py: Python<'_>, operation: F) -> PyResult<R>
    where
        F: FnOnce(&mut Engine) -> R + Send,
        R: Send,
    {
        self.ensure_creator_thread()?;
        let mut state = lock_unpoisoned(&self.engine);
        if self.closing.load(Ordering::Acquire) {
            return Err(Self::closed_error());
        }
        let engine = state.as_mut().ok_or_else(Self::closed_error)?.get_mut();
        let lease = GilReleasedEngineOperation::new(engine);
        let result = py.allow_threads(move || lease.run(operation));
        drop(state);
        Ok(result)
    }

    fn run_allow_threads(
        &self,
        py: Python<'_>,
        limit: RunLimit,
    ) -> PyResult<Result<NativeRunResult, EngineError>> {
        self.ensure_creator_thread()?;
        let mut state = lock_unpoisoned(&self.engine);
        if self.closing.load(Ordering::Acquire) {
            return Err(Self::closed_error());
        }
        let engine = state.as_mut().ok_or_else(Self::closed_error)?.get_mut();

        // Publication occurs after admission and before releasing the GIL.
        let active_run = self.active_run.publish(&self.closing);
        let cancel = active_run.token();
        let closing = &self.closing;
        let lease = GilReleasedEngineOperation::new(engine);
        let result = py.allow_threads(move || {
            lease.run(|engine| run_with_external_cancel(engine, limit, &cancel, closing))
        });

        // The run is no longer signalable before its exclusive engine lease is
        // released, so a later halt cannot latch onto a future run.
        drop(active_run);
        drop(state);
        Ok(result)
    }

    fn begin_close(&self) {
        self.closing.store(true, Ordering::Release);
        self.active_run.cancel_active();
    }

    /// Destroy the engine exactly once while holding the state mutex so every
    /// concurrent close observes completion only after native destruction.
    fn destroy_engine(&self) {
        let mut state = lock_unpoisoned(&self.engine);
        let engine = state.take();
        drop(engine);
        drop(state);
    }
}

impl Drop for PyEngine {
    fn drop(&mut self) {
        self.begin_close();
        self.destroy_engine();
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
        Self::from_engine(Engine::new(config))
    }

    /// Create an engine from CLIPS source, loading and resetting in one step.
    #[staticmethod]
    #[pyo3(signature = (source, *, strategy=None, encoding=None))]
    fn from_source(
        py: Python<'_>,
        source: &str,
        strategy: Option<Strategy>,
        encoding: Option<Encoding>,
    ) -> PyResult<Self> {
        let config = make_config(strategy, encoding);
        let source = source.to_owned();
        let engine = py.allow_threads(move || {
            Engine::with_rules_config(&source, config).map(OwnedThreadAffineEngine::new)
        });
        engine
            .map(Self::from_owned_engine)
            .map_err(init_error_to_pyerr)
    }

    // -- Context manager --

    fn __enter__(slf: Py<Self>, py: Python<'_>) -> PyResult<Py<Self>> {
        slf.bind(py).borrow().with_engine(|_| Ok(()))?;
        Ok(slf)
    }

    #[pyo3(signature = (_exc_type=None, _exc_val=None, _exc_tb=None))]
    fn __exit__(
        &self,
        py: Python<'_>,
        _exc_type: Option<&Bound<'_, PyAny>>,
        _exc_val: Option<&Bound<'_, PyAny>>,
        _exc_tb: Option<&Bound<'_, PyAny>>,
    ) -> bool {
        self.close(py);
        false // don't suppress exceptions
    }

    /// Explicitly close and destroy this engine from any supported Python thread.
    ///
    /// Closing requests cancellation of an active run, releases the GIL while
    /// waiting for admitted native work, and returns after native destruction.
    /// After calling `close()`, later ordinary engine operations raise
    /// `FerricRuntimeError`; close and context-manager exit remain idempotent.
    fn close(&self, py: Python<'_>) {
        self.begin_close();
        py.allow_threads(|| self.destroy_engine());
    }

    // -- Loading --

    /// Load CLIPS source into the engine.
    fn load(&self, py: Python<'_>, source: &str) -> PyResult<()> {
        let source = source.to_owned();
        let result =
            self.with_engine_allow_threads(py, move |engine| engine.load_str(&source).map(|_| ()))?;
        result.map_err(load_errors_to_pyerr)
    }

    /// Load CLIPS source from a file path (str or os.PathLike).
    fn load_file(&self, py: Python<'_>, path: PathBuf) -> PyResult<()> {
        let result =
            self.with_engine_allow_threads(py, move |engine| engine.load_file(&path).map(|_| ()))?;
        result.map_err(load_errors_to_pyerr)
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
    fn run(&self, py: Python<'_>, limit: Option<usize>) -> PyResult<RunResult> {
        let run_limit = match limit {
            Some(count) => RunLimit::Count(count),
            None => RunLimit::Unlimited,
        };
        let result = self.run_allow_threads(py, run_limit)?;
        result.map(Into::into).map_err(engine_error_to_pyerr)
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

    /// Promptly request cancellation of the currently active run from any
    /// supported Python thread.
    ///
    /// This operation is idempotent and does not latch onto a future run when
    /// the engine is idle or already closed.
    fn halt(&self) {
        self.active_run.cancel_active();
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

    /// Action evaluation diagnostics from the most recent run/step.
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
        let format = format.unwrap_or(crate::config::Format::Bincode).into();
        let bytes = self.with_engine_allow_threads(py, move |engine| engine.serialize(format))?;
        let bytes = bytes.map_err(|error| crate::error::FerricError::new_err(error.to_string()))?;
        Ok(pyo3::types::PyBytes::new(py, &bytes))
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
    fn from_snapshot(
        py: Python<'_>,
        data: &[u8],
        format: Option<crate::config::Format>,
    ) -> PyResult<Self> {
        let data = data.to_vec();
        let format = format.unwrap_or(crate::config::Format::Bincode).into();
        let engine = py.allow_threads(move || {
            Engine::deserialize(&data, format).map(OwnedThreadAffineEngine::new)
        });
        engine
            .map(Self::from_owned_engine)
            .map_err(|error| crate::error::FerricError::new_err(error.to_string()))
    }

    /// Save a serialized engine snapshot to a file.
    ///
    /// # Arguments
    ///
    /// * `path` -- File path (str or os.PathLike).
    /// * `format` -- Serialization format (default: `Format.BINCODE`).
    #[cfg(feature = "serde")]
    #[pyo3(signature = (path, *, format=None))]
    fn save_snapshot(
        &self,
        py: Python<'_>,
        path: PathBuf,
        format: Option<crate::config::Format>,
    ) -> PyResult<()> {
        let format = format.unwrap_or(crate::config::Format::Bincode).into();
        let result = self.with_engine_allow_threads(py, move |engine| {
            let bytes = engine
                .serialize(format)
                .map_err(SnapshotFileError::Serialization)?;
            std::fs::write(path, bytes).map_err(SnapshotFileError::Io)
        })?;
        result.map_err(snapshot_file_error_to_pyerr)
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
    fn from_snapshot_file(
        py: Python<'_>,
        path: PathBuf,
        format: Option<crate::config::Format>,
    ) -> PyResult<Self> {
        let format = format.unwrap_or(crate::config::Format::Bincode).into();
        let engine = py.allow_threads(move || {
            let data = std::fs::read(path).map_err(SnapshotFileError::Io)?;
            let engine =
                Engine::deserialize(&data, format).map_err(SnapshotFileError::Serialization)?;
            Ok(OwnedThreadAffineEngine::new(engine))
        });
        engine
            .map(Self::from_owned_engine)
            .map_err(snapshot_file_error_to_pyerr)
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
