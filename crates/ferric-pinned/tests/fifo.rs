//! FIFO ordering and concurrent-producer safety.

use std::sync::{Arc, Mutex};
use std::thread;

use ferric_pinned::{PinnedEngine, PinnedEngineOptions};

#[test]
fn requests_from_one_producer_run_in_order() {
    let engine = PinnedEngine::new(PinnedEngineOptions {
        queue_capacity: 256,
        ..Default::default()
    })
    .unwrap();
    let order = Arc::new(Mutex::new(Vec::<usize>::new()));

    for i in 0..200_usize {
        let order = order.clone();
        engine
            .with_engine(move |_engine| {
                order.lock().unwrap().push(i);
                Ok(())
            })
            .unwrap();
    }

    let collected = order.lock().unwrap().clone();
    let expected: Vec<usize> = (0..200).collect();
    assert_eq!(collected, expected);
}

#[test]
fn requests_from_multiple_producers_preserve_per_producer_order() {
    let engine = Arc::new(
        PinnedEngine::new(PinnedEngineOptions {
            queue_capacity: 256,
            ..Default::default()
        })
        .unwrap(),
    );
    let log: Arc<Mutex<Vec<(usize, usize)>>> = Arc::new(Mutex::new(Vec::new()));

    let producers = 4_usize;
    let per_producer = 50_usize;

    let handles: Vec<_> = (0..producers)
        .map(|producer_id| {
            let engine = engine.clone();
            let log = log.clone();
            thread::spawn(move || {
                for seq in 0..per_producer {
                    let log = log.clone();
                    engine
                        .with_engine(move |_engine| {
                            log.lock().unwrap().push((producer_id, seq));
                            Ok(())
                        })
                        .unwrap();
                }
            })
        })
        .collect();

    for h in handles {
        h.join().unwrap();
    }

    let log = log.lock().unwrap().clone();
    assert_eq!(log.len(), producers * per_producer);

    // Per-producer order must be preserved (FIFO from the worker's POV).
    for producer_id in 0..producers {
        let sequence: Vec<usize> = log
            .iter()
            .filter(|(p, _)| *p == producer_id)
            .map(|(_, s)| *s)
            .collect();
        assert_eq!(sequence, (0..per_producer).collect::<Vec<_>>());
    }
}
