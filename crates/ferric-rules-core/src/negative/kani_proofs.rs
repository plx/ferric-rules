//! Kani proof harnesses for `negative` (see docs/verification.md).
//!
//! What is proved, on the production code:
//!
//! * `negative_empty_memory_removals_are_noops`: on a fresh memory,
//!   `remove_blocker` (for every `replacement_order`) reports "not unblocked",
//!   `remove_unblocked` returns `None`, `remove_parent_token` is a no-op, all
//!   queries report an untracked parent, and `validate_consistency` holds.
//! * `negative_passthrough_lifecycle`: after `set_unblocked(p, pt)` the memory
//!   is consistent and reports `pt` for `p`; both removal paths
//!   (`remove_unblocked`, `remove_parent_token`) remove all bookkeeping of `p`
//!   and leave a consistent, empty memory.
//!
//! Bounds: concrete token and fact identities; at most one hash-table
//! insertion per harness.
//!
//! Known gaps: the blocker operations (`add_blocker` and `remove_blocker` on a
//! populated memory) are not covered here. A single concrete `add_blocker`
//! performs four hash-table insertions (three of them allocate a table, one
//! inside a map value); symbolic execution of it alone did not finish within
//! 20 minutes. `negative_passthrough_lifecycle` (one insertion, then a
//! symbolic choice of removal path) took 655 s, 496 s of it in symbolic
//! execution and under 15 s in the SAT solver. A review variant that built a
//! parent with two blockers directly from the fields, using maps and sets with
//! spare capacity so that no insertion resizes, and then removed both
//! blockers in a symbolic order (no validation of the populated states) did
//! not finish within 20 minutes either.
//! Invariant preservation, the blocked/unblocked transition signal,
//! chronology and parent cleanup are instead proved without bounds on a model
//! of this module in verification/verus/negative_memory.rs.

use super::*;

fn tok(n: u32) -> TokenId {
    TokenId::from(slotmap::KeyData::from_ffi(u64::from(n)))
}

fn fact(n: u32) -> FactId {
    FactId::from(slotmap::KeyData::from_ffi(u64::from(n)))
}

/// Every query and removal on a fresh memory is a no-op that reports "not
/// blocked", for any chronology argument.
#[kani::proof]
#[kani::unwind(5)]
fn negative_empty_memory_removals_are_noops() {
    let mut mem = NegativeMemory::new(NegativeMemoryId(0));
    let order: u64 = kani::any();
    assert!(!mem.remove_blocker(tok(1), fact(1), order));
    assert_eq!(mem.remove_unblocked(tok(1)), None);
    mem.remove_parent_token(tok(1));
    assert!(mem.is_empty());
    assert!(!mem.is_blocked(tok(1)));
    assert!(!mem.is_unblocked(tok(1)));
    assert!(mem.tokens_blocked_by(fact(1)).is_empty());
    assert_eq!(mem.primary_blocker(tok(1)), None);
    assert_eq!(mem.block_order(tok(1)), None);
    assert_eq!(mem.blocked_count(), 0);
    assert_eq!(mem.unblocked_count(), 0);
    assert!(mem.validate_consistency().is_ok());
}

/// Pass-through tracking for one parent: recording it keeps the memory
/// consistent, and either removal path removes all of its bookkeeping.
#[kani::proof]
#[kani::unwind(5)]
fn negative_passthrough_lifecycle() {
    let mut mem = NegativeMemory::new(NegativeMemoryId(0));
    mem.set_unblocked(tok(1), tok(2));
    assert!(mem.validate_consistency().is_ok());
    assert_eq!(mem.get_passthrough(tok(1)), Some(tok(2)));
    assert!(mem.is_unblocked(tok(1)));
    assert!(!mem.is_blocked(tok(1)));
    if kani::any() {
        assert_eq!(mem.remove_unblocked(tok(1)), Some(tok(2)));
    } else {
        mem.remove_parent_token(tok(1));
    }
    assert!(!mem.is_unblocked(tok(1)));
    assert!(mem.is_empty());
    assert!(mem.validate_consistency().is_ok());
}
