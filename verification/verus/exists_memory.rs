//! Verus model of `ExistsMemory` (crates/ferric-rules-core/src/exists.rs).
//!
//! What is proved (unbounded: any number of parents and supporting facts,
//! any history of operations):
//!
//! 1. `new` and `clear` establish `validate_consistency`; `add_support`,
//!    `set_satisfied` (called only with support), `remove_satisfied` and
//!    `remove_parent_token` preserve it; `remove_support` preserves it unless
//!    it reports the N->0 transition for a satisfied parent, and the rete's
//!    reaction to that report (`remove_satisfied`, rete.rs
//!    `exists_handle_retraction`) restores it.
//! 2. Counts: `add_support` returns `|support(p) + {f}|`, `remove_support`
//!    returns `(|support(p) - {f}|, f in support(p))`, `support_count`
//!    returns `|support(p)|` (0 for an untracked parent).
//! 3. Transition signals: the rete's 0->N test `old_count == 0 && new_count > 0`
//!    around `add_support` holds exactly when the support set goes from empty
//!    to non-empty, and the N->0 test `was_removed && new_count == 0` after
//!    `remove_support` holds exactly when it goes from non-empty to empty.
//! 4. `parents_supported_by(f) == { p | f in support(p) }` under the invariant.
//! 5. `remove_parent_token(p)` removes every trace of `p` and leaves every
//!    other parent's support untouched, for every iteration order of the
//!    removed support set.
//!
//! Model / abstractions (the trusted base besides Verus, vstd and Z3):
//!
//! * `FxHashMap<K, V>` is modelled as `Map<K, V>` and `FxHashSet<K>` as
//!   `Set<K>`; `HashSet::len` is `Set::len`. Hashing, capacity and iteration
//!   order are not modelled. The `for fact_id in facts` loop in
//!   `remove_parent_token` iterates a `HashSet` in an unspecified order, so the
//!   model takes that order as an argument (`order`, any duplicate-free
//!   enumeration of the set) and the theorem holds for all of them.
//! * Counts are `nat`; the production `usize` lengths cannot overflow because
//!   they are lengths of in-memory sets.
//! * `TokenId`/`FactId` are `u64`; the `id` field and error strings are omitted.
//!
//! Caller protocol (precondition, enforced by `ReteNetwork`): `set_satisfied`
//! is only called for a parent with non-empty support (rete.rs
//! `exists_left_activate` / `exists_right_activate` call it right after
//! `add_support`).
//!
//! Known gaps: model fidelity is checked by review (each spec function cites
//! its production function and mirrors its branches), not mechanically. The
//! bounded Kani harnesses in crates/ferric-rules-core/src/exists/kani_proofs.rs
//! exercise the production code itself.
use vstd::prelude::*;

