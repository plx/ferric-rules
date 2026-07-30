//! Raw C logical-run continuation contract tests.

use std::ffi::{CStr, CString};

use crate::engine::{
    ferric_engine_action_diagnostic_count, ferric_engine_agenda_count,
    ferric_engine_continue_run_ex, ferric_engine_fact_count, ferric_engine_free,
    ferric_engine_halt, ferric_engine_is_halted, ferric_engine_last_error,
    ferric_engine_load_string, ferric_engine_new, ferric_engine_reset, ferric_engine_run_ex,
    FerricEngine,
};
use crate::error::FerricError;
use crate::types::FerricHaltReason;

unsafe fn load_and_reset(engine: *mut FerricEngine, source: &str) {
    let source = CString::new(source).unwrap();
    assert_eq!(
        ferric_engine_load_string(engine, source.as_ptr()),
        FerricError::Ok
    );
    assert_eq!(ferric_engine_reset(engine), FerricError::Ok);
}

unsafe fn action_diagnostic_count(engine: *const FerricEngine) -> usize {
    let mut count = usize::MAX;
    assert_eq!(
        ferric_engine_action_diagnostic_count(engine, &mut count),
        FerricError::Ok
    );
    count
}

const TWO_ACTIVATION_PROGRAM: &str = r"
    (defrule first (initial-fact) => (assert (next)))
    (defrule second (next) => (assert (done)))
";

const EARLY_DIAGNOSTIC_PROGRAM: &str = r"
    (defrule seed
        (initial-fact)
        =>
        (assert (marker keep))
        (assert (candidate 1))
        (assert (follow-up)))
    (defrule bad-match
        (candidate ?value&:(/ 1 0))
        =>
        (assert (must-not-fire)))
    (defrule finish
        ?pending <- (follow-up)
        =>
        (retract ?pending)
        (assert (done)))
";

#[test]
fn continuation_rejects_invalid_lifecycle_without_writing_outputs() {
    unsafe {
        let engine = ferric_engine_new();
        let mut fired = 77;
        let mut reason = FerricHaltReason::ActionError;

        assert_eq!(
            ferric_engine_continue_run_ex(engine, -1, &mut fired, &mut reason),
            FerricError::InvalidArgument
        );
        assert_eq!(fired, 77);
        assert_eq!(reason, FerricHaltReason::ActionError);
        let error = ferric_engine_last_error(engine);
        assert!(!error.is_null());
        assert!(CStr::from_ptr(error)
            .to_string_lossy()
            .contains("requires a preceding logical-run chunk"));

        load_and_reset(engine, TWO_ACTIVATION_PROGRAM);
        assert_eq!(
            ferric_engine_run_ex(engine, 1, &mut fired, &mut reason),
            FerricError::Ok
        );
        assert_eq!(fired, 1);
        assert_eq!(reason, FerricHaltReason::LimitReached);

        let mut agenda_count = 0;
        assert_eq!(
            ferric_engine_agenda_count(engine, &mut agenda_count),
            FerricError::Ok
        );
        assert_eq!(agenda_count, 1);

        assert_eq!(
            ferric_engine_continue_run_ex(engine, -1, &mut fired, &mut reason),
            FerricError::Ok
        );
        assert_eq!(fired, 1);
        assert_eq!(reason, FerricHaltReason::AgendaEmpty);

        fired = 88;
        reason = FerricHaltReason::HaltRequested;
        assert_eq!(
            ferric_engine_continue_run_ex(engine, -1, &mut fired, &mut reason),
            FerricError::InvalidArgument
        );
        assert_eq!(fired, 88);
        assert_eq!(reason, FerricHaltReason::HaltRequested);

        assert_eq!(ferric_engine_reset(engine), FerricError::Ok);
        assert_eq!(
            ferric_engine_run_ex(engine, 1, &mut fired, &mut reason),
            FerricError::Ok
        );
        assert_eq!(reason, FerricHaltReason::LimitReached);
        assert_eq!(ferric_engine_halt(engine), FerricError::Ok);
        assert_eq!(
            ferric_engine_continue_run_ex(engine, -1, &mut fired, &mut reason),
            FerricError::InvalidArgument,
            "runtime mutation between chunks must end continuation eligibility"
        );

        ferric_engine_free(engine);

        fired = 99;
        reason = FerricHaltReason::ActionError;
        assert_eq!(
            ferric_engine_continue_run_ex(std::ptr::null_mut(), -1, &mut fired, &mut reason),
            FerricError::NullPointer
        );
        assert_eq!(fired, 99);
        assert_eq!(reason, FerricHaltReason::ActionError);
    }
}

