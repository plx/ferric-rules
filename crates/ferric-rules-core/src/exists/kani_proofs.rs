//! Kani proof harnesses for `exists` (see docs/verification.md).
//!
//! What is proved, on the production code:
//!
//! * `exists_empty_memory_removals_are_noops`: on a fresh memory,
//!   `remove_support` returns `(0, false)` (no N->0 transition is reported),
//!   `remove_satisfied` returns `None`, `remove_parent_token` is a no-op, all
//!   queries report no support, and `validate_consistency` holds.
//!
//! Bounds: concrete token and fact identities; no hash-table insertion.
//!
//! Known gaps: `add_support`/`remove_support` on a populated memory are not
//! covered here: each performs at least three hash-table insertions, and
//! harnesses with several insertions did not finish within 20 minutes (see
//! negative/kani_proofs.rs). Two one-insertion harnesses were dropped because
//! CBMC ran out of memory (7 GB limit, shared machine) during symbolic
//! execution: `validate_consistency` rejecting a satisfied parent without
//! support (after 496 s, in the error-message formatting) and
//! `set_satisfied` followed by `remove_parent_token` (after 1147 s). Counts, the 0->N / N->0 transition signals, invariant
//! preservation and parent cleanup are proved without bounds on a model of
//! this module in verification/verus/exists_memory.rs.

use super::*;

fn tok(n: u32) -> TokenId {
    TokenId::from(slotmap::KeyData::from_ffi(u64::from(n)))
}

fn fact(n: u32) -> FactId {
    FactId::from(slotmap::KeyData::from_ffi(u64::from(n)))
}

/// Removals and queries on a fresh memory report zero support and change
/// nothing.
#[kani::proof]
#[kani::unwind(5)]
fn exists_empty_memory_removals_are_noops() {
    let mut mem = ExistsMemory::new(ExistsMemoryId(0));
    assert_eq!(mem.remove_support(tok(1), fact(1)), (0, false));
    assert_eq!(mem.remove_satisfied(tok(1)), None);
    mem.remove_parent_token(tok(1));
    assert_eq!(mem.support_count(tok(1)), 0);
    assert!(mem.parents_supported_by(fact(1)).is_empty());
    assert!(!mem.is_satisfied(tok(1)));
    assert!(mem.is_empty());
    assert!(mem.validate_consistency().is_ok());
}
