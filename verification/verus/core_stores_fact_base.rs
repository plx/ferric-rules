//! Verus model of `FactBase` (crates/ferric-rules-core/src/fact.rs).
//!
//! What is proved (unbounded: any number of facts and operations):
//!
//! * `FactBaseModel::inv` holds for `new()` and is preserved by every
//!   checked insertion entry point (`try_assert_fact` with either duplicate
//!   policy, `try_assert_distinct_fact`) and by `retract`. The invariant says:
//!   every live timestamp is below `next_timestamp` and unique; `by_relation`
//!   and `by_template` hold exactly the live facts of each relation/template
//!   and never an empty set; when present, the structural-fingerprint index
//!   lists, for every fingerprint, exactly the live facts with that
//!   fingerprint in strictly increasing timestamp order; every initialized
//!   chronology maps exactly the timestamps of the live facts of its
//!   relation/template to their ids.
//! * Timestamps: an inserted fact receives the old `next_timestamp`, which
//!   then increases by one; insertion fails with `FactTimestampExhausted`
//!   exactly when `next_timestamp == u64::MAX` (and, for checked assertion,
//!   no equivalent fact exists), and a failed insertion leaves the facts,
//!   their indexes and `next_timestamp` unchanged.
//! * Duplicate detection: `try_assert_fact(fact, false)` returns
//!   `Duplicate(id)` exactly when a structurally equal fact is live, and then
//!   `id` is the oldest such fact; otherwise the fact is inserted (or the
//!   call fails as above). Duplicate suppression works even when exhausted.
//! * `retract` returns the entry exactly when the id is live and removes
//!   exactly that fact.
//! * Chronology cursors: `next_relation_fact_after(relation, after)`
//!   (resp. `next_template_fact_after(template, after)`) returns the oldest
//!   live fact of that relation (resp. template) whose timestamp is strictly
//!   greater than `after` (any timestamp for `None`), or `None` exactly when
//!   there is no such fact, whether or not the chronology was initialized
//!   before; it leaves the facts unchanged and preserves the invariant.
//! * The lazy rebuild in `ensure_structural_index` itself
//!   (`lemma_rebuild_refines_model`): the grouping loop over the slot map,
//!   in any iteration order, followed by `sort_unstable_by_key(timestamp)`
//!   on every group longer than one (specified only by its documented
//!   contract: a permutation that is non-decreasing in the key) never hits
//!   the `expect` in the sort key and yields exactly the abstract
//!   `rebuilt_index` (per fingerprint, the live ids in timestamp order) that
//!   the model of `ensure_structural_index` uses; an exact index is unique
//!   (`lemma_index_exact_unique`).
//!
//! Trusted base and abstractions:
//!
//! * A fact is `FactModel::{Ordered, Template}` with its relation/template
//!   key and an abstract `content`; `Fact::structural_eq` is modelled as
//!   equality of `FactModel`s. `structural_fingerprint` is an arbitrary
//!   function `fp` of the model fact, i.e. equal facts have equal
//!   fingerprints. That consistency property of the production hash is only
//!   partly machine-checked: the Kani harnesses `value_structural_eq_*` in
//!   crates/ferric-rules-core/src/fact/kani_proofs.rs check which components
//!   `Value::structural_eq` compares for the scalar and fact-address kinds,
//!   and `fingerprint_ignores_*` check that the hash ignores the one
//!   component equality ignores (the fact-address public index, with the
//!   compared components fixed); that the hash of a scalar is a function of
//!   the compared components rests on inspection of
//!   `hash_value_structurally`, and strings and multifields are not covered
//!   by Kani at all.
//! * `SlotMap<FactId, FactEntry>` is a `Map`; the fresh key returned by
//!   `insert_with_key` is a parameter `id` with precondition "not live".
//! * `SymbolMap` / `FxHashMap` / `FxHashSet` / `BTreeMap` are `Map`/`Set`.
//!   `SymbolMap` itself is checked against a map model by the Kani harness
//!   `symbol_map_refines_partial_map`.
//! * `ensure_structural_index`'s rebuild is used by the operation models
//!   through its result `rebuilt_index` (per fingerprint, the live ids in
//!   timestamp order, `ids_by_time`); `lemma_rebuild_refines_model` proves
//!   that the production grouping loop plus sort produce exactly that
//!   result. The slot map's iteration order is an arbitrary duplicate-free
//!   enumeration of the live ids, and `sort_unstable_by_key` is trusted to
//!   meet its documented contract (its algorithm is not modelled).
//! * `u64` timestamps are `nat`s bounded by `U64_MAX`; `checked_add(1)` is
//!   modelled as failing exactly at `U64_MAX`.
//! * `BTreeMap::range((Excluded(t), Unbounded)).next()` / `first_key_value`
//!   are modelled by their specification: the least key above `t` / the
//!   least key (`first_key_after`).
//!
//! Known gaps: the public `find_equivalent` wrapper and the panicking
//! `assert_fact` / `assert_ordered` / `assert_template` wrappers are not
//! modelled separately (they are `ensure_structural_index` plus the modelled
//! lookup, and `try_assert_fact` plus `expect`). No TRUSTED glue, `assume`,
//! `admit` or `external_body` is used.
use vstd::prelude::*;

verus! {

broadcast use {vstd::set::group_set_lemmas, vstd::map::group_map_lemmas};

pub spec const U64_MAX: nat = 0xffff_ffff_ffff_ffff;

/// Models `Fact` (crates/ferric-rules-core/src/fact.rs:252).
pub enum FactModel<R, Tm, C> {
    Ordered { relation: R, content: C },
    Template { template: Tm, content: C },
}

/// The relation of an ordered fact.
pub open spec fn relation_of<R, Tm, C>(fact: FactModel<R, Tm, C>) -> Option<R> {
    match fact {
        FactModel::Ordered { relation, .. } => Some(relation),
        FactModel::Template { .. } => None,
    }
}

/// The template of a template fact.
pub open spec fn template_of<R, Tm, C>(fact: FactModel<R, Tm, C>) -> Option<Tm> {
    match fact {
        FactModel::Ordered { .. } => None,
        FactModel::Template { template, .. } => Some(template),
    }
}

/// Models `FactEntry` (crates/ferric-rules-core/src/fact.rs:347); the `id`
/// field equals the map key and is omitted.
pub struct EntryModel<R, Tm, C> {
    pub fact: FactModel<R, Tm, C>,
    pub timestamp: nat,
}

/// Models `FactInsertionResult` plus the `FactTimestampExhausted` error.
pub enum InsertOutcome<I> {
    Inserted(I),
    Duplicate(I),
    Exhausted,
}

/// Models `FactBase` (crates/ferric-rules-core/src/fact.rs:366).
pub struct FactBaseModel<I, R, Tm, C> {
    pub facts: Map<I, EntryModel<R, Tm, C>>,
    pub by_template: Map<Tm, Set<I>>,
    pub by_relation: Map<R, Set<I>>,
    pub by_structural_fingerprint: Option<Map<u64, Seq<I>>>,
    pub by_template_chronology: Map<Tm, Map<nat, I>>,
    pub by_relation_chronology: Map<R, Map<nat, I>>,
    pub next_timestamp: nat,
}

/// `index.entry(key).or_default().insert(id)`.
pub open spec fn add_to_set_index<K, I>(index: Map<K, Set<I>>, key: K, id: I) -> Map<K, Set<I>> {
    index.insert(
        key,
        if index.contains_key(key) {
            index[key]
        } else {
            Set::empty()
        }.insert(id),
    )
}

/// Models crates/ferric-rules-core/src/fact.rs:51 fn remove_from_set_index
/// (and the identical crates/ferric-rules-core/src/fact.rs:160
/// fn remove_from_symbol_set_index).
pub open spec fn remove_from_set_index<K, I>(index: Map<K, Set<I>>, key: K, id: I) -> Map<
    K,
    Set<I>,
> {
    if index.contains_key(key) {
        let set = index[key].remove(id);
        if set.len() == 0 {
            index.remove(key)
        } else {
            index.insert(key, set)
        }
    } else {
        index
    }
}

/// The `retain(|candidate| *candidate != id)` predicate.
pub open spec fn other_than<I>(id: I) -> spec_fn(I) -> bool {
    |candidate: I| candidate != id
}

/// Live ids whose fact has fingerprint `f`.
pub open spec fn fingerprint_is<I, R, Tm, C>(
    facts: Map<I, EntryModel<R, Tm, C>>,
    fp: spec_fn(FactModel<R, Tm, C>) -> u64,
    f: u64,
) -> spec_fn(I) -> bool {
    |id: I| fp(facts[id].fact) == f
}

/// Models crates/ferric-rules-core/src/fact.rs:66 fn remove_from_candidate_index.
pub open spec fn remove_from_candidate_index<I>(index: Map<u64, Seq<I>>, fingerprint: u64, id: I) -> Map<
    u64,
    Seq<I>,
> {
    if index.contains_key(fingerprint) {
        let candidates = index[fingerprint].filter(other_than(id));
        if candidates.len() == 0 {
            index.remove(fingerprint)
        } else {
            index.insert(fingerprint, candidates)
        }
    } else {
        index
    }
}

/// `chronology.get_mut(&key)` followed by `insert(timestamp, id)`.
pub open spec fn chronology_insert<K, I>(
    chronologies: Map<K, Map<nat, I>>,
    key: K,
    timestamp: nat,
    id: I,
) -> Map<K, Map<nat, I>> {
    if chronologies.contains_key(key) {
        chronologies.insert(key, chronologies[key].insert(timestamp, id))
    } else {
        chronologies
    }
}

/// `chronology.get_mut(&key)` followed by `remove(&timestamp)`.
pub open spec fn chronology_remove<K, I>(chronologies: Map<K, Map<nat, I>>, key: K, timestamp: nat) -> Map<
    K,
    Map<nat, I>,
> {
    if chronologies.contains_key(key) {
        chronologies.insert(key, chronologies[key].remove(timestamp))
    } else {
        chronologies
    }
}

/// Models the `find(..)` scan of
/// crates/ferric-rules-core/src/fact.rs:486 fn find_equivalent_with_fingerprint:
/// the first candidate that is live and structurally equal to `fact`.
pub open spec fn find_first<I, R, Tm, C>(
    facts: Map<I, EntryModel<R, Tm, C>>,
    candidates: Seq<I>,
    fact: FactModel<R, Tm, C>,
) -> Option<I>
    decreases candidates.len(),
{
    if candidates.len() == 0 {
        None
    } else if facts.contains_key(candidates[0]) && facts[candidates[0]].fact == fact {
        Some(candidates[0])
    } else {
        find_first(facts, candidates.drop_first(), fact)
    }
}

/// The live ids satisfying `pred` with timestamps below `n`, in timestamp
/// order (the result of the rebuild in `ensure_structural_index`).
pub open spec fn ids_by_time<I, R, Tm, C>(
    facts: Map<I, EntryModel<R, Tm, C>>,
    pred: spec_fn(I) -> bool,
    n: nat,
) -> Seq<I>
    decreases n,
{
    if n == 0 {
        Seq::empty()
    } else {
        let prev = ids_by_time(facts, pred, (n - 1) as nat);
        if exists|id: I| #[trigger]
            facts.contains_key(id) && facts[id].timestamp == n - 1 && pred(id) {
            prev.push(
                choose|id: I| #[trigger]
                    facts.contains_key(id) && facts[id].timestamp == n - 1 && pred(id),
            )
        } else {
            prev
        }
    }
}

pub open spec fn nat_leq() -> spec_fn(nat, nat) -> bool {
    |a: nat, b: nat| a <= b
}

/// The keys a `BTreeMap::range((Excluded(after), Unbounded))` (or, for
/// `None`, the whole map) can return.
pub open spec fn keys_after<I>(chronology: Map<nat, I>, after: Option<nat>) -> Set<nat> {
    match after {
        Some(t) => chronology.dom().filter(|k: nat| k > t),
        None => chronology.dom(),
    }
}

/// The first key of that range: its least element.
pub open spec fn first_key_after<I>(chronology: Map<nat, I>, after: Option<nat>) -> Option<nat> {
    let keys = keys_after(chronology, after);
    if keys.len() == 0 {
        None
    } else {
        Some(keys.find_unique_minimal(nat_leq()))
    }
}

pub proof fn lemma_first_key_after<I>(chronology: Map<nat, I>, after: Option<nat>)
    ensures
        first_key_after(chronology, after) is None <==> keys_after(chronology, after).len() == 0,
        first_key_after(chronology, after) matches Some(k) ==> {
            &&& keys_after(chronology, after).contains(k)
            &&& forall|x: nat| #[trigger] keys_after(chronology, after).contains(x) ==> k <= x
        },
{
    let keys = keys_after(chronology, after);
    if keys.len() != 0 {
        assert(vstd::relations::total_ordering(nat_leq()));
        keys.find_unique_minimal_ensures(nat_leq());
        let leq = nat_leq();
        let k = keys.find_unique_minimal(leq);
        assert(keys.to_iset().has_minimum(leq, k));
        assert forall|x: nat| #[trigger] keys.contains(x) implies k <= x by {
            if x < k {
                assert(leq(x, k));
                assert(leq(k, x));
            }
        }
    }
}

