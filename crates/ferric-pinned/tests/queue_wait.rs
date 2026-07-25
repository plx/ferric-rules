//! Bounded-queue admission waits.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use ferric_pinned::{
    PinnedEngine, PinnedEngineOptions, PinnedError, PreDispatchCancelToken, QueueWait, RunLimit,
};

fn blocked_engine() -> (PinnedEngine, mpsc::SyncSender<()>) {
    let engine = PinnedEngine::new(PinnedEngineOptions {
        queue_capacity: 1,
        ..Default::default()
    })
    .unwrap();
    let (started_tx, started_rx) = mpsc::sync_channel(0);
    let (release_tx, release_rx) = mpsc::sync_channel(0);
    engine
        .submit(move |_| {
            started_tx.send(()).unwrap();
            release_rx.recv().unwrap();
        })
        .unwrap();
    started_rx.recv().unwrap();
    (engine, release_tx)
}

#[test]
fn queue_wait_blocks_until_capacity_then_preserves_fifo_order() {
    let (engine, release) = blocked_engine();
    let order = Arc::new(Mutex::new(Vec::new()));

    let queued_order = order.clone();
    engine
        .submit(move |_| {
            queued_order.lock().unwrap().push("queued");
        })
        .unwrap();

    let waiter_engine = engine.clone();
    let waited_order = order.clone();
    let (result_tx, result_rx) = mpsc::channel();
    let waiter = thread::spawn(move || {
        let result = waiter_engine.submit_with_queue_wait(
            QueueWait::Timeout(Duration::from_secs(2)),
            move |_| {
                waited_order.lock().unwrap().push("waited");
            },
        );
        result_tx.send(result).unwrap();
    });

    assert!(
        matches!(
            result_rx.recv_timeout(Duration::from_millis(50)),
            Err(RecvTimeoutError::Timeout)
        ),
        "submission returned while the bounded queue was still full"
    );
    release.send(()).unwrap();
    assert!(result_rx
        .recv_timeout(Duration::from_secs(2))
        .unwrap()
        .is_ok());
    waiter.join().unwrap();

    engine.close().unwrap();
    assert_eq!(*order.lock().unwrap(), ["queued", "waited"]);
}

#[test]
fn queue_wait_timeout_waits_then_reports_queue_full() {
    let (engine, release) = blocked_engine();
    engine.submit(|_| {}).unwrap();
    assert!(matches!(engine.submit(|_| {}), Err(PinnedError::QueueFull)));

    let started = Instant::now();
    let result = engine
        .submit_with_queue_wait(QueueWait::Timeout(Duration::from_millis(40)), |_| {
            panic!("timed-out request must not execute")
        });
    let elapsed = started.elapsed();

    assert!(matches!(result, Err(PinnedError::QueueFull)));
    assert!(
        elapsed >= Duration::from_millis(20),
        "queue wait returned too early after {elapsed:?}"
    );
    release.send(()).unwrap();
    engine.close().unwrap();
}

#[test]
fn canceled_queue_wait_returns_without_admitting_or_completing() {
    let (engine, release) = blocked_engine();
    engine.submit(|_| {}).unwrap();

    let token = Arc::new(PreDispatchCancelToken::new());
    let waiter_engine = engine.clone();
    let waiter_token = token.clone();
    let completion_fired = Arc::new(AtomicBool::new(false));
    let waiter_completion_fired = completion_fired.clone();
    let (result_tx, result_rx) = mpsc::channel();
    let waiter = thread::spawn(move || {
        let result = waiter_engine.run_async_cancelable_with_queue_wait(
            RunLimit::Count(1),
            waiter_token,
            QueueWait::Indefinite,
            move |_| waiter_completion_fired.store(true, Ordering::Release),
        );
        result_tx.send(result).unwrap();
    });

    assert!(matches!(
        result_rx.recv_timeout(Duration::from_millis(50)),
        Err(RecvTimeoutError::Timeout)
    ));
    assert!(token.cancel_run());
    assert!(matches!(
        result_rx.recv_timeout(Duration::from_secs(2)).unwrap(),
        Err(PinnedError::Canceled)
    ));
    waiter.join().unwrap();

    release.send(()).unwrap();
    engine.close().unwrap();
    assert!(!completion_fired.load(Ordering::Acquire));
}

#[test]
fn close_wakes_indefinite_queue_wait_without_admitting_it() {
    let (engine, release) = blocked_engine();
    engine.submit(|_| {}).unwrap();

    let waiter_engine = engine.clone();
    let request_ran = Arc::new(AtomicBool::new(false));
    let waiter_request_ran = request_ran.clone();
    let (result_tx, result_rx) = mpsc::channel();
    let waiter = thread::spawn(move || {
        let result = waiter_engine.submit_with_queue_wait(QueueWait::Indefinite, move |_| {
            waiter_request_ran.store(true, Ordering::Release);
        });
        result_tx.send(result).unwrap();
    });
    assert!(matches!(
        result_rx.recv_timeout(Duration::from_millis(50)),
        Err(RecvTimeoutError::Timeout)
    ));

    let closer_engine = engine.clone();
    let (close_tx, close_rx) = mpsc::channel();
    let closer = thread::spawn(move || close_tx.send(closer_engine.close()).unwrap());
    assert!(matches!(
        result_rx.recv_timeout(Duration::from_secs(2)).unwrap(),
        Err(PinnedError::Closed)
    ));
    waiter.join().unwrap();

    release.send(()).unwrap();
    assert!(close_rx
        .recv_timeout(Duration::from_secs(2))
        .unwrap()
        .is_ok());
    closer.join().unwrap();
    assert!(!request_ran.load(Ordering::Acquire));
}

#[test]
fn blocking_queue_wait_from_worker_is_rejected_as_reentrant() {
    let engine = PinnedEngine::new(PinnedEngineOptions::default()).unwrap();
    let worker_engine = engine.clone();
    let (result_tx, result_rx) = mpsc::channel();
    engine
        .submit(move |_| {
            let result = worker_engine.submit_with_queue_wait(QueueWait::Indefinite, |_| {});
            result_tx.send(result).unwrap();
        })
        .unwrap();

    assert!(matches!(
        result_rx.recv_timeout(Duration::from_secs(2)).unwrap(),
        Err(PinnedError::ReentrantCall)
    ));
    engine.close().unwrap();
}
