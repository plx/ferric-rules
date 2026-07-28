//! Concurrency and snapshot-ordering regressions for raw-engine diagnostics.

use crate::engine::{
    ferric_engine_action_diagnostic_count, ferric_engine_clear_error, ferric_engine_free,
    ferric_engine_last_error, ferric_engine_last_error_copy, ferric_engine_new,
    ferric_engine_retract, FerricEngine,
};
#[cfg(feature = "serde")]
use crate::engine::{ferric_engine_reset, ferric_engine_serialize_bincode};
use crate::error::FerricError;
use std::ffi::CStr;
use std::os::raw::c_char;
use std::sync::{mpsc, Arc, Barrier};

const STRESS_ROUNDS: usize = 10_000;

unsafe fn copy_snapshot(engine: *const FerricEngine) -> Result<String, FerricError> {
    let mut buffer = [0_u8; 512];
    let mut written = 0;
    let result = ferric_engine_last_error_copy(
        engine,
        buffer.as_mut_ptr().cast::<c_char>(),
        buffer.len(),
        &mut written,
    );
    if result != FerricError::Ok {
        return Err(result);
    }
    assert!(written > 1);
    assert!(written <= buffer.len());
    assert_eq!(buffer[written - 1], 0);
    Ok(CStr::from_ptr(buffer.as_ptr().cast::<c_char>())
        .to_str()
        .unwrap()
        .to_string())
}

unsafe fn publish_missing_fact(engine: *mut FerricEngine, fact_id: u64) -> String {
    assert_eq!(
        ferric_engine_retract(engine, fact_id),
        FerricError::NotFound
    );
    copy_snapshot(engine).unwrap()
}

unsafe fn validate_stress_snapshot(
    engine: *const FerricEngine,
    first: &str,
    second: &str,
) -> Result<(), String> {
    let mut buffer = [0_u8; 512];
    let mut written = 0;
    let result = ferric_engine_last_error_copy(
        engine,
        buffer.as_mut_ptr().cast::<c_char>(),
        buffer.len(),
        &mut written,
    );
    if result != FerricError::Ok {
        return Err(format!("diagnostic copy returned {result:?}"));
    }
    if written < 2 || written > buffer.len() || buffer[written - 1] != 0 {
        return Err(format!(
            "diagnostic copy returned invalid length or NUL: {written}"
        ));
    }
    let message = &buffer[..written - 1];
    let is_thread_violation = std::str::from_utf8(message)
        .is_ok_and(|text| text.starts_with("engine called from wrong thread"));
    if message != first.as_bytes() && message != second.as_bytes() && !is_thread_violation {
        return Err(format!(
            "reader observed a torn or stale snapshot: {:?}",
            String::from_utf8_lossy(message)
        ));
    }
    Ok(())
}

#[test]
fn ordered_snapshots_observe_published_transitions() {
    enum Command {
        Read,
        Stop,
    }

    unsafe {
        let engine = ferric_engine_new();
        let first = publish_missing_fact(engine, u64::MAX);
        let second = publish_missing_fact(engine, u64::MAX - 1);
        assert_ne!(first, second);

        let engine_addr = engine as usize;
        let (command_tx, command_rx) = mpsc::channel();
        let (snapshot_tx, snapshot_rx) = mpsc::channel();
        let reader = std::thread::spawn(move || {
            let engine = engine_addr as *const FerricEngine;
            while let Ok(command) = command_rx.recv() {
                match command {
                    Command::Read => snapshot_tx.send(copy_snapshot(engine)).unwrap(),
                    Command::Stop => break,
                }
            }
        });

        assert_eq!(
            ferric_engine_retract(engine, u64::MAX),
            FerricError::NotFound
        );
        command_tx.send(Command::Read).unwrap();
        assert_eq!(snapshot_rx.recv().unwrap().unwrap(), first);

        assert_eq!(
            ferric_engine_retract(engine, u64::MAX - 1),
            FerricError::NotFound
        );
        command_tx.send(Command::Read).unwrap();
        assert_eq!(snapshot_rx.recv().unwrap().unwrap(), second);

        assert_eq!(ferric_engine_clear_error(engine), FerricError::Ok);
        command_tx.send(Command::Read).unwrap();
        assert_eq!(snapshot_rx.recv().unwrap(), Err(FerricError::NotFound));

        assert_eq!(
            ferric_engine_retract(engine, u64::MAX),
            FerricError::NotFound
        );
        command_tx.send(Command::Read).unwrap();
        assert_eq!(snapshot_rx.recv().unwrap().unwrap(), first);

        command_tx.send(Command::Stop).unwrap();
        reader.join().unwrap();
        assert_eq!(ferric_engine_free(engine), FerricError::Ok);
    }
}

