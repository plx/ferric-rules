//! Verus model of `crates/ferric-rules-core/src/ordered_set.rs`: `OrderedSet<K>`
//! (insertion-ordered membership with a header-only small representation that
//! is promoted to a hash table of doubly linked `Links`) and its double-ended
//! `Iter`, instantiated at `K = u64`.
//!
//! Check with `verus --crate-type=lib verification/verus/ordered_set.rs`.
//!
//! # What is proved (unbounded: every reachable state, every size)
//!
//! * Representation invariant. `OrderedSet::inv` relates the concrete fields
//!   (table contents, the capacity the table reports, `first`, `last`) to the
//!   ghost view `self@: Seq<u64>`: while the table reports capacity 0 it is
//!   empty and the endpoints hold the at most two members (`small_rep`);
//!   otherwise the table maps exactly the members, each to the
//!   `previous`/`next` links of its position, and `first`/`last` are the ends
//!   (`linked_rep`). The view never has duplicates. `new` establishes it and
//!   every operation preserves it, so it holds in every reachable state.
//! * Refinement of the abstract insertion-ordered set:
//!   - `insert(k)` returns `!old@.contains(k)`; the view becomes
//!     `old@.push(k)` if it returns true and is unchanged otherwise. This
//!     covers all four header-only branches (first member, duplicate of an
//!     endpoint, second member, promotion via `reserve(3)` plus three table
//!     inserts) and the linked branch.
//!   - `remove(k)` returns `old@.contains(k)`; the view becomes
//!     `old@.remove(old@.index_of(k))` (the unique position) if it returns
//!     true and is unchanged otherwise. `lemma_remove_contract_is_filter`
//!     shows that either way the new view is `old@.filter(|x| x != k)`.
//!   - `contains`, `len`, `is_empty` agree with the view; `clear` empties it.
//!   - `iter`: `next`/`next_back` return the front/back of the unvisited
//!     window `view[lo..hi]` and shrink it, and return `None` exactly when it
//!     is empty, for any interleaving; `size_hint` (hence
//!     `ExactSizeIterator::len`) is exactly `hi - lo`. `collect_forward`
//!     yields the view and `collect_backward` yields `view.reverse()`.
//!   - `run_trace`: for every trace of `insert`/`remove`/`clear` from
//!     `default()`, every result and the final view equal the position-free
//!     abstract semantics `spec_run` (insert appends iff absent, remove
//!     filters the key out, clear empties).
//!   - `decode` (snapshot `Deserialize`): accepts exactly the duplicate-free
//!     sequences, and then the view equals the decoded sequence.
//! * Panic freedom of the modeled control flow: every production `unwrap()`
//!   (crates/ferric-rules-core/src/ordered_set.rs:49, 50, 84, 113, 118) is applied to
//!   `Some`; the `?` early returns of `next`/`next_back` (lines 174, 185, 188, 203, 212,
//!   215) are never taken while `remaining > 0`; `remaining -= 1` never underflows; the
//!   `debug_assert_eq!(capacity(), 0)` at lines 182 and 209 never fires (a table miss on
//!   a remaining member happens only in the header-only representation); `reserve(3)`
//!   never overflows.
//! * Shape of the linked structure (`lemma_linked_walks`): following `next`
//!   from `first` visits `v[0], ..., v[len-1]` (pairwise distinct, and
//!   exactly the table keys) and then reaches `None`; following `previous`
//!   from `last` visits them in reverse and then reaches `None`. So the
//!   list is acyclic and covers exactly the keys.
//! * The ghost view adds no information (`lemma_rep_is_abs`,
//!   `lemma_view_determined`): it equals `abs(...)`, a function of the
//!   concrete fields alone.
//!
//! # Abstractions and simplifications (each one is also noted at its site)
//!
//! * `K = u64`. Production is generic over `K: Copy + Eq + Hash` and only
//!   compares and hashes keys; vstd's hash-map model needs a key type with
//!   its key-model axiom, which vstd provides for `u64`.
//! * The table is `Table`: vstd's `HashMapWithView<u64, Links>` plus an exec
//!   field `cap`, the value `HashMap::capacity()` reports. Every mutating
//!   table call sets `cap` from a free input (`cap_after`, supplied through
//!   `Caps`), clamped only by what the std contract guarantees: capacity is
//!   at least the length, and at least `len + additional` after
//!   `reserve(additional)`. The proofs therefore hold for every capacity
//!   trajectory, including the allocated-but-zero-capacity state that
//!   tombstone-heavy removal can leave (production comment at lines 33-35),
//!   and do not depend on `default()` reporting 0. Non-mutating calls leave
//!   `cap` unchanged, as in hashbrown (`items + growth_left`).
//! * `*get_mut(&k).unwrap()` field writes are `get` (a copy, whose `unwrap`
//!   is checked) followed by `set_links` (overwrite of a present key; length
//!   and capacity unchanged).
//! * `entries.entry(key)` + `VacantEntry::insert` is `contains_key` followed
//!   by `insert`, with one capacity change (the `reserve(1)` inside `entry`
//!   and the insertion).
//! * `get_key_value(&k)` returns the stored key, which for `u64` is `k`; the
//!   model returns the probe key and only reads the links.
//! * `usize::from(bool)` is written `if b { 1 } else { 0 }`; the iterator
//!   yields `u64` by value instead of `&'a K`.
//! * `Deserialize` is modeled after serde has produced the `Vec<K>`;
//!   `Serialize` (`collect_seq(self.iter())`) is `collect_forward`.
//!
//! # Trusted base
//!
//! Verus, Z3, and vstd (in particular the `HashMapWithView` specification of
//! `std::collections::HashMap` and vstd's axiom that `u64` obeys the hash-table
//! key model). This file contains no `assume`, `admit`, `external_body`,
//! `external`, or `assume_specification`.
//!
//! # Known gaps
//!
//! * This is a hand transcription: fidelity to the production source rests
//!   on line-by-line comparison (each function cites its counterpart), not on
//!   compiling the production code. `ordered_set/kani_refinement.rs` checks
//!   the production code itself against the same specification on bounded
//!   traces.
//! * Hashbrown internals (probing, tombstones, rehashing, SIMD groups) are
//!   abstracted by vstd's map specification and the capacity contract above.
//! * Only `K = u64`; allocation retention ("promoted sets retain their
//!   allocation") is a performance property and is not modeled.
use vstd::hash_map::HashMapWithView;
use vstd::prelude::*;

