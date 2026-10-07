//! Tests for FFI execution and fact mutation APIs.

use crate::engine::{
    ferric_engine_assert_string, ferric_engine_fact_count, ferric_engine_free,
    ferric_engine_get_output, ferric_engine_load_string, ferric_engine_new, ferric_engine_reset,
    ferric_engine_retract, ferric_engine_run, ferric_engine_step,
};
use crate::error::FerricError;

#[test]
fn run_empty_engine_fires_nothing() {
    unsafe {
        let engine = ferric_engine_new();
        ferric_engine_reset(engine);
        let mut fired: u64 = 999;
        let result = ferric_engine_run(engine, -1, &mut fired);
        assert_eq!(result, FerricError::Ok);
        assert_eq!(fired, 0);
        ferric_engine_free(engine);
    }
}

#[test]
fn run_with_simple_rule() {
    unsafe {
        let engine = ferric_engine_new();
        let source = std::ffi::CString::new(
            r#"(defrule hello (initial-fact) => (printout t "hello" crlf))"#,
        )
        .unwrap();
        assert_eq!(
            ferric_engine_load_string(engine, source.as_ptr()),
            FerricError::Ok
        );
        ferric_engine_reset(engine);

        let mut fired: u64 = 0;
        let result = ferric_engine_run(engine, -1, &mut fired);
        assert_eq!(result, FerricError::Ok);
        assert_eq!(fired, 1);

        let channel = std::ffi::CString::new("t").unwrap();
        let output = ferric_engine_get_output(engine, channel.as_ptr());
        assert!(!output.is_null());
        let output_str = std::ffi::CStr::from_ptr(output).to_str().unwrap();
        assert!(output_str.contains("hello"), "output was: {output_str}");

        ferric_engine_free(engine);
    }
}

#[test]
fn run_with_limit() {
    unsafe {
        let engine = ferric_engine_new();
        let source = std::ffi::CString::new(
            "(defrule r1 (initial-fact) => (assert (a)))\n\
             (defrule r2 (a) => (assert (b)))\n\
             (defrule r3 (b) => (assert (c)))",
        )
        .unwrap();
        assert_eq!(
            ferric_engine_load_string(engine, source.as_ptr()),
            FerricError::Ok
        );
        ferric_engine_reset(engine);

        let mut fired: u64 = 0;
        let result = ferric_engine_run(engine, 2, &mut fired);
        assert_eq!(result, FerricError::Ok);
        assert_eq!(fired, 2);

        ferric_engine_free(engine);
    }
}

#[test]
fn run_null_out_fired_is_ok() {
    unsafe {
        let engine = ferric_engine_new();
        ferric_engine_reset(engine);
        let result = ferric_engine_run(engine, -1, std::ptr::null_mut());
        assert_eq!(result, FerricError::Ok);
        ferric_engine_free(engine);
    }
}

#[test]
fn run_null_engine() {
    unsafe {
        let mut fired: u64 = 0;
        let result = ferric_engine_run(std::ptr::null_mut(), -1, &mut fired);
        assert_eq!(result, FerricError::NullPointer);
    }
}

#[test]
fn step_empty_returns_agenda_empty() {
    unsafe {
        let engine = ferric_engine_new();
        ferric_engine_reset(engine);
        let mut status: i32 = 99;
        let result = ferric_engine_step(engine, &mut status);
        assert_eq!(result, FerricError::Ok);
        assert_eq!(status, 0);
        ferric_engine_free(engine);
    }
}

#[test]
fn step_fires_one_rule() {
    unsafe {
        let engine = ferric_engine_new();
        let source = std::ffi::CString::new(
            r#"(defrule test (initial-fact) => (printout t "stepped" crlf))"#,
        )
        .unwrap();
        assert_eq!(
            ferric_engine_load_string(engine, source.as_ptr()),
            FerricError::Ok
        );
        ferric_engine_reset(engine);

        let mut status: i32 = 0;
        let result = ferric_engine_step(engine, &mut status);
        assert_eq!(result, FerricError::Ok);
        assert_eq!(status, 1);

        // Second step should see an empty agenda
        let result = ferric_engine_step(engine, &mut status);
        assert_eq!(result, FerricError::Ok);
        assert_eq!(status, 0);

        ferric_engine_free(engine);
    }
}

#[test]
fn step_null_out_status_is_ok() {
    unsafe {
        let engine = ferric_engine_new();
        ferric_engine_reset(engine);
        let result = ferric_engine_step(engine, std::ptr::null_mut());
        assert_eq!(result, FerricError::Ok);
        ferric_engine_free(engine);
    }
}

