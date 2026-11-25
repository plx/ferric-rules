use vstd::prelude::*;

verus! {

// =====================================================================
// Section 0. Shared abstractions
// =====================================================================

/// Opaque identifiers.  Production `TokenId`/`FactId` are slotmap keys
/// (crates/ferric-rules-core/src/token.rs:18, fact.rs); only equality matters.
pub type TokenId = int;

pub type FactId = int;

// ---------------------------------------------------------------------
// OrderedSet<K> (crates/ferric-rules-core/src/ordered_set.rs:15) modelled as
// its abstract value: the insertion-ordered key sequence without duplicates.
// ---------------------------------------------------------------------

/// Models ordered_set.rs:32 `OrderedSet::insert` (append if absent).
pub open spec fn os_insert<K>(s: Seq<K>, k: K) -> Seq<K> {
    if s.contains(k) {
        s
    } else {
        s.push(k)
    }
}

/// Models ordered_set.rs:92 `OrderedSet::remove` (unlink, keep order).
pub open spec fn os_remove<K>(s: Seq<K>, k: K) -> Seq<K> {
    if s.contains(k) {
        s.remove(s.index_of(k))
    } else {
        s
    }
}

/// Models `OrderedSet::iter().next()` (ordered_set.rs:151, 168).
pub open spec fn os_first<K>(s: Seq<K>) -> Option<K> {
    if s.len() > 0 {
        Some(s[0])
    } else {
        None
    }
}

/// `key` is strictly increasing along `s`.
pub open spec fn sorted_by<K>(s: Seq<K>, key: spec_fn(K) -> nat) -> bool {
    forall|i: int, j: int| 0 <= i < j < s.len() ==> key(#[trigger] s[i]) < key(#[trigger] s[j])
}

pub proof fn lemma_os_insert<K>(s: Seq<K>, k: K)
    requires
        s.no_duplicates(),
    ensures
        os_insert(s, k).no_duplicates(),
        forall|x: K| #[trigger] os_insert(s, k).contains(x) <==> (s.contains(x) || x == k),
        os_insert(s, k).len() > 0,
        os_insert(s, k).len() == if s.contains(k) { s.len() as int } else { s.len() + 1 as int },
        s.len() > 0 ==> os_insert(s, k)[0] == s[0],
        s.len() == 0 ==> os_insert(s, k)[0] == k,
        forall|key: spec_fn(K) -> nat|
            sorted_by(s, key) && (forall|x: K| s.contains(x) ==> key(x) < key(k)) ==> #[trigger] sorted_by(
                os_insert(s, k),
                key,
            ),
{
    if !s.contains(k) {
        let t = s.push(k);
        assert(t[t.len() - 1] == k);
        assert forall|x: K| #[trigger] t.contains(x) <==> (s.contains(x) || x == k) by {
            if s.contains(x) {
                let i = choose|i: int| 0 <= i < s.len() && s[i] == x;
                assert(t[i] == x);
            }
            if x == k {
                assert(t[t.len() - 1] == x);
            }
            if t.contains(x) {
                let i = choose|i: int| 0 <= i < t.len() && t[i] == x;
                if i < s.len() {
                    assert(s[i] == x);
                }
            }
        }
        assert forall|key: spec_fn(K) -> nat|
            sorted_by(s, key) && (forall|x: K| s.contains(x) ==> key(x) < key(k)) implies #[trigger] sorted_by(
            t,
            key,
        ) by {
            assert forall|i: int, j: int| 0 <= i < j < t.len() implies key(#[trigger] t[i]) < key(
                #[trigger] t[j],
            ) by {
                if j == s.len() {
                    assert(s.contains(s[i]));
                } else {
                    assert(t[i] == s[i] && t[j] == s[j]);
                }
            }
        }
    }
}

pub proof fn lemma_os_remove<K>(s: Seq<K>, k: K)
    requires
        s.no_duplicates(),
    ensures
        os_remove(s, k).no_duplicates(),
        forall|x: K| #[trigger] os_remove(s, k).contains(x) <==> (s.contains(x) && x != k),
        os_remove(s, k).len() == if s.contains(k) { s.len() - 1 } else { s.len() as int },
        s.len() > 0 && s[0] != k ==> os_remove(s, k)[0] == s[0],
        s.len() > 1 && s[0] == k ==> os_remove(s, k)[0] == s[1],
        forall|key: spec_fn(K) -> nat| sorted_by(s, key) ==> #[trigger] sorted_by(os_remove(s, k), key),
{
    if s.contains(k) {
        let i = s.index_of(k);
        let t = s.remove(i);
        assert(0 <= i < s.len() && s[i] == k);
        assert(t.len() == s.len() - 1);
        assert forall|j: int| 0 <= j < t.len() implies #[trigger] t[j] == (if j < i {
            s[j]
        } else {
            s[j + 1]
        }) by {}
        assert forall|x: K| #[trigger] t.contains(x) <==> (s.contains(x) && x != k) by {
            if t.contains(x) {
                let j = choose|j: int| 0 <= j < t.len() && t[j] == x;
                if j < i {
                    assert(s[j] == x);
                    assert(j != i);
                } else {
                    assert(s[j + 1] == x);
                    assert(j + 1 != i);
                }
            }
            if s.contains(x) && x != k {
                let j = choose|j: int| 0 <= j < s.len() && s[j] == x;
                assert(j != i);
                if j < i {
                    assert(t[j] == x);
                } else {
                    assert(t[j - 1] == x);
                }
            }
        }
        if s.len() > 0 && s[0] == k {
            assert(i == 0);
        }
        if s.len() > 0 && s[0] != k {
            assert(i != 0);
        }
        assert forall|key: spec_fn(K) -> nat| sorted_by(s, key) implies #[trigger] sorted_by(
            t,
            key,
        ) by {
            assert forall|a: int, b: int| 0 <= a < b < t.len() implies key(#[trigger] t[a]) < key(
                #[trigger] t[b],
            ) by {
                let a2 = if a < i {
                    a
                } else {
                    a + 1
                };
                let b2 = if b < i {
                    b
                } else {
                    b + 1
                };
                assert(t[a] == s[a2] && t[b] == s[b2]);
            }
        }
    }
}

// ---------------------------------------------------------------------
// HashMap<K, HashSet<T>> reverse-index idioms used verbatim by several
// production functions.
// ---------------------------------------------------------------------
/// `m.entry(k).or_default().insert(t)`
pub open spec fn link<K, T>(m: Map<K, Set<T>>, k: K, t: T) -> Map<K, Set<T>> {
    m.insert(
        k,
        (if m.contains_key(k) {
            m[k]
        } else {
            Set::empty()
        }).insert(t),
    )
}

/// `if let Some(ts) = m.get_mut(&k) { ts.remove(&t); if ts.is_empty() { m.remove(&k); } }`
/// (negative.rs:117-122, negative.rs:202-207, exists.rs:96-101, exists.rs:157-162).
pub open spec fn unlink<K, T>(m: Map<K, Set<T>>, k: K, t: T) -> Map<K, Set<T>> {
    if m.contains_key(k) {
        let ts = m[k].remove(t);
        if ts.is_empty() {
            m.remove(k)
        } else {
            m.insert(k, ts)
        }
    } else {
        m
    }
}

/// The `for k in ks { unlink(k, t) }` loops of negative.rs:201 / exists.rs:156,
/// as a left fold in iteration order.
pub open spec fn unlink_all<K, T>(m: Map<K, Set<T>>, ks: Seq<K>, t: T) -> Map<K, Set<T>>
    decreases ks.len(),
{
    if ks.len() == 0 {
        m
    } else {
        unlink(unlink_all(m, ks.drop_last(), t), ks.last(), t)
    }
}

pub open spec fn in_rel<K, T>(m: Map<K, Set<T>>, k: K, t: T) -> bool {
    m.contains_key(k) && m[k].contains(t)
}

pub open spec fn no_empty_sets<K, T>(m: Map<K, Set<T>>) -> bool {
    forall|k: K| #[trigger] m.contains_key(k) ==> !m[k].is_empty()
}

pub proof fn lemma_link<K, T>(m: Map<K, Set<T>>, k: K, t: T)
    ensures
        forall|k2: K, t2: T| #[trigger]
            in_rel(link(m, k, t), k2, t2) <==> (in_rel(m, k2, t2) || (k2 == k && t2 == t)),
        no_empty_sets(m) ==> no_empty_sets(link(m, k, t)),
        link(m, k, t).dom() == m.dom().insert(k),
        forall|k2: K| k2 != k && m.contains_key(k2) ==> #[trigger] link(m, k, t)[k2] == m[k2],
        link(m, k, t)[k] == (if m.contains_key(k) {
            m[k]
        } else {
            Set::empty()
        }).insert(t),
{
    let r = link(m, k, t);
    assert(r.dom() =~= m.dom().insert(k));
    assert(r[k].contains(t));
}

pub proof fn lemma_unlink<K, T>(m: Map<K, Set<T>>, k: K, t: T)
    requires
        no_empty_sets(m),
    ensures
        forall|k2: K, t2: T| #[trigger]
            in_rel(unlink(m, k, t), k2, t2) <==> (in_rel(m, k2, t2) && !(k2 == k && t2 == t)),
        no_empty_sets(unlink(m, k, t)),
        forall|k2: K| #[trigger] unlink(m, k, t).contains_key(k2) ==> m.contains_key(k2),
        forall|k2: K|
            k2 != k ==> (#[trigger] unlink(m, k, t).contains_key(k2) == m.contains_key(k2)),
        forall|k2: K| k2 != k && m.contains_key(k2) ==> #[trigger] unlink(m, k, t)[k2] == m[k2],
{
}

pub proof fn lemma_unlink_all<K, T>(m: Map<K, Set<T>>, ks: Seq<K>, t: T)
    requires
        no_empty_sets(m),
    ensures
        forall|k2: K, t2: T| #[trigger]
            in_rel(unlink_all(m, ks, t), k2, t2) <==> (in_rel(m, k2, t2) && !(t2 == t && ks.contains(
                k2,
            ))),
        no_empty_sets(unlink_all(m, ks, t)),
        forall|k2: K| #[trigger] unlink_all(m, ks, t).contains_key(k2) ==> m.contains_key(k2),
    decreases ks.len(),
{
    if ks.len() > 0 {
        let prev = unlink_all(m, ks.drop_last(), t);
        lemma_unlink_all(m, ks.drop_last(), t);
        lemma_unlink(prev, ks.last(), t);
        assert forall|k2: K| ks.contains(k2) <==> (ks.drop_last().contains(k2) || k2 == ks.last()) by {
            if ks.contains(k2) {
                let i = choose|i: int| 0 <= i < ks.len() && ks[i] == k2;
                if i < ks.len() - 1 {
                    assert(ks.drop_last()[i] == k2);
                }
            }
            if ks.drop_last().contains(k2) {
                let i = choose|i: int| 0 <= i < ks.drop_last().len() && ks.drop_last()[i] == k2;
                assert(ks[i] == k2);
            }
            if k2 == ks.last() {
                assert(ks[ks.len() - 1] == k2);
            }
        }
    }
}

/// With no empty sets stored, a key is present iff it relates to something.
pub proof fn lemma_dom_has<K, T>(m: Map<K, Set<T>>, k: K)
    requires
        no_empty_sets(m),
    ensures
        m.contains_key(k) <==> exists|t: T| in_rel(m, k, t),
{
    if m.contains_key(k) {
        assert(!m[k].is_empty());
        let t = m[k].choose();
        assert(m[k].contains(t));
        assert(in_rel(m, k, t));
    }
}

// ---------------------------------------------------------------------
// From-scratch views of a ground-truth relation `pairs: Set<(A, B)>`.
// ---------------------------------------------------------------------
/// The from-scratch forward index: each `a` related to something maps to its
/// set of partners.
pub open spec fn scratch_fwd<A, B>(pairs: Set<(A, B)>) -> Map<A, Set<B>> {
    Map::new(
        pairs.map(|pr: (A, B)| pr.0),
        |a: A| pairs.filter(|pr: (A, B)| pr.0 == a).map_by(|pr: (A, B)| pr.1, |b: B| (a, b)),
    )
}

/// The from-scratch reverse index.
pub open spec fn scratch_rev<A, B>(pairs: Set<(A, B)>) -> Map<B, Set<A>> {
    Map::new(
        pairs.map(|pr: (A, B)| pr.1),
        |b: B| pairs.filter(|pr: (A, B)| pr.1 == b).map_by(|pr: (A, B)| pr.0, |a: A| (a, b)),
    )
}

/// A map of sets that stores no empty set and agrees pointwise with `pairs`
/// *is* the from-scratch forward index of `pairs`.
pub proof fn lemma_scratch_fwd<A, B>(m: Map<A, Set<B>>, pairs: Set<(A, B)>)
    requires
        no_empty_sets(m),
        forall|a: A, b: B| #[trigger] pairs.contains((a, b)) <==> in_rel(m, a, b),
    ensures
        m == scratch_fwd(pairs),
        forall|a: A| #[trigger] m.contains_key(a) <==> exists|b: B| pairs.contains((a, b)),
{
    let sf = scratch_fwd(pairs);
    assert forall|a: A| #[trigger] m.contains_key(a) <==> exists|b: B| pairs.contains((a, b)) by {
        lemma_dom_has(m, a);
        if m.contains_key(a) {
            let b = choose|b: B| in_rel(m, a, b);
            assert(pairs.contains((a, b)));
        }
    }
    assert forall|a: A| #[trigger] m.contains_key(a) <==> sf.contains_key(a) by {
        if m.contains_key(a) {
            let b = choose|b: B| pairs.contains((a, b));
            assert(pairs.contains((a, b)) && (a, b).0 == a);
        }
        if sf.contains_key(a) {
            let pr = choose|pr: (A, B)| #![trigger pairs.contains(pr)] pairs.contains(pr) && a == pr.0;
            assert(pairs.contains((pr.0, pr.1)));
        }
    }
    assert(m.dom() =~= sf.dom());
    assert forall|a: A| #[trigger] m.contains_key(a) implies m[a] == sf[a] by {
        let fa = pairs.filter(|pr: (A, B)| pr.0 == a);
        assert(sf[a] == fa.map_by(|pr: (A, B)| pr.1, |b: B| (a, b)));
        assert forall|b: B| m[a].contains(b) <==> sf[a].contains(b) by {
            assert(in_rel(m, a, b) <==> pairs.contains((a, b)));
            assert(fa.contains((a, b)) <==> pairs.contains((a, b)));
        }
        assert(m[a] =~= sf[a]);
    }
    assert(m =~= sf);
}

/// Same for the reverse index.
pub proof fn lemma_scratch_rev<A, B>(m: Map<B, Set<A>>, pairs: Set<(A, B)>)
    requires
        no_empty_sets(m),
        forall|a: A, b: B| #[trigger] pairs.contains((a, b)) <==> in_rel(m, b, a),
    ensures
        m == scratch_rev(pairs),
{
    let sr = scratch_rev(pairs);
    assert forall|b: B| #[trigger] m.contains_key(b) <==> sr.contains_key(b) by {
        lemma_dom_has(m, b);
        if m.contains_key(b) {
            let a = choose|a: A| in_rel(m, b, a);
            assert(pairs.contains((a, b)) && (a, b).1 == b);
        }
        if sr.contains_key(b) {
            let pr = choose|pr: (A, B)| #![trigger pairs.contains(pr)] pairs.contains(pr) && b == pr.1;
            assert(pairs.contains((pr.0, pr.1)));
            assert(in_rel(m, b, pr.0));
        }
    }
    assert(m.dom() =~= sr.dom());
    assert forall|b: B| #[trigger] m.contains_key(b) implies m[b] == sr[b] by {
        let fb = pairs.filter(|pr: (A, B)| pr.1 == b);
        assert(sr[b] == fb.map_by(|pr: (A, B)| pr.0, |a: A| (a, b)));
        assert forall|a: A| m[b].contains(a) <==> sr[b].contains(a) by {
            assert(in_rel(m, b, a) <==> pairs.contains((a, b)));
            assert(fb.contains((a, b)) <==> pairs.contains((a, b)));
        }
        assert(m[b] =~= sr[b]);
    }
    assert(m =~= sr);
}

/// A block-list chronology value not used by any parent of this memory
/// (what rete.rs:193 `allocate_block_order` hands out).
pub open spec fn fresh_order(m: Map<TokenId, u64>, o: u64) -> bool {
    forall|q: TokenId| #[trigger] m.contains_key(q) ==> m[q] != o
}

// =====================================================================
// Section 1. NegativeMemory (crates/ferric-rules-core/src/negative.rs)
// =====================================================================

/// negative.rs:41 `struct NegativeMemory` (the `id` field only feeds error
/// messages and is omitted).  `HashMap` -> `Map`, `HashSet` -> `Set`,
/// `OrderedSet` -> insertion-ordered `Seq` (see `os_*`).
pub struct NegMem {
    pub blocked: Map<TokenId, Seq<FactId>>,
    pub block_order: Map<TokenId, u64>,
    pub fact_to_blocked: Map<FactId, Set<TokenId>>,
    pub unblocked: Map<TokenId, TokenId>,
}

/// negative.rs:63 `NegativeMemory::new`
pub open spec fn neg_new() -> NegMem {
    NegMem {
        blocked: Map::empty(),
        block_order: Map::empty(),
        fact_to_blocked: Map::empty(),
        unblocked: Map::empty(),
    }
}

/// negative.rs:78 `NegativeMemory::add_blocker`
pub open spec fn neg_add_blocker(s: NegMem, p: TokenId, f: FactId, order: u64) -> NegMem {
    // 79-81: the first blocker fixes the block-list position.
    let block_order = if !s.blocked.contains_key(p) {
        s.block_order.insert(p, order)
    } else {
        s.block_order
    };
    // 82-85: self.blocked.entry(p).or_default().insert(f)
    let blockers = if s.blocked.contains_key(p) {
        s.blocked[p]
    } else {
        Seq::<FactId>::empty()
    };
    let blocked = s.blocked.insert(p, os_insert(blockers, f));
    // 86-89: self.fact_to_blocked.entry(f).or_default().insert(p)
    let fact_to_blocked = link(s.fact_to_blocked, f, p);
    NegMem { blocked, block_order, fact_to_blocked, unblocked: s.unblocked }
}

/// negative.rs:97 `NegativeMemory::remove_blocker`; returns `(state, now_unblocked)`.
pub open spec fn neg_remove_blocker(s: NegMem, p: TokenId, f: FactId, replacement_order: u64) -> (
    NegMem,
    bool,
) {
    // 105-115
    let (blocked, block_order, now_unblocked) = if s.blocked.contains_key(p) {
        let was_primary = os_first(s.blocked[p]) == Some(f);  // 106
        let blockers = os_remove(s.blocked[p], f);  // 107
        if blockers.len() == 0 {  // 108 (OrderedSet::is_empty)
            (s.blocked.remove(p), s.block_order.remove(p), true)  // 109-111
        } else if was_primary {
            (s.blocked.insert(p, blockers), s.block_order.insert(p, replacement_order), false)  // 113
        } else {
            (s.blocked.insert(p, blockers), s.block_order, false)
        }
    } else {
        (s.blocked, s.block_order, false)
    };
    // 117-122
    let fact_to_blocked = unlink(s.fact_to_blocked, f, p);
    (NegMem { blocked, block_order, fact_to_blocked, unblocked: s.unblocked }, now_unblocked)
}

/// negative.rs:137 `is_blocked`
pub open spec fn neg_is_blocked(s: NegMem, p: TokenId) -> bool {
    s.blocked.contains_key(p)
}

/// negative.rs:142 `tokens_blocked_by` (the returned SmallVec holds exactly this set).
pub open spec fn neg_tokens_blocked_by(s: NegMem, f: FactId) -> Set<TokenId> {
    if s.fact_to_blocked.contains_key(f) {
        s.fact_to_blocked[f]
    } else {
        Set::empty()
    }
}

/// negative.rs:151 `primary_blocker`
pub open spec fn neg_primary_blocker(s: NegMem, p: TokenId) -> Option<FactId> {
    if s.blocked.contains_key(p) {
        os_first(s.blocked[p])
    } else {
        None
    }
}

/// negative.rs:168 `set_unblocked`
pub open spec fn neg_set_unblocked(s: NegMem, p: TokenId, pt: TokenId) -> NegMem {
    NegMem {
        blocked: s.blocked,
        block_order: s.block_order,
        fact_to_blocked: s.fact_to_blocked,
        unblocked: s.unblocked.insert(p, pt),
    }
}

/// negative.rs:181 `remove_unblocked`; returns `(state, removed pass-through)`.
pub open spec fn neg_remove_unblocked(s: NegMem, p: TokenId) -> (NegMem, Option<TokenId>) {
    (
        NegMem {
            blocked: s.blocked,
            block_order: s.block_order,
            fact_to_blocked: s.fact_to_blocked,
            unblocked: s.unblocked.remove(p),
        },
        s.unblocked.get(p),
    )
}

/// negative.rs:194 `remove_parent_token`
pub open spec fn neg_remove_parent_token(s: NegMem, p: TokenId) -> NegMem {
    let unblocked = s.unblocked.remove(p);  // 196
    let block_order = s.block_order.remove(p);  // 199
    if s.blocked.contains_key(p) {  // 200
        NegMem {
            blocked: s.blocked.remove(p),
            block_order,
            fact_to_blocked: unlink_all(s.fact_to_blocked, s.blocked[p], p),  // 201-208
            unblocked,
        }
    } else {
        NegMem { blocked: s.blocked, block_order, fact_to_blocked: s.fact_to_blocked, unblocked }
    }
}

/// negative.rs:128 `clear`
pub open spec fn neg_clear(s: NegMem) -> NegMem {
    NegMem {
        blocked: Map::empty(),
        block_order: Map::empty(),
        fact_to_blocked: Map::empty(),
        unblocked: Map::empty(),
    }
}

/// negative.rs:214 `is_empty`
pub open spec fn neg_is_empty(s: NegMem) -> bool {
    s.blocked.is_empty() && s.fact_to_blocked.is_empty() && s.unblocked.is_empty()
}

