//! Verus model of `NegativeMemory` (crates/ferric-rules-core/src/negative.rs).
//!
//! What is proved (unbounded: any number of parents, facts and blockers, any
//! history of operations):
//!
//! 1. `new` and `clear` establish `validate_consistency`, and every mutating
//!    operation (`add_blocker`, `remove_blocker`, `set_unblocked`,
//!    `remove_unblocked`, `remove_parent_token`) preserves it, provided the
//!    caller follows the protocol stated in the `requires` clauses (see
//!    "Caller protocol" below).
//! 2. Abstract refinement: the blocker list of each parent behaves as a set
//!    under `add_blocker` (union) and `remove_blocker` (difference), all other
//!    parents are untouched, and the reverse index answers
//!    `tokens_blocked_by(f) == { p | f is a blocker of p }`.
//! 3. Transition signal: `remove_blocker` returns `true` exactly when the
//!    parent's blocker set goes from non-empty to empty.
//! 4. Chronology: the primary blocker is the oldest surviving blocker; the
//!    parent's block order is set by its first blocker, replaced by
//!    `replacement_order` exactly when the primary blocker is removed while
//!    another blocker survives, and is otherwise unchanged.
//! 5. `remove_parent_token(p)` removes every trace of `p`: it is neither
//!    blocked nor unblocked, has no block order, and no fact's reverse set
//!    mentions it; other parents keep their blockers.
//!
//! Model / abstractions (the trusted base besides Verus, vstd and Z3):
//!
//! * `FxHashMap<K, V>` is modelled as `Map<K, V>` and `FxHashSet<K>` as
//!   `Set<K>`; hashing, capacity and iteration order are not modelled.
//! * `OrderedSet<K>` is modelled as a duplicate-free `Seq<K>` in insertion
//!   order (`os_insert` appends an absent key, `os_remove` deletes the key in
//!   place). This is the refinement that `ordered_set` is expected to satisfy;
//!   it is not proved here.
//! * `TokenId` and `FactId` are modelled as `u64` (only equality matters).
//! * The struct's `id` field and the `Result<(), String>` error messages are
//!   omitted: `validate_consistency` is modelled as a boolean predicate whose
//!   conjuncts follow the production checks one-to-one.
//! * Loops are modelled by recursive spec functions that process the same
//!   elements in the same order (`purge_reverse` for the loop in
//!   `remove_parent_token`).
//!
//! Caller protocol (preconditions, enforced by `ReteNetwork`, not by the
//! memory itself):
//!
//! * `add_blocker` is never called for a parent that is tracked as unblocked
//!   (rete.rs `negative_right_activate` calls `remove_unblocked` first;
//!   `negative_left_activate` only blocks new parents).
//! * The `order` given to `add_blocker` for a parent that is not yet blocked,
//!   and the `replacement_order` given to `remove_blocker` when the primary
//!   blocker is removed with survivors, are fresh: distinct from every other
//!   live block order (rete.rs `allocate_block_order`).
//! * `set_unblocked` is only called for a parent that is not blocked.
//!
//! Known gaps: the model does not cover the `Debug`/`serde` paths, and the
//! fidelity of the model to the Rust source is checked by review (each spec
//! function cites the production function and mirrors its branches), not
//! mechanically. The bounded Kani harnesses in
//! crates/ferric-rules-core/src/negative/kani_proofs.rs exercise the
//! production code itself.
use vstd::prelude::*;