verus! {

// ---------------------------------------------------------------------------
// Abstract helpers over the abstract view `v: Seq<u64>`.
// ---------------------------------------------------------------------------
/// `Some(v[i])` inside the sequence, `None` outside it (both ends).
pub open spec fn at(v: Seq<u64>, i: int) -> Option<u64> {
    if 0 <= i < v.len() {
        Some(v[i])
    } else {
        None
    }
}

/// Models `struct Links<K>` (crates/ferric-rules-core/src/ordered_set.rs:10).
#[derive(Clone, Copy)]
pub struct Links {
    pub previous: Option<u64>,
    pub next: Option<u64>,
}

/// The links the member at position `i` of `v` must carry.
pub open spec fn links_at(v: Seq<u64>, i: int) -> Links {
    Links { previous: at(v, i - 1), next: at(v, i + 1) }
}

/// Header-only representation used while the table reports zero capacity:
/// at most two members, held by `first` and `last` (equal for one member).
pub open spec fn small_rep(first: Option<u64>, last: Option<u64>, v: Seq<u64>) -> bool {
    &&& v.len() <= 2
    &&& v.no_duplicates()
    &&& first == at(v, 0)
    &&& last == at(v, v.len() - 1)
}

/// Doubly linked representation: the table maps exactly the members of `v`,
/// each to the links of its position, and `first`/`last` are the ends.
pub open spec fn linked_rep(
    entries: Map<u64, Links>,
    first: Option<u64>,
    last: Option<u64>,
    v: Seq<u64>,
) -> bool {
    &&& v.no_duplicates()
    &&& first == at(v, 0)
    &&& last == at(v, v.len() - 1)
    &&& forall|k: u64| #[trigger] entries.contains_key(k) ==> v.contains(k)
    &&& forall|i: int|
        #![trigger v[i]]
        0 <= i < v.len() ==> entries.contains_key(v[i]) && entries[v[i]] == links_at(v, i)
}

/// The representation invariant, as a relation between the concrete state
/// (table contents, table capacity, endpoints) and the abstract view.
pub open spec fn rep(
    entries: Map<u64, Links>,
    cap: nat,
    first: Option<u64>,
    last: Option<u64>,
    v: Seq<u64>,
) -> bool {
    &&& cap >= entries.len()
    &&& if cap == 0 {
        &&& entries == Map::<u64, Links>::empty()
        &&& small_rep(first, last, v)
    } else {
        linked_rep(entries, first, last, v)
    }
}

// ---------------------------------------------------------------------------
// The hash table: `entries: FxHashMap<K, Links<K>>`.
// ---------------------------------------------------------------------------
/// The capacities the hash table reports after each table-mutating call that
/// one `OrderedSet` operation makes (at most four: `reserve` plus three
/// `insert`s during promotion). They are free inputs, so every theorem below
/// holds for every capacity behavior allowed by the `HashMap` contract.
#[derive(Clone, Copy)]
pub struct Caps {
    pub c0: usize,
    pub c1: usize,
    pub c2: usize,
    pub c3: usize,
}

/// Models `entries: HashMap<K, Links<K>>`
/// (crates/ferric-rules-core/src/ordered_set.rs:16) together with the value its
/// `capacity()` reports. `cap` is only changed by mutating calls, so repeated
/// `capacity()` calls on an unchanged table agree.
pub struct Table {
    pub map: HashMapWithView<u64, Links>,
    pub cap: usize,
}

impl View for Table {
    type V = Map<u64, Links>;

    open spec fn view(&self) -> Map<u64, Links> {
        self.map@
    }
}

pub open spec fn max_usize(a: usize, b: usize) -> usize {
    if a >= b {
        a
    } else {
        b
    }
}

impl Table {
    /// `HashMap::capacity()` is never below `len()`.
    pub open spec fn wf(&self) -> bool {
        self.cap >= self@.len()
    }

    /// `HashMap::default()`. The initial capacity is a free input (std
    /// reports 0 today; the proofs do not depend on it).
    pub fn new(cap_after: usize) -> (t: Table)
        ensures
            t.wf(),
            t@ == Map::<u64, Links>::empty(),
            t.cap == cap_after,
    {
        Table { map: HashMapWithView::new(), cap: cap_after }
    }

    /// `HashMap::capacity()`.
    pub fn capacity(&self) -> (c: usize)
        ensures
            c == self.cap,
    {
        self.cap
    }

    /// `HashMap::len()`.
    pub fn len(&self) -> (n: usize)
        ensures
            n == self@.len(),
    {
        self.map.len()
    }

    /// `HashMap::contains_key()`.
    pub fn contains_key(&self, k: &u64) -> (b: bool)
        ensures
            b == self@.contains_key(*k),
    {
        self.map.contains_key(k)
    }

    /// `HashMap::get()` / `get_key_value()` (the stored key equals the probe
    /// key for `u64`, so only the links are returned, by copy).
    pub fn get(&self, k: &u64) -> (r: Option<Links>)
        ensures
            r == (if self@.contains_key(*k) {
                Some(self@[*k])
            } else {
                None
            }),
    {
        match self.map.get(k) {
            Some(l) => Some(*l),
            None => None,
        }
    }

    /// `HashMap::insert()` and `VacantEntry::insert()`: afterwards the table
    /// reports any capacity that is at least its length.
    pub fn insert(&mut self, k: u64, v: Links, cap_after: usize)
        requires
            old(self).wf(),
        ensures
            final(self).wf(),
            final(self)@ == old(self)@.insert(k, v),
            final(self).cap == max_usize(cap_after, final(self)@.len() as usize),
    {
        self.map.insert(k, v);
        let n = self.map.len();
        self.cap = if cap_after >= n {
            cap_after
        } else {
            n
        };
    }

    /// `*self.get_mut(&k).unwrap() = v` for a present key: a write through
    /// `get_mut`, which never changes the capacity.
    pub fn set_links(&mut self, k: u64, v: Links)
        requires
            old(self).wf(),
            old(self)@.contains_key(k),
        ensures
            final(self).wf(),
            final(self)@ == old(self)@.insert(k, v),
            final(self).cap == old(self).cap,
    {
        let ghost before = self@;
        self.map.insert(k, v);
        assert(self@.dom() =~= before.dom());
    }

    /// `HashMap::remove()`: afterwards the table reports any capacity that is
    /// at least its length (removal can leave tombstones that lower it).
    pub fn remove(&mut self, k: &u64, cap_after: usize) -> (r: Option<Links>)
        requires
            old(self).wf(),
        ensures
            final(self).wf(),
            r == (if old(self)@.contains_key(*k) {
                Some(old(self)@[*k])
            } else {
                None
            }),
            final(self)@ == old(self)@.remove(*k),
            final(self).cap == max_usize(cap_after, final(self)@.len() as usize),
    {
        let r = self.map.remove(k);
        let n = self.map.len();
        self.cap = if cap_after >= n {
            cap_after
        } else {
            n
        };
        proof {
            if r is None {
                assert(self@ =~= old(self)@.remove(*k));
            }
        }
        r
    }

    /// `HashMap::reserve()`: afterwards the table reports a capacity of at
    /// least `len + additional` (any such value).
    pub fn reserve(&mut self, additional: usize, cap_after: usize)
        requires
            old(self).wf(),
            old(self)@.len() + additional <= usize::MAX,
        ensures
            final(self).wf(),
            final(self)@ == old(self)@,
            final(self).cap >= final(self)@.len() + additional,
            final(self).cap == max_usize(cap_after, (old(self)@.len() + additional) as usize),
    {
        self.map.reserve(additional);
        let need = self.map.len() + additional;
        self.cap = if cap_after >= need {
            cap_after
        } else {
            need
        };
    }

    /// `HashMap::clear()`: afterwards the table reports any capacity (it keeps
    /// its allocation, or has none).
    pub fn clear(&mut self, cap_after: usize)
        requires
            old(self).wf(),
        ensures
            final(self).wf(),
            final(self)@ == Map::<u64, Links>::empty(),
            final(self).cap == cap_after,
    {
        self.map.clear();
        self.cap = cap_after;
    }
}

// ---------------------------------------------------------------------------
// The ordered set.
// ---------------------------------------------------------------------------
/// Models `struct OrderedSet<K>` (crates/ferric-rules-core/src/ordered_set.rs:15)
/// with `K = u64`, plus a ghost abstract view `elems`: the members in insertion
/// order.
pub struct OrderedSet {
    pub entries: Table,
    pub first: Option<u64>,
    pub last: Option<u64>,
    pub elems: Ghost<Seq<u64>>,
}

impl View for OrderedSet {
    type V = Seq<u64>;

    open spec fn view(&self) -> Seq<u64> {
        self.elems@
    }
}

impl OrderedSet {
    pub open spec fn inv(&self) -> bool {
        &&& self.entries.wf()
        &&& rep(self.entries@, self.entries.cap as nat, self.first, self.last, self@)
    }

    /// Models `impl Default for OrderedSet`
    /// (crates/ferric-rules-core/src/ordered_set.rs:22 fn default).
    pub fn new(caps: Caps) -> (s: OrderedSet)
        ensures
            s.inv(),
            s@ == Seq::<u64>::empty(),
    {
        let s = OrderedSet {
            entries: Table::new(caps.c0),
            first: None,
            last: None,
            elems: Ghost(Seq::empty()),
        };
        assert(s.entries@.len() == 0);
        s
    }

    /// Models `OrderedSet::contains` (crates/ferric-rules-core/src/ordered_set.rs:125 fn contains).
    pub fn contains(&self, key: &u64) -> (r: bool)
        requires
            self.inv(),
        ensures
            r == self@.contains(*key),
    {
        if self.entries.capacity() == 0 {
            proof {
                if self@.contains(*key) {
                    let i = choose|i: int| 0 <= i < self@.len() && self@[i] == *key;
                    assert(i == 0 || i == self@.len() - 1);
                }
            }
            self.first == Some(*key) || self.last == Some(*key)
        } else {
            proof {
                if self@.contains(*key) {
                    let i = choose|i: int| 0 <= i < self@.len() && self@[i] == *key;
                    assert(self.entries@.contains_key(self@[i]));
                }
            }
            self.entries.contains_key(key)
        }
    }

    /// Models `OrderedSet::len` (crates/ferric-rules-core/src/ordered_set.rs:133 fn len).
    /// `usize::from(b)` is written as `if b { 1 } else { 0 }`.
    pub fn len(&self) -> (r: usize)
        requires
            self.inv(),
        ensures
            r == self@.len(),
    {
        if self.entries.capacity() == 0 {
            (if self.first.is_some() {
                1usize
            } else {
                0usize
            }) + (if self.first != self.last {
                1usize
            } else {
                0usize
            })
        } else {
            proof {
                lemma_linked_len(self.entries@, self.first, self.last, self@);
            }
            self.entries.len()
        }
    }

    /// Models `OrderedSet::is_empty` (crates/ferric-rules-core/src/ordered_set.rs:141 fn is_empty).
    pub fn is_empty(&self) -> (r: bool)
        requires
            self.inv(),
        ensures
            r == (self@.len() == 0),
    {
        self.first.is_none()
    }

    /// Models `OrderedSet::clear` (crates/ferric-rules-core/src/ordered_set.rs:145 fn clear).
    pub fn clear(&mut self, caps: Caps)
        requires
            old(self).inv(),
        ensures
            final(self).inv(),
            final(self)@ == Seq::<u64>::empty(),
    {
        self.entries.clear(caps.c0);
        self.first = None;
        self.last = None;
        self.elems = Ghost(Seq::empty());
    }

    /// Models `OrderedSet::insert` (crates/ferric-rules-core/src/ordered_set.rs:32 fn insert).
    pub fn insert(&mut self, key: u64, caps: Caps) -> (r: bool)
        requires
            old(self).inv(),
        ensures
            final(self).inv(),
            r == !old(self)@.contains(key),
            final(self)@ == (if r {
                old(self)@.push(key)
            } else {
                old(self)@
            }),
    {
        let ghost v = self@;
        if self.entries.capacity() == 0 {
            // Header-only representation.
            if self.first.is_none() {
                self.first = Some(key);
                self.last = Some(key);
                self.elems = Ghost(v.push(key));
                return true;
            }
            if self.first == Some(key) || self.last == Some(key) {
                assert(self@[0] == key || self@[self@.len() - 1] == key);
                return false;
            }
            proof {
                lemma_small_not_contains(self.first, self.last, v, key);
            }
            if self.first == self.last {
                self.last = Some(key);
                self.elems = Ghost(v.push(key));
                return true;
            }
            // Promotion: two members in the endpoints plus `key`.
            let first = self.first.unwrap();
            let middle = self.last.unwrap();
            assert(v.len() == 2 && v[0] == first && v[1] == middle);
            self.entries.reserve(3, caps.c0);
            self.entries.insert(first, Links { previous: None, next: Some(middle) }, caps.c1);
            self.entries.insert(middle, Links { previous: Some(first), next: Some(key) }, caps.c2);
            self.entries.insert(key, Links { previous: Some(middle), next: None }, caps.c3);
            self.last = Some(key);
            self.elems = Ghost(v.push(key));
            proof {
                let w = v.push(key);
                assert(w[0] == first && w[1] == middle && w[2] == key);
                assert forall|k: u64| #[trigger] self.entries@.contains_key(k) implies w.contains(
                    k,
                ) by {
                    if k == first {
                        assert(w[0] == k);
                    } else if k == middle {
                        assert(w[1] == k);
                    } else {
                        assert(w[2] == k);
                    }
                }
                lemma_linked_len(self.entries@, self.first, self.last, w);
            }
            return true;
        }
        // Linked representation: `let Entry::Vacant(entry) = self.entries.entry(key) else ...`.

        if self.entries.contains_key(&key) {
            proof {
                lemma_linked_contains(self.entries@, self.first, self.last, v, key);
            }
            return false;
        }
        proof {
            lemma_linked_contains(self.entries@, self.first, self.last, v, key);
        }
        // `entry.insert(Links { previous: self.last, next: None })`.
        self.entries.insert(key, Links { previous: self.last, next: None }, caps.c0);
        match self.last {
            Some(previous) => {
                // `self.entries.get_mut(&previous).unwrap().next = Some(key)`.
                assert(v[v.len() - 1] == previous);
                let mut links = self.entries.get(&previous).unwrap();
                links.next = Some(key);
                self.entries.set_links(previous, links);
            },
            None => {
                self.first = Some(key);
            },
        }
        self.last = Some(key);
        self.elems = Ghost(v.push(key));
        proof {
            let w = v.push(key);
            let n = v.len() as int;
            assert forall|k: u64| #[trigger] self.entries@.contains_key(k) implies w.contains(
                k,
            ) by {
                if k == key {
                    assert(w[n] == k);
                } else {
                    let j = choose|j: int| 0 <= j < v.len() && v[j] == k;
                    assert(w[j] == k);
                }
            }
            assert forall|i: int| #![trigger w[i]]
                0 <= i < w.len() implies self.entries@.contains_key(w[i])
                    && self.entries@[w[i]] == links_at(w, i) by {
                if i < n {
                    assert(w[i] == v[i]);
                    assert(v[i] != key);
                    if i < n - 1 {
                        assert(v[i] != v[n - 1]);
                    }
                }
            }
            lemma_linked_len(self.entries@, self.first, self.last, w);
        }
        true
    }

    /// Models `OrderedSet::remove` (crates/ferric-rules-core/src/ordered_set.rs:92 fn remove).
    pub fn remove(&mut self, key: &u64, caps: Caps) -> (r: bool)
        requires
            old(self).inv(),
        ensures
            final(self).inv(),
            r == old(self)@.contains(*key),
            r ==> final(self)@ == old(self)@.remove(old(self)@.index_of(*key)),
            !r ==> final(self)@ == old(self)@,
    {
        let ghost v = self@;
        if self.entries.capacity() == 0 {
            // Header-only representation.
            if self.first == Some(*key) {
                proof {
                    assert(v[0] == *key);
                    lemma_index_of_unique(v, 0);
                }
                if self.first == self.last {
                    self.first = None;
                    self.last = None;
                } else {
                    self.first = self.last;
                }
                self.elems = Ghost(v.remove(0));
                return true;
            }
            if self.last == Some(*key) {
                proof {
                    assert(v[v.len() - 1] == *key);
                    lemma_index_of_unique(v, v.len() - 1);
                }
                self.last = self.first;
                self.elems = Ghost(v.remove(v.len() - 1));
                return true;
            }
            proof {
                lemma_small_not_contains(self.first, self.last, v, *key);
            }
            return false;
        }
        // Linked representation: `let Some(links) = self.entries.remove(key) else ...`.

        proof {
            lemma_linked_contains(self.entries@, self.first, self.last, v, *key);
        }
        let removed = self.entries.remove(key, caps.c0);
        let links = match removed {
            Some(links) => links,
            None => {
                proof {
                    // The table may now report zero capacity; then it is
                    // empty, so the view is empty and both endpoints are None.
                    lemma_linked_len(self.entries@, self.first, self.last, v);
                    if self.entries.cap == 0 {
                        assert(self.entries@.dom() =~= Set::<u64>::empty());
                        assert(self.entries@ =~= Map::<u64, Links>::empty());
                    }
                }
                return false;
            },
        };
        let ghost i = v.index_of(*key);
        proof {
            assert(v[i] == *key);
            lemma_index_of_unique(v, i);
            assert(links == links_at(v, i));
        }
        let ghost w = v.remove(i);
        let ghost m0 = old(self).entries@;
        assert(self.entries@ == m0.remove(v[i]));
        match links.previous {
            Some(previous) => {
                // `self.entries.get_mut(&previous).unwrap().next = links.next`.
                assert(previous == v[i - 1]);
                assert(v[i - 1] != v[i]);
                let mut previous_links = self.entries.get(&previous).unwrap();
                previous_links.next = links.next;
                self.entries.set_links(previous, previous_links);
            },
            None => {
                self.first = links.next;
            },
        }
        assert(self.entries@ == unlink_previous(m0, v, i));
        match links.next {
            Some(next) => {
                // `self.entries.get_mut(&next).unwrap().previous = links.previous`.
                assert(next == v[i + 1]);
                assert(v[i + 1] != v[i]);
                let mut next_links = self.entries.get(&next).unwrap();
                next_links.previous = links.previous;
                self.entries.set_links(next, next_links);
            },
            None => {
                self.last = links.previous;
            },
        }
        assert(self.entries@ == unlink(m0, v, i));
        self.elems = Ghost(w);
        proof {
            lemma_unlink(m0, old(self).first, old(self).last, v, i);
            lemma_remove_at(v, i);
            lemma_linked_len(self.entries@, self.first, self.last, w);
            if self.entries.cap == 0 {
                assert(self.entries@.dom() =~= Set::<u64>::empty());
                assert(self.entries@ =~= Map::<u64, Links>::empty());
            }
        }
        true
    }
}

