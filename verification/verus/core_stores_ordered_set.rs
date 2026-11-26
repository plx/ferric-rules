//! Verus model of `OrderedSet` (crates/ferric-rules-core/src/ordered_set.rs).
//!
//! What is proved (unbounded: any key type, any set size, any operation
//! sequence):
//!
//! * `OrderedSetModel::inv(s)` relates the concrete representation (the
//!   `entries` link table, the `first`/`last` endpoints and whether the table
//!   reports zero capacity) to an abstract duplicate-free sequence `s`.
//! * `Default` establishes the invariant for the empty sequence.
//! * `insert` returns `!s.contains(key)` and refines `s.push(key)` (or leaves
//!   `s` unchanged), in both the zero-capacity endpoint representation and
//!   the linked table, including the promotion from two endpoints to the
//!   three-entry table.
//! * `remove` returns `s.contains(key)` and refines deletion of `key` from `s`,
//!   preserving the order of the remaining members, in both representations,
//!   whatever capacity the table reports afterwards (see "HashMap contract").
//! * `contains`, `len` and `is_empty` agree with `s`.
//! * `clear` refines the empty sequence.
//! * The double-ended iterator: starting from `iter()`, every interleaving
//!   of `next` and `next_back` yields exactly `s[lo]` / `s[hi - 1]` for the
//!   current window `[lo, hi)`, returns `None` exactly when the window is
//!   empty, and keeps `remaining == hi - lo` (the `ExactSizeIterator` length).
//! * Snapshot decoding (`Deserialize`) succeeds exactly when the encoded
//!   sequence has no duplicates, and then refines that sequence.
//! * Panic freedom of the modelled code: every `unwrap()`, every `?` that
//!   would end iteration early, and the `debug_assert_eq!` in the iterator
//!   hold whenever the invariant holds (`*_obligations` predicates).
//!
//! Trusted base and abstractions:
//!
//! * The `FxHashMap<K, Links<K>>` is modelled as a finite `Map<K, Links<K>>`
//!   plus a boolean `zero_capacity` standing for `entries.capacity() == 0`.
//!   The only facts assumed about `HashMap` are its documented map semantics
//!   and `capacity() >= len()`, which give: after `reserve` followed by
//!   inserts, or after any insert, capacity is non-zero; after `remove` the
//!   capacity is unconstrained when the map becomes empty (hashbrown can
//!   report zero insertion capacity for an allocated table full of
//!   tombstones) and non-zero otherwise; after `clear` the capacity is
//!   unconstrained.  The unconstrained choices are explicit `bool`
//!   parameters, so every lemma holds for every choice.
//! * `K: Eq` is modelled by spec equality (true for the production keys
//!   `FactId` and `TokenId`, whose `Eq` is derived). `get_key_value` returns
//!   the stored key, which is then equal to the probe key.
//! * Mutation through `get_mut(..).unwrap()` is modelled as `Map::insert`
//!   of the updated value at the same key, after proving the key is present.
//! * Rust's `usize` arithmetic in `len` and `Iter::remaining` is modelled with
//!   `nat`; the decrements are only reached when the value is positive
//!   (proved), so they cannot underflow.
//!
//! Known gaps: hashing, table growth and memory layout are not modelled (they
//! are std's responsibility). The serde code is modelled only for the
//! membership logic, not for the serializer/deserializer frameworks. No
//! TRUSTED glue, `assume`, `admit` or `external_body` is used.
use vstd::prelude::*;