/// Strictly increasing timestamps along a candidate list of live facts.
pub open spec fn sorted_live<I, R, Tm, C>(facts: Map<I, EntryModel<R, Tm, C>>, s: Seq<I>) -> bool {
    &&& forall|i: int| 0 <= i < s.len() ==> facts.contains_key(#[trigger] s[i])
    &&& forall|i: int, j: int|
        0 <= i < j < s.len() ==> facts[#[trigger] s[i]].timestamp < facts[#[trigger] s[j]].timestamp
}

pub proof fn lemma_ids_by_time<I, R, Tm, C>(
    facts: Map<I, EntryModel<R, Tm, C>>,
    pred: spec_fn(I) -> bool,
    n: nat,
)
    requires
        forall|a: I, b: I|
            #[trigger] facts.contains_key(a) && #[trigger] facts.contains_key(b) && a != b
                ==> facts[a].timestamp != facts[b].timestamp,
    ensures
        sorted_live(facts, ids_by_time(facts, pred, n)),
        forall|i: int|
            0 <= i < ids_by_time(facts, pred, n).len() ==> pred(
                #[trigger] ids_by_time(facts, pred, n)[i],
            ) && facts[ids_by_time(facts, pred, n)[i]].timestamp < n,
        forall|id: I|
            facts.contains_key(id) && pred(id) && facts[id].timestamp < n ==> #[trigger] ids_by_time(
                facts,
                pred,
                n,
            ).contains(id),
    decreases n,
{
    if n > 0 {
        let m = (n - 1) as nat;
        lemma_ids_by_time(facts, pred, m);
        let prev = ids_by_time(facts, pred, m);
        let s = ids_by_time(facts, pred, n);
        if exists|id: I| #[trigger]
            facts.contains_key(id) && facts[id].timestamp == n - 1 && pred(id) {
            let x = choose|id: I| #[trigger]
                facts.contains_key(id) && facts[id].timestamp == n - 1 && pred(id);
            assert(s == prev.push(x));
            assert forall|i: int, j: int| 0 <= i < j < s.len() implies facts[#[trigger] s[i]].timestamp
                < facts[#[trigger] s[j]].timestamp by {
                if j == prev.len() {
                    assert(s[i] == prev[i]);
                } else {
                    assert(s[i] == prev[i]);
                    assert(s[j] == prev[j]);
                }
            }
            assert forall|i: int| 0 <= i < s.len() implies facts.contains_key(#[trigger] s[i]) by {
                if i < prev.len() {
                    assert(s[i] == prev[i]);
                }
            }
            assert forall|i: int| 0 <= i < s.len() implies pred(#[trigger] s[i])
                && facts[s[i]].timestamp < n by {
                if i < prev.len() {
                    assert(s[i] == prev[i]);
                }
            }
            assert forall|id: I|
                facts.contains_key(id) && pred(id) && facts[id].timestamp < n implies #[trigger] s.contains(
                id,
            ) by {
                if facts[id].timestamp == n - 1 {
                    assert(id == x);
                    assert(s[prev.len() as int] == x);
                } else {
                    assert(prev.contains(id));
                    let k = choose|k: int| 0 <= k < prev.len() && prev[k] == id;
                    assert(s[k] == id);
                }
            }
        } else {
            assert forall|id: I|
                facts.contains_key(id) && pred(id) && facts[id].timestamp < n implies #[trigger] s.contains(
                id,
            ) by {
                assert(facts[id].timestamp != n - 1);
                assert(prev.contains(id));
            }
        }
    }
}

/// `retain(|candidate| *candidate != id)` keeps the remaining candidates in
/// order, and they stay live after `id` is removed from the slot map.
pub proof fn lemma_filter_other_than<I, R, Tm, C>(
    facts: Map<I, EntryModel<R, Tm, C>>,
    s: Seq<I>,
    id: I,
)
    requires
        sorted_live(facts, s),
    ensures
        sorted_live(facts.remove(id), s.filter(other_than(id))),
        forall|x: I| #[trigger] s.filter(other_than(id)).contains(x) <==> s.contains(x) && x != id,
    decreases s.len(),
{
    reveal_with_fuel(Seq::filter, 1);
    let out = s.filter(other_than(id));
    if s.len() == 0 {
        assert(out =~= s);
    } else {
        let prefix = s.drop_last();
        let last = s.last();
        assert forall|i: int| 0 <= i < prefix.len() implies facts.contains_key(#[trigger] prefix[i]) by {
            assert(prefix[i] == s[i]);
        }
        assert forall|i: int, j: int|
            0 <= i < j < prefix.len() implies facts[#[trigger] prefix[i]].timestamp
            < facts[#[trigger] prefix[j]].timestamp by {
            assert(prefix[i] == s[i]);
            assert(prefix[j] == s[j]);
        }
        lemma_filter_other_than(facts, prefix, id);
        let sub = prefix.filter(other_than(id));
        // Every kept prefix element precedes `last`.
        assert forall|k: int| 0 <= k < sub.len() implies facts[#[trigger] sub[k]].timestamp
            < facts[last].timestamp by {
            let y = sub[k];
            assert(sub.contains(y)) by {
                assert(sub[k] == y);
            }
            assert(prefix.contains(y));
            let i = choose|i: int| 0 <= i < prefix.len() && prefix[i] == y;
            assert(s[i] == y);
        }
        if last != id {
            assert(out == sub.push(last));
            assert forall|i: int, j: int|
                0 <= i < j < out.len() implies facts.remove(id)[#[trigger] out[i]].timestamp
                < facts.remove(id)[#[trigger] out[j]].timestamp by {
                assert(out[i] == sub[i]);
                if j < sub.len() {
                    assert(out[j] == sub[j]);
                }
            }
            assert forall|i: int| 0 <= i < out.len() implies facts.remove(id).contains_key(
                #[trigger] out[i],
            ) by {
                if i < sub.len() {
                    assert(out[i] == sub[i]);
                }
            }
            assert forall|x: I| #[trigger] out.contains(x) <==> s.contains(x) && x != id by {
                if out.contains(x) {
                    let k = choose|k: int| 0 <= k < out.len() && out[k] == x;
                    if k < sub.len() {
                        assert(sub[k] == x);
                        assert(sub.contains(x));
                        let i = choose|i: int| 0 <= i < prefix.len() && prefix[i] == x;
                        assert(s[i] == x);
                    } else {
                        assert(s[s.len() - 1] == x);
                    }
                }
                if s.contains(x) && x != id {
                    let i = choose|i: int| 0 <= i < s.len() && s[i] == x;
                    if i < prefix.len() {
                        assert(prefix[i] == x);
                        assert(sub.contains(x));
                        let k = choose|k: int| 0 <= k < sub.len() && sub[k] == x;
                        assert(out[k] == x);
                    } else {
                        assert(out[out.len() - 1] == x);
                    }
                }
            }
        } else {
            assert(out == sub);
            assert forall|x: I| #[trigger] out.contains(x) <==> s.contains(x) && x != id by {
                if s.contains(x) && x != id {
                    let i = choose|i: int| 0 <= i < s.len() && s[i] == x;
                    assert(i < prefix.len());
                    assert(prefix[i] == x);
                }
                if out.contains(x) {
                    assert(prefix.contains(x));
                    let i = choose|i: int| 0 <= i < prefix.len() && prefix[i] == x;
                    assert(s[i] == x);
                }
            }
        }
    }
}

/// `find_first` over an oldest-first candidate list returns the oldest
/// candidate equal to `fact`, or `None` when no candidate is.
pub proof fn lemma_find_first<I, R, Tm, C>(
    facts: Map<I, EntryModel<R, Tm, C>>,
    candidates: Seq<I>,
    fact: FactModel<R, Tm, C>,
)
    requires
        sorted_live(facts, candidates),
    ensures
        find_first(facts, candidates, fact) is None ==> forall|i: int|
            0 <= i < candidates.len() ==> facts[#[trigger] candidates[i]].fact != fact,
        find_first(facts, candidates, fact) matches Some(found) ==> {
            &&& candidates.contains(found)
            &&& facts[found].fact == fact
            &&& forall|i: int|
                0 <= i < candidates.len() && facts[#[trigger] candidates[i]].fact == fact
                    ==> facts[found].timestamp <= facts[candidates[i]].timestamp
        },
    decreases candidates.len(),
{
    if candidates.len() > 0 {
        let rest = candidates.drop_first();
        assert forall|i: int| 0 <= i < rest.len() implies facts.contains_key(#[trigger] rest[i]) by {
            assert(rest[i] == candidates[i + 1]);
        }
        assert forall|i: int, j: int| 0 <= i < j < rest.len() implies facts[#[trigger] rest[i]].timestamp
            < facts[#[trigger] rest[j]].timestamp by {
            assert(rest[i] == candidates[i + 1]);
            assert(rest[j] == candidates[j + 1]);
        }
        lemma_find_first(facts, rest, fact);
        if facts.contains_key(candidates[0]) && facts[candidates[0]].fact == fact {
            assert(candidates[0] == candidates[0]);
            assert forall|i: int|
                0 <= i < candidates.len() && facts[#[trigger] candidates[i]].fact == fact implies facts[candidates[0]].timestamp
                <= facts[candidates[i]].timestamp by {
                if i > 0 {
                    assert(facts[candidates[0]].timestamp < facts[candidates[i]].timestamp);
                }
            }
        } else {
            assert forall|i: int|
                0 <= i < candidates.len() && facts[#[trigger] candidates[i]].fact == fact implies i
                > 0 by {}
            if let Some(found) = find_first(facts, rest, fact) {
                let k = choose|k: int| 0 <= k < rest.len() && rest[k] == found;
                assert(candidates[k + 1] == found);
                assert forall|i: int|
                    0 <= i < candidates.len() && facts[#[trigger] candidates[i]].fact == fact implies facts[found].timestamp
                    <= facts[candidates[i]].timestamp by {
                    assert(i > 0);
                    assert(rest[i - 1] == candidates[i]);
                }
            } else {
                assert forall|i: int| 0 <= i < candidates.len() implies facts[#[trigger] candidates[i]].fact
                    != fact by {
                    if i > 0 {
                        assert(rest[i - 1] == candidates[i]);
                    }
                }
            }
        }
    }
}

impl<I, R, Tm, C> FactBaseModel<I, R, Tm, C> {
    // -----------------------------------------------------------------------
    // Invariant
    // -----------------------------------------------------------------------
    pub open spec fn timestamps_ok(self) -> bool {
        &&& self.next_timestamp <= U64_MAX
        &&& forall|id: I| #[trigger]
            self.facts.contains_key(id) ==> self.facts[id].timestamp < self.next_timestamp
        &&& forall|a: I, b: I|
            #[trigger] self.facts.contains_key(a) && #[trigger] self.facts.contains_key(b) && a
                != b ==> self.facts[a].timestamp != self.facts[b].timestamp
    }

    pub open spec fn relation_index_ok(self) -> bool {
        &&& forall|r: R| #[trigger] self.by_relation.contains_key(r) ==> self.by_relation[r].len() > 0
        &&& forall|r: R, id: I|
            #[trigger] self.by_relation.contains_key(r) && #[trigger] self.by_relation[r].contains(
                id,
            ) ==> self.facts.contains_key(id) && relation_of(self.facts[id].fact) == Some(r)
        &&& forall|id: I| #[trigger]
            self.facts.contains_key(id) && relation_of(self.facts[id].fact) is Some ==> {
                let r = relation_of(self.facts[id].fact)->0;
                self.by_relation.contains_key(r) && self.by_relation[r].contains(id)
            }
    }

    pub open spec fn template_index_ok(self) -> bool {
        &&& forall|t: Tm| #[trigger] self.by_template.contains_key(t) ==> self.by_template[t].len() > 0
        &&& forall|t: Tm, id: I|
            #[trigger] self.by_template.contains_key(t) && #[trigger] self.by_template[t].contains(
                id,
            ) ==> self.facts.contains_key(id) && template_of(self.facts[id].fact) == Some(t)
        &&& forall|id: I| #[trigger]
            self.facts.contains_key(id) && template_of(self.facts[id].fact) is Some ==> {
                let t = template_of(self.facts[id].fact)->0;
                self.by_template.contains_key(t) && self.by_template[t].contains(id)
            }
    }

    /// The fingerprint index (when present) is exact and oldest-first.
    pub open spec fn index_exact(
        self,
        index: Map<u64, Seq<I>>,
        fp: spec_fn(FactModel<R, Tm, C>) -> u64,
    ) -> bool {
        &&& forall|f: u64| #[trigger] index.contains_key(f) ==> index[f].len() > 0
        &&& forall|f: u64, i: int|
            #[trigger] index.contains_key(f) && 0 <= i < index[f].len() ==> self.facts.contains_key(
                #[trigger] index[f][i],
            ) && fp(self.facts[index[f][i]].fact) == f
        &&& forall|f: u64, i: int, j: int|
            #[trigger] index.contains_key(f) && 0 <= i < j < index[f].len()
                ==> self.facts[#[trigger] index[f][i]].timestamp < self.facts[#[trigger] index[f][j]].timestamp
        &&& forall|id: I| #[trigger]
            self.facts.contains_key(id) ==> index.contains_key(fp(self.facts[id].fact))
                && index[fp(self.facts[id].fact)].contains(id)
    }

    pub open spec fn chronologies_ok(self) -> bool {
        &&& forall|r: R, ts: nat|
            #[trigger] self.by_relation_chronology.contains_key(r)
                && #[trigger] self.by_relation_chronology[r].contains_key(ts) ==> {
                let id = self.by_relation_chronology[r][ts];
                &&& self.facts.contains_key(id)
                &&& self.facts[id].timestamp == ts
                &&& relation_of(self.facts[id].fact) == Some(r)
            }
        &&& forall|r: R, id: I|
            #[trigger] self.by_relation_chronology.contains_key(r) && #[trigger] self.facts.contains_key(
                id,
            ) && relation_of(self.facts[id].fact) == Some(r)
                ==> self.by_relation_chronology[r].contains_key(self.facts[id].timestamp)
        &&& forall|t: Tm, ts: nat|
            #[trigger] self.by_template_chronology.contains_key(t)
                && #[trigger] self.by_template_chronology[t].contains_key(ts) ==> {
                let id = self.by_template_chronology[t][ts];
                &&& self.facts.contains_key(id)
                &&& self.facts[id].timestamp == ts
                &&& template_of(self.facts[id].fact) == Some(t)
            }
        &&& forall|t: Tm, id: I|
            #[trigger] self.by_template_chronology.contains_key(t) && #[trigger] self.facts.contains_key(
                id,
            ) && template_of(self.facts[id].fact) == Some(t)
                ==> self.by_template_chronology[t].contains_key(self.facts[id].timestamp)
    }

    pub open spec fn inv(self, fp: spec_fn(FactModel<R, Tm, C>) -> u64) -> bool {
        &&& self.timestamps_ok()
        &&& self.relation_index_ok()
        &&& self.template_index_ok()
        &&& self.chronologies_ok()
        &&& (self.by_structural_fingerprint matches Some(index) ==> self.index_exact(index, fp))
    }

    // -----------------------------------------------------------------------
    // Operations
    // -----------------------------------------------------------------------
    /// Models crates/ferric-rules-core/src/fact.rs:410 fn new.
    pub open spec fn new_model() -> Self {
        FactBaseModel {
            facts: Map::empty(),
            by_template: Map::empty(),
            by_relation: Map::empty(),
            by_structural_fingerprint: Some(Map::empty()),
            by_template_chronology: Map::empty(),
            by_relation_chronology: Map::empty(),
            next_timestamp: 0,
        }
    }

    /// Models crates/ferric-rules-core/src/fact.rs:422 fn insert_fact; `id` is
    /// the fresh key chosen by `insert_with_key`.
    pub open spec fn insert_fact(
        self,
        fact: FactModel<R, Tm, C>,
        fingerprint: Option<u64>,
        id: I,
    ) -> Option<Self> {
        if self.next_timestamp + 1 > U64_MAX {
            // `checked_add(1).ok_or(FactTimestampExhausted)?`
            None
        } else {
            let timestamp = self.next_timestamp;
            let facts = self.facts.insert(id, EntryModel { fact, timestamp });
            let by_structural_fingerprint = match (self.by_structural_fingerprint, fingerprint) {
                (Some(index), Some(f)) => Some(
                    index.insert(
                        f,
                        if index.contains_key(f) {
                            index[f]
                        } else {
                            Seq::empty()
                        }.push(id),
                    ),
                ),
                _ => self.by_structural_fingerprint,
            };
            Some(
                FactBaseModel {
                    facts,
                    by_structural_fingerprint,
                    next_timestamp: self.next_timestamp + 1,
                    ..self
                },
            )
        }
    }

    /// Models crates/ferric-rules-core/src/fact.rs:567 fn insert_indexed.
    pub open spec fn insert_indexed(
        self,
        fact: FactModel<R, Tm, C>,
        fingerprint: Option<u64>,
        id: I,
    ) -> Option<Self> {
        match self.insert_fact(fact, fingerprint, id) {
            None => None,
            Some(inserted) => {
                let timestamp = inserted.facts[id].timestamp;
                match fact {
                    FactModel::Ordered { relation, .. } => Some(
                        FactBaseModel {
                            by_relation: add_to_set_index(inserted.by_relation, relation, id),
                            by_relation_chronology: chronology_insert(
                                inserted.by_relation_chronology,
                                relation,
                                timestamp,
                                id,
                            ),
                            ..inserted
                        },
                    ),
                    FactModel::Template { template, .. } => Some(
                        FactBaseModel {
                            by_template: add_to_set_index(inserted.by_template, template, id),
                            by_template_chronology: chronology_insert(
                                inserted.by_template_chronology,
                                template,
                                timestamp,
                                id,
                            ),
                            ..inserted
                        },
                    ),
                }
            },
        }
    }

    /// The rebuilt fingerprint index.
    pub open spec fn rebuilt_index(self, fp: spec_fn(FactModel<R, Tm, C>) -> u64) -> Map<u64, Seq<I>> {
        Map::new(
            self.facts.dom().map(|id: I| fp(self.facts[id].fact)),
            |f: u64| ids_by_time(self.facts, fingerprint_is(self.facts, fp, f), self.next_timestamp),
        )
    }

    /// Models crates/ferric-rules-core/src/fact.rs:456 fn ensure_structural_index.
    pub open spec fn ensure_structural_index(self, fp: spec_fn(FactModel<R, Tm, C>) -> u64) -> Self {
        if self.by_structural_fingerprint is Some {
            self
        } else {
            FactBaseModel { by_structural_fingerprint: Some(self.rebuilt_index(fp)), ..self }
        }
    }

    /// Models crates/ferric-rules-core/src/fact.rs:528 fn try_assert_fact.
    /// Returns the new state and the outcome (`Exhausted` is the `Err`).
    pub open spec fn try_assert_fact(
        self,
        fact: FactModel<R, Tm, C>,
        allow_duplicates: bool,
        fp: spec_fn(FactModel<R, Tm, C>) -> u64,
        id: I,
    ) -> (Self, InsertOutcome<I>) {
        if allow_duplicates {
            let cleared = FactBaseModel { by_structural_fingerprint: None, ..self };
            match cleared.insert_indexed(fact, None, id) {
                Some(next) => (next, InsertOutcome::Inserted(id)),
                None => (cleared, InsertOutcome::Exhausted),
            }
        } else {
            let indexed = self.ensure_structural_index(fp);
            let fingerprint = fp(fact);
            let index = indexed.by_structural_fingerprint->0;
            let existing = if index.contains_key(fingerprint) {
                find_first(indexed.facts, index[fingerprint], fact)
            } else {
                None
            };
            match existing {
                Some(existing) => (indexed, InsertOutcome::Duplicate(existing)),
                None => match indexed.insert_indexed(fact, Some(fingerprint), id) {
                    Some(next) => (next, InsertOutcome::Inserted(id)),
                    None => (indexed, InsertOutcome::Exhausted),
                },
            }
        }
    }

    /// Models crates/ferric-rules-core/src/fact.rs:556 fn try_assert_distinct_fact.
    pub open spec fn try_assert_distinct_fact(
        self,
        fact: FactModel<R, Tm, C>,
        fp: spec_fn(FactModel<R, Tm, C>) -> u64,
        id: I,
    ) -> (Self, InsertOutcome<I>) {
        let fingerprint = match self.by_structural_fingerprint {
            Some(_) => Some(fp(fact)),
            None => None,
        };
        match self.insert_indexed(fact, fingerprint, id) {
            Some(next) => (next, InsertOutcome::Inserted(id)),
            None => (self, InsertOutcome::Exhausted),
        }
    }

    /// Models crates/ferric-rules-core/src/fact.rs:623 fn retract.
    pub open spec fn retract(self, id: I, fp: spec_fn(FactModel<R, Tm, C>) -> u64) -> (
        Self,
        Option<EntryModel<R, Tm, C>>,
    ) {
        if !self.facts.contains_key(id) {
            (self, None)
        } else {
            let entry = self.facts[id];
            let facts = self.facts.remove(id);
            let removed = match entry.fact {
                FactModel::Ordered { relation, .. } => FactBaseModel {
                    facts,
                    by_relation: remove_from_set_index(self.by_relation, relation, id),
                    by_relation_chronology: chronology_remove(
                        self.by_relation_chronology,
                        relation,
                        entry.timestamp,
                    ),
                    ..self
                },
                FactModel::Template { template, .. } => FactBaseModel {
                    facts,
                    by_template: remove_from_set_index(self.by_template, template, id),
                    by_template_chronology: chronology_remove(
                        self.by_template_chronology,
                        template,
                        entry.timestamp,
                    ),
                    ..self
                },
            };
            let by_structural_fingerprint = match self.by_structural_fingerprint {
                Some(index) => Some(remove_from_candidate_index(index, fp(entry.fact), id)),
                None => None,
            };
            (FactBaseModel { by_structural_fingerprint, ..removed }, Some(entry))
        }
    }

    // -----------------------------------------------------------------------
    // Proofs
    // -----------------------------------------------------------------------
    pub proof fn lemma_new(fp: spec_fn(FactModel<R, Tm, C>) -> u64)
        ensures
            Self::new_model().inv(fp),
    {
    }

    /// The fingerprint a caller passes to `insert_indexed`: present exactly
    /// when the index is.
    pub open spec fn matching_fingerprint(
        self,
        fact: FactModel<R, Tm, C>,
        fp: spec_fn(FactModel<R, Tm, C>) -> u64,
    ) -> Option<u64> {
        match self.by_structural_fingerprint {
            Some(_) => Some(fp(fact)),
            None => None,
        }
    }

    pub proof fn lemma_insert_indexed(
        self,
        fact: FactModel<R, Tm, C>,
        id: I,
        fp: spec_fn(FactModel<R, Tm, C>) -> u64,
    )
        requires
            self.inv(fp),
            !self.facts.contains_key(id),
        ensures
            ({
                let result = self.insert_indexed(fact, self.matching_fingerprint(fact, fp), id);
                &&& result is None <==> self.next_timestamp == U64_MAX
                &&& result matches Some(next) ==> {
                    &&& next.inv(fp)
                    &&& next.facts == self.facts.insert(
                        id,
                        EntryModel { fact, timestamp: self.next_timestamp },
                    )
                    &&& next.next_timestamp == self.next_timestamp + 1
                    &&& (next.by_structural_fingerprint is Some) == (
                    self.by_structural_fingerprint is Some)
                }
            }),
    {
        let fingerprint = self.matching_fingerprint(fact, fp);
        if self.next_timestamp == U64_MAX {
            return ;
        }
        let inserted = self.insert_fact(fact, fingerprint, id)->0;
        let next = self.insert_indexed(fact, fingerprint, id)->0;
        let ts = self.next_timestamp;
        assert(next.facts == self.facts.insert(id, EntryModel { fact, timestamp: ts }));
        // Every other live fact is unchanged and strictly older.
        assert forall|x: I| #[trigger]
            next.facts.contains_key(x) && x != id implies self.facts.contains_key(x)
            && next.facts[x] == self.facts[x] && next.facts[x].timestamp < ts by {}
        assert(next.timestamps_ok());
        assert(next.relation_index_ok()) by {
            assert forall|r: R, x: I|
                #[trigger] next.by_relation.contains_key(r) && #[trigger] next.by_relation[r].contains(
                    x,
                ) implies next.facts.contains_key(x) && relation_of(next.facts[x].fact) == Some(
                r,
            ) by {
                if x != id {
                    assert(self.by_relation.contains_key(r) && self.by_relation[r].contains(x));
                }
            }
            assert forall|x: I| #[trigger]
                next.facts.contains_key(x) && relation_of(next.facts[x].fact) is Some implies {
                let r = relation_of(next.facts[x].fact)->0;
                next.by_relation.contains_key(r) && next.by_relation[r].contains(x)
            } by {
                if x != id {
                    assert(self.facts.contains_key(x));
                }
            }
            assert forall|r: R| #[trigger]
                next.by_relation.contains_key(r) implies next.by_relation[r].len() > 0 by {
                if relation_of(fact) == Some(r) {
                    assert(next.by_relation[r].contains(id));
                }
            }
        }
        assert(next.template_index_ok()) by {
            assert forall|t: Tm, x: I|
                #[trigger] next.by_template.contains_key(t) && #[trigger] next.by_template[t].contains(
                    x,
                ) implies next.facts.contains_key(x) && template_of(next.facts[x].fact) == Some(
                t,
            ) by {
                if x != id {
                    assert(self.by_template.contains_key(t) && self.by_template[t].contains(x));
                }
            }
            assert forall|x: I| #[trigger]
                next.facts.contains_key(x) && template_of(next.facts[x].fact) is Some implies {
                let t = template_of(next.facts[x].fact)->0;
                next.by_template.contains_key(t) && next.by_template[t].contains(x)
            } by {
                if x != id {
                    assert(self.facts.contains_key(x));
                }
            }
            assert forall|t: Tm| #[trigger]
                next.by_template.contains_key(t) implies next.by_template[t].len() > 0 by {
                if template_of(fact) == Some(t) {
                    assert(next.by_template[t].contains(id));
                }
            }
        }
        assert(next.chronologies_ok()) by {
            assert forall|r: R, k: nat|
                #[trigger] next.by_relation_chronology.contains_key(r)
                    && #[trigger] next.by_relation_chronology[r].contains_key(k) implies {
                let x = next.by_relation_chronology[r][k];
                &&& next.facts.contains_key(x)
                &&& next.facts[x].timestamp == k
                &&& relation_of(next.facts[x].fact) == Some(r)
            } by {
                assert(self.by_relation_chronology.contains_key(r));
                if !(relation_of(fact) == Some(r) && k == ts) {
                    assert(self.by_relation_chronology[r].contains_key(k));
                    assert(self.facts.contains_key(self.by_relation_chronology[r][k]));
                }
            }
            assert forall|r: R, x: I|
                #[trigger] next.by_relation_chronology.contains_key(r)
                    && #[trigger] next.facts.contains_key(x) && relation_of(next.facts[x].fact)
                    == Some(r) implies next.by_relation_chronology[r].contains_key(
                next.facts[x].timestamp,
            ) by {
                assert(self.by_relation_chronology.contains_key(r));
                if x != id {
                    assert(self.facts.contains_key(x));
                }
            }
            assert forall|t: Tm, k: nat|
                #[trigger] next.by_template_chronology.contains_key(t)
                    && #[trigger] next.by_template_chronology[t].contains_key(k) implies {
                let x = next.by_template_chronology[t][k];
                &&& next.facts.contains_key(x)
                &&& next.facts[x].timestamp == k
                &&& template_of(next.facts[x].fact) == Some(t)
            } by {
                assert(self.by_template_chronology.contains_key(t));
                if !(template_of(fact) == Some(t) && k == ts) {
                    assert(self.by_template_chronology[t].contains_key(k));
                    assert(self.facts.contains_key(self.by_template_chronology[t][k]));
                }
            }
            assert forall|t: Tm, x: I|
                #[trigger] next.by_template_chronology.contains_key(t)
                    && #[trigger] next.facts.contains_key(x) && template_of(next.facts[x].fact)
                    == Some(t) implies next.by_template_chronology[t].contains_key(
                next.facts[x].timestamp,
            ) by {
                assert(self.by_template_chronology.contains_key(t));
                if x != id {
                    assert(self.facts.contains_key(x));
                }
            }
        }
        if let Some(index) = self.by_structural_fingerprint {
            let f = fp(fact);
            let index2 = next.by_structural_fingerprint->0;
            assert forall|g: u64, i: int|
                #[trigger] index2.contains_key(g) && 0 <= i < index2[g].len() implies next.facts.contains_key(
                #[trigger] index2[g][i],
            ) && fp(next.facts[index2[g][i]].fact) == g by {
                if g == f && index.contains_key(f) && i < index[f].len() {
                    assert(index2[g][i] == index[f][i]);
                } else if g != f {
                    assert(index2[g][i] == index[g][i]);
                }
            }
            assert forall|g: u64, i: int, j: int|
                #[trigger] index2.contains_key(g) && 0 <= i < j < index2[g].len()
                    implies next.facts[#[trigger] index2[g][i]].timestamp < next.facts[#[trigger] index2[g][j]].timestamp by {
                if g == f {
                    if index.contains_key(f) {
                        assert(index2[g][i] == index[f][i]);
                        if j < index[f].len() {
                            assert(index2[g][j] == index[f][j]);
                        } else {
                            assert(index2[g][j] == id);
                            assert(self.facts.contains_key(index[f][i]));
                        }
                    }
                } else {
                    assert(index2[g][i] == index[g][i]);
                    assert(index2[g][j] == index[g][j]);
                    assert(self.facts.contains_key(index[g][i]));
                    assert(self.facts.contains_key(index[g][j]));
                }
            }
            assert forall|x: I| #[trigger] next.facts.contains_key(x) implies index2.contains_key(
                fp(next.facts[x].fact),
            ) && index2[fp(next.facts[x].fact)].contains(x) by {
                if x == id {
                    assert(index2[f][index2[f].len() - 1] == id);
                } else {
                    assert(self.facts.contains_key(x));
                    let g = fp(self.facts[x].fact);
                    assert(index[g].contains(x));
                    let k = choose|k: int| 0 <= k < index[g].len() && index[g][k] == x;
                    assert(index2[g][k] == x);
                }
            }
            assert forall|g: u64| #[trigger] index2.contains_key(g) implies index2[g].len() > 0 by {}
            assert(next.index_exact(index2, fp));
        }
    }

    /// The rebuilt index satisfies the index invariant.
    pub proof fn lemma_rebuilt_index(self, fp: spec_fn(FactModel<R, Tm, C>) -> u64)
        requires
            self.timestamps_ok(),
        ensures
            self.index_exact(self.rebuilt_index(fp), fp),
    {
        let index = self.rebuilt_index(fp);
        let keys = self.facts.dom().map(|id: I| fp(self.facts[id].fact));
        assert(index.dom() == keys);
        assert forall|f: u64| #[trigger] index.contains_key(f) implies index[f]
            == ids_by_time(self.facts, fingerprint_is(self.facts, fp, f), self.next_timestamp) by {}
        assert forall|f: u64, i: int|
            #[trigger] index.contains_key(f) && 0 <= i < index[f].len() implies self.facts.contains_key(
            #[trigger] index[f][i],
        ) && fp(self.facts[index[f][i]].fact) == f by {
            lemma_ids_by_time(self.facts, fingerprint_is(self.facts, fp, f), self.next_timestamp);
        }
        assert forall|f: u64, i: int, j: int|
            #[trigger] index.contains_key(f) && 0 <= i < j < index[f].len()
                implies self.facts[#[trigger] index[f][i]].timestamp < self.facts[#[trigger] index[f][j]].timestamp by {
            lemma_ids_by_time(self.facts, fingerprint_is(self.facts, fp, f), self.next_timestamp);
        }
        assert forall|id: I| #[trigger] self.facts.contains_key(id) implies index.contains_key(
            fp(self.facts[id].fact),
        ) && index[fp(self.facts[id].fact)].contains(id) by {
            let f = fp(self.facts[id].fact);
            assert(keys.contains(f));
            lemma_ids_by_time(self.facts, fingerprint_is(self.facts, fp, f), self.next_timestamp);
        }
        assert forall|f: u64| #[trigger] index.contains_key(f) implies index[f].len() > 0 by {
            let id = choose|id: I| #[trigger] self.facts.contains_key(id) && fp(self.facts[id].fact) == f;
            lemma_ids_by_time(self.facts, fingerprint_is(self.facts, fp, f), self.next_timestamp);
            assert(index[f].contains(id));
        }
    }

    pub proof fn lemma_retract(self, id: I, fp: spec_fn(FactModel<R, Tm, C>) -> u64)
        requires
            self.inv(fp),
        ensures
            ({
                let (next, entry) = self.retract(id, fp);
                &&& next.inv(fp)
                &&& entry == (if self.facts.contains_key(id) {
                    Some(self.facts[id])
                } else {
                    None
                })
                &&& next.facts == self.facts.remove(id)
                &&& next.next_timestamp == self.next_timestamp
            }),
    {
        if !self.facts.contains_key(id) {
            assert(self.facts.remove(id) =~= self.facts);
            return ;
        }
        let entry = self.facts[id];
        let next = self.retract(id, fp).0;
        assert(next.facts =~= self.facts.remove(id));
        assert(next.timestamps_ok());
        assert(next.relation_index_ok()) by {
            assert forall|r: R, x: I|
                #[trigger] next.by_relation.contains_key(r) && #[trigger] next.by_relation[r].contains(
                    x,
                ) implies next.facts.contains_key(x) && relation_of(next.facts[x].fact) == Some(
                r,
            ) by {
                assert(self.by_relation.contains_key(r) && self.by_relation[r].contains(x));
            }
            assert forall|x: I| #[trigger]
                next.facts.contains_key(x) && relation_of(next.facts[x].fact) is Some implies {
                let r = relation_of(next.facts[x].fact)->0;
                next.by_relation.contains_key(r) && next.by_relation[r].contains(x)
            } by {
                assert(self.facts.contains_key(x));
                let r = relation_of(self.facts[x].fact)->0;
                assert(self.by_relation[r].contains(x));
                if relation_of(entry.fact) == Some(r) {
                    assert(self.by_relation[r].remove(id).contains(x));
                }
            }
        }
        assert(next.template_index_ok()) by {
            assert forall|t: Tm, x: I|
                #[trigger] next.by_template.contains_key(t) && #[trigger] next.by_template[t].contains(
                    x,
                ) implies next.facts.contains_key(x) && template_of(next.facts[x].fact) == Some(
                t,
            ) by {
                assert(self.by_template.contains_key(t) && self.by_template[t].contains(x));
            }
            assert forall|x: I| #[trigger]
                next.facts.contains_key(x) && template_of(next.facts[x].fact) is Some implies {
                let t = template_of(next.facts[x].fact)->0;
                next.by_template.contains_key(t) && next.by_template[t].contains(x)
            } by {
                assert(self.facts.contains_key(x));
                let t = template_of(self.facts[x].fact)->0;
                assert(self.by_template[t].contains(x));
                if template_of(entry.fact) == Some(t) {
                    assert(self.by_template[t].remove(id).contains(x));
                }
            }
        }
        assert(next.chronologies_ok()) by {
            assert forall|r: R, k: nat|
                #[trigger] next.by_relation_chronology.contains_key(r)
                    && #[trigger] next.by_relation_chronology[r].contains_key(k) implies {
                let x = next.by_relation_chronology[r][k];
                &&& next.facts.contains_key(x)
                &&& next.facts[x].timestamp == k
                &&& relation_of(next.facts[x].fact) == Some(r)
            } by {
                assert(self.by_relation_chronology.contains_key(r));
                assert(self.by_relation_chronology[r].contains_key(k));
                let x = self.by_relation_chronology[r][k];
                assert(self.facts.contains_key(x));
                if x == id {
                    assert(k == entry.timestamp);
                }
            }
            assert forall|r: R, x: I|
                #[trigger] next.by_relation_chronology.contains_key(r)
                    && #[trigger] next.facts.contains_key(x) && relation_of(next.facts[x].fact)
                    == Some(r) implies next.by_relation_chronology[r].contains_key(
                next.facts[x].timestamp,
            ) by {
                assert(self.facts.contains_key(x));
                assert(self.facts[x].timestamp != entry.timestamp);
            }
            assert forall|t: Tm, k: nat|
                #[trigger] next.by_template_chronology.contains_key(t)
                    && #[trigger] next.by_template_chronology[t].contains_key(k) implies {
                let x = next.by_template_chronology[t][k];
                &&& next.facts.contains_key(x)
                &&& next.facts[x].timestamp == k
                &&& template_of(next.facts[x].fact) == Some(t)
            } by {
                assert(self.by_template_chronology.contains_key(t));
                assert(self.by_template_chronology[t].contains_key(k));
                let x = self.by_template_chronology[t][k];
                assert(self.facts.contains_key(x));
                if x == id {
                    assert(k == entry.timestamp);
                }
            }
            assert forall|t: Tm, x: I|
                #[trigger] next.by_template_chronology.contains_key(t)
                    && #[trigger] next.facts.contains_key(x) && template_of(next.facts[x].fact)
                    == Some(t) implies next.by_template_chronology[t].contains_key(
                next.facts[x].timestamp,
            ) by {
                assert(self.facts.contains_key(x));
                assert(self.facts[x].timestamp != entry.timestamp);
            }
        }
        if let Some(index) = self.by_structural_fingerprint {
            let f = fp(entry.fact);
            let index2 = next.by_structural_fingerprint->0;
            assert(index.contains_key(f));
            assert forall|g: u64| #[trigger] index.contains_key(g) implies sorted_live(
                self.facts,
                index[g],
            ) by {}
            lemma_filter_other_than(self.facts, index[f], id);
            let kept = index[f].filter(other_than(id));
            assert forall|g: u64, i: int|
                #[trigger] index2.contains_key(g) && 0 <= i < index2[g].len() implies next.facts.contains_key(
                #[trigger] index2[g][i],
            ) && fp(next.facts[index2[g][i]].fact) == g by {
                if g == f {
                    assert(index2[g] == kept);
                    assert(kept.contains(kept[i]));
                } else {
                    assert(index2[g][i] == index[g][i]);
                    assert(index[g][i] != id);
                }
            }
            assert forall|g: u64, i: int, j: int|
                #[trigger] index2.contains_key(g) && 0 <= i < j < index2[g].len()
                    implies next.facts[#[trigger] index2[g][i]].timestamp < next.facts[#[trigger] index2[g][j]].timestamp by {
                if g != f {
                    assert(index2[g][i] == index[g][i]);
                    assert(index2[g][j] == index[g][j]);
                    assert(index[g][i] != id);
                    assert(index[g][j] != id);
                }
            }
            assert forall|x: I| #[trigger] next.facts.contains_key(x) implies index2.contains_key(
                fp(next.facts[x].fact),
            ) && index2[fp(next.facts[x].fact)].contains(x) by {
                assert(self.facts.contains_key(x));
                let g = fp(self.facts[x].fact);
                assert(index[g].contains(x));
                if g == f {
                    assert(kept.contains(x));
                }
            }
            assert(next.index_exact(index2, fp));
        }
    }

    /// Main theorem for `try_assert_fact`.
    pub proof fn lemma_try_assert_fact(
        self,
        fact: FactModel<R, Tm, C>,
        allow_duplicates: bool,
        fp: spec_fn(FactModel<R, Tm, C>) -> u64,
        id: I,
    )
        requires
            self.inv(fp),
            !self.facts.contains_key(id),
        ensures
            ({
                let (next, outcome) = self.try_assert_fact(fact, allow_duplicates, fp, id);
                let equivalent = exists|x: I| #[trigger]
                    self.facts.contains_key(x) && self.facts[x].fact == fact;
                &&& next.inv(fp)
                // duplicate detection: exactly when an equal fact is live, oldest first
                &&& outcome is Duplicate <==> !allow_duplicates && equivalent
                &&& outcome matches InsertOutcome::Duplicate(found) ==> {
                    &&& self.facts.contains_key(found)
                    &&& self.facts[found].fact == fact
                    &&& forall|x: I|
                        #[trigger] self.facts.contains_key(x) && self.facts[x].fact == fact
                            ==> self.facts[found].timestamp <= self.facts[x].timestamp
                }
                // exhaustion: only without a duplicate, exactly at u64::MAX
                &&& outcome is Exhausted <==> !(outcome is Duplicate) && self.next_timestamp
                    == U64_MAX
                // insertion: the next timestamp, then increment
                &&& outcome matches InsertOutcome::Inserted(x) ==> {
                    &&& x == id
                    &&& next.facts == self.facts.insert(
                        id,
                        EntryModel { fact, timestamp: self.next_timestamp },
                    )
                    &&& next.next_timestamp == self.next_timestamp + 1
                }
                // no insertion: working memory and its indexes are unchanged
                &&& !(outcome is Inserted) ==> {
                    &&& next.facts == self.facts
                    &&& next.next_timestamp == self.next_timestamp
                    &&& next.by_relation == self.by_relation
                    &&& next.by_template == self.by_template
                    &&& next.by_relation_chronology == self.by_relation_chronology
                    &&& next.by_template_chronology == self.by_template_chronology
                }
            }),
    {
        let equivalent = exists|x: I| #[trigger]
            self.facts.contains_key(x) && self.facts[x].fact == fact;
        if allow_duplicates {
            let cleared = FactBaseModel { by_structural_fingerprint: None, ..self };
            assert(cleared.inv(fp));
            assert(cleared.matching_fingerprint(fact, fp) == None::<u64>);
            cleared.lemma_insert_indexed(fact, id, fp);
        } else {
            let indexed = self.ensure_structural_index(fp);
            if self.by_structural_fingerprint is None {
                self.lemma_rebuilt_index(fp);
            }
            assert(indexed.inv(fp));
            let index = indexed.by_structural_fingerprint->0;
            let f = fp(fact);
            if index.contains_key(f) {
                assert(sorted_live(indexed.facts, index[f]));
                lemma_find_first(indexed.facts, index[f], fact);
                match find_first(indexed.facts, index[f], fact) {
                    Some(found) => {
                        let k = choose|k: int| 0 <= k < index[f].len() && index[f][k] == found;
                        assert(self.facts.contains_key(found));
                        assert forall|x: I| #[trigger]
                            self.facts.contains_key(x) && self.facts[x].fact == fact implies self.facts[found].timestamp
                            <= self.facts[x].timestamp by {
                            assert(index[f].contains(x));
                            let i = choose|i: int| 0 <= i < index[f].len() && index[f][i] == x;
                            assert(index[f][i] == x);
                        }
                    },
                    None => {
                        assert(!equivalent) by {
                            if equivalent {
                                let x = choose|x: I| #[trigger]
                                    self.facts.contains_key(x) && self.facts[x].fact == fact;
                                assert(index[f].contains(x));
                                let i = choose|i: int| 0 <= i < index[f].len() && index[f][i] == x;
                                assert(index[f][i] == x);
                            }
                        }
                        assert(indexed.matching_fingerprint(fact, fp) == Some(f));
                        indexed.lemma_insert_indexed(fact, id, fp);
                    },
                }
            } else {
                assert(!equivalent) by {
                    if equivalent {
                        let x = choose|x: I| #[trigger]
                            self.facts.contains_key(x) && self.facts[x].fact == fact;
                        assert(index.contains_key(fp(self.facts[x].fact)));
                    }
                }
                assert(indexed.matching_fingerprint(fact, fp) == Some(f));
                indexed.lemma_insert_indexed(fact, id, fp);
            }
        }
    }

    /// The chronology built on first use: the live members of
    /// `by_relation[relation]`, keyed by timestamp.
    pub open spec fn built_relation_chronology(self, relation: R) -> Map<nat, I> {
        let ids = if self.by_relation.contains_key(relation) {
            self.by_relation[relation]
        } else {
            Set::empty()
        };
        let live = ids.filter(|id: I| self.facts.contains_key(id));
        Map::new(
            live.map(|id: I| self.facts[id].timestamp),
            |ts: nat| choose|id: I| #[trigger] live.contains(id) && self.facts[id].timestamp == ts,
        )
    }

    /// Models crates/ferric-rules-core/src/fact.rs:729 fn next_relation_fact_after.
    pub open spec fn next_relation_fact_after(self, relation: R, after: Option<nat>) -> (
        Self,
        Option<(I, nat)>,
    ) {
        let chronology = if self.by_relation_chronology.contains_key(relation) {
            self.by_relation_chronology[relation]
        } else {
            self.built_relation_chronology(relation)
        };
        let next = FactBaseModel {
            by_relation_chronology: self.by_relation_chronology.insert(relation, chronology),
            ..self
        };
        match first_key_after(chronology, after) {
            None => (next, None),
            Some(k) => (next, Some((chronology[k], k))),
        }
    }

    /// The cursor returns the oldest live fact of `relation` strictly after
    /// `after` (any live fact for `None`), or `None` when there is none, and
    /// initializes the chronology without breaking the invariant.
    pub proof fn lemma_next_relation_fact_after(
        self,
        relation: R,
        after: Option<nat>,
        fp: spec_fn(FactModel<R, Tm, C>) -> u64,
    )
        requires
            self.inv(fp),
        ensures
            ({
                let (next, result) = self.next_relation_fact_after(relation, after);
                let eligible = |x: I|
                    self.facts.contains_key(x) && relation_of(self.facts[x].fact) == Some(relation)
                        && (after matches Some(t) ==> self.facts[x].timestamp > t);
                &&& next.inv(fp)
                &&& next.facts == self.facts
                &&& next.by_relation_chronology.contains_key(relation)
                &&& result is None <==> forall|x: I| !#[trigger] eligible(x)
                &&& result matches Some((found, ts)) ==> {
                    &&& eligible(found)
                    &&& self.facts[found].timestamp == ts
                    &&& forall|x: I| #[trigger] eligible(x) ==> ts <= self.facts[x].timestamp
                }
            }),
    {
        let chronology = if self.by_relation_chronology.contains_key(relation) {
            self.by_relation_chronology[relation]
        } else {
            self.built_relation_chronology(relation)
        };
        let (next, result) = self.next_relation_fact_after(relation, after);
        let eligible = |x: I|
            self.facts.contains_key(x) && relation_of(self.facts[x].fact) == Some(relation) && (
            after matches Some(t) ==> self.facts[x].timestamp > t);
        // The chronology in use is exact for `relation`.
        assert forall|k: nat| #[trigger] chronology.contains_key(k) implies {
            let x = chronology[k];
            &&& self.facts.contains_key(x)
            &&& self.facts[x].timestamp == k
            &&& relation_of(self.facts[x].fact) == Some(relation)
        } by {
            if !self.by_relation_chronology.contains_key(relation) {
                let ids = self.by_relation[relation];
                let live = ids.filter(|id: I| self.facts.contains_key(id));
                assert(live.map(|id: I| self.facts[id].timestamp).contains(k));
                let w = choose|id: I| #[trigger] live.contains(id) && self.facts[id].timestamp == k;
                assert(live.contains(w));
                assert(self.by_relation[relation].contains(w));
            }
        }
        assert forall|x: I|
            #[trigger] self.facts.contains_key(x) && relation_of(self.facts[x].fact) == Some(
                relation,
            ) implies chronology.contains_key(self.facts[x].timestamp) by {
            if !self.by_relation_chronology.contains_key(relation) {
                let ids = self.by_relation[relation];
                let live = ids.filter(|id: I| self.facts.contains_key(id));
                assert(ids.contains(x));
                assert(live.contains(x));
                assert(live.map(|id: I| self.facts[id].timestamp).contains(
                    self.facts[x].timestamp,
                ));
            }
        }
        lemma_first_key_after(chronology, after);
        let keys = keys_after(chronology, after);
        assert forall|x: I| #[trigger] eligible(x) implies keys.contains(self.facts[x].timestamp) by {
            assert(chronology.contains_key(self.facts[x].timestamp));
        }
        match first_key_after(chronology, after) {
            None => {
                assert forall|x: I| !#[trigger] eligible(x) by {
                    if eligible(x) {
                        assert(keys.contains(self.facts[x].timestamp));
                    }
                }
            },
            Some(k) => {
                assert(keys.contains(k));
                assert(chronology.contains_key(k));
                let found = chronology[k];
                assert(eligible(found));
            },
        }
        assert(next.chronologies_ok()) by {
            assert forall|r: R, k: nat|
                #[trigger] next.by_relation_chronology.contains_key(r)
                    && #[trigger] next.by_relation_chronology[r].contains_key(k) implies {
                let x = next.by_relation_chronology[r][k];
                &&& next.facts.contains_key(x)
                &&& next.facts[x].timestamp == k
                &&& relation_of(next.facts[x].fact) == Some(r)
            } by {
                if r != relation {
                    assert(self.by_relation_chronology.contains_key(r));
                }
            }
        }
    }

    /// The template chronology built on first use.
    pub open spec fn built_template_chronology(self, template: Tm) -> Map<nat, I> {
        let ids = if self.by_template.contains_key(template) {
            self.by_template[template]
        } else {
            Set::empty()
        };
        let live = ids.filter(|id: I| self.facts.contains_key(id));
        Map::new(
            live.map(|id: I| self.facts[id].timestamp),
            |ts: nat| choose|id: I| #[trigger] live.contains(id) && self.facts[id].timestamp == ts,
        )
    }

    /// Models crates/ferric-rules-core/src/fact.rs:693 fn next_template_fact_after.
    pub open spec fn next_template_fact_after(self, template: Tm, after: Option<nat>) -> (
        Self,
        Option<(I, nat)>,
    ) {
        let chronology = if self.by_template_chronology.contains_key(template) {
            self.by_template_chronology[template]
        } else {
            self.built_template_chronology(template)
        };
        let next = FactBaseModel {
            by_template_chronology: self.by_template_chronology.insert(template, chronology),
            ..self
        };
        match first_key_after(chronology, after) {
            None => (next, None),
            Some(k) => (next, Some((chronology[k], k))),
        }
    }

    /// The template cursor returns the oldest live fact of `template`
    /// strictly after `after` (any live fact for `None`), or `None` when there
    /// is none, and preserves the invariant.
    pub proof fn lemma_next_template_fact_after(
        self,
        template: Tm,
        after: Option<nat>,
        fp: spec_fn(FactModel<R, Tm, C>) -> u64,
    )
        requires
            self.inv(fp),
        ensures
            ({
                let (next, result) = self.next_template_fact_after(template, after);
                let eligible = |x: I|
                    self.facts.contains_key(x) && template_of(self.facts[x].fact) == Some(template)
                        && (after matches Some(t) ==> self.facts[x].timestamp > t);
                &&& next.inv(fp)
                &&& next.facts == self.facts
                &&& next.by_template_chronology.contains_key(template)
                &&& result is None <==> forall|x: I| !#[trigger] eligible(x)
                &&& result matches Some((found, ts)) ==> {
                    &&& eligible(found)
                    &&& self.facts[found].timestamp == ts
                    &&& forall|x: I| #[trigger] eligible(x) ==> ts <= self.facts[x].timestamp
                }
            }),
    {
        let chronology = if self.by_template_chronology.contains_key(template) {
            self.by_template_chronology[template]
        } else {
            self.built_template_chronology(template)
        };
        let (next, result) = self.next_template_fact_after(template, after);
        let eligible = |x: I|
            self.facts.contains_key(x) && template_of(self.facts[x].fact) == Some(template) && (
            after matches Some(t) ==> self.facts[x].timestamp > t);
        assert forall|k: nat| #[trigger] chronology.contains_key(k) implies {
            let x = chronology[k];
            &&& self.facts.contains_key(x)
            &&& self.facts[x].timestamp == k
            &&& template_of(self.facts[x].fact) == Some(template)
        } by {
            if !self.by_template_chronology.contains_key(template) {
                let ids = self.by_template[template];
                let live = ids.filter(|id: I| self.facts.contains_key(id));
                assert(live.map(|id: I| self.facts[id].timestamp).contains(k));
                let w = choose|id: I| #[trigger] live.contains(id) && self.facts[id].timestamp == k;
                assert(live.contains(w));
                assert(self.by_template[template].contains(w));
            }
        }
        assert forall|x: I|
            #[trigger] self.facts.contains_key(x) && template_of(self.facts[x].fact) == Some(
                template,
            ) implies chronology.contains_key(self.facts[x].timestamp) by {
            if !self.by_template_chronology.contains_key(template) {
                let ids = self.by_template[template];
                let live = ids.filter(|id: I| self.facts.contains_key(id));
                assert(ids.contains(x));
                assert(live.contains(x));
                assert(live.map(|id: I| self.facts[id].timestamp).contains(
                    self.facts[x].timestamp,
                ));
            }
        }
        lemma_first_key_after(chronology, after);
        let keys = keys_after(chronology, after);
        assert forall|x: I| #[trigger] eligible(x) implies keys.contains(self.facts[x].timestamp) by {
            assert(chronology.contains_key(self.facts[x].timestamp));
        }
        match first_key_after(chronology, after) {
            None => {
                assert forall|x: I| !#[trigger] eligible(x) by {
                    if eligible(x) {
                        assert(keys.contains(self.facts[x].timestamp));
                    }
                }
            },
            Some(k) => {
                assert(keys.contains(k));
                assert(chronology.contains_key(k));
                let found = chronology[k];
                assert(eligible(found));
            },
        }
        assert(next.chronologies_ok()) by {
            assert forall|t: Tm, k: nat|
                #[trigger] next.by_template_chronology.contains_key(t)
                    && #[trigger] next.by_template_chronology[t].contains_key(k) implies {
                let x = next.by_template_chronology[t][k];
                &&& next.facts.contains_key(x)
                &&& next.facts[x].timestamp == k
                &&& template_of(next.facts[x].fact) == Some(t)
            } by {
                if t != template {
                    assert(self.by_template_chronology.contains_key(t));
                }
            }
        }
    }

    /// Main theorem for `try_assert_distinct_fact`.
    pub proof fn lemma_try_assert_distinct_fact(
        self,
        fact: FactModel<R, Tm, C>,
        fp: spec_fn(FactModel<R, Tm, C>) -> u64,
        id: I,
    )
        requires
            self.inv(fp),
            !self.facts.contains_key(id),
        ensures
            ({
                let (next, outcome) = self.try_assert_distinct_fact(fact, fp, id);
                &&& next.inv(fp)
                &&& outcome is Exhausted <==> self.next_timestamp == U64_MAX
                &&& !(outcome is Duplicate)
                &&& outcome matches InsertOutcome::Inserted(x) ==> {
                    &&& x == id
                    &&& next.facts == self.facts.insert(
                        id,
                        EntryModel { fact, timestamp: self.next_timestamp },
                    )
                    &&& next.next_timestamp == self.next_timestamp + 1
                    &&& (next.by_structural_fingerprint is Some) == (
                    self.by_structural_fingerprint is Some)
                }
                &&& outcome is Exhausted ==> next == self
            }),
    {
        self.lemma_insert_indexed(fact, id, fp);
    }
}

