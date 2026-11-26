//! Verus model of activation-sequence bookkeeping in `Agenda`
//! (crates/ferric-rules-core/src/agenda.rs).
//!
//! Proved (unbounded: any number of live activations and of operations; every
//! operation, the exhaustion rebase included, preserves `inv` from every state
//! satisfying it, so the results hold across any number of rebases):
//!
//! - `rebase`: the sequence rebase assigns `0..n` to the `n` live activations,
//!   keeps them pairwise distinct and below `next_seq == n`, and preserves the
//!   relative order of every pair of live activations.
//! - `add`: never overflows the `u64` sequence counter (it rebases first when
//!   `next_seq == u64::MAX`; Verus checks the `+ 1` that models
//!   `ActivationSeq::next`); the new activation gets exactly `next_seq`, or
//!   the live count after the exhaustion rebase, and `next_seq` becomes its
//!   successor; without a rebase the existing sequences are untouched; the new
//!   activation's sequence is strictly greater than every live one; the
//!   relative order of existing activations is unchanged.
//! - `remove` / `clear`: preserve the invariant; `clear` resets `next_seq`.
//! - Invariant `inv` (preserved by every operation): live sequences are
//!   pairwise distinct, below `next_seq`, and ordered exactly like the ghost
//!   creation counter `created`, i.e. sequence order is creation order
//!   (`sequence_order_is_creation_order`). Hence depth (`Reverse(seq)`) always
//!   prefers the most recently created live activation and breadth the oldest,
//!   across rebases. (That depth/breadth keys order by the sequence is the
//!   Kani `build_key_*` result; it is not re-proved here.)
//! - `run_ops_from`: every sequence of add / remove-at-any-position / clear
//!   from any state satisfying `inv` (any `next_seq`, so rebases occur) keeps
//!   `inv`, with the rebase order supplied by `chronological`, which shows that
//!   the sort contract `rebase` assumes is satisfiable in every such state;
//!   `run_ops` is the special case of a new agenda. (From a new agenda alone,
//!   fewer than `u64::MAX` operations never reach the rebase, which is why the
//!   general start state matters.)
//! - `exhaustion_example`: the rebase branch is reachable and reproduces the
//!   production unit test at agenda.rs:685 (three adds from
//!   `u64::MAX - 2` give sequences 0, 1, 2 and `next_seq == 3`).
//!
//! Model and abstractions (see each function for the production line it
//! models):
//!
//! - Only the sequence state is modeled: `seqs` holds the `activation_seq` of
//!   every live activation (`activations`, agenda.rs:160), and `next_seq` is
//!   `Agenda::next_seq` (agenda.rs:167). The position order of `seqs` is the
//!   model's own (`add` appends, `remove` closes the gap); it is not the
//!   `SlotMap` slot order, which reuses freed slots (slotmap 1.1.1
//!   `try_insert_with_key`). This does not affect the results: `rebase`
//!   accepts every sorted permutation of the positions, and every invariant
//!   and postcondition quantifies over all pairs of live activations.
//!   Keys, the `ordering` / `id_to_key` / token / per-rule indexes and which
//!   activation a removal path selects are not modeled here.
//! - `rebase` receives `chron`, the slot order produced in production by
//!   `collect` + `sort_unstable_by_key` (agenda.rs:278-283). The model does not
//!   re-implement the standard sort; it takes the sort's contract as a
//!   precondition (`sorted_permutation`: a permutation of the live slots in
//!   strictly increasing old sequence, strict because live sequences are
//!   distinct) and proves the result for every such order. `chronological`
//!   (an insertion sort) is driver code for `run_ops`, not a model of
//!   production code.
//! - `remove(i)` models the effect on live sequences of every removal path
//!   (`pop`, `pop_matching`, `pop_matching_rule`,
//!   `remove_activations_for_token`, `remove_activations_for_rule`): one live
//!   activation disappears and the others keep their sequence and relative
//!   iteration order.
//! - `add` (and `rebase`) require fewer than `u64::MAX` live activations. In
//!   production this is guaranteed by the `SlotMap` capacity (fewer than
//!   `u32::MAX` slots; slotmap 1.1.1 `try_insert_with_key` panics with
//!   "SlotMap is full"), not by the `expect("live activations fit the
//!   sequence space")` calls at agenda.rs:286 and 290, which only guard the
//!   `usize` to `u64` conversion and cannot fail on 64-bit targets.
//!
//! Trusted base: Verus and vstd's specifications of `Vec` (`new`, `push`,
//! `set`, `insert`, `remove`, `clear`, `len`, indexing). No `assume`, `admit`,
//! external bodies or assumed specifications are used in this file.