verus! {

/// Models `TokenId` (crates/ferric-rules-core/src/token.rs:18).
pub type Tok = u64;

/// Models `FactId` (crates/ferric-rules-core/src/fact.rs:172).
pub type Fct = u64;

/// Models `struct ExistsMemory` (exists.rs:37), without the `id` field.
pub struct ExMem {
    /// `support: HashMap<TokenId, HashSet<FactId>>`
    pub support: Map<Tok, Set<Fct>>,
    /// `satisfied: HashMap<TokenId, TokenId>`
    pub satisfied: Map<Tok, Tok>,
    /// `fact_to_parents: HashMap<FactId, HashSet<TokenId>>`
    pub fact_to_parents: Map<Fct, Set<Tok>>,
}

/// Models `ExistsMemory::validate_consistency` (exists.rs:189); one conjunct
/// per `require!`.
pub open spec fn validate_consistency(m: ExMem) -> bool {
    // exists.rs:192-196
    &&& forall|p: Tok| #[trigger] m.support.dom().contains(p) ==> m.support[p].len() > 0
    // exists.rs:197-204
    &&& forall|p: Tok, f: Fct|
        #![trigger m.support[p].contains(f)]
        m.support.dom().contains(p) && m.support[p].contains(f) ==> m.fact_to_parents.dom().contains(
            f,
        ) && m.fact_to_parents[f].contains(p)
    // exists.rs:208-212
    &&& forall|f: Fct| #[trigger]
        m.fact_to_parents.dom().contains(f) ==> m.fact_to_parents[f].len() > 0
    // exists.rs:213-220
    &&& forall|f: Fct, p: Tok|
        #![trigger m.fact_to_parents[f].contains(p)]
        m.fact_to_parents.dom().contains(f) && m.fact_to_parents[f].contains(p)
            ==> m.support.dom().contains(p) && m.support[p].contains(f)
    // exists.rs:224-230
    &&& forall|p: Tok| #[trigger] m.satisfied.dom().contains(p) ==> support_count(m, p) > 0
}

/// Abstract support set of a parent (empty when untracked).
pub open spec fn support_of(m: ExMem, p: Tok) -> Set<Fct> {
    if m.support.dom().contains(p) {
        m.support[p]
    } else {
        Set::empty()
    }
}

// ---------------------------------------------------------------------------
// Operations
// ---------------------------------------------------------------------------
/// Models `ExistsMemory::new` (exists.rs:59).
pub open spec fn new_mem() -> ExMem {
    ExMem { support: Map::empty(), satisfied: Map::empty(), fact_to_parents: Map::empty() }
}

/// Models `ExistsMemory::clear` (exists.rs:168).
pub open spec fn clear(m: ExMem) -> ExMem {
    new_mem()
}

/// Models `ExistsMemory::support_count` (exists.rs:111).
pub open spec fn support_count(m: ExMem, p: Tok) -> nat {
    if m.support.dom().contains(p) {
        m.support[p].len()
    } else {
        0
    }
}

/// Models `ExistsMemory::add_support` (exists.rs:71); returns the new state and
/// the returned count.
pub open spec fn add_support(m: ExMem, p: Tok, f: Fct) -> (ExMem, nat) {
    // self.support.entry(parent_token_id).or_default().insert(fact_id)
    let facts = if m.support.dom().contains(p) {
        m.support[p]
    } else {
        Set::<Fct>::empty()
    };
    let support = m.support.insert(p, facts.insert(f));
    // self.fact_to_parents.entry(fact_id).or_default().insert(parent_token_id)
    let parents = if m.fact_to_parents.dom().contains(f) {
        m.fact_to_parents[f]
    } else {
        Set::<Tok>::empty()
    };
    let fact_to_parents = m.fact_to_parents.insert(f, parents.insert(p));
    let r = ExMem { support, fact_to_parents, satisfied: m.satisfied };
    // self.support.get(&parent_token_id).map_or(0, HashSet::len)
    (r, support_count(r, p))
}

/// The reverse-index update in `remove_support` (exists.rs:96-101) and in the
/// loop body of `remove_parent_token` (exists.rs:157-162).
pub open spec fn purge_one(f2p: Map<Fct, Set<Tok>>, p: Tok, f: Fct) -> Map<Fct, Set<Tok>> {
    if f2p.dom().contains(f) {
        let parents = f2p[f].remove(p);
        if parents.len() == 0 {
            f2p.remove(f)
        } else {
            f2p.insert(f, parents)
        }
    } else {
        f2p
    }
}

/// Models `ExistsMemory::remove_support` (exists.rs:86); returns the new state,
/// `new_count` and `was_removed`.
pub open spec fn remove_support(m: ExMem, p: Tok, f: Fct) -> (ExMem, nat, bool) {
    // if let Some(facts) = self.support.get_mut(&parent_token_id)
    let (support, was_removed) = if m.support.dom().contains(p) {
        let was_removed = m.support[p].contains(f);
        let facts = m.support[p].remove(f);
        if facts.len() == 0 {
            (m.support.remove(p), was_removed)
        } else {
            (m.support.insert(p, facts), was_removed)
        }
    } else {
        (m.support, false)
    };
    let fact_to_parents = purge_one(m.fact_to_parents, p, f);
    let r = ExMem { support, fact_to_parents, satisfied: m.satisfied };
    (r, support_count(r, p), was_removed)
}

/// Models `ExistsMemory::set_satisfied` (exists.rs:126).
pub open spec fn set_satisfied(m: ExMem, p: Tok, pt: Tok) -> ExMem {
    ExMem { satisfied: m.satisfied.insert(p, pt), ..m }
}

/// Models `ExistsMemory::remove_satisfied` (exists.rs:139).
pub open spec fn remove_satisfied(m: ExMem, p: Tok) -> (ExMem, Option<Tok>) {
    let old = if m.satisfied.dom().contains(p) {
        Some(m.satisfied[p])
    } else {
        None
    };
    (ExMem { satisfied: m.satisfied.remove(p), ..m }, old)
}

/// Models the `for fact_id in facts` loop of `remove_parent_token`
/// (exists.rs:156-163), visiting `order[i..]` front to back.
pub open spec fn purge_reverse(f2p: Map<Fct, Set<Tok>>, p: Tok, order: Seq<Fct>, i: int) -> Map<
    Fct,
    Set<Tok>,
>
    decreases order.len() - i,
{
    if i < 0 || i >= order.len() {
        f2p
    } else {
        purge_reverse(purge_one(f2p, p, order[i]), p, order, i + 1)
    }
}

/// `order` is one possible iteration order of the support set of `p`.
pub open spec fn is_iteration_order(m: ExMem, p: Tok, order: Seq<Fct>) -> bool {
    order.no_duplicates() && forall|f: Fct| #[trigger] order.contains(f) <==> support_of(m, p).contains(f)
}

/// Models `ExistsMemory::remove_parent_token` (exists.rs:150); `order` is the
/// (unspecified) iteration order of the removed `HashSet`.
pub open spec fn remove_parent_token(m: ExMem, p: Tok, order: Seq<Fct>) -> ExMem {
    let satisfied = m.satisfied.remove(p);
    let (support, fact_to_parents) = if m.support.dom().contains(p) {
        (m.support.remove(p), purge_reverse(m.fact_to_parents, p, order, 0))
    } else {
        (m.support, m.fact_to_parents)
    };
    ExMem { support, satisfied, fact_to_parents }
}

// ---------------------------------------------------------------------------
// Proofs
// ---------------------------------------------------------------------------
pub proof fn lemma_purge_one(f2p: Map<Fct, Set<Tok>>, p: Tok, f: Fct)
    ensures
        forall|g: Fct|
            g != f ==> (#[trigger] purge_one(f2p, p, f).dom().contains(g) <==> f2p.dom().contains(
                g,
            )),
        forall|g: Fct| g != f && f2p.dom().contains(g) ==> #[trigger] purge_one(f2p, p, f)[g] == f2p[g],
        purge_one(f2p, p, f).dom().contains(f) <==> (f2p.dom().contains(f) && f2p[f].remove(p).len()
            > 0),
        purge_one(f2p, p, f).dom().contains(f) ==> purge_one(f2p, p, f)[f] == f2p[f].remove(p),
{
}