#[test]
fn assert_string_ordered_fact() {
    unsafe {
        let engine = ferric_engine_new();
        ferric_engine_reset(engine);

        let source = std::ffi::CString::new("(assert (color red))").unwrap();
        let mut fact_id: u64 = 0;
        let result = ferric_engine_assert_string(engine, source.as_ptr(), &mut fact_id);
        assert_eq!(result, FerricError::Ok);
        assert_ne!(fact_id, 0, "assert should return a non-zero fact id");

        ferric_engine_free(engine);
    }
}

#[test]
fn assert_string_evaluates_ordered_and_template_expressions() {
    unsafe {
        let engine = ferric_engine_new();
        let setup = std::ffi::CString::new(
            r#"
            (deftemplate item (slot n) (multislot tags))
            (defglobal ?*g* = 5 ?*tags* = (create$ a b))
            (defrule verified
                (p 3 5 x y a b q)
                (item (n 6) (tags prefix a b x y))
                (item (n nil) (tags 5))
                => (printout t "verified" crlf))
            "#,
        )
        .unwrap();
        assert_eq!(
            ferric_engine_load_string(engine, setup.as_ptr()),
            FerricError::Ok
        );
        let source = std::ffi::CString::new(
            "(assert
                (p (+ 1 2) ?*g* (create$ x y) ?*tags* q)
                (item (n (+ ?*g* 1)) (tags prefix ?*tags* (create$ x y)))
                (item (tags ?*g*)))",
        )
        .unwrap();
        let mut first_fact_id = 0;
        assert_eq!(
            ferric_engine_assert_string(engine, source.as_ptr(), &mut first_fact_id),
            FerricError::Ok
        );
        assert_ne!(first_fact_id, 0);
        let mut count = 0;
        assert_eq!(
            ferric_engine_fact_count(engine, &mut count),
            FerricError::Ok
        );
        assert_eq!(count, 3);
        let mut fired = 0;
        assert_eq!(ferric_engine_run(engine, -1, &mut fired), FerricError::Ok);
        assert_eq!(fired, 1);
        let channel = std::ffi::CString::new("t").unwrap();
        let output = ferric_engine_get_output(engine, channel.as_ptr());
        assert!(!output.is_null());
        assert_eq!(
            std::ffi::CStr::from_ptr(output).to_str().unwrap(),
            "verified\n"
        );
        ferric_engine_free(engine);
    }
}

#[test]
fn assert_string_expression_errors_do_not_assert_partial_facts() {
    unsafe {
        let engine = ferric_engine_new();
        let setup = std::ffi::CString::new("(deftemplate item (slot n) (multislot tags))").unwrap();
        assert_eq!(
            ferric_engine_load_string(engine, setup.as_ptr()),
            FerricError::Ok
        );
        for source in [
            "(assert (bad prefix ?missing suffix))",
            "(assert (bad prefix (missing-function) suffix))",
            "(assert (bad prefix ?*missing* suffix))",
            "(assert (item (n ?missing)))",
            "(assert (item (tags prefix (missing-function) suffix)))",
            "(assert (item (n (create$ 3))))",
        ] {
            let source = std::ffi::CString::new(source).unwrap();
            assert_ne!(
                ferric_engine_assert_string(engine, source.as_ptr(), std::ptr::null_mut()),
                FerricError::Ok,
                "source: {source:?}"
            );
            let mut count = 999;
            assert_eq!(
                ferric_engine_fact_count(engine, &mut count),
                FerricError::Ok
            );
            assert_eq!(count, 0, "source: {source:?}");
        }
        ferric_engine_free(engine);
    }
}

#[test]
fn assert_string_error_retains_only_completed_facts() {
    // An evaluation error keeps earlier facts; a static error asserts nothing.
    for (expression, retained, firings) in [("?missing", 1, 1), ("(missing-function)", 0, 0)] {
        unsafe {
            let engine = ferric_engine_new();
            let setup = std::ffi::CString::new(
                r#"(defrule retained (before 3) => (printout t "retained" crlf))"#,
            )
            .unwrap();
            assert_eq!(
                ferric_engine_load_string(engine, setup.as_ptr()),
                FerricError::Ok
            );
            let source = std::ffi::CString::new(format!(
                "(assert (before (+ 1 2)) (bad {expression}) (after))"
            ))
            .unwrap();
            assert_ne!(
                ferric_engine_assert_string(engine, source.as_ptr(), std::ptr::null_mut()),
                FerricError::Ok
            );
            let mut count = 0;
            assert_eq!(
                ferric_engine_fact_count(engine, &mut count),
                FerricError::Ok
            );
            assert_eq!(count, retained, "expression: {expression}");
            let mut fired = 0;
            assert_eq!(ferric_engine_run(engine, -1, &mut fired), FerricError::Ok);
            assert_eq!(fired, firings, "expression: {expression}");
            ferric_engine_free(engine);
        }
    }
}

