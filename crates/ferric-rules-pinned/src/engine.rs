//! The public [`PinnedEngine`] handle.

use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use std::sync::mpsc::{self, sync_channel};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle, ThreadId};
use std::time::Duration;

use crossbeam_channel::{after, bounded, never, Receiver, Sender};
use ferric_rules_runtime::{Engine, LoadResult, RunLimit, RunResult};

#[cfg(feature = "serde")]
use ferric_rules_runtime::SerializationFormat;

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

/// How long an asynchronous submission may wait for bounded-queue capacity.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum QueueWait {
    /// Return [`PinnedError::QueueFull`] immediately when the queue is full.
    NoWait,
    /// Wait up to this duration for queue capacity. A zero duration is
    /// equivalent to [`Self::NoWait`].
    Timeout(Duration),
    /// Wait until capacity becomes available, the request is canceled, or the
    /// engine closes.
    Indefinite,
}

/// Shared internal state of a [`PinnedEngine`].
struct PinnedInner {
    /// Wrapped sender. `None` means subsequent submissions return `Closed`.
    tx: Mutex<Option<Sender<Request>>>,
    /// Dropping the sole sender wakes every queue-admission waiter on close.
    close_tx: Mutex<Option<Sender<()>>>,
    close_rx: Receiver<()>,
    /// Worker join handle. `None` after `close()` joins (or detaches) it.
    worker: Mutex<Option<JoinHandle<()>>>,
    /// Identity of the worker thread. Used by `do_close` to detect a
    /// re-entrant close from inside a worker-side callback and skip the
    /// self-join (which would deadlock).
    worker_thread_id: ThreadId,
    /// Cancellation state shared with worker-side `run_with_cancel` calls.
    cancel: Arc<CancelState>,
    /// Fast-path "is closed" without touching the sender mutex.
    closed: Arc<AtomicBool>,
}

/// Coordinates `halt()` with the worker's currently active run request.
struct CancelState {
    active_run: Mutex<Option<Arc<AtomicBool>>>,
}

struct ActiveRunGuard<'a> {
    state: &'a CancelState,
    token: Arc<AtomicBool>,
}

const REQUEST_PENDING: u8 = 0;
const REQUEST_STARTED: u8 = 1;
const REQUEST_CANCELED: u8 = 2;
const REQUEST_FINISHED: u8 = 3;
const REQUEST_CANCEL_REQUESTED: u8 = 4;

/// Token used to cancel a registered async request.
///
/// Every request can be canceled while waiting for capacity or before
/// dispatch. Run requests also use the token's per-run flag for cooperative
/// cancellation after they have started.
#[derive(Debug)]
pub struct PreDispatchCancelToken {
    state: AtomicU8,
    run_cancel: Arc<AtomicBool>,
    admission_cancel_tx: Sender<()>,
    admission_cancel_rx: Receiver<()>,
}

impl PinnedEngine {
    /// Spawn a worker thread and construct a new pinned engine.
    ///
    /// Returns once the worker has finished engine construction. Errors during
    /// construction propagate as [`PinnedError::Init`].
    pub fn new(options: PinnedEngineOptions) -> Result<Self, PinnedError> {
        let resolved = ResolvedOptions::from_user(options);
        let (tx, rx) = bounded::<Request>(resolved.queue_capacity);
        let (close_tx, close_rx) = bounded::<()>(0);
        let (init_tx, init_rx) = sync_channel::<Result<(), PinnedError>>(1);
        let cancel = Arc::new(CancelState::new());
        let closed = Arc::new(AtomicBool::new(false));

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

        let worker_thread_id = worker.thread().id();
        let inner = PinnedInner {
            tx: Mutex::new(Some(tx)),
            close_tx: Mutex::new(Some(close_tx)),
            close_rx,
            worker: Mutex::new(Some(worker)),
            worker_thread_id,
            cancel,
            closed,
        };
        Ok(Self {
            inner: Arc::new(inner),
        })
    }

