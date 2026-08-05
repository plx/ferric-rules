//! Raw C logical-run continuation contract tests.

use std::ffi::{CStr, CString};

#[cfg(feature = "serde")]
use crate::engine::{
    ferric_bytes_free, ferric_engine_deserialize_bincode, ferric_engine_serialize_bincode,
};
use crate::engine::{
    ferric_engine_action_diagnostic_count, ferric_engine_agenda_count, ferric_engine_clear_error,
    ferric_engine_continue_run_ex, ferric_engine_fact_count, ferric_engine_free,
    ferric_engine_halt, ferric_engine_is_halted, ferric_engine_last_error,
    ferric_engine_load_string, ferric_engine_new, ferric_engine_reset, ferric_engine_retract,
    ferric_engine_run, ferric_engine_run_ex, ferric_engine_step, FerricEngine,
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

/// Everything a host can observe about a finished logical run.
#[derive(Debug, PartialEq, Eq)]
struct RunObservation {
    fired: u64,
    reason: FerricHaltReason,
    agenda: usize,
    diagnostics: usize,
}

/// Execute `source` as one logical run and report the observable outcome.
///
/// `chunk_size` of `None` runs the program in a single unlimited call;
/// `Some(n)` drives it as an `n`-activation chunk loop, exactly as a
/// cancelable host would.
unsafe fn observe_logical_run(source: &str, chunk_size: Option<u64>) -> RunObservation {
    let engine = ferric_engine_new();
    load_and_reset(engine, source);

    let limit = chunk_size.map_or(-1, |size| i64::try_from(size).unwrap());
    let mut chunk_fired = 0;
    let mut reason = FerricHaltReason::AgendaEmpty;
    assert_eq!(
        ferric_engine_run_ex(engine, limit, &mut chunk_fired, &mut reason),
        FerricError::Ok
    );
    let mut fired = chunk_fired;

    while chunk_size.is_some() && reason == FerricHaltReason::LimitReached {
        assert_eq!(
            ferric_engine_continue_run_ex(engine, limit, &mut chunk_fired, &mut reason),
            FerricError::Ok
        );
        fired += chunk_fired;
    }

    let mut agenda = 0;
    assert_eq!(
        ferric_engine_agenda_count(engine, &mut agenda),
        FerricError::Ok
    );
    let diagnostics = action_diagnostic_count(engine);
    ferric_engine_free(engine);

    RunObservation {
        fired,
        reason,
        agenda,
        diagnostics,
    }
}

/// A program that fires exactly `boundary` rules and then halts, leaving one
/// lower-salience activation on the agenda.
fn exact_boundary_halt_program(boundary: u64) -> String {
    assert!(boundary > 0);
    let target = boundary - 1;
    format!(
        r"
        (deffacts start (position 0))
        (defrule halt-at-boundary
            (declare (salience 100))
            (position {target})
            =>
            (halt))
        (defrule advance
            ?current <- (position ?n&:(< ?n {target}))
            =>
            (retract ?current)
            (assert (position (+ ?n 1))))
        (defrule after-halt
            (declare (salience -100))
            ?current <- (position {target})
            =>
            (retract ?current)
            (assert (past-boundary)))
        "
    )
}

#[test]
fn chunked_run_matches_one_shot_at_exact_halt_boundaries() {
    unsafe {
        // Chunk sizes that land exactly on the halting activation are the
        // regression case: without continuation the next chunk clears the
        // pending halt and fires one rule too many.
        for (boundary, chunk_size) in [(1, 1), (100, 100), (200, 100), (200, 200), (100, 7)] {
            let source = exact_boundary_halt_program(boundary);
            let one_shot = observe_logical_run(&source, None);
            let chunked = observe_logical_run(&source, Some(chunk_size));
            assert_eq!(
                one_shot.reason,
                FerricHaltReason::HaltRequested,
                "boundary {boundary} must halt in one-shot execution"
            );
            assert_eq!(one_shot.fired, boundary);
            assert_eq!(
                chunked, one_shot,
                "chunked execution diverged at exact halt boundary {boundary}"
            );
        }
    }
}

#[test]
fn chunked_run_matches_one_shot_for_diagnostics_and_agenda() {
    unsafe {
        let one_shot = observe_logical_run(EARLY_DIAGNOSTIC_PROGRAM, None);
        assert!(
            one_shot.diagnostics > 0,
            "the fixture must emit at least one diagnostic"
        );
        for chunk_size in [1, 2, 3] {
            assert_eq!(
                observe_logical_run(EARLY_DIAGNOSTIC_PROGRAM, Some(chunk_size)),
                one_shot,
                "chunk size {chunk_size} lost diagnostics or agenda state"
            );
        }
    }
}

#[test]
fn host_cancellation_is_distinct_from_every_engine_terminal_state() {
    unsafe {
        // A host that cancels simply stops submitting chunks. The engine is
        // then in a state no terminal halt reason can produce: the last chunk
        // reported LimitReached, nothing is halted, and work remains queued.
        let source = exact_boundary_halt_program(100);
        let engine = ferric_engine_new();
        load_and_reset(engine, &source);

        let mut fired = 0;
        let mut reason = FerricHaltReason::AgendaEmpty;
        assert_eq!(
            ferric_engine_run_ex(engine, 10, &mut fired, &mut reason),
            FerricError::Ok
        );
        let mut total_fired = fired;
        // Two chunks in, the host observes cancellation and stops.
        assert_eq!(
            ferric_engine_continue_run_ex(engine, 10, &mut fired, &mut reason),
            FerricError::Ok
        );
        total_fired += fired;

        assert_eq!(total_fired, 20);
        assert_eq!(
            reason,
            FerricHaltReason::LimitReached,
            "an abandoned chunk loop must not look like an engine halt"
        );

        let mut halted = 1;
        assert_eq!(
            ferric_engine_is_halted(engine, &mut halted),
            FerricError::Ok
        );
        assert_eq!(halted, 0, "cancellation must not set the engine halt flag");

        let mut agenda = 0;
        assert_eq!(
            ferric_engine_agenda_count(engine, &mut agenda),
            FerricError::Ok
        );
        assert!(
            agenda > 0,
            "cancellation must leave pending work on the agenda"
        );

        // Reporting cancellation and starting the next logical run is legal and
        // completes the work the canceled run left behind.
        assert_eq!(
            ferric_engine_run_ex(engine, -1, &mut fired, &mut reason),
            FerricError::Ok
        );
        assert_eq!(fired, 80, "the fresh run resumes from the preserved agenda");
        assert_eq!(reason, FerricHaltReason::HaltRequested);

        ferric_engine_free(engine);
    }
}

#[test]
fn read_only_queries_and_error_maintenance_preserve_continuation() {
    unsafe {
        let engine = ferric_engine_new();
        load_and_reset(engine, TWO_ACTIVATION_PROGRAM);

        let mut fired = 0;
        let mut reason = FerricHaltReason::AgendaEmpty;
        assert_eq!(
            ferric_engine_run_ex(engine, 1, &mut fired, &mut reason),
            FerricError::Ok
        );
        assert_eq!(reason, FerricHaltReason::LimitReached);

        // Everything a host may legitimately do between chunks.
        let mut facts = 0;
        assert_eq!(
            ferric_engine_fact_count(engine, &mut facts),
            FerricError::Ok
        );
        let mut agenda = 0;
        assert_eq!(
            ferric_engine_agenda_count(engine, &mut agenda),
            FerricError::Ok
        );
        let mut halted = 1;
        assert_eq!(
            ferric_engine_is_halted(engine, &mut halted),
            FerricError::Ok
        );
        let _ = action_diagnostic_count(engine);
        assert_eq!(ferric_engine_clear_error(engine), FerricError::Ok);

        assert_eq!(
            ferric_engine_continue_run_ex(engine, -1, &mut fired, &mut reason),
            FerricError::Ok,
            "read-only queries and error maintenance must not end the logical run"
        );
        assert_eq!(reason, FerricHaltReason::AgendaEmpty);

        ferric_engine_free(engine);
    }
}

#[test]
fn a_failed_call_that_reached_engine_state_still_ends_the_logical_run() {
    unsafe {
        let engine = ferric_engine_new();
        load_and_reset(engine, TWO_ACTIVATION_PROGRAM);

        let mut fired = 0;
        let mut reason = FerricHaltReason::AgendaEmpty;
        assert_eq!(
            ferric_engine_run_ex(engine, 1, &mut fired, &mut reason),
            FerricError::Ok
        );
        assert_eq!(reason, FerricHaltReason::LimitReached);

        // Retracting a fact ID that was never asserted fails, but the call is
        // still a mutating entry point and closes continuation eligibility.
        assert_ne!(ferric_engine_retract(engine, u64::MAX), FerricError::Ok);
        assert_eq!(
            ferric_engine_continue_run_ex(engine, -1, &mut fired, &mut reason),
            FerricError::InvalidArgument
        );

        ferric_engine_free(engine);
    }
}

const ACTION_ERROR_PROGRAM: &str = r"
    (defrule first (initial-fact) => (assert (go)))
    (defrule boom (go) => (/ 1 0) (assert (never)))
";

/// Drive one chunk and assert the logical run is eligible to continue.
unsafe fn start_eligible_logical_run(engine: *mut FerricEngine, source: &str) {
    load_and_reset(engine, source);
    let mut fired = 0;
    let mut reason = FerricHaltReason::AgendaEmpty;
    assert_eq!(
        ferric_engine_run_ex(engine, 1, &mut fired, &mut reason),
        FerricError::Ok
    );
    assert_eq!(reason, FerricHaltReason::LimitReached);
}

#[test]
fn a_wrong_thread_mutating_call_leaves_the_logical_run_intact() {
    unsafe {
        let engine = ferric_engine_new();
        start_eligible_logical_run(engine, TWO_ACTIVATION_PROGRAM);

        // A wrong-thread call is rejected before it reaches engine state, so
        // per the thread-affinity contract it must change nothing at all —
        // including this engine's continuation eligibility.
        let engine_addr = engine as usize;
        let rejected = std::thread::spawn(move || {
            let engine = engine_addr as *mut FerricEngine;
            ferric_engine_reset(engine)
        })
        .join()
        .unwrap();
        assert_eq!(rejected, FerricError::ThreadViolation);

        let mut fired = 0;
        let mut reason = FerricHaltReason::AgendaEmpty;
        assert_eq!(
            ferric_engine_continue_run_ex(engine, -1, &mut fired, &mut reason),
            FerricError::Ok,
            "a rejected wrong-thread call must not end the owner's logical run"
        );
        assert_eq!(fired, 1);
        assert_eq!(reason, FerricHaltReason::AgendaEmpty);

        ferric_engine_free(engine);
    }
}

#[test]
fn a_wrong_thread_continuation_is_rejected_without_consuming_eligibility() {
    unsafe {
        let engine = ferric_engine_new();
        start_eligible_logical_run(engine, TWO_ACTIVATION_PROGRAM);

        let engine_addr = engine as usize;
        let (code, fired, reason) = std::thread::spawn(move || {
            let engine = engine_addr as *mut FerricEngine;
            let mut fired = 55;
            let mut reason = FerricHaltReason::ActionError;
            let code = ferric_engine_continue_run_ex(engine, -1, &mut fired, &mut reason);
            (code, fired, reason)
        })
        .join()
        .unwrap();
        assert_eq!(code, FerricError::ThreadViolation);
        assert_eq!(fired, 55);
        assert_eq!(reason, FerricHaltReason::ActionError);

        let mut fired = 0;
        let mut reason = FerricHaltReason::AgendaEmpty;
        assert_eq!(
            ferric_engine_continue_run_ex(engine, -1, &mut fired, &mut reason),
            FerricError::Ok
        );
        assert_eq!(reason, FerricHaltReason::AgendaEmpty);

        ferric_engine_free(engine);
    }
}

#[test]
fn every_terminal_halt_reason_ends_continuation_eligibility() {
    unsafe {
        // AgendaEmpty is covered by the lifecycle test above; check the two
        // remaining terminal reasons.
        let engine = ferric_engine_new();
        start_eligible_logical_run(engine, ACTION_ERROR_PROGRAM);
        let mut fired = 0;
        let mut reason = FerricHaltReason::AgendaEmpty;
        assert_eq!(
            ferric_engine_continue_run_ex(engine, -1, &mut fired, &mut reason),
            FerricError::Ok
        );
        assert_eq!(reason, FerricHaltReason::ActionError);
        assert_eq!(
            ferric_engine_continue_run_ex(engine, -1, &mut fired, &mut reason),
            FerricError::InvalidArgument,
            "ActionError is terminal for the logical run"
        );
        ferric_engine_free(engine);

        let engine = ferric_engine_new();
        let halting = exact_boundary_halt_program(2);
        start_eligible_logical_run(engine, &halting);
        // Chunk two lands exactly on the halting activation.
        assert_eq!(
            ferric_engine_continue_run_ex(engine, 1, &mut fired, &mut reason),
            FerricError::Ok
        );
        assert_eq!(reason, FerricHaltReason::LimitReached);
        assert_eq!(
            ferric_engine_continue_run_ex(engine, 1, &mut fired, &mut reason),
            FerricError::Ok
        );
        assert_eq!(fired, 0);
        assert_eq!(reason, FerricHaltReason::HaltRequested);
        assert_eq!(
            ferric_engine_continue_run_ex(engine, -1, &mut fired, &mut reason),
            FerricError::InvalidArgument,
            "HaltRequested is terminal for the logical run"
        );
        ferric_engine_free(engine);
    }
}

#[test]
fn zero_limit_chunks_fire_nothing_and_stay_eligible() {
    unsafe {
        let engine = ferric_engine_new();
        load_and_reset(engine, &exact_boundary_halt_program(2));

        let mut fired = 7;
        let mut reason = FerricHaltReason::AgendaEmpty;
        assert_eq!(
            ferric_engine_run_ex(engine, 0, &mut fired, &mut reason),
            FerricError::Ok
        );
        assert_eq!(fired, 0);
        assert_eq!(reason, FerricHaltReason::LimitReached);

        for _ in 0..2 {
            assert_eq!(
                ferric_engine_continue_run_ex(engine, 0, &mut fired, &mut reason),
                FerricError::Ok
            );
            assert_eq!(fired, 0);
            assert_eq!(reason, FerricHaltReason::LimitReached);
        }

        // Run up to and including the halting activation, then confirm that a
        // zero-limit chunk reports the neutral boundary rather than the pending
        // halt — the halt surfaces on the next chunk that can actually run.
        assert_eq!(
            ferric_engine_continue_run_ex(engine, 2, &mut fired, &mut reason),
            FerricError::Ok
        );
        assert_eq!(fired, 2);
        assert_eq!(reason, FerricHaltReason::LimitReached);
        let mut halted = 0;
        assert_eq!(
            ferric_engine_is_halted(engine, &mut halted),
            FerricError::Ok
        );
        assert_eq!(halted, 1);
        assert_eq!(
            ferric_engine_continue_run_ex(engine, 0, &mut fired, &mut reason),
            FerricError::Ok
        );
        assert_eq!(reason, FerricHaltReason::LimitReached);
        assert_eq!(
            ferric_engine_continue_run_ex(engine, -1, &mut fired, &mut reason),
            FerricError::Ok
        );
        assert_eq!(fired, 0);
        assert_eq!(reason, FerricHaltReason::HaltRequested);

        ferric_engine_free(engine);
    }
}

#[test]
fn continuation_accepts_null_output_pointers_on_the_success_path() {
    unsafe {
        // Either output may be discarded independently. Whichever pointer is
        // supplied still receives its value, and the discarded reason still
        // drives eligibility.
        for (want_fired, want_reason) in [(false, false), (true, false), (false, true)] {
            let engine = ferric_engine_new();
            start_eligible_logical_run(engine, TWO_ACTIVATION_PROGRAM);

            let mut fired = u64::MAX;
            let mut reason = FerricHaltReason::LimitReached;
            let fired_out = if want_fired {
                std::ptr::addr_of_mut!(fired)
            } else {
                std::ptr::null_mut()
            };
            let reason_out = if want_reason {
                std::ptr::addr_of_mut!(reason)
            } else {
                std::ptr::null_mut()
            };
            assert_eq!(
                ferric_engine_continue_run_ex(engine, -1, fired_out, reason_out),
                FerricError::Ok,
                "null outputs must not affect the success path ({want_fired}, {want_reason})"
            );
            if want_fired {
                assert_eq!(fired, 1);
            } else {
                assert_eq!(fired, u64::MAX, "a null out_fired must not be written");
            }
            if want_reason {
                assert_eq!(reason, FerricHaltReason::AgendaEmpty);
            } else {
                assert_eq!(
                    reason,
                    FerricHaltReason::LimitReached,
                    "a null out_reason must not be written"
                );
            }

            // The run reached AgendaEmpty, so eligibility must be closed even
            // when the host discarded that reason.
            let mut probe_fired = 0;
            let mut probe_reason = FerricHaltReason::AgendaEmpty;
            assert_eq!(
                ferric_engine_continue_run_ex(engine, -1, &mut probe_fired, &mut probe_reason),
                FerricError::InvalidArgument
            );

            ferric_engine_free(engine);
        }
    }
}

#[test]
fn legacy_run_and_step_do_not_participate_in_logical_runs() {
    unsafe {
        // `ferric_engine_run` cannot report LimitReached, so it never arms
        // continuation even when it stops at its limit.
        let engine = ferric_engine_new();
        load_and_reset(engine, TWO_ACTIVATION_PROGRAM);
        let mut legacy_fired = 0;
        assert_eq!(
            ferric_engine_run(engine, 1, &mut legacy_fired),
            FerricError::Ok
        );
        assert_eq!(legacy_fired, 1);
        let mut fired = 0;
        let mut reason = FerricHaltReason::AgendaEmpty;
        assert_eq!(
            ferric_engine_continue_run_ex(engine, -1, &mut fired, &mut reason),
            FerricError::InvalidArgument
        );
        ferric_engine_free(engine);

        // A single-activation step is a mutating call and ends the logical run.
        let engine = ferric_engine_new();
        start_eligible_logical_run(engine, TWO_ACTIVATION_PROGRAM);
        let mut stepped = 0;
        assert_eq!(ferric_engine_step(engine, &mut stepped), FerricError::Ok);
        assert_eq!(
            ferric_engine_continue_run_ex(engine, -1, &mut fired, &mut reason),
            FerricError::InvalidArgument
        );
        ferric_engine_free(engine);
    }
}

/// Caller storage plus the outcome of a same-engine runtime call attempted
/// from inside a serialization allocator callback.
#[cfg(feature = "serde")]
struct ReentrantResetContext {
    engine: *mut FerricEngine,
    storage: Vec<u8>,
    reset_result: FerricError,
}

#[cfg(feature = "serde")]
unsafe extern "C" fn reentrant_reset_allocator(
    size: usize,
    context: *mut std::ffi::c_void,
) -> *mut u8 {
    let context = &mut *context.cast::<ReentrantResetContext>();
    context.reset_result = ferric_engine_reset(context.engine);
    context.storage.resize(size, 0);
    context.storage.as_mut_ptr()
}

#[cfg(feature = "serde")]
#[test]
fn a_reentrant_mutating_call_leaves_the_logical_run_intact() {
    unsafe {
        let engine = ferric_engine_new();
        start_eligible_logical_run(engine, TWO_ACTIVATION_PROGRAM);

        // Serialization is a read-only query, and the mutating call its
        // allocator callback attempts is rejected before it reaches engine
        // state. Neither may end the owner's logical run.
        let mut context = ReentrantResetContext {
            engine,
            storage: Vec::new(),
            reset_result: FerricError::Ok,
        };
        let mut data = std::ptr::null_mut();
        let mut len = 0;
        assert_eq!(
            ferric_engine_serialize_bincode(
                engine,
                Some(reentrant_reset_allocator),
                std::ptr::addr_of_mut!(context).cast::<std::ffi::c_void>(),
                &mut data,
                &mut len,
            ),
            FerricError::Ok
        );
        assert_eq!(
            context.reset_result,
            FerricError::InternalError,
            "same-engine runtime reentry must be rejected"
        );

        let mut fired = 0;
        let mut reason = FerricHaltReason::AgendaEmpty;
        assert_eq!(
            ferric_engine_continue_run_ex(engine, -1, &mut fired, &mut reason),
            FerricError::Ok,
            "a rejected reentrant call must not end the logical run"
        );
        assert_eq!(fired, 1);
        assert_eq!(reason, FerricHaltReason::AgendaEmpty);

        ferric_engine_free(engine);
    }
}

#[cfg(feature = "serde")]
#[test]
fn continuation_eligibility_is_per_handle_and_not_serialized() {
    unsafe {
        let engine = ferric_engine_new();
        start_eligible_logical_run(engine, TWO_ACTIVATION_PROGRAM);

        let mut data = std::ptr::null_mut();
        let mut len = 0;
        assert_eq!(
            ferric_engine_serialize_bincode(
                engine,
                None,
                std::ptr::null_mut(),
                &mut data,
                &mut len
            ),
            FerricError::Ok
        );

        let mut restored = std::ptr::null_mut();
        assert_eq!(
            ferric_engine_deserialize_bincode(data, len, &mut restored),
            FerricError::Ok
        );
        ferric_bytes_free(data, len);

        let mut fired = 42;
        let mut reason = FerricHaltReason::ActionError;
        assert_eq!(
            ferric_engine_continue_run_ex(restored, -1, &mut fired, &mut reason),
            FerricError::InvalidArgument,
            "a restored handle always begins a fresh logical run"
        );
        assert_eq!(fired, 42);
        assert_eq!(reason, FerricHaltReason::ActionError);
        ferric_engine_free(restored);

        // Serializing is a read-only query, so the source handle is untouched.
        assert_eq!(
            ferric_engine_continue_run_ex(engine, -1, &mut fired, &mut reason),
            FerricError::Ok
        );
        assert_eq!(reason, FerricHaltReason::AgendaEmpty);

        ferric_engine_free(engine);
    }
}

#[test]
fn repeated_fresh_runs_overfire_at_an_exact_halt_boundary() {
    unsafe {
        // The reproducer from FR-CABI-009, kept as a characterization test.
        // A host that chunks a long run with repeated `ferric_engine_run_ex`
        // calls discards the halt each chunk cleared. At a 100-activation
        // boundary with a 100-activation chunk the run reports 101 fired
        // rules instead of 100, and the engine ends up un-halted with the
        // post-halt rule already fired. This is correct fresh-run behavior —
        // it is why `ferric_engine_continue_run_ex` exists.
        let source = exact_boundary_halt_program(100);
        let engine = ferric_engine_new();
        load_and_reset(engine, &source);

        let mut fired = 0;
        let mut reason = FerricHaltReason::AgendaEmpty;
        let mut total = 0;
        loop {
            assert_eq!(
                ferric_engine_run_ex(engine, 100, &mut fired, &mut reason),
                FerricError::Ok
            );
            total += fired;
            if reason != FerricHaltReason::LimitReached {
                break;
            }
        }

        assert_eq!(total, 101, "the naive chunk loop fires one rule too many");
        assert_eq!(reason, FerricHaltReason::AgendaEmpty);

        // The continuation-based loop agrees with one-shot execution instead.
        assert_eq!(
            observe_logical_run(&source, Some(100)),
            observe_logical_run(&source, None)
        );

        ferric_engine_free(engine);
    }
}