// ---------------------------------------------------------------------------
// Iteration.
// ---------------------------------------------------------------------------
/// Models `struct Iter<'a, K>` (crates/ferric-rules-core/src/ordered_set.rs:161). The ghost cursors
/// `lo`/`hi` delimit the still-unvisited window `set@[lo..hi]` of the view.
pub struct Iter<'a> {
    pub set: &'a OrderedSet,
    pub front: Option<u64>,
    pub back: Option<u64>,
    pub remaining: usize,
    pub lo: Ghost<int>,
    pub hi: Ghost<int>,
}

impl<'a> Iter<'a> {
    pub open spec fn inv(&self) -> bool {
        &&& self.set.inv()
        &&& 0 <= self.lo@ <= self.hi@ <= self.set@.len()
        &&& self.remaining == self.hi@ - self.lo@
        &&& self.lo@ < self.hi@ ==> {
            &&& self.front == Some(self.set@[self.lo@])
            &&& self.back == Some(self.set@[self.hi@ - 1])
        }
    }

    /// Models `Iterator::next` for `Iter`
    /// (crates/ferric-rules-core/src/ordered_set.rs:170 fn next).
    pub fn next(&mut self) -> (r: Option<u64>)
        requires
            old(self).inv(),
        ensures
            final(self).inv(),
            final(self).set == old(self).set,
            final(self).hi@ == old(self).hi@,
            old(self).lo@ < old(self).hi@ ==> r == Some(old(self).set@[old(self).lo@])
                && final(self).lo@ == old(self).lo@ + 1,
            old(self).lo@ == old(self).hi@ ==> r is None && final(self).lo@ == old(self).lo@,
    {
        if self.remaining == 0 {
            return None;
        }
        let front = match self.front {
            Some(front) => front,
            None => {
                return None;
            },
        };
        let ghost v = self.set@;
        let ghost lo = self.lo@;
        // `if let Some((key, links)) = self.set.entries.get_key_value(&front)`.
        match self.set.entries.get(&front) {
            Some(links) => {
                assert(self.set.entries.cap != 0);
                assert(links == links_at(v, lo));
                self.front = links.next;
                self.remaining = self.remaining - 1;
                self.lo = Ghost(lo + 1);
                return Some(front);
            },
            None => {},
        }
        // The table missed a remaining member: header-only representation.
        // This is the production `debug_assert_eq!(capacity, 0)` (line 182).
        let capacity = self.set.entries.capacity();
        assert(capacity == 0) by {
            if capacity != 0 {
                assert(self.set.entries@.contains_key(v[lo]));
            }
        };
        let key = if self.set.first == Some(front) {
            self.front = self.set.last;
            match self.set.first {
                Some(k) => k,
                None => {
                    return None;
                },
            }
        } else {
            self.front = None;
            match self.set.last {
                Some(k) => k,
                None => {
                    return None;
                },
            }
        };
        self.remaining = self.remaining - 1;
        self.lo = Ghost(lo + 1);
        proof {
            if v.len() == 2 && lo == 1 {
                assert(v[0] != v[1]);
            }
        }
        Some(key)
    }