#[test]
fn reset_deffacts_expression_error_is_runtime_error() {
    unsafe {
        let engine = ferric_engine_new();
        let source = std::ffi::CString::new(
            "(deffacts seed (before (+ 1 2)) (bad (/ 1 0)) (after))
             (defrule retained (before 3) =>)",
        )
        .unwrap();
        assert_eq!(
            ferric_engine_load_string(engine, source.as_ptr()),
            FerricError::Ok
        );
        let mut count = 999;
        assert_eq!(
            ferric_engine_fact_count(engine, &mut count),
            FerricError::Ok
        );
        assert_eq!(count, 0);
        assert_eq!(ferric_engine_reset(engine), FerricError::RuntimeError);
        assert_eq!(
            ferric_engine_fact_count(engine, &mut count),
            FerricError::Ok
        );
        assert_eq!(count, 1);
        let mut fired = 0;
        assert_eq!(ferric_engine_run(engine, -1, &mut fired), FerricError::Ok);
        assert_eq!(fired, 1);
        ferric_engine_free(engine);
    }
}

#[test]
fn assert_string_fact_id_can_be_retracted() {
    unsafe {
        let engine = ferric_engine_new();
        ferric_engine_reset(engine);

        let source = std::ffi::CString::new("(assert (item widget))").unwrap();
        let mut fact_id: u64 = 0;
        let result = ferric_engine_assert_string(engine, source.as_ptr(), &mut fact_id);
        assert_eq!(result, FerricError::Ok);
        assert_ne!(fact_id, 0);

        let retract_result = ferric_engine_retract(engine, fact_id);
        assert_eq!(retract_result, FerricError::Ok);

        ferric_engine_free(engine);
    }
}

#[test]
fn assert_string_null_engine() {
    unsafe {
        let source = std::ffi::CString::new("(assert (color red))").unwrap();
        let result = ferric_engine_assert_string(
            std::ptr::null_mut(),
            source.as_ptr(),
            std::ptr::null_mut(),
        );
        assert_eq!(result, FerricError::NullPointer);
    }
}

#[test]
fn assert_string_null_source() {
    unsafe {
        let engine = ferric_engine_new();
        let result = ferric_engine_assert_string(engine, std::ptr::null(), std::ptr::null_mut());
        assert_eq!(result, FerricError::NullPointer);
        ferric_engine_free(engine);
    }
}

#[test]
fn assert_string_invalid_syntax() {
    unsafe {
        let engine = ferric_engine_new();
        let source = std::ffi::CString::new("(assert (this is not closed").unwrap();
        let result = ferric_engine_assert_string(engine, source.as_ptr(), std::ptr::null_mut());
        assert_ne!(result, FerricError::Ok);
        ferric_engine_free(engine);
    }
}

#[test]
fn retract_nonexistent_fact() {
    unsafe {
        let engine = ferric_engine_new();
        ferric_engine_reset(engine);
        let result = ferric_engine_retract(engine, 0xDEAD_BEEF);
        assert_eq!(result, FerricError::NotFound);
        ferric_engine_free(engine);
    }
}

#[test]
fn retract_null_engine() {
    unsafe {
        let result = ferric_engine_retract(std::ptr::null_mut(), 1);
        assert_eq!(result, FerricError::NullPointer);
    }
}

#[test]
fn get_output_null_engine() {
    unsafe {
        let channel = std::ffi::CString::new("stdout").unwrap();
        let result = ferric_engine_get_output(std::ptr::null(), channel.as_ptr());
        assert!(result.is_null());
    }
}

#[test]
fn get_output_null_channel() {
    unsafe {
        let engine = ferric_engine_new();
        let result = ferric_engine_get_output(engine, std::ptr::null());
        assert!(result.is_null());
        ferric_engine_free(engine);
    }
}

#[test]
fn get_output_no_output_is_null() {
    unsafe {
        let engine = ferric_engine_new();
        ferric_engine_reset(engine);
        let channel = std::ffi::CString::new("stdout").unwrap();
        let result = ferric_engine_get_output(engine, channel.as_ptr());
        assert!(result.is_null());
        ferric_engine_free(engine);
    }
}

