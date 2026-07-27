//! `halt` interrupts an in-flight `run` within a bounded number of firings.

use std::sync::{mpsc, Arc};
use std::thread;
use std::time::{Duration, Instant};

use ferric_rules_pinned::{
    HaltReason, PinnedEngine, PinnedEngineOptions, PinnedError, PreDispatchCancelToken, RunLimit,
};

const CYCLING_RULES: &str = r"
(defrule cycle ?f <- (counter ?n) => (retract ?f) (assert (counter (+ ?n 1))))
(deffacts initial (counter 0))
";

#[test]
fn rule_halt_on_chunk_boundary_stops_without_extra_firing() {
    let engine = PinnedEngine::new(PinnedEngineOptions::default()).unwrap();
    engine
        .load_str(
            r"
            (defrule count-to-boundary
                ?f <- (counter ?n&:(< ?n 65))
                =>
                (retract ?f)
                (assert (counter (+ ?n 1)))
                (if (= ?n 63) then (halt)))
            (deffacts initial (counter 0))
            ",
        )
        .unwrap();
    engine.reset().unwrap();

    let result = engine.run(RunLimit::Unlimited).unwrap();

    assert_eq!(result.rules_fired, 64);
    assert_eq!(result.halt_reason, HaltReason::HaltRequested);
}

#[test]
fn halt_cancels_unlimited_run() {
    let engine = Arc::new(PinnedEngine::new(PinnedEngineOptions::default()).unwrap());
    engine.load_str(CYCLING_RULES).unwrap();
    engine.reset().unwrap();

    let halt_engine = engine.clone();
    let halter = thread::spawn(move || {
        thread::sleep(Duration::from_millis(50));
        halt_engine.halt();
    });

    let started = Instant::now();
    let result = engine.run(RunLimit::Unlimited).unwrap();
    let elapsed = started.elapsed();
    halter.join().unwrap();

    assert_eq!(result.halt_reason, HaltReason::HaltRequested);
    assert!(result.rules_fired > 0, "should have fired at least once");
    // Cooperative; allow generous upper bound for slow CI.
    assert!(
        elapsed < Duration::from_secs(5),
        "halt should propagate within seconds, got {elapsed:?}"
    );
}

#[test]
fn halt_while_idle_does_not_affect_later_run() {
    let engine = PinnedEngine::new(PinnedEngineOptions::default()).unwrap();
    engine.load_str(CYCLING_RULES).unwrap();
    engine.reset().unwrap();

    // Global halt is active-run-only; an idle call must not poison a later run.
    engine.halt();

    let result = engine.run(RunLimit::Count(5)).unwrap();
    assert_eq!(result.halt_reason, HaltReason::LimitReached);
    assert_eq!(result.rules_fired, 5);
}

#[test]
fn halt_does_not_cancel_a_queued_run() {
    let engine = Arc::new(PinnedEngine::new(PinnedEngineOptions::default()).unwrap());
    engine.load_str(CYCLING_RULES).unwrap();
    engine.reset().unwrap();

    let (entered_tx, entered_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    let blocker_engine = engine.clone();
    let blocker = thread::spawn(move || {
        blocker_engine
            .with_engine(move |_engine| {
                entered_tx.send(()).unwrap();
                release_rx.recv().unwrap();
                Ok(())
            })
            .unwrap();
    });
    entered_rx.recv_timeout(Duration::from_secs(2)).unwrap();

    let (first_tx, first_rx) = mpsc::channel();
    engine
        .run_async(RunLimit::Count(10), move |result| {
            first_tx.send(result).unwrap();
        })
        .unwrap();

    // This halt happens after the first run is accepted but before the worker
    // dispatches it. Global halt must not latch onto queued work.
    engine.halt();

    let (second_tx, second_rx) = mpsc::channel();
    engine
        .run_async(RunLimit::Count(1), move |result| {
            second_tx.send(result).unwrap();
        })
        .unwrap();

    release_tx.send(()).unwrap();
    blocker.join().unwrap();

    let first = first_rx
        .recv_timeout(Duration::from_secs(2))
        .unwrap()
        .unwrap();
    assert_eq!(first.halt_reason, HaltReason::LimitReached);
    assert_eq!(first.rules_fired, 10);

    let second = second_rx
        .recv_timeout(Duration::from_secs(2))
        .unwrap()
        .unwrap();
    assert_eq!(second.halt_reason, HaltReason::LimitReached);
    assert_eq!(second.rules_fired, 1);
}

#[test]
fn pre_dispatch_cancel_reports_canceled_without_running_request() {
    let engine = Arc::new(PinnedEngine::new(PinnedEngineOptions::default()).unwrap());
    engine.load_str(CYCLING_RULES).unwrap();
    engine.reset().unwrap();

    let (entered_tx, entered_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    let blocker_engine = engine.clone();
    let blocker = thread::spawn(move || {
        blocker_engine
            .with_engine(move |_engine| {
                entered_tx.send(()).unwrap();
                release_rx.recv().unwrap();
                Ok(())
            })
            .unwrap();
    });
    entered_rx.recv_timeout(Duration::from_secs(2)).unwrap();

    let token = Arc::new(PreDispatchCancelToken::new());
    let (result_tx, result_rx) = mpsc::channel();
    engine
        .run_async_cancelable(RunLimit::Count(10), token.clone(), move |result| {
            result_tx.send(result).unwrap();
        })
        .unwrap();

    assert!(token.cancel_before_start());

    release_tx.send(()).unwrap();
    blocker.join().unwrap();

    let err = result_rx
        .recv_timeout(Duration::from_secs(2))
        .unwrap()
        .unwrap_err();
    assert!(matches!(err, PinnedError::Canceled));
}
