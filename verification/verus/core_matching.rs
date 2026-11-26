//! Verus models of the ferric-rules-core pattern-matching primitives: alpha
//! memory slot indexes, indexed candidate collection for joins, `BindingSet`
//! and `VarMap`.
//!
//! What is proved (unbounded: any number of facts, slots, keys, tests and
//! variables):
//!
//! * `lemma_join_eq_implies_same_key` / `lemma_same_key_implies_join_eq`:
//!   for a bound value with an `AtomKey`, the join `Equal` test passes iff
//!   the fact value has the same key (index soundness and completeness).
//! * Alpha memory (`AlphaMem` = `facts` + `slot_indices` + `indexed_slots`):
//!   `insert`, `remove`, `request_index` (backfill), `request_index_empty`
//!   and `clear` preserve `inv`, which says every lookup on an indexed slot
//!   equals the naive filter of the memory's facts by key, in memory order,
//!   that only indexed slots have entries, and that empty buckets and empty
//!   slot maps are pruned (`lemma_*_inv`, `theorem_lookup_is_naive_filter`).
//!   `theorem_reachable_memories_are_consistent`: `inv` holds after every
//!   operation sequence that meets the production preconditions.
//! * `theorem_candidates_then_residual_equals_scan`: for any memory satisfying
//!   `inv`, any join tests and any bindings, `collect_candidate_facts`
//!   (index path or scan, threshold 16) followed by `evaluate_join` keeps
//!   exactly the facts, in the same order, that `evaluate_join` keeps from a
//!   full scan.
//! * `AlphaNetwork` (`theorem_assert_fact_keeps_reverse_index_exact`,
//!   `theorem_retract_fact_removes_everywhere`): the `fact_to_memories`
//!   reverse index stays exact — a memory holds a fact iff the index records
//!   that memory for the fact — across `assert_fact` (which inserts the fact
//!   into exactly the memories `propagate` reaches) and `retract_fact` (which
//!   removes it from every memory and leaves other facts untouched), and
//!   every memory keeps its own index invariant. (Kani cannot run
//!   `AlphaNetwork::new`, whose `SparseSecondaryMap` seeds a std
//!   `RandomState` through the `getrandom` foreign function.)
//!   `theorem_propagate_is_reachability` characterizes `propagate`
//!   declaratively: for a network built parent-first, it accepts exactly the
//!   memories reachable from the start node through nodes whose tests all
//!   pass, so `theorem_assert_fact_reaches_exactly_passing_paths` says a
//!   fresh fact lands in memory `m` iff its type has an entry node with a
//!   passing path to `m`. `theorem_network_operations_preserve_inv`: every
//!   sequence of fresh-fact assertions and retractions keeps the network
//!   invariant (constant tests and entry types stay uninterpreted).
//! * Sequence index keys: `theorem_ordered_selector_reads_physical_field` and
//!   `theorem_template_selector_reads_physical_slot` — whenever
//!   `physical_selector` maps a logical field to a physical selector, that
//!   logical field of every split (any capture lengths) is the single value
//!   the physical selector reads (for template plans, under `validate`'s
//!   rule that a scalar slot segment projects one single field).
//!   `theorem_plan_candidates_then_residual_equals_scan`: with a sequence
//!   plan, if every physical selector reads the same value as its logical
//!   field in every split, collecting candidates through the selector-mapped
//!   index and keeping the facts some split of which passes every join test
//!   gives exactly the facts, in memory order, that a full scan gives.
//! * `BindingSet` (executable model with `Vec`): `get` is total over every
//!   `VarId`; `set` refines map insertion and widens exactly to `var + 1`
//!   (`theorem_set_refines_map_insert`); `extend_from` is a left-biased union
//!   whose width grows to the highest variable bound in `other`
//!   (`theorem_extend_width_is_highest_bound`) and its `idx as u16` cast
//!   never truncates (binding vectors never exceed 65536 entries).
//! * `VarMap`: `get_or_create` keeps names and dense ids in bijection,
//!   returns the existing id or the next sequential one (which fits a `u16`),
//!   and returns `TooManyVariables` exactly for a new name when all 65536
//!   ids are used (`theorem_get_or_create`).
//!
//! Trusted base: Verus, Z3 and vstd (including vstd's specifications of
//! `Vec::resize`, `Vec::set`, `Vec::push` and `Vec::new`). No `assume`,
//! `admit`, `external_body` or `assume_specification` is used here.
//!
//! Abstractions (see each item's comment for the production function):
//!
//! * These are models, not the production code: the alpha memory, candidate
//!   collection and `VarMap` are spec functions mirroring the production
//!   control flow; `BindingSet` is executable Verus code over `Vec`.
//! * `OrderedSet<K>` is modelled by its abstract view, an insertion-ordered
//!   sequence without duplicates (`oset_insert` appends, `oset_remove`
//!   filters the key out). Its linked-hash-table implementation is not
//!   modelled. Hash maps are modelled as `Map`s.
//! * Fact contents are an uninterpreted function `field(fact, slot)`: facts
//!   are immutable, and every theorem holds for every interpretation. This
//!   assumes `insert`/`remove` are called with the fact data the id was
//!   asserted with (production passes the fact-base entry).
//! * Values: atoms (integers, float bit patterns, symbols, byte strings),
//!   `Void` and flat multifields of atoms. Instance names, fact addresses and
//!   external addresses behave like symbols for keys and join equality and
//!   are not modelled separately.
//! * Join tests other than `Equal` are an uninterpreted predicate: the
//!   candidate theorem holds whatever they compute. Their value semantics
//!   are checked by Kani in crates/ferric-rules-core/src/rete/kani_proofs.rs.
//! * Candidate collection is modelled for the scalar (non-sequence) index
//!   path; for sequence plans only the selector/placement correspondence is
//!   modelled (`place_segment`/`place_all` describe where each logical field
//!   of a split reads from; which splits the search enumerates and how it
//!   prunes them is not modelled here, and the Kani harnesses comparing
//!   `evaluate_pattern`/`split_matches` with a naive split enumeration ran
//!   out of memory, see crates/ferric-rules-core/src/rete/kani_proofs.rs).
//!   `theorem_plan_candidates_then_residual_equals_scan` abstracts splits to
//!   an uninterpreted `split_value(fact, split, logical field)` and takes
//!   selector soundness (`selector_sound`) as a hypothesis; the selector
//!   theorems establish it for the placement model only, so the link to
//!   the production split search is by the citations, not by proof.
//! * A `ValueRef` in a `BindingSet` is abstracted to a `u64` token (the set
//!   only stores, copies and returns values); the production `extend_from`
//!   iterates with `enumerate`, the model with an index over the same
//!   positions.
//!
//! Known gaps: `request_index` is proved under the precondition that every
//! memory fact is in the fact base (a fact missing from it would be left out
//! of the index; production upholds this because alpha memories only hold
//! live facts). `assert_fact` is proved for a fact not yet in the network
//! (each fact id is asserted once). The network theorems assume `net_wf`
//! (every child id exceeds its parent's), which node creation and
//! `retain_memories` maintain but which is not proved here.
//! `retain_memories`, `backfill_memory`, `clear_all_memories` and
//! beta-memory (token-side) indexes are not modelled; the network model
//! abstracts constant tests and entry types to uninterpreted functions.

use vstd::prelude::*;

verus! {

// ===========================================================================
// Values, atom keys and join equality
// ===========================================================================

/// Models crates/ferric-rules-core/src/value.rs:279 enum `Value`, restricted to the variants whose behaviour the
/// index theorems depend on. Floats are represented by their bit pattern
/// (the only way production compares them for equality), strings by their
/// bytes. Multifields hold atoms only (CLIPS multifields are flat).
pub enum Atom {
    Int(int),
    FloatBits(u64),
    Sym(int),
    Str(Seq<u8>),
}

pub enum Val {
    A(Atom),
    Void,
    Multi(Seq<Atom>),
}

/// Models crates/ferric-rules-core/src/value.rs:473 enum `AtomKey`: atoms keep their identity, the key of a float is
/// its bit pattern.
pub type Key = Atom;

/// Models crates/ferric-rules-core/src/value.rs:505 `<AtomKey as TryFrom<&Value>>::try_from` (via
/// `AtomKey::from_value`, crates/ferric-rules-core/src/value.rs:491): every atom has a key; `Multifield`
/// and `Void` have none.
pub open spec fn atom_key(v: Val) -> Option<Key> {
    match v {
        Val::A(a) => Some(a),
        Val::Void => None,
        Val::Multi(_) => None,
    }
}

/// Models crates/ferric-rules-core/src/rete.rs:2930 `values_join_eq`. Same-type atoms compare by content
/// (floats by bits), multifields element-wise (`Value::structural_eq`),
/// `Void` is incomparable, and every other pair is unequal.
pub open spec fn join_eq(a: Val, b: Val) -> Option<bool> {
    match (a, b) {
        (Val::A(Atom::Sym(x)), Val::A(Atom::Sym(y))) => Some(x == y),
        (Val::A(Atom::Int(x)), Val::A(Atom::Int(y))) => Some(x == y),
        (Val::A(Atom::FloatBits(x)), Val::A(Atom::FloatBits(y))) => Some(x == y),
        (Val::A(Atom::Str(x)), Val::A(Atom::Str(y))) => Some(x == y),
        (Val::Multi(x), Val::Multi(y)) => Some(x == y),
        (Val::Void, _) => None,
        (_, Val::Void) => None,
        _ => Some(false),
    }
}

/// Index soundness lemma: when the bound value has a key, a passing `Equal`
/// join test forces the fact value to have the very same key.
pub proof fn lemma_join_eq_implies_same_key(a: Val, b: Val)
    requires
        join_eq(a, b) == Some(true),
        atom_key(b) is Some,
    ensures
        atom_key(a) == atom_key(b),
{
}

/// And the converse (index completeness): equal keys make `Equal` pass.
pub proof fn lemma_same_key_implies_join_eq(a: Val, b: Val)
    requires
        atom_key(a) is Some,
        atom_key(a) == atom_key(b),
    ensures
        join_eq(a, b) == Some(true),
{
}

// ===========================================================================
// Fact contents
// ===========================================================================

/// The value a fact holds at a slot, as `get_slot_value` (crates/ferric-rules-core/src/alpha.rs:331) reads
/// it from the fact base. Facts are immutable once asserted, so this is a
/// fixed (uninterpreted) function: every theorem below holds for every
/// possible content of every fact.
pub uninterp spec fn field(f: int, slot: int) -> Option<Val>;

/// The index key of fact `f` at `slot`: `get_slot_value` then
/// `AtomKey::from_value` (crates/ferric-rules-core/src/alpha.rs:203-204).
pub open spec fn key_at(f: int, slot: int) -> Option<Key> {
    match field(f, slot) {
        Some(v) => atom_key(v),
        None => None,
    }
}

// ===========================================================================
// OrderedSet (abstract sequence view)
// ===========================================================================

/// Models crates/ferric-rules-core/src/ordered_set.rs:32 fn `OrderedSet::insert` through its abstract view, an
/// insertion-ordered sequence without duplicates: a new key is appended.
pub open spec fn oset_insert(s: Seq<int>, x: int) -> (Seq<int>, bool) {
    if s.contains(x) {
        (s, false)
    } else {
        (s.push(x), true)
    }
}

/// Every element except `x`.
pub open spec fn other_than(x: int) -> spec_fn(int) -> bool {
    |y: int| y != x
}

/// Models crates/ferric-rules-core/src/ordered_set.rs:92 fn `OrderedSet::remove` through the same view: the key's single
/// occurrence is unlinked and the order of the rest is kept.
pub open spec fn oset_remove(s: Seq<int>, x: int) -> (Seq<int>, bool) {
    if s.contains(x) {
        (s.filter(other_than(x)), true)
    } else {
        (s, false)
    }
}

// ===========================================================================
// Alpha memory
// ===========================================================================

pub type Index = Map<int, Map<Key, Seq<int>>>;

/// Models crates/ferric-rules-core/src/alpha.rs:167 `AlphaMemory`: `facts` (an `OrderedSet<FactId>`),
/// `slot_indices` and `indexed_slots`.
pub struct AlphaMem {
    pub facts: Seq<int>,
    pub index: Index,
    pub indexed: Seq<int>,
}

/// The facts an index bucket must hold: the memory's facts with that key at
/// that slot, in memory order.
pub open spec fn bucket(facts: Seq<int>, slot: int, key: Key) -> Seq<int> {
    facts.filter(|f: int| key_at(f, slot) == Some(key))
}

/// Models crates/ferric-rules-core/src/alpha.rs:288 `lookup_by_slot`: `None` when the slot or key has no
/// entry.
pub open spec fn lookup_by_slot(m: AlphaMem, slot: int, key: Key) -> Option<Seq<int>> {
    if m.index.contains_key(slot) && m.index[slot].contains_key(key) {
        Some(m.index[slot][key])
    } else {
        None
    }
}

/// The bucket an index holds for (slot, key), empty when absent.
pub open spec fn ilookup(index: Index, slot: int, key: Key) -> Seq<int> {
    if index.contains_key(slot) && index[slot].contains_key(key) {
        index[slot][key]
    } else {
        Seq::empty()
    }
}

pub open spec fn lookup(m: AlphaMem, slot: int, key: Key) -> Seq<int> {
    ilookup(m.index, slot, key)
}

/// Empty buckets and empty per-slot maps are pruned eagerly.
pub open spec fn pruned(index: Index) -> bool {
    &&& forall|slot: int, key: Key|
        index.contains_key(slot) && #[trigger] index[slot].contains_key(key)
            ==> index[slot][key].len() > 0
    &&& forall|slot: int| #[trigger] index.contains_key(slot) ==> index[slot].dom().len() > 0
}

/// Representation invariant: no duplicate facts or indexed slots; every
/// index entry belongs to an indexed slot; for every indexed slot and key the
/// lookup is exactly the bucket; empty buckets and empty slot maps are pruned.
pub open spec fn inv(m: AlphaMem) -> bool {
    &&& m.facts.no_duplicates()
    &&& m.indexed.no_duplicates()
    &&& forall|slot: int| #[trigger] m.index.contains_key(slot) ==> m.indexed.contains(slot)
    &&& forall|slot: int, key: Key|
        m.indexed.contains(slot) ==> #[trigger] lookup(m, slot, key) == bucket(
            m.facts,
            slot,
            key,
        )
    &&& pruned(m.index)
}