    /// Stop accepting new requests, drain already-queued requests, and join
    /// the worker. Active and queued runs exit with
    /// [`ferric_rules_runtime::HaltReason::HaltRequested`]; other accepted work runs
    /// to completion. Idempotent.
    pub fn close(&self) -> Result<(), PinnedError> {
        self.inner.do_close()
    }

    /// `true` once [`Self::close`] (or `Drop` of the last handle) has begun.
    pub fn is_closed(&self) -> bool {
        self.inner.closed.load(Ordering::Acquire)
    }

    /// Request that the active `run` exit at the next cancel-chunk boundary.
    ///
    /// Has no effect while the worker is idle or handling another operation,
    /// and does not latch onto queued or future runs.
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
        if thread::current().id() == self.inner.worker_thread_id {
            return Err(PinnedError::ReentrantCall);
        }
        let (tx, rx) = mpsc::channel::<Result<R, PinnedError>>();
        let req = Request::new(move |engine: &mut Engine| {
            let result = f(engine);
            // Caller may have abandoned; send failure is fine.
            let _ = tx.send(result);
        });
        self.send_request(req, QueueWait::NoWait, None)?;
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
        let closed = self.inner.closed.clone();
        self.with_engine(move |engine| {
            let guard = cancel_state.activate(cancel_token.clone());
            let result = worker::run_with_cancel(engine, limit, &cancel_token, &closed)
                .map_err(PinnedError::from);
            drop(guard);
            result
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
        let req = Request::new(f);
        self.send_request(req, QueueWait::NoWait, None)
    }

    /// Submit a closure, optionally waiting for bounded-queue capacity.
    pub fn submit_with_queue_wait<F>(&self, wait: QueueWait, f: F) -> Result<(), PinnedError>
    where
        F: FnOnce(&mut Engine) + Send + 'static,
    {
        let req = Request::new(f);
        self.send_request(req, wait, None)
    }

    /// Submit a typed asynchronous operation with an exactly-once completion.
    ///
    /// Once submission succeeds, `completion` is invoked once on the worker
    /// thread with the operation result. A panic in `operation` is contained
    /// and reported as [`PinnedError::Internal`]. A synchronous submission
    /// error means the request was not accepted and `completion` will not run.
    /// The completion is consumed before invocation; if it panics, the worker
    /// contains that panic without retrying it. As with the typed async
    /// helpers, completion work must be transport-only and must not block or
    /// synchronously re-enter this engine.
    pub fn submit_with_completion<T, F, C>(
        &self,
        operation: F,
        completion: C,
    ) -> Result<(), PinnedError>
    where
        T: Send + 'static,
        F: FnOnce(&mut Engine) -> Result<T, PinnedError> + Send + 'static,
        C: FnOnce(Result<T, PinnedError>) + Send + 'static,
    {
        self.send_request(
            Request::with_completion(operation, completion),
            QueueWait::NoWait,
            None,
        )
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

    /// Async variant of [`Self::run`] with configurable queue admission.
    pub fn run_async_with_queue_wait<F>(
        &self,
        limit: RunLimit,
        queue_wait: QueueWait,
        completion: F,
    ) -> Result<(), PinnedError>
    where
        F: FnOnce(Result<RunResult, PinnedError>) + Send + 'static,
    {
        self.run_async_cancelable_with_queue_wait(
            limit,
            Arc::new(PreDispatchCancelToken::new()),
            queue_wait,
            completion,
        )
    }

    /// Async variant of [`Self::run`] with pre-dispatch and in-flight
    /// cooperative cancellation support.
    pub fn run_async_cancelable<F>(
        &self,
        limit: RunLimit,
        request_cancel: Arc<PreDispatchCancelToken>,
        completion: F,
    ) -> Result<(), PinnedError>
    where
        F: FnOnce(Result<RunResult, PinnedError>) + Send + 'static,
    {
        self.run_async_cancelable_with_queue_wait(
            limit,
            request_cancel,
            QueueWait::NoWait,
            completion,
        )
    }

    /// Async variant of [`Self::run`] with cancel-aware queue admission.
    pub fn run_async_cancelable_with_queue_wait<F>(
        &self,
        limit: RunLimit,
        request_cancel: Arc<PreDispatchCancelToken>,
        queue_wait: QueueWait,
        completion: F,
    ) -> Result<(), PinnedError>
    where
        F: FnOnce(Result<RunResult, PinnedError>) + Send + 'static,
    {
        let cancel_state = self.inner.cancel.clone();
        let cancel_token = request_cancel.run_cancel_flag();
        let closed = self.inner.closed.clone();
        let admission_cancel = request_cancel.clone();
        let completion_cancel = request_cancel.clone();
        let req = Request::with_completion(
            move |engine| {
                request_cancel.begin()?;
                let guard = cancel_state.activate(cancel_token.clone());
                let result = crate::worker::run_with_cancel(engine, limit, &cancel_token, &closed)
                    .map_err(PinnedError::from);
                drop(guard);
                result
            },
            move |mut result| {
                if completion_cancel.finish() {
                    if let Ok(run_result) = &mut result {
                        run_result.halt_reason = ferric_rules_runtime::HaltReason::HaltRequested;
                    }
                }
                completion(result);
            },
        );
        self.send_request(req, queue_wait, Some(&admission_cancel))
    }

    /// Async variant of [`Self::load_str`].
    pub fn load_str_async<F>(&self, source: String, completion: F) -> Result<(), PinnedError>
    where
        F: FnOnce(Result<ferric_rules_runtime::LoadResult, PinnedError>) + Send + 'static,
    {
        self.load_str_async_cancelable(source, Arc::new(PreDispatchCancelToken::new()), completion)
    }

    /// Async variant of [`Self::load_str`] with configurable queue admission.
    pub fn load_str_async_with_queue_wait<F>(
        &self,
        source: String,
        queue_wait: QueueWait,
        completion: F,
    ) -> Result<(), PinnedError>
    where
        F: FnOnce(Result<ferric_rules_runtime::LoadResult, PinnedError>) + Send + 'static,
    {
        self.load_str_async_cancelable_with_queue_wait(
            source,
            Arc::new(PreDispatchCancelToken::new()),
            queue_wait,
            completion,
        )
    }

    /// Async variant of [`Self::load_str`] with pre-dispatch cancellation support.
    pub fn load_str_async_cancelable<F>(
        &self,
        source: String,
        request_cancel: Arc<PreDispatchCancelToken>,
        completion: F,
    ) -> Result<(), PinnedError>
    where
        F: FnOnce(Result<ferric_rules_runtime::LoadResult, PinnedError>) + Send + 'static,
    {
        self.load_str_async_cancelable_with_queue_wait(
            source,
            request_cancel,
            QueueWait::NoWait,
            completion,
        )
    }

    /// Async variant of [`Self::load_str`] with cancel-aware queue admission.
    pub fn load_str_async_cancelable_with_queue_wait<F>(
        &self,
        source: String,
        request_cancel: Arc<PreDispatchCancelToken>,
        queue_wait: QueueWait,
        completion: F,
    ) -> Result<(), PinnedError>
    where
        F: FnOnce(Result<ferric_rules_runtime::LoadResult, PinnedError>) + Send + 'static,
    {
        let admission_cancel = request_cancel.clone();
        let completion_cancel = request_cancel.clone();
        let req = Request::with_completion(
            move |engine| {
                request_cancel.begin()?;
                engine.load_str(&source).map_err(PinnedError::from)
            },
            move |result| {
                let _ = completion_cancel.finish();
                completion(result);
            },
        );
        self.send_request(req, queue_wait, Some(&admission_cancel))
    }

    fn send_request(
        &self,
        req: Request,
        wait: QueueWait,
        cancel: Option<&PreDispatchCancelToken>,
    ) -> Result<(), PinnedError> {
        let may_block = !matches!(wait, QueueWait::NoWait | QueueWait::Timeout(Duration::ZERO));
        if may_block && thread::current().id() == self.inner.worker_thread_id {
            return Err(PinnedError::ReentrantCall);
        }
        if self.inner.closed.load(Ordering::Acquire) {
            return Err(PinnedError::Closed);
        }
        let sender = {
            let guard = self
                .inner
                .tx
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            guard.as_ref().cloned().ok_or(PinnedError::Closed)?
        };
        let close_rx = self.inner.close_rx.clone();
        let cancel_rx = cancel.map_or_else(never, PreDispatchCancelToken::cancel_receiver);

        match wait {
            QueueWait::NoWait | QueueWait::Timeout(Duration::ZERO) => {
                crossbeam_channel::select_biased! {
                    recv(close_rx) -> _ => Err(PinnedError::Closed),
                    recv(cancel_rx) -> _ => Err(PinnedError::Canceled),
                    send(sender, req) -> result => {
                        result.map_err(|_| self.disconnected_error())
                    }
                    default => Err(PinnedError::QueueFull),
                }
            }
            QueueWait::Timeout(duration) => {
                let timeout_rx = after(duration);
                crossbeam_channel::select_biased! {
                    recv(close_rx) -> _ => Err(PinnedError::Closed),
                    recv(cancel_rx) -> _ => Err(PinnedError::Canceled),
                    send(sender, req) -> result => {
                        result.map_err(|_| self.disconnected_error())
                    }
                    recv(timeout_rx) -> _ => Err(PinnedError::QueueFull),
                }
            }
            QueueWait::Indefinite => {
                crossbeam_channel::select_biased! {
                    recv(close_rx) -> _ => Err(PinnedError::Closed),
                    recv(cancel_rx) -> _ => Err(PinnedError::Canceled),
                    send(sender, req) -> result => {
                        result.map_err(|_| self.disconnected_error())
                    }
                }
            }
        }
    }

    fn disconnected_error(&self) -> PinnedError {
        if self.inner.closed.load(Ordering::Acquire) {
            PinnedError::Closed
        } else {
            PinnedError::DispatchFailed
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
    pub fn new() -> Self {
        let (admission_cancel_tx, admission_cancel_rx) = bounded(1);
        Self {
            state: AtomicU8::new(REQUEST_PENDING),
            run_cancel: Arc::new(AtomicBool::new(false)),
            admission_cancel_tx,
            admission_cancel_rx,
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
                        self.signal_admission_cancel();
                        return true;
                    }
                }
                REQUEST_CANCELED => return true,
                _ => return false,
            }
        }
    }

    /// Cancel a run before dispatch or request cooperative cancellation after
    /// it has started.
    ///
    /// Returns `false` once the run has finished.
    pub fn cancel_run(&self) -> bool {
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
                        self.signal_admission_cancel();
                        return true;
                    }
                }
                REQUEST_STARTED => {
                    if self
                        .state
                        .compare_exchange(
                            REQUEST_STARTED,
                            REQUEST_CANCEL_REQUESTED,
                            Ordering::AcqRel,
                            Ordering::Acquire,
                        )
                        .is_ok()
                    {
                        self.run_cancel.store(true, Ordering::Release);
                        self.signal_admission_cancel();
                        return true;
                    }
                }
                REQUEST_CANCELED | REQUEST_CANCEL_REQUESTED => return true,
                _ => return false,
            }
        }
    }

    /// Returns whether the worker has started this request.
    #[doc(hidden)]
    pub fn is_started(&self) -> bool {
        matches!(
            self.state.load(Ordering::Acquire),
            REQUEST_STARTED | REQUEST_CANCEL_REQUESTED
        )
    }

    fn run_cancel_flag(&self) -> Arc<AtomicBool> {
        self.run_cancel.clone()
    }

    fn cancel_receiver(&self) -> Receiver<()> {
        self.admission_cancel_rx.clone()
    }

    fn signal_admission_cancel(&self) {
        let _ = self.admission_cancel_tx.try_send(());
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

    /// Mark the request finished and report whether in-flight cancellation won
    /// the race with completion.
    fn finish(&self) -> bool {
        self.state.swap(REQUEST_FINISHED, Ordering::AcqRel) == REQUEST_CANCEL_REQUESTED
    }
}

