//! C ABI consumer costs: complete lifecycle and owned typed reads/output copies.
//! All state assertions run before timing and reuse the measured operations.

use std::ffi::{CStr, CString};
use std::fmt::Write;
use std::hint::black_box;
use std::ptr::{self, NonNull};

use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion};
use ferric_rules_ffi::engine::{
    ferric_engine_fact_count, ferric_engine_find_fact_ids, ferric_engine_free,
    ferric_engine_get_fact_field, ferric_engine_get_output_copy, ferric_engine_last_error_copy,
    ferric_engine_load_string, ferric_engine_new, ferric_engine_reset, ferric_engine_retract,
    ferric_engine_run_ex, FerricEngine,
};
use ferric_rules_ffi::error::FerricError;
use ferric_rules_ffi::types::{ferric_value_free, FerricHaltReason, FerricValue, FerricValueType};

// Black-box function POINTERS (not zero-sized function items) keep release LTO
// from inlining away the C ABI boundary which an ordinary C/Swift host crosses.
macro_rules! call {
    ($function:path, $signature:ty $(, $argument:expr)* $(,)?) => {{
        // SAFETY: Call sites provide live, exclusively owned handles and valid
        // input/output buffers. None of this benchmark's handles cross threads.
        unsafe {
            let function: $signature = $function;
            black_box(function)($($argument),*)
        }
    }};
}

fn check(code: FerricError) {
    assert_eq!(code, FerricError::Ok, "C ABI operation failed");
}

/// Owns one allocation, with one successful free on its creating thread.
/// This ownership wrapper is identical for the confined base and candidate.
struct Handle(NonNull<FerricEngine>);

impl Handle {
    fn new() -> Self {
        Self(NonNull::new(call!(ferric_engine_new, unsafe extern "C" fn() -> _)).unwrap())
    }

    fn load_reset_run(source: &CStr) -> (Self, u64, FerricHaltReason) {
        let engine = Self::new();
        check(call!(
            ferric_engine_load_string,
            unsafe extern "C" fn(_, _) -> _,
            engine.0.as_ptr(),
            source.as_ptr()
        ));
        check(call!(
            ferric_engine_reset,
            unsafe extern "C" fn(_) -> _,
            engine.0.as_ptr()
        ));
        let mut fired = 0;
        let mut reason = FerricHaltReason::LimitReached;
        check(call!(
            ferric_engine_run_ex,
            unsafe extern "C" fn(_, _, _, _) -> _,
            engine.0.as_ptr(),
            -1,
            ptr::addr_of_mut!(fired),
            ptr::addr_of_mut!(reason)
        ));
        (engine, fired, reason)
    }

    fn output(&self) -> Vec<u8> {
        let mut length = 0;
        check(call!(
            ferric_engine_get_output_copy,
            unsafe extern "C" fn(_, _, _, _, _) -> _,
            self.0.as_ptr().cast_const(),
            b"t\0".as_ptr().cast(),
            ptr::null_mut(),
            0,
            ptr::addr_of_mut!(length)
        ));
        let mut bytes = vec![0; length];
        check(call!(
            ferric_engine_get_output_copy,
            unsafe extern "C" fn(_, _, _, _, _) -> _,
            self.0.as_ptr().cast_const(),
            b"t\0".as_ptr().cast(),
            bytes.as_mut_ptr().cast(),
            bytes.len(),
            ptr::addr_of_mut!(length)
        ));
        bytes
    }

    fn read(&self) -> Observation {
        let mut fact_count = 0;
        check(call!(
            ferric_engine_fact_count,
            unsafe extern "C" fn(_, _) -> _,
            self.0.as_ptr().cast_const(),
            ptr::addr_of_mut!(fact_count)
        ));
        let mut selected_count = 0;
        check(call!(
            ferric_engine_find_fact_ids,
            unsafe extern "C" fn(_, _, _, _, _) -> _,
            self.0.as_ptr().cast_const(),
            b"selected\0".as_ptr().cast(),
            ptr::null_mut(),
            0,
            ptr::addr_of_mut!(selected_count)
        ));
        let mut ids = vec![0; selected_count];
        check(call!(
            ferric_engine_find_fact_ids,
            unsafe extern "C" fn(_, _, _, _, _) -> _,
            self.0.as_ptr().cast_const(),
            b"selected\0".as_ptr().cast(),
            ids.as_mut_ptr(),
            ids.len(),
            ptr::addr_of_mut!(selected_count)
        ));
        let selected = ids.into_iter().map(|id| self.fields(id)).collect();
        Observation {
            fact_count,
            selected,
            output: self.output(),
        }
    }