// ----------------------------- invariants -----------------------------
/// Forward index relation: `p` is blocked by `f`.
pub open spec fn neg_fwd(s: NegMem, p: TokenId, f: FactId) -> bool {
    s.blocked.contains_key(p) && s.blocked[p].contains(f)
}

/// Structural invariant maintained by every operation unconditionally
/// (modulo order freshness).
pub open spec fn neg_wf(s: NegMem) -> bool {
    &&& forall|p: TokenId| #[trigger]
        s.blocked.contains_key(p) ==> s.blocked[p].no_duplicates() && s.blocked[p].len() > 0
    &&& s.block_order.dom() == s.blocked.dom()
    &&& s.block_order.is_injective()
    &&& no_empty_sets(s.fact_to_blocked)
    &&& forall|p: TokenId, f: FactId|
        #![trigger neg_fwd(s, p, f)]
        #![trigger in_rel(s.fact_to_blocked, f, p)]
        neg_fwd(s, p, f) <==> in_rel(s.fact_to_blocked, f, p)
}

/// The cross-map protocol invariant: no parent both blocked and unblocked.
pub open spec fn neg_disjoint(s: NegMem) -> bool {
    forall|p: TokenId| #[trigger] s.unblocked.contains_key(p) ==> !s.blocked.contains_key(p)
}

pub open spec fn neg_valid(s: NegMem) -> bool {
    neg_wf(s) && neg_disjoint(s)
}

/// Literal model of negative.rs:249 `validate_consistency` returning `Ok(())`.
pub open spec fn neg_validate_consistency(s: NegMem) -> bool {
    // 250-258
    &&& s.block_order.len() == s.blocked.len()
    &&& forall|p: TokenId| #[trigger] s.blocked.contains_key(p) ==> s.block_order.contains_key(p)
    // 259-264: `unique_orders: HashSet<_>` has one entry per parent
    &&& s.block_order.values().len() == s.block_order.len()
    // 266-280
    &&& forall|p: TokenId| #[trigger]
        s.blocked.contains_key(p) ==> s.blocked[p].len() != 0 && (forall|f: FactId| #[trigger]
            s.blocked[p].contains(f) ==> in_rel(s.fact_to_blocked, f, p))
    // 282-296
    &&& forall|f: FactId| #[trigger]
        s.fact_to_blocked.contains_key(f) ==> !s.fact_to_blocked[f].is_empty() && (forall|
            p: TokenId,
        | #[trigger] s.fact_to_blocked[f].contains(p) ==> neg_fwd(s, p, f))
    // 299-305
    &&& forall|p: TokenId| #[trigger] s.unblocked.contains_key(p) ==> !s.blocked.contains_key(p)
}

pub proof fn lemma_neg_valid_implies_validate(s: NegMem)
    requires
        neg_valid(s),
    ensures
        neg_validate_consistency(s),
{
    s.block_order.lemma_injective_values_len();
    assert forall|p: TokenId| #[trigger] s.blocked.contains_key(p) implies s.blocked[p].len() != 0
        && (forall|f: FactId| #[trigger]
        s.blocked[p].contains(f) ==> in_rel(s.fact_to_blocked, f, p)) by {
        assert forall|f: FactId| #[trigger] s.blocked[p].contains(f) implies in_rel(
            s.fact_to_blocked,
            f,
            p,
        ) by {
            assert(neg_fwd(s, p, f));
        }
    }
    assert forall|f: FactId| #[trigger] s.fact_to_blocked.contains_key(f) implies !s.fact_to_blocked[f].is_empty()
        && (forall|p: TokenId| #[trigger] s.fact_to_blocked[f].contains(p) ==> neg_fwd(s, p, f)) by {
        assert forall|p: TokenId| #[trigger] s.fact_to_blocked[f].contains(p) implies neg_fwd(
            s,
            p,
            f,
        ) by {
            assert(in_rel(s.fact_to_blocked, f, p));
        }
    }
}

pub proof fn lemma_neg_new()
    ensures
        neg_valid(neg_new()),
        neg_is_empty(neg_new()),
{
    let s = neg_new();
    assert(s.block_order.dom() =~= s.blocked.dom());
    assert(s.blocked.dom() =~= Set::empty());
    assert(s.fact_to_blocked.dom() =~= Set::empty());
    assert(s.unblocked.dom() =~= Set::empty());
}

// ------------------------ per-operation lemmas ------------------------
pub proof fn lemma_neg_add_blocker(s: NegMem, p: TokenId, f: FactId, order: u64)
    requires
        neg_wf(s),
        !s.blocked.contains_key(p) ==> fresh_order(s.block_order, order),
    ensures
        ({
            let t = neg_add_blocker(s, p, f, order);
            &&& neg_wf(t)
            &&& forall|q: TokenId, g: FactId| #[trigger]
                neg_fwd(t, q, g) <==> (neg_fwd(s, q, g) || (q == p && g == f))
            &&& t.unblocked == s.unblocked
            &&& t.blocked.dom() == s.blocked.dom().insert(p)
            &&& forall|q: TokenId| #[trigger]
                s.blocked.contains_key(q) && q != p ==> t.blocked[q] == s.blocked[q]
                    && t.block_order[q] == s.block_order[q]
            &&& t.blocked[p] == os_insert(
                if s.blocked.contains_key(p) {
                    s.blocked[p]
                } else {
                    Seq::empty()
                },
                f,
            )
            &&& t.block_order[p] == if s.blocked.contains_key(p) {
                s.block_order[p]
            } else {
                order
            }
        }),
{
    let t = neg_add_blocker(s, p, f, order);
    let old_b = if s.blocked.contains_key(p) {
        s.blocked[p]
    } else {
        Seq::<FactId>::empty()
    };
    lemma_os_insert(old_b, f);
    lemma_link(s.fact_to_blocked, f, p);
    assert(t.blocked.dom() =~= s.blocked.dom().insert(p));
    assert(t.block_order.dom() =~= t.blocked.dom());
    assert forall|q: TokenId, g: FactId| #[trigger]
        neg_fwd(t, q, g) <==> (neg_fwd(s, q, g) || (q == p && g == f)) by {
        if q == p {
            assert(t.blocked[p].contains(g) <==> (old_b.contains(g) || g == f));
        }
    }
    assert forall|q: TokenId, g: FactId|
        #![trigger neg_fwd(t, q, g)]
        #![trigger in_rel(t.fact_to_blocked, g, q)]
        neg_fwd(t, q, g) <==> in_rel(t.fact_to_blocked, g, q) by {
        assert(neg_fwd(s, q, g) <==> in_rel(s.fact_to_blocked, g, q));
    }
    assert forall|x: TokenId, y: TokenId|
        x != y && t.block_order.dom().contains(x) && t.block_order.dom().contains(
            y,
        ) implies #[trigger] t.block_order[x] != #[trigger] t.block_order[y] by {
        if !s.blocked.contains_key(p) && (x == p || y == p) {
            if x == p {
                assert(s.block_order.contains_key(y));
            } else {
                assert(s.block_order.contains_key(x));
            }
        }
    }
}

pub proof fn lemma_neg_remove_blocker(s: NegMem, p: TokenId, f: FactId, o: u64)
    requires
        neg_wf(s),
        neg_primary_blocker(s, p) == Some(f) && s.blocked[p].len() > 1 ==> fresh_order(
            s.block_order,
            o,
        ),
    ensures
        ({
            let (t, r) = neg_remove_blocker(s, p, f, o);
            &&& neg_wf(t)
            &&& forall|q: TokenId, g: FactId| #[trigger]
                neg_fwd(t, q, g) <==> (neg_fwd(s, q, g) && !(q == p && g == f))
            &&& r == (s.blocked.contains_key(p) && !t.blocked.contains_key(p))
            &&& t.unblocked == s.unblocked
            &&& forall|q: TokenId| #[trigger]
                t.blocked.contains_key(q) ==> s.blocked.contains_key(q)
            &&& forall|q: TokenId| #[trigger]
                s.blocked.contains_key(q) && q != p ==> t.blocked.contains_key(q) && t.blocked[q]
                    == s.blocked[q] && t.block_order[q] == s.block_order[q]
            &&& t.blocked.contains_key(p) ==> t.blocked[p] == os_remove(s.blocked[p], f)
                && t.block_order[p] == if s.blocked[p][0] == f {
                o
            } else {
                s.block_order[p]
            }
        }),
{
    let (t, r) = neg_remove_blocker(s, p, f, o);
    lemma_unlink(s.fact_to_blocked, f, p);
    if s.blocked.contains_key(p) {
        lemma_os_remove(s.blocked[p], f);
        let nb = os_remove(s.blocked[p], f);
        if nb.len() == 0 {
            assert(t.blocked.dom() =~= s.blocked.dom().remove(p));
            assert forall|g: FactId| !s.blocked[p].contains(g) || g == f by {
                if s.blocked[p].contains(g) && g != f {
                    assert(nb.contains(g));
                }
            }
        } else {
            assert(t.blocked.dom() =~= s.blocked.dom());
        }
    } else {
        assert(t.blocked == s.blocked);
    }
    assert(t.block_order.dom() =~= t.blocked.dom());
    assert forall|q: TokenId, g: FactId| #[trigger]
        neg_fwd(t, q, g) <==> (neg_fwd(s, q, g) && !(q == p && g == f)) by {
        if q == p && s.blocked.contains_key(p) {
            let nb = os_remove(s.blocked[p], f);
            if nb.len() != 0 {
                assert(t.blocked[p] == nb);
            }
        }
    }
    assert forall|q: TokenId, g: FactId|
        #![trigger neg_fwd(t, q, g)]
        #![trigger in_rel(t.fact_to_blocked, g, q)]
        neg_fwd(t, q, g) <==> in_rel(t.fact_to_blocked, g, q) by {
        assert(neg_fwd(s, q, g) <==> in_rel(s.fact_to_blocked, g, q));
    }
    assert forall|x: TokenId, y: TokenId|
        x != y && t.block_order.dom().contains(x) && t.block_order.dom().contains(
            y,
        ) implies #[trigger] t.block_order[x] != #[trigger] t.block_order[y] by {
        assert(s.block_order.contains_key(x) && s.block_order.contains_key(y));
        if x == p && s.blocked[p][0] == f {
            assert(s.blocked[p].len() > 1);
            assert(s.block_order.contains_key(y));
        } else if y == p && s.blocked[p][0] == f {
            assert(s.blocked[p].len() > 1);
            assert(s.block_order.contains_key(x));
        }
    }
}

pub proof fn lemma_neg_remove_parent_token(s: NegMem, p: TokenId)
    requires
        neg_wf(s),
    ensures
        ({
            let t = neg_remove_parent_token(s, p);
            &&& neg_wf(t)
            &&& neg_disjoint(s) ==> neg_disjoint(t)
            &&& forall|q: TokenId, g: FactId| #[trigger]
                neg_fwd(t, q, g) <==> (neg_fwd(s, q, g) && q != p)
            // all bookkeeping for `p` is gone ...
            &&& !t.blocked.contains_key(p)
            &&& !t.block_order.contains_key(p)
            &&& !t.unblocked.contains_key(p)
            &&& forall|g: FactId| !(#[trigger] in_rel(t.fact_to_blocked, g, p))
            // ... and nothing else changed.
            &&& forall|q: TokenId| #[trigger]
                s.blocked.contains_key(q) && q != p ==> t.blocked.contains_key(q) && t.blocked[q]
                    == s.blocked[q] && t.block_order[q] == s.block_order[q]
            &&& forall|q: TokenId| #[trigger] t.blocked.contains_key(q) ==> s.blocked.contains_key(q)
            &&& t.unblocked == s.unblocked.remove(p)
        }),
{
    let t = neg_remove_parent_token(s, p);
    if s.blocked.contains_key(p) {
        lemma_unlink_all(s.fact_to_blocked, s.blocked[p], p);
        assert forall|q: TokenId, g: FactId|
            #![trigger neg_fwd(t, q, g)]
            #![trigger in_rel(t.fact_to_blocked, g, q)]
            neg_fwd(t, q, g) <==> in_rel(t.fact_to_blocked, g, q) by {
            assert(neg_fwd(s, q, g) <==> in_rel(s.fact_to_blocked, g, q));
            if q == p {
                assert(in_rel(s.fact_to_blocked, g, p) ==> s.blocked[p].contains(g));
            }
        }
        assert forall|g: FactId| !(#[trigger] in_rel(t.fact_to_blocked, g, p)) by {
            assert(in_rel(s.fact_to_blocked, g, p) ==> neg_fwd(s, p, g));
        }
        assert(t.blocked.dom() =~= s.blocked.dom().remove(p));
    } else {
        assert forall|g: FactId| !(#[trigger] in_rel(t.fact_to_blocked, g, p)) by {
            assert(in_rel(s.fact_to_blocked, g, p) ==> neg_fwd(s, p, g));
        }
    }
    assert(t.block_order.dom() =~= t.blocked.dom());
}

pub proof fn lemma_neg_clear(s: NegMem)
    ensures
        neg_valid(neg_clear(s)),
        neg_is_empty(neg_clear(s)),
        neg_clear(s) == neg_new(),
{
    lemma_neg_new();
}

/// Callers pass a dummy order (0) where it cannot be used: rete.rs:1373 adds a
/// blocker to an already blocked parent; rete.rs:1442 removes a nonprimary
/// blocker.  Both are order-independent.
pub proof fn lemma_neg_order_irrelevant(s: NegMem, p: TokenId, f: FactId, o1: u64, o2: u64)
    requires
        neg_wf(s),
    ensures
        s.blocked.contains_key(p) ==> neg_add_blocker(s, p, f, o1) == neg_add_blocker(s, p, f, o2),
        neg_primary_blocker(s, p) != Some(f) ==> neg_remove_blocker(s, p, f, o1)
            == neg_remove_blocker(s, p, f, o2),
{
}

// ------------------- ground truth and refinement ----------------------
/// Abstract ground truth: the live (parent, blocker) pairs, each stamped with
/// the logical time at which it was (first) recorded.
pub struct NegAbs {
    pub pairs: Map<(TokenId, FactId), nat>,
    pub clock: nat,
}

pub open spec fn neg_abs_new() -> NegAbs {
    NegAbs { pairs: Map::empty(), clock: 0 }
}

pub open spec fn neg_abs_add(a: NegAbs, p: TokenId, f: FactId) -> NegAbs {
    if a.pairs.contains_key((p, f)) {
        a
    } else {
        NegAbs { pairs: a.pairs.insert((p, f), a.clock), clock: a.clock + 1 }
    }
}

pub open spec fn neg_abs_remove(a: NegAbs, p: TokenId, f: FactId) -> NegAbs {
    NegAbs { pairs: a.pairs.remove((p, f)), clock: a.clock }
}

pub open spec fn neg_abs_remove_parent(a: NegAbs, p: TokenId) -> NegAbs {
    NegAbs {
        pairs: Map::new(
            a.pairs.dom().filter(|pr: (TokenId, FactId)| pr.0 != p),
            |pr: (TokenId, FactId)| a.pairs[pr],
        ),
        clock: a.clock,
    }
}

pub open spec fn neg_abs_clear(a: NegAbs) -> NegAbs {
    NegAbs { pairs: Map::empty(), clock: a.clock }
}

pub open spec fn neg_stamp(a: NegAbs, p: TokenId) -> spec_fn(FactId) -> nat {
    |g: FactId| a.pairs[(p, g)]
}

/// The concrete memory represents the ground truth: the forward index holds
/// exactly the live pairs, each blocker list is ordered oldest first.
pub open spec fn neg_refines(s: NegMem, a: NegAbs) -> bool {
    &&& forall|p: TokenId, f: FactId|
        #![trigger a.pairs.contains_key((p, f))]
        #![trigger neg_fwd(s, p, f)]
        a.pairs.contains_key((p, f)) <==> neg_fwd(s, p, f)
    &&& forall|pr: (TokenId, FactId)| #[trigger] a.pairs.contains_key(pr) ==> a.pairs[pr] < a.clock
    &&& forall|p: TokenId| #[trigger]
        s.blocked.contains_key(p) ==> sorted_by(s.blocked[p], neg_stamp(a, p))
}

/// The forward index viewed as plain sets.
pub open spec fn neg_fwd_view(s: NegMem) -> Map<TokenId, Set<FactId>> {
    Map::new(s.blocked.dom(), |p: TokenId| s.blocked[p].to_set())
}

/// THEOREM (from-scratch equality): both indexes equal the values computed
/// from scratch from the ground-truth pair set.
pub proof fn lemma_neg_from_scratch(s: NegMem, a: NegAbs)
    requires
        neg_wf(s),
        neg_refines(s, a),
    ensures
        neg_fwd_view(s) == scratch_fwd(a.pairs.dom()),
        s.fact_to_blocked == scratch_rev(a.pairs.dom()),
        forall|p: TokenId| #[trigger]
            s.blocked.contains_key(p) <==> scratch_fwd(a.pairs.dom()).contains_key(p),
{
    let v = neg_fwd_view(s);
    assert forall|p: TokenId| #[trigger] v.contains_key(p) implies !v[p].is_empty() by {
        assert(v[p].contains(s.blocked[p][0]));
    }
    assert forall|p: TokenId, f: FactId| #[trigger]
        a.pairs.dom().contains((p, f)) <==> in_rel(v, p, f) by {
        assert(a.pairs.contains_key((p, f)) <==> neg_fwd(s, p, f));
    }
    lemma_scratch_fwd(v, a.pairs.dom());
    assert forall|p: TokenId, f: FactId| #[trigger]
        a.pairs.dom().contains((p, f)) <==> in_rel(s.fact_to_blocked, f, p) by {
        assert(a.pairs.contains_key((p, f)) <==> neg_fwd(s, p, f));
        assert(neg_fwd(s, p, f) <==> in_rel(s.fact_to_blocked, f, p));
    }
    lemma_scratch_rev(s.fact_to_blocked, a.pairs.dom());
}

/// THEOREM (primary = oldest surviving blocker, negative.rs:149-152).
pub proof fn lemma_neg_primary_is_oldest(s: NegMem, a: NegAbs, p: TokenId)
    requires
        neg_wf(s),
        neg_refines(s, a),
        s.blocked.contains_key(p),
    ensures
        neg_primary_blocker(s, p) == Some(s.blocked[p][0]),
        a.pairs.contains_key((p, s.blocked[p][0])),
        forall|g: FactId|
            a.pairs.contains_key((p, g)) && g != s.blocked[p][0] ==> a.pairs[(p, s.blocked[p][0])]
                < #[trigger] a.pairs[(p, g)],
{
    let b = s.blocked[p];
    assert(neg_fwd(s, p, b[0]));
    assert forall|g: FactId|
        a.pairs.contains_key((p, g)) && g != b[0] implies a.pairs[(p, b[0])] < #[trigger] a.pairs[(
        p,
        g,
    )] by {
        assert(neg_fwd(s, p, g));
        let j = choose|j: int| 0 <= j < b.len() && b[j] == g;
        assert(j != 0);
        assert(sorted_by(b, neg_stamp(a, p)));
        assert(neg_stamp(a, p)(b[0]) < neg_stamp(a, p)(b[j]));
    }
}

pub proof fn lemma_neg_refines_add(s: NegMem, a: NegAbs, p: TokenId, f: FactId, o: u64)
    requires
        neg_wf(s),
        neg_refines(s, a),
        !s.blocked.contains_key(p) ==> fresh_order(s.block_order, o),
    ensures
        neg_refines(neg_add_blocker(s, p, f, o), neg_abs_add(a, p, f)),
{
    let t = neg_add_blocker(s, p, f, o);
    let b = neg_abs_add(a, p, f);
    lemma_neg_add_blocker(s, p, f, o);
    assert forall|q: TokenId, g: FactId|
        #![trigger b.pairs.contains_key((q, g))]
        #![trigger neg_fwd(t, q, g)]
        b.pairs.contains_key((q, g)) <==> neg_fwd(t, q, g) by {
        assert(a.pairs.contains_key((q, g)) <==> neg_fwd(s, q, g));
    }
    assert forall|q: TokenId| #[trigger] t.blocked.contains_key(q) implies sorted_by(
        t.blocked[q],
        neg_stamp(b, q),
    ) by {
        if q != p {
            let sq = s.blocked[q];
            assert(sorted_by(sq, neg_stamp(a, q)));
            assert forall|i: int, j: int| 0 <= i < j < sq.len() implies neg_stamp(b, q)(
                #[trigger] sq[i],
            ) < neg_stamp(b, q)(#[trigger] sq[j]) by {
                assert(neg_stamp(a, q)(sq[i]) < neg_stamp(a, q)(sq[j]));
            }
        } else if a.pairs.contains_key((p, f)) {
            assert(neg_fwd(s, p, f));
            assert(t.blocked[p] == s.blocked[p]);
            assert(sorted_by(s.blocked[p], neg_stamp(a, p)));
        } else {
            let ob = if s.blocked.contains_key(p) {
                s.blocked[p]
            } else {
                Seq::<FactId>::empty()
            };
            lemma_os_insert(ob, f);
            assert(!ob.contains(f)) by {
                if ob.contains(f) {
                    assert(neg_fwd(s, p, f));
                }
            }
            assert forall|i: int, j: int| 0 <= i < j < ob.len() implies neg_stamp(b, p)(
                #[trigger] ob[i],
            ) < neg_stamp(b, p)(#[trigger] ob[j]) by {
                assert(sorted_by(ob, neg_stamp(a, p)));
                assert(neg_stamp(a, p)(ob[i]) < neg_stamp(a, p)(ob[j]));
                assert(ob.contains(ob[i]) && ob.contains(ob[j]));
            }
            assert forall|x: FactId| ob.contains(x) implies neg_stamp(b, p)(x) < neg_stamp(b, p)(
                f,
            ) by {
                assert(neg_fwd(s, p, x));
                assert(a.pairs.contains_key((p, x)));
            }
            assert(sorted_by(ob, neg_stamp(b, p)));
            assert(sorted_by(os_insert(ob, f), neg_stamp(b, p)));
        }
    }
}