pub proof fn lemma_purge_reverse(f2p: Map<Fct, Set<Tok>>, p: Tok, order: Seq<Fct>, i: int)
    requires
        0 <= i <= order.len(),
        order.no_duplicates(),
    ensures
        forall|g: Fct|
            !(#[trigger] order.subrange(i, order.len() as int).contains(g)) ==> (purge_reverse(
                f2p,
                p,
                order,
                i,
            ).dom().contains(g) <==> f2p.dom().contains(g)),
        forall|g: Fct|
            !(#[trigger] order.subrange(i, order.len() as int).contains(g)) && f2p.dom().contains(g)
                ==> purge_reverse(f2p, p, order, i)[g] == f2p[g],
        forall|g: Fct| #[trigger]
            order.subrange(i, order.len() as int).contains(g) ==> (purge_reverse(
                f2p,
                p,
                order,
                i,
            ).dom().contains(g) <==> (f2p.dom().contains(g) && f2p[g].remove(p).len() > 0)),
        forall|g: Fct| #[trigger]
            order.subrange(i, order.len() as int).contains(g) && purge_reverse(
                f2p,
                p,
                order,
                i,
            ).dom().contains(g) ==> purge_reverse(f2p, p, order, i)[g] == f2p[g].remove(p),
    decreases order.len() - i,
{
    let suffix = order.subrange(i, order.len() as int);
    if i < order.len() {
        let f = order[i];
        let next = purge_one(f2p, p, f);
        lemma_purge_one(f2p, p, f);
        lemma_purge_reverse(next, p, order, i + 1);
        let rest = order.subrange(i + 1, order.len() as int);
        assert(suffix[0] == f);
        assert forall|g: Fct| suffix.contains(g) <==> (g == f || rest.contains(g)) by {
            if suffix.contains(g) {
                let j = choose|j: int| 0 <= j < suffix.len() && suffix[j] == g;
                if j > 0 {
                    assert(rest[j - 1] == g);
                }
            }
            if rest.contains(g) {
                let j = choose|j: int| 0 <= j < rest.len() && rest[j] == g;
                assert(suffix[j + 1] == g);
            }
        }
        assert(!rest.contains(f)) by {
            if rest.contains(f) {
                let j = choose|j: int| 0 <= j < rest.len() && rest[j] == f;
                assert(order[i + 1 + j] == f);
            }
        }
    } else {
        assert(suffix.len() == 0);
    }
}

pub proof fn theorem_new_and_clear_valid(m: ExMem)
    ensures
        validate_consistency(new_mem()),
        validate_consistency(clear(m)),
{
}

/// `parents_supported_by` (exists.rs:118) answers exactly from the reverse index.
pub proof fn theorem_parents_supported_by_exact(m: ExMem, f: Fct, p: Tok)
    requires
        validate_consistency(m),
    ensures
        (m.fact_to_parents.dom().contains(f) && m.fact_to_parents[f].contains(p)) <==> support_of(
            m,
            p,
        ).contains(f),
{
}

pub proof fn theorem_add_support(m: ExMem, p: Tok, f: Fct)
    requires
        validate_consistency(m),
    ensures
        ({
            let (r, new_count) = add_support(m, p, f);
            let old_count = support_count(m, p);
            &&& validate_consistency(r)
            &&& support_of(r, p) == support_of(m, p).insert(f)
            &&& forall|q: Tok| q != p ==> #[trigger] support_of(r, q) == support_of(m, q)
            &&& r.satisfied == m.satisfied
            &&& new_count == support_of(r, p).len()
            &&& new_count == old_count + (if support_of(m, p).contains(f) {
                0nat
            } else {
                1nat
            })
            // the rete's 0->N test is exactly the empty -> non-empty transition
            &&& (old_count == 0 && new_count > 0) == (support_of(m, p).len() == 0 && support_of(
                r,
                p,
            ).len() > 0)
            &&& new_count > 0
        }),
{
    let (r, new_count) = add_support(m, p, f);
    assert(support_of(r, p) =~= support_of(m, p).insert(f));
    assert forall|q: Tok, g: Fct| r.support.dom().contains(q) && #[trigger] r.support[q].contains(
        g,
    ) implies r.fact_to_parents.dom().contains(g) && r.fact_to_parents[g].contains(q) by {
        if q != p || g != f {
            if q == p {
                assert(m.support[q].contains(g));
            } else {
                assert(m.support[q].contains(g));
            }
        }
    }
    assert forall|g: Fct, q: Tok|
        r.fact_to_parents.dom().contains(g) && #[trigger] r.fact_to_parents[g].contains(
            q,
        ) implies r.support.dom().contains(q) && r.support[q].contains(g) by {
        if g != f || q != p {
            assert(m.fact_to_parents[g].contains(q));
        }
    }
    assert forall|g: Fct| #[trigger] r.fact_to_parents.dom().contains(g) implies r.fact_to_parents[g].len()
        > 0 by {
        if g == f {
            assert(r.fact_to_parents[g].contains(p));
        }
    }
    assert forall|q: Tok| #[trigger] r.support.dom().contains(q) implies r.support[q].len() > 0 by {
        if q == p {
            assert(r.support[q].contains(f));
        }
    }
    assert forall|q: Tok| #[trigger] r.satisfied.dom().contains(q) implies support_count(r, q)
        > 0 by {
        if q == p {
            assert(r.support[q].contains(f));
        }
    }
    assert(support_of(r, p).contains(f));
}