#[test]
fn continuation_preserves_diagnostics_from_an_early_chunk() {
    unsafe {
        let engine = ferric_engine_new();
        load_and_reset(engine, EARLY_DIAGNOSTIC_PROGRAM);

        let mut fired = 0;
        let mut reason = FerricHaltReason::AgendaEmpty;
        assert_eq!(
            ferric_engine_run_ex(engine, 1, &mut fired, &mut reason),
            FerricError::Ok
        );
        assert_eq!(fired, 1);
        assert_eq!(reason, FerricHaltReason::LimitReached);
        let early_count = action_diagnostic_count(engine);
        assert!(early_count > 0, "the first chunk must emit a diagnostic");

        assert_eq!(
            ferric_engine_continue_run_ex(engine, -1, &mut fired, &mut reason),
            FerricError::Ok
        );
        assert_eq!(fired, 1);
        assert_eq!(reason, FerricHaltReason::AgendaEmpty);
        assert_eq!(
            action_diagnostic_count(engine),
            early_count,
            "continuation must not clear diagnostics from an earlier chunk"
        );

        ferric_engine_free(engine);
    }
}

#[test]
fn fresh_run_resets_execution_diagnostics_without_resetting_working_memory() {
    unsafe {
        let engine = ferric_engine_new();
        load_and_reset(engine, EARLY_DIAGNOSTIC_PROGRAM);

        let mut fired = 0;
        let mut reason = FerricHaltReason::AgendaEmpty;
        assert_eq!(
            ferric_engine_run_ex(engine, 1, &mut fired, &mut reason),
            FerricError::Ok
        );
        assert_eq!(reason, FerricHaltReason::LimitReached);
        assert!(action_diagnostic_count(engine) > 0);

        let mut facts_before = 0;
        assert_eq!(
            ferric_engine_fact_count(engine, &mut facts_before),
            FerricError::Ok
        );
        assert!(facts_before > 0);

        assert_eq!(
            ferric_engine_run_ex(engine, -1, &mut fired, &mut reason),
            FerricError::Ok
        );
        assert_eq!(fired, 1);
        assert_eq!(reason, FerricHaltReason::AgendaEmpty);
        assert_eq!(
            action_diagnostic_count(engine),
            0,
            "a fresh logical run must clear prior execution diagnostics"
        );

        let mut facts_after = 0;
        assert_eq!(
            ferric_engine_fact_count(engine, &mut facts_after),
            FerricError::Ok
        );
        assert_eq!(
            facts_after, facts_before,
            "starting a fresh logical run must not reset working memory"
        );

        ferric_engine_free(engine);
    }
}

#[test]
fn fresh_run_clears_an_exact_boundary_halt_without_resetting_working_memory() {
    unsafe {
        let engine = ferric_engine_new();
        load_and_reset(
            engine,
            r"
            (defrule halt-first
                (declare (salience 100))
                (initial-fact)
                =>
                (halt))
            (defrule after-halt
                (declare (salience -100))
                (initial-fact)
                =>
                (assert (done)))
            ",
        );

        let mut fired = 0;
        let mut reason = FerricHaltReason::AgendaEmpty;
        assert_eq!(
            ferric_engine_run_ex(engine, 1, &mut fired, &mut reason),
            FerricError::Ok
        );
        assert_eq!(fired, 1);
        assert_eq!(reason, FerricHaltReason::LimitReached);

        let mut halted = 0;
        assert_eq!(
            ferric_engine_is_halted(engine, &mut halted),
            FerricError::Ok
        );
        assert_eq!(halted, 1);

        let mut facts_before = 0;
        assert_eq!(
            ferric_engine_fact_count(engine, &mut facts_before),
            FerricError::Ok
        );

        assert_eq!(
            ferric_engine_run_ex(engine, -1, &mut fired, &mut reason),
            FerricError::Ok
        );
        assert_eq!(fired, 1);
        assert_eq!(reason, FerricHaltReason::AgendaEmpty);
        assert_eq!(
            ferric_engine_is_halted(engine, &mut halted),
            FerricError::Ok
        );
        assert_eq!(halted, 0);

        let mut facts_after = 0;
        assert_eq!(
            ferric_engine_fact_count(engine, &mut facts_after),
            FerricError::Ok
        );
        assert_eq!(
            facts_after,
            facts_before + 1,
            "the fresh run must execute the preserved agenda instead of resetting working memory"
        );

        ferric_engine_free(engine);
    }
}
