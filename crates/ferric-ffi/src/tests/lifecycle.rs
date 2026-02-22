//! Tests for FFI engine lifecycle and thread affinity (Pass 004).

use crate::engine::{
    ferric_engine_clear_error, ferric_engine_free, ferric_engine_last_error,
    ferric_engine_load_string, ferric_engine_new, ferric_engine_new_with_config,
    ferric_engine_reset,
};
use crate::error::{ferric_last_error_global, FerricError};
use crate::types::{FerricConfig, FerricConflictStrategy, FerricStringEncoding};

#[test]
fn engine_new_returns_non_null() {
    unsafe {
        let engine = ferric_engine_new();
        assert!(!engine.is_null());
        ferric_engine_free(engine);
    }
}

#[test]
fn engine_new_with_null_config_returns_non_null() {
    unsafe {
        let engine = ferric_engine_new_with_config(std::ptr::null());
        assert!(!engine.is_null());
        ferric_engine_free(engine);
    }
}

#[test]
fn engine_new_with_custom_config_returns_non_null() {
    unsafe {
        let config = FerricConfig {
            string_encoding: FerricStringEncoding::Ascii.as_raw(),
            strategy: FerricConflictStrategy::Breadth.as_raw(),
            max_call_depth: 32,
        };
        let engine = ferric_engine_new_with_config(&config);
        assert!(!engine.is_null());
        ferric_engine_free(engine);
    }
}

#[test]
fn engine_new_with_invalid_config_returns_null() {
    unsafe {
        let config = FerricConfig {
            string_encoding: 999,
            strategy: FerricConflictStrategy::Depth.as_raw(),
            max_call_depth: 32,
        };
        let engine = ferric_engine_new_with_config(&config);
        assert!(engine.is_null());
        let err = ferric_last_error_global();
        assert!(!err.is_null(), "global error should be populated");
        let msg = std::ffi::CStr::from_ptr(err).to_string_lossy();
        assert!(
            msg.contains("invalid string_encoding"),
            "error should explain invalid discriminant, got: {msg}"
        );
    }
}

#[test]
fn engine_free_null_is_ok() {
    unsafe {
        let result = ferric_engine_free(std::ptr::null_mut());
        assert_eq!(result, FerricError::Ok);
    }
}

#[test]
fn engine_free_returns_ok() {
    unsafe {
        let engine = ferric_engine_new();
        let result = ferric_engine_free(engine);
        assert_eq!(result, FerricError::Ok);
    }
}