    /// Models `DoubleEndedIterator::next_back` for `Iter`
    /// (crates/ferric-rules-core/src/ordered_set.rs:199 fn next_back).
    pub fn next_back(&mut self) -> (r: Option<u64>)
        requires
            old(self).inv(),
        ensures
            final(self).inv(),
            final(self).set == old(self).set,
            final(self).lo@ == old(self).lo@,
            old(self).lo@ < old(self).hi@ ==> r == Some(old(self).set@[old(self).hi@ - 1])
                && final(self).hi@ == old(self).hi@ - 1,
            old(self).lo@ == old(self).hi@ ==> r is None && final(self).hi@ == old(self).hi@,
    {
        if self.remaining == 0 {
            return None;
        }
        let back = match self.back {
            Some(back) => back,
            None => {
                return None;
            },
        };
        let ghost v = self.set@;
        let ghost hi = self.hi@;
        // `if let Some((key, links)) = self.set.entries.get_key_value(&back)`.
        match self.set.entries.get(&back) {
            Some(links) => {
                assert(self.set.entries.cap != 0);
                assert(links == links_at(v, hi - 1));
                self.back = links.previous;
                self.remaining = self.remaining - 1;
                self.hi = Ghost(hi - 1);
                return Some(back);
            },
            None => {},
        }
        // The production `debug_assert_eq!(capacity, 0)` (line 209).
        let capacity = self.set.entries.capacity();
        assert(capacity == 0) by {
            if capacity != 0 {
                assert(self.set.entries@.contains_key(v[hi - 1]));
            }
        };
        let key = if self.set.first == Some(back) {
            self.back = None;
            match self.set.first {
                Some(k) => k,
                None => {
                    return None;
                },
            }
        } else {
            self.back = self.set.first;
            match self.set.last {
                Some(k) => k,
                None => {
                    return None;
                },
            }
        };
        self.remaining = self.remaining - 1;
        self.hi = Ghost(hi - 1);
        proof {
            if v.len() == 2 {
                assert(v[0] != v[1]);
            }
        }
        Some(key)
    }