// ---------------------------------------------------------------------------
// ensure_structural_index: the rebuild itself (grouping loop and sort)
// ---------------------------------------------------------------------------
/// The grouping loop of crates/ferric-rules-core/src/fact.rs:456
/// fn ensure_structural_index,
/// `for (id, entry) in &self.facts {
///     index.entry(structural_fingerprint(&entry.fact)).or_default().push(id); }`,
/// visiting the live ids in the slot map's iteration order `slot_order`.
pub open spec fn group_loop<I, R, Tm, C>(
    facts: Map<I, EntryModel<R, Tm, C>>,
    fp: spec_fn(FactModel<R, Tm, C>) -> u64,
    slot_order: Seq<I>,
    index: Map<u64, Seq<I>>,
) -> Map<u64, Seq<I>>
    decreases slot_order.len(),
{
    if slot_order.len() == 0 {
        index
    } else {
        let id = slot_order[0];
        let f = fp(facts[id].fact);
        let index2 = index.insert(
            f,
            if index.contains_key(f) {
                index[f]
            } else {
                Seq::empty()
            }.push(id),
        );
        group_loop(facts, fp, slot_order.drop_first(), index2)
    }
}

/// Non-decreasing in the sort key `self.facts.get(*id).expect(..).timestamp`.
pub open spec fn sorted_by_timestamp<I, R, Tm, C>(
    facts: Map<I, EntryModel<R, Tm, C>>,
    s: Seq<I>,
) -> bool {
    forall|i: int, j: int|
        0 <= i < j < s.len() ==> facts[#[trigger] s[i]].timestamp <= facts[#[trigger] s[j]].timestamp
}

/// The second loop of crates/ferric-rules-core/src/fact.rs:456
/// fn ensure_structural_index,
/// `for candidates in index.values_mut() { if candidates.len() > 1 {
///     candidates.sort_unstable_by_key(..) } }`: `sorted` is a possible index
/// after the loop. `sort_unstable_by_key` is specified by its documented
/// contract only: the result is a permutation (same multiset) of its input
/// that is non-decreasing in the key; groups of length at most one are left
/// untouched.
pub open spec fn sort_loop_result<I, R, Tm, C>(
    facts: Map<I, EntryModel<R, Tm, C>>,
    grouped: Map<u64, Seq<I>>,
    sorted: Map<u64, Seq<I>>,
) -> bool {
    &&& sorted.dom() == grouped.dom()
    &&& forall|f: u64| #[trigger]
        grouped.contains_key(f) ==> if grouped[f].len() > 1 {
            &&& sorted[f].to_multiset() == grouped[f].to_multiset()
            &&& sorted_by_timestamp(facts, sorted[f])
        } else {
            sorted[f] == grouped[f]
        }
}

/// The grouping loop's invariant after visiting `done`: every group is
/// non-empty and duplicate-free and lists exactly the visited ids with its
/// fingerprint.
pub open spec fn grouped_exactly<I, R, Tm, C>(
    facts: Map<I, EntryModel<R, Tm, C>>,
    fp: spec_fn(FactModel<R, Tm, C>) -> u64,
    done: Seq<I>,
    index: Map<u64, Seq<I>>,
) -> bool {
    &&& forall|f: u64| #[trigger] index.contains_key(f) ==> index[f].len() > 0
    &&& forall|f: u64| #[trigger] index.contains_key(f) ==> index[f].no_duplicates()
    &&& forall|f: u64, i: int|
        #[trigger] index.contains_key(f) && 0 <= i < index[f].len() ==> done.contains(
            #[trigger] index[f][i],
        ) && fp(facts[index[f][i]].fact) == f
    &&& forall|id: I| #[trigger]
        done.contains(id) ==> index.contains_key(fp(facts[id].fact)) && index[fp(
            facts[id].fact,
        )].contains(id)
}

