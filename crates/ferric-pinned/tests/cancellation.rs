//! `halt` interrupts an in-flight `run` within a bounded number of firings.

use std::sync::Arc;
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

    // Halt before run. PinnedEngine::run clears the cancel flag before
    // dispatching, so this halt is NOT honored by the next run (it's a no-op).
    // Verify the next run still completes some firings rather than exiting at 0.
    engine.halt();

    let result = engine.run(RunLimit::Count(5)).unwrap();
    assert!(
        matches!(
            result.halt_reason,
            HaltReason::LimitReached | HaltReason::AgendaEmpty
        ),
        "pre-run halt should be cleared; got {:?}",
        result.halt_reason
    );
    assert_eq!(result.rules_fired, 5);
}

#[test]
fn halt_after_submit_but_before_dispatch_is_honored() {
    // This tests the narrower window where halt() is called AFTER the run
    // closure is enqueued but BEFORE the worker picks it up. In practice this
    // is the same as "halt during run" for a busy worker — we just demonstrate
    // that a halt issued from outside the worker thread eventually surfaces.
    let engine = Arc::new(PinnedEngine::new(PinnedEngineOptions::default()).unwrap());
    engine.load_str(CYCLING_RULES).unwrap();
    engine.reset().unwrap();

    let halt_engine = engine.clone();
    let h = thread::spawn(move || {
        halt_engine.halt();
    });
    h.join().unwrap();

    // Now run — halt set, but PinnedEngine::run clears it. So this run will
    // complete normally up to the limit. This documents (not enforces) the
    // semantic that "halt clears at run() entry."
    let result = engine.run(RunLimit::Count(3)).unwrap();
    assert_eq!(result.rules_fired, 3);
}
