//! Close drains accepted requests before joining the worker.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Barrier};
use std::thread;
use std::time::Duration;

use ferric_pinned::{PinnedEngine, PinnedEngineOptions, PinnedError};

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
