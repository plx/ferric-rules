//! Verus model of `NccMemory` (crates/ferric-rules-core/src/ncc.rs).
//!
//! What is proved (unbounded: any number of parents and subnetwork results,
//! any history of operations):
//!
//! 1. `new` and `clear` establish `validate_consistency`; `remove_result`,
//!    `remove_unblocked`, `remove_parent_token` and `set_unblocked` (for a
//!    parent without results) preserve it; `add_result` preserves it unless it
//!    reports the 0->1 transition for a parent that still has a pass-through,
//!    and the rete's reaction to that report (`remove_unblocked`, rete.rs
//!    `ncc_partner_receive_result`) restores it. The two-phase cascade
//!    protocol is covered too: `remove_result_support` of a non-primary result
//!    preserves the invariant, and of a primary result followed by
//!    `reassign_primary` restores it.
//! 2. Counts: `result_count(p)` always equals the number of distinct results
//!    owned by `p`; `add_result` never overflows the `usize` count (given the
//!    physical bound `|result_owner| < usize::MAX`), and the decrement guard
//!    `if *count > 0` is always taken for an owned result (no hidden underflow).
//! 3. Transition signals: `add_result`'s `(old, new) == (0, 1)` holds exactly
//!    when the parent's result set goes from empty to non-empty, and
//!    `remove_result` returning `Some((p, 0))` holds exactly when it goes from
//!    non-empty to empty. Re-adding a tracked result changes nothing.
//! 4. Chronology: the primary result is always the oldest surviving result;
//!    the block order is set by the first result, replaced by
//!    `replacement_order` exactly when the primary is removed with survivors,
//!    and preserved otherwise. With fresh orders, block orders stay unique.
//! 5. `remove_parent_token(p)` removes every trace of `p` (count, results,
//!    owners pointing at `p`, primary, chronology, pass-through) and leaves the
//!    other parents untouched.
//! 6. Batched cascades (several pending primaries at once). `cascade_inv(m, P)`
//!    describes the state between rete.rs `collect_ncc_result_retractions`
//!    and the `apply_ncc_result_retraction` calls: consistent except that each
//!    parent in `P` still records a stale primary (no longer a tracked result)
//!    and its block order. A consistent memory satisfies it with `P` empty and
//!    `P` empty implies `validate_consistency`. `remove_result_support`
//!    preserves it, adding the parent to `P` exactly when the removed result
//!    was its recorded primary (at most once per parent per batch), and leaves
//!    all primaries and block orders untouched, so each recorded transition's
//!    guard still holds when it is applied. `reassign_primary` (true exactly
//!    when results survive, oldest survivor becomes primary at the fresh
//!    order) and `remove_parent_token` each settle one pending parent without
//!    touching the others; `set_unblocked` of a settled, unblocked parent
//!    preserves it. `theorem_cascade_batch` composes these: phase one over any
//!    sequence of removed results followed by settling every recorded parent
//!    once, in any order, by either operation, restores `validate_consistency`.
//!
//! Model / abstractions (the trusted base besides Verus, vstd and Z3):
//!
//! * `FxHashMap<K, V>` is modelled as `Map<K, V>`; `OrderedSet<K>` as a
//!   duplicate-free `Seq<K>` in insertion order (the refinement that
//!   `ordered_set` is expected to satisfy; not proved here). Hashing, capacity
//!   and map iteration order are not modelled; the only loop over an ordered
//!   set (`remove_parent_token`) is modelled in its iteration order.
//! * The counting check of `validate_consistency` (ncc.rs:335-354), which
//!   builds a temporary per-parent histogram, is modelled as the cardinality of
//!   the owner preimage `{r | result_owner[r] == p}`.
//! * Counts are `usize` as in production; `+ 1` is modelled as an exact
//!   addition whose absence of overflow is proved.
//! * `TokenId` is `u64`; the `id` field and error strings are omitted.
//!
//! Caller protocol (preconditions, enforced by `ReteNetwork`):
//! * `set_unblocked(p, ..)` only when `result_count(p) == 0` (rete.rs
//!   `ncc_left_activate`).
//! * `order` / `replacement_order` are fresh when they become a parent's
//!   block order (rete.rs `allocate_block_order`); only needed for the
//!   uniqueness theorem, which the rete validates network-wide.
//!
//! Known gaps: model fidelity is checked by review (each spec function cites
//! its production function and mirrors its branches), not mechanically.
//! `increment_results`/`decrement_results` are `pub`, but are modelled only as
//! used by `add_result`/`remove_result_support` (they have no other callers).
//! The batch theorem covers one memory and no activity interleaved with the
//! settlement: in the rete, settling an unblocked parent runs
//! `ncc_left_activate`, whose propagation may call `add_result` on a pending
//! parent before its own transition is applied; that interleaving is not
//! covered. The spec functions `collect` and `settle` model the rete loops
//! (rete.rs:1947 and rete.rs:1994); their fidelity is checked by review.
//! The bounded Kani harnesses in crates/ferric-rules-core/src/ncc/kani_proofs.rs
//! exercise the production code itself.
use vstd::prelude::*;