    /// Models `Iterator::size_hint` for `Iter`
    /// (crates/ferric-rules-core/src/ordered_set.rs:193); the `ExactSizeIterator::len`
    /// (ordered_set.rs:221) is its lower bound.
    pub fn size_hint(&self) -> (r: (usize, Option<usize>))
        requires
            self.inv(),
        ensures
            r.0 == self.hi@ - self.lo@,
            r.1 == Some(r.0),
    {
        (self.remaining, Some(self.remaining))
    }
}

impl OrderedSet {
    /// Models `OrderedSet::iter` (crates/ferric-rules-core/src/ordered_set.rs:151 fn iter).
    pub fn iter<'a>(&'a self) -> (it: Iter<'a>)
        requires
            self.inv(),
        ensures
            it.inv(),
            it.set == self,
            it.lo@ == 0,
            it.hi@ == self@.len(),
    {
        let remaining = self.len();
        Iter {
            set: self,
            front: self.first,
            back: self.last,
            remaining,
            lo: Ghost(0),
            hi: Ghost(self@.len() as int),
        }
    }
}

/// `set.iter().copied().collect::<Vec<_>>()` (also what `Serialize` emits,
/// crates/ferric-rules-core/src/ordered_set.rs:234) yields exactly the view.
pub fn collect_forward(set: &OrderedSet) -> (out: Vec<u64>)
    requires
        set.inv(),
    ensures
        out@ == set@,
{
    let mut it = set.iter();
    let mut out: Vec<u64> = Vec::new();
    loop
        invariant
            it.inv(),
            it.set == set,
            it.hi@ == set@.len(),
            out@ == set@.subrange(0, it.lo@),
        decreases it.hi@ - it.lo@,
    {
        let ghost lo = it.lo@;
        match it.next() {
            Some(k) => {
                out.push(k);
                assert(out@ =~= set@.subrange(0, it.lo@));
            },
            None => {
                assert(lo == set@.len());
                assert(out@ =~= set@);
                return out;
            },
        }
    }
}

/// `set.iter().rev().copied().collect::<Vec<_>>()` yields exactly the
/// reversed view.
pub fn collect_backward(set: &OrderedSet) -> (out: Vec<u64>)
    requires
        set.inv(),
    ensures
        out@ == set@.reverse(),
{
    let mut it = set.iter();
    let mut out: Vec<u64> = Vec::new();
    loop
        invariant
            it.inv(),
            it.set == set,
            it.lo@ == 0,
            out.len() == set@.len() - it.hi@,
            forall|j: int| 0 <= j < out.len() ==> out@[j] == set@[set@.len() - 1 - j],
        decreases it.hi@ - it.lo@,
    {
        let ghost hi = it.hi@;
        match it.next_back() {
            Some(k) => {
                out.push(k);
            },
            None => {
                assert(hi == 0);
                assert(out@ =~= set@.reverse());
                return out;
            },
        }
    }
}

/// The table after `remove` deleted member `v[i]` and the predecessor's
/// `next` link was redirected (crates/ferric-rules-core/src/ordered_set.rs:109-116).
pub open spec fn unlink_previous(m0: Map<u64, Links>, v: Seq<u64>, i: int) -> Map<u64, Links> {
    let m1 = m0.remove(v[i]);
    if i > 0 {
        m1.insert(v[i - 1], Links { previous: m1[v[i - 1]].previous, next: at(v, i + 1) })
    } else {
        m1
    }
}

/// ... and the successor's `previous` link was redirected
/// (crates/ferric-rules-core/src/ordered_set.rs:117-121).
pub open spec fn unlink(m0: Map<u64, Links>, v: Seq<u64>, i: int) -> Map<u64, Links> {
    let m2 = unlink_previous(m0, v, i);
    if i + 1 < v.len() {
        m2.insert(v[i + 1], Links { previous: at(v, i - 1), next: m2[v[i + 1]].next })
    } else {
        m2
    }
}

/// Unlinking position `i` from a well-formed linked representation yields
/// the linked representation of `v.remove(i)`.
pub proof fn lemma_unlink(
    m0: Map<u64, Links>,
    first: Option<u64>,
    last: Option<u64>,
    v: Seq<u64>,
    i: int,
)
    requires
        linked_rep(m0, first, last, v),
        0 <= i < v.len(),
    ensures
        linked_rep(
            unlink(m0, v, i),
            if i == 0 {
                at(v, 1)
            } else {
                first
            },
            if i + 1 == v.len() {
                at(v, i - 1)
            } else {
                last
            },
            v.remove(i),
        ),
{
    let m = unlink(m0, v, i);
    let w = v.remove(i);
    lemma_remove_at(v, i);
    assert forall|k: u64| #[trigger] m.contains_key(k) implies w.contains(k) by {
        let j = choose|j: int| 0 <= j < v.len() && v[j] == k;
        assert(j != i);
        if j < i {
            assert(w[j] == k);
        } else {
            assert(w[j - 1] == k);
        }
    }
    assert forall|j: int| #![trigger w[j]] 0 <= j < w.len() implies m.contains_key(w[j])
        && m[w[j]] == links_at(w, j) by {
        lemma_unlink_at(m0, first, last, v, i, j);
    }
}

