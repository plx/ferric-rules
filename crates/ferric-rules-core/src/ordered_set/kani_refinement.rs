//! Kani refinement harnesses linking this module to its Verus model for `ordered_set` (see docs/verification.md).
//!
//! The Verus model `verification/verus/ordered_set.rs` proves, for a
//! transcription of this module, that `OrderedSet` refines a duplicate-free
//! insertion-ordered sequence. These harnesses check the production code
//! (`K = u8`, the real `FxHashMap`) against an independent oracle of the same
//! specification (`SpecSeq`), mirroring the Verus postconditions:
//!
//! * results: `insert` returns "was absent" and then appends; `remove`
//!   returns "was present" and then deletes exactly that member; `clear`
//!   empties;
//! * the representation invariant `rep` (`assert_rep`): the reported capacity
//!   bounds the length; at capacity 0 the table is empty and the endpoints
//!   hold at most two members; otherwise every member is in the table with
//!   `previous`/`next` equal to its neighbours in the oracle and the table
//!   holds nothing else; `first`/`last` are the oracle's ends;
//! * observations: `len`, `is_empty`, `contains`, `ExactSizeIterator::len`
//!   before every step, `next`/`next_back` yielding the front/back of the
//!   unvisited window, and `None` from both ends afterwards.
//!
//! The header-only harness checks all of these; the table-mode harnesses
//! check the subset they can afford (listed below).
//!
//! Harnesses and bounds (all with Kani's default checks and unwinding
//! assertions):
//!
//! * `header_only_step_refines_spec` (symbolic): arbitrary distinct keys
//!   `a`, `b`, `c`; every header-only state of 0, 1 or 2 members (`[]`,
//!   `[a]`, `[a, b]`), then any one operation (insert or remove of `a`, `b`
//!   or `c`, or `clear`; inserting `c` into `[a, b]` promotes and is left to
//!   the table-mode harnesses), then every observation including every
//!   `next`/`next_back` interleaving. In this representation the table is
//!   never allocated and `assert_rep` pins `first`/`last` to the view, so the
//!   concrete state is a function of the view and this is an inductive step:
//!   with `default()` satisfying `assert_rep`, every trace of any length
//!   that never promotes refines the oracle.
//! * Table mode (concrete keys). `promotion_refines_spec`: `[10, 20]` plus
//!   the promoting insert of `30`, then the full invariant including every
//!   member's links. Every other table-mode harness starts from the promoted
//!   `[10, 20, 30]` and performs one operation: `linked_iteration_*` (`next`
//!   yields the head, `next_back` the tail, both cursors land on the
//!   middle), `linked_contains_*` (a hit and a miss), `linked_remove_absent_*`
//!   (a table miss), `linked_remove_head_*` / `linked_remove_tail_*` (removal
//!   with one `get_mut(..).unwrap()` relink, whose rewritten links are then
//!   read back with one `get` and compared with the oracle),
//!   `linked_remove_middle_*` (removal with two relinks; both `unwrap`s hit,
//!   the rewritten links are not read back), and `linked_clear_*` (`clear`
//!   keeps capacity and resets the endpoints). After a mutation they check
//!   the lookup-free part of the invariant (`assert_linked_fields`: allocated
//!   table, table length, `len`, `is_empty`, `first`, `last`).
//!
//! Known gaps. On an allocated table every hash-table operation is very
//! expensive under Kani: symbolic execution cannot see that hashbrown's probe
//! loops stop after the first group, so each lookup unrolls them to the
//! unwind bound, and the bound cannot drop below 5 because a
//! `ptr::swap_nonoverlapping` chunk loop on the promotion path runs four
//! iterations. Measured here (7 GB address-space limit, alongside other
//! agents' cbmc runs): promotion alone took 146 s of symbolic execution;
//! promotion plus three `get`s 500 s (669 s in total); a removal plus its
//! relink plus one read-back `get` 766-791 s (1027-1078 s in total, close to
//! the 20-minute limit); promotion plus three iterator lookups, and a removal followed by a walk of the whole
//! table (`HashMap::iter`), ran out of memory in the solver back end; and
//! every harness that called `insert` on an allocated table (re-inserting a
//! member, or inserting after `clear`) was still in symbolic execution at the
//! 20-minute limit, because symbolic execution cannot tell whether the
//! `entry` lookup hits and so also explores `reserve(1)` and rehashing. So
//! the following are not checked by Kani: any insertion into an allocated
//! table (`Entry::Occupied`, and the linked append with or without a tail);
//! the links rewritten by an interior removal, and the links of the members
//! a removal did not touch; removal of the last member of an allocated
//! table; symbolic keys, symbolic operation choices or traces longer than one
//! operation in table mode; growth beyond the first 4-bucket table; and the
//! allocated-but-zero-capacity state that tombstone-heavy removal can leave
//! (it needs a full probe group of colliding entries; see the unit test
//! `full_collision_table_survives_emptying_and_small_reuse`). Snapshot
//! decoding (`Deserialize`, `serde` feature) is not checked either: decoding
//! two arbitrary keys through `serde::de::value::SeqDeserializer` was still
//! in symbolic execution at the 20-minute limit, and with an arbitrary input
//! length of at most two it ran out of memory. The Verus model proves all of
//! these for every size and every capacity behaviour, for its transcription
//! of this module.