#[test]
fn full_load_reset_run_get_output_cycle() {
    unsafe {
        let engine = ferric_engine_new();

        let source = std::ffi::CString::new(
            r#"(defrule greet (initial-fact) => (printout t "Hello, FFI!" crlf))"#,
        )
        .unwrap();
        assert_eq!(
            ferric_engine_load_string(engine, source.as_ptr()),
            FerricError::Ok
        );

        assert_eq!(ferric_engine_reset(engine), FerricError::Ok);

        let mut fired: u64 = 0;
        assert_eq!(ferric_engine_run(engine, -1, &mut fired), FerricError::Ok);
        assert_eq!(fired, 1);

        let channel = std::ffi::CString::new("t").unwrap();
        let output = ferric_engine_get_output(engine, channel.as_ptr());
        assert!(!output.is_null());
        let output_str = std::ffi::CStr::from_ptr(output).to_str().unwrap();
        assert!(output_str.contains("Hello, FFI!"));

        ferric_engine_free(engine);
    }
}

#[test]
fn get_output_pointer_stays_stable_across_reads_without_writes() {
    unsafe {
        let engine = ferric_engine_new();
        let source = std::ffi::CString::new(
            r#"(defrule emit (initial-fact) => (printout t "hello") (printout stderr "err"))"#,
        )
        .unwrap();
        assert_eq!(
            ferric_engine_load_string(engine, source.as_ptr()),
            FerricError::Ok
        );
        assert_eq!(ferric_engine_reset(engine), FerricError::Ok);

        let mut fired: u64 = 0;
        assert_eq!(ferric_engine_run(engine, -1, &mut fired), FerricError::Ok);
        assert_eq!(fired, 1);

        let t_channel = std::ffi::CString::new("t").unwrap();
        let stderr_channel = std::ffi::CString::new("stderr").unwrap();

        let t_ptr_first = ferric_engine_get_output(engine, t_channel.as_ptr());
        assert!(!t_ptr_first.is_null());
        assert_eq!(
            std::ffi::CStr::from_ptr(t_ptr_first).to_str().unwrap(),
            "hello"
        );

        let stderr_ptr = ferric_engine_get_output(engine, stderr_channel.as_ptr());
        assert!(!stderr_ptr.is_null());
        assert_eq!(
            std::ffi::CStr::from_ptr(stderr_ptr).to_str().unwrap(),
            "err"
        );

        // Reading another channel must not invalidate an earlier channel pointer.
        assert_eq!(
            std::ffi::CStr::from_ptr(t_ptr_first).to_str().unwrap(),
            "hello"
        );

        let t_ptr_second = ferric_engine_get_output(engine, t_channel.as_ptr());
        assert_eq!(t_ptr_first, t_ptr_second);

        ferric_engine_free(engine);
    }
}

#[test]
fn borrowed_output_survives_serialized_transfer() {
    unsafe {
        let engine = ferric_engine_new();
        let source =
            std::ffi::CString::new(r#"(defrule emit (initial-fact) => (printout t "hello"))"#)
                .unwrap();
        assert_eq!(
            ferric_engine_load_string(engine, source.as_ptr()),
            FerricError::Ok
        );
        assert_eq!(ferric_engine_reset(engine), FerricError::Ok);

        let mut fired: u64 = 0;
        assert_eq!(ferric_engine_run(engine, -1, &mut fired), FerricError::Ok);
        assert_eq!(fired, 1);

        let channel = std::ffi::CString::new("t").unwrap();
        let owner_ptr = ferric_engine_get_output(engine, channel.as_ptr());
        assert!(!owner_ptr.is_null());

        let engine_addr = engine as usize;
        let snapshot_addr = owner_ptr as usize;
        let copied = std::thread::spawn(move || {
            // The owner waits for join, retaining both engine and borrowed
            // snapshot. Fact inspection does not invalidate the output cache.
            let engine = engine_addr as *const crate::engine::FerricEngine;
            let snapshot = snapshot_addr as *const std::os::raw::c_char;
            let mut count = 0;
            assert_eq!(
                ferric_engine_fact_count(engine, &mut count),
                FerricError::Ok
            );
            std::ffi::CStr::from_ptr(snapshot)
                .to_str()
                .unwrap()
                .to_owned()
        })
        .join()
        .unwrap();
        assert_eq!(copied, "hello");

        ferric_engine_free(engine);
    }
}
