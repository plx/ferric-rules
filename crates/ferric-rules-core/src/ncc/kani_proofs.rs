//! Kani proof harnesses for `ncc` (see docs/verification.md).
//!
//! What is proved, on the production code:
//!
//! * `ncc_empty_memory_removals_are_noops`: on a fresh memory, for every
//!   chronology argument, `remove_result` and `remove_result_support` return
//!   `None`, `decrement_results` returns 0 without underflowing,
//!   `reassign_primary` reports an unblocked parent, `remove_unblocked` and
//!   `remove_parent_token` are no-ops, and `validate_consistency` holds.
//! * `ncc_count_without_results_is_rejected`: a count recorded by
//!   `increment_results` without a tracked result makes the parent blocked and
//!   is rejected by `validate_consistency`.
//!
//! Bounds: concrete token identities; at most one hash-table insertion per
//! harness.
//!
//! Known gaps: `add_result`/`remove_result` on a populated memory are not
//! covered here. A harness for one `add_result` (five hash-table insertions),
//! a repeated `add_result`, and `remove_result` on concrete tokens with
//! symbolic chronology did not finish within 20 minutes, almost all of it in
//! symbolic execution of the hash tables (see negative/kani_proofs.rs). Counts (including absence of overflow and
//! underflow), the block/unblock transition signals, primary-result
//! chronology, invariant preservation and parent cleanup are proved without
//! bounds on a model of this module in verification/verus/ncc_memory.rs.

use super::*;

fn tok(n: u32) -> TokenId {
    TokenId::from(slotmap::KeyData::from_ffi(u64::from(n)))
}

/// Removals on a fresh memory change nothing and never underflow, for any
/// chronology argument.
#[kani::proof]
#[kani::unwind(5)]
fn ncc_empty_memory_removals_are_noops() {
    let mut mem = NccMemory::new(NccMemoryId(0));
    let order: u64 = kani::any();
    assert_eq!(mem.remove_result(tok(1), order), None);
    assert_eq!(mem.remove_result_support(tok(1)), None);
    assert_eq!(mem.decrement_results(tok(2)), 0);
    assert!(!mem.reassign_primary(tok(2), order));
    assert_eq!(mem.remove_unblocked(tok(2)), None);
    mem.remove_parent_token(tok(2));
    assert_eq!(mem.result_count(tok(2)), 0);
    assert!(!mem.is_blocked(tok(2)));
    assert!(mem.is_empty());
    assert!(mem.validate_consistency().is_ok());
}

/// A bare count without a tracked result is rejected by
/// `validate_consistency`; this is why `increment_results` is only used
/// through `add_result`.
#[kani::proof]
#[kani::unwind(5)]
fn ncc_count_without_results_is_rejected() {
    let mut mem = NccMemory::new(NccMemoryId(0));
    assert_eq!(mem.increment_results(tok(2)), 1);
    assert!(mem.is_blocked(tok(2)));
    assert!(mem.validate_consistency().is_err());
}