verus! {

/// Models `TokenId` (crates/ferric-rules-core/src/token.rs:18).
pub type Tok = u64;

/// Models `FactId` (crates/ferric-rules-core/src/fact.rs:172).
pub type Fct = u64;

// ---------------------------------------------------------------------------
// OrderedSet<K> abstraction (crates/ferric-rules-core/src/ordered_set.rs)
// ---------------------------------------------------------------------------
/// Models `OrderedSet::insert` (ordered_set.rs:36): an absent key is appended.
pub open spec fn os_insert<K>(s: Seq<K>, k: K) -> Seq<K> {
    if s.contains(k) {
        s
    } else {
        s.push(k)
    }
}

/// Models `OrderedSet::remove` (ordered_set.rs:96): the key is unlinked in place.
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
        os_insert(s, k).len() > 0,
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
/// Models `struct NegativeMemory` (negative.rs:41), without the `id` field.
pub struct NegMem {
    /// `blocked: HashMap<TokenId, OrderedSet<FactId>>`
    pub blocked: Map<Tok, Seq<Fct>>,
    /// `block_order: HashMap<TokenId, u64>`
    pub block_order: Map<Tok, u64>,
    /// `fact_to_blocked: HashMap<FactId, HashSet<TokenId>>`
    pub fact_to_blocked: Map<Fct, Set<Tok>>,
    /// `unblocked: HashMap<TokenId, TokenId>`
    pub unblocked: Map<Tok, Tok>,
}

/// Representation facts guaranteed by the production types: every
/// `OrderedSet` is duplicate-free.
pub open spec fn wf(m: NegMem) -> bool {
    forall|p: Tok| #[trigger] m.blocked.dom().contains(p) ==> m.blocked[p].no_duplicates()
}

/// Models `NegativeMemory::validate_consistency` (negative.rs:249); one
/// conjunct per `require!`.
pub open spec fn validate_consistency(m: NegMem) -> bool {
    // negative.rs:250-258: chronology covers exactly the blocked parents.
    &&& m.block_order.len() == m.blocked.len()
    &&& forall|p: Tok| #[trigger] m.blocked.dom().contains(p) ==> m.block_order.dom().contains(p)
    // negative.rs:259-264: block orders are unique.
    &&& forall|p: Tok, q: Tok|
        #![trigger m.block_order[p], m.block_order[q]]
        m.block_order.dom().contains(p) && m.block_order.dom().contains(q) && p != q
            ==> m.block_order[p] != m.block_order[q]
    // negative.rs:266-271: no empty blocker set.
    &&& forall|p: Tok| #[trigger] m.blocked.dom().contains(p) ==> m.blocked[p].len() > 0
    // negative.rs:272-279: forward entries have reverse entries.
    &&& forall|p: Tok, f: Fct|
        #![trigger m.blocked[p].contains(f)]
        m.blocked.dom().contains(p) && m.blocked[p].contains(f) ==> m.fact_to_blocked.dom().contains(
            f,
        ) && m.fact_to_blocked[f].contains(p)
    // negative.rs:282-287: no empty reverse set.
    &&& forall|f: Fct| #[trigger]
        m.fact_to_blocked.dom().contains(f) ==> m.fact_to_blocked[f].len() > 0
    // negative.rs:288-295: reverse entries have forward entries.
    &&& forall|f: Fct, p: Tok|
        #![trigger m.fact_to_blocked[f].contains(p)]
        m.fact_to_blocked.dom().contains(f) && m.fact_to_blocked[f].contains(p)
            ==> m.blocked.dom().contains(p) && m.blocked[p].contains(f)
    // negative.rs:299-305: no parent is both blocked and unblocked.
    &&& forall|p: Tok| #[trigger] m.unblocked.dom().contains(p) ==> !m.blocked.dom().contains(p)
}

pub open spec fn inv(m: NegMem) -> bool {
    wf(m) && validate_consistency(m)
}

/// The abstract blocker set of a parent (empty when not blocked).
pub open spec fn blockers(m: NegMem, p: Tok) -> Set<Fct> {
    if m.blocked.dom().contains(p) {
        m.blocked[p].to_set()
    } else {
        Set::empty()
    }
}

/// `order` differs from the block order of every blocked parent other than `p`.
pub open spec fn fresh_order(m: NegMem, p: Tok, order: u64) -> bool {
    forall|q: Tok| #[trigger] m.block_order.dom().contains(q) && q != p ==> m.block_order[q] != order
}

// ---------------------------------------------------------------------------
// Operations (one spec function per production method)
// ---------------------------------------------------------------------------
/// Models `NegativeMemory::new` (negative.rs:63).
pub open spec fn new_mem() -> NegMem {
    NegMem {
        blocked: Map::empty(),
        block_order: Map::empty(),
        fact_to_blocked: Map::empty(),
        unblocked: Map::empty(),
    }
}

/// Models `NegativeMemory::clear` (negative.rs:128).
pub open spec fn clear(m: NegMem) -> NegMem {
    new_mem()
}

/// Models `NegativeMemory::add_blocker` (negative.rs:78).
pub open spec fn add_blocker(m: NegMem, p: Tok, f: Fct, order: u64) -> NegMem {
    // if !self.blocked.contains_key(&parent_token_id) { self.block_order.insert(..) }
    let block_order = if !m.blocked.dom().contains(p) {
        m.block_order.insert(p, order)
    } else {
        m.block_order
    };
    // self.blocked.entry(parent_token_id).or_default().insert(fact_id)
    let current = if m.blocked.dom().contains(p) {
        m.blocked[p]
    } else {
        Seq::<Fct>::empty()
    };
    let blocked = m.blocked.insert(p, os_insert(current, f));
    // self.fact_to_blocked.entry(fact_id).or_default().insert(parent_token_id)
    let reverse = if m.fact_to_blocked.dom().contains(f) {
        m.fact_to_blocked[f]
    } else {
        Set::<Tok>::empty()
    };
    let fact_to_blocked = m.fact_to_blocked.insert(f, reverse.insert(p));
    NegMem { blocked, block_order, fact_to_blocked, unblocked: m.unblocked }
}

/// The reverse-index update shared by `remove_blocker` (negative.rs:117-122)
/// and the loop body of `remove_parent_token` (negative.rs:202-207).
pub open spec fn purge_one(f2b: Map<Fct, Set<Tok>>, p: Tok, f: Fct) -> Map<Fct, Set<Tok>> {
    if f2b.dom().contains(f) {
        let tokens = f2b[f].remove(p);
        if tokens.len() == 0 {
            f2b.remove(f)
        } else {
            f2b.insert(f, tokens)
        }
    } else {
        f2b
    }
}

/// Models `NegativeMemory::remove_blocker` (negative.rs:97); returns the new
/// state and `now_unblocked`.
pub open spec fn remove_blocker(m: NegMem, p: Tok, f: Fct, replacement_order: u64) -> (NegMem, bool) {
    // if let Some(blockers) = self.blocked.get_mut(&parent_token_id)
    let (blocked, block_order, now_unblocked) = if m.blocked.dom().contains(p) {
        let blockers = m.blocked[p];
        // let was_primary = blockers.iter().next() == Some(&fact_id);
        let was_primary = blockers.len() > 0 && blockers[0] == f;
        // blockers.remove(&fact_id);
        let remaining = os_remove(blockers, f);
        if remaining.len() == 0 {
            (m.blocked.remove(p), m.block_order.remove(p), true)
        } else if was_primary {
            (m.blocked.insert(p, remaining), m.block_order.insert(p, replacement_order), false)
        } else {
            (m.blocked.insert(p, remaining), m.block_order, false)
        }
    } else {
        (m.blocked, m.block_order, false)
    };
    let fact_to_blocked = purge_one(m.fact_to_blocked, p, f);
    (NegMem { blocked, block_order, fact_to_blocked, unblocked: m.unblocked }, now_unblocked)
}

/// Models `NegativeMemory::set_unblocked` (negative.rs:168).
pub open spec fn set_unblocked(m: NegMem, p: Tok, pt: Tok) -> NegMem {
    NegMem { unblocked: m.unblocked.insert(p, pt), ..m }
}

/// Models `NegativeMemory::remove_unblocked` (negative.rs:181).
pub open spec fn remove_unblocked(m: NegMem, p: Tok) -> (NegMem, Option<Tok>) {
    let old = if m.unblocked.dom().contains(p) {
        Some(m.unblocked[p])
    } else {
        None
    };
    (NegMem { unblocked: m.unblocked.remove(p), ..m }, old)
}

/// Models the `for fact_id in &blockers` loop of `remove_parent_token`
/// (negative.rs:201-208): processes `facts[i..]` front to back.
pub open spec fn purge_reverse(f2b: Map<Fct, Set<Tok>>, p: Tok, facts: Seq<Fct>, i: int) -> Map<
    Fct,
    Set<Tok>,
>
    decreases facts.len() - i,
{
    if i < 0 || i >= facts.len() {
        f2b
    } else {
        purge_reverse(purge_one(f2b, p, facts[i]), p, facts, i + 1)
    }
}

/// Models `NegativeMemory::remove_parent_token` (negative.rs:194).
pub open spec fn remove_parent_token(m: NegMem, p: Tok) -> NegMem {
    let unblocked = m.unblocked.remove(p);
    let block_order = m.block_order.remove(p);
    let (blocked, fact_to_blocked) = if m.blocked.dom().contains(p) {
        (m.blocked.remove(p), purge_reverse(m.fact_to_blocked, p, m.blocked[p], 0))
    } else {
        (m.blocked, m.fact_to_blocked)
    };
    NegMem { blocked, block_order, fact_to_blocked, unblocked }
}

// ---------------------------------------------------------------------------
// Proofs
// ---------------------------------------------------------------------------
pub proof fn lemma_purge_one(f2b: Map<Fct, Set<Tok>>, p: Tok, f: Fct)
    ensures
        forall|g: Fct|
            g != f ==> (#[trigger] purge_one(f2b, p, f).dom().contains(g) <==> f2b.dom().contains(
                g,
            )),
        forall|g: Fct| g != f && f2b.dom().contains(g) ==> #[trigger] purge_one(f2b, p, f)[g] == f2b[g],
        purge_one(f2b, p, f).dom().contains(f) <==> (f2b.dom().contains(f) && f2b[f].remove(p).len()
            > 0),
        purge_one(f2b, p, f).dom().contains(f) ==> purge_one(f2b, p, f)[f] == f2b[f].remove(p),
{
}

