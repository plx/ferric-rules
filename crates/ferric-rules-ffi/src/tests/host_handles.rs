//! The numeric C ABI uses engine-local opaque host handles, not raw arena IDs.
use std::ptr;

use crate::engine::{
    ferric_engine_assert_ordered, ferric_engine_fact_count, ferric_engine_free, ferric_engine_new,
    ferric_engine_reset, ferric_engine_retract,
};
use crate::error::FerricError;
use crate::types::{
    ferric_value_free, ferric_value_integer, ferric_value_multifield_copy, ferric_value_void,
};

#[test]
fn c_fact_ids_are_lossless_local_and_stale_after_reset() {
    unsafe {
        let a = ferric_engine_new();
        let b = ferric_engine_new();
        let mut first = 0;
        let mut second = 0;
        assert_eq!(
            ferric_engine_assert_ordered(a, b"item\0".as_ptr().cast(), ptr::null(), 0, &mut first),
            FerricError::Ok
        );
        assert_eq!(
            ferric_engine_assert_ordered(b, b"item\0".as_ptr().cast(), ptr::null(), 0, &mut second),
            FerricError::Ok
        );
        assert!(first > (1_u64 << 53));
        assert_ne!(first, second);
        assert_eq!(ferric_engine_retract(b, first), FerricError::NotFound);
        let mut count = 0;
        assert_eq!(ferric_engine_fact_count(b, &mut count), FerricError::Ok);
        assert_eq!(count, 1);
        assert_eq!(ferric_engine_reset(a), FerricError::Ok);
        assert_eq!(ferric_engine_retract(a, first), FerricError::NotFound);
        assert_eq!(ferric_engine_retract(b, second), FerricError::Ok);
        ferric_engine_free(a);
        ferric_engine_free(b);
    }
}

#[test]
fn c_input_limits_reject_before_allocating_or_installing_facts() {
    unsafe {
        let engine = ferric_engine_new();
        let value = ferric_value_integer(1);
        let mut id = 0;
        assert_eq!(
            ferric_engine_assert_ordered(
                engine,
                b"large\0".as_ptr().cast(),
                &value,
                usize::MAX,
                &mut id
            ),
            FerricError::InvalidArgument
        );
        let mut nested = value;
        for _ in 0..33 {
            let mut outer = ferric_value_void();
            assert_eq!(
                ferric_value_multifield_copy(&nested, 1, &mut outer),
                FerricError::Ok
            );
            ferric_value_free(&mut nested);
            nested = outer;
        }
        assert_eq!(
            ferric_engine_assert_ordered(engine, b"deep\0".as_ptr().cast(), &nested, 1, &mut id),
            FerricError::InvalidArgument
        );
        ferric_value_free(&mut nested);
        let mut count = 99;
        assert_eq!(
            ferric_engine_fact_count(engine, &mut count),
            FerricError::Ok
        );
        assert_eq!(count, 0);
        assert_eq!(
            ferric_engine_assert_ordered(
                engine,
                b"valid\0".as_ptr().cast(),
                ptr::null(),
                0,
                &mut id
            ),
            FerricError::Ok
        );
        assert_eq!(ferric_engine_retract(engine, id), FerricError::Ok);
        ferric_engine_free(engine);
    }
}