pub proof fn theorem_remove_support(m: ExMem, p: Tok, f: Fct)
    requires
        validate_consistency(m),
    ensures
        ({
            let (r, new_count, was_removed) = remove_support(m, p, f);
            &&& support_of(r, p) == support_of(m, p).remove(f)
            &&& forall|q: Tok| q != p ==> #[trigger] support_of(r, q) == support_of(m, q)
            &&& r.satisfied == m.satisfied
            &&& new_count == support_of(r, p).len()
            &&& was_removed == support_of(m, p).contains(f)
            // the rete's N->0 test is exactly the non-empty -> empty transition
            &&& (was_removed && new_count == 0) == (support_of(m, p).len() > 0 && support_of(
                r,
                p,
            ).len() == 0)
            // the invariant survives unless a satisfied parent lost its last support ...
            &&& !(was_removed && new_count == 0 && m.satisfied.dom().contains(p))
                ==> validate_consistency(r)
            // ... and the rete's reaction (remove_satisfied) restores it in every case
            &&& validate_consistency(remove_satisfied(r, p).0)
        }),
{
    let (r, new_count, was_removed) = remove_support(m, p, f);
    lemma_purge_one(m.fact_to_parents, p, f);
    assert(support_of(r, p) =~= support_of(m, p).remove(f));
    assert forall|q: Tok, g: Fct| r.support.dom().contains(q) && #[trigger] r.support[q].contains(
        g,
    ) implies r.fact_to_parents.dom().contains(g) && r.fact_to_parents[g].contains(q) by {
        assert(m.support[q].contains(g));
        assert(m.fact_to_parents[g].contains(q));
        if g == f {
            assert(m.fact_to_parents[g].remove(p).contains(q));
        }
    }
    assert forall|g: Fct, q: Tok|
        r.fact_to_parents.dom().contains(g) && #[trigger] r.fact_to_parents[g].contains(
            q,
        ) implies r.support.dom().contains(q) && r.support[q].contains(g) by {
        assert(m.fact_to_parents[g].contains(q));
        if q == p {
            assert(m.support[p].remove(f).contains(g));
        }
    }
    if m.support.dom().contains(p) && m.support[p].contains(f) && m.support[p].remove(f).len()
        == 0 {
        assert(support_of(m, p).len() > 0);
    }
    let r2 = remove_satisfied(r, p).0;
    assert forall|q: Tok| #[trigger] r2.satisfied.dom().contains(q) implies support_count(r2, q)
        > 0 by {
        assert(m.satisfied.dom().contains(q));
    }
    if !(was_removed && new_count == 0 && m.satisfied.dom().contains(p)) {
        assert forall|q: Tok| #[trigger] r.satisfied.dom().contains(q) implies support_count(r, q)
            > 0 by {
            if q == p && !was_removed {
                assert(support_of(r, p) =~= support_of(m, p));
            }
        }
    }
}