/// Models `slot_indices.entry(slot).or_default().entry(key).or_default()
/// .insert(fact_id)` (crates/ferric-rules-core/src/alpha.rs:205-210 and 252-257).
pub open spec fn index_add(index: Index, slot: int, key: Key, f: int) -> Index {
    let keys = if index.contains_key(slot) {
        index[slot]
    } else {
        Map::empty()
    };
    let set = if keys.contains_key(key) {
        keys[key]
    } else {
        Seq::empty()
    };
    index.insert(slot, keys.insert(key, oset_insert(set, f).0))
}

/// Models crates/ferric-rules-core/src/alpha.rs:339 `remove_from_slot_index`.
pub open spec fn remove_from_slot_index(index: Index, slot: int, key: Key, f: int) -> Index {
    if index.contains_key(slot) {
        let key_map = index[slot];
        let key_map = if key_map.contains_key(key) {
            let set = oset_remove(key_map[key], f).0;
            if set.len() == 0 {
                key_map.remove(key)
            } else {
                key_map.insert(key, set)
            }
        } else {
            key_map
        };
        if key_map.dom().len() == 0 {
            index.remove(slot)
        } else {
            index.insert(slot, key_map)
        }
    } else {
        index
    }
}

/// The `for &slot in &self.indexed_slots` loop of crates/ferric-rules-core/src/alpha.rs:196 `insert`, after
/// its first `n` iterations.
pub open spec fn insert_loop(index: Index, slots: Seq<int>, f: int, n: nat) -> Index
    decreases n,
{
    if n == 0 {
        index
    } else {
        let index = insert_loop(index, slots, f, (n - 1) as nat);
        let slot = slots[n - 1];
        match key_at(f, slot) {
            Some(key) => index_add(index, slot, key, f),
            None => index,
        }
    }
}

/// Models crates/ferric-rules-core/src/alpha.rs:196 `AlphaMemory::insert`.
pub open spec fn insert(m: AlphaMem, f: int) -> AlphaMem {
    let (facts, added) = oset_insert(m.facts, f);
    if !added {
        m
    } else {
        AlphaMem { facts, index: insert_loop(m.index, m.indexed, f, m.indexed.len()), ..m }
    }
}

/// The `for &slot in &self.indexed_slots` loop of crates/ferric-rules-core/src/alpha.rs:220 `remove`.
pub open spec fn remove_loop(index: Index, slots: Seq<int>, f: int, n: nat) -> Index
    decreases n,
{
    if n == 0 {
        index
    } else {
        let index = remove_loop(index, slots, f, (n - 1) as nat);
        let slot = slots[n - 1];
        match key_at(f, slot) {
            Some(key) => remove_from_slot_index(index, slot, key, f),
            None => index,
        }
    }
}

/// Models crates/ferric-rules-core/src/alpha.rs:220 `AlphaMemory::remove`.
pub open spec fn remove(m: AlphaMem, f: int) -> AlphaMem {
    let (facts, removed) = oset_remove(m.facts, f);
    if !removed {
        m
    } else {
        AlphaMem { facts, index: remove_loop(m.index, m.indexed, f, m.indexed.len()), ..m }
    }
}

/// The backfill loop of crates/ferric-rules-core/src/alpha.rs:239 `request_index` over the first `n`
/// facts; `in_base` models `fact_base.get(fact_id).is_some()`.
pub open spec fn backfill_loop(
    index: Index,
    facts: Seq<int>,
    slot: int,
    in_base: Set<int>,
    n: nat,
) -> Index
    decreases n,
{
    if n == 0 {
        index
    } else {
        let index = backfill_loop(index, facts, slot, in_base, (n - 1) as nat);
        let f = facts[n - 1];
        if in_base.contains(f) {
            match key_at(f, slot) {
                Some(key) => index_add(index, slot, key, f),
                None => index,
            }
        } else {
            index
        }
    }
}

/// Models crates/ferric-rules-core/src/alpha.rs:239 `AlphaMemory::request_index`.
pub open spec fn request_index(m: AlphaMem, slot: int, in_base: Set<int>) -> AlphaMem {
    if m.indexed.contains(slot) {
        m
    } else {
        AlphaMem {
            indexed: m.indexed.push(slot),
            index: backfill_loop(m.index, m.facts, slot, in_base, m.facts.len()),
            ..m
        }
    }
}

/// Models crates/ferric-rules-core/src/alpha.rs:297 `AlphaMemory::clear`: facts and indices are dropped,
/// the indexed slots are kept.
pub open spec fn clear(m: AlphaMem) -> AlphaMem {
    AlphaMem { facts: Seq::empty(), index: Map::empty(), ..m }
}


// ===========================================================================
// Sequence lemmas
// ===========================================================================

pub proof fn lemma_filter_ext(s: Seq<int>, p: spec_fn(int) -> bool, q: spec_fn(int) -> bool)
    requires
        forall|i: int| 0 <= i < s.len() ==> p(s[i]) == q(s[i]),
    ensures
        s.filter(p) == s.filter(q),
    decreases s.len(),
{
    reveal(Seq::filter);
    if s.len() > 0 {
        lemma_filter_ext(s.drop_last(), p, q);
    }
}

pub proof fn lemma_filter_all(s: Seq<int>, p: spec_fn(int) -> bool)
    requires
        forall|i: int| 0 <= i < s.len() ==> p(s[i]),
    ensures
        s.filter(p) == s,
    decreases s.len(),
{
    reveal(Seq::filter);
    if s.len() > 0 {
        lemma_filter_all(s.drop_last(), p);
        assert(s.drop_last().push(s.last()) =~= s);
    }
}

pub proof fn lemma_filter_none(s: Seq<int>, p: spec_fn(int) -> bool)
    requires
        forall|i: int| 0 <= i < s.len() ==> !p(s[i]),
    ensures
        s.filter(p) == Seq::<int>::empty(),
    decreases s.len(),
{
    reveal(Seq::filter);
    if s.len() > 0 {
        lemma_filter_none(s.drop_last(), p);
    } else {
        assert(s =~= Seq::<int>::empty());
    }
}

pub proof fn lemma_filter_filter(s: Seq<int>, p: spec_fn(int) -> bool, q: spec_fn(int) -> bool)
    ensures
        s.filter(p).filter(q) == s.filter(|x: int| p(x) && q(x)),
    decreases s.len(),
{
    reveal(Seq::filter);
    if s.len() > 0 {
        lemma_filter_filter(s.drop_last(), p, q);
        let rest = s.drop_last().filter(p);
        if p(s.last()) {
            rest.lemma_filter_push(s.last(), q);
        }
    }
}

/// Filtering commutes.
pub proof fn lemma_filter_commute(s: Seq<int>, p: spec_fn(int) -> bool, q: spec_fn(int) -> bool)
    ensures
        s.filter(p).filter(q) == s.filter(q).filter(p),
{
    lemma_filter_filter(s, p, q);
    lemma_filter_filter(s, q, p);
    lemma_filter_ext(s, |x: int| p(x) && q(x), |x: int| q(x) && p(x));
}

/// Membership in a filter: an element of the source satisfying the predicate.
pub proof fn lemma_filter_contains_iff(s: Seq<int>, p: spec_fn(int) -> bool, x: int)
    ensures
        s.filter(p).contains(x) <==> (s.contains(x) && p(x)),
{
    broadcast use Seq::lemma_filter_contains_rev, Seq::lemma_filter_pred, Seq::lemma_filter_contains;

    if s.filter(p).contains(x) {
        let i = choose|i: int| 0 <= i < s.filter(p).len() && s.filter(p)[i] == x;
        assert(p(s.filter(p)[i]));
    }
    if s.contains(x) && p(x) {
        let i = choose|i: int| 0 <= i < s.len() && s[i] == x;
        assert(s.filter(p).contains(s[i]));
    }
}

pub proof fn lemma_filter_no_duplicates(s: Seq<int>, p: spec_fn(int) -> bool)
    requires
        s.no_duplicates(),
    ensures
        s.filter(p).no_duplicates(),
    decreases s.len(),
{
    reveal(Seq::filter);
    if s.len() > 0 {
        let rest = s.drop_last();
        assert(rest.no_duplicates());
        lemma_filter_no_duplicates(rest, p);
        if p(s.last()) {
            lemma_filter_contains_iff(rest, p, s.last());
            assert(!rest.contains(s.last()));
            let out = rest.filter(p).push(s.last());
            assert forall|i: int, j: int| 0 <= i < out.len() && 0 <= j < out.len() && i != j
                implies out[i] != out[j] by {
                if i == out.len() - 1 && j < out.len() - 1 {
                    assert(rest.filter(p).contains(out[j]));
                } else if j == out.len() - 1 && i < out.len() - 1 {
                    assert(rest.filter(p).contains(out[i]));
                }
            }
        }
    }
}

/// Removing every occurrence of an absent element changes nothing.
pub proof fn lemma_filter_out_absent(s: Seq<int>, x: int)
    requires
        !s.contains(x),
    ensures
        s.filter(other_than(x)) == s,
{
    assert forall|i: int| 0 <= i < s.len() implies other_than(x)(s[i]) by {
        if s[i] == x {
            assert(s.contains(x));
        }
    }
    lemma_filter_all(s, other_than(x));
}

pub proof fn lemma_take_push(s: Seq<int>, n: int)
    requires
        0 < n <= s.len(),
    ensures
        s.take(n) == s.take(n - 1).push(s[n - 1]),
{
    assert(s.take(n) =~= s.take(n - 1).push(s[n - 1]));
}

// ===========================================================================
// Index update lemmas
// ===========================================================================

pub proof fn lemma_index_add(index: Index, slot: int, key: Key, f: int)
    requires
        pruned(index),
        !ilookup(index, slot, key).contains(f),
    ensures
        pruned(index_add(index, slot, key, f)),
        forall|s2: int, k2: Key|
            #[trigger] ilookup(index_add(index, slot, key, f), s2, k2) == if s2 == slot && k2
                == key {
                ilookup(index, s2, k2).push(f)
            } else {
                ilookup(index, s2, k2)
            },
        forall|s2: int|
            #[trigger] index_add(index, slot, key, f).contains_key(s2) <==> (index.contains_key(
                s2,
            ) || s2 == slot),
{
    let out = index_add(index, slot, key, f);
    let keys = if index.contains_key(slot) {
        index[slot]
    } else {
        Map::empty()
    };
    assert(out[slot].dom().contains(key));
    assert(out[slot].dom().len() > 0) by {
        vstd::set_lib::lemma_set_empty_equivalency_len(out[slot].dom());
    }
    assert forall|s2: int, k2: Key|
        out.contains_key(s2) && #[trigger] out[s2].contains_key(k2) implies out[s2][k2].len()
        > 0 by {
        if s2 != slot {
            assert(index.contains_key(s2) && index[s2].contains_key(k2));
        } else if k2 != key {
            assert(index.contains_key(s2) && index[s2].contains_key(k2));
        }
    }
}

pub proof fn lemma_remove_from_slot_index(index: Index, slot: int, key: Key, f: int)
    requires
        pruned(index),
    ensures
        pruned(remove_from_slot_index(index, slot, key, f)),
        forall|s2: int, k2: Key|
            #[trigger] ilookup(remove_from_slot_index(index, slot, key, f), s2, k2) == if s2
                == slot && k2 == key {
                ilookup(index, s2, k2).filter(other_than(f))
            } else {
                ilookup(index, s2, k2)
            },
        forall|s2: int|
            #[trigger] remove_from_slot_index(index, slot, key, f).contains_key(s2)
                ==> index.contains_key(s2),
{
    let out = remove_from_slot_index(index, slot, key, f);
    reveal(Seq::filter);
    if index.contains_key(slot) {
        let key_map = index[slot];
        if key_map.contains_key(key) {
            if !key_map[key].contains(f) {
                lemma_filter_out_absent(key_map[key], f);
            }
            let set = oset_remove(key_map[key], f).0;
            assert(set == key_map[key].filter(other_than(f)));
        } else {
            assert(ilookup(index, slot, key) == Seq::<int>::empty());
            assert(Seq::<int>::empty().filter(other_than(f)) =~= Seq::<int>::empty());
        }
        let key_map2 = if key_map.contains_key(key) {
            let set = oset_remove(key_map[key], f).0;
            if set.len() == 0 {
                key_map.remove(key)
            } else {
                key_map.insert(key, set)
            }
        } else {
            key_map
        };
        if key_map2.dom().len() == 0 {
            vstd::set_lib::lemma_set_empty_equivalency_len(key_map2.dom());
            assert forall|k2: Key| !key_map2.contains_key(k2) by {
                assert(!key_map2.dom().contains(k2));
            }
            if key_map.contains_key(key) {
                let set = oset_remove(key_map[key], f).0;
                if set.len() > 0 {
                    assert(key_map2.contains_key(key));
                }
            }
        } else {
            assert forall|s2: int, k2: Key|
                out.contains_key(s2) && #[trigger] out[s2].contains_key(k2) implies out[s2][k2].len()
                > 0 by {
                if s2 == slot && k2 != key {
                    assert(index[s2].contains_key(k2));
                }
            }
        }
    } else {
        assert(Seq::<int>::empty().filter(other_than(f)) =~= Seq::<int>::empty());
    }
}