pub proof fn lemma_group_loop<I, R, Tm, C>(
    facts: Map<I, EntryModel<R, Tm, C>>,
    fp: spec_fn(FactModel<R, Tm, C>) -> u64,
    done: Seq<I>,
    rest: Seq<I>,
    index: Map<u64, Seq<I>>,
)
    requires
        (done + rest).no_duplicates(),
        grouped_exactly(facts, fp, done, index),
    ensures
        grouped_exactly(facts, fp, done + rest, group_loop(facts, fp, rest, index)),
    decreases rest.len(),
{
    if rest.len() == 0 {
        assert(done + rest =~= done);
    } else {
        let id = rest[0];
        let f = fp(facts[id].fact);
        let old_group = if index.contains_key(f) {
            index[f]
        } else {
            Seq::<I>::empty()
        };
        let index2 = index.insert(f, old_group.push(id));
        let done2 = done.push(id);
        assert(done2 + rest.drop_first() =~= done + rest);
        assert(!done.contains(id)) by {
            if done.contains(id) {
                let i = choose|i: int| 0 <= i < done.len() && done[i] == id;
                assert((done + rest)[i] == (done + rest)[done.len() as int]);
            }
        }
        assert(!old_group.contains(id)) by {
            if old_group.contains(id) {
                let i = choose|i: int| 0 <= i < old_group.len() && old_group[i] == id;
                assert(done.contains(index[f][i]));
            }
        }
        assert forall|g: u64| #[trigger] index2.contains_key(g) implies index2[g].no_duplicates() by {
            if g == f {
                assert forall|i: int, j: int|
                    0 <= i < index2[g].len() && 0 <= j < index2[g].len() && i != j implies index2[g][i]
                    != index2[g][j] by {
                    if i == old_group.len() && j < old_group.len() {
                        assert(old_group[j] == index2[g][j]);
                    }
                    if j == old_group.len() && i < old_group.len() {
                        assert(old_group[i] == index2[g][i]);
                    }
                }
            }
        }
        assert forall|g: u64, i: int|
            #[trigger] index2.contains_key(g) && 0 <= i < index2[g].len() implies done2.contains(
            #[trigger] index2[g][i],
        ) && fp(facts[index2[g][i]].fact) == g by {
            if g == f && i == old_group.len() {
                assert(done2[done.len() as int] == id);
            } else {
                let x = index[g][i];
                assert(done.contains(x));
                let k = choose|k: int| 0 <= k < done.len() && done[k] == x;
                assert(done2[k] == x);
            }
        }
        assert forall|x: I| #[trigger] done2.contains(x) implies index2.contains_key(
            fp(facts[x].fact),
        ) && index2[fp(facts[x].fact)].contains(x) by {
            if x == id {
                assert(index2[f][old_group.len() as int] == id);
            } else {
                let k = choose|k: int| 0 <= k < done2.len() && done2[k] == x;
                assert(k < done.len());
                assert(done[k] == x);
                assert(done.contains(x));
                let g = fp(facts[x].fact);
                assert(index.contains_key(g) && index[g].contains(x));
                let i = choose|i: int| 0 <= i < index[g].len() && index[g][i] == x;
                if g == f {
                    assert(old_group == index[f]);
                    assert(index2[g] == old_group.push(id));
                }
                assert(index2[g][i] == x);
            }
        }
        lemma_group_loop(facts, fp, done2, rest.drop_first(), index2);
    }
}