/// One position of `lemma_unlink`: the member at position `j` of
/// `v.remove(i)` carries the links of that position after unlinking.
pub proof fn lemma_unlink_at(
    m0: Map<u64, Links>,
    first: Option<u64>,
    last: Option<u64>,
    v: Seq<u64>,
    i: int,
    j: int,
)
    requires
        linked_rep(m0, first, last, v),
        0 <= i < v.len(),
        0 <= j < v.len() - 1,
    ensures
        unlink(m0, v, i).contains_key(v.remove(i)[j]),
        unlink(m0, v, i)[v.remove(i)[j]] == links_at(v.remove(i), j),
{
    let w = v.remove(i);
    let m1 = m0.remove(v[i]);
    let m2 = unlink_previous(m0, v, i);
    let m = unlink(m0, v, i);
    lemma_remove_at(v, i);
    if j < i {
        let k = v[j];
        assert(w[j] == k);
        assert(k != v[i]);
        assert(m1.contains_key(k) && m1[k] == links_at(v, j));
        if j == i - 1 {
            assert(m2[k] == Links { previous: at(v, j - 1), next: at(v, i + 1) });
        } else {
            assert(k != v[i - 1]);
            assert(m2[k] == m1[k]);
        }
        if i + 1 < v.len() {
            assert(k != v[i + 1]);
        }
        assert(m[k] == m2[k]);
        assert(at(w, j - 1) == at(v, j - 1));
        if j + 1 < i {
            assert(at(w, j + 1) == at(v, j + 1));
        } else {
            assert(at(w, j + 1) == at(v, i + 1));
        }
    } else {
        let k = v[j + 1];
        assert(w[j] == k);
        assert(k != v[i]);
        assert(m1.contains_key(k) && m1[k] == links_at(v, j + 1));
        if i > 0 {
            assert(k != v[i - 1]);
        }
        assert(m2.contains_key(k) && m2[k] == m1[k]);
        if j == i {
            assert(m[k] == Links { previous: at(v, i - 1), next: at(v, j + 2) });
            assert(at(w, j - 1) == at(v, i - 1));
        } else {
            assert(k != v[i + 1]);
            assert(m[k] == m2[k]);
            assert(at(w, j - 1) == at(v, j));
        }
        assert(at(w, j + 1) == at(v, j + 2));
    }
}

// ---------------------------------------------------------------------------
// The linked structure on its own: acyclicity and the abstraction function.
// ---------------------------------------------------------------------------
/// The key reached from `from` after `n` steps along `next` links, or None
/// once a walk steps past the end (or off the table).
pub open spec fn walk_next(m: Map<u64, Links>, from: Option<u64>, n: nat) -> Option<u64>
    decreases n,
{
    if n == 0 {
        from
    } else {
        match from {
            Some(k) => if m.contains_key(k) {
                walk_next(m, m[k].next, (n - 1) as nat)
            } else {
                None
            },
            None => None,
        }
    }
}

/// The key reached from `from` after `n` steps along `previous` links.
pub open spec fn walk_previous(m: Map<u64, Links>, from: Option<u64>, n: nat) -> Option<u64>
    decreases n,
{
    if n == 0 {
        from
    } else {
        match from {
            Some(k) => if m.contains_key(k) {
                walk_previous(m, m[k].previous, (n - 1) as nat)
            } else {
                None
            },
            None => None,
        }
    }
}

pub open spec fn key_or_zero(o: Option<u64>) -> u64 {
    match o {
        Some(k) => k,
        None => 0,
    }
}

/// The abstraction function: the member sequence a concrete state denotes,
/// computed from the concrete state alone (no ghost view involved).
pub open spec fn abs(m: Map<u64, Links>, cap: nat, first: Option<u64>, last: Option<u64>) -> Seq<
    u64,
> {
    if cap == 0 {
        match (first, last) {
            (Some(a), Some(b)) => if a == b {
                seq![a]
            } else {
                seq![a, b]
            },
            _ => Seq::empty(),
        }
    } else {
        Seq::new(m.len(), |i: int| key_or_zero(walk_next(m, first, i as nat)))
    }
}

/// Walking `n` steps forward from position `i` lands on position `i + n`
/// (None exactly at the end).
pub proof fn lemma_walk_next_from(
    m: Map<u64, Links>,
    first: Option<u64>,
    last: Option<u64>,
    v: Seq<u64>,
    i: int,
    n: nat,
)
    requires
        linked_rep(m, first, last, v),
        0 <= i <= v.len(),
        i + n <= v.len(),
    ensures
        walk_next(m, at(v, i), n) == at(v, i + n),
    decreases n,
{
    if n > 0 {
        assert(m.contains_key(v[i]) && m[v[i]] == links_at(v, i));
        lemma_walk_next_from(m, first, last, v, i + 1, (n - 1) as nat);
    }
}

/// Walking `n` steps backward from position `i` lands on position `i - n`.
pub proof fn lemma_walk_previous_from(
    m: Map<u64, Links>,
    first: Option<u64>,
    last: Option<u64>,
    v: Seq<u64>,
    i: int,
    n: nat,
)
    requires
        linked_rep(m, first, last, v),
        -1 <= i < v.len(),
        i - n >= -1,
    ensures
        walk_previous(m, at(v, i), n) == at(v, i - n),
    decreases n,
{
    if n > 0 {
        assert(m.contains_key(v[i]) && m[v[i]] == links_at(v, i));
        lemma_walk_previous_from(m, first, last, v, i - 1, (n - 1) as nat);
    }
}

/// Acyclicity and coverage of the linked representation: following `next`
/// from `first` visits exactly `v[0], v[1], ..., v[len-1]` (pairwise
/// distinct, and every table key) and then stops; following `previous`
/// from `last` visits the same keys in reverse and then stops.
pub proof fn lemma_linked_walks(
    m: Map<u64, Links>,
    first: Option<u64>,
    last: Option<u64>,
    v: Seq<u64>,
)
    requires
        linked_rep(m, first, last, v),
    ensures
        forall|n: nat| n < v.len() ==> #[trigger] walk_next(m, first, n) == Some(v[n as int]),
        walk_next(m, first, v.len()) is None,
        forall|n: nat|
            n < v.len() ==> #[trigger] walk_previous(m, last, n) == Some(v[v.len() - 1 - n]),
        walk_previous(m, last, v.len()) is None,
        forall|a: nat, b: nat|
            a < b < v.len() ==> #[trigger] walk_next(m, first, a) != #[trigger] walk_next(
                m,
                first,
                b,
            ),
        forall|k: u64|
            m.contains_key(k) <==> exists|n: nat| n < v.len() && #[trigger] walk_next(m, first, n)
                == Some(k),
{
    assert forall|n: nat| n <= v.len() implies #[trigger] walk_next(m, first, n) == at(
        v,
        n as int,
    ) by {
        lemma_walk_next_from(m, first, last, v, 0, n);
    }
    assert forall|n: nat| n <= v.len() implies #[trigger] walk_previous(m, last, n) == at(
        v,
        v.len() - 1 - n,
    ) by {
        lemma_walk_previous_from(m, first, last, v, v.len() - 1, n);
    }
    assert(walk_next(m, first, v.len()) == at(v, v.len() as int));
    assert(walk_previous(m, last, v.len()) == at(v, -1));
    assert forall|k: u64|
        m.contains_key(k) <==> exists|n: nat| n < v.len() && #[trigger] walk_next(m, first, n)
            == Some(k) by {
        if m.contains_key(k) {
            let j = choose|j: int| 0 <= j < v.len() && v[j] == k;
            assert(walk_next(m, first, j as nat) == at(v, j));
        }
        if exists|n: nat| n < v.len() && #[trigger] walk_next(m, first, n) == Some(k) {
            let n = choose|n: nat| n < v.len() && #[trigger] walk_next(m, first, n) == Some(k);
            assert(walk_next(m, first, n) == at(v, n as int));
            assert(m.contains_key(v[n as int]));
        }
    }
}