pub proof fn lemma_take_contains_step(s: Seq<int>, n: int, x: int)
    requires
        0 < n <= s.len(),
    ensures
        s.take(n).contains(x) <==> (s.take(n - 1).contains(x) || x == s[n - 1]),
{
    lemma_take_push(s, n);
    vstd::seq_lib::lemma_seq_contains_after_push(s.take(n - 1), s[n - 1], x);
}

pub proof fn lemma_no_dup_last_not_in_prefix(s: Seq<int>, n: int)
    requires
        0 < n <= s.len(),
        s.no_duplicates(),
    ensures
        !s.take(n - 1).contains(s[n - 1]),
{
    if s.take(n - 1).contains(s[n - 1]) {
        let j = choose|j: int| 0 <= j < n - 1 && #[trigger] s.take(n - 1)[j] == s[n - 1];
        assert(s[j] == s[n - 1]);
    }
}

pub proof fn lemma_push_no_duplicates(s: Seq<int>, x: int)
    requires
        s.no_duplicates(),
        !s.contains(x),
    ensures
        s.push(x).no_duplicates(),
{
    let out = s.push(x);
    assert forall|i: int, j: int| 0 <= i < out.len() && 0 <= j < out.len() && i != j implies out[i]
        != out[j] by {
        if i == out.len() - 1 && j < out.len() - 1 {
            assert(s.contains(out[j]));
        } else if j == out.len() - 1 && i < out.len() - 1 {
            assert(s.contains(out[i]));
        }
    }
}

// ===========================================================================
// Loop lemmas
// ===========================================================================

pub proof fn lemma_insert_loop(index: Index, slots: Seq<int>, f: int, n: nat)
    requires
        n <= slots.len(),
        slots.no_duplicates(),
        pruned(index),
        forall|s2: int, k2: Key| !(#[trigger] ilookup(index, s2, k2)).contains(f),
    ensures
        pruned(insert_loop(index, slots, f, n)),
        forall|s2: int, k2: Key|
            #[trigger] ilookup(insert_loop(index, slots, f, n), s2, k2) == if slots.take(
                n as int,
            ).contains(s2) && key_at(f, s2) == Some(k2) {
                ilookup(index, s2, k2).push(f)
            } else {
                ilookup(index, s2, k2)
            },
        forall|s2: int|
            #[trigger] insert_loop(index, slots, f, n).contains_key(s2) ==> index.contains_key(s2)
                || slots.take(n as int).contains(s2),
    decreases n,
{
    if n == 0 {
        assert forall|s2: int| !slots.take(0).contains(s2) by {}
    } else {
        let prev = insert_loop(index, slots, f, (n - 1) as nat);
        lemma_insert_loop(index, slots, f, (n - 1) as nat);
        let slot = slots[n - 1];
        lemma_no_dup_last_not_in_prefix(slots, n as int);
        assert forall|x: int|
            #[trigger] slots.take(n as int).contains(x) <==> (slots.take(n - 1).contains(x) || x
                == slot) by {
            lemma_take_contains_step(slots, n as int, x);
        }
        match key_at(f, slot) {
            Some(key) => {
                assert(ilookup(prev, slot, key) == ilookup(index, slot, key));
                lemma_index_add(prev, slot, key, f);
            },
            None => {},
        }
    }
}

pub proof fn lemma_remove_loop(index: Index, slots: Seq<int>, f: int, n: nat)
    requires
        n <= slots.len(),
        slots.no_duplicates(),
        pruned(index),
    ensures
        pruned(remove_loop(index, slots, f, n)),
        forall|s2: int, k2: Key|
            #[trigger] ilookup(remove_loop(index, slots, f, n), s2, k2) == if slots.take(
                n as int,
            ).contains(s2) && key_at(f, s2) == Some(k2) {
                ilookup(index, s2, k2).filter(other_than(f))
            } else {
                ilookup(index, s2, k2)
            },
        forall|s2: int|
            #[trigger] remove_loop(index, slots, f, n).contains_key(s2) ==> index.contains_key(s2),
    decreases n,
{
    if n == 0 {
        assert forall|s2: int| !slots.take(0).contains(s2) by {}
    } else {
        let prev = remove_loop(index, slots, f, (n - 1) as nat);
        lemma_remove_loop(index, slots, f, (n - 1) as nat);
        let slot = slots[n - 1];
        lemma_no_dup_last_not_in_prefix(slots, n as int);
        assert forall|x: int|
            #[trigger] slots.take(n as int).contains(x) <==> (slots.take(n - 1).contains(x) || x
                == slot) by {
            lemma_take_contains_step(slots, n as int, x);
        }
        match key_at(f, slot) {
            Some(key) => {
                assert(ilookup(prev, slot, key) == ilookup(index, slot, key));
                lemma_remove_from_slot_index(prev, slot, key, f);
            },
            None => {},
        }
    }
}

pub proof fn lemma_backfill_loop(
    index: Index,
    facts: Seq<int>,
    slot: int,
    in_base: Set<int>,
    n: nat,
)
    requires
        n <= facts.len(),
        facts.no_duplicates(),
        pruned(index),
        !index.contains_key(slot),
        forall|i: int| 0 <= i < facts.len() ==> in_base.contains(#[trigger] facts[i]),
    ensures
        pruned(backfill_loop(index, facts, slot, in_base, n)),
        forall|k2: Key|
            #[trigger] ilookup(backfill_loop(index, facts, slot, in_base, n), slot, k2) == bucket(
                facts.take(n as int),
                slot,
                k2,
            ),
        forall|s2: int, k2: Key|
            s2 != slot ==> #[trigger] ilookup(backfill_loop(index, facts, slot, in_base, n), s2, k2)
                == ilookup(index, s2, k2),
        forall|s2: int|
            #[trigger] backfill_loop(index, facts, slot, in_base, n).contains_key(s2)
                ==> index.contains_key(s2) || s2 == slot,
    decreases n,
{
    reveal(Seq::filter);
    if n == 0 {
        assert forall|k2: Key| #[trigger]
            bucket(facts.take(0), slot, k2) == Seq::<int>::empty() by {
            assert(facts.take(0) =~= Seq::<int>::empty());
        }
    } else {
        let prev = backfill_loop(index, facts, slot, in_base, (n - 1) as nat);
        lemma_backfill_loop(index, facts, slot, in_base, (n - 1) as nat);
        let f = facts[n - 1];
        lemma_take_push(facts, n as int);
        lemma_no_dup_last_not_in_prefix(facts, n as int);
        assert(in_base.contains(f));
        assert forall|k2: Key|
            #[trigger] bucket(facts.take(n as int), slot, k2) == if key_at(f, slot) == Some(k2) {
                bucket(facts.take(n - 1), slot, k2).push(f)
            } else {
                bucket(facts.take(n - 1), slot, k2)
            } by {
            facts.take(n - 1).lemma_filter_push(f, |g: int| key_at(g, slot) == Some(k2));
        }
        match key_at(f, slot) {
            Some(key) => {
                lemma_filter_contains_iff(
                    facts.take(n - 1),
                    |g: int| key_at(g, slot) == Some(key),
                    f,
                );
                assert(!ilookup(prev, slot, key).contains(f));
                lemma_index_add(prev, slot, key, f);
            },
            None => {},
        }
    }
}

// ===========================================================================
// Invariant preservation
// ===========================================================================

/// Models crates/ferric-rules-core/src/alpha.rs:183 `AlphaMemory::new`.
pub open spec fn new_memory() -> AlphaMem {
    AlphaMem { facts: Seq::empty(), index: Map::empty(), indexed: Seq::empty() }
}

pub proof fn lemma_new_inv()
    ensures
        inv(new_memory()),
{
    reveal(Seq::filter);
}

pub proof fn lemma_insert_inv(m: AlphaMem, f: int)
    requires
        inv(m),
    ensures
        inv(insert(m, f)),
        insert(m, f).facts == if m.facts.contains(f) {
            m.facts
        } else {
            m.facts.push(f)
        },
        insert(m, f).indexed == m.indexed,
{
    if !m.facts.contains(f) {
        let out = insert(m, f);
        assert forall|s2: int, k2: Key| !(#[trigger] ilookup(m.index, s2, k2)).contains(f) by {
            if m.index.contains_key(s2) {
                assert(m.indexed.contains(s2));
                assert(lookup(m, s2, k2) == bucket(m.facts, s2, k2));
                lemma_filter_contains_iff(m.facts, |g: int| key_at(g, s2) == Some(k2), f);
            }
        }
        lemma_insert_loop(m.index, m.indexed, f, m.indexed.len());
        assert(m.indexed.take(m.indexed.len() as int) =~= m.indexed);
        lemma_push_no_duplicates(m.facts, f);
        assert forall|slot: int, key: Key| out.indexed.contains(slot) implies #[trigger] lookup(
            out,
            slot,
            key,
        ) == bucket(out.facts, slot, key) by {
            m.facts.lemma_filter_push(f, |g: int| key_at(g, slot) == Some(key));
            assert(lookup(m, slot, key) == bucket(m.facts, slot, key));
        }
    }
}

pub proof fn lemma_remove_inv(m: AlphaMem, f: int)
    requires
        inv(m),
    ensures
        inv(remove(m, f)),
        remove(m, f).facts == m.facts.filter(other_than(f)),
        remove(m, f).indexed == m.indexed,
{
    if !m.facts.contains(f) {
        lemma_filter_out_absent(m.facts, f);
    } else {
        let out = remove(m, f);
        lemma_remove_loop(m.index, m.indexed, f, m.indexed.len());
        assert(m.indexed.take(m.indexed.len() as int) =~= m.indexed);
        lemma_filter_no_duplicates(m.facts, other_than(f));
        assert forall|slot: int, key: Key| out.indexed.contains(slot) implies #[trigger] lookup(
            out,
            slot,
            key,
        ) == bucket(out.facts, slot, key) by {
            let p = |g: int| key_at(g, slot) == Some(key);
            assert(lookup(m, slot, key) == bucket(m.facts, slot, key));
            lemma_filter_commute(m.facts, other_than(f), p);
            if key_at(f, slot) != Some(key) {
                lemma_filter_contains_iff(m.facts, p, f);
                lemma_filter_out_absent(m.facts.filter(p), f);
            }
        }
    }
}

pub proof fn lemma_request_index_inv(m: AlphaMem, slot: int, in_base: Set<int>)
    requires
        inv(m),
        forall|i: int| 0 <= i < m.facts.len() ==> in_base.contains(#[trigger] m.facts[i]),
    ensures
        inv(request_index(m, slot, in_base)),
        request_index(m, slot, in_base).facts == m.facts,
        request_index(m, slot, in_base).indexed.contains(slot),
{
    if !m.indexed.contains(slot) {
        let out = request_index(m, slot, in_base);
        assert(!m.index.contains_key(slot));
        lemma_backfill_loop(m.index, m.facts, slot, in_base, m.facts.len());
        assert(m.facts.take(m.facts.len() as int) =~= m.facts);
        assert forall|x: int| #[trigger]
            out.indexed.contains(x) <==> (m.indexed.contains(x) || x == slot) by {
            vstd::seq_lib::lemma_seq_contains_after_push(m.indexed, slot, x);
        }
        lemma_push_no_duplicates(m.indexed, slot);
        assert forall|s2: int, key: Key| out.indexed.contains(s2) implies #[trigger] lookup(
            out,
            s2,
            key,
        ) == bucket(out.facts, s2, key) by {
            if s2 != slot {
                assert(lookup(m, s2, key) == bucket(m.facts, s2, key));
            }
        }
    }
}

/// Models crates/ferric-rules-core/src/alpha.rs:268 `request_index_empty`, whose `assert!` panics on a
/// non-empty memory; the model therefore takes that as a precondition.
pub open spec fn request_index_empty(m: AlphaMem, slot: int) -> AlphaMem
    recommends
        m.facts.len() == 0,
{
    if !m.indexed.contains(slot) {
        AlphaMem { indexed: m.indexed.push(slot), ..m }
    } else {
        m
    }
}

pub proof fn lemma_empty_memory_has_empty_index(m: AlphaMem)
    requires
        inv(m),
        m.facts.len() == 0,
    ensures
        forall|s2: int| !m.index.contains_key(s2),
{
    reveal(Seq::filter);
    assert forall|s2: int| !m.index.contains_key(s2) by {
        if m.index.contains_key(s2) {
            assert(m.index[s2].dom().len() > 0);
            vstd::set_lib::lemma_set_empty_equivalency_len(m.index[s2].dom());
            let k = m.index[s2].dom().choose();
            assert(m.index[s2].contains_key(k));
            assert(m.indexed.contains(s2));
            assert(lookup(m, s2, k) == bucket(m.facts, s2, k));
        }
    }
}