verus! {

/// Models `TokenId` (crates/ferric-rules-core/src/token.rs:18).
pub type Tok = u64;

/// Models `OrderedSet::insert` (ordered_set.rs:36).
pub open spec fn os_insert<K>(s: Seq<K>, k: K) -> Seq<K> {
    if s.contains(k) {
        s
    } else {
        s.push(k)
    }
}

/// Models `OrderedSet::remove` (ordered_set.rs:96).
pub open spec fn os_remove<K>(s: Seq<K>, k: K) -> Seq<K> {
    if s.contains(k) {
        s.remove(s.index_of(k))
    } else {
        s
    }
}

pub proof fn lemma_os_insert<K>(s: Seq<K>, k: K)
    requires
        s.no_duplicates(),
    ensures
        os_insert(s, k).no_duplicates(),
        forall|x: K| #[trigger] os_insert(s, k).contains(x) <==> (s.contains(x) || x == k),
        os_insert(s, k).len() == s.len() + (if s.contains(k) {
            0int
        } else {
            1
        }),
        s.len() > 0 ==> os_insert(s, k)[0] == s[0],
        s.len() == 0 ==> os_insert(s, k)[0] == k,
{
    let r = os_insert(s, k);
    if !s.contains(k) {
        assert forall|x: K| r.contains(x) <==> (s.contains(x) || x == k) by {
            if r.contains(x) {
                let j = choose|j: int| 0 <= j < r.len() && r[j] == x;
                if j < s.len() {
                    assert(s[j] == x);
                }
            }
            if s.contains(x) {
                let j = choose|j: int| 0 <= j < s.len() && s[j] == x;
                assert(r[j] == x);
            }
            if x == k {
                assert(r[s.len() as int] == x);
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
        os_remove(s, k).len() == (if s.contains(k) {
            s.len() - 1
        } else {
            s.len() as int
        }),
        s.len() > 0 && s[0] != k ==> os_remove(s, k)[0] == s[0],
        s.len() > 1 && s[0] == k ==> os_remove(s, k)[0] == s[1],
{
    let r = os_remove(s, k);
    if s.contains(k) {
        let i = s.index_of(k);
        assert(0 <= i < s.len() && s[i] == k);
        s.remove_ensures(i);
        assert forall|x: K| r.contains(x) <==> (s.contains(x) && x != k) by {
            if r.contains(x) {
                let j = choose|j: int| 0 <= j < r.len() && r[j] == x;
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
                    assert(r[j] == x);
                } else {
                    assert(r[j - 1] == x);
                }
            }
        }
        assert forall|a: int, b: int| 0 <= a < r.len() && 0 <= b < r.len() && a != b implies r[a]
            != r[b] by {
            let sa = if a < i {
                a
            } else {
                a + 1
            };
            let sb = if b < i {
                b
            } else {
                b + 1
            };
            assert(r[a] == s[sa]);
            assert(r[b] == s[sb]);
        }
        if s.len() > 0 && s[0] != k {
            assert(i != 0);
        }
        if s.len() > 1 && s[0] == k {
            assert(i == 0);
        }
    }
}

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------
/// Models `struct NccMemory` (ncc.rs:39), without the `id` field.
pub struct NccMem {
    /// `result_count: HashMap<TokenId, usize>`
    pub result_count: Map<Tok, usize>,
    /// `result_owner: HashMap<TokenId, TokenId>`
    pub result_owner: Map<Tok, Tok>,
    /// `parent_results: HashMap<TokenId, OrderedSet<TokenId>>`
    pub parent_results: Map<Tok, Seq<Tok>>,
    /// `primary_result: HashMap<TokenId, TokenId>`
    pub primary_result: Map<Tok, Tok>,
    /// `block_order: HashMap<TokenId, u64>`
    pub block_order: Map<Tok, u64>,
    /// `unblocked: HashMap<TokenId, TokenId>`
    pub unblocked: Map<Tok, Tok>,
}

/// Representation fact guaranteed by `OrderedSet`: no duplicates.
pub open spec fn wf(m: NccMem) -> bool {
    forall|p: Tok| #[trigger] m.parent_results.dom().contains(p) ==> m.parent_results[p].no_duplicates()
}

/// `{ r | result_owner[r] == p }`, whose size the histogram in
/// ncc.rs:335-354 computes.
pub open spec fn owner_preimage(m: NccMem, p: Tok) -> Set<Tok> {
    m.result_owner.dom().filter(|r: Tok| m.result_owner[r] == p)
}

/// Models `NccMemory::validate_consistency` (ncc.rs:274); one conjunct per check.
pub open spec fn validate_consistency(m: NccMem) -> bool {
    // ncc.rs:275-289: index lengths agree
    &&& m.result_count.len() == m.parent_results.len()
    &&& m.result_count.len() == m.primary_result.len()
    &&& m.result_count.len() == m.block_order.len()
    // ncc.rs:290-315: per-parent checks
    &&& forall|p: Tok| #[trigger]
        m.result_count.dom().contains(p) ==> {
            &&& m.parent_results.dom().contains(p)
            &&& m.parent_results[p].len() == m.result_count[p]
            &&& m.primary_result.dom().contains(p)
            &&& m.parent_results[p].contains(m.primary_result[p])
            // `results.iter().next() == Some(primary)`
            &&& m.parent_results[p].len() > 0 && m.parent_results[p][0] == m.primary_result[p]
            &&& m.block_order.dom().contains(p)
        }
    &&& forall|p: Tok, r: Tok|
        #![trigger m.parent_results[p].contains(r)]
        m.result_count.dom().contains(p) && m.parent_results[p].contains(r)
            ==> m.result_owner.dom().contains(r) && m.result_owner[r] == p
    // ncc.rs:317-323: no parent is both blocked and unblocked
    &&& forall|p: Tok| #[trigger] m.unblocked.dom().contains(p) ==> !m.result_count.dom().contains(p)
    // ncc.rs:326-332: stored counts are positive
    &&& forall|p: Tok| #[trigger] m.result_count.dom().contains(p) ==> m.result_count[p] > 0
    // ncc.rs:336-344: owners have counts
    &&& forall|r: Tok| #[trigger]
        m.result_owner.dom().contains(r) ==> m.result_count.dom().contains(m.result_owner[r])
    // ncc.rs:347-354: counts match tracked results
    &&& forall|p: Tok| #[trigger]
        m.result_count.dom().contains(p) ==> owner_preimage(m, p).len() == m.result_count[p]
}

/// A form of the invariant that is easier to maintain: the counting check is
/// replaced by "every owned result is listed under its owner".
pub open spec fn strong(m: NccMem) -> bool {
    &&& wf(m)
    &&& m.result_count.dom() =~= m.parent_results.dom()
    &&& m.result_count.dom() =~= m.primary_result.dom()
    &&& m.result_count.dom() =~= m.block_order.dom()
    &&& forall|p: Tok| #[trigger]
        m.result_count.dom().contains(p) ==> {
            &&& m.parent_results[p].len() == m.result_count[p]
            &&& m.parent_results[p].len() > 0 && m.parent_results[p][0] == m.primary_result[p]
        }
    &&& forall|p: Tok, r: Tok|
        #![trigger m.parent_results[p].contains(r)]
        m.result_count.dom().contains(p) && m.parent_results[p].contains(r)
            ==> m.result_owner.dom().contains(r) && m.result_owner[r] == p
    &&& forall|r: Tok| #[trigger]
        m.result_owner.dom().contains(r) ==> m.result_count.dom().contains(m.result_owner[r])
            && m.parent_results[m.result_owner[r]].contains(r)
    &&& forall|p: Tok| #[trigger] m.unblocked.dom().contains(p) ==> !m.result_count.dom().contains(p)
    &&& forall|p: Tok| #[trigger] m.result_count.dom().contains(p) ==> m.result_count[p] > 0
}

/// The abstract result set of a parent.
pub open spec fn results(m: NccMem, p: Tok) -> Set<Tok> {
    if m.parent_results.dom().contains(p) {
        m.parent_results[p].to_set()
    } else {
        Set::empty()
    }
}

/// Block orders are unique within the memory (checked network-wide by
/// rete.rs `validate_block_orders`).
pub open spec fn orders_unique(m: NccMem) -> bool {
    forall|p: Tok, q: Tok|
        #![trigger m.block_order[p], m.block_order[q]]
        m.block_order.dom().contains(p) && m.block_order.dom().contains(q) && p != q
            ==> m.block_order[p] != m.block_order[q]
}

pub open spec fn fresh_order(m: NccMem, p: Tok, order: u64) -> bool {
    forall|q: Tok| #[trigger] m.block_order.dom().contains(q) && q != p ==> m.block_order[q] != order
}

// ---------------------------------------------------------------------------
// Operations
// ---------------------------------------------------------------------------
/// Models `NccMemory::new` (ncc.rs:64).
pub open spec fn new_mem() -> NccMem {
    NccMem {
        result_count: Map::empty(),
        result_owner: Map::empty(),
        parent_results: Map::empty(),
        primary_result: Map::empty(),
        block_order: Map::empty(),
        unblocked: Map::empty(),
    }
}

/// Models `NccMemory::clear` (ncc.rs:245).
pub open spec fn clear(m: NccMem) -> NccMem {
    new_mem()
}

/// Models `NccMemory::result_count` (ncc.rs:198).
pub open spec fn result_count(m: NccMem, p: Tok) -> usize {
    if m.result_count.dom().contains(p) {
        m.result_count[p]
    } else {
        0
    }
}

/// Models `NccMemory::increment_results` (ncc.rs:79):
/// `*entry(p).or_insert(0) += 1`. The `+ 1` is exact when
/// `result_count(m, p) < usize::MAX`, which `theorem_add_result` proves.
pub open spec fn increment_results(m: NccMem, p: Tok) -> (NccMem, usize) {
    let count = (result_count(m, p) + 1) as usize;
    (NccMem { result_count: m.result_count.insert(p, count), ..m }, count)
}

/// Models `NccMemory::add_result` (ncc.rs:91); returns the new state and
/// `(old_count, new_count)`.
pub open spec fn add_result(m: NccMem, p: Tok, r: Tok, order: u64) -> (NccMem, usize, usize) {
    if m.result_owner.dom().contains(r) {
        let current = result_count(m, m.result_owner[r]);
        (m, current, current)
    } else {
        let old_count = result_count(m, p);
        let (m1, new_count) = increment_results(m, p);
        let result_owner = m1.result_owner.insert(r, p);
        // self.parent_results.entry(parent_token_id).or_default().insert(result_token_id)
        let results = if m1.parent_results.dom().contains(p) {
            m1.parent_results[p]
        } else {
            Seq::<Tok>::empty()
        };
        let parent_results = m1.parent_results.insert(p, os_insert(results, r));
        let (primary_result, block_order) = if old_count == 0 {
            (m1.primary_result.insert(p, r), m1.block_order.insert(p, order))
        } else {
            (m1.primary_result, m1.block_order)
        };
        (
            NccMem { result_owner, parent_results, primary_result, block_order, ..m1 },
            old_count,
            new_count,
        )
    }
}

/// Models `NccMemory::decrement_results` (ncc.rs:119).
pub open spec fn decrement_results(m: NccMem, p: Tok) -> (NccMem, usize) {
    if m.result_count.dom().contains(p) {
        let count = m.result_count[p];
        // if *count > 0 { *count -= 1; }
        let new_count: usize = if count > 0 {
            (count - 1) as usize
        } else {
            count
        };
        if new_count == 0 {
            (NccMem { result_count: m.result_count.remove(p), ..m }, new_count)
        } else {
            (NccMem { result_count: m.result_count.insert(p, new_count), ..m }, new_count)
        }
    } else {
        (m, 0)
    }
}

/// Models `NccMemory::remove_result_support` (ncc.rs:153).
pub open spec fn remove_result_support(m: NccMem, r: Tok) -> (NccMem, Option<(Tok, usize)>) {
    if !m.result_owner.dom().contains(r) {
        (m, None)
    } else {
        let p = m.result_owner[r];
        let result_owner = m.result_owner.remove(r);
        let parent_results = if m.parent_results.dom().contains(p) {
            let remaining = os_remove(m.parent_results[p], r);
            if remaining.len() == 0 {
                m.parent_results.remove(p)
            } else {
                m.parent_results.insert(p, remaining)
            }
        } else {
            m.parent_results
        };
        let (m2, new_count) = decrement_results(NccMem { result_owner, parent_results, ..m }, p);
        (m2, Some((p, new_count)))
    }
}

/// Models `NccMemory::reassign_primary` (ncc.rs:169).
pub open spec fn reassign_primary(m: NccMem, p: Tok, order: u64) -> (NccMem, bool) {
    // parent_results.get(&parent).and_then(|results| results.iter().next())
    if m.parent_results.dom().contains(p) && m.parent_results[p].len() > 0 {
        let first = m.parent_results[p][0];
        (
            NccMem {
                primary_result: m.primary_result.insert(p, first),
                block_order: m.block_order.insert(p, order),
                ..m
            },
            true,
        )
    } else {
        (
            NccMem {
                primary_result: m.primary_result.remove(p),
                block_order: m.block_order.remove(p),
                ..m
            },
            false,
        )
    }
}

/// Models `NccMemory::remove_result` (ncc.rs:140).
pub open spec fn remove_result(m: NccMem, r: Tok, replacement_order: u64) -> (
    NccMem,
    Option<(Tok, usize)>,
) {
    let (m1, removed) = remove_result_support(m, r);
    match removed {
        None => (m1, None),
        Some((p, c)) => if m1.primary_result.dom().contains(p) && m1.primary_result[p] == r {
            (reassign_primary(m1, p, replacement_order).0, removed)
        } else {
            (m1, removed)
        },
    }
}

/// Models `NccMemory::set_unblocked` (ncc.rs:206).
pub open spec fn set_unblocked(m: NccMem, p: Tok, pt: Tok) -> NccMem {
    NccMem { unblocked: m.unblocked.insert(p, pt), ..m }
}

/// Models `NccMemory::remove_unblocked` (ncc.rs:219).
pub open spec fn remove_unblocked(m: NccMem, p: Tok) -> (NccMem, Option<Tok>) {
    let old = if m.unblocked.dom().contains(p) {
        Some(m.unblocked[p])
    } else {
        None
    };
    (NccMem { unblocked: m.unblocked.remove(p), ..m }, old)
}

/// Models the `for result in &results { self.result_owner.remove(result); }`
/// loop of `remove_parent_token` (ncc.rs:235-237).
pub open spec fn remove_owners(owner: Map<Tok, Tok>, results: Seq<Tok>, i: int) -> Map<Tok, Tok>
    decreases results.len() - i,
{
    if i < 0 || i >= results.len() {
        owner
    } else {
        remove_owners(owner.remove(results[i]), results, i + 1)
    }
}

/// Models `NccMemory::remove_parent_token` (ncc.rs:232).
pub open spec fn remove_parent_token(m: NccMem, p: Tok) -> NccMem {
    let result_count = m.result_count.remove(p);
    let (parent_results, result_owner) = if m.parent_results.dom().contains(p) {
        (m.parent_results.remove(p), remove_owners(m.result_owner, m.parent_results[p], 0))
    } else {
        (m.parent_results, m.result_owner)
    };
    NccMem {
        result_count,
        parent_results,
        result_owner,
        primary_result: m.primary_result.remove(p),
        block_order: m.block_order.remove(p),
        unblocked: m.unblocked.remove(p),
    }
}

// ---------------------------------------------------------------------------
// Proofs
// ---------------------------------------------------------------------------
/// `A ⊆ B` and `|A| == |B|` imply `A == B` (finite sets).
pub proof fn lemma_subset_same_len<A>(a: Set<A>, b: Set<A>)
    requires
        a.subset_of(b),
        a.len() == b.len(),
    ensures
        a =~= b,
{
    if !(a =~= b) {
        let x = choose|x: A| b.contains(x) && !a.contains(x);
        assert(a.subset_of(b.remove(x)));
        vstd::set_lib::lemma_len_subset(a, b.remove(x));
    }
}

/// Domains of equal-length maps with one included in the other coincide.
pub proof fn lemma_dom_eq<A>(a: Set<A>, b: Set<A>)
    requires
        a.subset_of(b),
        a.len() == b.len(),
    ensures
        a =~= b,
{
    lemma_subset_same_len(a, b);
}

/// Under the invariant the owner preimage of a blocked parent is exactly its
/// result list.
pub proof fn lemma_preimage(m: NccMem, p: Tok)
    requires
        wf(m),
        m.result_count.dom().contains(p),
        m.parent_results.dom().contains(p),
        forall|r: Tok| #[trigger]
            m.parent_results[p].contains(r) ==> m.result_owner.dom().contains(r)
                && m.result_owner[r] == p,
        forall|r: Tok| #[trigger]
            m.result_owner.dom().contains(r) && m.result_owner[r] == p
                ==> m.parent_results[p].contains(r),
    ensures
        owner_preimage(m, p) =~= m.parent_results[p].to_set(),
        owner_preimage(m, p).len() == m.parent_results[p].len(),
{
    m.parent_results[p].to_set_ensures();
    assert(owner_preimage(m, p) =~= m.parent_results[p].to_set());
    m.parent_results[p].unique_seq_to_set();
}

pub proof fn lemma_validate_implies_strong(m: NccMem)
    requires
        wf(m),
        validate_consistency(m),
    ensures
        strong(m),
{
    assert(m.result_count.dom().subset_of(m.parent_results.dom()));
    lemma_dom_eq(m.result_count.dom(), m.parent_results.dom());
    assert(m.result_count.dom().subset_of(m.primary_result.dom()));
    lemma_dom_eq(m.result_count.dom(), m.primary_result.dom());
    assert(m.result_count.dom().subset_of(m.block_order.dom()));
    lemma_dom_eq(m.result_count.dom(), m.block_order.dom());
    assert forall|r: Tok| #[trigger] m.result_owner.dom().contains(r) implies m.result_count.dom().contains(
        m.result_owner[r],
    ) && m.parent_results[m.result_owner[r]].contains(r) by {
        let p = m.result_owner[r];
        let listed = m.parent_results[p].to_set();
        m.parent_results[p].to_set_ensures();
        m.parent_results[p].unique_seq_to_set();
        assert(listed.subset_of(owner_preimage(m, p)));
        lemma_subset_same_len(listed, owner_preimage(m, p));
        assert(owner_preimage(m, p).contains(r));
    }
}

pub proof fn lemma_strong_implies_validate(m: NccMem)
    requires
        strong(m),
    ensures
        validate_consistency(m),
        wf(m),
{
    assert forall|p: Tok| #[trigger] m.result_count.dom().contains(p) implies owner_preimage(
        m,
        p,
    ).len() == m.result_count[p] by {
        lemma_preimage(m, p);
    }
    assert forall|p: Tok| #[trigger] m.result_count.dom().contains(p) implies m.parent_results[p].contains(
        m.primary_result[p],
    ) by {
        assert(m.parent_results[p][0] == m.primary_result[p]);
    }
}

/// Every result list is bounded by the number of owned results, so a count
/// can only reach `usize::MAX` if `usize::MAX` results are live.
pub proof fn lemma_count_bound(m: NccMem, p: Tok)
    requires
        strong(m),
    ensures
        result_count(m, p) <= m.result_owner.len(),
{
    if m.result_count.dom().contains(p) {
        let listed = m.parent_results[p].to_set();
        m.parent_results[p].to_set_ensures();
        m.parent_results[p].unique_seq_to_set();
        assert(listed.subset_of(m.result_owner.dom()));
        vstd::set_lib::lemma_len_subset(listed, m.result_owner.dom());
    }
}

pub proof fn theorem_new_and_clear_valid(m: NccMem)
    ensures
        validate_consistency(new_mem()) && wf(new_mem()) && orders_unique(new_mem()),
        validate_consistency(clear(m)) && wf(clear(m)),
{
    assert(owner_preimage(new_mem(), 0) =~= Set::empty());
}

pub proof fn theorem_add_result(m: NccMem, p: Tok, r: Tok, order: u64)
    requires
        wf(m),
        validate_consistency(m),
        // physical bound: fewer than usize::MAX results are live
        m.result_owner.len() < usize::MAX,
    ensures
        ({
            let (m1, old_count, new_count) = add_result(m, p, r, order);
            // no overflow in `*count += 1`
            &&& !m.result_owner.dom().contains(r) ==> result_count(m, p) + 1 <= usize::MAX
            // re-adding a tracked result is a no-op
            &&& m.result_owner.dom().contains(r) ==> m1 == m && old_count == new_count
            // counts and result sets
            &&& !m.result_owner.dom().contains(r) ==> {
                &&& old_count == results(m, p).len()
                &&& new_count == old_count + 1
                &&& new_count == results(m1, p).len()
                &&& results(m1, p) == results(m, p).insert(r)
                &&& m1.primary_result[p] == (if old_count == 0 {
                    r
                } else {
                    m.primary_result[p]
                })
                &&& m1.block_order[p] == (if old_count == 0 {
                    order
                } else {
                    m.block_order[p]
                })
            }
            &&& forall|q: Tok|
                q != p && !m.result_owner.dom().contains(r) ==> #[trigger] results(m1, q) == results(
                    m,
                    q,
                )
            // the 0 -> 1 signal is exactly the empty -> non-empty transition of `p`
            &&& (old_count == 0 && new_count == 1) == (results(m, p).len() == 0 && results(
                m1,
                p,
            ).len() > 0)
            // invariant, and the rete's reaction to the 0 -> 1 signal
            &&& wf(m1)
            &&& !(old_count == 0 && new_count == 1 && m.unblocked.dom().contains(p))
                ==> validate_consistency(m1)
            &&& validate_consistency(remove_unblocked(m1, p).0)
            &&& wf(remove_unblocked(m1, p).0)
            // chronology uniqueness with a fresh order
            &&& orders_unique(m) && (results(m, p).len() == 0 ==> fresh_order(m, p, order))
                ==> orders_unique(m1)
        }),
{
    let (m1, old_count, new_count) = add_result(m, p, r, order);
    lemma_validate_implies_strong(m);
    if m.result_owner.dom().contains(r) {
        let q = m.result_owner[r];
        assert(m.parent_results[q].contains(r));
        assert(results(m, q).contains(r)) by {
            m.parent_results[q].to_set_ensures();
        }
        if q == p {
            assert(results(m, p).len() > 0);
        }
        assert(result_count(m, q) > 0);
        assert(remove_unblocked(m1, p).0.result_count.dom() =~= m.result_count.dom());
        let m2 = remove_unblocked(m1, p).0;
        assert(strong(m2));
        lemma_strong_implies_validate(m2);
    } else {
        lemma_count_bound(m, p);
        let old_list = if m.parent_results.dom().contains(p) {
            m.parent_results[p]
        } else {
            Seq::<Tok>::empty()
        };
        lemma_os_insert(old_list, r);
        assert(!old_list.contains(r)) by {
            if old_list.contains(r) {
                assert(m.result_owner.dom().contains(r));
            }
        }
        old_list.to_set_ensures();
        old_list.unique_seq_to_set();
        os_insert(old_list, r).to_set_ensures();
        assert(results(m1, p) =~= results(m, p).insert(r));
        assert(results(m, p) =~= old_list.to_set());
        assert(old_count == old_list.len());
        assert(results(m1, p).len() == old_list.len() + 1) by {
            os_insert(old_list, r).unique_seq_to_set();
        }
        assert(m1.result_count.dom() =~= m.result_count.dom().insert(p));
        assert(strong(m1) || (old_count == 0 && m.unblocked.dom().contains(p))) by {
            assert forall|q: Tok, x: Tok|
                m1.result_count.dom().contains(q) && #[trigger] m1.parent_results[q].contains(
                    x,
                ) implies m1.result_owner.dom().contains(x) && m1.result_owner[x] == q by {
                if q != p {
                    assert(m.parent_results[q].contains(x));
                } else if x != r {
                    assert(old_list.contains(x));
                }
            }
            assert forall|x: Tok| #[trigger] m1.result_owner.dom().contains(x) implies m1.result_count.dom().contains(
                m1.result_owner[x],
            ) && m1.parent_results[m1.result_owner[x]].contains(x) by {
                if x != r {
                    let q = m.result_owner[x];
                    assert(m.parent_results[q].contains(x));
                    if q == p {
                        assert(old_list.contains(x));
                    }
                }
            }
        }
        let m2 = remove_unblocked(m1, p).0;
        assert(strong(m2)) by {
            assert forall|q: Tok, x: Tok|
                m2.result_count.dom().contains(q) && #[trigger] m2.parent_results[q].contains(
                    x,
                ) implies m2.result_owner.dom().contains(x) && m2.result_owner[x] == q by {
                if q != p {
                    assert(m.parent_results[q].contains(x));
                } else if x != r {
                    assert(old_list.contains(x));
                }
            }
            assert forall|x: Tok| #[trigger] m2.result_owner.dom().contains(x) implies m2.result_count.dom().contains(
                m2.result_owner[x],
            ) && m2.parent_results[m2.result_owner[x]].contains(x) by {
                if x != r {
                    let q = m.result_owner[x];
                    assert(m.parent_results[q].contains(x));
                    if q == p {
                        assert(old_list.contains(x));
                    }
                }
            }
        }
        lemma_strong_implies_validate(m2);
        if strong(m1) {
            lemma_strong_implies_validate(m1);
        }
        assert forall|q: Tok| q != p implies #[trigger] results(m1, q) == results(m, q) by {}
        if orders_unique(m) && (results(m, p).len() == 0 ==> fresh_order(m, p, order)) {
            assert forall|a: Tok, b: Tok|
                m1.block_order.dom().contains(a) && m1.block_order.dom().contains(b) && a
                    != b implies #[trigger] m1.block_order[a] != #[trigger] m1.block_order[b] by {
                if old_count == 0 && (a == p || b == p) {
                    assert(results(m, p).len() == 0);
                }
            }
        }
    }
}

