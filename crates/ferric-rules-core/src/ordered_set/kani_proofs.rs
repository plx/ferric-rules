//! Kani proof harnesses for `ordered_set` (see docs/verification.md).
//!
//! What is proved, on the production code with `K = u8`:
//!
//! * `endpoint_representation_refines_sequence`: for every reachable
//!   endpoint state (0, 1 or 2 members, arbitrary distinct keys) followed by
//!   one operation (insertion of an arbitrary key into a set with fewer than
//!   two members, re-insertion of either member of a two-member set, removal
//!   of an arbitrary key, `clear`, or nothing) and then a second removal of
//!   an arbitrary key, `clear`, or nothing, the set refines an independent
//!   oracle sequence: every `insert`/`remove` return value, `len`,
//!   `is_empty`, `contains` of an arbitrary key, and every interleaving of
//!   `next`/`next_back` (with the exact `ExactSizeIterator::len` before each
//!   step and `None` from both ends afterwards) agree with the oracle, and
//!   the table is never allocated.
//! * The same harness checks (white-box, `assert_canonical`) that the
//!   initial state and the state after each operation are the canonical
//!   endpoint representation of the oracle sequence (`first`/`last` are its
//!   first and last members, no allocation). The first operation ranges over
//!   every operation that does not promote, so this is the induction step
//!   (and `default()` the base case) of an argument that every sequence of
//!   non-promoting operations from the empty set, of any length, refines the
//!   oracle: such sequences only visit canonical states, and every canonical
//!   state is one that `build_endpoints` constructs. The induction itself is
//!   a meta-argument, not checked by Kani.
//!
//! Known gaps: the third distinct insertion promotes the set to a hashbrown
//! table, and every operation on an allocated table is beyond Kani's budget
//! here: a harness that only promotes `{1, 2, 3}` and inspects the table by
//! raw iteration was still in symbolic execution when it was stopped at the
//! 20-minute limit (an earlier variant with three keyed lookups ran over ten
//! minutes as well). So promotion, removal, iteration, re-use after
//! emptying and `clear` on the linked table are not checked by Kani. The
//! capacity-zero-after-tombstones state needs a full probe group of
//! colliding entries (at least 16 buckets; the unit test
//! `full_collision_table_survives_emptying_and_small_reuse` uses 64) and is
//! out of reach for bounded harnesses anyway. All of these are proved
//! unboundedly, for any key type, on the model in
//! verification/verus/core_stores_ordered_set.rs.

use super::*;

/// Independent oracle: a duplicate-free sequence in insertion order.
#[derive(Clone, Copy)]
struct SeqModel {
    items: [u8; 4],
    len: usize,
}

impl SeqModel {
    fn new() -> Self {
        Self {
            items: [0; 4],
            len: 0,
        }
    }

    fn position(&self, key: u8) -> Option<usize> {
        let mut index = 0;
        while index < self.len {
            if self.items[index] == key {
                return Some(index);
            }
            index += 1;
        }
        None
    }

    fn contains(&self, key: u8) -> bool {
        self.position(key).is_some()
    }

    fn push(&mut self, key: u8) {
        self.items[self.len] = key;
        self.len += 1;
    }

    fn delete(&mut self, key: u8) {
        if let Some(mut index) = self.position(key) {
            while index + 1 < self.len {
                self.items[index] = self.items[index + 1];
                index += 1;
            }
            self.len -= 1;
        }
    }
}