pub proof fn lemma_request_index_empty_inv(m: AlphaMem, slot: int)
    requires
        inv(m),
        m.facts.len() == 0,
    ensures
        inv(request_index_empty(m, slot)),
        request_index_empty(m, slot).indexed.contains(slot),
        request_index_empty(m, slot).facts == m.facts,
{
    reveal(Seq::filter);
    lemma_empty_memory_has_empty_index(m);
    if !m.indexed.contains(slot) {
        let out = request_index_empty(m, slot);
        lemma_push_no_duplicates(m.indexed, slot);
        vstd::seq_lib::lemma_seq_contains_after_push(m.indexed, slot, slot);
        assert forall|s2: int, key: Key| out.indexed.contains(s2) implies #[trigger] lookup(
            out,
            s2,
            key,
        ) == bucket(out.facts, s2, key) by {}
    }
}

pub proof fn lemma_clear_inv(m: AlphaMem)
    requires
        inv(m),
    ensures
        inv(clear(m)),
{
    reveal(Seq::filter);
}

/// Every lookup on an indexed slot returns exactly the facts a naive filter
/// would — the facts whose slot value has that key, in memory (insertion)
/// order — and `None` exactly when there is no such fact.
pub proof fn theorem_lookup_is_naive_filter(m: AlphaMem, slot: int, key: Key)
    requires
        inv(m),
        m.indexed.contains(slot),
    ensures
        lookup_by_slot(m, slot, key) == if bucket(m.facts, slot, key).len() == 0 {
            None::<Seq<int>>
        } else {
            Some(bucket(m.facts, slot, key))
        },
{
    assert(lookup(m, slot, key) == bucket(m.facts, slot, key));
}

// ===========================================================================
// Arbitrary operation sequences
// ===========================================================================

/// The `AlphaMemory` operations that change facts or indices.
pub enum MemOp {
    Insert(int),
    Remove(int),
    RequestIndex(int),
    RequestIndexEmpty(int),
    Clear,
}

/// `base` models the fact base: `request_index` backfills only facts it can
/// find there (crates/ferric-rules-core/src/alpha.rs:249).
pub open spec fn apply(m: AlphaMem, op: MemOp, base: Set<int>) -> AlphaMem {
    match op {
        MemOp::Insert(f) => insert(m, f),
        MemOp::Remove(f) => remove(m, f),
        MemOp::RequestIndex(slot) => request_index(m, slot, base),
        MemOp::RequestIndexEmpty(slot) => request_index_empty(m, slot),
        MemOp::Clear => clear(m),
    }
}

/// Production preconditions: a fact entering an alpha memory is in the fact
/// base (it is asserted there first), and `request_index_empty` is only used
/// on an empty memory (it panics otherwise, crates/ferric-rules-core/src/alpha.rs:269).
pub open spec fn allowed(m: AlphaMem, op: MemOp, base: Set<int>) -> bool {
    match op {
        MemOp::Insert(f) => base.contains(f),
        MemOp::RequestIndexEmpty(_) => m.facts.len() == 0,
        _ => true,
    }
}

pub open spec fn run(m: AlphaMem, ops: Seq<MemOp>, base: Set<int>) -> AlphaMem
    decreases ops.len(),
{
    if ops.len() == 0 {
        m
    } else {
        apply(run(m, ops.drop_last(), base), ops.last(), base)
    }
}

pub open spec fn run_allowed(m: AlphaMem, ops: Seq<MemOp>, base: Set<int>) -> bool
    decreases ops.len(),
{
    if ops.len() == 0 {
        true
    } else {
        run_allowed(m, ops.drop_last(), base) && allowed(
            run(m, ops.drop_last(), base),
            ops.last(),
            base,
        )
    }
}

pub open spec fn facts_in_base(m: AlphaMem, base: Set<int>) -> bool {
    forall|i: int| 0 <= i < m.facts.len() ==> base.contains(#[trigger] m.facts[i])
}

pub proof fn lemma_apply_preserves(m: AlphaMem, op: MemOp, base: Set<int>)
    requires
        inv(m),
        facts_in_base(m, base),
        allowed(m, op, base),
    ensures
        inv(apply(m, op, base)),
        facts_in_base(apply(m, op, base), base),
{
    match op {
        MemOp::Insert(f) => {
            lemma_insert_inv(m, f);
        },
        MemOp::Remove(f) => {
            lemma_remove_inv(m, f);
            let out = remove(m, f);
            assert forall|i: int| 0 <= i < out.facts.len() implies base.contains(
                #[trigger] out.facts[i],
            ) by {
                lemma_filter_contains_iff(m.facts, other_than(f), out.facts[i]);
                assert(out.facts.contains(out.facts[i]));
            }
        },
        MemOp::RequestIndex(slot) => {
            lemma_request_index_inv(m, slot, base);
        },
        MemOp::RequestIndexEmpty(slot) => {
            lemma_request_index_empty_inv(m, slot);
        },
        MemOp::Clear => {
            lemma_clear_inv(m);
        },
    }
}

/// From a new memory, every sequence of operations meeting the production
/// preconditions keeps the representation invariant, so in every reachable
/// state every indexed lookup equals the naive filter
/// (`theorem_lookup_is_naive_filter`) and candidate collection followed by
/// the join tests equals a full scan
/// (`theorem_candidates_then_residual_equals_scan`).
pub proof fn theorem_reachable_memories_are_consistent(ops: Seq<MemOp>, base: Set<int>)
    requires
        run_allowed(new_memory(), ops, base),
    ensures
        inv(run(new_memory(), ops, base)),
        facts_in_base(run(new_memory(), ops, base), base),
    decreases ops.len(),
{
    if ops.len() == 0 {
        lemma_new_inv();
    } else {
        theorem_reachable_memories_are_consistent(ops.drop_last(), base);
        lemma_apply_preserves(run(new_memory(), ops.drop_last(), base), ops.last(), base);
    }
}

// ===========================================================================
// Alpha network: propagation and the fact-to-memories reverse index
// ===========================================================================

/// Models crates/ferric-rules-core/src/alpha.rs:121 enum `AlphaNode`: an
/// entry node (`test` is `None`) or a constant-test node, its children and
/// its optional memory.
pub struct NetNode {
    pub test: Option<int>,
    pub children: Seq<int>,
    pub memory: Option<int>,
}

/// Models crates/ferric-rules-core/src/alpha.rs:366 struct `AlphaNetwork`:
/// nodes, memories (each the `AlphaMem` model above), entry nodes by entry
/// type, and `fact_to_memories`.
pub struct Net {
    pub nodes: Seq<NetNode>,
    pub mems: Seq<AlphaMem>,
    pub entry: Map<int, int>,
    pub f2m: Map<int, Seq<int>>,
}

/// `evaluate_test(fact, test)` (crates/ferric-rules-core/src/alpha.rs:822); its semantics are checked by
/// the Kani harnesses, here it is uninterpreted.
pub uninterp spec fn passes(f: int, test: int) -> bool;

/// The entry type of a fact (crates/ferric-rules-core/src/alpha.rs:611-614).
pub uninterp spec fn entry_of(f: int) -> int;

/// Nodes are created parent-first (crates/ferric-rules-core/src/alpha.rs:508
/// `create_constant_test_node` gives the new child the next id,
/// `nodes.len()`, and only appends that id to the parent's children), and
/// crates/ferric-rules-core/src/alpha.rs:405 `retain_memories` renumbers
/// the kept nodes in their original order, so every child id exceeds its
/// parent's. Assumed here (a precondition of the network theorems), not
/// proved: snapshot restore is not modelled.
pub open spec fn net_wf(net: Net) -> bool {
    forall|n: int, i: int|
        0 <= n < net.nodes.len() && 0 <= i < net.nodes[n].children.len() ==> #[trigger] net.nodes[n].children[i]
            > n
}

/// Models crates/ferric-rules-core/src/alpha.rs:738 fn `propagate`: the
/// memories accepting `f` below `node`, in visiting order. A missing node
/// accepts nothing; a failing test prunes the subtree; the node's memory
/// (if it exists) is visited before the children, in child order.
pub open spec fn propagate(net: Net, node: int, f: int) -> Seq<int>
    decreases net.nodes.len() - node, child_count(net, node) + 1,
{
    if node < 0 || node >= net.nodes.len() {
        Seq::empty()
    } else {
        let n = net.nodes[node];
        if n.test is Some && !passes(f, n.test->Some_0) {
            Seq::empty()
        } else {
            let here = match n.memory {
                Some(m) => if 0 <= m < net.mems.len() {
                    seq![m]
                } else {
                    Seq::empty()
                },
                None => Seq::empty(),
            };
            here + propagate_children(net, node, f, n.children.len() as int)
        }
    }
}

pub open spec fn child_count(net: Net, node: int) -> int {
    if 0 <= node < net.nodes.len() {
        net.nodes[node].children.len() as int
    } else {
        0
    }
}

/// The child loop of `propagate` (crates/ferric-rules-core/src/alpha.rs:762-771) over the first `k`
/// children of `node`.
pub open spec fn propagate_children(net: Net, node: int, f: int, k: int) -> Seq<int>
    decreases net.nodes.len() - node, k,
{
    if k <= 0 || node < 0 || node >= net.nodes.len() || k > net.nodes[node].children.len() {
        Seq::empty()
    } else {
        let child = net.nodes[node].children[k - 1];
        let previous = propagate_children(net, node, f, k - 1);
        // A child id beyond the node list is a missing node, which
        // `propagate` would ignore anyway.
        if node < child < net.nodes.len() {
            previous + propagate(net, child, f)
        } else {
            previous
        }
    }
}

/// Inserting `f` into each accepted memory, in order (crates/ferric-rules-core/src/alpha.rs:754-759).
pub open spec fn insert_all(mems: Seq<AlphaMem>, accepted: Seq<int>, f: int, n: int) -> Seq<AlphaMem>
    decreases n,
{
    if n <= 0 || n > accepted.len() {
        mems
    } else {
        let mems = insert_all(mems, accepted, f, n - 1);
        let m = accepted[n - 1];
        if 0 <= m < mems.len() {
            mems.update(m, insert(mems[m], f))
        } else {
            mems
        }
    }
}

/// Removing `f` from each recorded memory (crates/ferric-rules-core/src/alpha.rs:643-647).
pub open spec fn remove_all(mems: Seq<AlphaMem>, recorded: Seq<int>, f: int, n: int) -> Seq<AlphaMem>
    decreases n,
{
    if n <= 0 || n > recorded.len() {
        mems
    } else {
        let mems = remove_all(mems, recorded, f, n - 1);
        let m = recorded[n - 1];
        if 0 <= m < mems.len() {
            mems.update(m, remove(mems[m], f))
        } else {
            mems
        }
    }
}

/// The memories that accept `f`: none when no entry node exists for its type
/// (crates/ferric-rules-core/src/alpha.rs:616-619).
pub open spec fn accepted(net: Net, f: int) -> Seq<int> {
    if net.entry.contains_key(entry_of(f)) {
        propagate(net, net.entry[entry_of(f)], f)
    } else {
        Seq::empty()
    }
}

/// Models crates/ferric-rules-core/src/alpha.rs:609 fn `assert_fact`.
pub open spec fn assert_fact(net: Net, f: int) -> Net {
    let accepted = accepted(net, f);
    Net {
        mems: insert_all(net.mems, accepted, f, accepted.len() as int),
        f2m: if accepted.len() > 0 {
            net.f2m.insert(f, accepted)
        } else {
            net.f2m
        },
        ..net
    }
}

/// Models crates/ferric-rules-core/src/alpha.rs:634 fn `retract_fact`.
pub open spec fn retract_fact(net: Net, f: int) -> Net {
    if net.f2m.contains_key(f) {
        let recorded = net.f2m[f];
        Net {
            mems: remove_all(net.mems, recorded, f, recorded.len() as int),
            f2m: net.f2m.remove(f),
            ..net
        }
    } else {
        net
    }
}

/// The reverse index is exact: `f` is recorded for memory `m` iff memory `m`
/// holds `f`; recorded ids are valid; every memory keeps its own invariant.
pub open spec fn net_inv(net: Net) -> bool {
    &&& net_wf(net)
    &&& forall|i: int| 0 <= i < net.mems.len() ==> inv(#[trigger] net.mems[i])
    &&& forall|f: int, m: int|
        0 <= m < net.mems.len() ==> (#[trigger] net.mems[m].facts.contains(f) <==> (
        net.f2m.contains_key(f) && net.f2m[f].contains(m)))
    &&& forall|f: int, j: int|
        net.f2m.contains_key(f) && 0 <= j < net.f2m[f].len() ==> 0 <= #[trigger] net.f2m[f][j]
            < net.mems.len()
}

pub proof fn lemma_propagate_valid(net: Net, node: int, f: int)
    ensures
        forall|j: int|
            0 <= j < propagate(net, node, f).len() ==> 0 <= #[trigger] propagate(net, node, f)[j]
                < net.mems.len(),
    decreases net.nodes.len() - node, child_count(net, node) + 1,
{
    if 0 <= node < net.nodes.len() {
        let n = net.nodes[node];
        let k = n.children.len() as int;
        lemma_propagate_children_valid(net, node, f, k);
        if !(n.test is Some && !passes(f, n.test->Some_0)) {
            let here = match n.memory {
                Some(m) => if 0 <= m < net.mems.len() {
                    seq![m]
                } else {
                    Seq::empty()
                },
                None => Seq::empty(),
            };
            let rest = propagate_children(net, node, f, k);
            assert(propagate(net, node, f) == here + rest);
            assert forall|j: int| 0 <= j < (here + rest).len() implies 0 <= #[trigger] (here
                + rest)[j] < net.mems.len() by {
                if j >= here.len() {
                    assert((here + rest)[j] == rest[j - here.len()]);
                }
            }
        }
    }
}