#[test]
fn copy_is_coherent_during_10_000_owner_mutations() {
    unsafe {
        let engine = ferric_engine_new();
        let first = publish_missing_fact(engine, u64::MAX);
        let second = publish_missing_fact(engine, u64::MAX - 1);
        assert_ne!(first, second);
        assert_eq!(
            ferric_engine_retract(engine, u64::MAX),
            FerricError::NotFound
        );

        let barrier = Arc::new(Barrier::new(2));
        let reader_barrier = Arc::clone(&barrier);
        let engine_addr = engine as usize;
        let expected_first = first.clone();
        let expected_second = second.clone();
        let reader = std::thread::spawn(move || {
            let engine = engine_addr as *const FerricEngine;
            let mut failure = None;
            for round in 0..STRESS_ROUNDS {
                reader_barrier.wait();

                if failure.is_none() {
                    failure =
                        validate_stress_snapshot(engine, &expected_first, &expected_second).err();
                }
                if failure.is_none() && round % 257 == 0 {
                    let mut count = usize::MAX;
                    let result = ferric_engine_action_diagnostic_count(engine, &mut count);
                    if result != FerricError::ThreadViolation || count != usize::MAX {
                        failure = Some(format!(
                            "action diagnostics lost affinity: result={result:?}, count={count}"
                        ));
                    }
                }
                reader_barrier.wait();
            }
            failure
        });

        let mut owner_failure = None;
        for round in 0..STRESS_ROUNDS {
            barrier.wait();
            let fact_id = if round % 2 == 0 {
                u64::MAX - 1
            } else {
                u64::MAX
            };
            let result = ferric_engine_retract(engine, fact_id);
            if owner_failure.is_none() && result != FerricError::NotFound {
                owner_failure = Some(format!(
                    "owner mutation returned {result:?} at round {round}"
                ));
            }
            barrier.wait();
        }

        let reader_failure = reader.join().unwrap();
        assert!(owner_failure.is_none(), "{owner_failure:?}");
        assert!(reader_failure.is_none(), "{reader_failure:?}");
        assert_eq!(ferric_engine_free(engine), FerricError::Ok);
    }
}

#[test]
fn size_query_and_copy_are_independent_coherent_snapshots() {
    unsafe {
        let engine = ferric_engine_new();
        let short = "short";
        let long = "a much longer replacement diagnostic";
        (*engine).set_error_for_test(short.to_string());

        let mut short_size = 0;
        assert_eq!(
            ferric_engine_last_error_copy(engine, std::ptr::null_mut(), 0, &mut short_size),
            FerricError::Ok
        );
        assert_eq!(short_size, short.len() + 1);

        (*engine).set_error_for_test(long.to_string());
        let mut small_buffer = vec![0_u8; short_size];
        let mut required = 0;
        assert_eq!(
            ferric_engine_last_error_copy(
                engine,
                small_buffer.as_mut_ptr().cast::<c_char>(),
                small_buffer.len(),
                &mut required,
            ),
            FerricError::BufferTooSmall
        );
        assert_eq!(required, long.len() + 1);
        assert_eq!(small_buffer[small_buffer.len() - 1], 0);
        assert_eq!(
            &small_buffer[..small_buffer.len() - 1],
            &long.as_bytes()[..small_buffer.len() - 1]
        );

        let mut full_buffer = vec![0_u8; required];
        let mut written = 0;
        assert_eq!(
            ferric_engine_last_error_copy(
                engine,
                full_buffer.as_mut_ptr().cast::<c_char>(),
                full_buffer.len(),
                &mut written,
            ),
            FerricError::Ok
        );
        assert_eq!(written, required);
        assert_eq!(&full_buffer[..long.len()], long.as_bytes());
        assert_eq!(full_buffer[long.len()], 0);

        assert_eq!(ferric_engine_free(engine), FerricError::Ok);
    }
}