/// The loop in `remove_parent_token` removes `p` from the reverse set of
/// every fact in `facts[i..]`, dropping sets that become empty, and leaves all
/// other entries alone.
pub proof fn lemma_purge_reverse(f2b: Map<Fct, Set<Tok>>, p: Tok, facts: Seq<Fct>, i: int)
    requires
        0 <= i <= facts.len(),
        facts.no_duplicates(),
    ensures
        forall|g: Fct|
            !(#[trigger] facts.subrange(i, facts.len() as int).contains(g)) ==> (purge_reverse(
                f2b,
                p,
                facts,
                i,
            ).dom().contains(g) <==> f2b.dom().contains(g)),
        forall|g: Fct|
            !(#[trigger] facts.subrange(i, facts.len() as int).contains(g)) && f2b.dom().contains(g)
                ==> purge_reverse(f2b, p, facts, i)[g] == f2b[g],
        forall|g: Fct| #[trigger]
            facts.subrange(i, facts.len() as int).contains(g) ==> (purge_reverse(
                f2b,
                p,
                facts,
                i,
            ).dom().contains(g) <==> (f2b.dom().contains(g) && f2b[g].remove(p).len() > 0)),
        forall|g: Fct| #[trigger]
            facts.subrange(i, facts.len() as int).contains(g) && purge_reverse(
                f2b,
                p,
                facts,
                i,
            ).dom().contains(g) ==> purge_reverse(f2b, p, facts, i)[g] == f2b[g].remove(p),
    decreases facts.len() - i,
{
    let suffix = facts.subrange(i, facts.len() as int);
    if i < facts.len() {
        let f = facts[i];
        let next = purge_one(f2b, p, f);
        lemma_purge_one(f2b, p, f);
        lemma_purge_reverse(next, p, facts, i + 1);
        let rest = facts.subrange(i + 1, facts.len() as int);
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
        // `f` occurs once, so the remaining iterations never revisit it.
        assert(!rest.contains(f)) by {
            if rest.contains(f) {
                let j = choose|j: int| 0 <= j < rest.len() && rest[j] == f;
                assert(facts[i + 1 + j] == f);
            }
        }
    } else {
        assert(suffix.len() == 0);
    }
}

pub proof fn theorem_new_valid()
    ensures
        inv(new_mem()),
{
}

pub proof fn theorem_clear_valid(m: NegMem)
    ensures
        inv(clear(m)),
        clear(m).blocked.len() == 0,
        clear(m).unblocked.len() == 0,
{
}

/// Under the invariant, the reverse index answers `tokens_blocked_by` exactly
/// (negative.rs:142): `p` is listed for `f` iff `f` blocks `p`.
pub proof fn theorem_tokens_blocked_by_exact(m: NegMem, f: Fct, p: Tok)
    requires
        inv(m),
    ensures
        (m.fact_to_blocked.dom().contains(f) && m.fact_to_blocked[f].contains(p)) <==> blockers(
            m,
            p,
        ).contains(f),
{
    if m.blocked.dom().contains(p) {
        m.blocked[p].to_set_ensures();
    }
}

pub proof fn theorem_add_blocker(m: NegMem, p: Tok, f: Fct, order: u64)
    requires
        inv(m),
        !m.unblocked.dom().contains(p),
        !m.blocked.dom().contains(p) ==> fresh_order(m, p, order),
    ensures
        inv(add_blocker(m, p, f, order)),
        // refinement: blockers(p) := blockers(p) + {f}; everything else unchanged
        blockers(add_blocker(m, p, f, order), p) == blockers(m, p).insert(f),
        forall|q: Tok| q != p ==> #[trigger] blockers(add_blocker(m, p, f, order), q) == blockers(m, q),
        add_blocker(m, p, f, order).unblocked == m.unblocked,
        // chronology: the first blocker fixes the position; later ones keep it
        add_blocker(m, p, f, order).block_order[p] == (if m.blocked.dom().contains(p) {
            m.block_order[p]
        } else {
            order
        }),
        add_blocker(m, p, f, order).blocked[p][0] == (if m.blocked.dom().contains(p) {
            m.blocked[p][0]
        } else {
            f
        }),
        forall|q: Tok|
            q != p && #[trigger] m.block_order.dom().contains(q) ==> add_blocker(
                m,
                p,
                f,
                order,
            ).block_order.dom().contains(q) && add_blocker(m, p, f, order).block_order[q]
                == m.block_order[q],
{
    let r = add_blocker(m, p, f, order);
    let current = if m.blocked.dom().contains(p) {
        m.blocked[p]
    } else {
        Seq::<Fct>::empty()
    };
    lemma_os_insert(current, f);
    // chronology length: a new key is added to both maps or to neither
    if !m.blocked.dom().contains(p) {
        assert(!m.block_order.dom().contains(p)) by {
            // block_order.dom() == blocked.dom(): same size, one contained in the other
            lemma_dom_eq(m);
        }
    }
    lemma_dom_eq(m);
    assert(r.block_order.len() == r.blocked.len());
    assert forall|q: Tok, g: Fct| r.blocked.dom().contains(q) && r.blocked[q].contains(g) implies r.fact_to_blocked.dom().contains(
        g,
    ) && r.fact_to_blocked[g].contains(q) by {
        if q != p {
            assert(m.blocked[q].contains(g));
        }
    }
    assert forall|g: Fct, q: Tok| r.fact_to_blocked.dom().contains(g) && r.fact_to_blocked[g].contains(q) implies r.blocked.dom().contains(
        q,
    ) && r.blocked[q].contains(g) by {
        if g != f || q != p {
            if g == f {
                assert(m.fact_to_blocked.dom().contains(g) && m.fact_to_blocked[g].contains(q));
            }
        }
    }
    assert forall|g: Fct| r.fact_to_blocked.dom().contains(g) implies r.fact_to_blocked[g].len()
        > 0 by {
        if g == f {
            let reverse = if m.fact_to_blocked.dom().contains(f) {
                m.fact_to_blocked[f]
            } else {
                Set::<Tok>::empty()
            };
            assert(reverse.insert(p).contains(p));
        }
    }
    assert(blockers(r, p) =~= blockers(m, p).insert(f)) by {
        os_insert(current, f).to_set_ensures();
        if m.blocked.dom().contains(p) {
            m.blocked[p].to_set_ensures();
        }
    }
}