pub proof fn lemma_propagate_children_valid(net: Net, node: int, f: int, k: int)
    ensures
        forall|j: int|
            0 <= j < propagate_children(net, node, f, k).len() ==> 0
                <= #[trigger] propagate_children(net, node, f, k)[j] < net.mems.len(),
    decreases net.nodes.len() - node, k,
{
    if k > 0 && 0 <= node < net.nodes.len() && k <= net.nodes[node].children.len() {
        lemma_propagate_children_valid(net, node, f, k - 1);
        let child = net.nodes[node].children[k - 1];
        let previous = propagate_children(net, node, f, k - 1);
        if node < child < net.nodes.len() {
            lemma_propagate_valid(net, child, f);
            let next = propagate(net, child, f);
            assert forall|j: int| 0 <= j < (previous + next).len() implies 0
                <= #[trigger] (previous + next)[j] < net.mems.len() by {
                if j >= previous.len() {
                    assert((previous + next)[j] == next[j - previous.len()]);
                }
            }
        }
    }
}

pub proof fn lemma_insert_all(mems: Seq<AlphaMem>, accepted: Seq<int>, f: int, n: int)
    requires
        0 <= n <= accepted.len(),
        forall|i: int| 0 <= i < mems.len() ==> inv(#[trigger] mems[i]),
    ensures
        insert_all(mems, accepted, f, n).len() == mems.len(),
        forall|i: int| 0 <= i < mems.len() ==> inv(#[trigger] insert_all(mems, accepted, f, n)[i]),
        forall|i: int, x: int|
            0 <= i < mems.len() ==> (#[trigger] insert_all(mems, accepted, f, n)[i].facts.contains(x)
                <==> (mems[i].facts.contains(x) || (x == f && accepted.take(n).contains(i)))),
    decreases n,
{
    if n == 0 {
        assert forall|i: int| !accepted.take(0).contains(i) by {}
    } else {
        lemma_insert_all(mems, accepted, f, n - 1);
        let previous = insert_all(mems, accepted, f, n - 1);
        let m = accepted[n - 1];
        assert forall|i: int| #[trigger]
            accepted.take(n).contains(i) <==> (accepted.take(n - 1).contains(i) || i == m) by {
            lemma_take_contains_step(accepted, n, i);
        }
        if 0 <= m < previous.len() {
            lemma_insert_inv(previous[m], f);
            assert forall|x: int| #[trigger]
                insert(previous[m], f).facts.contains(x) <==> (previous[m].facts.contains(x) || x
                    == f) by {
                if !previous[m].facts.contains(f) {
                    vstd::seq_lib::lemma_seq_contains_after_push(previous[m].facts, f, x);
                }
            }
        }
    }
}

pub proof fn lemma_remove_all(mems: Seq<AlphaMem>, recorded: Seq<int>, f: int, n: int)
    requires
        0 <= n <= recorded.len(),
        forall|i: int| 0 <= i < mems.len() ==> inv(#[trigger] mems[i]),
    ensures
        remove_all(mems, recorded, f, n).len() == mems.len(),
        forall|i: int| 0 <= i < mems.len() ==> inv(#[trigger] remove_all(mems, recorded, f, n)[i]),
        forall|i: int, x: int|
            0 <= i < mems.len() ==> (#[trigger] remove_all(mems, recorded, f, n)[i].facts.contains(x)
                <==> (mems[i].facts.contains(x) && !(x == f && recorded.take(n).contains(i)))),
    decreases n,
{
    if n == 0 {
        assert forall|i: int| !recorded.take(0).contains(i) by {}
    } else {
        lemma_remove_all(mems, recorded, f, n - 1);
        let previous = remove_all(mems, recorded, f, n - 1);
        let m = recorded[n - 1];
        assert forall|i: int| #[trigger]
            recorded.take(n).contains(i) <==> (recorded.take(n - 1).contains(i) || i == m) by {
            lemma_take_contains_step(recorded, n, i);
        }
        if 0 <= m < previous.len() {
            lemma_remove_inv(previous[m], f);
            assert forall|x: int| #[trigger]
                remove(previous[m], f).facts.contains(x) <==> (previous[m].facts.contains(x) && x
                    != f) by {
                lemma_filter_contains_iff(previous[m].facts, other_than(f), x);
            }
        }
    }
}

/// Asserting a fact that no memory holds yet (each fact is asserted once)
/// keeps the reverse index exact: afterwards `memories_containing_fact`
/// (= `f2m[f]`) lists exactly the memories holding `f`, which are exactly
/// the memories `propagate` reached.
pub proof fn theorem_assert_fact_keeps_reverse_index_exact(net: Net, f: int)
    requires
        net_inv(net),
        !net.f2m.contains_key(f),
    ensures
        net_inv(assert_fact(net, f)),
        forall|m: int|
            0 <= m < net.mems.len() ==> (#[trigger] assert_fact(net, f).mems[m].facts.contains(f)
                <==> accepted(net, f).contains(m)),
{
    let acc = accepted(net, f);
    let out = assert_fact(net, f);
    if net.entry.contains_key(entry_of(f)) {
        lemma_propagate_valid(net, net.entry[entry_of(f)], f);
    }
    lemma_insert_all(net.mems, acc, f, acc.len() as int);
    assert(acc.take(acc.len() as int) =~= acc);
    assert forall|g: int, m: int| 0 <= m < out.mems.len() implies (
    #[trigger] out.mems[m].facts.contains(g) <==> (out.f2m.contains_key(g) && out.f2m[g].contains(
        m,
    ))) by {
        if g == f {
            assert(!net.mems[m].facts.contains(f));
            if acc.len() == 0 {
                assert(!acc.contains(m));
            }
        } else {
            assert(net.mems[m].facts.contains(g) == out.mems[m].facts.contains(g));
        }
    }
    assert forall|g: int, j: int| out.f2m.contains_key(g) && 0 <= j < out.f2m[g].len() implies 0
        <= #[trigger] out.f2m[g][j] < out.mems.len() by {
        if g == f {
            assert(acc[j] == out.f2m[g][j]);
        }
    }
}

/// Retracting removes the fact from exactly the recorded memories, so
/// afterwards no memory holds it, every other membership is unchanged and the
/// reverse index stays exact.
pub proof fn theorem_retract_fact_removes_everywhere(net: Net, f: int)
    requires
        net_inv(net),
    ensures
        net_inv(retract_fact(net, f)),
        forall|m: int|
            0 <= m < net.mems.len() ==> !(#[trigger] retract_fact(net, f).mems[m].facts.contains(f)),
        forall|g: int, m: int|
            g != f && 0 <= m < net.mems.len() ==> (#[trigger] retract_fact(net, f).mems[m].facts.contains(
                g,
            ) == net.mems[m].facts.contains(g)),
{
    if net.f2m.contains_key(f) {
        let recorded = net.f2m[f];
        let out = retract_fact(net, f);
        lemma_remove_all(net.mems, recorded, f, recorded.len() as int);
        assert(recorded.take(recorded.len() as int) =~= recorded);
        assert forall|g: int, m: int| 0 <= m < out.mems.len() implies (
        #[trigger] out.mems[m].facts.contains(g) <==> (out.f2m.contains_key(g)
            && out.f2m[g].contains(m))) by {
            if g != f {
                assert(net.mems[m].facts.contains(g) == out.mems[m].facts.contains(g));
            }
        }
    } else {
        assert forall|m: int| 0 <= m < net.mems.len() implies !(#[trigger] retract_fact(
            net,
            f,
        ).mems[m].facts.contains(f)) by {}
    }
}

// ===========================================================================
// Alpha network: declarative characterization of propagation
// ===========================================================================

/// Node `n` lets `f` through: an entry node always does, a constant-test
/// node when its test passes.
pub open spec fn node_passes(net: Net, n: int, f: int) -> bool {
    !(net.nodes[n].test is Some && !passes(f, net.nodes[n].test->Some_0))
}

/// The memory of node `n`, when it names an existing memory.
pub open spec fn memory_of(net: Net, n: int) -> Option<int> {
    match net.nodes[n].memory {
        Some(m) => if 0 <= m < net.mems.len() {
            Some(m)
        } else {
            None
        },
        None => None,
    }
}

/// A downward path of existing nodes, each a child of the previous one,
/// every node letting `f` through.
pub open spec fn passing_path(net: Net, path: Seq<int>, f: int) -> bool {
    &&& path.len() > 0
    &&& forall|i: int|
        0 <= i < path.len() ==> 0 <= #[trigger] path[i] < net.nodes.len() && node_passes(
            net,
            path[i],
            f,
        )
    &&& forall|i: int|
        0 <= i < path.len() - 1 ==> net.nodes[path[i]].children.contains(#[trigger] path[i + 1])
}

/// Memory `m` is reachable for `f` from `node`: some passing path from
/// `node` ends at a node owning `m`.
pub open spec fn reaches(net: Net, node: int, f: int, m: int) -> bool {
    exists|path: Seq<int>|
        #[trigger] passing_path(net, path, f) && path[0] == node && memory_of(net, path.last())
            == Some(m)
}

/// The child loop accepts `m` only through some child before `k`.
pub proof fn lemma_propagate_children_member(net: Net, node: int, f: int, k: int, m: int)
    requires
        propagate_children(net, node, f, k).contains(m),
    ensures
        exists|j: int|
            0 <= j < k && j < net.nodes[node].children.len() && node
                < #[trigger] net.nodes[node].children[j] < net.nodes.len() && propagate(
                net,
                net.nodes[node].children[j],
                f,
            ).contains(m),
    decreases k,
{
    broadcast use vstd::seq_lib::lemma_seq_concat_contains_all_elements;

    let child = net.nodes[node].children[k - 1];
    let previous = propagate_children(net, node, f, k - 1);
    if previous.contains(m) {
        lemma_propagate_children_member(net, node, f, k - 1, m);
    } else {
        assert(node < child < net.nodes.len());
        assert(propagate(net, child, f).contains(m));
    }
}

/// And every child before `k` contributes what it accepts.
pub proof fn lemma_propagate_children_includes(net: Net, node: int, f: int, k: int, j: int, m: int)
    requires
        0 <= node < net.nodes.len(),
        0 <= j < k <= net.nodes[node].children.len(),
        node < net.nodes[node].children[j] < net.nodes.len(),
        propagate(net, net.nodes[node].children[j], f).contains(m),
    ensures
        propagate_children(net, node, f, k).contains(m),
    decreases k,
{
    broadcast use vstd::seq_lib::lemma_seq_concat_contains_all_elements;

    if j < k - 1 {
        lemma_propagate_children_includes(net, node, f, k - 1, j, m);
    }
}

/// Declarative characterization of `propagate` (crates/ferric-rules-core/src/alpha.rs:738): for a network built
/// parent-first, the memories `propagate` accepts below `node` are exactly
/// the memories reachable from `node` through nodes whose tests all pass.
pub proof fn theorem_propagate_is_reachability(net: Net, node: int, f: int, m: int)
    requires
        net_wf(net),
    ensures
        propagate(net, node, f).contains(m) <==> reaches(net, node, f, m),
    decreases net.nodes.len() - node,
{
    broadcast use vstd::seq_lib::lemma_seq_concat_contains_all_elements;

    if 0 <= node < net.nodes.len() && node_passes(net, node, f) {
        let n = net.nodes[node];
        let k = n.children.len() as int;
        let here = match n.memory {
            Some(mm) => if 0 <= mm < net.mems.len() {
                seq![mm]
            } else {
                Seq::empty()
            },
            None => Seq::empty(),
        };
        assert(propagate(net, node, f) == here + propagate_children(net, node, f, k));
        assert(here.contains(m) <==> memory_of(net, node) == Some(m)) by {
            if memory_of(net, node) == Some(m) {
                assert(here[0] == m);
            }
        }
        if propagate(net, node, f).contains(m) {
            if here.contains(m) {
                let path = seq![node];
                assert(passing_path(net, path, f));
            } else {
                lemma_propagate_children_member(net, node, f, k, m);
                let j = choose|j: int|
                    0 <= j < k && j < net.nodes[node].children.len() && node
                        < #[trigger] net.nodes[node].children[j] < net.nodes.len() && propagate(
                        net,
                        net.nodes[node].children[j],
                        f,
                    ).contains(m);
                let child = n.children[j];
                theorem_propagate_is_reachability(net, child, f, m);
                let tail = choose|path: Seq<int>|
                    #[trigger] passing_path(net, path, f) && path[0] == child && memory_of(
                        net,
                        path.last(),
                    ) == Some(m);
                let path = seq![node] + tail;
                assert(path.last() == tail.last());
                assert forall|i: int| 0 <= i < path.len() - 1 implies net.nodes[path[i]].children.contains(
                    #[trigger] path[i + 1],
                ) by {
                    if i == 0 {
                        assert(path[0] == node && path[1] == tail[0] && tail[0] == child);
                        assert(n.children[j] == child);
                        assert(n.children.contains(child));
                    } else {
                        assert(path[i] == tail[i - 1] && path[i + 1] == tail[i]);
                        assert(net.nodes[tail[i - 1]].children.contains(tail[(i - 1) + 1]));
                    }
                }
                assert forall|i: int| 0 <= i < path.len() implies 0 <= #[trigger] path[i]
                    < net.nodes.len() && node_passes(net, path[i], f) by {
                    if i > 0 {
                        assert(path[i] == tail[i - 1]);
                    }
                }
                assert(passing_path(net, path, f));
            }
        }
        if reaches(net, node, f, m) {
            let path = choose|path: Seq<int>|
                #[trigger] passing_path(net, path, f) && path[0] == node && memory_of(
                    net,
                    path.last(),
                ) == Some(m);
            if path.len() == 1 {
                assert(here.contains(m));
            } else {
                let child = path[1];
                assert(net.nodes[path[0]].children.contains(path[1]));
                let j = choose|j: int| 0 <= j < n.children.len() && n.children[j] == child;
                assert(child > node);
                let tail = path.drop_first();
                assert(passing_path(net, tail, f)) by {
                    assert forall|i: int| 0 <= i < tail.len() - 1 implies net.nodes[tail[i]].children.contains(
                        #[trigger] tail[i + 1],
                    ) by {
                        assert(tail[i] == path[i + 1] && tail[i + 1] == path[i + 2]);
                    }
                    assert forall|i: int| 0 <= i < tail.len() implies 0 <= #[trigger] tail[i]
                        < net.nodes.len() && node_passes(net, tail[i], f) by {
                        assert(tail[i] == path[i + 1]);
                    }
                }
                assert(tail.last() == path.last());
                theorem_propagate_is_reachability(net, child, f, m);
                lemma_propagate_children_includes(net, node, f, k, j, m);
            }
        }
    } else if reaches(net, node, f, m) {
        let path = choose|path: Seq<int>|
            #[trigger] passing_path(net, path, f) && path[0] == node && memory_of(net, path.last())
                == Some(m);
        assert(0 <= path[0] < net.nodes.len() && node_passes(net, path[0], f));
    }
}