pub proof fn lemma_neg_refines_remove(s: NegMem, a: NegAbs, p: TokenId, f: FactId, o: u64)
    requires
        neg_wf(s),
        neg_refines(s, a),
        neg_primary_blocker(s, p) == Some(f) && s.blocked[p].len() > 1 ==> fresh_order(
            s.block_order,
            o,
        ),
    ensures
        neg_refines(neg_remove_blocker(s, p, f, o).0, neg_abs_remove(a, p, f)),
{
    let t = neg_remove_blocker(s, p, f, o).0;
    let b = neg_abs_remove(a, p, f);
    lemma_neg_remove_blocker(s, p, f, o);
    assert forall|q: TokenId, g: FactId|
        #![trigger b.pairs.contains_key((q, g))]
        #![trigger neg_fwd(t, q, g)]
        b.pairs.contains_key((q, g)) <==> neg_fwd(t, q, g) by {
        assert(a.pairs.contains_key((q, g)) <==> neg_fwd(s, q, g));
    }
    assert forall|q: TokenId| #[trigger] t.blocked.contains_key(q) implies sorted_by(
        t.blocked[q],
        neg_stamp(b, q),
    ) by {
        let sq = s.blocked[q];
        assert(sorted_by(sq, neg_stamp(a, q)));
        let tq = t.blocked[q];
        if q == p {
            lemma_os_remove(sq, f);
            assert(sorted_by(tq, neg_stamp(a, q)));
        }
        assert forall|i: int, j: int| 0 <= i < j < tq.len() implies neg_stamp(b, q)(
            #[trigger] tq[i],
        ) < neg_stamp(b, q)(#[trigger] tq[j]) by {
            assert(sorted_by(tq, neg_stamp(a, q)));
            assert(neg_stamp(a, q)(tq[i]) < neg_stamp(a, q)(tq[j]));
            assert(tq.contains(tq[i]) && tq.contains(tq[j]));
            assert(neg_fwd(t, q, tq[i]) && neg_fwd(t, q, tq[j]));
        }
    }
}

pub proof fn lemma_neg_refines_remove_parent(s: NegMem, a: NegAbs, p: TokenId)
    requires
        neg_wf(s),
        neg_refines(s, a),
    ensures
        neg_refines(neg_remove_parent_token(s, p), neg_abs_remove_parent(a, p)),
{
    let t = neg_remove_parent_token(s, p);
    let b = neg_abs_remove_parent(a, p);
    lemma_neg_remove_parent_token(s, p);
    assert forall|q: TokenId, g: FactId|
        #![trigger b.pairs.contains_key((q, g))]
        #![trigger neg_fwd(t, q, g)]
        b.pairs.contains_key((q, g)) <==> neg_fwd(t, q, g) by {
        assert(a.pairs.contains_key((q, g)) <==> neg_fwd(s, q, g));
    }
    assert forall|q: TokenId| #[trigger] t.blocked.contains_key(q) implies sorted_by(
        t.blocked[q],
        neg_stamp(b, q),
    ) by {
        let sq = s.blocked[q];
        assert(sorted_by(sq, neg_stamp(a, q)));
        assert forall|i: int, j: int| 0 <= i < j < sq.len() implies neg_stamp(b, q)(
            #[trigger] sq[i],
        ) < neg_stamp(b, q)(#[trigger] sq[j]) by {
            assert(neg_stamp(a, q)(sq[i]) < neg_stamp(a, q)(sq[j]));
            assert(sq.contains(sq[i]) && sq.contains(sq[j]));
            assert(neg_fwd(s, q, sq[i]) && neg_fwd(s, q, sq[j]));
        }
    }
}

// ------------------- arbitrary operation sequences ---------------------
pub enum NegOp {
    AddBlocker(TokenId, FactId, u64),
    RemoveBlocker(TokenId, FactId, u64),
    SetUnblocked(TokenId, TokenId),
    RemoveUnblocked(TokenId),
    RemoveParentToken(TokenId),
    Clear,
}

pub open spec fn neg_step(s: NegMem, op: NegOp) -> NegMem {
    match op {
        NegOp::AddBlocker(p, f, o) => neg_add_blocker(s, p, f, o),
        NegOp::RemoveBlocker(p, f, o) => neg_remove_blocker(s, p, f, o).0,
        NegOp::SetUnblocked(p, pt) => neg_set_unblocked(s, p, pt),
        NegOp::RemoveUnblocked(p) => neg_remove_unblocked(s, p).0,
        NegOp::RemoveParentToken(p) => neg_remove_parent_token(s, p),
        NegOp::Clear => neg_clear(s),
    }
}

pub open spec fn neg_abs_step(a: NegAbs, op: NegOp) -> NegAbs {
    match op {
        NegOp::AddBlocker(p, f, _) => neg_abs_add(a, p, f),
        NegOp::RemoveBlocker(p, f, _) => neg_abs_remove(a, p, f),
        NegOp::RemoveParentToken(p) => neg_abs_remove_parent(a, p),
        NegOp::Clear => neg_abs_clear(a),
        _ => a,
    }
}

/// Caller obligations, discharged by the rete.rs call sites (see Section 4):
/// a parent gains blockers only after its pass-through entry is removed
/// (rete.rs:1396-1397) or when it has none (rete.rs:1306-1310, 1372-1374);
/// it is marked unblocked only when it has no blockers (rete.rs:1278-1297,
/// 1483-1507); every block-list position that is actually stored comes from
/// `allocate_block_order` (rete.rs:1305, 1394, 1477).
pub open spec fn neg_pre(s: NegMem, op: NegOp) -> bool {
    match op {
        NegOp::AddBlocker(p, f, o) => !s.unblocked.contains_key(p) && (!s.blocked.contains_key(p)
            ==> fresh_order(s.block_order, o)),
        NegOp::RemoveBlocker(p, f, o) => neg_primary_blocker(s, p) == Some(f) && s.blocked[p].len()
            > 1 ==> fresh_order(s.block_order, o),
        NegOp::SetUnblocked(p, _) => !s.blocked.contains_key(p),
        _ => true,
    }
}

pub open spec fn neg_run(ops: Seq<NegOp>) -> NegMem
    decreases ops.len(),
{
    if ops.len() == 0 {
        neg_new()
    } else {
        neg_step(neg_run(ops.drop_last()), ops.last())
    }
}

pub open spec fn neg_abs_run(ops: Seq<NegOp>) -> NegAbs
    decreases ops.len(),
{
    if ops.len() == 0 {
        neg_abs_new()
    } else {
        neg_abs_step(neg_abs_run(ops.drop_last()), ops.last())
    }
}

pub open spec fn neg_admissible(ops: Seq<NegOp>) -> bool
    decreases ops.len(),
{
    ops.len() == 0 || (neg_admissible(ops.drop_last()) && neg_pre(
        neg_run(ops.drop_last()),
        ops.last(),
    ))
}

/// One step preserves validity and refinement; `remove_blocker` reports
/// `now_unblocked` exactly when the parent's ground-truth blocker set goes
/// from non-empty to empty.
pub proof fn lemma_neg_step(s: NegMem, a: NegAbs, op: NegOp)
    requires
        neg_valid(s),
        neg_refines(s, a),
        neg_pre(s, op),
    ensures
        neg_valid(neg_step(s, op)),
        neg_refines(neg_step(s, op), neg_abs_step(a, op)),
        op is RemoveBlocker ==> ({
            let (p, f, o) = (
                op->RemoveBlocker_0,
                op->RemoveBlocker_1,
                op->RemoveBlocker_2,
            );
            neg_remove_blocker(s, p, f, o).1 == (scratch_fwd(a.pairs.dom()).contains_key(p)
                && !scratch_fwd(neg_abs_step(a, op).pairs.dom()).contains_key(p))
        }),
{
    match op {
        NegOp::AddBlocker(p, f, o) => {
            lemma_neg_add_blocker(s, p, f, o);
            lemma_neg_refines_add(s, a, p, f, o);
        },
        NegOp::RemoveBlocker(p, f, o) => {
            lemma_neg_remove_blocker(s, p, f, o);
            lemma_neg_refines_remove(s, a, p, f, o);
            let t = neg_step(s, op);
            lemma_neg_remove_blocker(s, p, f, o);
            lemma_neg_from_scratch(s, a);
            assert(neg_wf(t));
            lemma_neg_from_scratch(t, neg_abs_step(a, op));
        },
        NegOp::SetUnblocked(p, pt) => {
            let t = neg_step(s, op);
            assert forall|q: TokenId, g: FactId| #[trigger]
                neg_fwd(t, q, g) <==> neg_fwd(s, q, g) by {}
        },
        NegOp::RemoveUnblocked(p) => {
            let t = neg_step(s, op);
            assert forall|q: TokenId, g: FactId| #[trigger]
                neg_fwd(t, q, g) <==> neg_fwd(s, q, g) by {}
        },
        NegOp::RemoveParentToken(p) => {
            lemma_neg_remove_parent_token(s, p);
            lemma_neg_refines_remove_parent(s, a, p);
        },
        NegOp::Clear => {
            lemma_neg_clear(s);
        },
    }
}

/// THEOREM (NegativeMemory, arbitrary finite operation sequences): starting
/// from `new()`, after any admissible sequence the memory passes
/// `validate_consistency`, refines the ground truth (oldest-first blocker
/// lists), and both indexes equal their from-scratch values.
pub proof fn theorem_neg_sequences(ops: Seq<NegOp>)
    requires
        neg_admissible(ops),
    ensures
        ({
            let s = neg_run(ops);
            let a = neg_abs_run(ops);
            &&& neg_valid(s)
            &&& neg_validate_consistency(s)
            &&& neg_refines(s, a)
            &&& neg_fwd_view(s) == scratch_fwd(a.pairs.dom())
            &&& s.fact_to_blocked == scratch_rev(a.pairs.dom())
        }),
    decreases ops.len(),
{
    if ops.len() == 0 {
        lemma_neg_new();
    } else {
        theorem_neg_sequences(ops.drop_last());
        lemma_neg_step(neg_run(ops.drop_last()), neg_abs_run(ops.drop_last()), ops.last());
    }
    lemma_neg_valid_implies_validate(neg_run(ops));
    lemma_neg_from_scratch(neg_run(ops), neg_abs_run(ops));
}

// =====================================================================
// Section 2. ExistsMemory (crates/ferric-rules-core/src/exists.rs)
// =====================================================================

/// exists.rs:37 `struct ExistsMemory` (`id` omitted).
pub struct ExMem {
    pub support: Map<TokenId, Set<FactId>>,
    pub satisfied: Map<TokenId, TokenId>,
    pub fact_to_parents: Map<FactId, Set<TokenId>>,
}

/// exists.rs:59 `ExistsMemory::new`
pub open spec fn ex_new() -> ExMem {
    ExMem { support: Map::empty(), satisfied: Map::empty(), fact_to_parents: Map::empty() }
}

/// exists.rs:111 `support_count`: `support.get(p).map_or(0, HashSet::len)`.
pub open spec fn ex_support_count(s: ExMem, p: TokenId) -> nat {
    if s.support.contains_key(p) {
        s.support[p].len()
    } else {
        0
    }
}

/// exists.rs:71 `add_support`; returns `(state, new count)`.
pub open spec fn ex_add_support(s: ExMem, p: TokenId, f: FactId) -> (ExMem, nat) {
    let support = link(s.support, p, f);  // 72-75
    let fact_to_parents = link(s.fact_to_parents, f, p);  // 76-79
    let t = ExMem { support, satisfied: s.satisfied, fact_to_parents };
    (t, ex_support_count(t, p))  // 80

}

/// exists.rs:86 `remove_support`; returns `(state, (new_count, was_removed))`.
pub open spec fn ex_remove_support(s: ExMem, p: TokenId, f: FactId) -> (ExMem, (nat, bool)) {
    // 89-94: was_removed = facts.remove(&f) (HashSet::remove's result)
    let was_removed = s.support.contains_key(p) && s.support[p].contains(f);
    let support = unlink(s.support, p, f);
    let fact_to_parents = unlink(s.fact_to_parents, f, p);  // 96-101
    let t = ExMem { support, satisfied: s.satisfied, fact_to_parents };
    (t, (ex_support_count(t, p), was_removed))  // 103-104

}

/// exists.rs:118 `parents_supported_by` (the Vec holds exactly this set).
pub open spec fn ex_parents_supported_by(s: ExMem, f: FactId) -> Set<TokenId> {
    if s.fact_to_parents.contains_key(f) {
        s.fact_to_parents[f]
    } else {
        Set::empty()
    }
}

/// exists.rs:126 `set_satisfied`
pub open spec fn ex_set_satisfied(s: ExMem, p: TokenId, pt: TokenId) -> ExMem {
    ExMem {
        support: s.support,
        satisfied: s.satisfied.insert(p, pt),
        fact_to_parents: s.fact_to_parents,
    }
}

/// exists.rs:139 `remove_satisfied`
pub open spec fn ex_remove_satisfied(s: ExMem, p: TokenId) -> (ExMem, Option<TokenId>) {
    (
        ExMem {
            support: s.support,
            satisfied: s.satisfied.remove(p),
            fact_to_parents: s.fact_to_parents,
        },
        s.satisfied.get(p),
    )
}

/// exists.rs:150 `remove_parent_token`.  The `for fact_id in facts` loop over a
/// HashSet visits its elements in an unspecified order; `to_seq()` is an
/// arbitrary enumeration and `lemma_unlink_all` holds for every order.
pub open spec fn ex_remove_parent_token(s: ExMem, p: TokenId) -> ExMem {
    let satisfied = s.satisfied.remove(p);  // 152
    if s.support.contains_key(p) {  // 155
        ExMem {
            support: s.support.remove(p),
            satisfied,
            fact_to_parents: unlink_all(s.fact_to_parents, s.support[p].to_seq(), p),  // 156-163
        }
    } else {
        ExMem { support: s.support, satisfied, fact_to_parents: s.fact_to_parents }
    }
}

/// exists.rs:168 `clear`
pub open spec fn ex_clear(s: ExMem) -> ExMem {
    ex_new()
}

/// exists.rs:176 `is_empty`
pub open spec fn ex_is_empty(s: ExMem) -> bool {
    s.support.is_empty() && s.satisfied.is_empty() && s.fact_to_parents.is_empty()
}

// ----------------------------- invariants -----------------------------
pub open spec fn ex_wf(s: ExMem) -> bool {
    &&& no_empty_sets(s.support)
    &&& no_empty_sets(s.fact_to_parents)
    &&& forall|p: TokenId, f: FactId|
        #![trigger in_rel(s.support, p, f)]
        #![trigger in_rel(s.fact_to_parents, f, p)]
        in_rel(s.support, p, f) <==> in_rel(s.fact_to_parents, f, p)
}

/// Check 2 of validate_consistency (satisfied parents are supported); this
/// is the caller's protocol obligation (Section 5).
pub open spec fn ex_sat_supported(s: ExMem) -> bool {
    forall|p: TokenId| #[trigger] s.satisfied.contains_key(p) ==> s.support.contains_key(p)
}

pub open spec fn ex_valid(s: ExMem) -> bool {
    ex_wf(s) && ex_sat_supported(s)
}

/// Literal model of exists.rs:189 `validate_consistency` returning `Ok(())`.
pub open spec fn ex_validate_consistency(s: ExMem) -> bool {
    // 191-205
    &&& forall|p: TokenId| #[trigger]
        s.support.contains_key(p) ==> !s.support[p].is_empty() && (forall|f: FactId| #[trigger]
            s.support[p].contains(f) ==> in_rel(s.fact_to_parents, f, p))
    // 207-221
    &&& forall|f: FactId| #[trigger]
        s.fact_to_parents.contains_key(f) ==> !s.fact_to_parents[f].is_empty() && (forall|
            p: TokenId,
        | #[trigger] s.fact_to_parents[f].contains(p) ==> in_rel(s.support, p, f))
    // 224-230
    &&& forall|p: TokenId| #[trigger] s.satisfied.contains_key(p) ==> ex_support_count(s, p) > 0
}

pub proof fn lemma_ex_valid_implies_validate(s: ExMem)
    requires
        ex_valid(s),
    ensures
        ex_validate_consistency(s),
{
    assert forall|p: TokenId| #[trigger] s.support.contains_key(p) implies !s.support[p].is_empty()
        && (forall|f: FactId| #[trigger]
        s.support[p].contains(f) ==> in_rel(s.fact_to_parents, f, p)) by {
        assert forall|f: FactId| #[trigger] s.support[p].contains(f) implies in_rel(
            s.fact_to_parents,
            f,
            p,
        ) by {
            assert(in_rel(s.support, p, f));
        }
    }
    assert forall|f: FactId| #[trigger] s.fact_to_parents.contains_key(f) implies !s.fact_to_parents[f].is_empty()
        && (forall|p: TokenId| #[trigger] s.fact_to_parents[f].contains(p) ==> in_rel(s.support, p, f)) by {
        assert forall|p: TokenId| #[trigger] s.fact_to_parents[f].contains(p) implies in_rel(
            s.support,
            p,
            f,
        ) by {
            assert(in_rel(s.fact_to_parents, f, p));
        }
    }
    assert forall|p: TokenId| #[trigger] s.satisfied.contains_key(p) implies ex_support_count(s, p)
        > 0 by {
        vstd::set_lib::lemma_set_empty_equivalency_len(s.support[p]);
    }
}

pub proof fn lemma_ex_new()
    ensures
        ex_valid(ex_new()),
        ex_is_empty(ex_new()),
{
    let s = ex_new();
    assert(s.support.dom() =~= Set::empty());
    assert(s.fact_to_parents.dom() =~= Set::empty());
    assert(s.satisfied.dom() =~= Set::empty());
}

/// support_count is the size of the parent's support set, and positive
/// exactly when the parent has support.
pub proof fn lemma_ex_count(s: ExMem, p: TokenId)
    requires
        ex_wf(s),
    ensures
        ex_support_count(s, p) > 0 <==> s.support.contains_key(p),
        ex_support_count(s, p) > 0 <==> exists|f: FactId| in_rel(s.support, p, f),
{
    if s.support.contains_key(p) {
        vstd::set_lib::lemma_set_empty_equivalency_len(s.support[p]);
    }
    lemma_dom_has(s.support, p);
}

pub proof fn lemma_ex_add_support(s: ExMem, p: TokenId, f: FactId)
    requires
        ex_wf(s),
    ensures
        ({
            let (t, c) = ex_add_support(s, p, f);
            &&& ex_wf(t)
            &&& forall|q: TokenId, g: FactId| #[trigger]
                in_rel(t.support, q, g) <==> (in_rel(s.support, q, g) || (q == p && g == f))
            &&& t.satisfied == s.satisfied
            &&& c == ex_support_count(t, p)
            &&& c > 0
            &&& t.support.dom() == s.support.dom().insert(p)
            &&& forall|q: TokenId| #[trigger]
                s.support.contains_key(q) && q != p ==> t.support[q] == s.support[q]
        }),
{
    let (t, c) = ex_add_support(s, p, f);
    lemma_link(s.support, p, f);
    lemma_link(s.fact_to_parents, f, p);
    assert forall|q: TokenId, g: FactId|
        #![trigger in_rel(t.support, q, g)]
        #![trigger in_rel(t.fact_to_parents, g, q)]
        in_rel(t.support, q, g) <==> in_rel(t.fact_to_parents, g, q) by {
        assert(in_rel(s.support, q, g) <==> in_rel(s.fact_to_parents, g, q));
    }
    lemma_ex_count(t, p);
    assert(in_rel(t.support, p, f));
}

pub proof fn lemma_ex_remove_support(s: ExMem, p: TokenId, f: FactId)
    requires
        ex_wf(s),
    ensures
        ({
            let (t, (c, removed)) = ex_remove_support(s, p, f);
            &&& ex_wf(t)
            &&& forall|q: TokenId, g: FactId| #[trigger]
                in_rel(t.support, q, g) <==> (in_rel(s.support, q, g) && !(q == p && g == f))
            &&& t.satisfied == s.satisfied
            &&& removed == in_rel(s.support, p, f)
            &&& c == ex_support_count(t, p)
            &&& forall|q: TokenId| #[trigger] t.support.contains_key(q) ==> s.support.contains_key(q)
            &&& forall|q: TokenId| #[trigger]
                s.support.contains_key(q) && q != p ==> t.support.contains_key(q) && t.support[q]
                    == s.support[q]
        }),
{
    let (t, (c, removed)) = ex_remove_support(s, p, f);
    lemma_unlink(s.support, p, f);
    lemma_unlink(s.fact_to_parents, f, p);
    assert forall|q: TokenId, g: FactId|
        #![trigger in_rel(t.support, q, g)]
        #![trigger in_rel(t.fact_to_parents, g, q)]
        in_rel(t.support, q, g) <==> in_rel(t.fact_to_parents, g, q) by {
        assert(in_rel(s.support, q, g) <==> in_rel(s.fact_to_parents, g, q));
    }
}