use super::*;

/// Independent oracle for the abstract specification of
/// `verification/verus/ordered_set.rs` (`spec_apply` / `spec_result`): a
/// duplicate-free sequence in insertion order. Insert appends an absent key,
/// remove deletes a present key keeping the others in order, clear empties.
#[derive(Clone, Copy)]
struct SpecSeq {
    items: [u8; 4],
    len: usize,
}

impl SpecSeq {
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

    fn insert(&mut self, key: u8) -> bool {
        if self.contains(key) {
            return false;
        }
        self.items[self.len] = key;
        self.len += 1;
        true
    }

    fn remove(&mut self, key: u8) -> bool {
        let Some(mut index) = self.position(key) else {
            return false;
        };
        while index + 1 < self.len {
            self.items[index] = self.items[index + 1];
            index += 1;
        }
        self.len -= 1;
        true
    }

    fn clear(&mut self) {
        self.len = 0;
    }

    /// `at(v, i)` of the Verus model: the member at position `i`, or `None`
    /// outside the sequence.
    fn at(&self, index: isize) -> Option<u8> {
        if index >= 0 && (index as usize) < self.len {
            Some(self.items[index as usize])
        } else {
            None
        }
    }
}

/// Mirrors the Verus representation invariant `rep` on the concrete fields:
/// the reported capacity bounds the length; with capacity 0 the table is
/// empty and the endpoints hold at most two members; otherwise the table
/// holds exactly the members, each linked to its neighbours in the oracle
/// sequence. In both cases `first`/`last` are the oracle's ends.
fn assert_rep(set: &OrderedSet<u8>, spec: &SpecSeq) {
    let capacity = set.entries.capacity();
    assert!(capacity >= set.entries.len());
    assert!(set.first == spec.at(0));
    assert!(set.last == spec.at(spec.len as isize - 1));
    if capacity == 0 {
        assert!(set.entries.is_empty());
        assert!(spec.len <= 2);
    } else {
        assert_eq!(set.entries.len(), spec.len);
        let mut index = 0;
        while index < spec.len {
            let links = set.entries.get(&spec.items[index]).unwrap();
            assert!(links.previous == spec.at(index as isize - 1));
            assert!(links.next == spec.at(index as isize + 1));
            index += 1;
        }
    }
}

/// Mirrors the Verus postconditions of `len`, `is_empty`, `contains`,
/// `size_hint` and `next`/`next_back`: lengths and membership of `probes`
/// agree with the oracle, and an arbitrary interleaving of `next` and
/// `next_back` (chosen by Kani, so every interleaving is checked) yields the
/// front/back of the unvisited window, with exact `len()` before each step
/// and `None` from both ends once it is empty.
fn assert_observations(set: &OrderedSet<u8>, spec: &SpecSeq, probes: &[u8]) {
    assert_eq!(set.len(), spec.len);
    assert_eq!(set.is_empty(), spec.len == 0);
    for probe in probes {
        assert_eq!(set.contains(probe), spec.contains(*probe));
    }
    let mut iter = set.iter();
    let mut lo = 0;
    let mut hi = spec.len;
    while lo < hi {
        assert_eq!(iter.len(), hi - lo);
        if kani::any() {
            assert_eq!(iter.next(), Some(&spec.items[lo]));
            lo += 1;
        } else {
            assert_eq!(iter.next_back(), Some(&spec.items[hi - 1]));
            hi -= 1;
        }
    }
    assert_eq!(iter.len(), 0);
    assert_eq!(iter.next(), None);
    assert_eq!(iter.next_back(), None);
}