/// `assert_fact` in declarative terms: after asserting a fresh fact, memory
/// `m` holds it exactly when the network has an entry node for the fact's
/// type from which `m` is reachable through nodes whose tests all pass.
pub proof fn theorem_assert_fact_reaches_exactly_passing_paths(net: Net, f: int, m: int)
    requires
        net_inv(net),
        !net.f2m.contains_key(f),
        0 <= m < net.mems.len(),
    ensures
        assert_fact(net, f).mems[m].facts.contains(f) <==> (net.entry.contains_key(entry_of(f))
            && reaches(net, net.entry[entry_of(f)], f, m)),
{
    theorem_assert_fact_keeps_reverse_index_exact(net, f);
    if net.entry.contains_key(entry_of(f)) {
        theorem_propagate_is_reachability(net, net.entry[entry_of(f)], f, m);
    }
}

/// Network operations: assert a fact not yet recorded (each fact id is
/// asserted once, crates/ferric-rules-core/src/fact.rs ids are never reused
/// for new data) or retract any fact.
pub enum NetOp {
    Assert(int),
    Retract(int),
}

pub open spec fn apply_net(net: Net, op: NetOp) -> Net {
    match op {
        NetOp::Assert(f) => assert_fact(net, f),
        NetOp::Retract(f) => retract_fact(net, f),
    }
}

pub open spec fn net_allowed(net: Net, op: NetOp) -> bool {
    match op {
        NetOp::Assert(f) => !net.f2m.contains_key(f),
        NetOp::Retract(_) => true,
    }
}

pub open spec fn run_net(net: Net, ops: Seq<NetOp>) -> Net
    decreases ops.len(),
{
    if ops.len() == 0 {
        net
    } else {
        apply_net(run_net(net, ops.drop_last()), ops.last())
    }
}

pub open spec fn run_net_allowed(net: Net, ops: Seq<NetOp>) -> bool
    decreases ops.len(),
{
    if ops.len() == 0 {
        true
    } else {
        run_net_allowed(net, ops.drop_last()) && net_allowed(
            run_net(net, ops.drop_last()),
            ops.last(),
        )
    }
}

/// Every sequence of assertions (of fresh facts) and retractions keeps the
/// network invariant: the reverse index stays exact and every memory keeps
/// its index invariant, so `theorem_lookup_is_naive_filter` and
/// `theorem_candidates_then_residual_equals_scan` apply to every memory in
/// every reachable network state. The node graph and the memory count never
/// change.
pub proof fn theorem_network_operations_preserve_inv(net: Net, ops: Seq<NetOp>)
    requires
        net_inv(net),
        run_net_allowed(net, ops),
    ensures
        net_inv(run_net(net, ops)),
        run_net(net, ops).nodes == net.nodes,
        run_net(net, ops).mems.len() == net.mems.len(),
    decreases ops.len(),
{
    if ops.len() > 0 {
        theorem_network_operations_preserve_inv(net, ops.drop_last());
        let before = run_net(net, ops.drop_last());
        match ops.last() {
            NetOp::Assert(f) => {
                theorem_assert_fact_keeps_reverse_index_exact(before, f);
                let acc = accepted(before, f);
                lemma_insert_all(before.mems, acc, f, acc.len() as int);
            },
            NetOp::Retract(f) => {
                theorem_retract_fact_removes_everywhere(before, f);
                if before.f2m.contains_key(f) {
                    lemma_remove_all(before.mems, before.f2m[f], f, before.f2m[f].len() as int);
                }
            },
        }
    }
}

// ===========================================================================
// Join tests and candidate collection
// ===========================================================================

/// Models crates/ferric-rules-core/src/beta.rs:76 enum `JoinTestType`: `Equal` is modelled exactly; every other
/// test type is abstracted to an uninterpreted predicate (their exact value
/// semantics are checked by the Kani harnesses in rete/kani_proofs.rs).
pub enum TestTy {
    Equal,
    Other(int),
}

/// Models crates/ferric-rules-core/src/beta.rs:67 struct `JoinTest`.
pub struct JoinTest {
    pub slot: int,
    pub var: int,
    pub ty: TestTy,
}

pub uninterp spec fn other_test_holds(code: int, fact_value: Val, bound: Val) -> bool;

/// Models crates/ferric-rules-core/src/binding.rs:176 `BindingSet::get` on the binding vector.
pub open spec fn binding_get(b: Seq<Option<Val>>, var: int) -> Option<Val> {
    if 0 <= var < b.len() {
        b[var]
    } else {
        None
    }
}

/// One iteration of the loop of crates/ferric-rules-core/src/rete.rs:2699 `evaluate_join_fields` with
/// `field = |slot| get_slot_value(fact, slot)` (crates/ferric-rules-core/src/rete.rs:2690
/// `evaluate_join`).
pub open spec fn test_passes(f: int, b: Seq<Option<Val>>, t: JoinTest) -> bool {
    match field(f, t.slot) {
        None => false,
        Some(fact_value) => match binding_get(b, t.var) {
            None => false,
            Some(bound) => match t.ty {
                TestTy::Equal => join_eq(fact_value, bound) == Some(true),
                TestTy::Other(code) => other_test_holds(code, fact_value, bound),
            },
        },
    }
}

/// The loop of crates/ferric-rules-core/src/rete.rs:2699 `evaluate_join_fields` from test `i` on: the
/// first failing test returns `false`.
pub open spec fn evaluate_join_from(f: int, b: Seq<Option<Val>>, tests: Seq<JoinTest>, i: int) -> bool
    decreases tests.len() - i,
{
    if i >= tests.len() || i < 0 {
        true
    } else if !test_passes(f, b, tests[i]) {
        false
    } else {
        evaluate_join_from(f, b, tests, i + 1)
    }
}

pub open spec fn evaluate_join(f: int, b: Seq<Option<Val>>, tests: Seq<JoinTest>) -> bool {
    evaluate_join_from(f, b, tests, 0)
}

/// The first item of crates/ferric-rules-core/src/rete.rs:2797 `indexable_tests(tests, None)`, the only
/// one `collect_candidate_facts` consumes: the first plain `Equal` test.
pub open spec fn first_indexable(tests: Seq<JoinTest>, i: int) -> Option<(int, int)>
    decreases tests.len() - i,
{
    if i >= tests.len() || i < 0 {
        None
    } else if tests[i].ty is Equal {
        Some((tests[i].slot, tests[i].var))
    } else {
        first_indexable(tests, i + 1)
    }
}

/// crates/ferric-rules-core/src/rete.rs:2635 `INDEX_SCAN_THRESHOLD`.
pub open spec fn index_scan_threshold() -> int {
    16
}

/// The indexed branch of crates/ferric-rules-core/src/rete.rs:2637 `collect_candidate_facts`; `None`
/// means control falls through to the full scan.
pub open spec fn indexed_candidates(
    m: AlphaMem,
    first: Option<(int, int)>,
    b: Seq<Option<Val>>,
) -> Option<Seq<int>> {
    if m.facts.len() >= index_scan_threshold() {
        match first {
            Some((slot, var)) => if m.indexed.contains(slot) {
                match binding_get(b, var) {
                    Some(bound) => match atom_key(bound) {
                        Some(key) => Some(
                            match lookup_by_slot(m, slot, key) {
                                Some(facts) => facts,
                                None => Seq::empty(),
                            },
                        ),
                        None => None,
                    },
                    None => None,
                }
            } else {
                None
            },
            None => None,
        }
    } else {
        None
    }
}

/// Models crates/ferric-rules-core/src/rete.rs:2637 `collect_candidate_facts` (with the index tests from
/// `indexable_tests(tests, None)`).
pub open spec fn collect_candidate_facts(
    m: AlphaMem,
    first: Option<(int, int)>,
    b: Seq<Option<Val>>,
) -> Seq<int> {
    match indexed_candidates(m, first, b) {
        Some(candidates) => candidates,
        None => m.facts,
    }
}