pub proof fn lemma_ex_remove_parent_token(s: ExMem, p: TokenId)
    requires
        ex_wf(s),
    ensures
        ({
            let t = ex_remove_parent_token(s, p);
            &&& ex_wf(t)
            &&& ex_sat_supported(s) ==> ex_sat_supported(t)
            &&& forall|q: TokenId, g: FactId| #[trigger]
                in_rel(t.support, q, g) <==> (in_rel(s.support, q, g) && q != p)
            // all bookkeeping for `p` is gone ...
            &&& !t.support.contains_key(p)
            &&& !t.satisfied.contains_key(p)
            &&& forall|g: FactId| !(#[trigger] in_rel(t.fact_to_parents, g, p))
            // ... and nothing else changed.
            &&& forall|q: TokenId| #[trigger]
                s.support.contains_key(q) && q != p ==> t.support.contains_key(q) && t.support[q]
                    == s.support[q]
            &&& forall|q: TokenId| #[trigger] t.support.contains_key(q) ==> s.support.contains_key(q)
            &&& t.satisfied == s.satisfied.remove(p)
        }),
{
    let t = ex_remove_parent_token(s, p);
    if s.support.contains_key(p) {
        let fs = s.support[p].to_seq();
        lemma_unlink_all(s.fact_to_parents, fs, p);
        s.support[p].lemma_to_seq_to_set_id();
        assert(fs.to_set() =~= s.support[p]);
        assert forall|q: TokenId, g: FactId|
            #![trigger in_rel(t.support, q, g)]
            #![trigger in_rel(t.fact_to_parents, g, q)]
            in_rel(t.support, q, g) <==> in_rel(t.fact_to_parents, g, q) by {
            assert(in_rel(s.support, q, g) <==> in_rel(s.fact_to_parents, g, q));
            if q == p {
                assert(in_rel(s.fact_to_parents, g, p) ==> fs.to_set().contains(g));
            }
        }
        assert forall|g: FactId| !(#[trigger] in_rel(t.fact_to_parents, g, p)) by {
            assert(in_rel(s.fact_to_parents, g, p) ==> in_rel(s.support, p, g));
            assert(in_rel(s.support, p, g) ==> fs.to_set().contains(g));
        }
    } else {
        assert forall|g: FactId| !(#[trigger] in_rel(t.fact_to_parents, g, p)) by {
            assert(in_rel(s.fact_to_parents, g, p) ==> in_rel(s.support, p, g));
        }
    }
}

// ------------------- ground truth and refinement ----------------------
/// Abstract ground truth: the live (parent, supporting fact) pairs.
pub open spec fn ex_refines(s: ExMem, pairs: Set<(TokenId, FactId)>) -> bool {
    forall|p: TokenId, f: FactId| #[trigger] pairs.contains((p, f)) <==> in_rel(s.support, p, f)
}

/// The from-scratch support count of a parent.
pub open spec fn scratch_count<A, B>(pairs: Set<(A, B)>, a: A) -> nat {
    if scratch_fwd(pairs).contains_key(a) {
        scratch_fwd(pairs)[a].len()
    } else {
        0
    }
}

/// THEOREM (from-scratch equality): both indexes, and every support count,
/// equal the values computed from scratch from the ground-truth pair set.
pub proof fn lemma_ex_from_scratch(s: ExMem, pairs: Set<(TokenId, FactId)>)
    requires
        ex_wf(s),
        ex_refines(s, pairs),
    ensures
        s.support == scratch_fwd(pairs),
        s.fact_to_parents == scratch_rev(pairs),
        forall|p: TokenId| #[trigger] ex_support_count(s, p) == scratch_count(pairs, p),
{
    lemma_scratch_fwd(s.support, pairs);
    assert forall|p: TokenId, f: FactId| #[trigger]
        pairs.contains((p, f)) <==> in_rel(s.fact_to_parents, f, p) by {
        assert(in_rel(s.support, p, f) <==> in_rel(s.fact_to_parents, f, p));
    }
    lemma_scratch_rev(s.fact_to_parents, pairs);
}

pub enum ExOp {
    AddSupport(TokenId, FactId),
    RemoveSupport(TokenId, FactId),
    SetSatisfied(TokenId, TokenId),
    RemoveSatisfied(TokenId),
    RemoveParentToken(TokenId),
    Clear,
}

pub open spec fn ex_step(s: ExMem, op: ExOp) -> ExMem {
    match op {
        ExOp::AddSupport(p, f) => ex_add_support(s, p, f).0,
        ExOp::RemoveSupport(p, f) => ex_remove_support(s, p, f).0,
        ExOp::SetSatisfied(p, pt) => ex_set_satisfied(s, p, pt),
        ExOp::RemoveSatisfied(p) => ex_remove_satisfied(s, p).0,
        ExOp::RemoveParentToken(p) => ex_remove_parent_token(s, p),
        ExOp::Clear => ex_clear(s),
    }
}

pub open spec fn ex_abs_step(pairs: Set<(TokenId, FactId)>, op: ExOp) -> Set<(TokenId, FactId)> {
    match op {
        ExOp::AddSupport(p, f) => pairs.insert((p, f)),
        ExOp::RemoveSupport(p, f) => pairs.remove((p, f)),
        ExOp::RemoveParentToken(p) => pairs.filter(|pr: (TokenId, FactId)| pr.0 != p),
        ExOp::Clear => Set::empty(),
        _ => pairs,
    }
}

pub open spec fn ex_run(ops: Seq<ExOp>) -> ExMem
    decreases ops.len(),
{
    if ops.len() == 0 {
        ex_new()
    } else {
        ex_step(ex_run(ops.drop_last()), ops.last())
    }
}

pub open spec fn ex_abs_run(ops: Seq<ExOp>) -> Set<(TokenId, FactId)>
    decreases ops.len(),
{
    if ops.len() == 0 {
        Set::empty()
    } else {
        ex_abs_step(ex_abs_run(ops.drop_last()), ops.last())
    }
}

/// One step preserves wf and refinement.  The values returned by
/// `add_support` / `remove_support` are the from-scratch counts, and the
/// caller's transition tests (rete.rs:2217 `old == 0 && new > 0`,
/// rete.rs:2294 `was_removed && new == 0`) hold exactly on the
/// empty <-> non-empty transitions of the ground-truth support set.
pub proof fn lemma_ex_step(s: ExMem, pairs: Set<(TokenId, FactId)>, op: ExOp)
    requires
        ex_wf(s),
        ex_refines(s, pairs),
    ensures
        ex_wf(ex_step(s, op)),
        ex_refines(ex_step(s, op), ex_abs_step(pairs, op)),
        op is AddSupport ==> ({
            let (p, f) = (op->AddSupport_0, op->AddSupport_1);
            let c = ex_add_support(s, p, f).1;
            &&& c == scratch_count(ex_abs_step(pairs, op), p)
            &&& (ex_support_count(s, p) == 0 && c > 0) <==> (!scratch_fwd(pairs).contains_key(p)
                && scratch_fwd(ex_abs_step(pairs, op)).contains_key(p))
        }),
        op is RemoveSupport ==> ({
            let (p, f) = (op->RemoveSupport_0, op->RemoveSupport_1);
            let (c, removed) = ex_remove_support(s, p, f).1;
            &&& c == scratch_count(ex_abs_step(pairs, op), p)
            &&& removed == pairs.contains((p, f))
            &&& (removed && c == 0) <==> (scratch_fwd(pairs).contains_key(p) && !scratch_fwd(
                ex_abs_step(pairs, op),
            ).contains_key(p))
        }),
{
    let t = ex_step(s, op);
    let pairs2 = ex_abs_step(pairs, op);
    match op {
        ExOp::AddSupport(p, f) => {
            lemma_ex_add_support(s, p, f);
        },
        ExOp::RemoveSupport(p, f) => {
            lemma_ex_remove_support(s, p, f);
        },
        ExOp::SetSatisfied(p, pt) => {},
        ExOp::RemoveSatisfied(p) => {},
        ExOp::RemoveParentToken(p) => {
            lemma_ex_remove_parent_token(s, p);
        },
        ExOp::Clear => {
            lemma_ex_new();
        },
    }
    assert(ex_wf(t));
    assert forall|p: TokenId, f: FactId| #[trigger]
        pairs2.contains((p, f)) <==> in_rel(t.support, p, f) by {
        assert(pairs.contains((p, f)) <==> in_rel(s.support, p, f));
    }
    lemma_ex_from_scratch(s, pairs);
    lemma_ex_from_scratch(t, pairs2);
    match op {
        ExOp::AddSupport(p, f) => {
            lemma_ex_count(s, p);
            lemma_ex_count(t, p);
        },
        ExOp::RemoveSupport(p, f) => {
            lemma_ex_count(s, p);
            lemma_ex_count(t, p);
            if s.support.contains_key(p) && !t.support.contains_key(p) {
                assert(in_rel(s.support, p, f)) by {
                    let g = choose|g: FactId| in_rel(s.support, p, g);
                    assert(!in_rel(t.support, p, g));
                }
            }
        },
        _ => {},
    }
}

/// THEOREM (ExistsMemory, arbitrary finite operation sequences): both
/// indexes and all support counts equal their from-scratch values computed
/// from the ground-truth support pairs, and the structural checks of
/// `validate_consistency` hold.  (Check 2, satisfied => supported, depends
/// on the caller protocol and is proved in Section 5.)
pub proof fn theorem_ex_sequences(ops: Seq<ExOp>)
    ensures
        ({
            let s = ex_run(ops);
            let pairs = ex_abs_run(ops);
            &&& ex_wf(s)
            &&& ex_refines(s, pairs)
            &&& s.support == scratch_fwd(pairs)
            &&& s.fact_to_parents == scratch_rev(pairs)
            &&& forall|p: TokenId| #[trigger] ex_support_count(s, p) == scratch_count(pairs, p)
        }),
    decreases ops.len(),
{
    if ops.len() == 0 {
        lemma_ex_new();
    } else {
        theorem_ex_sequences(ops.drop_last());
        lemma_ex_step(ex_run(ops.drop_last()), ex_abs_run(ops.drop_last()), ops.last());
    }
    lemma_ex_from_scratch(ex_run(ops), ex_abs_run(ops));
}

// =====================================================================
// Section 3. NccMemory (crates/ferric-rules-core/src/ncc.rs)
// =====================================================================

/// ncc.rs:39 `struct NccMemory` (`id` omitted).  `result_count` values are
/// `usize` in production; here they are `nat` and the increment is proved
/// to stay within `usize::MAX` (lemma_ncc_add_result).
pub struct NccMem {
    pub result_count: Map<TokenId, nat>,
    pub result_owner: Map<TokenId, TokenId>,
    pub parent_results: Map<TokenId, Seq<TokenId>>,
    pub primary_result: Map<TokenId, TokenId>,
    pub block_order: Map<TokenId, u64>,
    pub unblocked: Map<TokenId, TokenId>,
}

/// ncc.rs:64 `NccMemory::new`
pub open spec fn ncc_new() -> NccMem {
    NccMem {
        result_count: Map::empty(),
        result_owner: Map::empty(),
        parent_results: Map::empty(),
        primary_result: Map::empty(),
        block_order: Map::empty(),
        unblocked: Map::empty(),
    }
}

/// ncc.rs:198 `result_count`
pub open spec fn ncc_result_count(s: NccMem, p: TokenId) -> nat {
    if s.result_count.contains_key(p) {
        s.result_count[p]
    } else {
        0
    }
}

/// ncc.rs:79 `increment_results` (`entry(p).or_insert(0)`; `*count += 1`)
pub open spec fn ncc_increment_results(s: NccMem, p: TokenId) -> (NccMem, nat) {
    let c = ncc_result_count(s, p) + 1;
    (NccMem { result_count: s.result_count.insert(p, c), ..s }, c)
}

/// ncc.rs:91 `add_result`; returns `(state, (old_count, new_count))`.
pub open spec fn ncc_add_result(s: NccMem, p: TokenId, r: TokenId, order: u64) -> (
    NccMem,
    (nat, nat),
) {
    if s.result_owner.contains_key(r) {
        // 97-100: an already tracked result changes nothing
        let current = ncc_result_count(s, s.result_owner[r]);
        (s, (current, current))
    } else {
        let old_count = ncc_result_count(s, p);  // 102
        let (s1, new_count) = ncc_increment_results(s, p);  // 103
        let prev = if s1.parent_results.contains_key(p) {
            s1.parent_results[p]
        } else {
            Seq::<TokenId>::empty()
        };
        let s2 = NccMem {
            result_owner: s1.result_owner.insert(r, p),  // 104
            parent_results: s1.parent_results.insert(p, os_insert(prev, r)),  // 105-108
            ..s1
        };
        let s3 = if old_count == 0 {
            // 109-112
            NccMem {
                primary_result: s2.primary_result.insert(p, r),
                block_order: s2.block_order.insert(p, order),
                ..s2
            }
        } else {
            s2
        };
        (s3, (old_count, new_count))
    }
}

/// ncc.rs:119 `decrement_results`
pub open spec fn ncc_decrement_results(s: NccMem, p: TokenId) -> (NccMem, nat) {
    if s.result_count.contains_key(p) {
        let c = s.result_count[p];
        let new_count: nat = if c > 0 {
            (c - 1) as nat
        } else {
            c
        };  // 121-124
        if new_count == 0 {
            (NccMem { result_count: s.result_count.remove(p), ..s }, new_count)  // 125-127
        } else {
            (NccMem { result_count: s.result_count.insert(p, new_count), ..s }, new_count)
        }
    } else {
        (s, 0)  // 129-131

    }
}

/// ncc.rs:153 `remove_result_support`; returns `(state, Some((parent, new_count)))`.
pub open spec fn ncc_remove_result_support(s: NccMem, r: TokenId) -> (NccMem, Option<
    (TokenId, nat),
>) {
    if !s.result_owner.contains_key(r) {
        (s, None)  // 157 (`?`)

    } else {
        let p = s.result_owner[r];
        let parent_results = if s.parent_results.contains_key(p) {
            // 158-163
            let rs = os_remove(s.parent_results[p], r);
            if rs.len() == 0 {
                s.parent_results.remove(p)
            } else {
                s.parent_results.insert(p, rs)
            }
        } else {
            s.parent_results
        };
        let s1 = NccMem { result_owner: s.result_owner.remove(r), parent_results, ..s };
        let (s2, c) = ncc_decrement_results(s1, p);  // 164
        (s2, Some((p, c)))
    }
}

/// ncc.rs:169 `reassign_primary`; returns `(state, still_blocked)`.
pub open spec fn ncc_reassign_primary(s: NccMem, p: TokenId, order: u64) -> (NccMem, bool) {
    if s.parent_results.contains_key(p) && s.parent_results[p].len() > 0 {
        // 170-178: the oldest surviving result becomes primary
        (
            NccMem {
                primary_result: s.primary_result.insert(p, s.parent_results[p][0]),
                block_order: s.block_order.insert(p, order),
                ..s
            },
            true,
        )
    } else {
        // 180-182
        (
            NccMem {
                primary_result: s.primary_result.remove(p),
                block_order: s.block_order.remove(p),
                ..s
            },
            false,
        )
    }
}

/// ncc.rs:140 `remove_result`
pub open spec fn ncc_remove_result(s: NccMem, r: TokenId, order: u64) -> (NccMem, Option<
    (TokenId, nat),
>) {
    let (s1, removed) = ncc_remove_result_support(s, r);  // 145
    match removed {
        None => (s1, None),
        Some(pc) => if s1.primary_result.get(pc.0) == Some(r) {
            (ncc_reassign_primary(s1, pc.0, order).0, removed)  // 146-148
        } else {
            (s1, removed)
        },
    }
}

/// ncc.rs:206 `set_unblocked`
pub open spec fn ncc_set_unblocked(s: NccMem, p: TokenId, pt: TokenId) -> NccMem {
    NccMem { unblocked: s.unblocked.insert(p, pt), ..s }
}

/// ncc.rs:219 `remove_unblocked`
pub open spec fn ncc_remove_unblocked(s: NccMem, p: TokenId) -> (NccMem, Option<TokenId>) {
    (NccMem { unblocked: s.unblocked.remove(p), ..s }, s.unblocked.get(p))
}

/// ncc.rs:227 `is_blocked`
pub open spec fn ncc_is_blocked(s: NccMem, p: TokenId) -> bool {
    ncc_result_count(s, p) > 0
}

/// The `for result in &results { self.result_owner.remove(result); }` loop.
pub open spec fn remove_all_keys<K, V>(m: Map<K, V>, ks: Seq<K>) -> Map<K, V>
    decreases ks.len(),
{
    if ks.len() == 0 {
        m
    } else {
        remove_all_keys(m, ks.drop_last()).remove(ks.last())
    }
}

/// ncc.rs:232 `remove_parent_token`
pub open spec fn ncc_remove_parent_token(s: NccMem, p: TokenId) -> NccMem {
    let result_owner = if s.parent_results.contains_key(p) {
        remove_all_keys(s.result_owner, s.parent_results[p])  // 234-238
    } else {
        s.result_owner
    };
    NccMem {
        result_count: s.result_count.remove(p),  // 233
        result_owner,
        parent_results: s.parent_results.remove(p),  // 234
        primary_result: s.primary_result.remove(p),  // 239
        block_order: s.block_order.remove(p),  // 240
        unblocked: s.unblocked.remove(p),  // 241
    }
}

/// ncc.rs:245 `clear`
pub open spec fn ncc_clear(s: NccMem) -> NccMem {
    ncc_new()
}

/// ncc.rs:256 `is_empty`
pub open spec fn ncc_is_empty(s: NccMem) -> bool {
    s.result_count.is_empty() && s.result_owner.is_empty() && s.unblocked.is_empty()
        && s.parent_results.is_empty() && s.primary_result.is_empty() && s.block_order.is_empty()
}

pub proof fn lemma_remove_all_keys<K, V>(m: Map<K, V>, ks: Seq<K>)
    ensures
        forall|k: K| #[trigger]
            remove_all_keys(m, ks).contains_key(k) <==> (m.contains_key(k) && !ks.contains(k)),
        forall|k: K| #[trigger]
            remove_all_keys(m, ks).contains_key(k) ==> remove_all_keys(m, ks)[k] == m[k],
    decreases ks.len(),
{
    if ks.len() == 0 {
        assert(remove_all_keys(m, ks) == m);
    } else {
        let prev = remove_all_keys(m, ks.drop_last());
        lemma_remove_all_keys(m, ks.drop_last());
        assert(remove_all_keys(m, ks) == prev.remove(ks.last()));
        assert forall|k: K| ks.contains(k) <==> (ks.drop_last().contains(k) || k == ks.last()) by {
            if ks.contains(k) {
                let i = choose|i: int| 0 <= i < ks.len() && ks[i] == k;
                if i < ks.len() - 1 {
                    assert(ks.drop_last()[i] == k);
                }
            }
            if ks.drop_last().contains(k) {
                let i = choose|i: int| 0 <= i < ks.drop_last().len() && ks.drop_last()[i] == k;
                assert(ks[i] == k);
            }
            if k == ks.last() {
                assert(ks[ks.len() - 1] == k);
            }
        }
    }
}

// ----------------------------- invariants -----------------------------
pub open spec fn ncc_fwd(s: NccMem, p: TokenId, r: TokenId) -> bool {
    s.parent_results.contains_key(p) && s.parent_results[p].contains(r)
}

pub open spec fn ncc_owns(s: NccMem, p: TokenId, r: TokenId) -> bool {
    s.result_owner.contains_key(r) && s.result_owner[r] == p
}

/// Holds in every state, including in the middle of the two-phase cascade
/// (rete.rs:1947-2037) when a removed primary is still recorded.
pub open spec fn ncc_core(s: NccMem) -> bool {
    &&& s.result_count.dom() == s.parent_results.dom()
    &&& s.block_order.dom() == s.primary_result.dom()
    &&& forall|p: TokenId| #[trigger]
        s.result_count.contains_key(p) ==> {
            &&& s.result_count[p] > 0
            &&& s.parent_results[p].no_duplicates()
            &&& s.parent_results[p].len() == s.result_count[p]
            &&& s.primary_result.contains_key(p)
        }
    &&& forall|p: TokenId, r: TokenId|
        #![trigger ncc_fwd(s, p, r)]
        #![trigger ncc_owns(s, p, r)]
        ncc_fwd(s, p, r) <==> ncc_owns(s, p, r)
    &&& s.block_order.is_injective()
}

/// `p`'s primary bookkeeping is settled: recorded only while `p` has
/// results, and then it is the oldest of them.
pub open spec fn ncc_settled(s: NccMem, p: TokenId) -> bool {
    s.primary_result.contains_key(p) ==> s.result_count.contains_key(p) && s.primary_result[p]
        == s.parent_results[p][0]
}

pub open spec fn ncc_wf(s: NccMem) -> bool {
    ncc_core(s) && forall|p: TokenId| #[trigger] ncc_settled(s, p)
}

pub open spec fn ncc_disjoint(s: NccMem) -> bool {
    forall|p: TokenId| #[trigger] s.unblocked.contains_key(p) ==> !s.result_count.contains_key(p)
}

pub open spec fn ncc_valid(s: NccMem) -> bool {
    ncc_wf(s) && ncc_disjoint(s)
}

/// ncc.rs:335-344 `per_parent_results[p]`: tracked results owned by `p`.
pub open spec fn ncc_tracked(s: NccMem, p: TokenId) -> nat {
    ncc_owned_set(s, p).len()
}

pub open spec fn ncc_owned_set(s: NccMem, p: TokenId) -> Set<TokenId> {
    s.result_owner.dom().filter(|r: TokenId| s.result_owner[r] == p)
}

/// Literal model of ncc.rs:274 `validate_consistency` returning `Ok(())`.
pub open spec fn ncc_validate_consistency(s: NccMem) -> bool {
    &&& s.result_count.len() == s.parent_results.len()  // 275-279
    &&& s.result_count.len() == s.primary_result.len()  // 280-284
    &&& s.result_count.len() == s.block_order.len()  // 285-289
    &&& forall|p: TokenId| #[trigger]
        s.result_count.contains_key(p) ==> {
            // 290-315
            &&& s.parent_results.contains_key(p)
            &&& s.parent_results[p].len() == s.result_count[p]
            &&& s.primary_result.contains_key(p)
            &&& s.parent_results[p].contains(s.primary_result[p])
            &&& os_first(s.parent_results[p]) == Some(s.primary_result[p])
            &&& s.block_order.contains_key(p)
            &&& forall|r: TokenId| #[trigger]
                s.parent_results[p].contains(r) ==> s.result_owner.get(r) == Some(p)
        }
    &&& forall|p: TokenId| #[trigger]
        s.unblocked.contains_key(p) ==> !s.result_count.contains_key(p)  // 317-323
    &&& forall|p: TokenId| #[trigger]
        s.result_count.contains_key(p) ==> s.result_count[p] > 0  // 326-332
    &&& forall|r: TokenId| #[trigger]
        s.result_owner.contains_key(r) ==> s.result_count.contains_key(s.result_owner[r])  // 336-344
    &&& forall|p: TokenId| #[trigger]
        s.result_count.contains_key(p) ==> ncc_tracked(s, p) == s.result_count[p]  // 347-354

}

/// The results owned by `p`, as a set, are exactly `parent_results[p]`.
pub proof fn lemma_ncc_tracked(s: NccMem, p: TokenId)
    requires
        ncc_core(s),
        s.result_count.contains_key(p),
    ensures
        ncc_owned_set(s, p) == s.parent_results[p].to_set(),
        ncc_tracked(s, p) == s.result_count[p],
{
    let fs = ncc_owned_set(s, p);
    assert forall|r: TokenId| fs.contains(r) <==> s.parent_results[p].to_set().contains(r) by {
        assert(ncc_fwd(s, p, r) <==> ncc_owns(s, p, r));
    }
    assert(fs =~= s.parent_results[p].to_set());
    s.parent_results[p].unique_seq_to_set();
}

pub proof fn lemma_ncc_valid_implies_validate(s: NccMem)
    requires
        ncc_valid(s),
    ensures
        ncc_validate_consistency(s),
{
    assert(s.primary_result.dom() =~= s.result_count.dom()) by {
        assert forall|p: TokenId| s.primary_result.contains_key(p) implies s.result_count.contains_key(
            p,
        ) by {
            assert(ncc_settled(s, p));
        }
    }
    assert forall|p: TokenId| #[trigger] s.result_count.contains_key(p) implies {
        &&& s.parent_results.contains_key(p)
        &&& s.parent_results[p].len() == s.result_count[p]
        &&& s.primary_result.contains_key(p)
        &&& s.parent_results[p].contains(s.primary_result[p])
        &&& os_first(s.parent_results[p]) == Some(s.primary_result[p])
        &&& s.block_order.contains_key(p)
        &&& forall|r: TokenId| #[trigger]
            s.parent_results[p].contains(r) ==> s.result_owner.get(r) == Some(p)
    } by {
        assert(ncc_settled(s, p));
        assert(s.parent_results[p].contains(s.parent_results[p][0]));
        assert forall|r: TokenId| #[trigger] s.parent_results[p].contains(r) implies s.result_owner.get(
            r,
        ) == Some(p) by {
            assert(ncc_fwd(s, p, r));
        }
    }
    assert forall|r: TokenId| #[trigger] s.result_owner.contains_key(r) implies s.result_count.contains_key(
        s.result_owner[r],
    ) by {
        assert(ncc_owns(s, s.result_owner[r], r));
    }
    assert forall|p: TokenId| #[trigger] s.result_count.contains_key(p) implies ncc_tracked(s, p)
        == s.result_count[p] by {
        lemma_ncc_tracked(s, p);
    }
}

