//! Close drains accepted requests before joining the worker.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Barrier};
use std::thread;
use std::time::{Duration, Instant};

use ferric_pinned::{
    HaltReason, PinnedEngine, PinnedEngineOptions, PinnedError, RunLimit, RunResult,
};

const CYCLING_RULES: &str = r"
(defrule cycle ?f <- (counter ?n) => (retract ?f) (assert (counter (+ ?n 1))))
(deffacts initial (counter 0))
";

#[test]
fn close_drains_already_queued_requests() {
    // Configure a queue large enough to hold a backlog.
    let engine = Arc::new(
        PinnedEngine::new(PinnedEngineOptions {
            queue_capacity: 64,
            ..Default::default()
        })
        .unwrap(),
    );

    let processed = Arc::new(AtomicUsize::new(0));
    // Have the first request block until released so subsequent requests pile up.
    let release = Arc::new(Barrier::new(2));

    {
        let release = release.clone();
        let processed = processed.clone();
        let engine_handle = engine.clone();
        let _ = thread::spawn(move || {
            engine_handle
                .with_engine(move |_engine| {
                    release.wait(); // hold the worker until main releases
                    processed.fetch_add(1, Ordering::SeqCst);
                    Ok(())
                })
                .unwrap();
        });
    }

    // Give the blocking request time to enter the worker.
    thread::sleep(Duration::from_millis(20));

    // Submit several more requests from background threads; each will park
    // waiting for the response and queue behind the blocker.
    let pending: Vec<_> = (0..8_usize)
        .map(|_| {
            let engine_handle = engine.clone();
            let processed = processed.clone();
            thread::spawn(move || {
                engine_handle
                    .with_engine(move |_engine| {
                        processed.fetch_add(1, Ordering::SeqCst);
                        Ok(())
                    })
                    .unwrap();
            })
        })
        .collect();

    // Let the queue fill.
    thread::sleep(Duration::from_millis(20));

    // Release the blocker. Worker drains the rest.
    release.wait();

    // Close concurrently with draining.
    engine.close().unwrap();
    for h in pending {
        h.join().unwrap();
    }

    // 1 (blocker) + 8 (queued) = 9
    assert_eq!(processed.load(Ordering::SeqCst), 9);
    assert!(engine.is_closed());
}

#[test]
fn requests_submitted_after_close_return_closed() {
    let engine = PinnedEngine::new(PinnedEngineOptions::default()).unwrap();
    engine.close().unwrap();

    let err = engine.with_engine(|_| Ok(())).unwrap_err();
    assert!(matches!(err, PinnedError::Closed));
}

#[test]
fn close_interrupts_active_and_queued_unlimited_runs() {
    let engine = PinnedEngine::new(PinnedEngineOptions::default()).unwrap();
    engine.load_str(CYCLING_RULES).unwrap();
    engine.reset().unwrap();

    let (release_first_tx, release_first_rx) = mpsc::channel();
    let (first_tx, first_rx) = mpsc::channel::<Result<RunResult, PinnedError>>();
    engine
        .run_async(RunLimit::Unlimited, move |result| {
            first_tx.send(result).unwrap();
            release_first_rx.recv().unwrap();
        })
        .unwrap();

    let (second_tx, second_rx) = mpsc::channel::<Result<RunResult, PinnedError>>();
    engine
        .run_async(RunLimit::Unlimited, move |result| {
            second_tx.send(result).unwrap();
        })
        .unwrap();

    // Ensure the first cycling run is active, then stop it so its completion
    // can hold the worker while close marks the engine closed.
    thread::sleep(Duration::from_millis(50));
    engine.halt();
    let first = first_rx
        .recv_timeout(Duration::from_secs(2))
        .unwrap()
        .unwrap();
    assert_eq!(first.halt_reason, HaltReason::HaltRequested);

    let close_engine = engine.clone();
    let (close_tx, close_rx) = mpsc::channel();
    let close_thread = thread::spawn(move || {
        let _ = close_tx.send(close_engine.close());
    });

    let closed_deadline = Instant::now() + Duration::from_secs(2);
    while !engine.is_closed() {
        assert!(
            Instant::now() < closed_deadline,
            "close never marked the engine closed"
        );
        thread::yield_now();
    }
    release_first_tx.send(()).unwrap();

    let Ok(close_result) = close_rx.recv_timeout(Duration::from_secs(2)) else {
        // Clean up the pre-fix failure mode before failing the assertion:
        // close is waiting for the second unlimited run, so halt it.
        engine.halt();
        let _ = second_rx.recv_timeout(Duration::from_secs(2));
        let _ = close_rx.recv_timeout(Duration::from_secs(2));
        close_thread.join().unwrap();
        panic!("close did not interrupt the queued unlimited run");
    };
    close_result.unwrap();
    close_thread.join().unwrap();

    let second = second_rx
        .recv_timeout(Duration::from_secs(2))
        .unwrap()
        .unwrap();
    assert_eq!(second.rules_fired, 0);
    assert_eq!(second.halt_reason, HaltReason::HaltRequested);
}

#[test]
fn reentrant_with_engine_returns_error_without_deadlock() {
    let engine = PinnedEngine::new(PinnedEngineOptions::default()).unwrap();
    let reentrant_engine = engine.clone();
    let (result_tx, result_rx) = mpsc::channel();

    engine
        .submit(move |_| {
            let _ = result_tx.send(reentrant_engine.reset());
        })
        .unwrap();

    let Ok(result) = result_rx.recv_timeout(Duration::from_secs(2)) else {
        // The pre-fix worker is waiting on a request it enqueued to itself.
        // Leaking the outer handle prevents Drop from joining that worker.
        std::mem::forget(engine);
        panic!("reentrant with_engine call deadlocked");
    };
    assert!(
        matches!(result, Err(PinnedError::ReentrantCall)),
        "reentrant call should be rejected, got {result:?}"
    );

    engine.reset().expect("worker should remain usable");
    engine.close().unwrap();
}

/// Calling `close()` from inside a worker-side callback must not self-join
/// (which would deadlock the worker). The detached worker still exits
/// cleanly once the callback returns and observes the dropped sender.
#[test]
fn reentrant_close_from_worker_callback_does_not_deadlock() {
    let (done_tx, done_rx) = mpsc::channel::<()>();
    let worker = thread::spawn(move || {
        let engine = PinnedEngine::new(PinnedEngineOptions::default()).unwrap();
        let inner = engine.clone();
        engine
            .with_engine(move |_engine| {
                inner.close().expect("re-entrant close should succeed");
                Ok(())
            })
            .expect("with_engine should complete");

        assert!(engine.is_closed());
        let err = engine.with_engine(|_| Ok(())).unwrap_err();
        assert!(matches!(err, PinnedError::Closed));
        done_tx.send(()).unwrap();
    });

    done_rx
        .recv_timeout(Duration::from_secs(5))
        .expect("re-entrant close deadlocked");
    worker.join().unwrap();
}
