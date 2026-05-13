//! The public [`PinnedEngine`] handle.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, sync_channel, SyncSender, TrySendError};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};

use ferric_runtime::{Engine, LoadResult, RunLimit, RunResult};

#[cfg(feature = "serde")]
use ferric_runtime::SerializationFormat;

use crate::error::PinnedError;
use crate::options::{PinnedEngineOptions, ResolvedOptions};
use crate::request::Request;
use crate::worker;

/// Send + Sync handle to a pinned engine.
///
/// Internally a thin `Arc` wrapper over [`PinnedInner`]. Cloning produces a
/// second handle that shares the same worker thread and request queue.
#[derive(Clone)]
pub struct PinnedEngine {
    inner: Arc<PinnedInner>,
}

/// Shared internal state of a [`PinnedEngine`].
struct PinnedInner {
    /// Wrapped sender. `None` ⇒ closed; subsequent `try_send` returns `Closed`.
    tx: Mutex<Option<SyncSender<Request>>>,
    /// Worker join handle. `None` after `close()` joins it.
    worker: Mutex<Option<JoinHandle<()>>>,
    /// Cancellation flag shared with the worker's `run_with_cancel` calls.
    /// `PinnedEngine::halt` flips this from `false` to `true`.
    cancel: Arc<AtomicBool>,
    /// Fast-path "is closed" without touching the sender mutex.
    closed: AtomicBool,
}

impl PinnedEngine {
    /// Spawn a worker thread and construct a new pinned engine.
    ///
    /// Returns once the worker has finished engine construction. Errors during
    /// construction propagate as [`PinnedError::Init`].
    pub fn new(options: PinnedEngineOptions) -> Result<Self, PinnedError> {
        let resolved = ResolvedOptions::from_user(options);
        let (tx, rx) = sync_channel::<Request>(resolved.queue_capacity);
        let (init_tx, init_rx) = sync_channel::<Result<(), PinnedError>>(1);
        let cancel = Arc::new(AtomicBool::new(false));

        let thread_name = resolved.thread_name.clone();
        let worker_opts = resolved.clone();
        // OS-level spawn failure (resource exhaustion etc.) surfaces as
        // DispatchFailed — there is no worker to dispatch through.
        let worker = thread::Builder::new()
            .name(thread_name)
            .spawn(move || worker::worker_main(rx, worker_opts, init_tx))
            .map_err(|_| PinnedError::DispatchFailed)?;

        match init_rx.recv() {
            Ok(Ok(())) => {}
            Ok(Err(err)) => {
                // Worker reported a construction failure; join it before returning.
                let _ = worker.join();
                return Err(err);
            }
            Err(_) => {
                // Worker dropped init_tx without sending. Treat as DispatchFailed.
                let _ = worker.join();
                return Err(PinnedError::DispatchFailed);
            }
        }

        let inner = PinnedInner {
            tx: Mutex::new(Some(tx)),
            worker: Mutex::new(Some(worker)),
            cancel,
            closed: AtomicBool::new(false),
        };
        Ok(Self {
            inner: Arc::new(inner),
        })
    }

    /// Stop accepting new requests, drain already-queued requests, and join
    /// the worker. Idempotent.
    pub fn close(&self) -> Result<(), PinnedError> {
        self.inner.do_close()
    }

    /// `true` once [`Self::close`] (or `Drop` of the last handle) has begun.
    pub fn is_closed(&self) -> bool {
        self.inner.closed.load(Ordering::Acquire)
    }

    /// Request that the in-flight (or next-dispatched) `run` exit at the next
    /// cancel-chunk boundary.
    ///
    /// Sets the shared cancellation flag. The flag persists until cleared by
    /// the next [`Self::run`] invocation; calls between runs accumulate.
    pub fn halt(&self) {
        self.inner.cancel.store(true, Ordering::Release);
    }

    /// Submit a closure to run on the worker thread with mutable engine access.
    ///
    /// Blocks until the worker completes the closure (or the queue rejects the
    /// request).
    pub fn with_engine<F, R>(&self, f: F) -> Result<R, PinnedError>
    where
        F: FnOnce(&mut Engine) -> Result<R, PinnedError> + Send + 'static,
        R: Send + 'static,
    {
        let (tx, rx) = mpsc::channel::<Result<R, PinnedError>>();
        let req: Request = Box::new(move |engine: &mut Engine| {
            let result = f(engine);
            // Caller may have abandoned; send failure is fine.
            let _ = tx.send(result);
        });
        self.try_send(req)?;
        rx.recv().map_err(|_| PinnedError::DispatchFailed)?
    }

    /// Load a CLIPS source string into the engine.
    pub fn load_str(&self, source: &str) -> Result<LoadResult, PinnedError> {
        let source = source.to_string();
        self.with_engine(move |engine| engine.load_str(&source).map_err(PinnedError::from))
    }

    /// Reset the engine to its initial state.
    pub fn reset(&self) -> Result<(), PinnedError> {
        self.with_engine(|engine| engine.reset().map_err(PinnedError::from))
    }

    /// Clear all engine state.
    pub fn clear(&self) -> Result<(), PinnedError> {
        self.with_engine(|engine| {
            engine.clear();
            Ok(())
        })
    }

    /// Run the engine until the agenda is empty, the limit is reached, the
    /// rule-side `(halt)` is invoked, or [`Self::halt`] is called from another
    /// thread.
    pub fn run(&self, limit: RunLimit) -> Result<RunResult, PinnedError> {
        // Clear the cancel flag synchronously so a stale halt from a previous
        // run does not preempt this one. Halts requested AFTER this point still
        // take effect through `run_with_cancel`'s per-chunk check.
        let cancel = self.inner.cancel.clone();
        cancel.store(false, Ordering::Release);
        self.with_engine(move |engine| {
            worker::run_with_cancel(engine, limit, &cancel).map_err(PinnedError::from)
        })
    }