pub proof fn lemma_ncc_new()
    ensures
        ncc_valid(ncc_new()),
        ncc_is_empty(ncc_new()),
{
    let s = ncc_new();
    assert(s.result_count.dom() =~= Set::empty());
    assert(s.parent_results.dom() =~= Set::empty());
    assert(s.primary_result.dom() =~= Set::empty());
    assert(s.block_order.dom() =~= Set::empty());
    assert(s.result_owner.dom() =~= Set::empty());
    assert(s.unblocked.dom() =~= Set::empty());
}

/// Every count is bounded by the number of tracked result tokens.
pub proof fn lemma_ncc_count_bound(s: NccMem, p: TokenId)
    requires
        ncc_core(s),
    ensures
        ncc_result_count(s, p) <= s.result_owner.len(),
{
    if s.result_count.contains_key(p) {
        lemma_ncc_tracked(s, p);
        let fs = ncc_owned_set(s, p);
        assert(fs.subset_of(s.result_owner.dom()));
        vstd::set_lib::lemma_len_subset(fs, s.result_owner.dom());
    }
}

// ------------------------ per-operation lemmas ------------------------
pub proof fn lemma_ncc_add_result(s: NccMem, p: TokenId, r: TokenId, o: u64)
    requires
        ncc_wf(s),
        !s.result_owner.contains_key(r) && ncc_result_count(s, p) == 0 ==> fresh_order(
            s.block_order,
            o,
        ),
    ensures
        ({
            let (t, (old_c, new_c)) = ncc_add_result(s, p, r, o);
            let fresh = !s.result_owner.contains_key(r);
            &&& ncc_wf(t)
            &&& ncc_disjoint(s) && (fresh ==> !s.unblocked.contains_key(p)) ==> ncc_disjoint(t)
            &&& forall|q: TokenId, x: TokenId| #[trigger]
                ncc_fwd(t, q, x) <==> (ncc_fwd(s, q, x) || (fresh && q == p && x == r))
            &&& t.unblocked == s.unblocked
            &&& !fresh ==> t == s && old_c == new_c
            // the increment cannot overflow: old count <= tracked results
            &&& fresh ==> old_c == ncc_result_count(s, p) && new_c == old_c + 1 && old_c
                <= s.result_owner.len() && t.result_owner == s.result_owner.insert(r, p)
            &&& fresh ==> t.parent_results[p] == os_insert(
                if s.parent_results.contains_key(p) {
                    s.parent_results[p]
                } else {
                    Seq::empty()
                },
                r,
            )
            &&& forall|q: TokenId| #[trigger]
                s.result_count.contains_key(q) && q != p ==> t.result_count.contains_key(q)
                    && t.parent_results[q] == s.parent_results[q] && t.primary_result[q]
                    == s.primary_result[q] && t.block_order[q] == s.block_order[q]
            &&& forall|q: TokenId| #[trigger]
                t.result_count.contains_key(q) ==> s.result_count.contains_key(q) || q == p
            &&& fresh && ncc_result_count(s, p) == 0 ==> t.block_order[p] == o
            &&& s.result_count.contains_key(p) ==> t.block_order[p] == s.block_order[p]
                && t.primary_result[p] == s.primary_result[p]
        }),
{
    let (t, (old_c, new_c)) = ncc_add_result(s, p, r, o);
    if !s.result_owner.contains_key(r) {
        lemma_ncc_count_bound(s, p);
        let prev = if s.parent_results.contains_key(p) {
            s.parent_results[p]
        } else {
            Seq::<TokenId>::empty()
        };
        lemma_os_insert(prev, r);
        assert(!prev.contains(r)) by {
            if prev.contains(r) {
                assert(ncc_fwd(s, p, r));
            }
        }
        assert(t.result_count.dom() =~= s.result_count.dom().insert(p));
        assert(t.parent_results.dom() =~= s.parent_results.dom().insert(p));
        if s.result_count.contains_key(p) {
            assert(t.primary_result == s.primary_result);
            assert(t.block_order == s.block_order);
        } else {
            assert(t.primary_result.dom() =~= s.primary_result.dom().insert(p));
            assert(t.block_order.dom() =~= s.block_order.dom().insert(p));
            assert(!s.primary_result.contains_key(p)) by {
                if s.primary_result.contains_key(p) {
                    assert(ncc_settled(s, p));
                }
            }
            assert(prev.len() == 0);
        }
        assert forall|q: TokenId, x: TokenId| #[trigger]
            ncc_fwd(t, q, x) <==> (ncc_fwd(s, q, x) || (q == p && x == r)) by {
            if q == p {
                assert(t.parent_results[p].contains(x) <==> (prev.contains(x) || x == r));
            }
        }
        assert forall|q: TokenId, x: TokenId|
            #![trigger ncc_fwd(t, q, x)]
            #![trigger ncc_owns(t, q, x)]
            ncc_fwd(t, q, x) <==> ncc_owns(t, q, x) by {
            assert(ncc_fwd(s, q, x) <==> ncc_owns(s, q, x));
        }
        assert forall|q: TokenId| #[trigger] ncc_settled(t, q) by {
            assert(ncc_settled(s, q));
        }
        assert forall|x: TokenId, y: TokenId|
            x != y && t.block_order.dom().contains(x) && t.block_order.dom().contains(
                y,
            ) implies #[trigger] t.block_order[x] != #[trigger] t.block_order[y] by {
            if !s.result_count.contains_key(p) && (x == p || y == p) {
                if x == p {
                    assert(s.block_order.contains_key(y));
                } else {
                    assert(s.block_order.contains_key(x));
                }
            }
        }
    }
}

pub proof fn lemma_ncc_remove_result_support(s: NccMem, r: TokenId)
    requires
        ncc_core(s),
    ensures
        ({
            let (t, res) = ncc_remove_result_support(s, r);
            let p = s.result_owner[r];
            &&& ncc_core(t)
            &&& forall|q: TokenId, x: TokenId| #[trigger]
                ncc_fwd(t, q, x) <==> (ncc_fwd(s, q, x) && x != r)
            &&& t.primary_result == s.primary_result
            &&& t.block_order == s.block_order
            &&& t.unblocked == s.unblocked
            &&& t.result_owner == s.result_owner.remove(r)
            &&& res == if s.result_owner.contains_key(r) {
                Some((p, ncc_result_count(t, p)))
            } else {
                None
            }
            // `decrement_results` always takes its `*count > 0` branch
            &&& s.result_owner.contains_key(r) ==> s.result_count.contains_key(p)
                && s.result_count[p] > 0
            // only a removed primary unsettles its parent
            &&& forall|q: TokenId| #[trigger]
                ncc_settled(s, q) && !(s.result_owner.contains_key(r) && q == p
                    && s.primary_result.contains_key(q) && s.primary_result[q] == r) ==> ncc_settled(
                    t,
                    q,
                )
            &&& forall|q: TokenId| #[trigger]
                t.result_count.contains_key(q) ==> s.result_count.contains_key(q)
            &&& forall|q: TokenId| #[trigger]
                s.result_count.contains_key(q) && !(s.result_owner.contains_key(r) && q == p)
                    ==> t.result_count.contains_key(q) && t.parent_results[q]
                    == s.parent_results[q]
            &&& s.result_owner.contains_key(r) && t.parent_results.contains_key(p)
                ==> t.parent_results[p] == os_remove(s.parent_results[p], r)
        }),
{
    let (t, res) = ncc_remove_result_support(s, r);
    if s.result_owner.contains_key(r) {
        let p = s.result_owner[r];
        assert(ncc_owns(s, p, r));
        assert(ncc_fwd(s, p, r));
        let ps = s.parent_results[p];
        lemma_os_remove(ps, r);
        let nps = os_remove(ps, r);
        if nps.len() == 0 {
            assert(t.result_count.dom() =~= s.result_count.dom().remove(p));
            assert(t.parent_results.dom() =~= s.parent_results.dom().remove(p));
        } else {
            assert(t.result_count.dom() =~= s.result_count.dom());
            assert(t.parent_results.dom() =~= s.parent_results.dom());
        }
        assert forall|q: TokenId, x: TokenId| #[trigger]
            ncc_fwd(t, q, x) <==> (ncc_fwd(s, q, x) && x != r) by {
            if q == p {
                if ps.contains(x) && x != r {
                    assert(nps.contains(x));
                }
            }
        }
        assert forall|q: TokenId, x: TokenId|
            #![trigger ncc_fwd(t, q, x)]
            #![trigger ncc_owns(t, q, x)]
            ncc_fwd(t, q, x) <==> ncc_owns(t, q, x) by {
            assert(ncc_fwd(s, q, x) <==> ncc_owns(s, q, x));
        }
        assert forall|q: TokenId| #[trigger]
            ncc_settled(s, q) && !(q == p && s.primary_result.contains_key(q)
                && s.primary_result[q] == r) implies ncc_settled(t, q) by {
            if q == p && s.primary_result.contains_key(q) {
                assert(ps[0] != r);
                assert(nps.len() > 0);
            }
        }
    }
}

pub proof fn lemma_ncc_reassign_primary(s: NccMem, p: TokenId, o: u64)
    requires
        ncc_core(s),
        s.parent_results.contains_key(p) ==> fresh_order(s.block_order, o),
    ensures
        ({
            let (t, still) = ncc_reassign_primary(s, p, o);
            &&& ncc_core(t)
            &&& ncc_settled(t, p)
            &&& forall|q: TokenId| q != p ==> (#[trigger] ncc_settled(t, q) == ncc_settled(s, q))
            &&& still == s.parent_results.contains_key(p)
            &&& t.result_count == s.result_count
            &&& t.result_owner == s.result_owner
            &&& t.parent_results == s.parent_results
            &&& t.unblocked == s.unblocked
            &&& still ==> t.block_order[p] == o
            &&& forall|q: TokenId| #[trigger]
                s.block_order.contains_key(q) && q != p ==> t.block_order.contains_key(q)
                    && t.block_order[q] == s.block_order[q]
            &&& forall|q: TokenId| #[trigger]
                t.block_order.contains_key(q) ==> s.block_order.contains_key(q) || q == p
        }),
{
    let (t, still) = ncc_reassign_primary(s, p, o);
    if s.parent_results.contains_key(p) {
        assert(s.result_count.contains_key(p));
        assert(t.primary_result.dom() =~= s.primary_result.dom());
        assert(t.block_order.dom() =~= s.block_order.dom());
    } else {
        assert(t.primary_result.dom() =~= s.primary_result.dom().remove(p));
        assert(t.block_order.dom() =~= s.block_order.dom().remove(p));
    }
    assert forall|q: TokenId, x: TokenId|
        #![trigger ncc_fwd(t, q, x)]
        #![trigger ncc_owns(t, q, x)]
        ncc_fwd(t, q, x) <==> ncc_owns(t, q, x) by {
        assert(ncc_fwd(s, q, x) <==> ncc_owns(s, q, x));
    }
}

pub proof fn lemma_ncc_remove_parent_token(s: NccMem, p: TokenId)
    requires
        ncc_core(s),
    ensures
        ({
            let t = ncc_remove_parent_token(s, p);
            &&& ncc_core(t)
            &&& ncc_settled(t, p)
            &&& forall|q: TokenId| q != p ==> (#[trigger] ncc_settled(t, q) == ncc_settled(s, q))
            &&& ncc_disjoint(s) ==> ncc_disjoint(t)
            &&& forall|q: TokenId, x: TokenId| #[trigger]
                ncc_fwd(t, q, x) <==> (ncc_fwd(s, q, x) && q != p)
            // all bookkeeping for `p` is gone ...
            &&& !t.result_count.contains_key(p)
            &&& !t.parent_results.contains_key(p)
            &&& !t.primary_result.contains_key(p)
            &&& !t.block_order.contains_key(p)
            &&& !t.unblocked.contains_key(p)
            &&& forall|x: TokenId| !(#[trigger] ncc_owns(t, p, x))
            // ... and nothing else changed.
            &&& forall|q: TokenId| #[trigger]
                s.result_count.contains_key(q) && q != p ==> t.result_count.contains_key(q)
                    && t.result_count[q] == s.result_count[q] && t.parent_results[q]
                    == s.parent_results[q]
            &&& forall|q: TokenId| #[trigger]
                s.primary_result.contains_key(q) && q != p ==> t.primary_result.contains_key(q)
                    && t.primary_result[q] == s.primary_result[q] && t.block_order[q]
                    == s.block_order[q]
            &&& forall|q: TokenId| #[trigger]
                t.result_count.contains_key(q) ==> s.result_count.contains_key(q)
            &&& forall|q: TokenId| #[trigger]
                t.block_order.contains_key(q) ==> s.block_order.contains_key(q)
            &&& t.unblocked == s.unblocked.remove(p)
            &&& forall|x: TokenId| #[trigger]
                t.result_owner.contains_key(x) <==> (s.result_owner.contains_key(x)
                    && s.result_owner[x] != p)
            &&& forall|x: TokenId| #[trigger]
                t.result_owner.contains_key(x) ==> t.result_owner[x] == s.result_owner[x]
        }),
{
    let t = ncc_remove_parent_token(s, p);
    if s.parent_results.contains_key(p) {
        lemma_remove_all_keys(s.result_owner, s.parent_results[p]);
        assert forall|x: TokenId| #[trigger]
            t.result_owner.contains_key(x) <==> (s.result_owner.contains_key(x)
                && s.result_owner[x] != p) by {
            if s.result_owner.contains_key(x) {
                assert(ncc_owns(s, s.result_owner[x], x));
            }
            if s.parent_results[p].contains(x) {
                assert(ncc_fwd(s, p, x));
            }
        }
    } else {
        assert forall|x: TokenId| #[trigger]
            t.result_owner.contains_key(x) <==> (s.result_owner.contains_key(x)
                && s.result_owner[x] != p) by {
            if s.result_owner.contains_key(x) && s.result_owner[x] == p {
                assert(ncc_owns(s, p, x));
            }
        }
    }
    assert(t.result_count.dom() =~= s.result_count.dom().remove(p));
    assert(t.parent_results.dom() =~= s.parent_results.dom().remove(p));
    assert(t.primary_result.dom() =~= s.primary_result.dom().remove(p));
    assert(t.block_order.dom() =~= s.block_order.dom().remove(p));
    assert forall|q: TokenId, x: TokenId|
        #![trigger ncc_fwd(t, q, x)]
        #![trigger ncc_owns(t, q, x)]
        ncc_fwd(t, q, x) <==> ncc_owns(t, q, x) by {
        assert(ncc_fwd(s, q, x) <==> ncc_owns(s, q, x));
        assert(ncc_fwd(t, q, x) <==> (ncc_fwd(s, q, x) && q != p));
        assert(t.result_owner.contains_key(x) <==> (s.result_owner.contains_key(x)
            && s.result_owner[x] != p));
        if t.result_owner.contains_key(x) {
            if s.parent_results.contains_key(p) {
                assert(t.result_owner[x] == s.result_owner[x]);
            }
        }
        assert(ncc_owns(t, q, x) <==> (ncc_owns(s, q, x) && q != p));
    }
}

/// The production composite `remove_result` keeps the memory settled.
pub proof fn lemma_ncc_remove_result(s: NccMem, r: TokenId, o: u64)
    requires
        ncc_wf(s),
        s.result_owner.contains_key(r) && s.primary_result.get(s.result_owner[r]) == Some(r)
            && s.result_count[s.result_owner[r]] > 1 ==> fresh_order(s.block_order, o),
    ensures
        ({
            let (t, res) = ncc_remove_result(s, r, o);
            &&& ncc_wf(t)
            &&& ncc_disjoint(s) ==> ncc_disjoint(t)
            &&& forall|q: TokenId, x: TokenId| #[trigger]
                ncc_fwd(t, q, x) <==> (ncc_fwd(s, q, x) && x != r)
            &&& t.unblocked == s.unblocked
            &&& t.result_owner == s.result_owner.remove(r)
            &&& res == if s.result_owner.contains_key(r) {
                Some((s.result_owner[r], ncc_result_count(t, s.result_owner[r])))
            } else {
                None
            }
        }),
{
    let (s1, res) = ncc_remove_result_support(s, r);
    lemma_ncc_remove_result_support(s, r);
    if s.result_owner.contains_key(r) {
        let p = s.result_owner[r];
        assert(ncc_settled(s, p));
        if s1.primary_result.get(p) == Some(r) {
            if s1.parent_results.contains_key(p) {
                assert(s.result_count[p] > 1) by {
                    assert(s1.result_count.contains_key(p));
                    assert(s1.result_count[p] == s.result_count[p] - 1);
                }
            }
            lemma_ncc_reassign_primary(s1, p, o);
            let t = ncc_reassign_primary(s1, p, o).0;
            assert forall|q: TokenId| #[trigger] ncc_settled(t, q) by {
                if q != p {
                    assert(ncc_settled(s, q));
                }
            }
        } else {
            assert forall|q: TokenId| #[trigger] ncc_settled(s1, q) by {
                assert(ncc_settled(s, q));
            }
        }
    } else {
        assert(s1 == s);
    }
}

// ------------------- ground truth and refinement ----------------------
/// Abstract ground truth: each live subnetwork result with its owning parent
/// and the logical time at which it was recorded.
pub struct NccAbs {
    pub owner: Map<TokenId, (TokenId, nat)>,
    pub clock: nat,
}

pub open spec fn ncc_abs_new() -> NccAbs {
    NccAbs { owner: Map::empty(), clock: 0 }
}

pub open spec fn ncc_abs_add(a: NccAbs, p: TokenId, r: TokenId) -> NccAbs {
    if a.owner.contains_key(r) {
        a
    } else {
        NccAbs { owner: a.owner.insert(r, (p, a.clock)), clock: a.clock + 1 }
    }
}

pub open spec fn ncc_abs_remove(a: NccAbs, r: TokenId) -> NccAbs {
    NccAbs { owner: a.owner.remove(r), clock: a.clock }
}

pub open spec fn ncc_abs_remove_parent(a: NccAbs, p: TokenId) -> NccAbs {
    NccAbs {
        owner: Map::new(
            a.owner.dom().filter(|r: TokenId| a.owner[r].0 != p),
            |r: TokenId| a.owner[r],
        ),
        clock: a.clock,
    }
}

pub open spec fn ncc_abs_clear(a: NccAbs) -> NccAbs {
    NccAbs { owner: Map::empty(), clock: a.clock }
}

/// The ground-truth relation as (parent, result) pairs.
pub open spec fn ncc_pairs(a: NccAbs) -> Set<(TokenId, TokenId)> {
    a.owner.dom().map_by(|r: TokenId| (a.owner[r].0, r), |pr: (TokenId, TokenId)| pr.1)
}

pub open spec fn ncc_stamp(a: NccAbs) -> spec_fn(TokenId) -> nat {
    |r: TokenId| a.owner[r].1
}

pub open spec fn ncc_refines(s: NccMem, a: NccAbs) -> bool {
    &&& s.result_owner.dom() == a.owner.dom()
    &&& forall|r: TokenId| #[trigger]
        s.result_owner.contains_key(r) ==> s.result_owner[r] == a.owner[r].0
    &&& forall|r: TokenId| #[trigger] a.owner.contains_key(r) ==> a.owner[r].1 < a.clock
    &&& forall|p: TokenId| #[trigger]
        s.parent_results.contains_key(p) ==> sorted_by(s.parent_results[p], ncc_stamp(a))
}

pub open spec fn ncc_fwd_view(s: NccMem) -> Map<TokenId, Set<TokenId>> {
    Map::new(s.parent_results.dom(), |p: TokenId| s.parent_results[p].to_set())
}

pub proof fn lemma_ncc_pairs(a: NccAbs, p: TokenId, r: TokenId)
    ensures
        ncc_pairs(a).contains((p, r)) <==> (a.owner.contains_key(r) && a.owner[r].0 == p),
{
}

/// THEOREM (from-scratch equality): the per-parent result sets and every
/// result count equal the values computed from the ground truth.
pub proof fn lemma_ncc_from_scratch(s: NccMem, a: NccAbs)
    requires
        ncc_core(s),
        ncc_refines(s, a),
    ensures
        ncc_fwd_view(s) == scratch_fwd(ncc_pairs(a)),
        forall|p: TokenId| #[trigger]
            s.result_count.contains_key(p) <==> scratch_fwd(ncc_pairs(a)).contains_key(p),
        forall|p: TokenId| #[trigger] ncc_result_count(s, p) == scratch_count(ncc_pairs(a), p),
{
    let v = ncc_fwd_view(s);
    let pairs = ncc_pairs(a);
    assert forall|p: TokenId| #[trigger] v.contains_key(p) implies !v[p].is_empty() by {
        assert(v[p].contains(s.parent_results[p][0]));
    }
    assert forall|p: TokenId, r: TokenId| #[trigger] pairs.contains((p, r)) <==> in_rel(v, p, r) by {
        lemma_ncc_pairs(a, p, r);
        assert(ncc_fwd(s, p, r) <==> ncc_owns(s, p, r));
    }
    lemma_scratch_fwd(v, pairs);
    assert forall|p: TokenId| #[trigger]
        ncc_result_count(s, p) == scratch_count(ncc_pairs(a), p) by {
        if s.result_count.contains_key(p) {
            s.parent_results[p].unique_seq_to_set();
        }
    }
}