/// The ghost view is not extra information: every view related to a
/// concrete state by `rep` equals `abs` of that state. In particular two
/// views of the same concrete state are equal.
pub proof fn lemma_rep_is_abs(
    m: Map<u64, Links>,
    cap: nat,
    first: Option<u64>,
    last: Option<u64>,
    v: Seq<u64>,
)
    requires
        rep(m, cap, first, last, v),
    ensures
        v == abs(m, cap, first, last),
{
    if cap == 0 {
        if v.len() == 2 {
            assert(v[0] != v[1]);
            assert(v =~= seq![v[0], v[1]]);
        } else if v.len() == 1 {
            assert(v =~= seq![v[0]]);
        } else {
            assert(v =~= Seq::<u64>::empty());
        }
    } else {
        lemma_linked_len(m, first, last, v);
        lemma_linked_walks(m, first, last, v);
        let a = abs(m, cap, first, last);
        assert forall|i: int| 0 <= i < v.len() implies a[i] == v[i] by {
            assert(walk_next(m, first, i as nat) == Some(v[i]));
        }
        assert(v =~= a);
    }
}

/// Corollary of `lemma_rep_is_abs`: the view of a valid set is a function
/// of its concrete fields.
pub proof fn lemma_view_determined(s1: &OrderedSet, s2: &OrderedSet)
    requires
        s1.inv(),
        s2.inv(),
        s1.entries@ == s2.entries@,
        s1.entries.cap == s2.entries.cap,
        s1.first == s2.first,
        s1.last == s2.last,
    ensures
        s1@ == s2@,
{
    lemma_rep_is_abs(s1.entries@, s1.entries.cap as nat, s1.first, s1.last, s1@);
    lemma_rep_is_abs(s2.entries@, s2.entries.cap as nat, s2.first, s2.last, s2@);
}

// ---------------------------------------------------------------------------
// Whole operation traces.
// ---------------------------------------------------------------------------
/// One mutating operation of a client trace.
#[derive(Clone, Copy)]
pub enum Op {
    Insert(u64),
    Remove(u64),
    Clear,
}

/// Abstract semantics of one operation on an insertion-ordered set, stated
/// without positions: insert appends an absent key, remove keeps every other
/// member in order (`filter`), clear empties.
pub open spec fn spec_apply(v: Seq<u64>, op: Op) -> Seq<u64> {
    match op {
        Op::Insert(k) => if v.contains(k) {
            v
        } else {
            v.push(k)
        },
        Op::Remove(k) => v.filter(other_than(k)),
        Op::Clear => Seq::empty(),
    }
}

/// The predicate "is not `k`".
pub open spec fn other_than(k: u64) -> spec_fn(u64) -> bool {
    |x: u64| x != k
}

/// Abstract return value of one operation (`clear` returns nothing; `true`).
pub open spec fn spec_result(v: Seq<u64>, op: Op) -> bool {
    match op {
        Op::Insert(k) => !v.contains(k),
        Op::Remove(k) => v.contains(k),
        Op::Clear => true,
    }
}

/// Abstract state after a prefix of a trace, starting from the empty set.
pub open spec fn spec_run(ops: Seq<Op>) -> Seq<u64>
    decreases ops.len(),
{
    if ops.len() == 0 {
        Seq::empty()
    } else {
        spec_apply(spec_run(ops.drop_last()), ops.last())
    }
}

/// Removing the unique occurrence of `v[i]` equals filtering it out.
pub proof fn lemma_remove_is_filter(v: Seq<u64>, i: int)
    requires
        0 <= i < v.len(),
        v.no_duplicates(),
    ensures
        v.remove(i) == v.filter(other_than(v[i])),
    decreases v.len(),
{
    let k = v[i];
    reveal_with_fuel(Seq::filter, 1);
    let front = v.drop_last();
    if i == v.len() - 1 {
        // Every member of `front` differs from `k`, so filtering keeps it.
        lemma_filter_keeps_all(front, k);
        assert(v.remove(i) =~= front);
    } else {
        assert forall|a: int, b: int|
            0 <= a < front.len() && 0 <= b < front.len() && a != b implies front[a]
                != front[b] by {
            assert(front[a] == v[a] && front[b] == v[b]);
        }
        lemma_remove_is_filter(front, i);
        assert(front[i] == k);
        assert(v.last() != k);
        assert(v.remove(i) =~= front.remove(i).push(v.last()));
    }
}

/// Corollary of the `remove` contract: on a duplicate-free view, the view
/// after `remove(k)` is the old view with `k` filtered out, whatever the
/// result.
pub proof fn lemma_remove_contract_is_filter(before: Seq<u64>, after: Seq<u64>, k: u64, r: bool)
    requires
        before.no_duplicates(),
        r == before.contains(k),
        r ==> after == before.remove(before.index_of(k)),
        !r ==> after == before,
    ensures
        after == before.filter(other_than(k)),
{
    if r {
        let j = before.index_of(k);
        assert(before[j] == k);
        lemma_remove_is_filter(before, j);
    } else {
        lemma_filter_keeps_all(before, k);
    }
}

/// Filtering out a key that does not occur keeps the sequence.
pub proof fn lemma_filter_keeps_all(v: Seq<u64>, k: u64)
    requires
        !v.contains(k),
    ensures
        v.filter(other_than(k)) == v,
    decreases v.len(),
{
    reveal_with_fuel(Seq::filter, 1);
    if v.len() > 0 {
        let front = v.drop_last();
        assert(!front.contains(k)) by {
            if front.contains(k) {
                let j = choose|j: int| 0 <= j < front.len() && front[j] == k;
                assert(v[j] == k);
            }
        }
        lemma_filter_keeps_all(front, k);
        assert(v.last() != k) by {
            assert(v[v.len() - 1] == v.last());
        }
        assert(front.push(v.last()) =~= v);
    }
}