use vstd::prelude::*;

verus! {

/// Live sequences are pairwise distinct.
pub open spec fn distinct(s: Seq<u64>) -> bool {
    forall|i: int, j: int|
        0 <= i < s.len() && 0 <= j < s.len() && i != j ==> #[trigger] s[i] != #[trigger] s[j]
}

/// `s` and `t` order every pair of positions the same way.
pub open spec fn same_order(s: Seq<u64>, t: Seq<int>) -> bool {
    &&& s.len() == t.len()
    &&& forall|i: int, j: int|
        0 <= i < s.len() && 0 <= j < s.len() ==> ((#[trigger] s[i] < #[trigger] s[j]) == (t[i]
            < t[j]))
}

/// Contract of `collect` + `sort_unstable_by_key` over distinct sequences
/// (agenda.rs:278-283): `chron` lists every live slot exactly once, by strictly
/// increasing old sequence.
pub open spec fn sorted_permutation(seqs: Seq<u64>, chron: Seq<usize>) -> bool {
    &&& chron.len() == seqs.len()
    &&& forall|k: int| 0 <= k < chron.len() ==> (#[trigger] chron[k] as int) < seqs.len()
    &&& forall|i: int| 0 <= i < seqs.len() ==> #[trigger] listed(chron, i)
    &&& forall|k1: int, k2: int|
        0 <= k1 < k2 < chron.len() ==> seqs[#[trigger] chron[k1] as int] < seqs[#[trigger] chron[
            k2] as int]
}

/// Slot `i` appears in `chron`.
pub open spec fn listed(chron: Seq<usize>, i: int) -> bool {
    exists|k: int| 0 <= k < chron.len() && chron[k] == i
}

/// Position of slot `i` in `chron`.
pub open spec fn pos(chron: Seq<usize>, i: int) -> int {
    choose|k: int| 0 <= k < chron.len() && chron[k] == i
}

/// The sequence state of an `Agenda`.
pub struct SeqState {
    /// `activation_seq` of each live activation in slot order (agenda.rs:160).
    pub seqs: Vec<u64>,
    /// `Agenda::next_seq` (agenda.rs:167).
    pub next_seq: u64,
    /// Ghost creation counter value of each live activation.
    pub created: Ghost<Seq<int>>,
    /// Ghost count of activations ever created.
    pub clock: Ghost<int>,
}

impl SeqState {
    pub open spec fn inv(&self) -> bool {
        &&& self.created@.len() == self.seqs@.len()
        &&& distinct(self.seqs@)
        &&& forall|i: int| 0 <= i < self.seqs@.len() ==> #[trigger] self.seqs@[i] < self.next_seq
        &&& forall|i: int| 0 <= i < self.created@.len() ==> #[trigger] self.created@[i] < self.clock@
        &&& same_order(self.seqs@, self.created@)
    }

    /// Models agenda.rs:192 `Agenda::with_strategy` (`next_seq: ActivationSeq::ZERO`).
    pub fn new() -> (r: Self)
        ensures
            r.inv(),
            r.seqs@.len() == 0,
            r.next_seq == 0,
    {
        SeqState { seqs: Vec::new(), next_seq: 0, created: Ghost(Seq::empty()), clock: Ghost(0) }
    }

    /// Models agenda.rs:276 `Agenda::rebase_sequences` (lines 278-291: the
    /// chronological renumbering and the new `next_seq`; lines 292-298 rebuild
    /// keys and are not modeled).
    pub fn rebase(&mut self, chron: &Vec<usize>)
        requires
            old(self).inv(),
            sorted_permutation(old(self).seqs@, chron@),
            old(self).seqs@.len() < u64::MAX,
        ensures
            final(self).inv(),
            final(self).seqs@.len() == old(self).seqs@.len(),
            final(self).next_seq == old(self).seqs@.len(),
            final(self).created@ == old(self).created@,
            final(self).clock@ == old(self).clock@,
            forall|i: int, j: int|
                0 <= i < old(self).seqs@.len() && 0 <= j < old(self).seqs@.len() ==> ((
                #[trigger] final(self).seqs@[i] < #[trigger] final(self).seqs@[j]) == (old(
                self).seqs@[i] < old(self).seqs@[j])),
    {
        let ghost old_seqs = self.seqs@;
        let n = self.seqs.len();
        let mut k: usize = 0;
        while k < n
            invariant
                self.seqs@.len() == n,
                chron@.len() == n,
                n == old_seqs.len(),
                0 <= k <= n,
                sorted_permutation(old_seqs, chron@),
                forall|k2: int| 0 <= k2 < k ==> self.seqs@[#[trigger] chron@[k2] as int] == k2,
                self.next_seq == old(self).next_seq,
                self.created@ == old(self).created@,
                self.clock@ == old(self).clock@,
            decreases n - k,
        {
            // `for (sequence, &(_, id)) in chronological.iter().enumerate()`
            let slot = chron[k];
            proof {
                // Earlier positions hold other slots (old sequences strictly increase).
                assert forall|k2: int| 0 <= k2 < k implies chron@[k2] != slot by {
                    assert(old_seqs[chron@[k2] as int] < old_seqs[chron@[k as int] as int]);
                }
            }
            // `self.activations[id].activation_seq = ActivationSeq::new(sequence)`
            self.seqs.set(slot, k as u64);
            k += 1;
        }
        // `self.next_seq = ActivationSeq::new(chronological.len())`
        self.next_seq = n as u64;
        proof {
            let new_seqs = self.seqs@;
            assert forall|i: int|
                #![trigger new_seqs[i]]
                #![trigger pos(chron@, i)]
                0 <= i < n implies new_seqs[i] == pos(chron@, i) && 0 <= pos(chron@, i) < n
                    && chron@[pos(chron@, i)] == i by {
                assert(listed(chron@, i));
                let p = pos(chron@, i);
                assert(new_seqs[chron@[p] as int] == p);
            }
            assert forall|i: int, j: int|
                0 <= i < n && 0 <= j < n implies ((#[trigger] new_seqs[i] < #[trigger] new_seqs[j])
                == (old_seqs[i] < old_seqs[j])) by {
                let (pi, pj) = (pos(chron@, i), pos(chron@, j));
                if pi < pj {
                    assert(old_seqs[chron@[pi] as int] < old_seqs[chron@[pj] as int]);
                } else if pj < pi {
                    assert(old_seqs[chron@[pj] as int] < old_seqs[chron@[pi] as int]);
                }
            }
            assert forall|i: int, j: int|
                0 <= i < n && 0 <= j < n && i != j implies #[trigger] new_seqs[i]
                != #[trigger] new_seqs[j] by {
                assert(chron@[pos(chron@, i)] == i);
                assert(chron@[pos(chron@, j)] == j);
            }
            assert forall|i: int| 0 <= i < n implies #[trigger] new_seqs[i] < self.next_seq by {
                assert(0 <= pos(chron@, i) < n);
            }
        }
    }

    /// Models the sequence handling of agenda.rs:245 `Agenda::add`
    /// (lines 249-253) and the new live entry (`activations.insert_with_key`,
    /// line 258). `chron` is consulted only when the rebase branch is taken.
    pub fn add(&mut self, chron: &Vec<usize>) -> (seq: u64)
        requires
            old(self).inv(),
            old(self).seqs@.len() < u64::MAX,
            old(self).next_seq == u64::MAX ==> sorted_permutation(old(self).seqs@, chron@),
        ensures
            final(self).inv(),
            final(self).seqs@.len() == old(self).seqs@.len() + 1,
            final(self).seqs@.last() == seq,
            final(self).created@ == old(self).created@.push(old(self).clock@),
            // Exact assignment: the current counter, or the live count after
            // the exhaustion rebase; the counter then advances by one.
            old(self).next_seq < u64::MAX ==> seq == old(self).next_seq,
            old(self).next_seq < u64::MAX ==> final(self).seqs@ == old(self).seqs@.push(seq),
            old(self).next_seq == u64::MAX ==> seq == old(self).seqs@.len(),
            final(self).next_seq == seq + 1,
            forall|i: int| 0 <= i < old(self).seqs@.len() ==> #[trigger] final(self).seqs@[i] < seq,
            forall|i: int, j: int|
                0 <= i < old(self).seqs@.len() && 0 <= j < old(self).seqs@.len() ==> ((
                #[trigger] final(self).seqs@[i] < #[trigger] final(self).seqs@[j]) == (old(
                self).seqs@[i] < old(self).seqs@[j])),
    {
        // `if self.next_seq.get() == u64::MAX { self.rebase_sequences(); }`
        if self.next_seq == u64::MAX {
            self.rebase(chron);
        }
        // `activation.activation_seq = self.next_seq;`
        let seq = self.next_seq;
        // `self.next_seq = self.next_seq.next();` (ActivationSeq::next, agenda.rs:46)
        self.next_seq = seq + 1;
        self.seqs.push(seq);
        self.created = Ghost(self.created@.push(self.clock@));
        self.clock = Ghost(self.clock@ + 1);
        proof {
            let s = self.seqs@;
            let c = self.created@;
            assert(s.len() == c.len());
            assert forall|i: int, j: int|
                0 <= i < s.len() && 0 <= j < s.len() implies ((#[trigger] s[i] < #[trigger] s[j])
                == (c[i] < c[j])) by {
                if i < s.len() - 1 && j < s.len() - 1 {
                    assert(s[i] == s.drop_last()[i]);
                }
            }
        }
        seq
    }

    /// Models the effect on live sequences of removing the activation at slot
    /// position `i`: agenda.rs:304 `pop`, :324 `pop_matching`, :364
    /// `pop_matching_rule`, :407 `remove_activations_for_token` and :435
    /// `remove_activations_for_rule` (`self.activations.remove(id)`).
    pub fn remove(&mut self, i: usize) -> (seq: u64)
        requires
            old(self).inv(),
            i < old(self).seqs@.len(),
        ensures
            final(self).inv(),
            final(self).seqs@ == old(self).seqs@.remove(i as int),
            final(self).created@ == old(self).created@.remove(i as int),
            final(self).next_seq == old(self).next_seq,
            seq == old(self).seqs@[i as int],
    {
        let seq = self.seqs.remove(i);
        self.created = Ghost(self.created@.remove(i as int));
        proof {
            let (s0, c0) = (old(self).seqs@, old(self).created@);
            let (s, c) = (self.seqs@, self.created@);
            assert forall|a: int| 0 <= a < s.len() implies s[a] == s0[if a < i {
                a
            } else {
                a + 1
            }] && c[a] == c0[if a < i {
                a
            } else {
                a + 1
            }] by {}
            assert forall|a: int, b: int|
                0 <= a < s.len() && 0 <= b < s.len() implies ((#[trigger] s[a] < #[trigger] s[b])
                == (c[a] < c[b])) by {
                let (a0, b0) = (if a < i {
                    a
                } else {
                    a + 1
                }, if b < i {
                    b
                } else {
                    b + 1
                });
                assert(s0[a0] == s[a] && s0[b0] == s[b]);
            }
            assert forall|a: int, b: int|
                0 <= a < s.len() && 0 <= b < s.len() && a != b implies #[trigger] s[a]
                != #[trigger] s[b] by {
                let (a0, b0) = (if a < i {
                    a
                } else {
                    a + 1
                }, if b < i {
                    b
                } else {
                    b + 1
                });
                assert(s0[a0] == s[a] && s0[b0] == s[b]);
            }
        }
        seq
    }

    /// Models agenda.rs:520 `Agenda::clear` (`activations.clear()`,
    /// `next_seq = ActivationSeq::ZERO`).
    pub fn clear(&mut self)
        requires
            old(self).inv(),
        ensures
            final(self).inv(),
            final(self).seqs@.len() == 0,
            final(self).next_seq == 0,
    {
        self.seqs.clear();
        self.next_seq = 0;
        self.created = Ghost(Seq::empty());
    }
}