pub proof fn theorem_remove_result(m: NccMem, r: Tok, replacement_order: u64)
    requires
        wf(m),
        validate_consistency(m),
    ensures
        ({
            let (m1, removed) = remove_result(m, r, replacement_order);
            &&& wf(m1)
            &&& validate_consistency(m1)
            &&& removed.is_some() == m.result_owner.dom().contains(r)
            &&& removed is None ==> m1 == m
            &&& removed matches Some((p, c)) ==> {
                let p0 = m.result_owner[r];
                &&& p == p0
                // the guard in decrement_results is always taken: no masked underflow
                &&& result_count(m, p) > 0
                &&& c == results(m1, p).len()
                &&& c + 1 == results(m, p).len()
                &&& results(m1, p) == results(m, p).remove(r)
                &&& forall|q: Tok| q != p ==> #[trigger] results(m1, q) == results(m, q)
                // N -> 0 signal is exactly the non-empty -> empty transition
                &&& (c == 0) == (results(m, p).len() > 0 && results(m1, p).len() == 0)
                // chronology: primary is the oldest survivor; order replaced iff
                // the primary was removed with survivors
                &&& c > 0 ==> m1.primary_result[p] == m1.parent_results[p][0]
                &&& c > 0 ==> m1.block_order[p] == (if m.primary_result[p] == r {
                    replacement_order
                } else {
                    m.block_order[p]
                })
                &&& c == 0 ==> !m1.block_order.dom().contains(p)
                    && !m1.primary_result.dom().contains(p)
                &&& forall|q: Tok|
                    q != p && #[trigger] m.block_order.dom().contains(q) ==> m1.block_order.dom().contains(
                        q,
                    ) && m1.block_order[q] == m.block_order[q]
                &&& orders_unique(m) && fresh_order(m, p, replacement_order) ==> orders_unique(m1)
            }
        }),
{
    let (m1, removed) = remove_result(m, r, replacement_order);
    lemma_validate_implies_strong(m);
    if m.result_owner.dom().contains(r) {
        let p = m.result_owner[r];
        let list = m.parent_results[p];
        lemma_os_remove(list, r);
        list.to_set_ensures();
        list.unique_seq_to_set();
        let remaining = os_remove(list, r);
        remaining.to_set_ensures();
        remaining.unique_seq_to_set();
        assert(results(m, p) =~= list.to_set());
        let (ms, out) = remove_result_support(m, r);
        assert(results(ms, p) =~= results(m, p).remove(r));
        assert(results(m1, p) =~= results(m, p).remove(r));
        assert(remaining.len() + 1 == list.len());
        assert(list[0] == m.primary_result[p]);
        if remaining.len() > 0 {
            assert(ms.result_count.dom() =~= m.result_count.dom());
        } else {
            assert(ms.result_count.dom() =~= m.result_count.dom().remove(p));
        }
        assert(strong(m1)) by {
            assert forall|q: Tok, x: Tok|
                m1.result_count.dom().contains(q) && #[trigger] m1.parent_results[q].contains(
                    x,
                ) implies m1.result_owner.dom().contains(x) && m1.result_owner[x] == q by {
                if q != p {
                    assert(m.parent_results[q].contains(x));
                } else {
                    assert(list.contains(x));
                }
            }
            assert forall|x: Tok| #[trigger] m1.result_owner.dom().contains(x) implies m1.result_count.dom().contains(
                m1.result_owner[x],
            ) && m1.parent_results[m1.result_owner[x]].contains(x) by {
                let q = m.result_owner[x];
                assert(m.parent_results[q].contains(x));
                if q == p {
                    assert(remaining.contains(x));
                }
            }
            assert forall|q: Tok| #[trigger] m1.result_count.dom().contains(q) implies m1.parent_results[q].len()
                == m1.result_count[q] && m1.parent_results[q].len() > 0
                && m1.parent_results[q][0] == m1.primary_result[q] by {
                if q == p {
                    if list[0] == r {
                        assert(list.len() > 1);
                    }
                }
            }
        }
        lemma_strong_implies_validate(m1);
        assert forall|q: Tok| q != p implies #[trigger] results(m1, q) == results(m, q) by {}
        if orders_unique(m) && fresh_order(m, p, replacement_order) {
            assert forall|a: Tok, b: Tok|
                m1.block_order.dom().contains(a) && m1.block_order.dom().contains(b) && a
                    != b implies #[trigger] m1.block_order[a] != #[trigger] m1.block_order[b] by {
            }
        }
    }
}