    /// Serialize the engine to bytes. Requires the `serde` feature.
    #[cfg(feature = "serde")]
    pub fn serialize(&self, format: SerializationFormat) -> Result<Vec<u8>, PinnedError> {
        self.with_engine(move |engine| engine.serialize(format).map_err(PinnedError::from))
    }

    // -----------------------------------------------------------------------
    // Asynchronous variants — accept a completion closure that the worker
    // invokes after running the operation.
    //
    // These are the building blocks the FFI uses to implement async C APIs
    // without spawning a thread per request.
    // -----------------------------------------------------------------------

    /// Submit a closure to run on the worker thread without waiting for its
    /// result. The closure is responsible for delivering any output (e.g., via
    /// a completion callback it captures).
    pub fn submit<F>(&self, f: F) -> Result<(), PinnedError>
    where
        F: FnOnce(&mut Engine) + Send + 'static,
    {
        let req: Request = Box::new(f);
        self.try_send(req)
    }

    /// Async variant of [`Self::run`].
    ///
    /// Returns immediately on successful submission. `completion` is invoked
    /// on the worker thread after `run` completes (or fails). The completion
    /// **must be transport-only** — it must not call back into this engine
    /// synchronously, perform long work, or block.
    pub fn run_async<F>(&self, limit: RunLimit, completion: F) -> Result<(), PinnedError>
    where
        F: FnOnce(Result<RunResult, PinnedError>) + Send + 'static,
    {
        let cancel = self.inner.cancel.clone();
        cancel.store(false, Ordering::Release);
        self.submit(move |engine| {
            let result =
                crate::worker::run_with_cancel(engine, limit, &cancel).map_err(PinnedError::from);
            completion(result);
        })
    }

    /// Async variant of [`Self::load_str`].
    pub fn load_str_async<F>(&self, source: String, completion: F) -> Result<(), PinnedError>
    where
        F: FnOnce(Result<ferric_runtime::LoadResult, PinnedError>) + Send + 'static,
    {
        self.submit(move |engine| {
            let result = engine.load_str(&source).map_err(PinnedError::from);
            completion(result);
        })
    }

    fn try_send(&self, req: Request) -> Result<(), PinnedError> {
        let guard = self
            .inner
            .tx
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let sender = guard.as_ref().ok_or(PinnedError::Closed)?;
        match sender.try_send(req) {
            Ok(()) => Ok(()),
            Err(TrySendError::Full(_)) => Err(PinnedError::QueueFull),
            Err(TrySendError::Disconnected(_)) => Err(PinnedError::DispatchFailed),
        }
    }
}

impl PinnedInner {
    fn do_close(&self) -> Result<(), PinnedError> {
        self.closed.store(true, Ordering::Release);
        // Drop the sender; worker will drain buffered requests then exit.
        let sender = {
            let mut guard = self
                .tx
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            guard.take()
        };
        drop(sender);
        let handle = {
            let mut guard = self
                .worker
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            guard.take()
        };
        if let Some(h) = handle {
            h.join().map_err(|_| PinnedError::DispatchFailed)?;
        }
        Ok(())
    }
}

impl Drop for PinnedInner {
    fn drop(&mut self) {
        // Best-effort shutdown. Swallow any join error to avoid double-panic.
        let _ = self.do_close();
    }
}

#[cfg(test)]
mod policy_tests {
    use super::*;
    use crate::autorelease::{self, TEST_MUTEX};
    use crate::AutoreleasePolicy;

    /// `PerItem`: every accepted request wraps exactly once.
    #[test]
    fn per_item_wraps_each_request() {
        let _guard = TEST_MUTEX.lock().unwrap();
        let engine = PinnedEngine::new(PinnedEngineOptions {
            autorelease_policy: AutoreleasePolicy::PerItem,
            ..Default::default()
        })
        .unwrap();
        let before = autorelease::wrap_count();
        for _ in 0..5 {
            engine.with_engine(|_| Ok(())).unwrap();
        }
        engine.close().unwrap();
        assert_eq!(autorelease::wrap_count() - before, 5);
    }

    /// None: no wraps occur for any number of requests.
    #[test]
    fn none_skips_wrap_entirely() {
        let _guard = TEST_MUTEX.lock().unwrap();
        let engine = PinnedEngine::new(PinnedEngineOptions {
            autorelease_policy: AutoreleasePolicy::None,
            ..Default::default()
        })
        .unwrap();
        let before = autorelease::wrap_count();
        for _ in 0..5 {
            engine.with_engine(|_| Ok(())).unwrap();
        }
        engine.close().unwrap();
        assert_eq!(autorelease::wrap_count() - before, 0);
    }

    /// `PerBatch`: at least one wrap per batch. With sync `with_engine` callers,
    /// each request lands in its own batch, so the count equals request count;
    /// this just verifies the wrap path is reached.
    #[test]
    fn per_batch_wraps_at_least_once_per_request_burst() {
        let _guard = TEST_MUTEX.lock().unwrap();
        let engine = PinnedEngine::new(PinnedEngineOptions {
            autorelease_policy: AutoreleasePolicy::PerBatch,
            ..Default::default()
        })
        .unwrap();
        let before = autorelease::wrap_count();
        for _ in 0..5 {
            engine.with_engine(|_| Ok(())).unwrap();
        }
        engine.close().unwrap();
        let delta = autorelease::wrap_count() - before;
        // Synchronous callers drain queue between each call, so 5 batches.
        // Concurrent callers would reduce this; we only assert the bounds.
        assert!(
            (1..=5).contains(&delta),
            "expected 1..=5 batches, got {delta}"
        );
    }
}