/// Depth and breadth consequence: in every reachable state, the live
/// activation with the greatest sequence (the first one popped under depth at
/// equal salience) is the most recently created one, and the one with the
/// least sequence (breadth) is the oldest.
pub proof fn sequence_order_is_creation_order(st: SeqState, i: int, j: int)
    requires
        st.inv(),
        0 <= i < st.seqs@.len(),
        0 <= j < st.seqs@.len(),
    ensures
        (st.seqs@[i] < st.seqs@[j]) == (st.created@[i] < st.created@[j]),
        (st.seqs@[i] == st.seqs@[j]) == (i == j),
{
}

/// Builds a `sorted_permutation` of the live slots by insertion. This is not
/// a model of production code: it stands in for `collect` +
/// `sort_unstable_by_key` in `run_ops` and shows that the contract `rebase`
/// takes as a precondition is satisfiable in every reachable state.
pub fn chronological(seqs: &Vec<u64>) -> (chron: Vec<usize>)
    requires
        distinct(seqs@),
    ensures
        sorted_permutation(seqs@, chron@),
{
    let n = seqs.len();
    let mut chron: Vec<usize> = Vec::new();
    let mut i: usize = 0;
    while i < n
        invariant
            0 <= i <= n,
            n == seqs@.len(),
            distinct(seqs@),
            chron@.len() == i,
            forall|k: int| 0 <= k < chron@.len() ==> (#[trigger] chron@[k] as int) < i,
            forall|s: int| 0 <= s < i ==> #[trigger] listed(chron@, s),
            forall|k1: int, k2: int|
                0 <= k1 < k2 < chron@.len() ==> seqs@[#[trigger] chron@[k1] as int]
                    < seqs@[#[trigger] chron@[k2] as int],
        decreases n - i,
    {
        let mut p: usize = 0;
        while p < chron.len() && seqs[chron[p]] < seqs[i]
            invariant
                p <= chron@.len(),
                i < n,
                n == seqs@.len(),
                chron@.len() == i,
                forall|k: int| 0 <= k < chron@.len() ==> (#[trigger] chron@[k] as int) < i,
                forall|k: int| 0 <= k < p ==> seqs@[#[trigger] chron@[k] as int] < seqs@[i as int],
            decreases chron@.len() - p,
        {
            p += 1;
        }
        let ghost before = chron@;
        proof {
            // Every slot from position `p` on has a later sequence than `i`.
            assert forall|k: int| p <= k < before.len() implies seqs@[i as int] < seqs@[
                #[trigger] before[k] as int] by {
                assert(seqs@[i as int] <= seqs@[before[p as int] as int]);
                assert(before[p as int] != i);
                if k > p {
                    assert(seqs@[before[p as int] as int] < seqs@[before[k] as int]);
                }
            }
            before.insert_ensures(p as int, i);
        }
        chron.insert(p, i);
        proof {
            let after = chron@;
            assert forall|k: int| 0 <= k < after.len() implies (#[trigger] after[k] as int) < i
                + 1 by {
                if k > p {
                    assert(after[k] == before[k - 1]);
                }
            }
            assert forall|s: int| 0 <= s < i + 1 implies #[trigger] listed(after, s) by {
                if s == i {
                    assert(after[p as int] == i);
                } else {
                    assert(listed(before, s));
                    let k = choose|k: int| 0 <= k < before.len() && before[k] == s;
                    if k < p {
                        assert(after[k] == s);
                    } else {
                        assert(after[k + 1] == s);
                    }
                }
            }
            assert forall|k1: int, k2: int|
                0 <= k1 < k2 < after.len() implies seqs@[#[trigger] after[k1] as int]
                < seqs@[#[trigger] after[k2] as int] by {
                if k1 < p && k2 == p {
                } else if k1 < p && k2 > p {
                    assert(after[k2] == before[k2 - 1]);
                } else if k1 == p {
                    assert(after[k2] == before[k2 - 1]);
                } else if k1 > p {
                    assert(after[k1] == before[k1 - 1]);
                    assert(after[k2] == before[k2 - 1]);
                }
            }
        }
        i += 1;
    }
    chron
}

/// Any sequence of operations from any state satisfying `inv` keeps `inv`:
/// `kinds[k]` selects add (0), removal of the live activation at `indexes[k]`
/// (1, when it exists) or clear (2); other values are ignored. The start state
/// may have any `next_seq` (including `u64::MAX`), so the exhaustion rebase is
/// reachable here; the only bound is that the live count stays below
/// `u64::MAX` (see the module documentation).
pub fn run_ops_from(st: &mut SeqState, kinds: &Vec<u8>, indexes: &Vec<usize>)
    requires
        old(st).inv(),
        kinds@.len() == indexes@.len(),
        old(st).seqs@.len() + kinds@.len() < u64::MAX,
    ensures
        final(st).inv(),
{
    let ghost n0 = st.seqs@.len();
    let mut k: usize = 0;
    while k < kinds.len()
        invariant
            st.inv(),
            0 <= k <= kinds@.len(),
            st.seqs@.len() <= n0 + k,
            kinds@.len() == indexes@.len(),
            n0 + kinds@.len() < u64::MAX,
        decreases kinds@.len() - k,
    {
        let kind = kinds[k];
        if kind == 0 {
            let chron = chronological(&st.seqs);
            st.add(&chron);
        } else if kind == 1 && indexes[k] < st.seqs.len() {
            st.remove(indexes[k]);
        } else if kind == 2 {
            st.clear();
        }
        k += 1;
    }
}

/// Any sequence of operations from a new agenda keeps `inv`.
pub fn run_ops(kinds: &Vec<u8>, indexes: &Vec<usize>) -> (st: SeqState)
    requires
        kinds@.len() == indexes@.len(),
        kinds@.len() < u64::MAX,
    ensures
        st.inv(),
{
    let mut st = SeqState::new();
    run_ops_from(&mut st, kinds, indexes);
    st
}

/// Witness that the exhaustion path is reachable and behaves as the
/// production unit test `sequence_exhaustion_preserves_live_chronology_and_indexes`
/// (agenda.rs:685) expects: from `next_seq == u64::MAX - 2`, three adds give
/// `u64::MAX - 2`, `u64::MAX - 1`, then (after rebasing the two live
/// activations to 0 and 1) 2, leaving `next_seq == 3` and the live sequences
/// in creation order.
pub fn exhaustion_example() {
    let mut st = SeqState {
        seqs: Vec::new(),
        next_seq: u64::MAX - 2,
        created: Ghost(Seq::empty()),
        clock: Ghost(0),
    };
    let chron = chronological(&st.seqs);
    let s0 = st.add(&chron);
    let chron = chronological(&st.seqs);
    let s1 = st.add(&chron);
    let ghost before = st.seqs@;
    assert(before[0] == u64::MAX - 2 && before[1] == u64::MAX - 1);
    let chron = chronological(&st.seqs);
    let s2 = st.add(&chron);
    assert(s0 == u64::MAX - 2 && s1 == u64::MAX - 1);
    assert(s2 == 2);
    assert(st.next_seq == 3);
    assert(st.seqs@.len() == 3);
    // Order of the two live activations is kept, and both are below 2.
    assert(st.seqs@[0] < st.seqs@[1] && st.seqs@[1] < 2);
    assert(st.seqs@[0] == 0 && st.seqs@[1] == 1 && st.seqs@[2] == 2);
}

} // verus!