/// Cascade protocol, phase one (rete.rs `collect_ncc_result_retractions`):
/// removing the support of a result that is not its parent's primary keeps
/// the invariant; removing a primary's support and then reassigning
/// (rete.rs `apply_ncc_result_retraction`) restores it.
pub proof fn theorem_two_phase_retraction(m: NccMem, r: Tok, order: u64)
    requires
        wf(m),
        validate_consistency(m),
        m.result_owner.dom().contains(r),
    ensures
        ({
            let p = m.result_owner[r];
            let (ms, out) = remove_result_support(m, r);
            &&& out == Some((p, (results(m, p).len() - 1) as usize))
            &&& m.primary_result[p] != r ==> validate_consistency(ms) && wf(ms)
            &&& m.primary_result[p] == r ==> validate_consistency(reassign_primary(ms, p, order).0)
                && wf(reassign_primary(ms, p, order).0)
            &&& m.primary_result[p] == r ==> reassign_primary(ms, p, order).1 == (results(
                ms,
                p,
            ).len() > 0)
        }),
{
    let p = m.result_owner[r];
    let (ms, out) = remove_result_support(m, r);
    theorem_remove_result(m, r, order);
    lemma_validate_implies_strong(m);
    let list = m.parent_results[p];
    lemma_os_remove(list, r);
    list.to_set_ensures();
    list.unique_seq_to_set();
    let remaining = os_remove(list, r);
    remaining.to_set_ensures();
    remaining.unique_seq_to_set();
    assert(results(m, p) =~= list.to_set());
    if m.primary_result[p] != r {
        assert(remove_result(m, r, order).0 == ms);
    } else {
        assert(remove_result(m, r, order).0 == reassign_primary(ms, p, order).0);
        if remaining.len() > 0 {
            assert(results(ms, p).contains(remaining[0]));
        } else {
            assert(results(ms, p) =~= Set::empty());
        }
    }
}