impl CancelState {
    fn new() -> Self {
        Self {
            active_run: Mutex::new(None),
        }
    }

    fn halt(&self) {
        let active = self
            .active_run
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(token) = active.as_ref() {
            token.store(true, Ordering::Release);
        }
    }

    fn activate(&self, token: Arc<AtomicBool>) -> ActiveRunGuard<'_> {
        let mut active = self
            .active_run
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
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
        // Disconnect the close channel to wake every blocked admission call.
        let close_sender = {
            let mut guard = self
                .close_tx
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            guard.take()
        };
        drop(close_sender);
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
            // Re-entrant close from a worker-owned callback: joining
            // ourselves would deadlock. Detach instead — the worker will
            // observe the dropped sender after the current callback
            // returns and exit on its own.
            if thread::current().id() == self.worker_thread_id {
                drop(h);
            } else {
                h.join().map_err(|_| PinnedError::DispatchFailed)?;
            }
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

#[cfg(test)]
mod completion_tests {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{mpsc, Arc, Barrier};
    use std::thread;
    use std::time::Duration;

    use super::*;

    #[test]
    fn cancellation_racing_operation_panic_finalizes_once() {
        let engine = PinnedEngine::new(PinnedEngineOptions::default()).unwrap();
        let completion_count = Arc::new(AtomicUsize::new(0));

        for expected_count in 1..=128 {
            let token = Arc::new(PreDispatchCancelToken::new());
            let operation_token = token.clone();
            let completion_token = token.clone();
            let admission_token = token.clone();
            let callback_count = completion_count.clone();
            let (entered_tx, entered_rx) = mpsc::channel();
            let (result_tx, result_rx) = mpsc::channel();
            let race_start = Arc::new(Barrier::new(2));
            let operation_start = race_start.clone();

            let request = Request::with_completion(
                move |_| -> Result<(), PinnedError> {
                    operation_token.begin()?;
                    entered_tx.send(()).unwrap();
                    operation_start.wait();
                    panic!("synthetic cancellation/panic race");
                },
                move |result| {
                    let cancellation_won = completion_token.finish();
                    callback_count.fetch_add(1, Ordering::SeqCst);
                    result_tx.send((result, cancellation_won)).unwrap();
                },
            );
            engine
                .send_request(request, QueueWait::NoWait, Some(&admission_token))
                .unwrap();

            entered_rx.recv_timeout(Duration::from_secs(2)).unwrap();
            let cancellation_token = token.clone();
            let cancellation = thread::spawn(move || {
                race_start.wait();
                cancellation_token.cancel_run()
            });

            let (result, cancellation_won) =
                result_rx.recv_timeout(Duration::from_secs(2)).unwrap();
            let cancellation_requested = cancellation.join().unwrap();
            assert!(matches!(result, Err(PinnedError::Internal)));
            assert_eq!(cancellation_won, cancellation_requested);
            assert!(!token.is_started());
            assert_eq!(completion_count.load(Ordering::SeqCst), expected_count);
        }

        engine
            .with_engine(|_| Ok(()))
            .expect("worker should remain usable after cancellation/panic race");
        assert_eq!(completion_count.load(Ordering::SeqCst), 128);
        engine.close().unwrap();
    }
}