/// `block_order` and `blocked` have the same domain under the invariant.
pub proof fn lemma_dom_eq(m: NegMem)
    requires
        validate_consistency(m),
    ensures
        m.block_order.dom() =~= m.blocked.dom(),
{
    let a = m.blocked.dom();
    let b = m.block_order.dom();
    assert(a.subset_of(b));
    vstd::set_lib::lemma_len_subset(a, b);
    if !(b =~= a) {
        let x = choose|x: Tok| b.contains(x) && !a.contains(x);
        assert(a.subset_of(b.remove(x)));
        vstd::set_lib::lemma_len_subset(a, b.remove(x));
    }
}

pub proof fn theorem_remove_blocker(m: NegMem, p: Tok, f: Fct, replacement_order: u64)
    requires
        inv(m),
        m.blocked.dom().contains(p) && m.blocked[p].len() > 1 && m.blocked[p][0] == f
            ==> fresh_order(m, p, replacement_order),
    ensures
        ({
            let (r, now_unblocked) = remove_blocker(m, p, f, replacement_order);
            &&& inv(r)
            // transition signal: non-empty -> empty, and nothing else
            &&& now_unblocked == (m.blocked.dom().contains(p) && !r.blocked.dom().contains(p))
            &&& now_unblocked == (blockers(m, p).len() > 0 && blockers(r, p).len() == 0)
            &&& now_unblocked == (m.blocked.dom().contains(p) && m.blocked[p] =~= seq![f])
            // refinement: blockers(p) := blockers(p) - {f}; everything else unchanged
            &&& blockers(r, p) == blockers(m, p).remove(f)
            &&& forall|q: Tok| q != p ==> #[trigger] blockers(r, q) == blockers(m, q)
            &&& r.unblocked == m.unblocked
            // chronology
            &&& r.blocked.dom().contains(p) ==> r.blocked[p][0] == (if m.blocked[p][0] == f {
                m.blocked[p][1]
            } else {
                m.blocked[p][0]
            })
            &&& r.blocked.dom().contains(p) ==> r.block_order[p] == (if m.blocked[p][0] == f {
                replacement_order
            } else {
                m.block_order[p]
            })
            &&& forall|q: Tok|
                q != p && #[trigger] m.block_order.dom().contains(q) ==> r.block_order.dom().contains(
                    q,
                ) && r.block_order[q] == m.block_order[q]
        }),
{
    let (r, now_unblocked) = remove_blocker(m, p, f, replacement_order);
    lemma_dom_eq(m);
    lemma_purge_one(m.fact_to_blocked, p, f);
    if m.blocked.dom().contains(p) {
        let blockers0 = m.blocked[p];
        lemma_os_remove(blockers0, f);
        blockers0.to_set_ensures();
        let remaining = os_remove(blockers0, f);
        remaining.to_set_ensures();
        assert(blockers(r, p) =~= blockers(m, p).remove(f));
        if remaining.len() == 0 {
            assert(blockers0.contains(f)) by {
                if !blockers0.contains(f) {
                    assert(remaining.len() == blockers0.len());
                }
            }
            assert(blockers0 =~= seq![f]) by {
                assert(blockers0.len() == 1);
                assert(blockers0[0] == f) by {
                    let j = choose|j: int| 0 <= j < blockers0.len() && blockers0[j] == f;
                }
            }
            assert(blockers(r, p).len() == 0);
        } else {
            assert(!(blockers0 =~= seq![f])) by {
                if blockers0 =~= seq![f] {
                    assert(blockers0.contains(f));
                    assert(remaining.len() == 0);
                }
            }
            assert(blockers(r, p).contains(remaining[0]));
            if blockers0[0] == f {
                assert(blockers0.len() > 1);
            }
        }
        assert(blockers(m, p).contains(blockers0[0]));
    } else {
        assert(blockers(r, p) =~= blockers(m, p).remove(f));
    }
    assert(r.block_order.len() == r.blocked.len()) by {
        assert(r.block_order.dom() =~= r.blocked.dom());
    }
    assert forall|q: Tok, g: Fct| r.blocked.dom().contains(q) && r.blocked[q].contains(g) implies r.fact_to_blocked.dom().contains(
        g,
    ) && r.fact_to_blocked[g].contains(q) by {
        assert(m.blocked[q].contains(g));
        assert(m.fact_to_blocked[g].contains(q));
        if g == f {
            assert(q != p);
            assert(m.fact_to_blocked[f].remove(p).contains(q));
        }
    }
    assert forall|g: Fct, q: Tok| r.fact_to_blocked.dom().contains(g) && r.fact_to_blocked[g].contains(q) implies r.blocked.dom().contains(
        q,
    ) && r.blocked[q].contains(g) by {
        assert(m.fact_to_blocked[g].contains(q));
        assert(m.blocked[q].contains(g));
        if q == p {
            assert(g != f);
            assert(os_remove(m.blocked[p], f).contains(g));
            assert(os_remove(m.blocked[p], f).len() > 0);
        }
    }
    assert forall|q: Tok| q != p implies #[trigger] blockers(r, q) == blockers(m, q) by {
    }
}