/// Two candidate lists of live facts in strictly increasing timestamp order
/// with the same members are equal.
pub proof fn lemma_sorted_live_unique<I, R, Tm, C>(
    facts: Map<I, EntryModel<R, Tm, C>>,
    x: Seq<I>,
    y: Seq<I>,
)
    requires
        sorted_live(facts, x),
        sorted_live(facts, y),
        forall|z: I| x.contains(z) <==> y.contains(z),
    ensures
        x == y,
    decreases x.len(),
{
    if x.len() == 0 {
        if y.len() > 0 {
            assert(y.contains(y[0]));
        }
        assert(x =~= y);
    } else {
        assert(x.contains(x[0]));
        assert(y.len() > 0);
        assert(x[0] == y[0]) by {
            if x[0] != y[0] {
                assert(y.contains(y[0]));
                let i = choose|i: int| 0 <= i < x.len() && x[i] == y[0];
                let j = choose|j: int| 0 <= j < y.len() && y[j] == x[0];
                assert(i != 0 && j != 0);
                assert(facts[x[0]].timestamp < facts[x[i]].timestamp);
                assert(facts[y[0]].timestamp < facts[y[j]].timestamp);
            }
        }
        let xs = x.drop_first();
        let ys = y.drop_first();
        assert forall|z: I| xs.contains(z) <==> ys.contains(z) by {
            if xs.contains(z) {
                let i = choose|i: int| 0 <= i < xs.len() && xs[i] == z;
                assert(x[i + 1] == z);
                assert(z != x[0]) by {
                    assert(facts[x[0]].timestamp < facts[x[i + 1]].timestamp);
                }
                assert(x.contains(z));
                let j = choose|j: int| 0 <= j < y.len() && y[j] == z;
                assert(j != 0);
                assert(ys[j - 1] == z);
            }
            if ys.contains(z) {
                let j = choose|j: int| 0 <= j < ys.len() && ys[j] == z;
                assert(y[j + 1] == z);
                assert(z != y[0]) by {
                    assert(facts[y[0]].timestamp < facts[y[j + 1]].timestamp);
                }
                assert(y.contains(z));
                let i = choose|i: int| 0 <= i < x.len() && x[i] == z;
                assert(i != 0);
                assert(xs[i - 1] == z);
            }
        }
        assert(sorted_live(facts, xs)) by {
            assert forall|i: int, j: int| 0 <= i < j < xs.len() implies facts[#[trigger] xs[i]].timestamp
                < facts[#[trigger] xs[j]].timestamp by {
                assert(xs[i] == x[i + 1] && xs[j] == x[j + 1]);
            }
            assert forall|i: int| 0 <= i < xs.len() implies facts.contains_key(#[trigger] xs[i]) by {
                assert(xs[i] == x[i + 1]);
            }
        }
        assert(sorted_live(facts, ys)) by {
            assert forall|i: int, j: int| 0 <= i < j < ys.len() implies facts[#[trigger] ys[i]].timestamp
                < facts[#[trigger] ys[j]].timestamp by {
                assert(ys[i] == y[i + 1] && ys[j] == y[j + 1]);
            }
            assert forall|i: int| 0 <= i < ys.len() implies facts.contains_key(#[trigger] ys[i]) by {
                assert(ys[i] == y[i + 1]);
            }
        }
        lemma_sorted_live_unique(facts, xs, ys);
        assert(x =~= seq![x[0]] + xs);
        assert(y =~= seq![y[0]] + ys);
    }
}

impl<I, R, Tm, C> FactBaseModel<I, R, Tm, C> {
    /// An exact fingerprint index is unique: any two coincide.
    pub proof fn lemma_index_exact_unique(
        self,
        a: Map<u64, Seq<I>>,
        b: Map<u64, Seq<I>>,
        fp: spec_fn(FactModel<R, Tm, C>) -> u64,
    )
        requires
            self.index_exact(a, fp),
            self.index_exact(b, fp),
        ensures
            a == b,
    {
        assert forall|f: u64| #[trigger] a.contains_key(f) implies b.contains_key(f) by {
            assert(a[f].len() > 0);
            let id = a[f][0];
            assert(self.facts.contains_key(id) && fp(self.facts[id].fact) == f);
        }
        assert forall|f: u64| #[trigger] b.contains_key(f) implies a.contains_key(f) by {
            assert(b[f].len() > 0);
            let id = b[f][0];
            assert(self.facts.contains_key(id) && fp(self.facts[id].fact) == f);
        }
        assert(a.dom() =~= b.dom());
        assert forall|f: u64| #[trigger] a.contains_key(f) implies a[f] == b[f] by {
            assert(sorted_live(self.facts, a[f]));
            assert(sorted_live(self.facts, b[f]));
            assert forall|z: I| a[f].contains(z) <==> b[f].contains(z) by {
                if a[f].contains(z) {
                    let i = choose|i: int| 0 <= i < a[f].len() && a[f][i] == z;
                    assert(self.facts.contains_key(z) && fp(self.facts[z].fact) == f);
                }
                if b[f].contains(z) {
                    let i = choose|i: int| 0 <= i < b[f].len() && b[f][i] == z;
                    assert(self.facts.contains_key(z) && fp(self.facts[z].fact) == f);
                }
            }
            lemma_sorted_live_unique(self.facts, a[f], b[f]);
        }
        assert(a =~= b);
    }

    /// The production rebuild (grouping loop over the slot map in any
    /// iteration order, then `sort_unstable_by_key(timestamp)` on every group
    /// longer than one, per its documented contract) never hits the
    /// `expect` in the sort key, and yields exactly the abstract
    /// `rebuilt_index` used by `ensure_structural_index` above.
    pub proof fn lemma_rebuild_refines_model(
        self,
        fp: spec_fn(FactModel<R, Tm, C>) -> u64,
        slot_order: Seq<I>,
        sorted: Map<u64, Seq<I>>,
    )
        requires
            self.timestamps_ok(),
            slot_order.no_duplicates(),
            forall|id: I| #[trigger] slot_order.contains(id) <==> self.facts.contains_key(id),
            sort_loop_result(
                self.facts,
                group_loop(self.facts, fp, slot_order, Map::empty()),
                sorted,
            ),
        ensures
            // the sort key's `expect("structural index only contains active facts")`
            forall|f: u64, i: int|
                #[trigger] group_loop(self.facts, fp, slot_order, Map::empty()).contains_key(f)
                    && 0 <= i < group_loop(self.facts, fp, slot_order, Map::empty())[f].len()
                    ==> self.facts.contains_key(
                    #[trigger] group_loop(self.facts, fp, slot_order, Map::empty())[f][i],
                ),
            self.index_exact(sorted, fp),
            sorted == self.rebuilt_index(fp),
    {
        broadcast use vstd::seq_lib::group_to_multiset_ensures;

        let grouped = group_loop(self.facts, fp, slot_order, Map::empty());
        let empty = Seq::<I>::empty();
        assert(empty + slot_order =~= slot_order);
        assert(grouped_exactly(self.facts, fp, empty, Map::<u64, Seq<I>>::empty()));
        lemma_group_loop(self.facts, fp, empty, slot_order, Map::empty());
        assert(grouped_exactly(self.facts, fp, slot_order, grouped));
        // Facts about each sorted group.
        assert forall|f: u64| #[trigger] sorted.contains_key(f) implies {
            &&& sorted[f].len() == grouped[f].len()
            &&& forall|z: I| sorted[f].contains(z) <==> grouped[f].contains(z)
            &&& sorted[f].no_duplicates()
            &&& sorted_by_timestamp(self.facts, sorted[f])
        } by {
            assert(grouped.contains_key(f));
            if grouped[f].len() > 1 {
                let s = sorted[f];
                let g = grouped[f];
                assert(s.to_multiset() == g.to_multiset());
                assert(s.len() == s.to_multiset().len());
                assert(g.len() == g.to_multiset().len());
                assert forall|z: I| s.contains(z) <==> g.contains(z) by {
                    assert(s.contains(z) <==> s.to_multiset().count(z) > 0);
                    assert(g.contains(z) <==> g.to_multiset().count(z) > 0);
                }
                g.lemma_multiset_has_no_duplicates();
                assert forall|z: I| s.to_multiset().contains(z) implies s.to_multiset().count(z)
                    == 1 by {
                    assert(g.to_multiset().contains(z));
                }
                s.lemma_multiset_has_no_duplicates_conv();
            } else {
                assert(sorted_by_timestamp(self.facts, sorted[f]));
            }
        }
        assert(self.index_exact(sorted, fp)) by {
            assert forall|f: u64, i: int|
                #[trigger] sorted.contains_key(f) && 0 <= i < sorted[f].len()
                    implies self.facts.contains_key(#[trigger] sorted[f][i]) && fp(
                self.facts[sorted[f][i]].fact,
            ) == f by {
                let z = sorted[f][i];
                assert(sorted[f].contains(z));
                assert(grouped[f].contains(z));
                let k = choose|k: int| 0 <= k < grouped[f].len() && grouped[f][k] == z;
                assert(slot_order.contains(grouped[f][k]));
            }
            assert forall|f: u64, i: int, j: int|
                #[trigger] sorted.contains_key(f) && 0 <= i < j < sorted[f].len()
                    implies self.facts[#[trigger] sorted[f][i]].timestamp
                    < self.facts[#[trigger] sorted[f][j]].timestamp by {
                let a = sorted[f][i];
                let b = sorted[f][j];
                assert(a != b);
                assert(self.facts[a].timestamp <= self.facts[b].timestamp);
                assert(sorted[f].contains(a) && sorted[f].contains(b));
                assert(grouped[f].contains(a) && grouped[f].contains(b));
                let ka = choose|k: int| 0 <= k < grouped[f].len() && grouped[f][k] == a;
                let kb = choose|k: int| 0 <= k < grouped[f].len() && grouped[f][k] == b;
                assert(slot_order.contains(grouped[f][ka]));
                assert(slot_order.contains(grouped[f][kb]));
                assert(self.facts.contains_key(a) && self.facts.contains_key(b));
            }
            assert forall|f: u64| #[trigger] sorted.contains_key(f) implies sorted[f].len() > 0 by {
                assert(grouped.contains_key(f));
            }
            assert forall|id: I| #[trigger] self.facts.contains_key(id) implies sorted.contains_key(
                fp(self.facts[id].fact),
            ) && sorted[fp(self.facts[id].fact)].contains(id) by {
                assert(slot_order.contains(id));
            }
        }
        self.lemma_rebuilt_index(fp);
        self.lemma_index_exact_unique(sorted, self.rebuilt_index(fp), fp);
    }
}

} // verus!