    fn fields(&self, id: u64) -> (i64, String) {
        let mut number = FerricValue::void();
        check(call!(
            ferric_engine_get_fact_field,
            unsafe extern "C" fn(_, _, _, _) -> _,
            self.0.as_ptr().cast_const(),
            id,
            0,
            ptr::addr_of_mut!(number)
        ));
        let integer = number.integer;
        let number_type = number.value_type;
        check(call!(
            ferric_value_free,
            unsafe extern "C" fn(_) -> _,
            ptr::addr_of_mut!(number)
        ));
        assert_eq!(number_type, FerricValueType::Integer.as_raw());

        let mut label = FerricValue::void();
        check(call!(
            ferric_engine_get_fact_field,
            unsafe extern "C" fn(_, _, _, _) -> _,
            self.0.as_ptr().cast_const(),
            id,
            1,
            ptr::addr_of_mut!(label)
        ));
        // Check the tag before interpreting its active pointer field, just as
        // a typed host wrapper must. This is decoding, not the workload oracle.
        assert_eq!(label.value_type, FerricValueType::String.as_raw());
        // SAFETY: successful String egress owns a non-null NUL-terminated value.
        let text = unsafe { CStr::from_ptr(label.string_ptr) }
            .to_str()
            .unwrap()
            .to_owned();
        check(call!(
            ferric_value_free,
            unsafe extern "C" fn(_) -> _,
            ptr::addr_of_mut!(label)
        ));
        (integer, text)
    }
}

impl Drop for Handle {
    fn drop(&mut self) {
        check(call!(
            ferric_engine_free,
            unsafe extern "C" fn(_) -> _,
            self.0.as_ptr()
        ));
    }
}

struct Observation {
    fact_count: usize,
    selected: Vec<(i64, String)>,
    output: Vec<u8>,
}

fn source(items: usize) -> CString {
    let mut source = String::from("(deffacts candidates ");
    for id in 0..items {
        write!(source, "(candidate {id} \"label-{id}\") ").unwrap();
    }
    source.push_str(") (defrule select (candidate ?id ?label) => (assert (selected ?id ?label)) (printout t ?label crlf))");
    CString::new(source).unwrap()
}

fn lifecycle(source: &CStr) -> (u64, FerricHaltReason, Observation) {
    let (engine, fired, reason) = Handle::load_reset_run(source);
    (fired, reason, engine.read())
    // The native allocation is freed before returning the owned observation.
}

fn verify(items: usize, fired: u64, reason: FerricHaltReason, mut state: Observation) {
    assert_eq!(fired, u64::try_from(items).unwrap());
    assert_eq!(reason, FerricHaltReason::AgendaEmpty);
    assert_eq!(state.fact_count, 2 * items);
    state.selected.sort();
    let expected: Vec<_> = (0..items)
        .map(|id| (i64::try_from(id).unwrap(), format!("label-{id}")))
        .collect();
    assert_eq!(state.selected, expected);
    assert_eq!(state.output.last(), Some(&0));
    let output = std::str::from_utf8(&state.output[..state.output.len() - 1]).unwrap();
    let mut lines: Vec<_> = output.lines().collect();
    lines.sort_unstable();
    let mut labels: Vec<_> = expected.iter().map(|(_, label)| label.as_str()).collect();
    labels.sort_unstable();
    assert_eq!(
        lines, labels,
        "every selected label must be copied exactly once"
    );
}

fn verify_error_copy(engine: &Handle) {
    assert_eq!(
        call!(
            ferric_engine_retract,
            unsafe extern "C" fn(_, _) -> _,
            engine.0.as_ptr(),
            u64::MAX
        ),
        FerricError::NotFound
    );
    let mut length = 0;
    check(call!(
        ferric_engine_last_error_copy,
        unsafe extern "C" fn(_, _, _, _) -> _,
        engine.0.as_ptr().cast_const(),
        ptr::null_mut(),
        0,
        ptr::addr_of_mut!(length)
    ));
    let mut bytes = vec![0_u8; length];
    check(call!(
        ferric_engine_last_error_copy,
        unsafe extern "C" fn(_, _, _, _) -> _,
        engine.0.as_ptr().cast_const(),
        bytes.as_mut_ptr().cast(),
        bytes.len(),
        ptr::addr_of_mut!(length)
    ));
    let message = CStr::from_bytes_with_nul(&bytes).unwrap().to_str().unwrap();
    assert!(
        message.contains("fact"),
        "missing-fact diagnostic lost: {message}"
    );
    assert!(
        message.contains("not found"),
        "wrong error diagnostic: {message}"
    );
}

fn capi(c: &mut Criterion) {
    let mut group = c.benchmark_group("capi");
    for items in [100, 1_000] {
        let source = source(items);
        group.bench_with_input(
            BenchmarkId::new("lifecycle", items),
            &source,
            |b, source| {
                let (fired, reason, state) = lifecycle(source);
                verify(items, fired, reason, state);
                b.iter(|| black_box(lifecycle(black_box(source))));
            },
        );
        group.bench_with_input(
            BenchmarkId::new("read_output", items),
            &source,
            |b, source| {
                let (engine, fired, reason) = Handle::load_reset_run(source);
                verify(items, fired, reason, engine.read());
                verify_error_copy(&engine);
                b.iter(|| black_box(engine.read()));
            },
        );
    }
    group.finish();
}

criterion_group!(benches, capi);
criterion_main!(benches);
