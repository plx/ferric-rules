//! Worker thread main loop and the cancel-aware run helper.
//!
//! The worker owns the [`Engine`] for its entire lifetime. It blocks on the
//! request channel for the first request of each batch, then drains
//! immediately-available requests (up to `max_batch_size`) and dispatches them
//! through the configured [`AutoreleasePolicy`].
//!
//! [`Engine`]: ferric_rules_runtime::Engine

use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::SyncSender;

use crossbeam_channel::Receiver;
use ferric_rules_runtime::engine::EngineError;
use ferric_rules_runtime::{Engine, HaltReason, RunLimit, RunResult};

use crate::autorelease;
use crate::error::PinnedError;
use crate::options::ResolvedOptions;
use crate::request::{discard_panic_payload, Request};

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
        // All senders dropped ⇒ recv returns Err. Buffered messages are
        // returned before disconnection, so close-drain falls out for free.
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
            dispatch_request(engine, first);
            drain_more(rx, engine, remaining_after_first);
        }
        P::PerItem => {
            autorelease::wrap(|| dispatch_request(engine, first));
            drain_more_per_item(rx, engine, remaining_after_first);
        }
        P::PerBatch => autorelease::wrap(|| {
            dispatch_request(engine, first);
            drain_more(rx, engine, remaining_after_first);
        }),
    }
}

fn dispatch_request(engine: &mut Engine, request: Request) {
    // One misbehaving request must not terminate the worker and drop every
    // queued completion. Sync requests observe their dropped response sender
    // as DispatchFailed; later requests remain available for dispatch.
    if let Err(payload) = catch_unwind(AssertUnwindSafe(|| request.dispatch(engine))) {
        discard_panic_payload(payload);
    }
}

fn drain_more(rx: &Receiver<Request>, engine: &mut Engine, max: usize) {
    for _ in 0..max {
        match rx.try_recv() {
            Ok(req) => dispatch_request(engine, req),
            Err(_) => return,
        }
    }
}

fn drain_more_per_item(rx: &Receiver<Request>, engine: &mut Engine, max: usize) {
    for _ in 0..max {
        match rx.try_recv() {
            Ok(req) => autorelease::wrap(|| dispatch_request(engine, req)),
            Err(_) => return,
        }
    }
}

/// Run the engine with cooperative cancellation.
///
/// Establishes fresh-run state with [`Engine::run`] and `RunLimit::Count(0)`, then
/// splits positive work into [`CANCEL_CHUNK_SIZE`] continuation chunks between
/// cancellation checks. The zero-fire entry clears prior halt and diagnostic
/// state before control is observed without firing a rule. The sticky `closed`
/// flag ensures shutdown also interrupts every accepted run while the queue
/// drains. When either flag flips to `true`, returns with
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
    closed: &AtomicBool,
) -> Result<RunResult, EngineError> {
    // Every accepted logical run enters the native fresh-run path exactly
    // once, even when its limit is zero or external control is already set.
    // Count(0) fires nothing while clearing prior halt and diagnostic state.
    let entry_result = engine.run(RunLimit::Count(0))?;
    let mut total = 0usize;
    let mut ran_positive_chunk = false;
    let mut remaining = match limit {
        RunLimit::Unlimited => usize::MAX,
        RunLimit::Count(n) => n,
    };

    loop {
        if cancel.load(Ordering::Acquire) || closed.load(Ordering::Acquire) {
            return Ok(RunResult {
                rules_fired: total,
                halt_reason: HaltReason::HaltRequested,
            });
        }
        if remaining == 0 {
            return if ran_positive_chunk {
                Ok(RunResult {
                    rules_fired: total,
                    halt_reason: HaltReason::LimitReached,
                })
            } else {
                Ok(entry_result)
            };
        }
        let step = remaining.min(CANCEL_CHUNK_SIZE);
        ran_positive_chunk = true;
        let r = engine.continue_run(RunLimit::Count(step))?;
        total = total.saturating_add(r.rules_fired);
        remaining = remaining.saturating_sub(r.rules_fired);
        // Preserve every native terminal from the chunk which already
        // started. Only an inner LimitReached can need repair: when its
        // limit-th firing requested a rule-side halt, the engine's flag is the
        // stronger terminal before another chunk can clear it on entry.
        if matches!(
            r.halt_reason,
            HaltReason::AgendaEmpty | HaltReason::HaltRequested | HaltReason::ActionError
        ) {
            return Ok(RunResult {
                rules_fired: total,
                halt_reason: r.halt_reason,
            });
        }
        if r.halt_reason == HaltReason::LimitReached && engine.is_halted() {
            return Ok(RunResult {
                rules_fired: total,
                halt_reason: HaltReason::HaltRequested,
            });
        }
        // LimitReached on the inner chunk just means "we firedCHUNK_SIZE, loop".
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preset_cancel_still_enters_fresh_run_without_firing() {
        let mut engine = Engine::with_rules(
            r"
            (deffacts initial (ready))
            (defrule halt-then-fault
                (declare (salience 100))
                =>
                (halt)
                (/ 1 0))
            (defrule must-remain-pending (ready) => (assert (unexpected)))
            ",
        )
        .unwrap();

        let first = engine.run(RunLimit::Unlimited).unwrap();
        assert_eq!(first.rules_fired, 1);
        assert_eq!(first.halt_reason, HaltReason::ActionError);
        assert!(engine.is_halted());
        assert_eq!(engine.action_diagnostics().len(), 1);
        let pending_before = engine.agenda_len();
        assert!(pending_before > 0);

        let cancel = AtomicBool::new(true);
        let closed = AtomicBool::new(false);
        let canceled = run_with_cancel(&mut engine, RunLimit::Unlimited, &cancel, &closed).unwrap();

        assert_eq!(canceled.rules_fired, 0);
        assert_eq!(canceled.halt_reason, HaltReason::HaltRequested);
        assert!(!engine.is_halted());
        assert!(engine.action_diagnostics().is_empty());
        assert_eq!(engine.agenda_len(), pending_before);
    }
}
