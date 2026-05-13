//! `halt` interrupts an in-flight `run` within a bounded number of firings.

use std::sync::{mpsc, Arc};
use std::thread;
use std::time::{Duration, Instant};

use ferric_pinned::{HaltReason, PinnedEngine, PinnedEngineOptions, RunLimit};

const CYCLING_RULES: &str = r"
(defrule cycle ?f <- (counter ?n) => (retract ?f) (assert (counter (+ ?n 1))))
(deffacts initial (counter 0))
";

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
fn halt_before_run_takes_effect_immediately() {
    let engine = PinnedEngine::new(PinnedEngineOptions::default()).unwrap();
    engine.load_str(CYCLING_RULES).unwrap();
    engine.reset().unwrap();

    // Halt before run applies to the next-dispatched run.
    engine.halt();

    let result = engine.run(RunLimit::Count(5)).unwrap();
    assert_eq!(result.halt_reason, HaltReason::HaltRequested);
    assert_eq!(result.rules_fired, 0);
}

#[test]
fn halt_after_submit_but_before_dispatch_is_honored() {
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
    // dispatches it.
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
    assert_eq!(first.halt_reason, HaltReason::HaltRequested);
    assert_eq!(first.rules_fired, 0);

    let second = second_rx
        .recv_timeout(Duration::from_secs(2))
        .unwrap()
        .unwrap();
    assert_eq!(second.halt_reason, HaltReason::LimitReached);
    assert_eq!(second.rules_fired, 1);
}