pub proof fn theorem_set_satisfied(m: ExMem, p: Tok, pt: Tok)
    requires
        validate_consistency(m),
        support_count(m, p) > 0,
    ensures
        validate_consistency(set_satisfied(m, p, pt)),
        set_satisfied(m, p, pt).satisfied[p] == pt,
{
}

pub proof fn theorem_remove_satisfied(m: ExMem, p: Tok)
    requires
        validate_consistency(m),
    ensures
        validate_consistency(remove_satisfied(m, p).0),
        !remove_satisfied(m, p).0.satisfied.dom().contains(p),
        remove_satisfied(m, p).1 == (if m.satisfied.dom().contains(p) {
            Some(m.satisfied[p])
        } else {
            None
        }),
{
}

pub proof fn theorem_remove_parent_token(m: ExMem, p: Tok, order: Seq<Fct>)
    requires
        validate_consistency(m),
        is_iteration_order(m, p, order),
    ensures
        ({
            let r = remove_parent_token(m, p, order);
            &&& validate_consistency(r)
            &&& support_count(r, p) == 0
            &&& !r.satisfied.dom().contains(p)
            &&& forall|f: Fct| #[trigger]
                r.fact_to_parents.dom().contains(f) ==> !r.fact_to_parents[f].contains(p)
            &&& forall|q: Tok| q != p ==> #[trigger] support_of(r, q) == support_of(m, q)
        }),
{
    let r = remove_parent_token(m, p, order);
    if m.support.dom().contains(p) {
        lemma_purge_reverse(m.fact_to_parents, p, order, 0);
    }
    assert(order.subrange(0, order.len() as int) =~= order);
    assert forall|g: Fct| #[trigger] r.fact_to_parents.dom().contains(g) implies m.fact_to_parents.dom().contains(
        g,
    ) && (r.fact_to_parents[g] == m.fact_to_parents[g] || r.fact_to_parents[g]
        == m.fact_to_parents[g].remove(p)) && !r.fact_to_parents[g].contains(p)
        && r.fact_to_parents[g].len() > 0 by {
        assert(order.subrange(0, order.len() as int).contains(g) == order.contains(g));
        if m.support.dom().contains(p) && !order.contains(g) && m.fact_to_parents[g].contains(p) {
            assert(support_of(m, p).contains(g));
        }
    }
    assert forall|q: Tok, g: Fct| r.support.dom().contains(q) && #[trigger] r.support[q].contains(
        g,
    ) implies r.fact_to_parents.dom().contains(g) && r.fact_to_parents[g].contains(q) by {
        assert(m.fact_to_parents[g].contains(q));
        assert(order.subrange(0, order.len() as int).contains(g) == order.contains(g));
        if m.support.dom().contains(p) && order.contains(g) {
            assert(m.fact_to_parents[g].remove(p).contains(q));
        }
    }
    assert forall|g: Fct, q: Tok|
        r.fact_to_parents.dom().contains(g) && #[trigger] r.fact_to_parents[g].contains(
            q,
        ) implies r.support.dom().contains(q) && r.support[q].contains(g) by {
        assert(m.fact_to_parents[g].contains(q));
    }
    assert forall|q: Tok| q != p implies #[trigger] support_of(r, q) == support_of(m, q) by {}
}

} // verus!