/// Builds the header-only set whose view is the first `size` keys of
/// `[a, b]`, by inserting them into `OrderedSet::default()`.
fn build_header_only(size: usize, a: u8, b: u8) -> (OrderedSet<u8>, SpecSeq) {
    let mut set = OrderedSet::<u8>::default();
    let mut spec = SpecSeq::new();
    if size >= 1 {
        assert!(set.insert(a));
        assert!(spec.insert(a));
    }
    if size >= 2 {
        assert!(set.insert(b));
        assert!(spec.insert(b));
    }
    assert_rep(&set, &spec);
    (set, spec)
}

/// Inductive step for the header-only representation. In that
/// representation the concrete state (an unallocated table, `first`,
/// `last`) is determined by the view (`assert_rep` pins `first`/`last` to
/// the view's ends and the table to empty with zero capacity), so every
/// reachable header-only state with at most two members is, up to renaming
/// of keys, one of the states built by `build_header_only` from arbitrary
/// distinct keys `a`, `b`. From each of them, one arbitrary operation
/// (insert or remove of a member, of a third arbitrary key, or `clear`;
/// inserting a third key into a two-member set promotes the set and is
/// checked by the table-mode harnesses instead) returns the oracle's result,
/// keeps the table unallocated, re-establishes the representation invariant
/// for the oracle's new view, and every observation (length, emptiness,
/// membership of all three keys, every interleaving of `next`/`next_back`)
/// agrees with the oracle. By induction over traces this covers every
/// trace of any length that stays in the header-only representation from
/// `default()`.
#[kani::proof]
#[kani::unwind(4)]
fn header_only_step_refines_spec() {
    let a: u8 = kani::any();
    let b: u8 = kani::any();
    let c: u8 = kani::any();
    kani::assume(a != b && a != c && b != c);
    let mut size = 0;
    while size < 3 {
        let (mut set, mut spec) = build_header_only(size, a, b);
        let choice: u8 = kani::any();
        match choice {
            0 => assert_eq!(set.insert(a), spec.insert(a)),
            1 => assert_eq!(set.insert(b), spec.insert(b)),
            2 if size < 2 => assert_eq!(set.insert(c), spec.insert(c)),
            3 => assert_eq!(set.remove(&a), spec.remove(a)),
            4 => assert_eq!(set.remove(&b), spec.remove(b)),
            5 => assert_eq!(set.remove(&c), spec.remove(c)),
            6 => {
                set.clear();
                spec.clear();
            }
            _ => kani::assume(false),
        }
        assert_eq!(set.entries.capacity(), 0);
        assert_rep(&set, &spec);
        kani::cover!(size == 0 && choice == 0, "first member");
        kani::cover!(size == 2 && choice == 5, "absent removal from two");
        kani::cover!(size == 2 && choice == 3, "remove first of two");
        kani::cover!(size == 2 && choice == 4, "remove last of two");
        kani::cover!(size == 1 && choice == 2, "second member");
        kani::cover!(size == 2 && choice == 0, "duplicate of first endpoint");
        kani::cover!(size == 2 && choice == 1, "duplicate of last endpoint");
        kani::cover!(size == 1 && choice == 3, "remove only member");
        kani::cover!(size == 2 && choice == 6, "clear two members");
        assert_observations(&set, &spec, &[a, b, c]);
        size += 1;
    }
}

/// Builds the promoted set `[10, 20, 30]` (and its oracle): two
/// header-only insertions and the promoting third one.
fn build_promoted() -> (OrderedSet<u8>, SpecSeq) {
    let mut set = OrderedSet::<u8>::default();
    let mut spec = SpecSeq::new();
    for key in [10, 20, 30] {
        assert_eq!(set.insert(key), spec.insert(key));
    }
    (set, spec)
}

/// Mirrors the cheap part of the Verus invariant without table lookups:
/// the table is allocated, its length is the oracle's, and the endpoints
/// are the oracle's ends.
fn assert_linked_fields(set: &OrderedSet<u8>, spec: &SpecSeq) {
    assert!(set.entries.capacity() > 0);
    assert_eq!(set.entries.len(), spec.len);
    assert_eq!(set.len(), spec.len);
    assert_eq!(set.is_empty(), spec.len == 0);
    assert!(set.first == spec.at(0));
    assert!(set.last == spec.at(spec.len as isize - 1));
}

/// Mirrors one conjunct of the Verus `linked_rep`: `key` is in the table
/// and carries the links of its oracle position (`links_at`). One table
/// lookup, used to read back a link that an operation rewrote.
fn assert_member_links(set: &OrderedSet<u8>, spec: &SpecSeq, key: u8) {
    let index = spec.position(key).unwrap() as isize;
    let links = set.entries.get(&key).unwrap();
    assert!(links.previous == spec.at(index - 1));
    assert!(links.next == spec.at(index + 1));
}