verus! {

/// Models `Links<K>` (crates/ferric-rules-core/src/ordered_set.rs:10).
pub struct Links<K> {
    pub previous: Option<K>,
    pub next: Option<K>,
}

/// Models `OrderedSet<K>` (crates/ferric-rules-core/src/ordered_set.rs:15).
/// `zero_capacity` stands for `entries.capacity() == 0`.
pub struct OrderedSetModel<K> {
    pub entries: Map<K, Links<K>>,
    pub zero_capacity: bool,
    pub first: Option<K>,
    pub last: Option<K>,
}

/// Models `Iter<'_, K>` (crates/ferric-rules-core/src/ordered_set.rs:161).
/// The `set` reference is passed separately to the step functions.
pub struct IterModel<K> {
    pub front: Option<K>,
    pub back: Option<K>,
    pub remaining: nat,
}

// ---------------------------------------------------------------------------
// Abstraction function helpers
// ---------------------------------------------------------------------------
/// The links that member `s[i]` must carry in the table.
pub open spec fn links_at<K>(s: Seq<K>, i: int) -> Links<K> {
    Links {
        previous: if i == 0 {
            None
        } else {
            Some(s[i - 1])
        },
        next: if i + 1 == s.len() {
            None
        } else {
            Some(s[i + 1])
        },
    }
}

pub open spec fn first_of<K>(s: Seq<K>) -> Option<K> {
    if s.len() == 0 {
        None
    } else {
        Some(s[0])
    }
}

pub open spec fn last_of<K>(s: Seq<K>) -> Option<K> {
    if s.len() == 0 {
        None
    } else {
        Some(s[s.len() - 1])
    }
}

impl<K> OrderedSetModel<K> {
    /// Representation invariant / abstraction relation to the sequence `s`.
    pub open spec fn inv(self, s: Seq<K>) -> bool {
        &&& s.no_duplicates()
        &&& self.first == first_of(s)
        &&& self.last == last_of(s)
        &&& if self.zero_capacity {
            // HashMap contract: capacity() >= len(), so the table is empty and
            // the endpoints hold at most two members.
            &&& forall|k: K| !self.entries.contains_key(k)
            &&& s.len() <= 2
        } else {
            &&& self.entries.len() == s.len()
            &&& forall|k: K| #[trigger] self.entries.contains_key(k) <==> s.contains(k)
            &&& forall|i: int| 0 <= i < s.len() ==> self.entries[#[trigger] s[i]] == links_at(s, i)
        }
    }

    // -----------------------------------------------------------------------
    // Operations (each follows the production control flow branch by branch)
    // -----------------------------------------------------------------------
    /// Models crates/ferric-rules-core/src/ordered_set.rs:22 fn default.
    pub open spec fn default_model() -> Self {
        OrderedSetModel { entries: Map::empty(), zero_capacity: true, first: None, last: None }
    }

    /// Models crates/ferric-rules-core/src/ordered_set.rs:32 fn insert.
    /// Returns the new state and the production return value.
    pub open spec fn insert(self, key: K) -> (Self, bool) {
        if self.zero_capacity {
            if self.first is None {
                (
                    OrderedSetModel {
                        entries: self.entries,
                        zero_capacity: self.zero_capacity,
                        first: Some(key),
                        last: Some(key),
                    },
                    true,
                )
            } else if self.first == Some(key) || self.last == Some(key) {
                (self, false)
            } else if self.first == self.last {
                (
                    OrderedSetModel {
                        entries: self.entries,
                        zero_capacity: self.zero_capacity,
                        first: self.first,
                        last: Some(key),
                    },
                    true,
                )
            } else {
                let first = self.first->0;  // self.first.unwrap()
                let middle = self.last->0;  // self.last.unwrap()
                // self.entries.reserve(3): capacity becomes non-zero.
                let entries = self.entries.insert(
                    first,
                    Links { previous: None, next: Some(middle) },
                ).insert(middle, Links { previous: Some(first), next: Some(key) }).insert(
                    key,
                    Links { previous: Some(middle), next: None },
                );
                (
                    OrderedSetModel {
                        entries,
                        zero_capacity: false,
                        first: self.first,
                        last: Some(key),
                    },
                    true,
                )
            }
        } else if self.entries.contains_key(key) {
            // `entry(key)` is Occupied.
            (self, false)
        } else {
            // Vacant entry: insert, then link from the previous tail.
            let entries1 = self.entries.insert(key, Links { previous: self.last, next: None });
            let (entries2, first2) = match self.last {
                Some(previous) => (
                    // self.entries.get_mut(&previous).unwrap().next = Some(key)
                    entries1.insert(
                        previous,
                        Links { previous: entries1[previous].previous, next: Some(key) },
                    ),
                    self.first,
                ),
                None => (entries1, Some(key)),
            };
            (
                OrderedSetModel {
                    entries: entries2,
                    zero_capacity: false,
                    first: first2,
                    last: Some(key),
                },
                true,
            )
        }
    }

    /// The `unwrap()` calls in `insert` succeed.
    pub open spec fn insert_obligations(self, key: K) -> bool {
        if self.zero_capacity {
            (self.first is Some && !(self.first == Some(key) || self.last == Some(key))
                && self.first != self.last) ==> (self.first is Some && self.last is Some)
        } else {
            (!self.entries.contains_key(key) && self.last is Some) ==> self.entries.insert(
                key,
                Links { previous: self.last, next: None },
            ).contains_key(self.last->0)
        }
    }

    /// Models crates/ferric-rules-core/src/ordered_set.rs:92 fn remove.
    /// `zero_capacity_after` is the table's unconstrained capacity report
    /// after `HashMap::remove` empties it (see the module documentation).
    pub open spec fn remove(self, key: K, zero_capacity_after: bool) -> (Self, bool) {
        if self.zero_capacity {
            if self.first == Some(key) {
                if self.first == self.last {
                    (
                        OrderedSetModel {
                            entries: self.entries,
                            zero_capacity: self.zero_capacity,
                            first: None,
                            last: None,
                        },
                        true,
                    )
                } else {
                    (
                        OrderedSetModel {
                            entries: self.entries,
                            zero_capacity: self.zero_capacity,
                            first: self.last,
                            last: self.last,
                        },
                        true,
                    )
                }
            } else if self.last == Some(key) {
                (
                    OrderedSetModel {
                        entries: self.entries,
                        zero_capacity: self.zero_capacity,
                        first: self.first,
                        last: self.first,
                    },
                    true,
                )
            } else {
                (self, false)
            }
        } else if !self.entries.contains_key(key) {
            (self, false)
        } else {
            let links = self.entries[key];
            let entries1 = self.entries.remove(key);
            let (entries2, first2) = match links.previous {
                Some(previous) => (
                    // self.entries.get_mut(&previous).unwrap().next = links.next
                    entries1.insert(
                        previous,
                        Links { previous: entries1[previous].previous, next: links.next },
                    ),
                    self.first,
                ),
                None => (entries1, links.next),
            };
            let (entries3, last3) = match links.next {
                Some(next) => (
                    // self.entries.get_mut(&next).unwrap().previous = links.previous
                    entries2.insert(
                        next,
                        Links { previous: links.previous, next: entries2[next].next },
                    ),
                    self.last,
                ),
                None => (entries2, links.previous),
            };
            (
                OrderedSetModel {
                    entries: entries3,
                    zero_capacity: zero_capacity_after && entries3.len() == 0,
                    first: first2,
                    last: last3,
                },
                true,
            )
        }
    }

    /// The `unwrap()` calls in `remove` succeed.
    pub open spec fn remove_obligations(self, key: K) -> bool {
        !self.zero_capacity && self.entries.contains_key(key) ==> {
            let links = self.entries[key];
            let entries1 = self.entries.remove(key);
            &&& (links.previous is Some ==> entries1.contains_key(links.previous->0))
            &&& (links.next is Some ==> entries1.contains_key(links.next->0))
        }
    }

    /// Models crates/ferric-rules-core/src/ordered_set.rs:125 fn contains.
    pub open spec fn contains(self, key: K) -> bool {
        if self.zero_capacity {
            self.first == Some(key) || self.last == Some(key)
        } else {
            self.entries.contains_key(key)
        }
    }

    /// Models crates/ferric-rules-core/src/ordered_set.rs:133 fn len.
    pub open spec fn len(self) -> nat {
        if self.zero_capacity {
            (if self.first is Some {
                1nat
            } else {
                0nat
            }) + (if self.first != self.last {
                1nat
            } else {
                0nat
            })
        } else {
            self.entries.len()
        }
    }

    /// Models crates/ferric-rules-core/src/ordered_set.rs:141 fn is_empty.
    pub open spec fn is_empty(self) -> bool {
        self.first is None
    }

    /// Models crates/ferric-rules-core/src/ordered_set.rs:145 fn clear.
    /// `zero_capacity_after`: `HashMap::clear` keeps any allocation; the
    /// reported capacity is left unconstrained.
    pub open spec fn clear(self, zero_capacity_after: bool) -> Self {
        OrderedSetModel {
            entries: Map::empty(),
            zero_capacity: zero_capacity_after,
            first: None,
            last: None,
        }
    }

    /// Models crates/ferric-rules-core/src/ordered_set.rs:151 fn iter.
    pub open spec fn iter(self) -> IterModel<K> {
        IterModel { front: self.first, back: self.last, remaining: self.len() }
    }

    /// Models crates/ferric-rules-core/src/ordered_set.rs:170 Iterator::next.
    pub open spec fn iter_next(self, it: IterModel<K>) -> (IterModel<K>, Option<K>) {
        if it.remaining == 0 {
            (it, None)
        } else if it.front is None {
            // `let front = self.front?;`
            (it, None)
        } else {
            let front = it.front->0;
            if self.entries.contains_key(front) {
                // `get_key_value(&front)` hit
                (
                    IterModel {
                        front: self.entries[front].next,
                        back: it.back,
                        remaining: (it.remaining - 1) as nat,
                    },
                    Some(front),
                )
            } else if self.first == Some(front) {
                // `self.front = self.set.last; self.set.first.as_ref()?`
                let it1 = IterModel { front: self.last, back: it.back, remaining: it.remaining };
                if self.first is None {
                    (it1, None)
                } else {
                    (
                        IterModel {
                            front: it1.front,
                            back: it1.back,
                            remaining: (it1.remaining - 1) as nat,
                        },
                        self.first,
                    )
                }
            } else {
                // `self.front = None; self.set.last.as_ref()?`
                let it1 = IterModel { front: None, back: it.back, remaining: it.remaining };
                if self.last is None {
                    (it1, None)
                } else {
                    (
                        IterModel {
                            front: it1.front,
                            back: it1.back,
                            remaining: (it1.remaining - 1) as nat,
                        },
                        self.last,
                    )
                }
            }
        }
    }

    /// The early `?` returns in `next` are never taken while items remain,
    /// and the table miss only happens with zero capacity (`debug_assert_eq!`).
    pub open spec fn iter_next_obligations(self, it: IterModel<K>) -> bool {
        it.remaining > 0 ==> {
            &&& it.front is Some
            &&& !self.entries.contains_key(it.front->0) ==> {
                &&& self.zero_capacity
                &&& (self.first == Some(it.front->0) ==> self.first is Some)
                &&& (self.first != Some(it.front->0) ==> self.last is Some)
            }
        }
    }

    /// Models crates/ferric-rules-core/src/ordered_set.rs:199
    /// DoubleEndedIterator::next_back.
    pub open spec fn iter_next_back(self, it: IterModel<K>) -> (IterModel<K>, Option<K>) {
        if it.remaining == 0 {
            (it, None)
        } else if it.back is None {
            // `let back = self.back?;`
            (it, None)
        } else {
            let back = it.back->0;
            if self.entries.contains_key(back) {
                (
                    IterModel {
                        front: it.front,
                        back: self.entries[back].previous,
                        remaining: (it.remaining - 1) as nat,
                    },
                    Some(back),
                )
            } else if self.first == Some(back) {
                // `self.back = None; self.set.first.as_ref()?`
                let it1 = IterModel { front: it.front, back: None, remaining: it.remaining };
                if self.first is None {
                    (it1, None)
                } else {
                    (
                        IterModel {
                            front: it1.front,
                            back: it1.back,
                            remaining: (it1.remaining - 1) as nat,
                        },
                        self.first,
                    )
                }
            } else {
                // `self.back = self.set.first; self.set.last.as_ref()?`
                let it1 = IterModel { front: it.front, back: self.first, remaining: it.remaining };
                if self.last is None {
                    (it1, None)
                } else {
                    (
                        IterModel {
                            front: it1.front,
                            back: it1.back,
                            remaining: (it1.remaining - 1) as nat,
                        },
                        self.last,
                    )
                }
            }
        }
    }

    pub open spec fn iter_next_back_obligations(self, it: IterModel<K>) -> bool {
        it.remaining > 0 ==> {
            &&& it.back is Some
            &&& !self.entries.contains_key(it.back->0) ==> {
                &&& self.zero_capacity
                &&& (self.first == Some(it.back->0) ==> self.first is Some)
                &&& (self.first != Some(it.back->0) ==> self.last is Some)
            }
        }
    }

    /// Iterator window invariant: the iterator has yielded `s[..lo]` from
    /// the front and `s[hi..]` from the back.
    pub open spec fn iter_inv(self, s: Seq<K>, it: IterModel<K>, lo: int, hi: int) -> bool {
        &&& self.inv(s)
        &&& 0 <= lo <= hi <= s.len()
        &&& it.remaining == hi - lo
        &&& it.remaining > 0 ==> it.front == Some(s[lo]) && it.back == Some(s[hi - 1])
    }

    /// Models the loop of crates/ferric-rules-core/src/ordered_set.rs:240
    /// `Deserialize::deserialize`: `None` is the "duplicate ordered
    /// membership" error.
    pub open spec fn insert_all(self, values: Seq<K>) -> Option<Self>
        decreases values.len(),
    {
        if values.len() == 0 {
            Some(self)
        } else {
            let (next, inserted) = self.insert(values[0]);
            if !inserted {
                None
            } else {
                next.insert_all(values.drop_first())
            }
        }
    }

    // -----------------------------------------------------------------------
    // Proofs
    // -----------------------------------------------------------------------
    pub proof fn lemma_default()
        ensures
            Self::default_model().inv(Seq::empty()),
    {
    }

    /// The table size agrees with the abstract length in every state.
    pub proof fn lemma_len(self, s: Seq<K>)
        requires
            self.inv(s),
        ensures
            self.len() == s.len(),
            self.is_empty() <==> s.len() == 0,
    {
        if self.zero_capacity {
            if s.len() == 2 {
                assert(s[0] != s[1]);
            }
        }
    }

    pub proof fn lemma_contains(self, s: Seq<K>, key: K)
        requires
            self.inv(s),
        ensures
            self.contains(key) == s.contains(key),
    {
        if self.zero_capacity && s.contains(key) {
            let i = choose|i: int| 0 <= i < s.len() && s[i] == key;
            assert(i == 0 || i == s.len() - 1);
        }
        if self.zero_capacity && self.contains(key) {
            if self.first == Some(key) {
                assert(s[0] == key);
            } else {
                assert(s[s.len() - 1] == key);
            }
        }
    }

    pub proof fn lemma_insert(self, s: Seq<K>, key: K)
        requires
            self.inv(s),
        ensures
            self.insert_obligations(key),
            self.insert(key).1 == !s.contains(key),
            self.insert(key).0.inv(
                if self.insert(key).1 {
                    s.push(key)
                } else {
                    s
                },
            ),
    {
        let (next, inserted) = self.insert(key);
        let s2 = s.push(key);
        self.lemma_contains(s, key);
        if self.zero_capacity {
            if self.first is None {
                assert(s2 =~= seq![key]);
            } else if self.first == Some(key) || self.last == Some(key) {
            } else if self.first == self.last {
                assert(s.len() == 1) by {
                    if s.len() == 2 {
                        assert(s[0] != s[1]);
                    }
                }
                assert(s2 =~= seq![s[0], key]);
            } else {
                assert(s.len() == 2);
                let a = s[0];
                let b = s[1];
                assert(s2 =~= seq![a, b, key]);
                assert(a != b && a != key && b != key);
                assert(s2.no_duplicates());
                assert(next.entries.dom() =~= set![a, b, key]);
                assert(set![a, b, key].len() == 3) by {
                    assert(set![a, b].len() == 2) by {
                        assert(Set::<K>::empty().insert(a).len() == 1);
                    }
                }
                assert forall|k: K| #[trigger] next.entries.contains_key(k) <==> s2.contains(k) by {
                    if k == a {
                        assert(s2[0] == k);
                    } else if k == b {
                        assert(s2[1] == k);
                    } else if k == key {
                        assert(s2[2] == k);
                    }
                }
                assert forall|i: int| 0 <= i < s2.len() implies next.entries[#[trigger] s2[i]]
                    == links_at(s2, i) by {
                    if i == 0 {
                    } else if i == 1 {
                    } else {
                    }
                }
            }
        } else if self.entries.contains_key(key) {
        } else {
            let n = s.len() as int;
            assert(s2.no_duplicates()) by {
                assert forall|i: int, j: int|
                    0 <= i < s2.len() && 0 <= j < s2.len() && i != j implies s2[i] != s2[j] by {
                    if i == n {
                        assert(!s.contains(key));
                        if s2[j] == key {
                            assert(s[j] == key);
                        }
                    } else if j == n {
                        if s2[i] == key {
                            assert(s[i] == key);
                        }
                    }
                }
            }
            assert(!self.entries.dom().contains(key));
            assert(next.entries.dom() =~= self.entries.dom().insert(key));
            assert forall|k: K| #[trigger] next.entries.contains_key(k) <==> s2.contains(k) by {
                if k == key {
                    assert(s2[n] == key);
                } else if s2.contains(k) {
                    let i = choose|i: int| 0 <= i < s2.len() && s2[i] == k;
                    assert(s[i] == k);
                } else if self.entries.contains_key(k) {
                    let i = choose|i: int| 0 <= i < s.len() && s[i] == k;
                    assert(s2[i] == k);
                }
            }
            assert forall|i: int| 0 <= i < s2.len() implies next.entries[#[trigger] s2[i]]
                == links_at(s2, i) by {
                if i == n {
                } else if i == n - 1 {
                    assert(s[i] != key);
                    assert(self.entries[s[i]] == links_at(s, i));
                } else {
                    assert(s[i] != key);
                    assert(s[i] != s[n - 1]);
                    assert(self.entries[s[i]] == links_at(s, i));
                }
            }
        }
    }

    pub proof fn lemma_remove(self, s: Seq<K>, key: K, zero_capacity_after: bool)
        requires
            self.inv(s),
        ensures
            self.remove_obligations(key),
            self.remove(key, zero_capacity_after).1 == s.contains(key),
            !s.contains(key) ==> self.remove(key, zero_capacity_after).0.inv(s),
            s.contains(key) ==> exists|j: int|
                0 <= j < s.len() && s[j] == key && #[trigger] self.remove(
                    key,
                    zero_capacity_after,
                ).0.inv(s.remove(j)),
    {
        let (next, removed) = self.remove(key, zero_capacity_after);
        self.lemma_contains(s, key);
        if !s.contains(key) {
            return ;
        }
        let j = choose|j: int| 0 <= j < s.len() && s[j] == key;
        let s2 = s.remove(j);
        let n = s.len() as int;
        assert(s2.len() == n - 1);
        assert forall|i: int| 0 <= i < j implies #[trigger] s2[i] == s[i] by {}
        assert forall|i: int| j <= i < n - 1 implies #[trigger] s2[i] == s[i + 1] by {}
        assert(s2.no_duplicates()) by {
            assert forall|a: int, b: int|
                0 <= a < s2.len() && 0 <= b < s2.len() && a != b implies s2[a] != s2[b] by {
                let a1 = if a < j {
                    a
                } else {
                    a + 1
                };
                let b1 = if b < j {
                    b
                } else {
                    b + 1
                };
                assert(s2[a] == s[a1]);
                assert(s2[b] == s[b1]);
            }
        }
        if self.zero_capacity {
            if self.first == Some(key) {
                assert(j == 0);
                if self.first == self.last {
                    assert(n == 1) by {
                        if n == 2 {
                            assert(s[0] != s[1]);
                        }
                    }
                    assert(s2 =~= Seq::<K>::empty());
                } else {
                    assert(n == 2);
                    assert(s2 =~= seq![s[1]]);
                }
            } else {
                assert(self.last == Some(key));
                assert(n == 2 && j == 1);
                assert(s2 =~= seq![s[0]]);
            }
            assert(next.inv(s2));
        } else {
            let links = self.entries[key];
            assert(links == links_at(s, j));
            let entries1 = self.entries.remove(key);
            assert(entries1.dom() =~= self.entries.dom().remove(key));
            // Members other than `key` stay in the table after its removal.
            if j > 0 {
                assert(s[j - 1] != key);
                assert(self.entries.contains_key(s[j - 1]));
            }
            if j + 1 < n {
                assert(s[j + 1] != key);
                assert(self.entries.contains_key(s[j + 1]));
            }
            assert(next.entries.dom() =~= self.entries.dom().remove(key));
            assert(next.entries.len() == s2.len());
            assert forall|k: K| #[trigger] next.entries.contains_key(k) <==> s2.contains(k) by {
                if s2.contains(k) {
                    let i = choose|i: int| 0 <= i < s2.len() && s2[i] == k;
                    if i < j {
                        assert(s[i] == k);
                        assert(k != key);
                    } else {
                        assert(s[i + 1] == k);
                        assert(k != key);
                    }
                }
                if next.entries.contains_key(k) {
                    assert(self.entries.contains_key(k));
                    let i = choose|i: int| 0 <= i < s.len() && s[i] == k;
                    assert(i != j);
                    if i < j {
                        assert(s2[i] == k);
                    } else {
                        assert(s2[i - 1] == k);
                    }
                }
            }
            assert forall|i: int| 0 <= i < s2.len() implies next.entries[#[trigger] s2[i]]
                == links_at(s2, i) by {
                if i < j - 1 {
                    assert(s2[i] == s[i]);
                    assert(s[i] != key && s[i] != s[j - 1]);
                    if j + 1 < n {
                        assert(s[i] != s[j + 1]);
                    }
                    assert(self.entries[s[i]] == links_at(s, i));
                    if i + 1 < j {
                        assert(s2[i + 1] == s[i + 1]);
                    }
                    if i > 0 {
                        assert(s2[i - 1] == s[i - 1]);
                    }
                } else if i == j - 1 {
                    assert(s2[i] == s[j - 1]);
                    assert(self.entries[s[j - 1]] == links_at(s, j - 1));
                    if j + 1 < n {
                        assert(s[j - 1] != s[j + 1]);
                        assert(s2[j] == s[j + 1]);
                    }
                    if i > 0 {
                        assert(s2[i - 1] == s[i - 1]);
                    }
                } else if i == j {
                    assert(s2[i] == s[j + 1]);
                    assert(self.entries[s[j + 1]] == links_at(s, j + 1));
                    if j > 0 {
                        assert(s[j - 1] != s[j + 1]);
                        assert(s2[j - 1] == s[j - 1]);
                    }
                    if i + 1 < s2.len() {
                        assert(s2[i + 1] == s[j + 2]);
                    }
                } else {
                    assert(s2[i] == s[i + 1]);
                    assert(s[i + 1] != key && s[i + 1] != s[j + 1]);
                    if j > 0 {
                        assert(s[i + 1] != s[j - 1]);
                    }
                    assert(self.entries[s[i + 1]] == links_at(s, i + 1));
                    assert(s2[i - 1] == s[i]);
                    if i + 1 < s2.len() {
                        assert(s2[i + 1] == s[i + 2]);
                    }
                }
            }
            if j == 0 {
                if n > 1 {
                    assert(s2[0] == s[1]);
                }
            } else {
                assert(s2[0] == s[0]);
            }
            if j == n - 1 {
                if n > 1 {
                    assert(s2[n - 2] == s[n - 2]);
                }
            } else {
                assert(s2[n - 2] == s[n - 1]);
            }
            if next.zero_capacity {
                assert(s2.len() == 0);
            }
            assert(next.inv(s2));
        }
    }

    pub proof fn lemma_clear(self, zero_capacity_after: bool)
        ensures
            self.clear(zero_capacity_after).inv(Seq::empty()),
    {
    }

    pub proof fn lemma_iter_start(self, s: Seq<K>)
        requires
            self.inv(s),
        ensures
            self.iter_inv(s, self.iter(), 0, s.len() as int),
    {
        self.lemma_len(s);
    }

    pub proof fn lemma_iter_next(self, s: Seq<K>, it: IterModel<K>, lo: int, hi: int)
        requires
            self.iter_inv(s, it, lo, hi),
        ensures
            self.iter_next_obligations(it),
            lo == hi ==> self.iter_next(it).1 is None,
            lo < hi ==> self.iter_next(it).1 == Some(s[lo]) && self.iter_inv(
                s,
                self.iter_next(it).0,
                lo + 1,
                hi,
            ),
    {
        if lo < hi {
            let front = s[lo];
            if self.entries.contains_key(front) {
                assert(!self.zero_capacity);
                assert(self.entries[s[lo]] == links_at(s, lo));
            } else {
                assert(self.zero_capacity) by {
                    assert(s.contains(front));
                }
                if self.first == Some(front) {
                    assert(lo == 0) by {
                        assert(s[0] == s[lo]);
                    }
                    if lo + 1 < hi {
                        assert(s.len() == 2);
                    }
                } else {
                    assert(lo == s.len() - 1) by {
                        if s.len() == 2 && lo == 0 {
                        }
                    }
                }
            }
        }
    }

    pub proof fn lemma_iter_next_back(self, s: Seq<K>, it: IterModel<K>, lo: int, hi: int)
        requires
            self.iter_inv(s, it, lo, hi),
        ensures
            self.iter_next_back_obligations(it),
            lo == hi ==> self.iter_next_back(it).1 is None,
            lo < hi ==> self.iter_next_back(it).1 == Some(s[hi - 1]) && self.iter_inv(
                s,
                self.iter_next_back(it).0,
                lo,
                hi - 1,
            ),
    {
        if lo < hi {
            let back = s[hi - 1];
            if self.entries.contains_key(back) {
                assert(!self.zero_capacity);
                assert(self.entries[s[hi - 1]] == links_at(s, hi - 1));
            } else {
                assert(self.zero_capacity) by {
                    assert(s.contains(back));
                }
                if self.first == Some(back) {
                    assert(hi - 1 == 0) by {
                        assert(s[0] == s[hi - 1]);
                    }
                } else {
                    assert(hi - 1 == s.len() - 1) by {
                        if s.len() == 2 && hi - 1 == 0 {
                        }
                    }
                    if lo < hi - 1 {
                        assert(s.len() == 2);
                    }
                }
            }
        }
    }

    /// Decoding refinement: inserting `values` one by one into a set that
    /// refines `s` succeeds exactly when `s + values` has no duplicates.
    pub proof fn lemma_insert_all(self, s: Seq<K>, values: Seq<K>)
        requires
            self.inv(s),
        ensures
            self.insert_all(values) is Some <==> (s + values).no_duplicates(),
            self.insert_all(values) is Some ==> self.insert_all(values)->0.inv(s + values),
        decreases values.len(),
    {
        if values.len() == 0 {
            assert(s + values =~= s);
        } else {
            let v = values[0];
            let rest = values.drop_first();
            self.lemma_insert(s, v);
            let (next, inserted) = self.insert(v);
            assert(s + values =~= s.push(v) + rest);
            if !inserted {
                // `v` already occurs in `s`, so `s + values` has a duplicate.
                let i = choose|i: int| 0 <= i < s.len() && s[i] == v;
                assert((s + values)[i] == (s + values)[s.len() as int]);
            } else {
                next.lemma_insert_all(s.push(v), rest);
                if (s + values).no_duplicates() {
                    assert(s.push(v).no_duplicates());
                }
            }
        }
    }

    /// `Deserialize` of a snapshot sequence: models
    /// crates/ferric-rules-core/src/ordered_set.rs:240.
    pub proof fn lemma_deserialize(values: Seq<K>)
        ensures
            Self::default_model().insert_all(values) is Some <==> values.no_duplicates(),
            Self::default_model().insert_all(values) is Some
                ==> Self::default_model().insert_all(values)->0.inv(values),
    {
        Self::lemma_default();
        Self::default_model().lemma_insert_all(Seq::empty(), values);
        assert(Seq::<K>::empty() + values =~= values);
    }
}

} // verus!