pub proof fn lemma_remove_owners(owner: Map<Tok, Tok>, results: Seq<Tok>, i: int)
    requires
        0 <= i <= results.len(),
    ensures
        forall|x: Tok| #[trigger]
            remove_owners(owner, results, i).dom().contains(x) <==> (owner.dom().contains(x)
                && !results.subrange(i, results.len() as int).contains(x)),
        forall|x: Tok| #[trigger]
            remove_owners(owner, results, i).dom().contains(x) ==> remove_owners(
                owner,
                results,
                i,
            )[x] == owner[x],
    decreases results.len() - i,
{
    if i < results.len() {
        lemma_remove_owners(owner.remove(results[i]), results, i + 1);
        let suffix = results.subrange(i, results.len() as int);
        let rest = results.subrange(i + 1, results.len() as int);
        assert forall|g: Tok| suffix.contains(g) <==> (g == results[i] || rest.contains(g)) by {
            if suffix.contains(g) {
                let j = choose|j: int| 0 <= j < suffix.len() && suffix[j] == g;
                if j > 0 {
                    assert(rest[j - 1] == g);
                } else {
                    assert(suffix[0] == results[i]);
                }
            }
            if rest.contains(g) {
                let j = choose|j: int| 0 <= j < rest.len() && rest[j] == g;
                assert(suffix[j + 1] == g);
            }
            if g == results[i] {
                assert(suffix[0] == g);
            }
        }
        let next = owner.remove(results[i]);
        assert(remove_owners(owner, results, i) == remove_owners(next, results, i + 1));
        assert forall|x: Tok| #[trigger]
            remove_owners(owner, results, i).dom().contains(x) <==> (owner.dom().contains(x)
                && !suffix.contains(x)) by {
            assert(remove_owners(next, results, i + 1).dom().contains(x) <==> (next.dom().contains(x)
                && !rest.contains(x)));
            assert(suffix.contains(x) <==> (x == results[i] || rest.contains(x)));
        }
        assert forall|x: Tok| #[trigger]
            remove_owners(owner, results, i).dom().contains(x) implies remove_owners(
            owner,
            results,
            i,
        )[x] == owner[x] by {
            assert(remove_owners(next, results, i + 1).dom().contains(x));
            assert(next.dom().contains(x));
        }
    } else {
        assert(results.subrange(i, results.len() as int).len() == 0);
    }
}