/// Promotion: the third distinct insertion moves the endpoints into the
/// table (`reserve(3)` plus three inserts). The result, length, emptiness
/// and the full representation invariant (every member's links) agree with
/// the oracle.
#[kani::proof]
#[kani::unwind(5)]
fn promotion_refines_spec() {
    let mut set = OrderedSet::<u8>::default();
    let mut spec = SpecSeq::new();
    for key in [10, 20] {
        assert_eq!(set.insert(key), spec.insert(key));
    }
    assert_eq!(set.entries.capacity(), 0);
    assert_rep(&set, &spec);
    assert_eq!(set.insert(30), spec.insert(30));
    assert!(set.entries.capacity() > 0);
    assert_eq!(set.len(), spec.len);
    assert!(!set.is_empty());
    assert_rep(&set, &spec);
}

/// Linked iteration: `next` yields the head and `next_back` the tail (table
/// hits in both directions), with exact `len()` before every step, and both
/// cursors then point at the middle member (the unvisited window). A third
/// lookup to consume the middle exceeds the 7 GB memory limit.
#[kani::proof]
#[kani::unwind(5)]
fn linked_iteration_refines_spec() {
    let (set, spec) = build_promoted();
    let mut iter = set.iter();
    assert_eq!(iter.len(), 3);
    assert_eq!(iter.next(), Some(&spec.items[0]));
    assert_eq!(iter.len(), 2);
    assert_eq!(iter.next_back(), Some(&spec.items[2]));
    assert_eq!(iter.len(), 1);
    assert!(iter.front == spec.at(1));
    assert!(iter.back == spec.at(1));
}

/// Linked membership: `contains` hits a member and misses an absent key.
#[kani::proof]
#[kani::unwind(5)]
fn linked_contains_refines_spec() {
    let (set, spec) = build_promoted();
    assert_eq!(set.contains(&10), spec.contains(10));
    assert_eq!(set.contains(&40), spec.contains(40));
}

/// Linked absent removal: a table miss returns false and changes neither the
/// length nor the endpoints.
#[kani::proof]
#[kani::unwind(5)]
fn linked_remove_absent_refines_spec() {
    let (mut set, mut spec) = build_promoted();
    assert_eq!(set.remove(&40), spec.remove(40));
    assert_linked_fields(&set, &spec);
}

/// Linked head removal: the successor's `previous` is cleared through
/// `get_mut(..).unwrap()` and `first` moves to it. The rewritten links of
/// the new head are read back and match the oracle.
#[kani::proof]
#[kani::unwind(5)]
fn linked_remove_head_refines_spec() {
    let (mut set, mut spec) = build_promoted();
    assert_eq!(set.remove(&10), spec.remove(10));
    assert_linked_fields(&set, &spec);
    assert_member_links(&set, &spec, 20);
}

/// Linked interior removal: both neighbours are relinked through
/// `get_mut(..).unwrap()` (both lookups hit, so neither `unwrap` panics) and
/// the endpoints stay. The rewritten links are not read back: that needs two
/// more lookups, and one read-back after a single relink already takes over
/// 1000 s, so it was not attempted.
#[kani::proof]
#[kani::unwind(5)]
fn linked_remove_middle_refines_spec() {
    let (mut set, mut spec) = build_promoted();
    assert_eq!(set.remove(&20), spec.remove(20));
    assert_linked_fields(&set, &spec);
}

/// Linked tail removal: the predecessor's `next` is cleared through
/// `get_mut(..).unwrap()` and `last` moves to it. The rewritten links of
/// the new tail are read back and match the oracle.
#[kani::proof]
#[kani::unwind(5)]
fn linked_remove_tail_refines_spec() {
    let (mut set, mut spec) = build_promoted();
    assert_eq!(set.remove(&30), spec.remove(30));
    assert_linked_fields(&set, &spec);
    assert_member_links(&set, &spec, 20);
}

/// Linked `clear`: the cleared table keeps reporting capacity (so later
/// insertions take the linked path), is empty, and has no endpoints.
#[kani::proof]
#[kani::unwind(5)]
fn linked_clear_refines_spec() {
    let (mut set, mut spec) = build_promoted();
    set.clear();
    spec.clear();
    assert_linked_fields(&set, &spec);
}