/// Compare every observation of `set` with the oracle sequence: length,
/// emptiness, membership of `probe`, and an arbitrary interleaving of
/// `next`/`next_back` (with exact `len()` before every step and `None` from
/// both ends once exhausted).
fn assert_observations_match(set: &OrderedSet<u8>, model: &SeqModel, probe: u8) {
    assert_eq!(set.len(), model.len);
    assert_eq!(set.is_empty(), model.len == 0);
    assert_eq!(set.contains(&probe), model.contains(probe));
    let mut iter = set.iter();
    let mut lo = 0;
    let mut hi = model.len;
    while lo < hi {
        assert_eq!(iter.len(), hi - lo);
        if kani::any() {
            assert_eq!(iter.next(), Some(&model.items[lo]));
            lo += 1;
        } else {
            assert_eq!(iter.next_back(), Some(&model.items[hi - 1]));
            hi -= 1;
        }
    }
    assert_eq!(iter.len(), 0);
    assert_eq!(iter.next(), None);
    assert_eq!(iter.next_back(), None);
}

/// White-box check that `set` is the canonical endpoint representation of
/// the oracle sequence: no table allocation, `first`/`last` hold the first and
/// last members (both `None` when empty). Every endpoint state is determined
/// by the sequence it represents, so the states built by `build_endpoints`
/// are all the endpoint states there are.
fn assert_canonical(set: &OrderedSet<u8>, model: &SeqModel) {
    assert_eq!(set.entries.capacity(), 0);
    if model.len == 0 {
        assert!(set.first.is_none() && set.last.is_none());
    } else {
        assert_eq!(set.first, Some(model.items[0]));
        assert_eq!(set.last, Some(model.items[model.len - 1]));
    }
}

/// Build an endpoint-representation set holding the first `n` of `[a, b]`.
fn build_endpoints(n: usize, a: u8, b: u8) -> (OrderedSet<u8>, SeqModel) {
    let mut set = OrderedSet::<u8>::default();
    let mut model = SeqModel::new();
    if n >= 1 {
        assert!(set.insert(a));
        model.push(a);
    }
    if n >= 2 {
        assert!(set.insert(b));
        model.push(b);
    }
    (set, model)
}

/// Every reachable endpoint state (0, 1 or 2 members with arbitrary distinct
/// keys), followed by one arbitrary operation and then an arbitrary removal
/// or clear, refines the oracle sequence. Fresh insertion into a two-member
/// set promotes to the table and is covered by the promotion harnesses; the
/// two-member case therefore only re-inserts existing members.
#[kani::proof]
#[kani::unwind(4)]
fn endpoint_representation_refines_sequence() {
    let a: u8 = kani::any();
    let b: u8 = kani::any();
    kani::assume(a != b);
    let mut n = 0;
    while n < 3 {
        let (mut set, mut model) = build_endpoints(n, a, b);
        assert_canonical(&set, &model);
        // First operation.
        match kani::any::<u8>() {
            0 if n < 2 => {
                let key: u8 = kani::any();
                let fresh = !model.contains(key);
                assert_eq!(set.insert(key), fresh);
                if fresh {
                    model.push(key);
                }
            }
            1 if n == 2 => {
                assert!(!set.insert(a));
            }
            2 if n == 2 => {
                assert!(!set.insert(b));
            }
            3 => {
                let key: u8 = kani::any();
                assert_eq!(set.remove(&key), model.contains(key));
                model.delete(key);
            }
            4 => {
                set.clear();
                model = SeqModel::new();
            }
            _ => {}
        }
        assert_canonical(&set, &model);
        // Second operation: removal (possibly emptying the set) or clear.
        match kani::any::<u8>() {
            0 => {
                let key: u8 = kani::any();
                assert_eq!(set.remove(&key), model.contains(key));
                model.delete(key);
            }
            1 => {
                set.clear();
                model = SeqModel::new();
            }
            _ => {}
        }
        // The endpoint representation never allocates and stays canonical.
        assert_canonical(&set, &model);
        assert_observations_match(&set, &model, kani::any());
        kani::cover!(n == 2 && model.len == 2);
        kani::cover!(n == 1 && model.len == 2);
        kani::cover!(n == 2 && model.len == 1 && model.items[0] == b);
        kani::cover!(n == 2 && model.len == 0);
        n += 1;
    }
}