#[test]
fn engine_load_valid_source() {
    unsafe {
        let engine = ferric_engine_new();
        let source =
            std::ffi::CString::new(r#"(defrule test (initial-fact) => (printout t "ok" crlf))"#)
                .unwrap();
        let result = ferric_engine_load_string(engine, source.as_ptr());
        assert_eq!(result, FerricError::Ok);
        ferric_engine_free(engine);
    }
}

#[test]
fn engine_load_invalid_source() {
    unsafe {
        let engine = ferric_engine_new();
        let source = std::ffi::CString::new("(defrule bad (this is not closed").unwrap();
        let result = ferric_engine_load_string(engine, source.as_ptr());
        assert_ne!(result, FerricError::Ok);
        // Error should be stored in global channel
        let err = ferric_last_error_global();
        assert!(!err.is_null());
        // ...and mirrored in the per-engine channel.
        let engine_err = ferric_engine_last_error(engine);
        assert!(!engine_err.is_null());
        ferric_engine_free(engine);
    }
}

#[test]
fn engine_load_null_engine() {
    unsafe {
        let source = std::ffi::CString::new("(assert (foo))").unwrap();
        let result = ferric_engine_load_string(std::ptr::null_mut(), source.as_ptr());
        assert_eq!(result, FerricError::NullPointer);
    }
}

#[test]
fn engine_load_null_source() {
    unsafe {
        let engine = ferric_engine_new();
        let result = ferric_engine_load_string(engine, std::ptr::null());
        assert_eq!(result, FerricError::NullPointer);
        ferric_engine_free(engine);
    }
}

#[test]
fn engine_reset_works() {
    unsafe {
        let engine = ferric_engine_new();
        let result = ferric_engine_reset(engine);
        assert_eq!(result, FerricError::Ok);
        ferric_engine_free(engine);
    }
}

#[test]
fn engine_per_error_starts_empty() {
    unsafe {
        let engine = ferric_engine_new();
        let err_ptr = ferric_engine_last_error(engine);
        assert!(err_ptr.is_null());
        ferric_engine_free(engine);
    }
}

#[test]
fn engine_clear_error_null_is_null_pointer() {
    unsafe {
        let result = ferric_engine_clear_error(std::ptr::null_mut());
        assert_eq!(result, FerricError::NullPointer);
    }
}

#[test]
fn engine_last_error_null_is_null() {
    unsafe {
        let ptr = ferric_engine_last_error(std::ptr::null());
        assert!(ptr.is_null());
    }
}

#[test]
fn thread_affinity_passes_on_creating_thread() {
    // Verify that the engine's thread affinity check succeeds when
    // called from the creating thread.
    unsafe {
        let engine = ferric_engine_new();
        // All operations should succeed on the creating thread
        let result = ferric_engine_reset(engine);
        assert_eq!(result, FerricError::Ok);
        ferric_engine_free(engine);
    }
}

#[test]
fn thread_violation_error_mapping() {
    // Test that EngineError::WrongThread maps to FerricError::ThreadViolation.
    // We can't actually cross threads with Engine (it's !Send), but we can
    // verify the error mapping path works correctly.
    use crate::error::map_engine_error;
    use ferric_runtime::engine::EngineError;

    let err = EngineError::WrongThread {
        creator: std::thread::current().id(),
        current: std::thread::current().id(),
    };
    assert_eq!(map_engine_error(&err), FerricError::ThreadViolation);
}

#[test]
fn thread_violation_from_other_thread_via_raw_pointer() {
    // Create an engine, convert to raw pointer, and attempt to use it
    // from another thread. Since Engine is !Send, we use raw pointers
    // (which are Send) to test the FFI's thread-affinity enforcement.
    // Thread violations always return FerricError::ThreadViolation.
    unsafe {
        let engine = ferric_engine_new();
        let engine_addr = engine as usize; // usize is Send

        let result = std::thread::spawn(move || {
            let engine = engine_addr as *mut crate::engine::FerricEngine;
            ferric_engine_reset(engine)
        })
        .join()
        .unwrap();

        assert_eq!(result, FerricError::ThreadViolation);

        // Free from the correct (creating) thread
        ferric_engine_free(engine);
    }
}

#[test]
fn clear_error_rejects_other_thread_before_mutation() {
    unsafe {
        let engine = ferric_engine_new();
        (*engine).error_state.set("sticky error".to_string());

        let engine_addr = engine as usize;
        let result = std::thread::spawn(move || {
            let engine = engine_addr as *mut crate::engine::FerricEngine;
            ferric_engine_clear_error(engine)
        })
        .join()
        .unwrap();

        assert_eq!(result, FerricError::ThreadViolation);

        let err_ptr = ferric_engine_last_error(engine);
        assert!(
            !err_ptr.is_null(),
            "error should remain set after violation"
        );

        ferric_engine_free(engine);
    }
}

#[test]
fn multiple_engines_independent() {
    unsafe {
        let e1 = ferric_engine_new();
        let e2 = ferric_engine_new();
        assert!(!e1.is_null());
        assert!(!e2.is_null());
        assert_ne!(e1, e2);

        // Load into one, other unaffected
        let src = std::ffi::CString::new(r#"(defrule r1 (initial-fact) => (printout t "1" crlf))"#)
            .unwrap();
        assert_eq!(ferric_engine_load_string(e1, src.as_ptr()), FerricError::Ok);

        ferric_engine_free(e1);
        ferric_engine_free(e2);
    }
}

#[test]
fn engine_last_error_pointer_storage_is_per_engine() {
    unsafe {
        let e1 = ferric_engine_new();
        let e2 = ferric_engine_new();
        (*e1).error_state.set("engine one".to_string());
        (*e2).error_state.set("engine two".to_string());

        let p1 = ferric_engine_last_error(e1);
        assert!(!p1.is_null());
        assert_eq!(std::ffi::CStr::from_ptr(p1).to_str().unwrap(), "engine one");

        let p2 = ferric_engine_last_error(e2);
        assert!(!p2.is_null());
        assert_eq!(std::ffi::CStr::from_ptr(p2).to_str().unwrap(), "engine two");

        // The first pointer should still read engine one's message after querying engine two.
        assert_eq!(std::ffi::CStr::from_ptr(p1).to_str().unwrap(), "engine one");

        ferric_engine_free(e1);
        ferric_engine_free(e2);
    }
}