pub proof fn lemma_first_indexable_is_an_equal_test(tests: Seq<JoinTest>, i: int)
    requires
        0 <= i,
        first_indexable(tests, i) is Some,
    ensures
        exists|j: int|
            i <= j < tests.len() && (#[trigger] tests[j]).ty is Equal && first_indexable(tests, i)
                == Some((tests[j].slot, tests[j].var)),
    decreases tests.len() - i,
{
    if !(tests[i].ty is Equal) {
        lemma_first_indexable_is_an_equal_test(tests, i + 1);
        let j = choose|j: int|
            i + 1 <= j < tests.len() && (#[trigger] tests[j]).ty is Equal && first_indexable(
                tests,
                i + 1,
            ) == Some((tests[j].slot, tests[j].var));
        assert(i <= j);
    }
}

pub proof fn lemma_evaluate_join_implies_each(
    f: int,
    b: Seq<Option<Val>>,
    tests: Seq<JoinTest>,
    i: int,
    j: int,
)
    requires
        0 <= i <= j < tests.len(),
        evaluate_join_from(f, b, tests, i),
    ensures
        test_passes(f, b, tests[j]),
    decreases j - i,
{
    if i < j {
        lemma_evaluate_join_implies_each(f, b, tests, i + 1, j);
    }
}

/// A fact passing every join test has, at the slot of an `Equal` test whose
/// bound value has a key, exactly that key.
pub proof fn lemma_passing_fact_has_index_key(
    f: int,
    b: Seq<Option<Val>>,
    tests: Seq<JoinTest>,
    j: int,
    key: Key,
)
    requires
        0 <= j < tests.len(),
        tests[j].ty is Equal,
        binding_get(b, tests[j].var) is Some,
        atom_key(binding_get(b, tests[j].var).unwrap()) == Some(key),
        evaluate_join(f, b, tests),
    ensures
        key_at(f, tests[j].slot) == Some(key),
{
    lemma_evaluate_join_implies_each(f, b, tests, 0, j);
    let fact_value = field(f, tests[j].slot).unwrap();
    lemma_join_eq_implies_same_key(fact_value, binding_get(b, tests[j].var).unwrap());
}

/// Main theorem (crates/ferric-rules-core/src/rete.rs:1185 `left_activate`, crates/ferric-rules-core/src/rete.rs:1255
/// `negative_left_activate`, crates/ferric-rules-core/src/rete.rs:2088 `exists_left_activate` and the
/// snapshot validators all collect candidates with `collect_candidate_facts`
/// and then keep the facts passing every join test): filtering the
/// candidates by the join tests yields exactly the facts — in the same
/// order — that filtering the whole memory would, whether or not the index
/// path is taken, for any number of facts.
pub proof fn theorem_candidates_then_residual_equals_scan(
    m: AlphaMem,
    tests: Seq<JoinTest>,
    b: Seq<Option<Val>>,
)
    requires
        inv(m),
    ensures
        collect_candidate_facts(m, first_indexable(tests, 0), b).filter(
            |f: int| evaluate_join(f, b, tests),
        ) == m.facts.filter(|f: int| evaluate_join(f, b, tests)),
{
    let pass = |f: int| evaluate_join(f, b, tests);
    let first = first_indexable(tests, 0);
    if indexed_candidates(m, first, b) is Some {
        lemma_first_indexable_is_an_equal_test(tests, 0);
        let j = choose|j: int|
            0 <= j < tests.len() && (#[trigger] tests[j]).ty is Equal && first_indexable(tests, 0)
                == Some((tests[j].slot, tests[j].var));
        let slot = tests[j].slot;
        let bound = binding_get(b, tests[j].var).unwrap();
        let key = atom_key(bound).unwrap();
        assert(lookup(m, slot, key) == bucket(m.facts, slot, key));
        assert(collect_candidate_facts(m, first, b) == bucket(m.facts, slot, key));
        let has_key = |f: int| key_at(f, slot) == Some(key);
        lemma_filter_filter(m.facts, has_key, pass);
        assert forall|i: int| 0 <= i < m.facts.len() implies (has_key(m.facts[i]) && pass(
            m.facts[i],
        )) == pass(m.facts[i]) by {
            if pass(m.facts[i]) {
                lemma_passing_fact_has_index_key(m.facts[i], b, tests, j, key);
            }
        }
        lemma_filter_ext(m.facts, |f: int| has_key(f) && pass(f), pass);
    }
}

// ===========================================================================
// Sequence plans: physical selectors
// ===========================================================================

/// Models crates/ferric-rules-core/src/sequence.rs:19 enum `SequenceField`.
pub enum SeqField {
    Single,
    Multi,
}

/// Models crates/ferric-rules-core/src/sequence.rs:27 enum `SequenceSource`.
pub enum SeqSource {
    Ordered,
    TemplateSlot(int),
    TemplateScalar(int),
}

/// Models crates/ferric-rules-core/src/sequence.rs:40 struct `SequenceSegment`.
pub struct Segment {
    pub source: SeqSource,
    pub fields: Seq<SeqField>,
}

/// Where a logical field of a split reads from: physical source (`None` for
/// the fields of an ordered fact, `Some(slot)` for a template slot), the
/// first position inside that source, and the number of values (1 for a
/// single field, the capture length for a capture).
pub struct Placed {
    pub source: Option<int>,
    pub start: int,
    pub len: int,
    pub single: bool,
}

pub open spec fn source_slot(source: SeqSource) -> Option<int> {
    match source {
        SeqSource::Ordered => None,
        SeqSource::TemplateSlot(slot) => Some(slot),
        SeqSource::TemplateScalar(slot) => Some(slot),
    }
}

/// The logical fields one segment places, from field `field` on at physical
/// `offset`, given the capture lengths of its remaining captures. Models the
/// per-segment part of crates/ferric-rules-core/src/sequence.rs:450 fn
/// `SplitSearch::place` (and of sequence.rs:302 fn `project`): a single
/// field takes the value at `offset` and advances it by one; a capture takes
/// the next `length` values.
pub open spec fn place_segment(
    source: Option<int>,
    fields: Seq<SeqField>,
    lengths: Seq<int>,
    offset: int,
) -> Seq<Placed>
    decreases fields.len(),
{
    if fields.len() == 0 {
        Seq::empty()
    } else {
        match fields[0] {
            SeqField::Single => seq![Placed { source, start: offset, len: 1, single: true }]
                + place_segment(source, fields.drop_first(), lengths, offset + 1),
            SeqField::Multi => {
                let length = if lengths.len() > 0 {
                    lengths[0]
                } else {
                    0
                };
                seq![Placed { source, start: offset, len: length, single: false }]
                    + place_segment(
                    source,
                    fields.drop_first(),
                    lengths.drop_first(),
                    offset + length,
                )
            },
        }
    }
}

pub open spec fn multis(fields: Seq<SeqField>) -> int
    decreases fields.len(),
{
    if fields.len() == 0 {
        0
    } else {
        (if fields[0] is Multi {
            1int
        } else {
            0int
        }) + multis(fields.drop_first())
    }
}

/// All logical fields of a split: each segment's fields in written order,
/// every segment starting at offset 0 of its own source, consuming its
/// share of the capture lengths (sequence.rs:465-467).
pub open spec fn place_all(segments: Seq<Segment>, lengths: Seq<int>) -> Seq<Placed>
    decreases segments.len(),
{
    if segments.len() == 0 {
        Seq::empty()
    } else {
        let first = segments[0];
        let own = multis(first.fields);
        place_segment(source_slot(first.source), first.fields, lengths, 0) + place_all(
            segments.drop_first(),
            lengths.subrange(if own <= lengths.len() { own } else { lengths.len() as int }, lengths.len() as int),
        )
    }
}

/// Models the ordered arm of crates/ferric-rules-core/src/sequence.rs:104 fn
/// `physical_selector`: an ordered field is a physical selector iff it and
/// every field before it is single.
pub open spec fn ordered_physical_selector(fields: Seq<SeqField>, index: int) -> Option<int> {
    if 0 <= index < fields.len() && (forall|j: int| 0 <= j <= index ==> fields[j] is Single) {
        Some(index)
    } else {
        None
    }
}

/// Models the template arm of crates/ferric-rules-core/src/sequence.rs:104 fn
/// `physical_selector`: walk the segments, subtracting each width, and
/// accept only a scalar slot segment.
pub open spec fn template_physical_selector(segments: Seq<Segment>, index: int) -> Option<int>
    decreases segments.len(),
{
    if segments.len() == 0 || index < 0 {
        None
    } else if index < segments[0].fields.len() {
        match segments[0].source {
            SeqSource::TemplateScalar(physical) => Some(physical),
            _ => None,
        }
    } else {
        template_physical_selector(segments.drop_first(), index - segments[0].fields.len())
    }
}

pub proof fn lemma_place_segment_len(
    source: Option<int>,
    fields: Seq<SeqField>,
    lengths: Seq<int>,
    offset: int,
)
    ensures
        place_segment(source, fields, lengths, offset).len() == fields.len(),
    decreases fields.len(),
{
    if fields.len() > 0 {
        let rest_lengths = if fields[0] is Multi {
            lengths.drop_first()
        } else {
            lengths
        };
        let length = if lengths.len() > 0 {
            lengths[0]
        } else {
            0
        };
        let next = if fields[0] is Multi {
            offset + length
        } else {
            offset + 1
        };
        lemma_place_segment_len(source, fields.drop_first(), rest_lengths, next);
    }
}

/// Ordered selectors are sound for every split: when every field up to
/// `index` is single, logical field `index` is the single physical field
/// `index` of the fact, whatever the capture lengths (unbounded: any number
/// of fields and captures, any lengths).
pub proof fn theorem_ordered_selector_reads_physical_field(
    fields: Seq<SeqField>,
    lengths: Seq<int>,
    offset: int,
    index: int,
)
    requires
        0 <= index < fields.len(),
        forall|j: int| 0 <= j <= index ==> fields[j] is Single,
    ensures
        place_segment(None, fields, lengths, offset)[index] == (Placed {
            source: None,
            start: offset + index,
            len: 1,
            single: true,
        }),
    decreases index,
{
    lemma_place_segment_len(None, fields.drop_first(), lengths, offset + 1);
    if index > 0 {
        assert forall|j: int| 0 <= j <= index - 1 implies (#[trigger] fields.drop_first()[j]) is Single by {
            assert(fields.drop_first()[j] == fields[j + 1]);
        }
        theorem_ordered_selector_reads_physical_field(
            fields.drop_first(),
            lengths,
            offset + 1,
            index - 1,
        );
    }
}

/// Validated template plans (crates/ferric-rules-core/src/sequence.rs:169 fn
/// `validate`): a scalar slot segment projects exactly one single field.
pub open spec fn scalar_segments_single(segments: Seq<Segment>) -> bool {
    forall|i: int|
        0 <= i < segments.len() && (#[trigger] segments[i]).source is TemplateScalar
            ==> segments[i].fields == seq![SeqField::Single]
}

/// Template selectors are sound for every split of a validated plan: when
/// `physical_selector` maps logical field `index` to template slot `p`, the
/// logical field is the single value at position 0 of slot `p`'s source,
/// which for a scalar source is the slot value itself.
pub proof fn theorem_template_selector_reads_physical_slot(
    segments: Seq<Segment>,
    lengths: Seq<int>,
    index: int,
)
    requires
        scalar_segments_single(segments),
        template_physical_selector(segments, index) is Some,
    ensures
        0 <= index < place_all(segments, lengths).len(),
        place_all(segments, lengths)[index] == (Placed {
            source: template_physical_selector(segments, index),
            start: 0,
            len: 1,
            single: true,
        }),
    decreases segments.len(),
{
    let first = segments[0];
    let own = multis(first.fields);
    let rest_lengths = lengths.subrange(
        if own <= lengths.len() {
            own
        } else {
            lengths.len() as int
        },
        lengths.len() as int,
    );
    lemma_place_segment_len(source_slot(first.source), first.fields, lengths, 0);
    if index < first.fields.len() {
        assert(segments[0].source is TemplateScalar);
        assert(first.fields == seq![SeqField::Single]);
        reveal_with_fuel(place_segment, 2);
    } else {
        assert forall|i: int|
            0 <= i < segments.drop_first().len() && (
            #[trigger] segments.drop_first()[i]).source is TemplateScalar implies segments.drop_first()[i].fields
            == seq![SeqField::Single] by {
            assert(segments.drop_first()[i] == segments[i + 1]);
        }
        theorem_template_selector_reads_physical_slot(
            segments.drop_first(),
            rest_lengths,
            index - first.fields.len(),
        );
    }
}

// ===========================================================================
// Candidate collection for sequence plans
// ===========================================================================

/// The value of logical field `logical` in split `split` of fact `f`
/// (`SplitView::get`, crates/ferric-rules-core/src/sequence.rs): an
/// uninterpreted function, so the theorem below holds for every split
/// enumeration and every projection.
pub uninterp spec fn split_value(f: int, split: int, logical: int) -> Option<Val>;

/// The number of splits the search can visit for fact `f`.
pub uninterp spec fn split_count(f: int) -> nat;

/// One join test against a split (crates/ferric-rules-core/src/rete.rs:2699
/// `evaluate_join_fields` with `field = |slot| split.get(slot)`).
pub open spec fn split_test_passes(f: int, split: int, b: Seq<Option<Val>>, t: JoinTest) -> bool {
    match split_value(f, split, t.slot) {
        None => false,
        Some(fact_value) => match binding_get(b, t.var) {
            None => false,
            Some(bound) => match t.ty {
                TestTy::Equal => join_eq(fact_value, bound) == Some(true),
                TestTy::Other(code) => other_test_holds(code, fact_value, bound),
            },
        },
    }
}

pub open spec fn split_join_from(
    f: int,
    split: int,
    b: Seq<Option<Val>>,
    tests: Seq<JoinTest>,
    i: int,
) -> bool
    decreases tests.len() - i,
{
    if i >= tests.len() || i < 0 {
        true
    } else if !split_test_passes(f, split, b, tests[i]) {
        false
    } else {
        split_join_from(f, split, b, tests, i + 1)
    }
}

/// `evaluate_pattern` with a plan (crates/ferric-rules-core/src/rete.rs:2816,
/// via `any_split_matches`): some split passes every join test. Which
/// splits the search visits and how it prunes them is abstracted to
/// `split_count`; only the existence of a passing split matters here.
pub open spec fn pattern_matches(f: int, b: Seq<Option<Val>>, tests: Seq<JoinTest>) -> bool {
    exists|split: int| 0 <= split < split_count(f) && #[trigger] split_join_from(f, split, b, tests, 0)
}

/// The first item of crates/ferric-rules-core/src/rete.rs:2797
/// `indexable_tests(tests, Some(plan))`: the first `Equal` test whose
/// logical field has a physical selector (`selector`, standing for
/// `SequencePattern::physical_selector`), mapped to that selector.
pub open spec fn first_indexable_with_plan(
    tests: Seq<JoinTest>,
    selector: spec_fn(int) -> Option<int>,
    i: int,
) -> Option<(int, int)>
    decreases tests.len() - i,
{
    if i >= tests.len() || i < 0 {
        None
    } else if tests[i].ty is Equal && selector(tests[i].slot) is Some {
        Some((selector(tests[i].slot)->Some_0, tests[i].var))
    } else {
        first_indexable_with_plan(tests, selector, i + 1)
    }
}

/// Selector soundness, the hypothesis of the theorem below: whenever the
/// selector maps logical field `l` to physical slot `p`, every split reads
/// at `l` the value the fact holds at `p`. For the placement model this is
/// `theorem_ordered_selector_reads_physical_field` and
/// `theorem_template_selector_reads_physical_slot`; for the production
/// split search it is not proved (see the header).
pub open spec fn selector_sound(selector: spec_fn(int) -> Option<int>) -> bool {
    forall|f: int, split: int, l: int|
        0 <= split < split_count(f) && selector(l) is Some ==> #[trigger] split_value(f, split, l)
            == field(f, selector(l)->Some_0)
}

pub proof fn lemma_first_indexable_with_plan_is_an_equal_test(
    tests: Seq<JoinTest>,
    selector: spec_fn(int) -> Option<int>,
    i: int,
)
    requires
        0 <= i,
        first_indexable_with_plan(tests, selector, i) is Some,
    ensures
        exists|j: int|
            i <= j < tests.len() && (#[trigger] tests[j]).ty is Equal && selector(tests[j].slot) is Some
                && first_indexable_with_plan(tests, selector, i) == Some(
                (selector(tests[j].slot)->Some_0, tests[j].var),
            ),
    decreases tests.len() - i,
{
    if !(tests[i].ty is Equal && selector(tests[i].slot) is Some) {
        lemma_first_indexable_with_plan_is_an_equal_test(tests, selector, i + 1);
        let j = choose|j: int|
            i + 1 <= j < tests.len() && (#[trigger] tests[j]).ty is Equal && selector(
                tests[j].slot,
            ) is Some && first_indexable_with_plan(tests, selector, i + 1) == Some(
                (selector(tests[j].slot)->Some_0, tests[j].var),
            );
        assert(i <= j);
    }
}

pub proof fn lemma_split_join_implies_each(
    f: int,
    split: int,
    b: Seq<Option<Val>>,
    tests: Seq<JoinTest>,
    i: int,
    j: int,
)
    requires
        0 <= i <= j < tests.len(),
        split_join_from(f, split, b, tests, i),
    ensures
        split_test_passes(f, split, b, tests[j]),
    decreases j - i,
{
    if i < j {
        lemma_split_join_implies_each(f, split, b, tests, i + 1, j);
    }
}

/// Candidate collection for a join with a sequence plan
/// (crates/ferric-rules-core/src/rete.rs:1185 `left_activate` calls
/// `collect_candidate_facts` with `indexable_tests(tests, plan)` and then
/// keeps the facts some split of which passes every join test): if the
/// physical selectors are sound, filtering the candidates by
/// `pattern_matches` yields exactly the facts, in the same order, that
/// filtering the whole memory would — for any memory satisfying `inv`, any
/// tests, bindings, split enumeration and number of facts.
pub proof fn theorem_plan_candidates_then_residual_equals_scan(
    m: AlphaMem,
    tests: Seq<JoinTest>,
    b: Seq<Option<Val>>,
    selector: spec_fn(int) -> Option<int>,
)
    requires
        inv(m),
        selector_sound(selector),
    ensures
        collect_candidate_facts(m, first_indexable_with_plan(tests, selector, 0), b).filter(
            |f: int| pattern_matches(f, b, tests),
        ) == m.facts.filter(|f: int| pattern_matches(f, b, tests)),
{
    let pass = |f: int| pattern_matches(f, b, tests);
    let first = first_indexable_with_plan(tests, selector, 0);
    if indexed_candidates(m, first, b) is Some {
        lemma_first_indexable_with_plan_is_an_equal_test(tests, selector, 0);
        let j = choose|j: int|
            0 <= j < tests.len() && (#[trigger] tests[j]).ty is Equal && selector(tests[j].slot) is Some
                && first_indexable_with_plan(tests, selector, 0) == Some(
                (selector(tests[j].slot)->Some_0, tests[j].var),
            );
        let slot = selector(tests[j].slot)->Some_0;
        let bound = binding_get(b, tests[j].var).unwrap();
        let key = atom_key(bound).unwrap();
        assert(lookup(m, slot, key) == bucket(m.facts, slot, key));
        assert(collect_candidate_facts(m, first, b) == bucket(m.facts, slot, key));
        let has_key = |f: int| key_at(f, slot) == Some(key);
        lemma_filter_filter(m.facts, has_key, pass);
        assert forall|i: int| 0 <= i < m.facts.len() implies (has_key(m.facts[i]) && pass(
            m.facts[i],
        )) == pass(m.facts[i]) by {
            let f = m.facts[i];
            if pass(f) {
                let split = choose|split: int|
                    0 <= split < split_count(f) && #[trigger] split_join_from(f, split, b, tests, 0);
                lemma_split_join_implies_each(f, split, b, tests, 0, j);
                let fact_value = split_value(f, split, tests[j].slot).unwrap();
                lemma_join_eq_implies_same_key(fact_value, bound);
                assert(split_value(f, split, tests[j].slot) == field(f, slot));
            }
        }
        lemma_filter_ext(m.facts, |f: int| has_key(f) && pass(f), pass);
    }
}

// ===========================================================================
// BindingSet (executable model)
// ===========================================================================

/// The binding vector after `set(var, value)`: grown with unbound slots to
/// cover `var`, then `var` bound.
pub open spec fn set_spec(b: Seq<Option<u64>>, var: int, value: u64) -> Seq<Option<u64>> {
    let grown = if var >= b.len() {
        b + Seq::new((var + 1 - b.len()) as nat, |i: int| None::<u64>)
    } else {
        b
    };
    grown.update(var, Some(value))
}

pub open spec fn get_spec(b: Seq<Option<u64>>, var: int) -> Option<u64> {
    if 0 <= var < b.len() {
        b[var]
    } else {
        None
    }
}

/// The binding vector width `extend_from` produces after its first `n`
/// iterations: the receiver's width, grown to cover each variable bound in
/// `other` before `n`.
pub open spec fn extend_width(mine: Seq<Option<u64>>, other: Seq<Option<u64>>, n: int) -> int
    decreases n,
{
    if n <= 0 {
        mine.len() as int
    } else {
        let previous = extend_width(mine, other, n - 1);
        if n - 1 < other.len() && other[n - 1] is Some && previous < n {
            n
        } else {
            previous
        }
    }
}

/// Left-biased union of two binding vectors, over the first `n` variables of
/// `other`.
pub open spec fn union_get(
    mine: Seq<Option<u64>>,
    other: Seq<Option<u64>>,
    n: int,
    var: int,
) -> Option<u64> {
    if get_spec(mine, var) is Some {
        get_spec(mine, var)
    } else if var < n {
        get_spec(other, var)
    } else {
        None
    }
}

/// Models crates/ferric-rules-core/src/binding.rs:161 `BindingSet`. A bound `ValueRef` is abstracted to a
/// `u64` token: `BindingSet` only stores, copies (`clone`) and returns values,
/// never inspects them. `MAX_VARS` = 65536 is the number of `VarId(u16)`s.
pub struct BindingSet {
    pub bindings: Vec<Option<u64>>,
}

pub open spec fn max_vars() -> int {
    65536
}

impl BindingSet {
    pub open spec fn wf(&self) -> bool {
        self.bindings.len() <= max_vars()
    }

    /// Models crates/ferric-rules-core/src/binding.rs:168 `BindingSet::new`.
    pub fn new() -> (r: Self)
        ensures
            r.bindings@ == Seq::<Option<u64>>::empty(),
            r.wf(),
    {
        BindingSet { bindings: Vec::new() }
    }

    /// Models crates/ferric-rules-core/src/binding.rs:176 `BindingSet::get`: total over every `VarId`.
    pub fn get(&self, var: u16) -> (r: Option<u64>)
        ensures
            r == get_spec(self.bindings@, var as int),
    {
        if (var as usize) < self.bindings.len() {
            self.bindings[var as usize]
        } else {
            None
        }
    }

    /// Models crates/ferric-rules-core/src/binding.rs:183 `BindingSet::set`.
    pub fn set(&mut self, var: u16, value: u64)
        requires
            old(self).wf(),
        ensures
            final(self).bindings@ == set_spec(old(self).bindings@, var as int, value),
            final(self).wf(),
    {
        let idx = var as usize;
        if idx >= self.bindings.len() {
            self.bindings.resize(idx + 1, None);
        }
        self.bindings.set(idx, Some(value));
        proof {
            let grown = if var as int >= old(self).bindings@.len() {
                old(self).bindings@ + Seq::new(
                    (var as int + 1 - old(self).bindings@.len()) as nat,
                    |i: int| None::<u64>,
                )
            } else {
                old(self).bindings@
            };
            assert(self.bindings@ =~= grown.update(var as int, Some(value)));
        }
    }

    /// Models crates/ferric-rules-core/src/binding.rs:195 `BindingSet::extend_from`. The production loop
    /// iterates `other.bindings.iter().enumerate()`; the model indexes the
    /// same positions in the same order.
    pub fn extend_from(&mut self, other: &BindingSet)
        requires
            old(self).wf(),
            other.wf(),
        ensures
            final(self).wf(),
            final(self).bindings.len() == extend_width(
                old(self).bindings@,
                other.bindings@,
                other.bindings.len() as int,
            ),
            forall|var: int|
                #[trigger] get_spec(final(self).bindings@, var) == union_get(
                    old(self).bindings@,
                    other.bindings@,
                    other.bindings.len() as int,
                    var,
                ),
    {
        let ghost mine = self.bindings@;
        let mut idx: usize = 0;
        while idx < other.bindings.len()
            invariant
                self.wf(),
                other.wf(),
                mine == old(self).bindings@,
                idx <= other.bindings.len(),
                self.bindings.len() == extend_width(mine, other.bindings@, idx as int),
                forall|var: int|
                    #[trigger] get_spec(self.bindings@, var) == union_get(
                        mine,
                        other.bindings@,
                        idx as int,
                        var,
                    ),
            decreases other.bindings.len() - idx,
        {
            let ghost before = self.bindings@;
            if let Some(val) = other.bindings[idx] {
                // `idx as u16` in production: no truncation, because a
                // well-formed binding vector has at most 65536 entries.
                assert(idx < 65536);
                let var = idx as u16;
                if self.get(var).is_none() {
                    self.set(var, val);
                }
            }
            proof {
                assert forall|v: int|
                    #[trigger] get_spec(self.bindings@, v) == union_get(
                        mine,
                        other.bindings@,
                        idx as int + 1,
                        v,
                    ) by {
                    assert(get_spec(before, v) == union_get(mine, other.bindings@, idx as int, v));
                    assert(get_spec(before, idx as int) == union_get(
                        mine,
                        other.bindings@,
                        idx as int,
                        idx as int,
                    ));
                }
            }
            idx += 1;
        }
    }

    /// Models crates/ferric-rules-core/src/binding.rs:208 `BindingSet::capacity`.
    pub fn capacity(&self) -> (r: usize)
        ensures
            r == self.bindings.len(),
    {
        self.bindings.len()
    }
}

/// `set` refines insertion into a total map from variables to optional
/// values: it binds `var`, leaves every other variable as it was, and
/// widens the vector exactly to cover `var`.
pub proof fn theorem_set_refines_map_insert(b: Seq<Option<u64>>, var: int, value: u64, probe: int)
    requires
        0 <= var,
    ensures
        get_spec(set_spec(b, var, value), probe) == if probe == var {
            Some(value)
        } else {
            get_spec(b, probe)
        },
        set_spec(b, var, value).len() == if var < b.len() {
            b.len() as int
        } else {
            var + 1
        },
{
}

/// `extend_from` is a left-biased union and the width grows to cover the
/// highest variable bound in `other` (unbound trailing slots of `other` do
/// not widen the receiver).
pub proof fn theorem_extend_width_is_highest_bound(
    mine: Seq<Option<u64>>,
    other: Seq<Option<u64>>,
    n: int,
)
    requires
        0 <= n <= other.len(),
    ensures
        extend_width(mine, other, n) >= mine.len(),
        forall|i: int| 0 <= i < n && other[i] is Some ==> extend_width(mine, other, n) > i,
        extend_width(mine, other, n) == mine.len() || (exists|i: int|
            0 <= i < n && other[i] is Some && extend_width(mine, other, n) == i + 1),
    decreases n,
{
    if n > 0 {
        theorem_extend_width_is_highest_bound(mine, other, n - 1);
    }
}

// ===========================================================================
// VarMap
// ===========================================================================

/// Models crates/ferric-rules-core/src/binding.rs:29 `VarMap`: `by_name` (an `FxHashMap<Symbol, VarId>`)
/// and `by_id` (a `Vec<Symbol>`); symbols are ints, ids are ints in
/// `0..=u16::MAX`.
pub struct VarMap {
    pub by_name: Map<int, int>,
    pub by_id: Seq<int>,
}

pub open spec fn var_map_inv(vm: VarMap) -> bool {
    &&& vm.by_id.len() <= max_vars()
    &&& vm.by_id.no_duplicates()
    &&& forall|name: int| #[trigger] vm.by_name.contains_key(name) <==> vm.by_id.contains(name)
    &&& forall|name: int|
        #[trigger] vm.by_name.contains_key(name) ==> 0 <= vm.by_name[name] < vm.by_id.len()
            && vm.by_id[vm.by_name[name]] == name
}

/// Models crates/ferric-rules-core/src/binding.rs:49 `VarMap::get_or_create`; `Err` is
/// `VarMapError::TooManyVariables`.
pub open spec fn get_or_create(vm: VarMap, name: int) -> (VarMap, Result<int, ()>) {
    if vm.by_name.contains_key(name) {
        (vm, Ok(vm.by_name[name]))
    } else {
        let id_val = vm.by_id.len() as int;
        if id_val > 65535 {
            (vm, Err(()))
        } else {
            (VarMap { by_name: vm.by_name.insert(name, id_val), by_id: vm.by_id.push(name) }, Ok(id_val))
        }
    }
}

/// Models crates/ferric-rules-core/src/binding.rs:68 `VarMap::lookup`.
pub open spec fn var_map_lookup(vm: VarMap, name: int) -> Option<int> {
    if vm.by_name.contains_key(name) {
        Some(vm.by_name[name])
    } else {
        None
    }
}

/// `get_or_create` preserves the bijection between names and dense ids,
/// returns the name's existing id, else the next sequential id (which fits a
/// `u16`, so the `as u16` cast never truncates), and fails exactly when a new
/// name arrives with all 65536 ids taken.
pub proof fn theorem_get_or_create(vm: VarMap, name: int)
    requires
        var_map_inv(vm),
    ensures
        var_map_inv(get_or_create(vm, name).0),
        get_or_create(vm, name).1 is Err <==> (!vm.by_name.contains_key(name) && vm.by_id.len()
            == max_vars()),
        get_or_create(vm, name).1 is Ok ==> ({
            let id = get_or_create(vm, name).1->Ok_0;
            &&& 0 <= id <= 65535
            &&& get_or_create(vm, name).0.by_id[id] == name
            &&& var_map_lookup(get_or_create(vm, name).0, name) == Some(id)
            &&& vm.by_id.contains(name) ==> id == vm.by_name[name]
            &&& !vm.by_id.contains(name) ==> id == vm.by_id.len()
        }),
        forall|other: int|
            other != name ==> #[trigger] var_map_lookup(get_or_create(vm, name).0, other)
                == var_map_lookup(vm, other),
{
    let (out, result) = get_or_create(vm, name);
    if !vm.by_name.contains_key(name) && vm.by_id.len() <= 65535 {
        lemma_push_no_duplicates(vm.by_id, name);
        assert forall|n: int| #[trigger] out.by_name.contains_key(n) <==> out.by_id.contains(n) by {
            vstd::seq_lib::lemma_seq_contains_after_push(vm.by_id, name, n);
        }
    }
}

} // verus!