#[test]
fn borrowed_pointer_survives_error_replacement_until_next_borrowed_read() {
    unsafe {
        let engine = ferric_engine_new();
        (*engine).set_error_for_test("first snapshot".to_string());
        let first = ferric_engine_last_error(engine);
        assert!(!first.is_null());

        (*engine).set_error_for_test("second snapshot".to_string());
        assert_eq!(
            copy_snapshot(engine).unwrap(),
            "second snapshot",
            "owned copies must see the current snapshot"
        );
        assert_eq!(
            CStr::from_ptr(first).to_str().unwrap(),
            "first snapshot",
            "writers and owned copies must not invalidate the borrowed cache"
        );

        let second = ferric_engine_last_error(engine);
        assert!(!second.is_null());
        assert_eq!(CStr::from_ptr(second).to_str().unwrap(), "second snapshot");

        assert_eq!(ferric_engine_free(engine), FerricError::Ok);
    }
}

#[cfg(feature = "serde")]
struct ReentrantAllocatorContext {
    engine: *mut FerricEngine,
    storage: Vec<u8>,
    diagnostic_result: FerricError,
    diagnostic: String,
    borrowed_diagnostic: String,
    ordinary_call_result: FerricError,
}

#[cfg(feature = "serde")]
unsafe extern "C" fn reentrant_allocator(size: usize, context: *mut std::ffi::c_void) -> *mut u8 {
    let context = &mut *context.cast::<ReentrantAllocatorContext>();

    context.diagnostic_result = match copy_snapshot(context.engine) {
        Ok(message) => {
            context.diagnostic = message;
            FerricError::Ok
        }
        Err(code) => code,
    };
    let borrowed = ferric_engine_last_error(context.engine);
    if !borrowed.is_null() {
        context.borrowed_diagnostic = CStr::from_ptr(borrowed).to_string_lossy().into_owned();
    }
    context.ordinary_call_result = ferric_engine_reset(context.engine);
    context.storage.resize(size, 0);
    context.storage.as_mut_ptr()
}

#[cfg(feature = "serde")]
#[test]
fn allocator_callback_can_reenter_diagnostics_but_not_runtime() {
    unsafe {
        let engine = ferric_engine_new();
        (*engine).set_error_for_test("callback snapshot".to_string());
        let mut context = ReentrantAllocatorContext {
            engine,
            storage: Vec::new(),
            diagnostic_result: FerricError::InternalError,
            diagnostic: String::new(),
            borrowed_diagnostic: String::new(),
            ordinary_call_result: FerricError::Ok,
        };
        let mut data = std::ptr::null_mut();
        let mut len = 0;

        assert_eq!(
            ferric_engine_serialize_bincode(
                engine,
                Some(reentrant_allocator),
                std::ptr::addr_of_mut!(context).cast::<std::ffi::c_void>(),
                &mut data,
                &mut len,
            ),
            FerricError::Ok
        );
        assert_eq!(context.diagnostic_result, FerricError::Ok);
        assert_eq!(context.diagnostic, "callback snapshot");
        assert_eq!(context.borrowed_diagnostic, "callback snapshot");
        assert_eq!(
            context.ordinary_call_result,
            FerricError::InternalError,
            "same-engine runtime reentry must fail deterministically"
        );
        assert_eq!(data, context.storage.as_mut_ptr());
        assert_eq!(len, context.storage.len());

        assert_eq!(ferric_engine_free(engine), FerricError::Ok);
    }
}