/// THEOREM (primary = oldest surviving result, ncc.rs:47-52, 168).
pub proof fn lemma_ncc_primary_is_oldest(s: NccMem, a: NccAbs, p: TokenId)
    requires
        ncc_wf(s),
        ncc_refines(s, a),
        s.result_count.contains_key(p),
    ensures
        s.primary_result.contains_key(p),
        ncc_pairs(a).contains((p, s.primary_result[p])),
        forall|r: TokenId|
            ncc_pairs(a).contains((p, r)) && r != s.primary_result[p] ==> a.owner[s.primary_result[p]].1
                < #[trigger] a.owner[r].1,
{
    assert(ncc_settled(s, p));
    let ps = s.parent_results[p];
    let r0 = ps[0];
    assert(ncc_fwd(s, p, r0) && ncc_owns(s, p, r0));
    lemma_ncc_pairs(a, p, r0);
    assert forall|r: TokenId| ncc_pairs(a).contains((p, r)) && r != r0 implies a.owner[r0].1
        < #[trigger] a.owner[r].1 by {
        lemma_ncc_pairs(a, p, r);
        assert(ncc_owns(s, p, r));
        assert(ncc_fwd(s, p, r));
        let j = choose|j: int| 0 <= j < ps.len() && ps[j] == r;
        assert(j != 0);
        assert(ncc_stamp(a)(ps[0]) < ncc_stamp(a)(ps[j]));
    }
}

pub proof fn lemma_ncc_refines_add(s: NccMem, a: NccAbs, p: TokenId, r: TokenId, o: u64)
    requires
        ncc_wf(s),
        ncc_refines(s, a),
        !s.result_owner.contains_key(r) && ncc_result_count(s, p) == 0 ==> fresh_order(
            s.block_order,
            o,
        ),
    ensures
        ncc_refines(ncc_add_result(s, p, r, o).0, ncc_abs_add(a, p, r)),
{
    let t = ncc_add_result(s, p, r, o).0;
    let b = ncc_abs_add(a, p, r);
    lemma_ncc_add_result(s, p, r, o);
    if !s.result_owner.contains_key(r) {
        assert(t.result_owner.dom() =~= b.owner.dom());
        assert forall|q: TokenId| #[trigger] t.parent_results.contains_key(q) implies sorted_by(
            t.parent_results[q],
            ncc_stamp(b),
        ) by {
            let prev = if s.parent_results.contains_key(q) {
                s.parent_results[q]
            } else {
                Seq::<TokenId>::empty()
            };
            assert forall|i: int, j: int| 0 <= i < j < prev.len() implies ncc_stamp(b)(
                #[trigger] prev[i],
            ) < ncc_stamp(b)(#[trigger] prev[j]) by {
                assert(sorted_by(prev, ncc_stamp(a)));
                assert(ncc_stamp(a)(prev[i]) < ncc_stamp(a)(prev[j]));
                assert(prev.contains(prev[i]) && prev.contains(prev[j]));
                assert(ncc_fwd(s, q, prev[i]) && ncc_fwd(s, q, prev[j]));
                assert(ncc_owns(s, q, prev[i]) && ncc_owns(s, q, prev[j]));
            }
            if q == p {
                lemma_os_insert(prev, r);
                assert forall|x: TokenId| prev.contains(x) implies ncc_stamp(b)(x) < ncc_stamp(b)(
                    r,
                ) by {
                    assert(ncc_fwd(s, p, x));
                    assert(ncc_owns(s, p, x));
                }
                assert(sorted_by(prev, ncc_stamp(b)));
                assert(sorted_by(os_insert(prev, r), ncc_stamp(b)));
            } else {
                assert(t.parent_results[q] == prev);
                assert(sorted_by(prev, ncc_stamp(b)));
            }
        }
    }
}

pub proof fn lemma_ncc_refines_remove_support(s: NccMem, a: NccAbs, r: TokenId)
    requires
        ncc_core(s),
        ncc_refines(s, a),
    ensures
        ncc_refines(ncc_remove_result_support(s, r).0, ncc_abs_remove(a, r)),
{
    let t = ncc_remove_result_support(s, r).0;
    let b = ncc_abs_remove(a, r);
    lemma_ncc_remove_result_support(s, r);
    assert(t.result_owner.dom() =~= b.owner.dom());
    assert forall|q: TokenId| #[trigger] t.parent_results.contains_key(q) implies sorted_by(
        t.parent_results[q],
        ncc_stamp(b),
    ) by {
        let sq = s.parent_results[q];
        let tq = t.parent_results[q];
        assert(sorted_by(sq, ncc_stamp(a)));
        if s.result_owner.contains_key(r) && q == s.result_owner[r] {
            lemma_os_remove(sq, r);
        }
        assert(sorted_by(tq, ncc_stamp(a)));
        assert forall|i: int, j: int| 0 <= i < j < tq.len() implies ncc_stamp(b)(#[trigger] tq[i])
            < ncc_stamp(b)(#[trigger] tq[j]) by {
            assert(ncc_stamp(a)(tq[i]) < ncc_stamp(a)(tq[j]));
            assert(tq.contains(tq[i]) && tq.contains(tq[j]));
            assert(ncc_fwd(t, q, tq[i]) && ncc_fwd(t, q, tq[j]));
            assert(ncc_owns(t, q, tq[i]) && ncc_owns(t, q, tq[j]));
        }
    }
}

pub proof fn lemma_ncc_refines_remove_parent(s: NccMem, a: NccAbs, p: TokenId)
    requires
        ncc_core(s),
        ncc_refines(s, a),
    ensures
        ncc_refines(ncc_remove_parent_token(s, p), ncc_abs_remove_parent(a, p)),
{
    let t = ncc_remove_parent_token(s, p);
    let b = ncc_abs_remove_parent(a, p);
    lemma_ncc_remove_parent_token(s, p);
    assert(t.result_owner.dom() =~= b.owner.dom());
    assert forall|q: TokenId| #[trigger] t.parent_results.contains_key(q) implies sorted_by(
        t.parent_results[q],
        ncc_stamp(b),
    ) by {
        let sq = s.parent_results[q];
        assert(sorted_by(sq, ncc_stamp(a)));
        assert forall|i: int, j: int| 0 <= i < j < sq.len() implies ncc_stamp(b)(#[trigger] sq[i])
            < ncc_stamp(b)(#[trigger] sq[j]) by {
            assert(ncc_stamp(a)(sq[i]) < ncc_stamp(a)(sq[j]));
            assert(sq.contains(sq[i]) && sq.contains(sq[j]));
            assert(ncc_fwd(t, q, sq[i]) && ncc_fwd(t, q, sq[j]));
            assert(ncc_owns(t, q, sq[i]) && ncc_owns(t, q, sq[j]));
        }
    }
}

// ------ two-phase result retraction (rete.rs:1526-1546, 399-456) -------
/// rete.rs:1947 `collect_ncc_result_retractions`, restricted to this memory,
/// over the removed tokens `rs[k..]` in order: a removed primary records a
/// `(parent, result)` transition (1978-1986), then its support is dropped
/// (1987).  The memory is left with dangling primaries.
pub open spec fn ncc_collect(s: NccMem, rs: Seq<TokenId>, k: int) -> (NccMem, Seq<
    (TokenId, TokenId),
>)
    decreases rs.len() - k,
{
    if k < 0 || k >= rs.len() {
        (s, Seq::empty())
    } else {
        let r = rs[k];
        if s.result_owner.contains_key(r) {
            let p = s.result_owner[r];
            let ev: Seq<(TokenId, TokenId)> = if s.primary_result.get(p) == Some(r) {
                seq![(p, r)]
            } else {
                Seq::empty()
            };
            let (t, evs) = ncc_collect(ncc_remove_result_support(s, r).0, rs, k + 1);
            (t, ev + evs)
        } else {
            ncc_collect(s, rs, k + 1)
        }
    }
}

/// rete.rs:1530-1543 / 432-441: `cleanup_negative_memories_for_token` calls
/// `remove_parent_token` for every removed token (the dead parents).
pub open spec fn ncc_drop_parents(s: NccMem, dead: Seq<TokenId>, k: int) -> NccMem
    decreases dead.len() - k,
{
    if k < 0 || k >= dead.len() {
        s
    } else {
        ncc_drop_parents(ncc_remove_parent_token(s, dead[k]), dead, k + 1)
    }
}

/// rete.rs:1994 `apply_ncc_result_retraction`, memory effects; the boolean
/// is true exactly when production goes on to `ncc_left_activate` (2036).
pub open spec fn ncc_apply(s: NccMem, p: TokenId, r: TokenId, alive: bool, order: u64) -> (
    NccMem,
    bool,
) {
    if s.primary_result.get(p) != Some(r) {
        (s, false)  // 2011-2013: stale transition

    } else if !alive {
        (ncc_remove_parent_token(s, p), false)  // 2014-2020

    } else {
        let o = if ncc_result_count(s, p) > 0 {
            order
        } else {
            0
        };  // 2021-2025
        let (t, still) = ncc_reassign_primary(s, p, o);  // 2026-2033
        (t, !still)
    }
}

/// The transitions `apply[k..]` applied in order (production sorts them,
/// rete.rs:1529 / 442; any order is allowed here).  `allocate_block_order`
/// is modelled as handing out `next + i` for the i-th transition.  Returns
/// the parents that unblock, in order.
pub open spec fn ncc_apply_all(
    s: NccMem,
    apply: Seq<(TokenId, TokenId)>,
    k: int,
    dead: Seq<TokenId>,
    next: nat,
) -> (NccMem, Seq<TokenId>)
    decreases apply.len() - k,
{
    if k < 0 || k >= apply.len() {
        (s, Seq::empty())
    } else {
        let (p, r) = apply[k];
        let (s1, u) = ncc_apply(s, p, r, !dead.contains(p), (next + k) as u64);
        let (t, us) = ncc_apply_all(s1, apply, k + 1, dead, next);
        (
            t,
            if u {
                seq![p] + us
            } else {
                us
            },
        )
    }
}

/// The whole cascade: collect, drop dead parents, apply transitions.
pub open spec fn ncc_cascade(
    s: NccMem,
    rs: Seq<TokenId>,
    dead: Seq<TokenId>,
    apply: Seq<(TokenId, TokenId)>,
    next: nat,
) -> (NccMem, Seq<TokenId>) {
    ncc_apply_all(ncc_drop_parents(ncc_collect(s, rs, 0).0, dead, 0), apply, 0, dead, next)
}

pub open spec fn ncc_abs_remove_seq(a: NccAbs, rs: Seq<TokenId>, k: int) -> NccAbs
    decreases rs.len() - k,
{
    if k < 0 || k >= rs.len() {
        a
    } else {
        ncc_abs_remove_seq(ncc_abs_remove(a, rs[k]), rs, k + 1)
    }
}

pub open spec fn ncc_abs_remove_parents(a: NccAbs, dead: Seq<TokenId>, k: int) -> NccAbs
    decreases dead.len() - k,
{
    if k < 0 || k >= dead.len() {
        a
    } else {
        ncc_abs_remove_parents(ncc_abs_remove_parent(a, dead[k]), dead, k + 1)
    }
}

pub open spec fn ncc_abs_cascade(a: NccAbs, rs: Seq<TokenId>, dead: Seq<TokenId>) -> NccAbs {
    ncc_abs_remove_parents(ncc_abs_remove_seq(a, rs, 0), dead, 0)
}

/// Every stored block-list position is below `n`.
pub open spec fn ncc_orders_below(s: NccMem, n: int) -> bool {
    forall|q: TokenId| #[trigger] s.block_order.contains_key(q) ==> s.block_order[q] < n
}

/// Caller obligations of the cascade: every transition the collect phase
/// records is applied, and fresh orders do not wrap.
pub open spec fn ncc_cascade_ok(
    s: NccMem,
    rs: Seq<TokenId>,
    apply: Seq<(TokenId, TokenId)>,
    next: nat,
) -> bool {
    &&& forall|e: (TokenId, TokenId)| #[trigger]
        ncc_collect(s, rs, 0).1.contains(e) ==> apply.contains(e)
    &&& ncc_orders_below(s, next as int)
    &&& next + apply.len() <= u64::MAX + 1
}

pub proof fn lemma_suffix_contains_step<A>(s: Seq<A>, k: int, x: A)
    requires
        0 <= k < s.len(),
        s.subrange(k, s.len() as int).contains(x),
        s[k] != x,
    ensures
        s.subrange(k + 1, s.len() as int).contains(x),
{
    let j = choose|j: int|
        0 <= j < s.subrange(k, s.len() as int).len() && s.subrange(k, s.len() as int)[j] == x;
    assert(s[k + j] == x);
    assert(j != 0);
    assert(s.subrange(k + 1, s.len() as int)[j - 1] == x);
}

