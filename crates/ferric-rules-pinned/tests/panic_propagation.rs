//! Synchronous request panics surface as `DispatchFailed`; completion-aware
//! asynchronous panics surface as `Internal`. The worker survives both.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{mpsc, Arc};
use std::thread;
use std::time::Duration;

use ferric_rules_pinned::{PinnedEngine, PinnedEngineOptions, PinnedError, RunLimit};

#[test]
fn closure_panic_returns_dispatch_failed_but_worker_survives() {
    let engine = PinnedEngine::new(PinnedEngineOptions::default()).unwrap();

    let result = engine.with_engine(|_engine| -> Result<(), PinnedError> {
        panic!("synthetic worker panic");
    });
    assert!(
        matches!(result, Err(PinnedError::DispatchFailed)),
        "expected DispatchFailed, got {result:?}"
    );

    engine
        .with_engine(|_| Ok(()))
        .expect("worker should accept requests after a caught panic");
    engine.close().unwrap();
}

#[test]
fn panic_does_not_drop_queued_async_completion() {
    let engine = Arc::new(PinnedEngine::new(PinnedEngineOptions::default()).unwrap());
    let (entered_tx, entered_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();

    let panic_engine = engine.clone();
    let panicking_request = thread::spawn(move || {
        panic_engine.with_engine(move |_| -> Result<(), PinnedError> {
            entered_tx.send(()).unwrap();
            release_rx.recv().unwrap();
            panic!("synthetic worker request panic");
        })
    });
    entered_rx.recv_timeout(Duration::from_secs(2)).unwrap();

    let (completion_tx, completion_rx) = mpsc::channel();
    engine
        .run_async(RunLimit::Count(0), move |result| {
            completion_tx.send(result).unwrap();
        })
        .unwrap();
    release_tx.send(()).unwrap();

    let panic_result = panicking_request.join().unwrap();
    assert!(
        matches!(panic_result, Err(PinnedError::DispatchFailed)),
        "expected DispatchFailed, got {panic_result:?}"
    );
    completion_rx
        .recv_timeout(Duration::from_secs(2))
        .expect("queued async completion was dropped")
        .expect("queued async request should complete successfully");

    engine
        .with_engine(|_| Ok(()))
        .expect("worker should remain usable");
    engine.close().unwrap();
}

#[test]
fn panicking_async_operation_completes_once_with_internal_error() {
    let engine = PinnedEngine::new(PinnedEngineOptions::default()).unwrap();
    let completion_count = Arc::new(AtomicUsize::new(0));
    let callback_count = completion_count.clone();
    let (completion_tx, completion_rx) = mpsc::channel();

    engine
        .submit_with_completion(
            |_| -> Result<(), PinnedError> {
                panic!("synthetic async request panic");
            },
            move |result| {
                callback_count.fetch_add(1, Ordering::SeqCst);
                completion_tx.send(result).unwrap();
            },
        )
        .unwrap();

    let result = completion_rx
        .recv_timeout(Duration::from_secs(2))
        .expect("accepted panicking request did not complete");
    assert!(matches!(result, Err(PinnedError::Internal)));
    assert_eq!(completion_count.load(Ordering::SeqCst), 1);

    engine
        .with_engine(|_| Ok(()))
        .expect("worker should remain usable after contained async panic");
    thread::sleep(Duration::from_millis(20));
    assert_eq!(completion_count.load(Ordering::SeqCst), 1);
    engine.close().unwrap();
}

#[test]
fn panicking_completion_is_consumed_before_worker_containment() {
    let engine = PinnedEngine::new(PinnedEngineOptions::default()).unwrap();
    let completion_count = Arc::new(AtomicUsize::new(0));
    let callback_count = completion_count.clone();
    let (entered_tx, entered_rx) = mpsc::channel();

    engine
        .submit_with_completion(
            |_| Ok(()),
            move |_| {
                callback_count.fetch_add(1, Ordering::SeqCst);
                entered_tx.send(()).unwrap();
                panic!("synthetic completion panic");
            },
        )
        .unwrap();

    entered_rx
        .recv_timeout(Duration::from_secs(2))
        .expect("completion did not run");
    engine
        .with_engine(|_| Ok(()))
        .expect("worker should remain usable after callback panic");
    assert_eq!(completion_count.load(Ordering::SeqCst), 1);
    engine.close().unwrap();
}