pub proof fn theorem_set_unblocked(m: NegMem, p: Tok, pt: Tok)
    requires
        inv(m),
        !m.blocked.dom().contains(p),
    ensures
        inv(set_unblocked(m, p, pt)),
        set_unblocked(m, p, pt).unblocked[p] == pt,
{
}

pub proof fn theorem_remove_unblocked(m: NegMem, p: Tok)
    requires
        inv(m),
    ensures
        inv(remove_unblocked(m, p).0),
        remove_unblocked(m, p).1 == (if m.unblocked.dom().contains(p) {
            Some(m.unblocked[p])
        } else {
            None
        }),
        !remove_unblocked(m, p).0.unblocked.dom().contains(p),
{
}

pub proof fn theorem_remove_parent_token(m: NegMem, p: Tok)
    requires
        inv(m),
    ensures
        inv(remove_parent_token(m, p)),
        !remove_parent_token(m, p).blocked.dom().contains(p),
        !remove_parent_token(m, p).unblocked.dom().contains(p),
        !remove_parent_token(m, p).block_order.dom().contains(p),
        forall|f: Fct| #[trigger]
            remove_parent_token(m, p).fact_to_blocked.dom().contains(f)
                ==> !remove_parent_token(m, p).fact_to_blocked[f].contains(p),
        forall|q: Tok| q != p ==> #[trigger] blockers(remove_parent_token(m, p), q) == blockers(m, q),
        forall|q: Tok|
            q != p && #[trigger] m.block_order.dom().contains(q) ==> remove_parent_token(
                m,
                p,
            ).block_order.dom().contains(q) && remove_parent_token(m, p).block_order[q]
                == m.block_order[q],
{
    let r = remove_parent_token(m, p);
    lemma_dom_eq(m);
    let facts = if m.blocked.dom().contains(p) {
        m.blocked[p]
    } else {
        Seq::<Fct>::empty()
    };
    if m.blocked.dom().contains(p) {
        lemma_purge_reverse(m.fact_to_blocked, p, facts, 0);
    }
    assert(facts.subrange(0, facts.len() as int) =~= facts);
    // Every reverse entry of the result is the old entry, possibly without `p`.
    assert forall|g: Fct| #[trigger] r.fact_to_blocked.dom().contains(g) implies m.fact_to_blocked.dom().contains(
        g,
    ) && (r.fact_to_blocked[g] == m.fact_to_blocked[g] || r.fact_to_blocked[g]
        == m.fact_to_blocked[g].remove(p)) && !r.fact_to_blocked[g].contains(p) by {
        assert(facts.subrange(0, facts.len() as int).contains(g) == facts.contains(g));
        if !facts.contains(g) && m.fact_to_blocked[g].contains(p) {
            assert(m.blocked[p].contains(g));
        }
    }
    // Every old reverse entry for another parent survives.
    assert forall|g: Fct, q: Tok|
        q != p && m.fact_to_blocked.dom().contains(g) && #[trigger] m.fact_to_blocked[g].contains(
            q,
        ) implies r.fact_to_blocked.dom().contains(g) && r.fact_to_blocked[g].contains(q) by {
        assert(facts.subrange(0, facts.len() as int).contains(g) == facts.contains(g));
        if facts.contains(g) {
            assert(m.fact_to_blocked[g].remove(p).contains(q));
        }
    }
    assert forall|q: Tok, g: Fct| r.blocked.dom().contains(q) && #[trigger] r.blocked[q].contains(
        g,
    ) implies r.fact_to_blocked.dom().contains(g) && r.fact_to_blocked[g].contains(q) by {
        assert(m.fact_to_blocked[g].contains(q));
    }
    assert forall|g: Fct, q: Tok|
        r.fact_to_blocked.dom().contains(g) && #[trigger] r.fact_to_blocked[g].contains(
            q,
        ) implies r.blocked.dom().contains(q) && r.blocked[q].contains(g) by {
        assert(m.fact_to_blocked[g].contains(q));
    }
    assert forall|g: Fct| #[trigger] r.fact_to_blocked.dom().contains(g) implies r.fact_to_blocked[g].len()
        > 0 by {
        assert(facts.subrange(0, facts.len() as int).contains(g) == facts.contains(g));
    }
    assert forall|a: Tok, b: Tok|
        r.block_order.dom().contains(a) && r.block_order.dom().contains(b) && a
            != b implies #[trigger] r.block_order[a] != #[trigger] r.block_order[b] by {
        assert(m.block_order[a] != m.block_order[b]);
    }
    assert(r.block_order.dom() =~= r.blocked.dom());
}

} // verus!