pub proof fn lemma_ncc_collect(s: NccMem, a: NccAbs, rs: Seq<TokenId>, k: int, e: Set<
    (TokenId, TokenId),
>)
    requires
        ncc_core(s),
        ncc_refines(s, a),
        forall|q: TokenId| !(#[trigger] ncc_settled(s, q)) ==> e.contains((q, s.primary_result[q])),
    ensures
        ({
            let (t, evs) = ncc_collect(s, rs, k);
            &&& ncc_core(t)
            &&& ncc_refines(t, ncc_abs_remove_seq(a, rs, k))
            &&& t.primary_result == s.primary_result
            &&& t.block_order == s.block_order
            &&& t.unblocked == s.unblocked
            &&& forall|x: TokenId| #[trigger] t.result_owner.contains_key(x) ==> s.result_owner.contains_key(x)
            &&& forall|q: TokenId| #[trigger]
                t.result_count.contains_key(q) ==> s.result_count.contains_key(q)
            &&& forall|q: TokenId|
                !(#[trigger] ncc_settled(t, q)) ==> e.contains((q, t.primary_result[q]))
                    || evs.contains((q, t.primary_result[q]))
        }),
    decreases rs.len() - k,
{
    if k < 0 || k >= rs.len() {
    } else {
        let r = rs[k];
        if s.result_owner.contains_key(r) {
            let p = s.result_owner[r];
            let ev: Seq<(TokenId, TokenId)> = if s.primary_result.get(p) == Some(r) {
                seq![(p, r)]
            } else {
                Seq::empty()
            };
            let s1 = ncc_remove_result_support(s, r).0;
            lemma_ncc_remove_result_support(s, r);
            lemma_ncc_refines_remove_support(s, a, r);
            let e1 = e + ev.to_set();
            assert forall|q: TokenId| !(#[trigger] ncc_settled(s1, q)) implies e1.contains(
                (q, s1.primary_result[q]),
            ) by {
                if ncc_settled(s, q) {
                    assert(q == p && s.primary_result[q] == r);
                    assert(ev[0] == (q, s1.primary_result[q]));
                    assert(ev.to_set().contains(ev[0]));
                }
            }
            lemma_ncc_collect(s1, ncc_abs_remove(a, r), rs, k + 1, e1);
            let (t, evs) = ncc_collect(s1, rs, k + 1);
            assert forall|q: TokenId| !(#[trigger] ncc_settled(t, q)) implies e.contains(
                (q, t.primary_result[q]),
            ) || (ev + evs).contains((q, t.primary_result[q])) by {
                let x = (q, t.primary_result[q]);
                if !e.contains(x) {
                    if ev.to_set().contains(x) {
                        let i = choose|i: int| 0 <= i < ev.len() && ev[i] == x;
                        assert((ev + evs)[i] == x);
                    } else {
                        assert(evs.contains(x));
                        let i = choose|i: int| 0 <= i < evs.len() && evs[i] == x;
                        assert((ev + evs)[ev.len() + i] == x);
                    }
                }
            }
        } else {
            lemma_ncc_collect(s, a, rs, k + 1, e);
            assert(ncc_abs_remove(a, r) == a) by {
                assert(ncc_abs_remove(a, r).owner =~= a.owner);
            }
        }
    }
}

pub proof fn lemma_ncc_drop_parents(s: NccMem, a: NccAbs, dead: Seq<TokenId>, k: int)
    requires
        ncc_core(s),
        ncc_refines(s, a),
        0 <= k <= dead.len(),
    ensures
        ({
            let t = ncc_drop_parents(s, dead, k);
            let gone = |q: TokenId| dead.subrange(k, dead.len() as int).contains(q);
            &&& ncc_core(t)
            &&& ncc_refines(t, ncc_abs_remove_parents(a, dead, k))
            &&& ncc_disjoint(s) ==> ncc_disjoint(t)
            &&& forall|x: TokenId| #[trigger] t.result_owner.contains_key(x) ==> s.result_owner.contains_key(x)
            &&& forall|q: TokenId| #[trigger]
                t.primary_result.contains_key(q) <==> (s.primary_result.contains_key(q) && !gone(
                    q,
                ))
            &&& forall|q: TokenId| #[trigger]
                t.result_count.contains_key(q) <==> (s.result_count.contains_key(q) && !gone(q))
            &&& forall|q: TokenId| #[trigger]
                t.primary_result.contains_key(q) ==> t.primary_result[q] == s.primary_result[q]
            &&& forall|q: TokenId| !(#[trigger] ncc_settled(t, q)) ==> !ncc_settled(s, q)
            &&& forall|q: TokenId| #[trigger]
                t.block_order.contains_key(q) ==> s.block_order.contains_key(q)
                    && t.block_order[q] == s.block_order[q]
        }),
    decreases dead.len() - k,
{
    if k >= dead.len() {
        assert forall|q: TokenId| !dead.subrange(k, dead.len() as int).contains(q) by {
            assert(dead.subrange(k, dead.len() as int).len() == 0);
        }
    } else {
        let p = dead[k];
        let s1 = ncc_remove_parent_token(s, p);
        lemma_ncc_remove_parent_token(s, p);
        lemma_ncc_refines_remove_parent(s, a, p);
        lemma_ncc_drop_parents(s1, ncc_abs_remove_parent(a, p), dead, k + 1);
        assert forall|q: TokenId|
            dead.subrange(k, dead.len() as int).contains(q) <==> (q == p || dead.subrange(
                k + 1,
                dead.len() as int,
            ).contains(q)) by {
            let sk = dead.subrange(k, dead.len() as int);
            let sk1 = dead.subrange(k + 1, dead.len() as int);
            if sk.contains(q) && q != p {
                lemma_suffix_contains_step(dead, k, q);
            }
            if q == p {
                assert(sk[0] == q);
            }
            if sk1.contains(q) {
                let j = choose|j: int| 0 <= j < sk1.len() && sk1[j] == q;
                assert(sk[j + 1] == q);
            }
        }
    }
}

pub proof fn lemma_ncc_apply_all(
    s: NccMem,
    apply: Seq<(TokenId, TokenId)>,
    k: int,
    dead: Seq<TokenId>,
    next: nat,
)
    requires
        ncc_core(s),
        0 <= k <= apply.len(),
        forall|q: TokenId|
            !(#[trigger] ncc_settled(s, q)) ==> apply.subrange(k, apply.len() as int).contains(
                (q, s.primary_result[q]),
            ),
        forall|q: TokenId| #[trigger] s.primary_result.contains_key(q) ==> !dead.contains(q),
        ncc_orders_below(s, next + k),
        next + apply.len() <= u64::MAX + 1,
    ensures
        ({
            let (t, us) = ncc_apply_all(s, apply, k, dead, next);
            &&& ncc_wf(t)
            &&& t.result_count == s.result_count
            &&& t.parent_results == s.parent_results
            &&& t.result_owner == s.result_owner
            &&& t.unblocked == s.unblocked
            &&& forall|q: TokenId| #[trigger]
                us.contains(q) <==> (s.primary_result.contains_key(q)
                    && !s.result_count.contains_key(q))
            &&& us.no_duplicates()
            &&& ncc_orders_below(t, next + apply.len() as int)
        }),
    decreases apply.len() - k,
{
    if k >= apply.len() {
        assert forall|q: TokenId| #[trigger] ncc_settled(s, q) by {
            if !ncc_settled(s, q) {
                assert(apply.subrange(k, apply.len() as int).len() == 0);
            }
        }
        assert forall|q: TokenId|
            !(s.primary_result.contains_key(q) && !s.result_count.contains_key(q)) by {
            assert(ncc_settled(s, q));
        }
    } else {
        let (p, r) = apply[k];
        let alive = !dead.contains(p);
        let (s1, u) = ncc_apply(s, p, r, alive, (next + k) as u64);
        if s.primary_result.get(p) != Some(r) {
            assert forall|q: TokenId|
                !(#[trigger] ncc_settled(s1, q)) implies apply.subrange(
                k + 1,
                apply.len() as int,
            ).contains((q, s1.primary_result[q])) by {
                lemma_suffix_contains_step(apply, k, (q, s.primary_result[q]));
            }
        } else {
            assert(alive);
            let o = if ncc_result_count(s, p) > 0 {
                (next + k) as u64
            } else {
                0
            };
            lemma_ncc_reassign_primary(s, p, o);
            assert forall|q: TokenId|
                !(#[trigger] ncc_settled(s1, q)) implies apply.subrange(
                k + 1,
                apply.len() as int,
            ).contains((q, s1.primary_result[q])) by {
                assert(q != p);
                assert(!ncc_settled(s, q));
                lemma_suffix_contains_step(apply, k, (q, s.primary_result[q]));
            }
        }
        lemma_ncc_apply_all(s1, apply, k + 1, dead, next);
        let (t, us1) = ncc_apply_all(s1, apply, k + 1, dead, next);
        let us = if u {
            seq![p] + us1
        } else {
            us1
        };
        assert(ncc_apply_all(s, apply, k, dead, next) == (t, us));
        assert forall|q: TokenId| #[trigger]
            us.contains(q) <==> (s.primary_result.contains_key(q) && !s.result_count.contains_key(
                q,
            )) by {
            if u {
                if us.contains(q) && q != p {
                    let i = choose|i: int| 0 <= i < us.len() && us[i] == q;
                    assert(us1[i - 1] == q);
                }
                if us1.contains(q) {
                    let i = choose|i: int| 0 <= i < us1.len() && us1[i] == q;
                    assert(us[i + 1] == q);
                }
                assert(us[0] == p);
            }
        }
        if u {
            assert(!us1.contains(p));
            assert forall|i: int, j: int|
                0 <= i < us.len() && 0 <= j < us.len() && i != j implies us[i] != us[j] by {
                if i == 0 {
                    assert(us1.contains(us[j]));
                } else if j == 0 {
                    assert(us1.contains(us[i]));
                } else {
                    assert(us[i] == us1[i - 1] && us[j] == us1[j - 1]);
                }
            }
        }
    }
}

/// THEOREM (NCC two-phase retraction cascade): starting from a settled
/// memory, collect + parent cleanup + applying every recorded transition
/// (in any order) yields a settled memory that refines the ground truth with
/// the removed results and dead parents deleted.  The parents reported as
/// unblocked (where production calls `ncc_left_activate`) are exactly the
/// surviving parents whose result set went from non-empty to empty, each
/// once.
pub proof fn theorem_ncc_cascade(
    s: NccMem,
    a: NccAbs,
    rs: Seq<TokenId>,
    dead: Seq<TokenId>,
    apply: Seq<(TokenId, TokenId)>,
    next: nat,
)
    requires
        ncc_wf(s),
        ncc_refines(s, a),
        ncc_cascade_ok(s, rs, apply, next),
    ensures
        ({
            let (t, us) = ncc_cascade(s, rs, dead, apply, next);
            let b = ncc_abs_cascade(a, rs, dead);
            &&& ncc_wf(t)
            &&& ncc_disjoint(s) ==> ncc_disjoint(t)
            &&& forall|x: TokenId| #[trigger] t.result_owner.contains_key(x) ==> s.result_owner.contains_key(x)
            &&& ncc_refines(t, b)
            &&& forall|q: TokenId| #[trigger]
                us.contains(q) <==> (s.result_count.contains_key(q)
                    && !t.result_count.contains_key(q) && !dead.contains(q))
            &&& forall|q: TokenId| #[trigger]
                us.contains(q) <==> (scratch_fwd(ncc_pairs(a)).contains_key(q) && !scratch_fwd(
                    ncc_pairs(b),
                ).contains_key(q) && !dead.contains(q))
            &&& us.no_duplicates()
            &&& ncc_orders_below(t, next + apply.len() as int)
        }),
{
    let (s1, evs) = ncc_collect(s, rs, 0);
    assert forall|q: TokenId| !(#[trigger] ncc_settled(s, q)) implies Set::<
        (TokenId, TokenId),
    >::empty().contains((q, s.primary_result[q])) by {}
    lemma_ncc_collect(s, a, rs, 0, Set::empty());
    let a1 = ncc_abs_remove_seq(a, rs, 0);
    let s2 = ncc_drop_parents(s1, dead, 0);
    lemma_ncc_drop_parents(s1, a1, dead, 0);
    assert(dead.subrange(0, dead.len() as int) == dead);
    assert(apply.subrange(0, apply.len() as int) == apply);
    assert forall|q: TokenId| !(#[trigger] ncc_settled(s2, q)) implies apply.subrange(
        0,
        apply.len() as int,
    ).contains((q, s2.primary_result[q])) by {
        assert(!ncc_settled(s1, q));
        assert(s2.primary_result.contains_key(q));
        assert(evs.contains((q, s1.primary_result[q])));
    }
    assert(ncc_orders_below(s2, next + 0int));
    lemma_ncc_apply_all(s2, apply, 0, dead, next);
    let (t, us) = ncc_cascade(s, rs, dead, apply, next);
    assert forall|q: TokenId| #[trigger]
        us.contains(q) <==> (s.result_count.contains_key(q) && !t.result_count.contains_key(q)
            && !dead.contains(q)) by {
        assert(ncc_settled(s, q));
        assert(s.result_count.contains_key(q) ==> s.primary_result.contains_key(q));
    }
    lemma_ncc_from_scratch(s, a);
    lemma_ncc_from_scratch(t, ncc_abs_cascade(a, rs, dead));
}

// ------------------- arbitrary operation sequences ---------------------
pub enum NccOp {
    AddResult(TokenId, TokenId, u64),
    RemoveResult(TokenId, u64),
    /// removed tokens, dead parents, transitions in apply order, first fresh order
    Cascade(Seq<TokenId>, Seq<TokenId>, Seq<(TokenId, TokenId)>, nat),
    SetUnblocked(TokenId, TokenId),
    RemoveUnblocked(TokenId),
    RemoveParentToken(TokenId),
    Clear,
}

pub open spec fn ncc_step(s: NccMem, op: NccOp) -> NccMem {
    match op {
        NccOp::AddResult(p, r, o) => ncc_add_result(s, p, r, o).0,
        NccOp::RemoveResult(r, o) => ncc_remove_result(s, r, o).0,
        NccOp::Cascade(rs, dead, apply, next) => ncc_cascade(s, rs, dead, apply, next).0,
        NccOp::SetUnblocked(p, pt) => ncc_set_unblocked(s, p, pt),
        NccOp::RemoveUnblocked(p) => ncc_remove_unblocked(s, p).0,
        NccOp::RemoveParentToken(p) => ncc_remove_parent_token(s, p),
        NccOp::Clear => ncc_clear(s),
    }
}

pub open spec fn ncc_abs_step(a: NccAbs, op: NccOp) -> NccAbs {
    match op {
        NccOp::AddResult(p, r, _) => ncc_abs_add(a, p, r),
        NccOp::RemoveResult(r, _) => ncc_abs_remove(a, r),
        NccOp::Cascade(rs, dead, _, _) => ncc_abs_cascade(a, rs, dead),
        NccOp::RemoveParentToken(p) => ncc_abs_remove_parent(a, p),
        NccOp::Clear => ncc_abs_clear(a),
        _ => a,
    }
}

/// Caller obligations: fresh block orders where they are stored, every
/// recorded cascade transition is applied, and the number of tracked result
/// tokens stays below `usize::MAX`.  The last holds because tracked results
/// are live tokens (snapshot.rs:1500-1504) and a slotmap holds fewer than
/// `u32::MAX` elements (slotmap 1.1.1 basic.rs:413), i.e. at most
/// `u32::MAX - 1 <= usize::MAX - 1` on 32- and 64-bit targets.
pub open spec fn ncc_pre(s: NccMem, op: NccOp) -> bool {
    match op {
        NccOp::AddResult(p, r, o) => s.result_owner.len() < usize::MAX && (
        !s.result_owner.contains_key(r) && ncc_result_count(s, p) == 0 ==> fresh_order(
            s.block_order,
            o,
        )),
        NccOp::RemoveResult(r, o) => s.result_owner.contains_key(r) && s.primary_result.get(
            s.result_owner[r],
        ) == Some(r) && s.result_count[s.result_owner[r]] > 1 ==> fresh_order(s.block_order, o),
        NccOp::Cascade(rs, _, apply, next) => ncc_cascade_ok(s, rs, apply, next),
        _ => true,
    }
}

pub open spec fn ncc_run(ops: Seq<NccOp>) -> NccMem
    decreases ops.len(),
{
    if ops.len() == 0 {
        ncc_new()
    } else {
        ncc_step(ncc_run(ops.drop_last()), ops.last())
    }
}

pub open spec fn ncc_abs_run(ops: Seq<NccOp>) -> NccAbs
    decreases ops.len(),
{
    if ops.len() == 0 {
        ncc_abs_new()
    } else {
        ncc_abs_step(ncc_abs_run(ops.drop_last()), ops.last())
    }
}

pub open spec fn ncc_admissible(ops: Seq<NccOp>) -> bool
    decreases ops.len(),
{
    ops.len() == 0 || (ncc_admissible(ops.drop_last()) && ncc_pre(
        ncc_run(ops.drop_last()),
        ops.last(),
    ))
}

/// Every stored count fits in a `usize`.
pub open spec fn ncc_counts_fit(s: NccMem) -> bool {
    forall|p: TokenId| #[trigger] s.result_count.contains_key(p) ==> s.result_count[p] <= usize::MAX
}

/// One step preserves the invariant and refinement; `add_result` cannot
/// overflow its `usize` counter, and its `(old, new)` result satisfies the
/// caller's blocking test (rete.rs:1901 `old == 0 && new == 1`) exactly when
/// the parent's ground-truth result set goes from empty to non-empty.
pub proof fn lemma_ncc_step(s: NccMem, a: NccAbs, op: NccOp)
    requires
        ncc_wf(s),
        ncc_refines(s, a),
        ncc_pre(s, op),
        ncc_owner_fits(s),
    ensures
        ncc_wf(ncc_step(s, op)),
        ncc_refines(ncc_step(s, op), ncc_abs_step(a, op)),
        ncc_counts_fit(ncc_step(s, op)),
        ncc_owner_fits(ncc_step(s, op)),
        op is AddResult ==> ({
            let (p, r, o) = (op->AddResult_0, op->AddResult_1, op->AddResult_2);
            let (old_c, new_c) = ncc_add_result(s, p, r, o).1;
            &&& (!s.result_owner.contains_key(r) ==> old_c + 1 <= usize::MAX)
            &&& (old_c == 0 && new_c == 1) <==> (!scratch_fwd(ncc_pairs(a)).contains_key(p)
                && scratch_fwd(ncc_pairs(ncc_abs_step(a, op))).contains_key(p))
        }),
{
    let t = ncc_step(s, op);
    let b = ncc_abs_step(a, op);
    match op {
        NccOp::AddResult(p, r, o) => {
            lemma_ncc_add_result(s, p, r, o);
            lemma_ncc_refines_add(s, a, p, r, o);
            lemma_ncc_from_scratch(s, a);
            lemma_ncc_from_scratch(t, b);
            if s.result_owner.contains_key(r) {
                assert(b == a);
            }
        },
        NccOp::RemoveResult(r, o) => {
            lemma_ncc_remove_result(s, r, o);
            let s1 = ncc_remove_result_support(s, r).0;
            lemma_ncc_remove_result_support(s, r);
            lemma_ncc_refines_remove_support(s, a, r);
            if s.result_owner.contains_key(r) && s1.primary_result.get(s.result_owner[r]) == Some(
                r,
            ) {
                let p = s.result_owner[r];
                if s1.parent_results.contains_key(p) {
                    assert(s.result_count[p] > 1) by {
                        assert(s1.result_count[p] == s.result_count[p] - 1);
                    }
                }
                lemma_ncc_reassign_primary(s1, p, o);
            }
        },
        NccOp::Cascade(rs, dead, apply, next) => {
            theorem_ncc_cascade(s, a, rs, dead, apply, next);
        },
        NccOp::SetUnblocked(p, pt) => {
            assert forall|q: TokenId| #[trigger] ncc_settled(t, q) by {
                assert(ncc_settled(s, q));
            }
            assert forall|q: TokenId, x: TokenId|
                #![trigger ncc_fwd(t, q, x)]
                #![trigger ncc_owns(t, q, x)]
                ncc_fwd(t, q, x) <==> ncc_owns(t, q, x) by {
                assert(ncc_fwd(s, q, x) <==> ncc_owns(s, q, x));
            }
        },
        NccOp::RemoveUnblocked(p) => {
            assert forall|q: TokenId| #[trigger] ncc_settled(t, q) by {
                assert(ncc_settled(s, q));
            }
            assert forall|q: TokenId, x: TokenId|
                #![trigger ncc_fwd(t, q, x)]
                #![trigger ncc_owns(t, q, x)]
                ncc_fwd(t, q, x) <==> ncc_owns(t, q, x) by {
                assert(ncc_fwd(s, q, x) <==> ncc_owns(s, q, x));
            }
        },
        NccOp::RemoveParentToken(p) => {
            lemma_ncc_remove_parent_token(s, p);
            lemma_ncc_refines_remove_parent(s, a, p);
            assert forall|q: TokenId| #[trigger] ncc_settled(t, q) by {
                if q != p {
                    assert(ncc_settled(s, q));
                }
            }
        },
        NccOp::Clear => {
            lemma_ncc_new();
            assert(ncc_refines(t, b)) by {
                assert(t.result_owner.dom() =~= b.owner.dom());
            }
        },
    }
    // The tracked-result map grows only through `add_result`, by one key.
    assert(t.result_owner.len() <= usize::MAX) by {
        match op {
            NccOp::AddResult(q, r, o) => {
                if !s.result_owner.contains_key(r) {
                    assert(t.result_owner.dom() =~= s.result_owner.dom().insert(r));
                } else {
                    assert(t == s);
                }
            },
            _ => {
                assert(t.result_owner.dom().subset_of(s.result_owner.dom())) by {
                    match op {
                        NccOp::RemoveParentToken(p) => {
                            lemma_ncc_remove_parent_token(s, p);
                        },
                        NccOp::Clear => {
                            lemma_ncc_new();
                        },
                        NccOp::RemoveResult(r, o) => {
                            lemma_ncc_remove_result(s, r, o);
                        },
                        NccOp::Cascade(rs, dead, apply, next) => {
                            theorem_ncc_cascade(s, a, rs, dead, apply, next);
                        },
                        _ => {},
                    }
                }
                vstd::set_lib::lemma_len_subset(t.result_owner.dom(), s.result_owner.dom());
            },
        }
    }
    assert forall|p: TokenId| #[trigger] t.result_count.contains_key(p) implies t.result_count[p]
        <= usize::MAX by {
        lemma_ncc_count_bound(t, p);
    }
}

/// Number of tracked results fits the documented bound.
pub open spec fn ncc_owner_fits(s: NccMem) -> bool {
    s.result_owner.len() <= usize::MAX
}

/// THEOREM (NccMemory, arbitrary finite operation sequences, including the
/// production two-phase cascade): starting from `new()`, after any admissible
/// sequence the memory is settled (all structural checks of
/// `validate_consistency` hold; check 1 is the caller protocol, Section 6),
/// refines the ground truth with oldest-first result lists, its result sets
/// and counts equal the from-scratch values, and every count fits in usize.
pub proof fn theorem_ncc_sequences(ops: Seq<NccOp>)
    requires
        ncc_admissible(ops),
    ensures
        ({
            let s = ncc_run(ops);
            let a = ncc_abs_run(ops);
            &&& ncc_wf(s)
            &&& ncc_refines(s, a)
            &&& ncc_fwd_view(s) == scratch_fwd(ncc_pairs(a))
            &&& forall|p: TokenId| #[trigger]
                ncc_result_count(s, p) == scratch_count(ncc_pairs(a), p)
            &&& ncc_counts_fit(s)
            &&& ncc_owner_fits(s)
        }),
    decreases ops.len(),
{
    if ops.len() == 0 {
        lemma_ncc_new();
        let s = ncc_run(ops);
        assert(s.result_owner.dom() =~= ncc_abs_new().owner.dom());
        assert(s.result_owner.len() == 0);
    } else {
        theorem_ncc_sequences(ops.drop_last());
        lemma_ncc_step(ncc_run(ops.drop_last()), ncc_abs_run(ops.drop_last()), ops.last());
    }
    lemma_ncc_from_scratch(ncc_run(ops), ncc_abs_run(ops));
}

// =====================================================================
// Section 4. Negative node protocol (crates/ferric-rules-core/src/rete.rs)
// =====================================================================
/// What a node operation does to downstream matches.
pub enum Signal {
    /// a pass-through token `pt` for `parent` is created and propagated
    Propagate(TokenId, TokenId),
    /// the pass-through token `pt` of `parent` is cascade-retracted
    Retract(TokenId, TokenId),
}

pub open spec fn sig_parent(x: Signal) -> TokenId {
    match x {
        Signal::Propagate(p, _) => p,
        Signal::Retract(p, _) => p,
    }
}

