//! Worker thread main loop and the cancel-aware run helper.
//!
//! The worker owns the [`Engine`] for its entire lifetime. It blocks on the
//! request channel for the first request of each batch, then drains
//! immediately-available requests (up to `max_batch_size`) and dispatches them
//! through the configured [`AutoreleasePolicy`].
//!
//! [`Engine`]: ferric_runtime::Engine

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, SyncSender};

use ferric_runtime::engine::EngineError;
use ferric_runtime::{Engine, HaltReason, RunLimit, RunResult};

use crate::autorelease;
use crate::error::PinnedError;
use crate::options::ResolvedOptions;
use crate::request::Request;

/// Maximum number of rule firings between cancellation checks inside
/// [`run_with_cancel`].
///
/// 64 matches the spirit of the TypeScript binding's `RUN_BATCH_SIZE = 200`
/// while keeping the upper bound on cancel latency in the microseconds range
/// for typical rule firings.
pub(crate) const CANCEL_CHUNK_SIZE: usize = 64;

/// Entry point for the worker thread.
///
/// `init_ack` is sent exactly one message describing whether engine
/// construction succeeded, then dropped.
#[allow(clippy::needless_pass_by_value)]
pub(crate) fn worker_main(
    rx: Receiver<Request>,
    opts: ResolvedOptions,
    init_ack: SyncSender<Result<(), PinnedError>>,
) {
    // Construct the engine on this thread so its creator_thread is the worker.
    let mut engine = Engine::new(opts.engine_config.clone());
    // Acknowledge readiness; ignore send failure (caller dropped the receiver).
    let _ = init_ack.send(Ok(()));
    drop(init_ack);

    let batch_limit = if opts.max_batch_size == 0 {
        usize::MAX
    } else {
        opts.max_batch_size
    };

    loop {
        // All senders dropped ⇒ recv returns Err. The standard library
        // guarantees `recv` returns buffered messages before signaling
        // disconnection, so close-drain falls out for free.
        let Ok(first) = rx.recv() else { return };
        run_batch(
            &rx,
            &mut engine,
            first,
            batch_limit,
            opts.autorelease_policy,
        );
    }
}

fn run_batch(
    rx: &Receiver<Request>,
    engine: &mut Engine,
    first: Request,
    batch_limit: usize,
    policy: crate::AutoreleasePolicy,
) {
    use crate::AutoreleasePolicy as P;
    let remaining_after_first = batch_limit.saturating_sub(1);
    match policy {
        P::None => {
            first(engine);
            drain_more(rx, engine, remaining_after_first);
        }
        P::PerItem => {
            autorelease::wrap(|| first(engine));
            drain_more_per_item(rx, engine, remaining_after_first);
        }
        P::PerBatch => autorelease::wrap(|| {
            first(engine);
            drain_more(rx, engine, remaining_after_first);
        }),
    }
}

fn drain_more(rx: &Receiver<Request>, engine: &mut Engine, max: usize) {
    for _ in 0..max {
        match rx.try_recv() {
            Ok(req) => req(engine),
            Err(_) => return,
        }
    }
}

fn drain_more_per_item(rx: &Receiver<Request>, engine: &mut Engine, max: usize) {
    for _ in 0..max {
        match rx.try_recv() {
            Ok(req) => autorelease::wrap(|| req(engine)),
            Err(_) => return,
        }
    }
}

/// Run the engine with cooperative cancellation.
///
/// Splits a user-requested [`RunLimit`] into chunks of [`CANCEL_CHUNK_SIZE`]
/// firings, calling [`Engine::run`] repeatedly and checking `cancel` between
/// chunks. When `cancel` flips to `true`, returns with
/// [`HaltReason::HaltRequested`] and the rules fired so far.
///
/// Note: the [`HaltReason::HaltRequested`] return code is the merged
/// "halt requested" signal — it covers external [`PinnedEngine::halt`]
/// (this function's `cancel` argument), a rule-level `(halt)` action, and
/// rule-level `(reset)` / `(clear)` requests (the engine's run loop returns
/// `HaltRequested` for those as well, deferring the reset/clear).
///
/// [`PinnedEngine::halt`]: crate::PinnedEngine::halt
pub(crate) fn run_with_cancel(
    engine: &mut Engine,
    limit: RunLimit,
    cancel: &AtomicBool,
) -> Result<RunResult, EngineError> {
    let mut total = 0usize;
    let mut remaining = match limit {
        RunLimit::Unlimited => usize::MAX,
        RunLimit::Count(n) => n,
    };

    loop {
        if cancel.load(Ordering::Acquire) {
            return Ok(RunResult {
                rules_fired: total,
                halt_reason: HaltReason::HaltRequested,
            });
        }
        if remaining == 0 {
            return Ok(RunResult {
                rules_fired: total,
                halt_reason: HaltReason::LimitReached,
            });
        }
        let step = remaining.min(CANCEL_CHUNK_SIZE);
        let r = engine.run(RunLimit::Count(step))?;
        total = total.saturating_add(r.rules_fired);
        remaining = remaining.saturating_sub(r.rules_fired);
        if matches!(
            r.halt_reason,
            HaltReason::AgendaEmpty | HaltReason::HaltRequested
        ) {
            return Ok(RunResult {
                rules_fired: total,
                halt_reason: r.halt_reason,
            });
        }
        // LimitReached on the inner chunk just means "we firedCHUNK_SIZE, loop".
    }
}