pub proof fn theorem_remove_parent_token(m: NccMem, p: Tok)
    requires
        wf(m),
        validate_consistency(m),
    ensures
        ({
            let m1 = remove_parent_token(m, p);
            &&& wf(m1)
            &&& validate_consistency(m1)
            &&& result_count(m1, p) == 0
            &&& results(m1, p) == Set::<Tok>::empty()
            &&& forall|x: Tok| #[trigger] m1.result_owner.dom().contains(x) ==> m1.result_owner[x] != p
            &&& !m1.primary_result.dom().contains(p)
            &&& !m1.block_order.dom().contains(p)
            &&& !m1.unblocked.dom().contains(p)
            &&& forall|q: Tok| q != p ==> #[trigger] results(m1, q) == results(m, q)
            &&& forall|q: Tok| q != p ==> #[trigger] result_count(m1, q) == result_count(m, q)
            &&& orders_unique(m) ==> orders_unique(m1)
        }),
{
    let m1 = remove_parent_token(m, p);
    lemma_validate_implies_strong(m);
    let list = if m.parent_results.dom().contains(p) {
        m.parent_results[p]
    } else {
        Seq::<Tok>::empty()
    };
    if m.parent_results.dom().contains(p) {
        lemma_remove_owners(m.result_owner, list, 0);
    }
    assert(list.subrange(0, list.len() as int) =~= list);
    assert forall|x: Tok| #[trigger] m1.result_owner.dom().contains(x) implies m.result_owner.dom().contains(
        x,
    ) && m1.result_owner[x] == m.result_owner[x] && m1.result_owner[x] != p by {
        assert(list.subrange(0, list.len() as int).contains(x) == list.contains(x));
        if m.result_owner[x] == p {
            assert(m.parent_results[p].contains(x));
        }
    }
    assert forall|x: Tok|
        m.result_owner.dom().contains(x) && m.result_owner[x] != p implies #[trigger] m1.result_owner.dom().contains(
        x,
    ) by {
        assert(list.subrange(0, list.len() as int).contains(x) == list.contains(x));
        if list.contains(x) {
            assert(m.result_owner[x] == p);
        }
    }
    assert(strong(m1)) by {
        assert forall|q: Tok, x: Tok|
            m1.result_count.dom().contains(q) && #[trigger] m1.parent_results[q].contains(
                x,
            ) implies m1.result_owner.dom().contains(x) && m1.result_owner[x] == q by {
            assert(m.parent_results[q].contains(x));
        }
        assert forall|x: Tok| #[trigger] m1.result_owner.dom().contains(x) implies m1.result_count.dom().contains(
            m1.result_owner[x],
        ) && m1.parent_results[m1.result_owner[x]].contains(x) by {
            assert(m.result_owner.dom().contains(x));
        }
    }
    lemma_strong_implies_validate(m1);
    assert(results(m1, p) =~= Set::<Tok>::empty());
    assert forall|q: Tok| q != p implies #[trigger] results(m1, q) == results(m, q) by {}
    if orders_unique(m) {
        assert forall|a: Tok, b: Tok|
            m1.block_order.dom().contains(a) && m1.block_order.dom().contains(b) && a
                != b implies #[trigger] m1.block_order[a] != #[trigger] m1.block_order[b] by {
            assert(m.block_order[a] != m.block_order[b]);
        }
    }
}

pub proof fn theorem_set_unblocked(m: NccMem, p: Tok, pt: Tok)
    requires
        wf(m),
        validate_consistency(m),
        result_count(m, p) == 0,
    ensures
        validate_consistency(set_unblocked(m, p, pt)),
        wf(set_unblocked(m, p, pt)),
{
    lemma_validate_implies_strong(m);
    lemma_strong_implies_validate(set_unblocked(m, p, pt));
}

pub proof fn theorem_remove_unblocked(m: NccMem, p: Tok)
    requires
        wf(m),
        validate_consistency(m),
    ensures
        validate_consistency(remove_unblocked(m, p).0),
        wf(remove_unblocked(m, p).0),
        !remove_unblocked(m, p).0.unblocked.dom().contains(p),
{
    lemma_validate_implies_strong(m);
    lemma_strong_implies_validate(remove_unblocked(m, p).0);
}

// ---------------------------------------------------------------------------
// Batched retraction cascade (several pending primaries at once)
// ---------------------------------------------------------------------------
/// The state between the two phases of a retraction cascade: rete.rs
/// `collect_ncc_result_retractions` calls `remove_result_support` for every
/// removed result first, recording a transition for each removed primary, and
/// only afterwards settles each transition with `apply_ncc_result_retraction`
/// (`reassign_primary`, or `remove_parent_token` when the parent token is
/// gone). In between, `validate_consistency` does not hold: each parent in
/// `pending` lost its primary result and still records that stale primary
/// and its block order. Otherwise the state satisfies `strong`.
pub open spec fn cascade_inv(m: NccMem, pending: Set<Tok>) -> bool {
    &&& wf(m)
    &&& m.result_count.dom() =~= m.parent_results.dom()
    &&& m.primary_result.dom() =~= m.result_count.dom().union(pending)
    &&& m.block_order.dom() =~= m.result_count.dom().union(pending)
    &&& forall|p: Tok| #[trigger]
        m.result_count.dom().contains(p) ==> {
            &&& m.parent_results[p].len() == m.result_count[p]
            &&& m.parent_results[p].len() > 0
            &&& !pending.contains(p) ==> m.parent_results[p][0] == m.primary_result[p]
        }
    // the recorded primary of a pending parent is no longer a tracked result
    &&& forall|p: Tok| #[trigger]
        pending.contains(p) ==> !m.result_owner.dom().contains(m.primary_result[p])
    &&& forall|p: Tok, r: Tok|
        #![trigger m.parent_results[p].contains(r)]
        m.result_count.dom().contains(p) && m.parent_results[p].contains(r)
            ==> m.result_owner.dom().contains(r) && m.result_owner[r] == p
    &&& forall|r: Tok| #[trigger]
        m.result_owner.dom().contains(r) ==> m.result_count.dom().contains(m.result_owner[r])
            && m.parent_results[m.result_owner[r]].contains(r)
    // a blocked or pending parent has no pass-through
    &&& forall|p: Tok| #[trigger]
        m.unblocked.dom().contains(p) ==> !m.result_count.dom().contains(p) && !pending.contains(p)
    &&& forall|p: Tok| #[trigger] m.result_count.dom().contains(p) ==> m.result_count[p] > 0
}

/// A cascade starts from a consistent memory with nothing pending.
pub proof fn theorem_cascade_start(m: NccMem)
    requires
        wf(m),
        validate_consistency(m),
    ensures
        cascade_inv(m, Set::empty()),
{
    lemma_validate_implies_strong(m);
    assert(m.result_count.dom().union(Set::empty()) =~= m.result_count.dom());
}

/// Once every recorded transition is settled, `validate_consistency` holds again.
pub proof fn theorem_cascade_finish(m: NccMem)
    requires
        cascade_inv(m, Set::empty()),
    ensures
        validate_consistency(m),
        wf(m),
{
    assert(m.result_count.dom().union(Set::empty()) =~= m.result_count.dom());
    assert(strong(m));
    lemma_strong_implies_validate(m);
}