/// No parent is signalled twice by one operation.
pub open spec fn one_per_parent(sig: Seq<Signal>) -> bool {
    forall|i: int, j: int|
        0 <= i < sig.len() && 0 <= j < sig.len() && i != j ==> sig_parent(#[trigger] sig[i])
            != sig_parent(#[trigger] sig[j])
}

/// The join tests of a node, as a fixed relation between (immutable) parent
/// tokens and facts.  Abstracts `evaluate_pattern` (rete.rs:1273, 1371,
/// 1386, 2106, 2206).
pub type JoinRel = spec_fn(TokenId, FactId) -> bool;

/// Ground truth: the facts of the alpha memory that match `p`.
pub open spec fn gt_matches(facts: Set<FactId>, m: JoinRel, p: TokenId) -> Set<FactId> {
    facts.filter(|f: FactId| m(p, f))
}

/// A negative node: its memory plus the world it summarises.
pub struct NegNode {
    pub mem: NegMem,
    /// live parent tokens in the upstream beta memory
    pub parents: Set<TokenId>,
    /// facts in the node's alpha memory
    pub facts: Set<FactId>,
    /// `ReteNetwork::next_block_order` (rete.rs:152)
    pub next_order: nat,
}

pub open spec fn neg_node_inv(n: NegNode, m: JoinRel) -> bool {
    &&& neg_valid(n.mem)
    &&& forall|p: TokenId, f: FactId| #[trigger]
        neg_fwd(n.mem, p, f) <==> (n.parents.contains(p) && n.facts.contains(f) && m(p, f))
    &&& forall|p: TokenId| #[trigger]
        n.mem.unblocked.contains_key(p) <==> (n.parents.contains(p) && !n.mem.blocked.contains_key(
            p,
        ))
    &&& forall|p: TokenId| #[trigger] n.mem.blocked.contains_key(p) ==> n.parents.contains(p)
    &&& neg_orders_below(n.mem, n.next_order as int)
}

pub open spec fn neg_orders_below(s: NegMem, n: int) -> bool {
    forall|q: TokenId| #[trigger] s.block_order.contains_key(q) ==> s.block_order[q] < n
}

/// Ground-truth "unblocked": live, and no fact of the alpha memory matches.
pub open spec fn neg_gt_unblocked(n: NegNode, m: JoinRel, p: TokenId) -> bool {
    n.parents.contains(p) && gt_matches(n.facts, m, p).is_empty()
}

/// Signals are exactly the ground-truth blocked <-> unblocked transitions of
/// live parents, carrying the pass-through token concerned.
pub open spec fn neg_signals_exact(n: NegNode, n2: NegNode, m: JoinRel, sig: Seq<Signal>) -> bool {
    &&& one_per_parent(sig)
    &&& forall|p: TokenId, x: TokenId| #[trigger]
        sig.contains(Signal::Propagate(p, x)) <==> (!neg_gt_unblocked(n, m, p) && neg_gt_unblocked(
            n2,
            m,
            p,
        ) && n2.mem.unblocked[p] == x)
    &&& forall|p: TokenId, x: TokenId| #[trigger]
        sig.contains(Signal::Retract(p, x)) <==> (neg_gt_unblocked(n, m, p) && !neg_gt_unblocked(
            n2,
            m,
            p,
        ) && n2.parents.contains(p) && n.mem.unblocked[p] == x)
}

pub proof fn lemma_neg_blocked_iff_fwd(s: NegMem, p: TokenId)
    requires
        neg_wf(s),
    ensures
        s.blocked.contains_key(p) <==> exists|f: FactId| neg_fwd(s, p, f),
{
    if s.blocked.contains_key(p) {
        assert(neg_fwd(s, p, s.blocked[p][0]));
    }
}

/// THEOREM (pass-through iff unblocked; incremental = from scratch): in every
/// node state satisfying the invariant, a parent has a pass-through token
/// iff it is live and no fact of the alpha memory matches it, its blocker
/// list holds exactly the matching facts, and `validate_consistency` holds.
pub proof fn lemma_neg_node_passthrough(n: NegNode, m: JoinRel, p: TokenId)
    requires
        neg_node_inv(n, m),
    ensures
        n.mem.unblocked.contains_key(p) <==> neg_gt_unblocked(n, m, p),
        n.mem.blocked.contains_key(p) <==> (n.parents.contains(p) && !gt_matches(
            n.facts,
            m,
            p,
        ).is_empty()),
        n.mem.blocked.contains_key(p) ==> n.mem.blocked[p].to_set() == gt_matches(n.facts, m, p),
        neg_validate_consistency(n.mem),
{
    let g = gt_matches(n.facts, m, p);
    lemma_neg_blocked_iff_fwd(n.mem, p);
    if n.mem.blocked.contains_key(p) {
        let f0 = n.mem.blocked[p][0];
        assert(neg_fwd(n.mem, p, f0));
        assert(g.contains(f0));
        assert(n.mem.blocked[p].to_set() =~= g) by {
            assert forall|f: FactId| n.mem.blocked[p].to_set().contains(f) <==> g.contains(f) by {
                assert(neg_fwd(n.mem, p, f) <==> (n.parents.contains(p) && n.facts.contains(f) && m(
                    p,
                    f,
                )));
            }
        }
    } else if n.parents.contains(p) && !g.is_empty() {
        let f = g.choose();
        assert(g.contains(f));
        assert(neg_fwd(n.mem, p, f));
    }
    lemma_neg_valid_implies_validate(n.mem);
}

/// Signals stated on the concrete memory imply the ground-truth statement.
pub proof fn lemma_neg_signals_from_concrete(n: NegNode, n2: NegNode, m: JoinRel, sig: Seq<Signal>)
    requires
        neg_node_inv(n, m),
        neg_node_inv(n2, m),
        one_per_parent(sig),
        forall|p: TokenId, x: TokenId| #[trigger]
            sig.contains(Signal::Propagate(p, x)) <==> (!n.mem.unblocked.contains_key(p)
                && n2.mem.unblocked.contains_key(p) && n2.mem.unblocked[p] == x),
        forall|p: TokenId, x: TokenId| #[trigger]
            sig.contains(Signal::Retract(p, x)) <==> (n.mem.unblocked.contains_key(p)
                && !n2.mem.unblocked.contains_key(p) && n2.parents.contains(p)
                && n.mem.unblocked[p] == x),
    ensures
        neg_signals_exact(n, n2, m, sig),
{
    assert forall|p: TokenId, x: TokenId| #[trigger]
        sig.contains(Signal::Propagate(p, x)) <==> (!neg_gt_unblocked(n, m, p) && neg_gt_unblocked(
            n2,
            m,
            p,
        ) && n2.mem.unblocked[p] == x) by {
        lemma_neg_node_passthrough(n, m, p);
        lemma_neg_node_passthrough(n2, m, p);
    }
    assert forall|p: TokenId, x: TokenId| #[trigger]
        sig.contains(Signal::Retract(p, x)) <==> (neg_gt_unblocked(n, m, p) && !neg_gt_unblocked(
            n2,
            m,
            p,
        ) && n2.parents.contains(p) && n.mem.unblocked[p] == x) by {
        lemma_neg_node_passthrough(n, m, p);
        lemma_neg_node_passthrough(n2, m, p);
    }
}

// ----------------------------- left activation -------------------------
/// rete.rs:1307-1309 `for fact_id in blocking_facts { add_blocker(p, fact_id, order) }`
pub open spec fn neg_add_blockers(s: NegMem, p: TokenId, fs: Seq<FactId>, order: u64) -> NegMem
    decreases fs.len(),
{
    if fs.len() == 0 {
        s
    } else {
        neg_add_blocker(neg_add_blockers(s, p, fs.drop_last(), order), p, fs.last(), order)
    }
}

/// rete.rs:1211 `negative_left_activate`: `bfs` are the blocking facts found
/// by `collect_candidate_facts` + `evaluate_pattern`; `pt` is the
/// pass-through token inserted into the token store (1287).
pub open spec fn neg_left_activate(n: NegNode, p: TokenId, bfs: Seq<FactId>, pt: TokenId) -> (
    NegNode,
    Seq<Signal>,
) {
    let parents = n.parents.insert(p);
    if bfs.len() == 0 {
        // 1278-1301
        (
            NegNode { mem: neg_set_unblocked(n.mem, p, pt), parents, ..n },
            seq![Signal::Propagate(p, pt)],
        )
    } else {
        // 1305-1310
        let order = n.next_order as u64;
        (
            NegNode {
                mem: neg_add_blockers(n.mem, p, bfs, order),
                parents,
                next_order: n.next_order + 1,
                ..n
            },
            Seq::empty(),
        )
    }
}

/// Preconditions: a new parent token; `bfs` lists exactly the matching facts
/// (the candidate index and join tests are trusted); no order wrap-around.
pub open spec fn neg_left_ok(n: NegNode, m: JoinRel, p: TokenId, bfs: Seq<FactId>) -> bool {
    &&& !n.parents.contains(p)
    &&& bfs.no_duplicates()
    &&& forall|f: FactId| #[trigger] bfs.contains(f) <==> (n.facts.contains(f) && m(p, f))
    &&& n.next_order < u64::MAX
}

pub proof fn lemma_neg_add_blockers(s: NegMem, p: TokenId, fs: Seq<FactId>, order: u64)
    requires
        neg_wf(s),
        !s.blocked.contains_key(p) ==> fresh_order(s.block_order, order),
    ensures
        ({
            let t = neg_add_blockers(s, p, fs, order);
            &&& neg_wf(t)
            &&& forall|q: TokenId, g: FactId| #[trigger]
                neg_fwd(t, q, g) <==> (neg_fwd(s, q, g) || (q == p && fs.contains(g)))
            &&& t.unblocked == s.unblocked
            &&& forall|q: TokenId| #[trigger]
                t.blocked.contains_key(q) <==> (s.blocked.contains_key(q) || (q == p && fs.len()
                    > 0))
            &&& forall|q: TokenId| #[trigger]
                t.block_order.contains_key(q) && q != p ==> t.block_order[q] == s.block_order[q]
            &&& t.block_order.contains_key(p) ==> t.block_order[p] == if s.blocked.contains_key(p) {
                s.block_order[p]
            } else {
                order
            }
        }),
    decreases fs.len(),
{
    if fs.len() > 0 {
        let prev = neg_add_blockers(s, p, fs.drop_last(), order);
        lemma_neg_add_blockers(s, p, fs.drop_last(), order);
        if !prev.blocked.contains_key(p) {
            assert(fs.drop_last().len() == 0);
            assert(prev == s);
        }
        lemma_neg_add_blocker(prev, p, fs.last(), order);
        let t = neg_add_blockers(s, p, fs, order);
        assert forall|q: TokenId, g: FactId| #[trigger]
            neg_fwd(t, q, g) <==> (neg_fwd(s, q, g) || (q == p && fs.contains(g))) by {
            if fs.contains(g) && g != fs.last() {
                let i = choose|i: int| 0 <= i < fs.len() && fs[i] == g;
                assert(fs.drop_last()[i] == g);
            }
            if fs.drop_last().contains(g) {
                let i = choose|i: int| 0 <= i < fs.drop_last().len() && fs.drop_last()[i] == g;
                assert(fs[i] == g);
            }
            assert(fs[fs.len() - 1] == fs.last());
        }
    }
}

pub proof fn lemma_neg_node_left(n: NegNode, m: JoinRel, p: TokenId, bfs: Seq<FactId>, pt: TokenId)
    requires
        neg_node_inv(n, m),
        neg_left_ok(n, m, p, bfs),
    ensures
        neg_node_inv(neg_left_activate(n, p, bfs, pt).0, m),
        neg_signals_exact(n, neg_left_activate(n, p, bfs, pt).0, m, neg_left_activate(n, p, bfs, pt).1),
{
    let (n2, sig) = neg_left_activate(n, p, bfs, pt);
    assert(!n.mem.blocked.contains_key(p) && !n.mem.unblocked.contains_key(p));
    if bfs.len() == 0 {
        assert forall|q: TokenId, f: FactId| #[trigger]
            neg_fwd(n2.mem, q, f) <==> (n2.parents.contains(q) && n2.facts.contains(f) && m(q, f)) by {
            assert(neg_fwd(n2.mem, q, f) == neg_fwd(n.mem, q, f));
            if q == p && n.facts.contains(f) && m(p, f) {
                assert(bfs.contains(f));
            }
        }
        assert(sig[0] == Signal::Propagate(p, pt));
        assert forall|q: TokenId, x: TokenId| #[trigger]
            sig.contains(Signal::Propagate(q, x)) <==> (!n.mem.unblocked.contains_key(q)
                && n2.mem.unblocked.contains_key(q) && n2.mem.unblocked[q] == x) by {
            if sig.contains(Signal::Propagate(q, x)) {
                let i = choose|i: int| 0 <= i < sig.len() && sig[i] == Signal::Propagate(q, x);
                assert(i == 0);
            }
        }
        assert forall|q: TokenId, x: TokenId| !(#[trigger] sig.contains(Signal::Retract(q, x))) by {
            if sig.contains(Signal::Retract(q, x)) {
                let i = choose|i: int| 0 <= i < sig.len() && sig[i] == Signal::Retract(q, x);
                assert(i == 0);
            }
        }
    } else {
        lemma_neg_add_blockers(n.mem, p, bfs, n.next_order as u64);
        assert forall|q: TokenId, f: FactId| #[trigger]
            neg_fwd(n2.mem, q, f) <==> (n2.parents.contains(q) && n2.facts.contains(f) && m(q, f)) by {
            assert(neg_fwd(n.mem, q, f) <==> (n.parents.contains(q) && n.facts.contains(f) && m(
                q,
                f,
            )));
        }
        assert forall|q: TokenId| #[trigger] n2.mem.unblocked.contains_key(q) implies !n2.mem.blocked.contains_key(
            q,
        ) by {}
    }
    lemma_neg_signals_from_concrete(n, n2, m, sig);
}

// ----------------------------- right activation ------------------------
/// rete.rs:1367-1376: already blocked parents record the new fact too.
pub open spec fn neg_ra_blocked(s: NegMem, f: FactId, ps: Seq<TokenId>) -> NegMem
    decreases ps.len(),
{
    if ps.len() == 0 {
        s
    } else {
        neg_add_blocker(neg_ra_blocked(s, f, ps.drop_last()), ps.last(), f, 0)
    }
}

/// rete.rs:1392-1404: newly blocked parents leave `unblocked`, attach to the
/// fact's block list at a fresh order, and their pass-through is retracted.
pub open spec fn neg_ra_unblocked(s: NegMem, f: FactId, ps: Seq<TokenId>, next: nat) -> (
    NegMem,
    Seq<Signal>,
)
    decreases ps.len(),
{
    if ps.len() == 0 {
        (s, Seq::empty())
    } else {
        let (s1, sig) = neg_ra_unblocked(s, f, ps.drop_last(), next);
        let p = ps.last();
        let order = (next + ps.len() - 1) as u64;  // 1394
        let pt = s1.unblocked[p];  // captured with the parent (1355-1361)
        let s2 = neg_remove_unblocked(s1, p).0;  // 1396
        (neg_add_blocker(s2, p, f, order), sig.push(Signal::Retract(p, pt)))  // 1397, 1403

    }
}

/// rete.rs:1321 `negative_right_activate` for a newly asserted fact `f`:
/// `bps` are the blocked parents it matches, `ups` the unblocked ones.
pub open spec fn neg_right_activate(
    n: NegNode,
    f: FactId,
    bps: Seq<TokenId>,
    ups: Seq<TokenId>,
) -> (NegNode, Seq<Signal>) {
    let s1 = neg_ra_blocked(n.mem, f, bps);
    let (s2, sig) = neg_ra_unblocked(s1, f, ups, n.next_order);
    (NegNode { mem: s2, facts: n.facts.insert(f), next_order: n.next_order + ups.len(), ..n }, sig)
}

pub open spec fn neg_right_ok(
    n: NegNode,
    m: JoinRel,
    f: FactId,
    bps: Seq<TokenId>,
    ups: Seq<TokenId>,
) -> bool {
    &&& !n.facts.contains(f)
    &&& bps.no_duplicates()
    &&& forall|p: TokenId| #[trigger]
        bps.contains(p) <==> (n.mem.blocked.contains_key(p) && m(p, f))
    &&& ups.no_duplicates()
    &&& forall|p: TokenId| #[trigger]
        ups.contains(p) <==> (n.mem.unblocked.contains_key(p) && m(p, f))
    &&& n.next_order + ups.len() <= u64::MAX
}

pub proof fn lemma_seq_contains_drop_last<A>(s: Seq<A>, x: A)
    requires
        s.len() > 0,
    ensures
        s.contains(x) <==> (s.drop_last().contains(x) || x == s.last()),
{
    if s.contains(x) {
        let i = choose|i: int| 0 <= i < s.len() && s[i] == x;
        if i < s.len() - 1 {
            assert(s.drop_last()[i] == x);
        }
    }
    if s.drop_last().contains(x) {
        let i = choose|i: int| 0 <= i < s.drop_last().len() && s.drop_last()[i] == x;
        assert(s[i] == x);
    }
    if x == s.last() {
        assert(s[s.len() - 1] == x);
    }
}

pub proof fn lemma_neg_ra_blocked(s: NegMem, f: FactId, ps: Seq<TokenId>)
    requires
        neg_wf(s),
        forall|p: TokenId| #[trigger] ps.contains(p) ==> s.blocked.contains_key(p),
    ensures
        ({
            let t = neg_ra_blocked(s, f, ps);
            &&& neg_wf(t)
            &&& forall|q: TokenId, g: FactId| #[trigger]
                neg_fwd(t, q, g) <==> (neg_fwd(s, q, g) || (g == f && ps.contains(q)))
            &&& t.unblocked == s.unblocked
            &&& t.blocked.dom() == s.blocked.dom()
            &&& t.block_order == s.block_order
        }),
    decreases ps.len(),
{
    if ps.len() > 0 {
        let prev = neg_ra_blocked(s, f, ps.drop_last());
        assert forall|p: TokenId| #[trigger] ps.drop_last().contains(p) implies s.blocked.contains_key(
            p,
        ) by {
            lemma_seq_contains_drop_last(ps, p);
        }
        lemma_neg_ra_blocked(s, f, ps.drop_last());
        lemma_seq_contains_drop_last(ps, ps.last());
        lemma_neg_add_blocker(prev, ps.last(), f, 0);
        let t = neg_ra_blocked(s, f, ps);
        assert(t.blocked.dom() =~= s.blocked.dom());
        assert forall|q: TokenId, g: FactId| #[trigger]
            neg_fwd(t, q, g) <==> (neg_fwd(s, q, g) || (g == f && ps.contains(q))) by {
            lemma_seq_contains_drop_last(ps, q);
        }
    }
}

pub proof fn lemma_neg_ra_unblocked(s: NegMem, f: FactId, ps: Seq<TokenId>, next: nat)
    requires
        neg_valid(s),
        ps.no_duplicates(),
        forall|p: TokenId| #[trigger] ps.contains(p) ==> s.unblocked.contains_key(p),
        neg_orders_below(s, next as int),
        next + ps.len() <= u64::MAX + 1,
    ensures
        ({
            let (t, sig) = neg_ra_unblocked(s, f, ps, next);
            &&& neg_valid(t)
            &&& forall|q: TokenId, g: FactId| #[trigger]
                neg_fwd(t, q, g) <==> (neg_fwd(s, q, g) || (g == f && ps.contains(q)))
            &&& forall|q: TokenId| #[trigger]
                t.unblocked.contains_key(q) <==> (s.unblocked.contains_key(q) && !ps.contains(q))
            &&& forall|q: TokenId| #[trigger]
                t.unblocked.contains_key(q) ==> t.unblocked[q] == s.unblocked[q]
            &&& forall|q: TokenId| #[trigger]
                t.blocked.contains_key(q) <==> (s.blocked.contains_key(q) || ps.contains(q))
            &&& neg_orders_below(t, next + ps.len() as int)
            &&& sig.len() == ps.len()
            &&& forall|i: int|
                0 <= i < ps.len() ==> #[trigger] sig[i] == Signal::Retract(
                    ps[i],
                    s.unblocked[ps[i]],
                )
        }),
    decreases ps.len(),
{
    if ps.len() > 0 {
        let pl = ps.drop_last();
        let p = ps.last();
        assert(pl.no_duplicates());
        assert forall|q: TokenId| #[trigger] pl.contains(q) implies s.unblocked.contains_key(q) by {
            lemma_seq_contains_drop_last(ps, q);
        }
        lemma_neg_ra_unblocked(s, f, pl, next);
        let (s1, sig1) = neg_ra_unblocked(s, f, pl, next);
        lemma_seq_contains_drop_last(ps, p);
        assert(!pl.contains(p)) by {
            if pl.contains(p) {
                let i = choose|i: int| 0 <= i < pl.len() && pl[i] == p;
                assert(ps[i] == ps[ps.len() - 1]);
            }
        }
        assert(s1.unblocked.contains_key(p));
        assert(!s1.blocked.contains_key(p));
        let order = (next + ps.len() - 1) as u64;
        let s2 = neg_remove_unblocked(s1, p).0;
        assert(forall|q: TokenId, g: FactId| #[trigger] neg_fwd(s2, q, g) == neg_fwd(s1, q, g));
        assert(fresh_order(s2.block_order, order));
        lemma_neg_add_blocker(s2, p, f, order);
        let (t, sig) = neg_ra_unblocked(s, f, ps, next);
        assert forall|q: TokenId, g: FactId| #[trigger]
            neg_fwd(t, q, g) <==> (neg_fwd(s, q, g) || (g == f && ps.contains(q))) by {
            lemma_seq_contains_drop_last(ps, q);
        }
        assert forall|q: TokenId| #[trigger]
            t.unblocked.contains_key(q) <==> (s.unblocked.contains_key(q) && !ps.contains(q)) by {
            lemma_seq_contains_drop_last(ps, q);
        }
        assert forall|q: TokenId| #[trigger]
            t.blocked.contains_key(q) <==> (s.blocked.contains_key(q) || ps.contains(q)) by {
            lemma_seq_contains_drop_last(ps, q);
        }
        assert forall|i: int|
            0 <= i < ps.len() implies #[trigger] sig[i] == Signal::Retract(
            ps[i],
            s.unblocked[ps[i]],
        ) by {
            if i < ps.len() - 1 {
                assert(sig[i] == sig1[i]);
                assert(pl[i] == ps[i]);
            }
        }
    }
}

pub proof fn lemma_neg_node_right(
    n: NegNode,
    m: JoinRel,
    f: FactId,
    bps: Seq<TokenId>,
    ups: Seq<TokenId>,
)
    requires
        neg_node_inv(n, m),
        neg_right_ok(n, m, f, bps, ups),
    ensures
        neg_node_inv(neg_right_activate(n, f, bps, ups).0, m),
        neg_signals_exact(
            n,
            neg_right_activate(n, f, bps, ups).0,
            m,
            neg_right_activate(n, f, bps, ups).1,
        ),
{
    let s1 = neg_ra_blocked(n.mem, f, bps);
    lemma_neg_ra_blocked(n.mem, f, bps);
    assert(neg_disjoint(s1));
    assert forall|p: TokenId| #[trigger] ups.contains(p) implies s1.unblocked.contains_key(p) by {}
    assert(neg_orders_below(s1, n.next_order as int));
    lemma_neg_ra_unblocked(s1, f, ups, n.next_order);
    let (n2, sig) = neg_right_activate(n, f, bps, ups);
    assert forall|q: TokenId, g: FactId| #[trigger]
        neg_fwd(n2.mem, q, g) <==> (n2.parents.contains(q) && n2.facts.contains(g) && m(q, g)) by {
        assert(neg_fwd(n.mem, q, g) <==> (n.parents.contains(q) && n.facts.contains(g) && m(q, g)));
        if g == f && n.parents.contains(q) && m(q, f) {
            assert(n.mem.blocked.contains_key(q) || n.mem.unblocked.contains_key(q));
        }
        if neg_fwd(n.mem, q, g) {
            assert(g != f);
        }
    }
    assert forall|q: TokenId| #[trigger]
        n2.mem.unblocked.contains_key(q) <==> (n2.parents.contains(q)
            && !n2.mem.blocked.contains_key(q)) by {}
    // signals
    assert forall|i: int, j: int|
        0 <= i < sig.len() && 0 <= j < sig.len() && i != j implies sig_parent(#[trigger] sig[i])
        != sig_parent(#[trigger] sig[j]) by {
        assert(sig[i] == Signal::Retract(ups[i], s1.unblocked[ups[i]]));
        assert(sig[j] == Signal::Retract(ups[j], s1.unblocked[ups[j]]));
    }
    assert forall|q: TokenId, x: TokenId| #[trigger]
        sig.contains(Signal::Propagate(q, x)) <==> (!n.mem.unblocked.contains_key(q)
            && n2.mem.unblocked.contains_key(q) && n2.mem.unblocked[q] == x) by {
        if sig.contains(Signal::Propagate(q, x)) {
            let i = choose|i: int| 0 <= i < sig.len() && sig[i] == Signal::Propagate(q, x);
            assert(sig[i] == Signal::Retract(ups[i], s1.unblocked[ups[i]]));
        }
    }
    assert forall|q: TokenId, x: TokenId| #[trigger]
        sig.contains(Signal::Retract(q, x)) <==> (n.mem.unblocked.contains_key(q)
            && !n2.mem.unblocked.contains_key(q) && n2.parents.contains(q) && n.mem.unblocked[q]
            == x) by {
        if sig.contains(Signal::Retract(q, x)) {
            let i = choose|i: int| 0 <= i < sig.len() && sig[i] == Signal::Retract(q, x);
            assert(sig[i] == Signal::Retract(ups[i], s1.unblocked[ups[i]]));
            assert(ups.contains(ups[i]));
        }
        if n.mem.unblocked.contains_key(q) && !n2.mem.unblocked.contains_key(q) {
            assert(ups.contains(q));
            let i = choose|i: int| 0 <= i < ups.len() && ups[i] == q;
            assert(sig[i] == Signal::Retract(q, s1.unblocked[q]));
        }
    }
    lemma_neg_signals_from_concrete(n, n2, m, sig);
}

} // verus!

