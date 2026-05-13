//! The public [`PinnedEngine`] handle.

use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
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
    /// Cancellation state shared with worker-side `run_with_cancel` calls.
    cancel: Arc<CancelState>,
    /// Fast-path "is closed" without touching the sender mutex.
    closed: AtomicBool,
}

/// Coordinates `halt()` with the worker's currently active or next-dispatched
/// run request.
struct CancelState {
    active_run: Mutex<Option<Arc<AtomicBool>>>,
    pending_halt: AtomicBool,
}

struct ActiveRunGuard<'a> {
    state: &'a CancelState,
    token: Arc<AtomicBool>,
}

const REQUEST_PENDING: u8 = 0;
const REQUEST_STARTED: u8 = 1;
const REQUEST_CANCELED: u8 = 2;

/// Token used to cancel an accepted async request before the worker starts it.
#[derive(Debug)]
pub struct PreDispatchCancelToken {
    state: AtomicU8,
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
        let cancel = Arc::new(CancelState::new());

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

    /// Request that the in-flight, or next-dispatched, `run` exit at the next
    /// cancel-chunk boundary.
    pub fn halt(&self) {
        self.inner.cancel.halt();
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
        let cancel_state = self.inner.cancel.clone();
        let cancel_token = Arc::new(AtomicBool::new(false));
        self.with_engine(move |engine| {
            let _guard = cancel_state.activate(cancel_token.clone());
            worker::run_with_cancel(engine, limit, &cancel_token).map_err(PinnedError::from)
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
        self.run_async_cancelable(limit, Arc::new(PreDispatchCancelToken::new()), completion)
    }

    /// Async variant of [`Self::run`] with pre-dispatch cancellation support.
    pub fn run_async_cancelable<F>(
        &self,
        limit: RunLimit,
        pre_dispatch: Arc<PreDispatchCancelToken>,
        completion: F,
    ) -> Result<(), PinnedError>
    where
        F: FnOnce(Result<RunResult, PinnedError>) + Send + 'static,
    {
        let cancel_state = self.inner.cancel.clone();
        let cancel_token = Arc::new(AtomicBool::new(false));
        self.submit(move |engine| {
            if let Err(err) = pre_dispatch.begin() {
                completion(Err(err));
                return;
            }
            let _guard = cancel_state.activate(cancel_token.clone());
            let result = crate::worker::run_with_cancel(engine, limit, &cancel_token)
                .map_err(PinnedError::from);
            completion(result);
        })
    }

    /// Async variant of [`Self::load_str`].
    pub fn load_str_async<F>(&self, source: String, completion: F) -> Result<(), PinnedError>
    where
        F: FnOnce(Result<ferric_runtime::LoadResult, PinnedError>) + Send + 'static,
    {
        self.load_str_async_cancelable(source, Arc::new(PreDispatchCancelToken::new()), completion)
    }

    /// Async variant of [`Self::load_str`] with pre-dispatch cancellation support.
    pub fn load_str_async_cancelable<F>(
        &self,
        source: String,
        pre_dispatch: Arc<PreDispatchCancelToken>,
        completion: F,
    ) -> Result<(), PinnedError>
    where
        F: FnOnce(Result<ferric_runtime::LoadResult, PinnedError>) + Send + 'static,
    {
        self.submit(move |engine| {
            if let Err(err) = pre_dispatch.begin() {
                completion(Err(err));
                return;
            }
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

impl Default for PreDispatchCancelToken {
    fn default() -> Self {
        Self::new()
    }
}

impl PreDispatchCancelToken {
    #[must_use]
    pub const fn new() -> Self {
        Self {
            state: AtomicU8::new(REQUEST_PENDING),
        }
    }

    /// Attempt to cancel before the worker starts this request.
    ///
    /// Returns `true` if this call either performed the cancellation or the
    /// request had already been canceled while still pending. Returns `false`
    /// once the worker has started the request.
    pub fn cancel_before_start(&self) -> bool {
        loop {
            match self.state.load(Ordering::Acquire) {
                REQUEST_PENDING => {
                    if self
                        .state
                        .compare_exchange(
                            REQUEST_PENDING,
                            REQUEST_CANCELED,
                            Ordering::AcqRel,
                            Ordering::Acquire,
                        )
                        .is_ok()
                    {
                        return true;
                    }
                }
                REQUEST_CANCELED => return true,
                _ => return false,
            }
        }
    }

    fn begin(&self) -> Result<(), PinnedError> {
        match self.state.compare_exchange(
            REQUEST_PENDING,
            REQUEST_STARTED,
            Ordering::AcqRel,
            Ordering::Acquire,
        ) {
            Ok(_) => Ok(()),
            Err(REQUEST_CANCELED) => Err(PinnedError::Canceled),
            Err(_) => Err(PinnedError::DispatchFailed),
        }
    }
}

impl CancelState {
    fn new() -> Self {
        Self {
            active_run: Mutex::new(None),
            pending_halt: AtomicBool::new(false),
        }
    }

    fn halt(&self) {
        let active = self
            .active_run
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(token) = active.as_ref() {
            token.store(true, Ordering::Release);
        } else {
            self.pending_halt.store(true, Ordering::Release);
        }
    }

    fn activate(&self, token: Arc<AtomicBool>) -> ActiveRunGuard<'_> {
        let mut active = self
            .active_run
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if self.pending_halt.swap(false, Ordering::AcqRel) {
            token.store(true, Ordering::Release);
        }
        *active = Some(token.clone());
        ActiveRunGuard { state: self, token }
    }
}

impl Drop for ActiveRunGuard<'_> {
    fn drop(&mut self) {
        let mut active = self
            .state
            .active_run
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if active
            .as_ref()
            .is_some_and(|token| Arc::ptr_eq(token, &self.token))
        {
            *active = None;
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
