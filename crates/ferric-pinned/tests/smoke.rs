//! Basic lifecycle and rule-execution smoke test.

use ferric_pinned::{HaltReason, PinnedEngine, PinnedEngineOptions, RunLimit};

#[test]
fn load_reset_run_close_lifecycle() {
    let engine = PinnedEngine::new(PinnedEngineOptions::default()).expect("construct");

    engine
        .load_str("(defrule r => (assert (fired)))")
        .expect("load");
    engine.reset().expect("reset");

    let result = engine.run(RunLimit::Unlimited).expect("run");
    assert_eq!(result.rules_fired, 1);
    assert_eq!(result.halt_reason, HaltReason::AgendaEmpty);

    engine.close().expect("close");
    assert!(engine.is_closed());
}

#[test]
fn second_close_is_idempotent() {
    let engine = PinnedEngine::new(PinnedEngineOptions::default()).unwrap();
    engine.close().expect("first close");
    engine.close().expect("second close idempotent");
}

#[test]
fn drop_without_explicit_close() {
    // No assertion needed: this test passes if drop joins the worker cleanly.
    let engine = PinnedEngine::new(PinnedEngineOptions::default()).unwrap();
    engine.load_str("(defrule r => (assert (a)))").unwrap();
    engine.reset().unwrap();
    let _ = engine.run(RunLimit::Count(1)).unwrap();
    drop(engine);
}

#[test]
fn ops_after_close_return_closed_error() {
    let engine = PinnedEngine::new(PinnedEngineOptions::default()).unwrap();
    engine.close().unwrap();
    let err = engine.reset().unwrap_err();
    assert!(matches!(err, ferric_pinned::PinnedError::Closed));
}