/// Runs a whole trace on the model, starting from `OrderedSet::default()`.
/// Every result and the final view agree with the abstract semantics, so
/// the production algorithm refines the abstract set on every trace.
pub fn run_trace(ops: &Vec<Op>, caps: &Vec<Caps>) -> (out: (OrderedSet, Vec<bool>))
    requires
        caps.len() == ops.len() + 1,
    ensures
        out.0.inv(),
        out.0@ == spec_run(ops@),
        out.1.len() == ops.len(),
        forall|i: int|
            #![trigger out.1@[i]]
            0 <= i < ops.len() ==> out.1@[i] == spec_result(spec_run(ops@.subrange(0, i)), ops@[i]),
{
    let mut set = OrderedSet::new(caps[0]);
    let mut results: Vec<bool> = Vec::new();
    let mut i: usize = 0;
    assert(ops@.subrange(0, 0) =~= Seq::<Op>::empty());
    while i < ops.len()
        invariant
            caps.len() == ops.len() + 1,
            0 <= i <= ops.len(),
            set.inv(),
            set@ == spec_run(ops@.subrange(0, i as int)),
            results.len() == i,
            forall|j: int|
                #![trigger results@[j]]
                0 <= j < i ==> results@[j] == spec_result(spec_run(ops@.subrange(0, j)), ops@[j]),
        decreases ops.len() - i,
    {
        let ghost before = set@;
        let op = ops[i];
        let r = match op {
            Op::Insert(k) => set.insert(k, caps[i + 1]),
            Op::Remove(k) => {
                let r = set.remove(&k, caps[i + 1]);
                proof {
                    lemma_remove_contract_is_filter(before, set@, k, r);
                }
                r
            },
            Op::Clear => {
                set.clear(caps[i + 1]);
                true
            },
        };
        proof {
            let prefix = ops@.subrange(0, i + 1);
            assert(prefix.drop_last() =~= ops@.subrange(0, i as int));
            assert(prefix.last() == op);
        }
        results.push(r);
        i = i + 1;
    }
    assert(ops@.subrange(0, i as int) =~= ops@);
    (set, results)
}

// ---------------------------------------------------------------------------
// Snapshot decoding.
// ---------------------------------------------------------------------------
/// Models `Deserialize for OrderedSet` (crates/ferric-rules-core/src/ordered_set.rs:240
/// fn deserialize) after the `Vec<K>` has been decoded: `None` stands for the "duplicate
/// ordered membership" error. `caps[i]` are the free capacity inputs for the `i`-th
/// `insert`. Decoding accepts exactly the duplicate-free sequences and then reproduces
/// them as the view.
pub fn decode(values: &Vec<u64>, caps: &Vec<Caps>) -> (r: Option<OrderedSet>)
    requires
        caps.len() == values.len(),
    ensures
        r is Some <==> values@.no_duplicates(),
        r matches Some(s) ==> s.inv() && s@ == values@,
{
    let mut set = OrderedSet::new(Caps { c0: 0, c1: 0, c2: 0, c3: 0 });
    let mut i: usize = 0;
    while i < values.len()
        invariant
            caps.len() == values.len(),
            0 <= i <= values.len(),
            set.inv(),
            set@ == values@.subrange(0, i as int),
            values@.subrange(0, i as int).no_duplicates(),
        decreases values.len() - i,
    {
        if !set.insert(values[i], caps[i]) {
            proof {
                let prefix = values@.subrange(0, i as int);
                let j = choose|j: int| 0 <= j < i && prefix[j] == values@[i as int];
                assert(values@[j] == values@[i as int]);
            }
            return None;
        }
        proof {
            assert(values@.subrange(0, i + 1) =~= values@.subrange(0, i as int).push(
                values@[i as int],
            ));
        }
        i = i + 1;
    }
    assert(values@.subrange(0, i as int) =~= values@);
    Some(set)
}

/// Facts about `Seq::remove` at index `i`, phrased with `at`.
pub proof fn lemma_remove_at(v: Seq<u64>, i: int)
    requires
        0 <= i < v.len(),
        v.no_duplicates(),
    ensures
        v.remove(i).len() == v.len() - 1,
        v.remove(i).no_duplicates(),
        forall|j: int| j < i ==> #[trigger] at(v.remove(i), j) == at(v, j),
        forall|j: int| j >= i ==> #[trigger] at(v.remove(i), j) == at(v, j + 1),
{
    let w = v.remove(i);
    assert forall|a: int, b: int| 0 <= a < w.len() && 0 <= b < w.len() && a != b implies w[a]
        != w[b] by {
        let a0 = if a < i {
            a
        } else {
            a + 1
        };
        let b0 = if b < i {
            b
        } else {
            b + 1
        };
        assert(w[a] == v[a0] && w[b] == v[b0]);
    }
}

/// With no duplicates, `index_of` finds the unique position of a member.
pub proof fn lemma_index_of_unique(v: Seq<u64>, i: int)
    requires
        0 <= i < v.len(),
        v.no_duplicates(),
    ensures
        v.contains(v[i]),
        v.index_of(v[i]) == i,
{
    assert(v[i] == v[i]);
    let j = v.index_of(v[i]);
    assert(0 <= j < v.len() && v[j] == v[i]);
}

// ---------------------------------------------------------------------------
// Lemmas about the representations.
// ---------------------------------------------------------------------------
/// In the linked representation the table has exactly one entry per member.
pub proof fn lemma_linked_len(
    entries: Map<u64, Links>,
    first: Option<u64>,
    last: Option<u64>,
    v: Seq<u64>,
)
    requires
        linked_rep(entries, first, last, v),
    ensures
        entries.len() == v.len(),
        entries.dom() == v.to_set(),
{
    assert forall|k: u64| entries.dom().contains(k) <==> #[trigger] v.to_set().contains(k) by {
        if entries.contains_key(k) {
            let i = choose|i: int| 0 <= i < v.len() && v[i] == k;
            assert(v.to_set().contains(v[i]));
        }
        if v.to_set().contains(k) {
            assert(v.contains(k));
            let i = choose|i: int| 0 <= i < v.len() && v[i] == k;
            assert(entries.contains_key(v[i]));
        }
    }
    assert(entries.dom() =~= v.to_set());
    v.unique_seq_to_set();
}

/// In the linked representation, table membership is view membership.
pub proof fn lemma_linked_contains(
    entries: Map<u64, Links>,
    first: Option<u64>,
    last: Option<u64>,
    v: Seq<u64>,
    key: u64,
)
    requires
        linked_rep(entries, first, last, v),
    ensures
        entries.contains_key(key) == v.contains(key),
{
    if v.contains(key) {
        let i = choose|i: int| 0 <= i < v.len() && v[i] == key;
        assert(entries.contains_key(v[i]));
    }
}

/// In the header-only representation, a key equal to neither endpoint is
/// not a member.
pub proof fn lemma_small_not_contains(
    first: Option<u64>,
    last: Option<u64>,
    v: Seq<u64>,
    key: u64,
)
    requires
        small_rep(first, last, v),
        first != Some(key),
        last != Some(key),
    ensures
        !v.contains(key),
{
    if v.contains(key) {
        let i = choose|i: int| 0 <= i < v.len() && v[i] == key;
        assert(i == 0 || i == v.len() - 1);
    }
}

} // verus!