/// Phase one: `remove_result_support` for one more removed result. The rete
/// records a transition exactly when the result is its parent's recorded
/// primary (rete.rs `collect_ncc_result_retractions`); that parent then joins
/// `pending`. A parent is recorded at most once per batch, and phase one
/// leaves every recorded primary and block order untouched, so the guard
/// `primary_result[parent] == result` in `apply_ncc_result_retraction` still
/// sees the recorded transition.
pub proof fn theorem_cascade_remove_support(m: NccMem, pending: Set<Tok>, r: Tok)
    requires
        cascade_inv(m, pending),
        m.result_owner.dom().contains(r),
    ensures
        ({
            let p = m.result_owner[r];
            let (m1, out) = remove_result_support(m, r);
            let transition = m.primary_result[p] == r;
            &&& out == Some((p, (results(m, p).len() - 1) as usize))
            &&& results(m1, p) == results(m, p).remove(r)
            &&& forall|q: Tok| q != p ==> #[trigger] results(m1, q) == results(m, q)
            &&& transition ==> !pending.contains(p)
            &&& cascade_inv(
                m1,
                if transition {
                    pending.insert(p)
                } else {
                    pending
                },
            )
            &&& m1.primary_result == m.primary_result
            &&& m1.block_order == m.block_order
            &&& m1.unblocked == m.unblocked
        }),
{
    let p = m.result_owner[r];
    let (m1, out) = remove_result_support(m, r);
    let transition = m.primary_result[p] == r;
    let pending1 = if transition {
        pending.insert(p)
    } else {
        pending
    };
    let list = m.parent_results[p];
    assert(list.contains(r));
    lemma_os_remove(list, r);
    list.to_set_ensures();
    list.unique_seq_to_set();
    let remaining = os_remove(list, r);
    remaining.to_set_ensures();
    remaining.unique_seq_to_set();
    assert(results(m, p) =~= list.to_set());
    assert(remaining.len() + 1 == list.len());
    assert(results(m1, p) =~= results(m, p).remove(r));
    if transition {
        // a pending parent's primary is not tracked, but `r` is
        assert(!pending.contains(p));
    }
    if !pending.contains(p) && !transition {
        // the primary survives, so the parent stays blocked by it
        assert(list[0] == m.primary_result[p]);
        assert(remaining.contains(list[0]));
    }
    if remaining.len() > 0 {
        assert(m1.result_count.dom() =~= m.result_count.dom());
    } else {
        assert(m1.result_count.dom() =~= m.result_count.dom().remove(p));
    }
    assert(m1.parent_results.dom() =~= m1.result_count.dom());
    assert(m1.primary_result.dom() =~= m1.result_count.dom().union(pending1));
    assert(m1.block_order.dom() =~= m1.result_count.dom().union(pending1));
    assert forall|q: Tok| #[trigger] m1.result_count.dom().contains(q) implies {
        &&& m1.parent_results[q].len() == m1.result_count[q]
        &&& m1.parent_results[q].len() > 0
        &&& !pending1.contains(q) ==> m1.parent_results[q][0] == m1.primary_result[q]
    } by {
        if q == p && !pending1.contains(q) {
            assert(list[0] == m.primary_result[p]);
        }
    }
    assert forall|q: Tok| #[trigger] pending1.contains(q) implies !m1.result_owner.dom().contains(
        m1.primary_result[q],
    ) by {}
    assert forall|q: Tok, x: Tok|
        m1.result_count.dom().contains(q) && #[trigger] m1.parent_results[q].contains(
            x,
        ) implies m1.result_owner.dom().contains(x) && m1.result_owner[x] == q by {
        if q != p {
            assert(m.parent_results[q].contains(x));
        } else {
            assert(list.contains(x));
        }
    }
    assert forall|x: Tok| #[trigger] m1.result_owner.dom().contains(x) implies m1.result_count.dom().contains(
        m1.result_owner[x],
    ) && m1.parent_results[m1.result_owner[x]].contains(x) by {
        let q = m.result_owner[x];
        assert(m.parent_results[q].contains(x));
        if q == p {
            assert(remaining.contains(x));
        }
    }
    assert forall|q: Tok| q != p implies #[trigger] results(m1, q) == results(m, q) by {}
}

/// Phase two, parent token still live: `reassign_primary` settles one pending
/// parent (rete.rs `apply_ncc_result_retraction`). It returns `true` exactly
/// when results survive, in which case the oldest survivor becomes primary at
/// the fresh `order`; otherwise the parent's chronology is dropped and it may
/// be unblocked. Other parents, settled or pending, are untouched.
pub proof fn theorem_cascade_reassign(m: NccMem, pending: Set<Tok>, p: Tok, order: u64)
    requires
        cascade_inv(m, pending),
        pending.contains(p),
    ensures
        ({
            let (m1, still_blocked) = reassign_primary(m, p, order);
            &&& cascade_inv(m1, pending.remove(p))
            &&& still_blocked == (results(m, p).len() > 0)
            &&& still_blocked == m.result_count.dom().contains(p)
            &&& still_blocked ==> m1.primary_result[p] == m.parent_results[p][0]
                && m1.block_order[p] == order
            &&& !still_blocked ==> !m1.primary_result.dom().contains(p)
                && !m1.block_order.dom().contains(p)
            &&& forall|q: Tok|
                q != p ==> (#[trigger] m1.primary_result.dom().contains(q)
                    <==> m.primary_result.dom().contains(q))
            &&& forall|q: Tok|
                q != p && #[trigger] m.primary_result.dom().contains(q) ==> m1.primary_result[q]
                    == m.primary_result[q] && m1.block_order[q] == m.block_order[q]
            &&& forall|q: Tok| #[trigger] results(m1, q) == results(m, q)
            &&& m1.unblocked == m.unblocked
            &&& orders_unique(m) && (still_blocked ==> fresh_order(m, p, order)) ==> orders_unique(
                m1,
            )
        }),
{
    let (m1, still_blocked) = reassign_primary(m, p, order);
    let pending1 = pending.remove(p);
    if m.result_count.dom().contains(p) {
        let list = m.parent_results[p];
        list.to_set_ensures();
        assert(results(m, p).contains(list[0]));
    } else {
        assert(results(m, p) =~= Set::empty());
    }
    assert(m1.primary_result.dom() =~= m1.result_count.dom().union(pending1));
    assert(m1.block_order.dom() =~= m1.result_count.dom().union(pending1));
    assert forall|q: Tok| #[trigger] m1.result_count.dom().contains(q) implies {
        &&& m1.parent_results[q].len() == m1.result_count[q]
        &&& m1.parent_results[q].len() > 0
        &&& !pending1.contains(q) ==> m1.parent_results[q][0] == m1.primary_result[q]
    } by {}
    assert forall|q: Tok| #[trigger] pending1.contains(q) implies !m1.result_owner.dom().contains(
        m1.primary_result[q],
    ) by {
        assert(pending.contains(q) && q != p);
    }
    assert forall|q: Tok, x: Tok|
        m1.result_count.dom().contains(q) && #[trigger] m1.parent_results[q].contains(
            x,
        ) implies m1.result_owner.dom().contains(x) && m1.result_owner[x] == q by {
        assert(m.parent_results[q].contains(x));
    }
    assert forall|q: Tok| #[trigger] results(m1, q) == results(m, q) by {}
    if orders_unique(m) && (still_blocked ==> fresh_order(m, p, order)) {
        assert forall|a: Tok, b: Tok|
            m1.block_order.dom().contains(a) && m1.block_order.dom().contains(b) && a
                != b implies #[trigger] m1.block_order[a] != #[trigger] m1.block_order[b] by {
            if a != p && b != p {
                assert(m.block_order[a] != m.block_order[b]);
            }
        }
    }
}

/// Phase two, parent token already gone: `remove_parent_token` settles a
/// pending parent (rete.rs `apply_ncc_result_retraction`) and removes every
/// trace of it; any other parent can be removed mid-cascade as well.
pub proof fn theorem_cascade_remove_parent(m: NccMem, pending: Set<Tok>, p: Tok)
    requires
        cascade_inv(m, pending),
    ensures
        ({
            let m1 = remove_parent_token(m, p);
            &&& cascade_inv(m1, pending.remove(p))
            &&& results(m1, p) == Set::<Tok>::empty()
            &&& forall|x: Tok| #[trigger] m1.result_owner.dom().contains(x) ==> m1.result_owner[x] != p
            &&& !m1.primary_result.dom().contains(p)
            &&& !m1.block_order.dom().contains(p)
            &&& !m1.unblocked.dom().contains(p)
            &&& forall|q: Tok| q != p ==> #[trigger] results(m1, q) == results(m, q)
            &&& forall|q: Tok|
                q != p && #[trigger] m.primary_result.dom().contains(q) ==> m1.primary_result.dom().contains(q)
                    && m1.primary_result[q] == m.primary_result[q] && m1.block_order[q]
                    == m.block_order[q]
        }),
{
    let m1 = remove_parent_token(m, p);
    let pending1 = pending.remove(p);
    let list = if m.parent_results.dom().contains(p) {
        m.parent_results[p]
    } else {
        Seq::<Tok>::empty()
    };
    if m.parent_results.dom().contains(p) {
        lemma_remove_owners(m.result_owner, list, 0);
    }
    assert(list.subrange(0, list.len() as int) =~= list);
    assert forall|x: Tok| #[trigger] m1.result_owner.dom().contains(x) implies m.result_owner.dom().contains(
        x,
    ) && m1.result_owner[x] == m.result_owner[x] && m1.result_owner[x] != p by {
        assert(list.subrange(0, list.len() as int).contains(x) == list.contains(x));
        if m.result_owner[x] == p {
            assert(m.parent_results[p].contains(x));
        }
    }
    assert forall|x: Tok|
        m.result_owner.dom().contains(x) && m.result_owner[x] != p implies #[trigger] m1.result_owner.dom().contains(
        x,
    ) by {
        assert(list.subrange(0, list.len() as int).contains(x) == list.contains(x));
        if list.contains(x) {
            assert(m.result_owner[x] == p);
        }
    }
    assert(m1.result_count.dom() =~= m.result_count.dom().remove(p));
    assert(m1.parent_results.dom() =~= m1.result_count.dom());
    assert(m1.primary_result.dom() =~= m1.result_count.dom().union(pending1));
    assert(m1.block_order.dom() =~= m1.result_count.dom().union(pending1));
    assert forall|q: Tok| #[trigger] pending1.contains(q) implies !m1.result_owner.dom().contains(
        m1.primary_result[q],
    ) by {
        assert(pending.contains(q));
    }
    assert forall|q: Tok, x: Tok|
        m1.result_count.dom().contains(q) && #[trigger] m1.parent_results[q].contains(
            x,
        ) implies m1.result_owner.dom().contains(x) && m1.result_owner[x] == q by {
        assert(m.parent_results[q].contains(x));
    }
    assert forall|x: Tok| #[trigger] m1.result_owner.dom().contains(x) implies m1.result_count.dom().contains(
        m1.result_owner[x],
    ) && m1.parent_results[m1.result_owner[x]].contains(x) by {
        assert(m.result_owner.dom().contains(x));
    }
    assert(results(m1, p) =~= Set::<Tok>::empty());
    assert forall|q: Tok| q != p implies #[trigger] results(m1, q) == results(m, q) by {}
}

/// Phase two, unblocked parent: after `reassign_primary` returned `false`,
/// `ncc_left_activate` may record a pass-through for the settled parent.
pub proof fn theorem_cascade_set_unblocked(m: NccMem, pending: Set<Tok>, p: Tok, pt: Tok)
    requires
        cascade_inv(m, pending),
        !m.result_count.dom().contains(p),
        !pending.contains(p),
    ensures
        cascade_inv(set_unblocked(m, p, pt), pending),
{
}

/// Models phase one over a whole batch, the loop of rete.rs:1947
/// `collect_ncc_result_retractions`, for one memory: for each removed result
/// `rs[i..]` in order, skip it if it is not tracked (`let Some(parent) =
/// result_owner.get(result) else { continue }`), record its parent if it is
/// the recorded primary, then `remove_result_support`. Returns the memory and
/// the parents with recorded transitions (added to `pending`).
pub open spec fn collect(m: NccMem, pending: Set<Tok>, rs: Seq<Tok>, i: int) -> (NccMem, Set<Tok>)
    decreases rs.len() - i,
{
    if i < 0 || i >= rs.len() {
        (m, pending)
    } else if !m.result_owner.dom().contains(rs[i]) {
        collect(m, pending, rs, i + 1)
    } else {
        let p = m.result_owner[rs[i]];
        let pending1 = if m.primary_result[p] == rs[i] {
            pending.insert(p)
        } else {
            pending
        };
        collect(remove_result_support(m, rs[i]).0, pending1, rs, i + 1)
    }
}

/// Models phase two over a whole batch, one rete.rs:1994
/// `apply_ncc_result_retraction` per recorded parent `ps[i..]` in order: its
/// guard `primary_result[parent] == result` always holds here (see
/// `theorem_cascade_remove_support`), so the parent is settled by
/// `reassign_primary` at `orders[i]` (any value; the rete passes a fresh
/// `allocate_block_order()` when results survive and 0 otherwise) when its
/// parent token is still live, or by `remove_parent_token` otherwise. `live` stands for `token_store.get(parent).is_some()`.
pub open spec fn settle(
    m: NccMem,
    ps: Seq<Tok>,
    live: spec_fn(Tok) -> bool,
    orders: Seq<u64>,
    i: int,
) -> NccMem
    decreases ps.len() - i,
{
    if i < 0 || i >= ps.len() {
        m
    } else {
        let m1 = if live(ps[i]) {
            reassign_primary(m, ps[i], if i < orders.len() {
                orders[i]
            } else {
                0
            }).0
        } else {
            remove_parent_token(m, ps[i])
        };
        settle(m1, ps, live, orders, i + 1)
    }
}

pub proof fn lemma_collect(m: NccMem, pending: Set<Tok>, rs: Seq<Tok>, i: int)
    requires
        cascade_inv(m, pending),
        0 <= i <= rs.len(),
    ensures
        cascade_inv(collect(m, pending, rs, i).0, collect(m, pending, rs, i).1),
        pending.subset_of(collect(m, pending, rs, i).1),
    decreases rs.len() - i,
{
    if i < rs.len() {
        if m.result_owner.dom().contains(rs[i]) {
            let p = m.result_owner[rs[i]];
            let pending1 = if m.primary_result[p] == rs[i] {
                pending.insert(p)
            } else {
                pending
            };
            theorem_cascade_remove_support(m, pending, rs[i]);
            lemma_collect(remove_result_support(m, rs[i]).0, pending1, rs, i + 1);
        } else {
            lemma_collect(m, pending, rs, i + 1);
        }
    }
}

pub proof fn lemma_settle(
    m: NccMem,
    ps: Seq<Tok>,
    live: spec_fn(Tok) -> bool,
    orders: Seq<u64>,
    i: int,
)
    requires
        0 <= i <= ps.len(),
        ps.no_duplicates(),
        cascade_inv(m, ps.subrange(i, ps.len() as int).to_set()),
    ensures
        cascade_inv(settle(m, ps, live, orders, i), Set::empty()),
    decreases ps.len() - i,
{
    let pending = ps.subrange(i, ps.len() as int).to_set();
    if i < ps.len() {
        let p = ps[i];
        let rest = ps.subrange(i + 1, ps.len() as int).to_set();
        assert(pending.remove(p) =~= rest) by {
            assert forall|x: Tok| pending.remove(p).contains(x) <==> rest.contains(x) by {
                let suffix = ps.subrange(i, ps.len() as int);
                let tail = ps.subrange(i + 1, ps.len() as int);
                suffix.to_set_ensures();
                tail.to_set_ensures();
                if tail.contains(x) {
                    let j = choose|j: int| 0 <= j < tail.len() && tail[j] == x;
                    assert(suffix[j + 1] == x);
                    assert(ps[i + 1 + j] == x);
                    assert(x != p);
                }
                if suffix.contains(x) && x != p {
                    let j = choose|j: int| 0 <= j < suffix.len() && suffix[j] == x;
                    assert(j != 0);
                    assert(tail[j - 1] == x);
                }
            }
        }
        assert(pending.contains(p)) by {
            ps.subrange(i, ps.len() as int).to_set_ensures();
            assert(ps.subrange(i, ps.len() as int)[0] == p);
        }
        let order = if i < orders.len() {
            orders[i]
        } else {
            0
        };
        if live(p) {
            theorem_cascade_reassign(m, pending, p, order);
            lemma_settle(reassign_primary(m, p, order).0, ps, live, orders, i + 1);
        } else {
            theorem_cascade_remove_parent(m, pending, p);
            lemma_settle(remove_parent_token(m, p), ps, live, orders, i + 1);
        }
    } else {
        assert(pending =~= Set::empty()) by {
            ps.subrange(i, ps.len() as int).to_set_ensures();
        }
    }
}

/// A whole batched cascade on one memory restores `validate_consistency`:
/// starting from a consistent memory, phase one removes the support of every
/// removed result `rs` (in any order, tracked or not), and phase two settles
/// every recorded transition exactly once (`ps` enumerates the recorded
/// parents, in any order), each by `reassign_primary` or
/// `remove_parent_token` as the liveness of its parent token dictates.
pub proof fn theorem_cascade_batch(
    m: NccMem,
    rs: Seq<Tok>,
    ps: Seq<Tok>,
    live: spec_fn(Tok) -> bool,
    orders: Seq<u64>,
)
    requires
        wf(m),
        validate_consistency(m),
        ps.no_duplicates(),
        ps.to_set() == collect(m, Set::empty(), rs, 0).1,
    ensures
        validate_consistency(settle(collect(m, Set::empty(), rs, 0).0, ps, live, orders, 0)),
        wf(settle(collect(m, Set::empty(), rs, 0).0, ps, live, orders, 0)),
{
    theorem_cascade_start(m);
    lemma_collect(m, Set::empty(), rs, 0);
    assert(ps.subrange(0, ps.len() as int) =~= ps);
    lemma_settle(collect(m, Set::empty(), rs, 0).0, ps, live, orders, 0);
    theorem_cascade_finish(settle(collect(m, Set::empty(), rs, 0).0, ps, live, orders, 0));
}

} // verus!
