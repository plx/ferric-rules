//! Verus model of `BetaMemory` (crates/ferric-rules-core/src/beta.rs): token
//! membership, the NCC parent-rank index and the variable-binding index.
//!
//! What is proved (unbounded: any number of tokens, indexed variables and
//! binding values, any history of operations):
//!
//! 1. NCC parent ranks: `enable_ncc_parent_ranks`, `insert`,
//!    `insert_indexed`, `remove`, `remove_indexed` and `clear` preserve the
//!    model of `validate_ncc_parent_ranks` (every member has a rank below the
//!    counter, ranks strictly increase in insertion order, no stale ranks),
//!    including the one-shot rebase when the counter reaches `u64::MAX`.
//!    The rank conversions `u64::try_from(index).expect(..)` are modelled as
//!    plain casts; that they cannot fail is not proved here but follows from
//!    the physical bound assumed in `wf` (fewer than `u64::MAX` members).
//! 2. Variable index agrees with a naive scan: if every token is inserted and
//!    removed through `insert_indexed` / `remove_indexed` with its own
//!    bindings, then for every indexed variable `v` and key `k`,
//!    `lookup_by_var(v, k)` is `None` exactly when no member token binds `v`
//!    to `k`, and otherwise lists exactly those tokens, without duplicates.
//!    This is preserved by `insert_indexed`, `remove_indexed`, `clear`,
//!    `request_var_index` (with backfill) and `request_var_index_empty`.
//!    The index also stays canonical: no empty key lists, no empty per-variable
//!    maps, no entries for variables that are not indexed.
//!
//! Model / abstractions (the trusted base besides Verus, vstd and Z3):
//!
//! * `OrderedSet<TokenId>` is a duplicate-free `Seq` in insertion order (the
//!   refinement `ordered_set` is expected to satisfy; not proved here);
//!   `FxHashMap` is `Map`; `SmallVec<[TokenId; 4]>` and
//!   `SmallVec<[VarId; 4]>` are `Seq`. `retain` is `without`.
//! * `TokenId` is `u64`, `VarId` is `u16`, `AtomKey` is an opaque `u64` (only
//!   equality matters).
//! * The bindings of a token are immutable once it is in the token store, so
//!   the model represents `bindings.get(v).and_then(AtomKey::from_value)` for
//!   token `t` by an uninterpreted function `key_of(t, v)`; every theorem
//!   holds for every such function. Passing a token's own bindings to
//!   `insert_indexed`/`remove_indexed` is the caller protocol (rete.rs always
//!   passes `token_store.get(id).bindings`).
//! * `BetaMemoryId` and serde are omitted. Physical bound: fewer than
//!   `u64::MAX` tokens are live.
//!
//! Known gaps: model fidelity is checked by review (each spec function cites
//! its production function and mirrors its branches), not mechanically. Mixing
//! `insert` with `remove_indexed` (or vice versa) on a memory with indexed
//! variables is outside the caller protocol and not covered. The bounded Kani
//! harnesses in crates/ferric-rules-core/src/beta/kani_proofs.rs exercise the
//! production code.
use vstd::prelude::*;

verus! {

/// Models `TokenId` (crates/ferric-rules-core/src/token.rs:18).
pub type Tok = u64;

/// Models `VarId` (crates/ferric-rules-core/src/binding.rs:19).
pub type Var = u16;

/// Models `AtomKey` (crates/ferric-rules-core/src/value.rs:473).
pub type Key = u64;

/// `token_store.get(t).bindings.get(v).and_then(AtomKey::from_value)`.
pub uninterp spec fn key_of(t: Tok, v: Var) -> Option<Key>;

/// Models `OrderedSet::remove` (ordered_set.rs:96).
pub open spec fn os_remove<K>(s: Seq<K>, k: K) -> Seq<K> {
    if s.contains(k) {
        s.remove(s.index_of(k))
    } else {
        s
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
        // removal keeps the relative order of the survivors
        s.contains(k) ==> forall|a: int|
            0 <= a < os_remove(s, k).len() ==> #[trigger] os_remove(s, k)[a] == (if a < s.index_of(
                k,
            ) {
                s[a]
            } else {
                s[a + 1]
            }),
        s.contains(k) ==> 0 <= s.index_of(k) < s.len() && s[s.index_of(k)] == k,
{
    let r = os_remove(s, k);
    if s.contains(k) {
        let i = s.index_of(k);
        s.remove_ensures(i);
        assert forall|x: K| r.contains(x) <==> (s.contains(x) && x != k) by {
            if r.contains(x) {
                let j = choose|j: int| 0 <= j < r.len() && r[j] == x;
                if j < i {
                    assert(s[j] == x);
                } else {
                    assert(s[j + 1] == x);
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
    }
}

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------
/// Models `struct NccParentRanks` (beta.rs:117).
pub struct Ranks {
    pub ranks: Map<Tok, u64>,
    pub next: u64,
}

/// Models `struct BetaMemory` (beta.rs:131), without the `id` field.
pub struct BetaMem {
    /// `tokens: OrderedSet<TokenId>`
    pub tokens: Seq<Tok>,
    /// `var_indices: HashMap<VarId, HashMap<AtomKey, SmallVec<[TokenId; 4]>>>`
    pub var_indices: Map<Var, Map<Key, Seq<Tok>>>,
    /// `indexed_vars: SmallVec<[VarId; 4]>`
    pub indexed_vars: Seq<Var>,
    /// `ncc_parent_ranks: Option<Box<NccParentRanks>>`
    pub ncc_parent_ranks: Option<Ranks>,
}

/// Representation facts: `OrderedSet` and `indexed_vars` (only extended
/// after a `contains` check) are duplicate-free; fewer than `u64::MAX` tokens.
pub open spec fn wf(m: BetaMem) -> bool {
    &&& m.tokens.no_duplicates()
    &&& m.indexed_vars.no_duplicates()
    &&& m.tokens.len() < u64::MAX
}

/// Models `BetaMemory::validate_ncc_parent_ranks` (beta.rs:201). The loop
/// compares each rank with its predecessor's.
pub open spec fn validate_ncc_parent_ranks(m: BetaMem) -> bool {
    match m.ncc_parent_ranks {
        None => true,
        Some(r) => {
            // beta.rs:205-209
            &&& r.ranks.len() == m.tokens.len()
            // beta.rs:211-212: every member has a rank
            &&& forall|i: int| 0 <= i < m.tokens.len() ==> #[trigger] r.ranks.dom().contains(m.tokens[i])
            // beta.rs:213
            &&& forall|i: int| 0 <= i < m.tokens.len() ==> #[trigger] r.ranks[m.tokens[i]] < r.next
            // beta.rs:214-218: strictly increasing along the member order
            &&& forall|i: int|
                0 < i < m.tokens.len() ==> r.ranks[m.tokens[i - 1]] < #[trigger] r.ranks[m.tokens[i]]
        },
    }
}

/// `var_indices[v][k]` exists.
pub open spec fn has(vi: Map<Var, Map<Key, Seq<Tok>>>, v: Var, k: Key) -> bool {
    vi.dom().contains(v) && vi[v].dom().contains(k)
}

/// `var_indices[v][k]`, or the empty list when absent.
pub open spec fn list_at(vi: Map<Var, Map<Key, Seq<Tok>>>, v: Var, k: Key) -> Seq<Tok> {
    if has(vi, v, k) {
        vi[v][k]
    } else {
        Seq::empty()
    }
}

/// No empty per-variable maps.
pub open spec fn canonical(vi: Map<Var, Map<Key, Seq<Tok>>>) -> bool {
    forall|v: Var| #[trigger] vi.dom().contains(v) ==> vi[v].dom().len() > 0
}

/// The variable index agrees with a naive scan of the members, and is
/// canonical.
pub open spec fn index_agrees(m: BetaMem) -> bool {
    // entries only for indexed variables; no empty maps
    &&& canonical(m.var_indices)
    &&& forall|v: Var| #[trigger] m.var_indices.dom().contains(v) ==> m.indexed_vars.contains(v)
    // no empty or duplicated lists
    &&& forall|v: Var, k: Key| #[trigger]
        has(m.var_indices, v, k) ==> list_at(m.var_indices, v, k).len() > 0 && list_at(
            m.var_indices,
            v,
            k,
        ).no_duplicates()
    // for an indexed variable, the list for `k` holds exactly the members
    // whose binding has key `k`
    &&& forall|v: Var, k: Key, t: Tok|
        #![trigger list_at(m.var_indices, v, k).contains(t)]
        m.indexed_vars.contains(v) ==> (list_at(m.var_indices, v, k).contains(t) <==> (
        m.tokens.contains(t) && key_of(t, v) == Some(k)))
}

/// Models `BetaMemory::lookup_by_var` (beta.rs:276).
pub open spec fn lookup_by_var(m: BetaMem, v: Var, k: Key) -> Option<Seq<Tok>> {
    if has(m.var_indices, v, k) {
        Some(m.var_indices[v][k])
    } else {
        None
    }
}

// ---------------------------------------------------------------------------
// Operations
// ---------------------------------------------------------------------------
/// Models `BetaMemory::new` (beta.rs:149).
pub open spec fn new_mem() -> BetaMem {
    BetaMem {
        tokens: Seq::empty(),
        var_indices: Map::empty(),
        indexed_vars: Seq::empty(),
        ncc_parent_ranks: None,
    }
}

/// The ranks after `n` iterations of
/// `for (index, &token) in self.tokens.iter().enumerate() { ranks.insert(token, index) }`
/// (beta.rs:165-169 and 189-193).
pub open spec fn index_ranks(tokens: Seq<Tok>, n: int) -> Map<Tok, u64>
    decreases n,
{
    if n <= 0 {
        Map::empty()
    } else {
        index_ranks(tokens, n - 1).insert(tokens[n - 1], (n - 1) as u64)
    }
}

/// Models `BetaMemory::enable_ncc_parent_ranks` (beta.rs:162).
pub open spec fn enable_ncc_parent_ranks(m: BetaMem) -> BetaMem {
    if m.ncc_parent_ranks is None {
        BetaMem {
            ncc_parent_ranks: Some(
                Ranks {
                    ranks: index_ranks(m.tokens, m.tokens.len() as int),
                    next: m.tokens.len() as u64,
                },
            ),
            ..m
        }
    } else {
        m
    }
}

/// Models `BetaMemory::record_ncc_parent_rank` (beta.rs:181).
pub open spec fn record_ncc_parent_rank(m: BetaMem, t: Tok) -> BetaMem {
    match m.ncc_parent_ranks {
        None => m,
        Some(r) => if r.next == u64::MAX {
            // rebase: clear, renumber members in order
            BetaMem {
                ncc_parent_ranks: Some(
                    Ranks {
                        ranks: index_ranks(m.tokens, m.tokens.len() as int),
                        next: m.tokens.len() as u64,
                    },
                ),
                ..m
            }
        } else {
            BetaMem {
                ncc_parent_ranks: Some(
                    Ranks { ranks: r.ranks.insert(t, r.next), next: (r.next + 1) as u64 },
                ),
                ..m
            }
        },
    }
}

/// Models `BetaMemory::insert` (beta.rs:283): `if self.tokens.insert(t) { record }`.
pub open spec fn insert(m: BetaMem, t: Tok) -> BetaMem {
    if m.tokens.contains(t) {
        m
    } else {
        record_ncc_parent_rank(BetaMem { tokens: m.tokens.push(t), ..m }, t)
    }
}

/// `self.var_indices.entry(v).or_default().entry(k).or_default().push(t)`
/// (beta.rs:301-306 and 245-250).
pub open spec fn push_entry(vi: Map<Var, Map<Key, Seq<Tok>>>, v: Var, k: Key, t: Tok) -> Map<
    Var,
    Map<Key, Seq<Tok>>,
> {
    let km = if vi.dom().contains(v) {
        vi[v]
    } else {
        Map::empty()
    };
    let list = if km.dom().contains(k) {
        km[k]
    } else {
        Seq::empty()
    };
    vi.insert(v, km.insert(k, list.push(t)))
}

/// The `for &var_id in &self.indexed_vars` loop of `insert_indexed`
/// (beta.rs:298-309), over `vars[i..]`.
pub open spec fn index_add(vi: Map<Var, Map<Key, Seq<Tok>>>, t: Tok, vars: Seq<Var>, i: int) -> Map<
    Var,
    Map<Key, Seq<Tok>>,
>
    decreases vars.len() - i,
{
    if i < 0 || i >= vars.len() {
        vi
    } else {
        let v = vars[i];
        let next = match key_of(t, v) {
            Some(k) => push_entry(vi, v, k, t),
            None => vi,
        };
        index_add(next, t, vars, i + 1)
    }
}

/// Models `BetaMemory::insert_indexed` (beta.rs:293).
pub open spec fn insert_indexed(m: BetaMem, t: Tok) -> BetaMem {
    if m.tokens.contains(t) {
        m
    } else {
        let m1 = record_ncc_parent_rank(BetaMem { tokens: m.tokens.push(t), ..m }, t);
        BetaMem { var_indices: index_add(m1.var_indices, t, m1.indexed_vars, 0), ..m1 }
    }
}

/// Models `BetaMemory::remove` (beta.rs:315).
pub open spec fn remove(m: BetaMem, t: Tok) -> BetaMem {
    BetaMem {
        tokens: os_remove(m.tokens, t),
        ncc_parent_ranks: match m.ncc_parent_ranks {
            None => None,
            Some(r) => Some(Ranks { ranks: r.ranks.remove(t), next: r.next }),
        },
        ..m
    }
}

/// The loop body of `remove_indexed` for one variable (beta.rs:329-345).
pub open spec fn index_remove_one(vi: Map<Var, Map<Key, Seq<Tok>>>, t: Tok, v: Var) -> Map<
    Var,
    Map<Key, Seq<Tok>>,
> {
    match key_of(t, v) {
        Some(k) => if vi.dom().contains(v) {
            let km = vi[v];
            let km2 = if km.dom().contains(k) {
                // token_vec.retain(|tid| *tid != token_id)
                let list = without(km[k], t);
                if list.len() == 0 {
                    km.remove(k)
                } else {
                    km.insert(k, list)
                }
            } else {
                km
            };
            if km2.dom().len() == 0 {
                vi.remove(v)
            } else {
                vi.insert(v, km2)
            }
        } else {
            vi
        },
        None => vi,
    }
}

/// The `for &var_id in &self.indexed_vars` loop of `remove_indexed`.
pub open spec fn index_remove(vi: Map<Var, Map<Key, Seq<Tok>>>, t: Tok, vars: Seq<Var>, i: int) -> Map<
    Var,
    Map<Key, Seq<Tok>>,
>
    decreases vars.len() - i,
{
    if i < 0 || i >= vars.len() {
        vi
    } else {
        index_remove(index_remove_one(vi, t, vars[i]), t, vars, i + 1)
    }
}

/// Models `BetaMemory::remove_indexed` (beta.rs:326).
pub open spec fn remove_indexed(m: BetaMem, t: Tok) -> BetaMem {
    let m1 = remove(m, t);
    BetaMem { var_indices: index_remove(m1.var_indices, t, m1.indexed_vars, 0), ..m1 }
}

/// Models `BetaMemory::clear` (beta.rs:378).
pub open spec fn clear(m: BetaMem) -> BetaMem {
    BetaMem {
        tokens: Seq::empty(),
        var_indices: Map::empty(),
        ncc_parent_ranks: match m.ncc_parent_ranks {
            None => None,
            Some(r) => Some(Ranks { ranks: Map::empty(), next: 0 }),
        },
        ..m
    }
}

/// The backfill loop of `request_var_index` (beta.rs:235-251), over `tokens[i..]`.
pub open spec fn backfill(vi: Map<Var, Map<Key, Seq<Tok>>>, v: Var, tokens: Seq<Tok>, i: int) -> Map<
    Var,
    Map<Key, Seq<Tok>>,
>
    decreases tokens.len() - i,
{
    if i < 0 || i >= tokens.len() {
        vi
    } else {
        let t = tokens[i];
        let next = match key_of(t, v) {
            Some(k) => push_entry(vi, v, k, t),
            None => vi,
        };
        backfill(next, v, tokens, i + 1)
    }
}

/// Models `BetaMemory::request_var_index` (beta.rs:228).
pub open spec fn request_var_index(m: BetaMem, v: Var) -> BetaMem {
    if m.indexed_vars.contains(v) {
        m
    } else {
        BetaMem {
            indexed_vars: m.indexed_vars.push(v),
            var_indices: backfill(m.var_indices, v, m.tokens, 0),
            ..m
        }
    }
}

/// Models `BetaMemory::request_var_index_empty` (beta.rs:255); the `assert!`
/// becomes a precondition.
pub open spec fn request_var_index_empty(m: BetaMem, v: Var) -> BetaMem {
    if !m.indexed_vars.contains(v) {
        BetaMem { indexed_vars: m.indexed_vars.push(v), ..m }
    } else {
        m
    }
}

// ---------------------------------------------------------------------------
// Rank proofs
// ---------------------------------------------------------------------------
/// The production loop checks consecutive ranks; that implies all pairs.
pub proof fn lemma_ranks_pairwise(m: BetaMem, r: Ranks, i: int, j: int)
    requires
        m.ncc_parent_ranks == Some(r),
        validate_ncc_parent_ranks(m),
        0 <= i < j < m.tokens.len(),
    ensures
        r.ranks[m.tokens[i]] < r.ranks[m.tokens[j]],
    decreases j - i,
{
    if i + 1 < j {
        lemma_ranks_pairwise(m, r, i, j - 1);
    }
}

pub proof fn lemma_index_ranks(tokens: Seq<Tok>, n: int)
    requires
        0 <= n <= tokens.len(),
        tokens.no_duplicates(),
        tokens.len() < u64::MAX,
    ensures
        index_ranks(tokens, n).len() == n,
        forall|t: Tok| #[trigger]
            index_ranks(tokens, n).dom().contains(t) <==> tokens.subrange(0, n).contains(t),
        forall|i: int| 0 <= i < n ==> #[trigger] index_ranks(tokens, n)[tokens[i]] == i,
    decreases n,
{
    if n > 0 {
        lemma_index_ranks(tokens, n - 1);
        let prev = index_ranks(tokens, n - 1);
        assert(!prev.dom().contains(tokens[n - 1])) by {
            if prev.dom().contains(tokens[n - 1]) {
                let j = choose|j: int|
                    0 <= j < n - 1 && #[trigger] tokens.subrange(0, n - 1)[j] == tokens[n - 1];
                assert(tokens[j] == tokens[n - 1]);
            }
        }
        assert forall|t: Tok| #[trigger]
            index_ranks(tokens, n).dom().contains(t) <==> tokens.subrange(0, n).contains(t) by {
            if tokens.subrange(0, n).contains(t) {
                let j = choose|j: int| 0 <= j < n && #[trigger] tokens.subrange(0, n)[j] == t;
                if j < n - 1 {
                    assert(tokens.subrange(0, n - 1)[j] == t);
                }
            }
            if tokens.subrange(0, n - 1).contains(t) {
                let j = choose|j: int| 0 <= j < n - 1 && #[trigger] tokens.subrange(0, n - 1)[j] == t;
                assert(tokens.subrange(0, n)[j] == t);
            }
            if t == tokens[n - 1] {
                assert(tokens.subrange(0, n)[n - 1] == t);
            }
        }
        assert forall|i: int| 0 <= i < n implies #[trigger] index_ranks(tokens, n)[tokens[i]] == i by {
            if i < n - 1 {
                assert(tokens[i] != tokens[n - 1]);
            }
        }
    }
}

/// Renumbering by position satisfies the rank invariant.
pub proof fn lemma_renumbered_valid(m: BetaMem)
    requires
        wf(m),
        m.ncc_parent_ranks == Some(
            Ranks { ranks: index_ranks(m.tokens, m.tokens.len() as int), next: m.tokens.len() as u64 },
        ),
    ensures
        validate_ncc_parent_ranks(m),
{
    lemma_index_ranks(m.tokens, m.tokens.len() as int);
    assert(m.tokens.subrange(0, m.tokens.len() as int) =~= m.tokens);
    assert forall|i: int| 0 <= i < m.tokens.len() implies #[trigger] index_ranks(
        m.tokens,
        m.tokens.len() as int,
    ).dom().contains(m.tokens[i]) by {
        assert(m.tokens.subrange(0, m.tokens.len() as int)[i] == m.tokens[i]);
    }
}

/// Under the rank invariant the ranked tokens are exactly the members.
pub proof fn lemma_rank_domain(m: BetaMem, r: Ranks)
    requires
        wf(m),
        m.ncc_parent_ranks == Some(r),
        validate_ncc_parent_ranks(m),
    ensures
        forall|t: Tok| #[trigger] r.ranks.dom().contains(t) <==> m.tokens.contains(t),
{
    m.tokens.to_set_ensures();
    m.tokens.unique_seq_to_set();
    let members = m.tokens.to_set();
    assert(members.subset_of(r.ranks.dom())) by {
        assert forall|t: Tok| members.contains(t) implies r.ranks.dom().contains(t) by {
            let i = choose|i: int| 0 <= i < m.tokens.len() && m.tokens[i] == t;
        }
    }
    if !(members =~= r.ranks.dom()) {
        let x = choose|x: Tok| r.ranks.dom().contains(x) && !members.contains(x);
        assert(members.subset_of(r.ranks.dom().remove(x)));
        vstd::set_lib::lemma_len_subset(members, r.ranks.dom().remove(x));
    }
}

pub proof fn theorem_enable_ranks(m: BetaMem)
    requires
        wf(m),
        validate_ncc_parent_ranks(m),
    ensures
        validate_ncc_parent_ranks(enable_ncc_parent_ranks(m)),
        enable_ncc_parent_ranks(m).ncc_parent_ranks is Some,
{
    if m.ncc_parent_ranks is None {
        lemma_renumbered_valid(enable_ncc_parent_ranks(m));
    }
}

pub proof fn lemma_record_after_push(m: BetaMem, t: Tok)
    requires
        wf(m),
        validate_ncc_parent_ranks(m),
        !m.tokens.contains(t),
        m.tokens.len() + 1 < u64::MAX,
    ensures
        validate_ncc_parent_ranks(record_ncc_parent_rank(BetaMem { tokens: m.tokens.push(t), ..m }, t)),
        wf(BetaMem { tokens: m.tokens.push(t), ..m }),
{
    let m1 = BetaMem { tokens: m.tokens.push(t), ..m };
    assert(m1.tokens.no_duplicates()) by {
        assert forall|a: int, b: int|
            0 <= a < m1.tokens.len() && 0 <= b < m1.tokens.len() && a != b implies m1.tokens[a]
            != m1.tokens[b] by {
            if a == m.tokens.len() {
                assert(m.tokens.contains(m1.tokens[b]) ==> m1.tokens[b] != t);
            }
            if b == m.tokens.len() {
                assert(m.tokens.contains(m1.tokens[a]) ==> m1.tokens[a] != t);
            }
        }
    }
    match m.ncc_parent_ranks {
        None => {},
        Some(r) => {
            if r.next == u64::MAX {
                lemma_renumbered_valid(record_ncc_parent_rank(m1, t));
            } else {
                lemma_rank_domain(m, r);
                let m2 = record_ncc_parent_rank(m1, t);
                let r2 = m2.ncc_parent_ranks->Some_0;
                assert(!r.ranks.dom().contains(t));
                assert(r2.ranks.len() == m1.tokens.len());
                assert forall|i: int| 0 <= i < m1.tokens.len() implies #[trigger] r2.ranks.dom().contains(
                    m1.tokens[i],
                ) && r2.ranks[m1.tokens[i]] < r2.next by {
                    if i < m.tokens.len() {
                        assert(m1.tokens[i] == m.tokens[i]);
                        assert(m.tokens[i] != t);
                    }
                }
                assert forall|i: int| 0 < i < m1.tokens.len() implies r2.ranks[m1.tokens[i - 1]]
                    < #[trigger] r2.ranks[m1.tokens[i]] by {
                    assert(m1.tokens[i - 1] == m.tokens[i - 1]);
                    assert(m.tokens[i - 1] != t);
                    if i < m.tokens.len() {
                        assert(m.tokens[i] != t);
                    }
                }
            }
        },
    }
}

pub proof fn theorem_insert_ranks(m: BetaMem, t: Tok)
    requires
        wf(m),
        validate_ncc_parent_ranks(m),
        m.tokens.len() + 1 < u64::MAX,
    ensures
        validate_ncc_parent_ranks(insert(m, t)),
        wf(insert(m, t)),
        validate_ncc_parent_ranks(insert_indexed(m, t)),
        wf(insert_indexed(m, t)),
        // a newly inserted token is the newest member
        !m.tokens.contains(t) ==> insert(m, t).tokens == m.tokens.push(t),
        // with ranks enabled, the new token outranks every other member
        !m.tokens.contains(t) && m.ncc_parent_ranks is Some ==> forall|i: int|
            0 <= i < m.tokens.len() ==> (insert(m, t).ncc_parent_ranks->Some_0).ranks[m.tokens[i]]
                < #[trigger] (insert(m, t).ncc_parent_ranks->Some_0).ranks.index(t),
{
    if !m.tokens.contains(t) {
        lemma_record_after_push(m, t);
        let m2 = insert(m, t);
        if m.ncc_parent_ranks is Some {
            let r2 = m2.ncc_parent_ranks->Some_0;
            assert(m2.tokens[m.tokens.len() as int] == t);
            assert forall|i: int| 0 <= i < m.tokens.len() implies r2.ranks[m.tokens[i]]
                < #[trigger] r2.ranks.index(t) by {
                assert(m2.tokens[i] == m.tokens[i]);
                lemma_ranks_pairwise(m2, r2, i, m.tokens.len() as int);
            }
        }
    }
}

/// Removing a member keeps the remaining ranks valid.
pub proof fn lemma_remove_ranks_present(m: BetaMem, r: Ranks, t: Tok)
    requires
        wf(m),
        m.ncc_parent_ranks == Some(r),
        validate_ncc_parent_ranks(m),
        m.tokens.contains(t),
    ensures
        validate_ncc_parent_ranks(remove(m, t)),
{
    lemma_os_remove(m.tokens, t);
    lemma_rank_domain(m, r);
    let s0 = m.tokens;
    let s1 = remove(m, t).tokens;
    let r1 = Ranks { ranks: r.ranks.remove(t), next: r.next };
    assert(remove(m, t).ncc_parent_ranks == Some(r1));
    let k = s0.index_of(t);
    assert(0 <= k < s0.len() && s0[k] == t);
    assert(s1.len() == s0.len() - 1);
    assert forall|i: int| 0 <= i < s1.len() implies #[trigger] s1[i] == s0[if i < k {
        i
    } else {
        i + 1
    }] && s1[i] != t by {
        let j = if i < k {
            i
        } else {
            i + 1
        };
        assert(s1[i] == s0[j]);
        assert(j != k);
    }
    assert(r.ranks.dom().contains(t));
    assert(r1.ranks.len() == s1.len());
    assert forall|i: int| 0 <= i < s1.len() implies #[trigger] r1.ranks.dom().contains(s1[i])
        && r1.ranks[s1[i]] < r1.next by {
        let j = if i < k {
            i
        } else {
            i + 1
        };
        assert(s1[i] == s0[j]);
        assert(r.ranks.dom().contains(s0[j]));
        assert(r.ranks[s0[j]] < r.next);
    }
    assert forall|i: int| 0 < i < s1.len() implies r1.ranks[s1[i - 1]] < #[trigger] r1.ranks[s1[i]] by {
        let a = if i - 1 < k {
            i - 1
        } else {
            i
        };
        let b = if i < k {
            i
        } else {
            i + 1
        };
        assert(s1[i - 1] == s0[a]);
        assert(s1[i] == s0[b]);
        lemma_ranks_pairwise(m, r, a, b);
    }
}

pub proof fn theorem_remove_ranks(m: BetaMem, t: Tok)
    requires
        wf(m),
        validate_ncc_parent_ranks(m),
    ensures
        validate_ncc_parent_ranks(remove(m, t)),
        wf(remove(m, t)),
        !remove(m, t).tokens.contains(t),
{
    lemma_os_remove(m.tokens, t);
    match m.ncc_parent_ranks {
        None => {},
        Some(r) => {
            if m.tokens.contains(t) {
                lemma_remove_ranks_present(m, r, t);
            } else {
                lemma_rank_domain(m, r);
                assert(!r.ranks.dom().contains(t));
                assert(r.ranks.remove(t) =~= r.ranks);
                assert(remove(m, t).tokens =~= m.tokens);
            }
        },
    }
}

/// `remove_indexed` changes members and ranks exactly like `remove`.
pub proof fn theorem_remove_indexed_ranks(m: BetaMem, t: Tok)
    requires
        wf(m),
        validate_ncc_parent_ranks(m),
    ensures
        validate_ncc_parent_ranks(remove_indexed(m, t)),
        wf(remove_indexed(m, t)),
{
    theorem_remove_ranks(m, t);
    let a = remove_indexed(m, t);
    let b = remove(m, t);
    assert(a.tokens == b.tokens && a.ncc_parent_ranks == b.ncc_parent_ranks && a.indexed_vars
        == b.indexed_vars);
}

pub proof fn theorem_clear_ranks(m: BetaMem)
    requires
        wf(m),
    ensures
        validate_ncc_parent_ranks(clear(m)),
        wf(clear(m)),
        index_agrees(clear(m)),
        clear(m).indexed_vars == m.indexed_vars,
        clear(m).ncc_parent_ranks is Some == m.ncc_parent_ranks is Some,
{
}

// ---------------------------------------------------------------------------
// Variable-index proofs
// ---------------------------------------------------------------------------
/// `v` occurs in `vars[i..]`.
pub open spec fn in_suffix(vars: Seq<Var>, i: int, v: Var) -> bool {
    exists|j: int| i <= j < vars.len() && #[trigger] vars[j] == v
}

/// The insertion/removal pass over `vars[i..]` touches `(v, k)` for token `t`.
pub open spec fn hit(vars: Seq<Var>, i: int, t: Tok, v: Var, k: Key) -> bool {
    in_suffix(vars, i, v) && key_of(t, v) == Some(k)
}

pub proof fn lemma_in_suffix_step(vars: Seq<Var>, i: int)
    requires
        0 <= i < vars.len(),
        vars.no_duplicates(),
    ensures
        forall|v: Var| #[trigger]
            in_suffix(vars, i, v) <==> (v == vars[i] || in_suffix(vars, i + 1, v)),
        !in_suffix(vars, i + 1, vars[i]),
        forall|v: Var| #[trigger] in_suffix(vars, 0, v) <==> vars.contains(v),
{
    assert forall|v: Var| #[trigger]
        in_suffix(vars, i, v) <==> (v == vars[i] || in_suffix(vars, i + 1, v)) by {
        if v == vars[i] {
            assert(vars[i] == v);
        }
    }
    if in_suffix(vars, i + 1, vars[i]) {
        let j = choose|j: int| i + 1 <= j < vars.len() && #[trigger] vars[j] == vars[i];
        assert(vars[j] == vars[i]);
    }
    assert forall|v: Var| #[trigger] in_suffix(vars, 0, v) <==> vars.contains(v) by {
        if vars.contains(v) {
            let j = choose|j: int| 0 <= j < vars.len() && vars[j] == v;
            assert(vars[j] == v);
        }
    }
}

pub proof fn lemma_push_entry(vi: Map<Var, Map<Key, Seq<Tok>>>, v0: Var, k0: Key, t: Tok)
    ensures
        forall|v: Var| #[trigger]
            push_entry(vi, v0, k0, t).dom().contains(v) <==> (vi.dom().contains(v) || v == v0),
        forall|v: Var, k: Key| #[trigger]
            has(push_entry(vi, v0, k0, t), v, k) <==> (has(vi, v, k) || (v == v0 && k == k0)),
        forall|v: Var, k: Key| #[trigger]
            list_at(push_entry(vi, v0, k0, t), v, k) == (if v == v0 && k == k0 {
                list_at(vi, v, k).push(t)
            } else {
                list_at(vi, v, k)
            }),
{
}

/// The insertion loop adds `t` under `(v, key_of(t, v))` for each `v` in
/// `vars[i..]` and leaves every other list unchanged.
pub proof fn lemma_index_add(vi: Map<Var, Map<Key, Seq<Tok>>>, t: Tok, vars: Seq<Var>, i: int)
    requires
        0 <= i <= vars.len(),
        vars.no_duplicates(),
    ensures
        forall|v: Var| #[trigger]
            index_add(vi, t, vars, i).dom().contains(v) <==> (vi.dom().contains(v) || (in_suffix(
                vars,
                i,
                v,
            ) && key_of(t, v) is Some)),
        forall|v: Var, k: Key| #[trigger]
            has(index_add(vi, t, vars, i), v, k) <==> (has(vi, v, k) || hit(vars, i, t, v, k)),
        forall|v: Var, k: Key| #[trigger]
            list_at(index_add(vi, t, vars, i), v, k) == (if hit(vars, i, t, v, k) {
                list_at(vi, v, k).push(t)
            } else {
                list_at(vi, v, k)
            }),
    decreases vars.len() - i,
{
    if i < vars.len() {
        let v0 = vars[i];
        let next = match key_of(t, v0) {
            Some(k) => push_entry(vi, v0, k, t),
            None => vi,
        };
        lemma_in_suffix_step(vars, i);
        lemma_index_add(next, t, vars, i + 1);
        if let Some(k0) = key_of(t, v0) {
            lemma_push_entry(vi, v0, k0, t);
        }
        let r = index_add(vi, t, vars, i);
        assert(r == index_add(next, t, vars, i + 1));
        assert forall|v: Var, k: Key| #[trigger]
            list_at(r, v, k) == (if hit(vars, i, t, v, k) {
                list_at(vi, v, k).push(t)
            } else {
                list_at(vi, v, k)
            }) by {
            assert(list_at(r, v, k) == (if hit(vars, i + 1, t, v, k) {
                list_at(next, v, k).push(t)
            } else {
                list_at(next, v, k)
            }));
        }
        assert forall|v: Var, k: Key| #[trigger]
            has(r, v, k) <==> (has(vi, v, k) || hit(vars, i, t, v, k)) by {
            assert(has(r, v, k) <==> (has(next, v, k) || hit(vars, i + 1, t, v, k)));
        }
        assert forall|v: Var| #[trigger]
            r.dom().contains(v) <==> (vi.dom().contains(v) || (in_suffix(vars, i, v) && key_of(
                t,
                v,
            ) is Some)) by {
            assert(r.dom().contains(v) <==> (next.dom().contains(v) || (in_suffix(vars, i + 1, v)
                && key_of(t, v) is Some)));
        }
    }
}

pub proof fn lemma_remove_one(vi: Map<Var, Map<Key, Seq<Tok>>>, t: Tok, v0: Var)
    requires
        canonical(vi),
    ensures
        canonical(index_remove_one(vi, t, v0)),
        forall|v: Var| #[trigger]
            index_remove_one(vi, t, v0).dom().contains(v) ==> vi.dom().contains(v),
        forall|v: Var, k: Key| #[trigger]
            has(index_remove_one(vi, t, v0), v, k) <==> (has(vi, v, k) && !(v == v0 && key_of(t, v0)
                == Some(k) && without(list_at(vi, v, k), t).len() == 0)),
        forall|v: Var, k: Key| #[trigger]
            list_at(index_remove_one(vi, t, v0), v, k) == (if v == v0 && key_of(t, v0) == Some(k) {
                without(list_at(vi, v, k), t)
            } else {
                list_at(vi, v, k)
            }),
{
    let r = index_remove_one(vi, t, v0);
    assert(without(Seq::<Tok>::empty(), t) == Seq::<Tok>::empty());
    if let Some(k0) = key_of(t, v0) {
        if vi.dom().contains(v0) {
            let km = vi[v0];
            let km2 = if km.dom().contains(k0) {
                let list = without(km[k0], t);
                if list.len() == 0 {
                    km.remove(k0)
                } else {
                    km.insert(k0, list)
                }
            } else {
                km
            };
            assert forall|k: Key| k != k0 && km.dom().contains(k) implies #[trigger] km2.dom().contains(
                k,
            ) && km2[k] == km[k] by {}
            assert forall|k: Key| k != k0 && #[trigger] km.dom().contains(k) implies km2.dom().len()
                > 0 by {
                assert(km2.dom().contains(k));
            }
            if !km.dom().contains(k0) {
                let k1 = km.dom().choose();
                assert(km.dom().contains(k1));
                assert(km2.dom().len() > 0);
            }
            if km2.dom().len() > 0 {
                assert(r.dom().contains(v0) && r[v0] == km2);
            } else {
                assert(!r.dom().contains(v0));
                assert forall|k: Key| !km.dom().contains(k) || k == k0 by {
                    if k != k0 && km.dom().contains(k) {
                        assert(km2.dom().len() > 0);
                    }
                }
            }
        }
    }
}

/// The removal loop deletes `t` from `(v, key_of(t, v))` for each `v` in
/// `vars[i..]`, dropping emptied lists and maps, and leaves the rest alone.
pub proof fn lemma_index_remove(vi: Map<Var, Map<Key, Seq<Tok>>>, t: Tok, vars: Seq<Var>, i: int)
    requires
        0 <= i <= vars.len(),
        vars.no_duplicates(),
        canonical(vi),
    ensures
        canonical(index_remove(vi, t, vars, i)),
        forall|v: Var| #[trigger]
            index_remove(vi, t, vars, i).dom().contains(v) ==> vi.dom().contains(v),
        forall|v: Var, k: Key| #[trigger]
            has(index_remove(vi, t, vars, i), v, k) <==> (has(vi, v, k) && !(hit(vars, i, t, v, k)
                && without(list_at(vi, v, k), t).len() == 0)),
        forall|v: Var, k: Key| #[trigger]
            list_at(index_remove(vi, t, vars, i), v, k) == (if hit(vars, i, t, v, k) {
                without(list_at(vi, v, k), t)
            } else {
                list_at(vi, v, k)
            }),
    decreases vars.len() - i,
{
    if i < vars.len() {
        let v0 = vars[i];
        let next = index_remove_one(vi, t, v0);
        lemma_in_suffix_step(vars, i);
        lemma_remove_one(vi, t, v0);
        lemma_index_remove(next, t, vars, i + 1);
        let r = index_remove(vi, t, vars, i);
        assert(r == index_remove(next, t, vars, i + 1));
        assert forall|v: Var, k: Key| #[trigger]
            list_at(r, v, k) == (if hit(vars, i, t, v, k) {
                without(list_at(vi, v, k), t)
            } else {
                list_at(vi, v, k)
            }) by {
            assert(list_at(r, v, k) == (if hit(vars, i + 1, t, v, k) {
                without(list_at(next, v, k), t)
            } else {
                list_at(next, v, k)
            }));
        }
        assert forall|v: Var, k: Key| #[trigger]
            has(r, v, k) <==> (has(vi, v, k) && !(hit(vars, i, t, v, k) && without(
                list_at(vi, v, k),
                t,
            ).len() == 0)) by {
            assert(has(r, v, k) <==> (has(next, v, k) && !(hit(vars, i + 1, t, v, k) && without(
                list_at(next, v, k),
                t,
            ).len() == 0)));
        }
    } else {
        assert forall|v: Var, k: Key| !hit(vars, i, t, v, k) by {}
    }
}

/// Models `SmallVec::retain(|tid| *tid != token_id)` (beta.rs:333): keeps
/// every other element, in order.
pub open spec fn without(s: Seq<Tok>, t: Tok) -> Seq<Tok>
    decreases s.len(),
{
    if s.len() == 0 {
        s
    } else {
        let init = without(s.drop_last(), t);
        if s.last() == t {
            init
        } else {
            init.push(s.last())
        }
    }
}

/// Retaining all but `t` keeps a duplicate-free list duplicate-free and
/// removes exactly `t`.
pub proof fn lemma_without(s: Seq<Tok>, t: Tok)
    requires
        s.no_duplicates(),
    ensures
        without(s, t).no_duplicates(),
        forall|x: Tok| #[trigger] without(s, t).contains(x) <==> (s.contains(x) && x != t),
    decreases s.len(),
{
    if s.len() > 0 {
        let init = s.drop_last();
        assert(init.no_duplicates());
        lemma_without(init, t);
        let fi = without(init, t);
        let f = without(s, t);
        assert(!init.contains(s.last())) by {
            if init.contains(s.last()) {
                let j = choose|j: int| 0 <= j < init.len() && init[j] == s.last();
                assert(s[j] == s[s.len() - 1]);
            }
        }
        assert forall|x: Tok| s.contains(x) <==> (init.contains(x) || x == s.last()) by {
            if s.contains(x) {
                let j = choose|j: int| 0 <= j < s.len() && s[j] == x;
                if j < init.len() {
                    assert(init[j] == x);
                }
            }
            if init.contains(x) {
                let j = choose|j: int| 0 <= j < init.len() && init[j] == x;
                assert(s[j] == x);
            }
        }
        if s.last() != t {
            assert(f == fi.push(s.last()));
            assert forall|x: Tok| #[trigger] f.contains(x) <==> (fi.contains(x) || x == s.last()) by {
                if f.contains(x) {
                    let j = choose|j: int| 0 <= j < f.len() && f[j] == x;
                    if j < fi.len() {
                        assert(fi[j] == x);
                    }
                }
                if fi.contains(x) {
                    let j = choose|j: int| 0 <= j < fi.len() && fi[j] == x;
                    assert(f[j] == x);
                }
                if x == s.last() {
                    assert(f[fi.len() as int] == x);
                }
            }
            assert(!fi.contains(s.last()));
            assert forall|a: int, b: int|
                0 <= a < f.len() && 0 <= b < f.len() && a != b implies f[a] != f[b] by {
                if a == fi.len() {
                    assert(fi[b] == f[b]);
                }
                if b == fi.len() {
                    assert(fi[a] == f[a]);
                }
            }
        }
    }
}

/// Under the invariant, `lookup_by_var` answers exactly like a scan of the
/// members.
pub proof fn theorem_lookup_matches_scan(m: BetaMem, v: Var, k: Key)
    requires
        index_agrees(m),
        m.indexed_vars.contains(v),
    ensures
        lookup_by_var(m, v, k) is None <==> !(exists|t: Tok|
            #[trigger] m.tokens.contains(t) && key_of(t, v) == Some(k)),
        lookup_by_var(m, v, k) matches Some(list) ==> list.no_duplicates() && forall|t: Tok|
            #[trigger] list.contains(t) <==> (m.tokens.contains(t) && key_of(t, v) == Some(k)),
{
    let list = list_at(m.var_indices, v, k);
    if has(m.var_indices, v, k) {
        assert(list.contains(list[0]));
    } else {
        if exists|t: Tok| #[trigger] m.tokens.contains(t) && key_of(t, v) == Some(k) {
            let t = choose|t: Tok| #[trigger] m.tokens.contains(t) && key_of(t, v) == Some(k);
            assert(list.contains(t));
        }
    }
}

pub proof fn theorem_insert_indexed_index(m: BetaMem, t: Tok)
    requires
        wf(m),
        index_agrees(m),
        m.tokens.len() + 1 < u64::MAX,
    ensures
        index_agrees(insert_indexed(m, t)),
{
    if !m.tokens.contains(t) {
        let r = insert_indexed(m, t);
        let vars = m.indexed_vars;
        let vi = m.var_indices;
        lemma_index_add(vi, t, vars, 0);
        if vars.len() > 0 {
            lemma_in_suffix_step(vars, 0);
        } else {
            assert forall|v: Var| #[trigger] in_suffix(vars, 0, v) <==> vars.contains(v) by {}
        }
        assert(r.tokens == m.tokens.push(t));
        assert(r.indexed_vars == vars);
        assert forall|x: Tok| #[trigger] r.tokens.contains(x) <==> (m.tokens.contains(x) || x == t) by {
            if r.tokens.contains(x) {
                let j = choose|j: int| 0 <= j < r.tokens.len() && r.tokens[j] == x;
                if j < m.tokens.len() {
                    assert(m.tokens[j] == x);
                }
            }
            if m.tokens.contains(x) {
                let j = choose|j: int| 0 <= j < m.tokens.len() && m.tokens[j] == x;
                assert(r.tokens[j] == x);
            }
            if x == t {
                assert(r.tokens[m.tokens.len() as int] == x);
            }
        }
        assert forall|v: Var| #[trigger] r.var_indices.dom().contains(v) implies r.indexed_vars.contains(
            v,
        ) && r.var_indices[v].dom().len() > 0 by {
            if vi.dom().contains(v) {
                let k = vi[v].dom().choose();
                assert(vi[v].dom().contains(k));
                assert(has(vi, v, k));
                assert(has(r.var_indices, v, k));
            } else {
                let k = key_of(t, v)->Some_0;
                assert(hit(vars, 0, t, v, k));
                assert(has(r.var_indices, v, k));
            }
        }
        assert forall|v: Var, k: Key| #[trigger] has(r.var_indices, v, k) implies list_at(
            r.var_indices,
            v,
            k,
        ).len() > 0 && list_at(r.var_indices, v, k).no_duplicates() by {
            let old = list_at(vi, v, k);
            if hit(vars, 0, t, v, k) {
                assert(!old.contains(t));
                let l = old.push(t);
                assert forall|a: int, b: int|
                    0 <= a < l.len() && 0 <= b < l.len() && a != b implies l[a] != l[b] by {
                    if a == old.len() {
                        assert(old[b] == l[b]);
                    }
                    if b == old.len() {
                        assert(old[a] == l[a]);
                    }
                }
            }
        }
        assert forall|v: Var, k: Key, x: Tok| r.indexed_vars.contains(v) implies (#[trigger] list_at(
            r.var_indices,
            v,
            k,
        ).contains(x) <==> (r.tokens.contains(x) && key_of(x, v) == Some(k))) by {
            let old = list_at(vi, v, k);
            assert(old.contains(x) <==> (m.tokens.contains(x) && key_of(x, v) == Some(k)));
            if hit(vars, 0, t, v, k) {
                let l = old.push(t);
                if l.contains(x) {
                    let j = choose|j: int| 0 <= j < l.len() && l[j] == x;
                    if j < old.len() {
                        assert(old[j] == x);
                    }
                }
                if old.contains(x) {
                    let j = choose|j: int| 0 <= j < old.len() && old[j] == x;
                    assert(l[j] == x);
                }
                if x == t {
                    assert(l[old.len() as int] == x);
                }
            } else {
                assert(old.contains(t) ==> m.tokens.contains(t));
            }
        }
    }
}

pub proof fn theorem_remove_indexed_index(m: BetaMem, t: Tok)
    requires
        wf(m),
        index_agrees(m),
    ensures
        index_agrees(remove_indexed(m, t)),
{
    let r = remove_indexed(m, t);
    let vars = m.indexed_vars;
    let vi = m.var_indices;
    lemma_os_remove(m.tokens, t);
    lemma_index_remove(vi, t, vars, 0);
    if vars.len() > 0 {
        lemma_in_suffix_step(vars, 0);
    } else {
        assert forall|v: Var| #[trigger] in_suffix(vars, 0, v) <==> vars.contains(v) by {}
    }
    assert(r.indexed_vars == vars);
    assert forall|v: Var, k: Key| #[trigger] has(r.var_indices, v, k) implies list_at(
        r.var_indices,
        v,
        k,
    ).len() > 0 && list_at(r.var_indices, v, k).no_duplicates() by {
        let old = list_at(vi, v, k);
        assert(has(vi, v, k));
        lemma_without(old, t);
    }
    assert forall|v: Var, k: Key, x: Tok| r.indexed_vars.contains(v) implies (#[trigger] list_at(
        r.var_indices,
        v,
        k,
    ).contains(x) <==> (r.tokens.contains(x) && key_of(x, v) == Some(k))) by {
        let old = list_at(vi, v, k);
        assert(old.contains(x) <==> (m.tokens.contains(x) && key_of(x, v) == Some(k)));
        if has(vi, v, k) {
            lemma_without(old, t);
        } else {
            assert(old == Seq::<Tok>::empty());
            assert(without(old, t) == old);
        }
        if !hit(vars, 0, t, v, k) {
            assert(old.contains(t) ==> key_of(t, v) == Some(k));
        }
    }
}

/// Backfilling `v` over `tokens[i..]` (beta.rs:235-251) lists exactly the
/// tokens of `tokens[i..]` whose key for `v` is `k`, after what was already
/// there, and leaves other variables alone.
pub proof fn lemma_backfill(vi: Map<Var, Map<Key, Seq<Tok>>>, v: Var, tokens: Seq<Tok>, i: int)
    requires
        0 <= i <= tokens.len(),
        tokens.no_duplicates(),
        forall|k: Key, x: Tok| #[trigger]
            list_at(vi, v, k).contains(x) ==> exists|j: int| 0 <= j < i && #[trigger] tokens[j] == x,
        forall|k: Key| #[trigger] list_at(vi, v, k).no_duplicates(),
    ensures
        forall|w: Var| #[trigger]
            backfill(vi, v, tokens, i).dom().contains(w) <==> (vi.dom().contains(w) || (w == v
                && exists|j: int| i <= j < tokens.len() && #[trigger] key_of(tokens[j], v) is Some)),
        forall|w: Var, k: Key| #[trigger]
            has(backfill(vi, v, tokens, i), w, k) <==> (has(vi, w, k) || (w == v && exists|j: int|
                i <= j < tokens.len() && #[trigger] key_of(tokens[j], v) == Some(k))),
        forall|w: Var, k: Key|
            w != v ==> #[trigger] list_at(backfill(vi, v, tokens, i), w, k) == list_at(vi, w, k),
        forall|k: Key, x: Tok| #[trigger]
            list_at(backfill(vi, v, tokens, i), v, k).contains(x) <==> (list_at(vi, v, k).contains(x)
                || (exists|j: int| i <= j < tokens.len() && #[trigger] tokens[j] == x) && key_of(x, v)
                == Some(k)),
        forall|k: Key| #[trigger] list_at(backfill(vi, v, tokens, i), v, k).no_duplicates(),
    decreases tokens.len() - i,
{
    if i < tokens.len() {
        let t = tokens[i];
        let next = match key_of(t, v) {
            Some(k) => push_entry(vi, v, k, t),
            None => vi,
        };
        if let Some(k0) = key_of(t, v) {
            lemma_push_entry(vi, v, k0, t);
        }
        assert forall|k: Key| #[trigger] list_at(vi, v, k).contains(t) implies false by {
            let j = choose|j: int| 0 <= j < i && #[trigger] tokens[j] == t;
            assert(tokens[j] == tokens[i]);
        }
        assert forall|k: Key, x: Tok| #[trigger] list_at(next, v, k).contains(x) implies exists|j: int|
            0 <= j < i + 1 && #[trigger] tokens[j] == x by {
            if list_at(vi, v, k).contains(x) {
                let j = choose|j: int| 0 <= j < i && #[trigger] tokens[j] == x;
                assert(tokens[j] == x);
            } else {
                assert(key_of(t, v) == Some(k));
                let l = list_at(vi, v, k).push(t);
                let j = choose|j: int| 0 <= j < l.len() && l[j] == x;
                assert(tokens[i] == x);
            }
        }
        assert forall|k: Key| #[trigger] list_at(next, v, k).no_duplicates() by {
            if key_of(t, v) == Some(k) {
                let old = list_at(vi, v, k);
                let l = old.push(t);
                assert(!old.contains(t));
                assert(old.no_duplicates());
                assert forall|a: int, b: int|
                    0 <= a < l.len() && 0 <= b < l.len() && a != b implies l[a] != l[b] by {
                    if a == old.len() {
                        assert(l[a] == t);
                        assert(old[b] == l[b]);
                        assert(old.contains(old[b]));
                    } else if b == old.len() {
                        assert(l[b] == t);
                        assert(old[a] == l[a]);
                        assert(old.contains(old[a]));
                    } else {
                        assert(old[a] == l[a]);
                        assert(old[b] == l[b]);
                    }
                }
                assert(list_at(next, v, k) == l);
            }
        }
        lemma_backfill(next, v, tokens, i + 1);
        let r = backfill(vi, v, tokens, i);
        assert(r == backfill(next, v, tokens, i + 1));
        assert forall|w: Var| #[trigger]
            r.dom().contains(w) <==> (vi.dom().contains(w) || (w == v && exists|j: int|
                i <= j < tokens.len() && #[trigger] key_of(tokens[j], v) is Some)) by {
            if w == v && (exists|j: int| i <= j < tokens.len() && #[trigger] key_of(tokens[j], v) is Some) {
                let j = choose|j: int| i <= j < tokens.len() && #[trigger] key_of(tokens[j], v) is Some;
                if j > i {
                    assert(key_of(tokens[j], v) is Some);
                }
            }
            if w == v && (exists|j: int|
                i + 1 <= j < tokens.len() && #[trigger] key_of(tokens[j], v) is Some) {
                let j = choose|j: int|
                    i + 1 <= j < tokens.len() && #[trigger] key_of(tokens[j], v) is Some;
                assert(key_of(tokens[j], v) is Some);
            }
        }
        assert forall|w: Var, k: Key| #[trigger]
            has(r, w, k) <==> (has(vi, w, k) || (w == v && exists|j: int|
                i <= j < tokens.len() && #[trigger] key_of(tokens[j], v) == Some(k))) by {
            if w == v && (exists|j: int|
                i <= j < tokens.len() && #[trigger] key_of(tokens[j], v) == Some(k)) {
                let j = choose|j: int|
                    i <= j < tokens.len() && #[trigger] key_of(tokens[j], v) == Some(k);
                if j > i {
                    assert(key_of(tokens[j], v) == Some(k));
                }
            }
            if w == v && (exists|j: int|
                i + 1 <= j < tokens.len() && #[trigger] key_of(tokens[j], v) == Some(k)) {
                let j = choose|j: int|
                    i + 1 <= j < tokens.len() && #[trigger] key_of(tokens[j], v) == Some(k);
                assert(key_of(tokens[j], v) == Some(k));
            }
        }
        assert forall|k: Key, x: Tok| #[trigger]
            list_at(r, v, k).contains(x) <==> (list_at(vi, v, k).contains(x) || (exists|j: int|
                i <= j < tokens.len() && #[trigger] tokens[j] == x) && key_of(x, v) == Some(k)) by {
            if exists|j: int| i <= j < tokens.len() && #[trigger] tokens[j] == x {
                let j = choose|j: int| i <= j < tokens.len() && #[trigger] tokens[j] == x;
                if j > i {
                    assert(tokens[j] == x);
                }
            }
            if exists|j: int| i + 1 <= j < tokens.len() && #[trigger] tokens[j] == x {
                let j = choose|j: int| i + 1 <= j < tokens.len() && #[trigger] tokens[j] == x;
                assert(tokens[j] == x);
            }
            if x == t && key_of(x, v) == Some(k) {
                assert(list_at(next, v, k).contains(x)) by {
                    assert(list_at(next, v, k)[list_at(vi, v, k).len() as int] == x);
                }
            }
            if list_at(next, v, k).contains(x) && !list_at(vi, v, k).contains(x) {
                let l = list_at(next, v, k);
                assert(key_of(t, v) == Some(k));
                let j = choose|j: int| 0 <= j < l.len() && l[j] == x;
                assert(x == t);
            }
            if list_at(vi, v, k).contains(x) && key_of(t, v) == Some(k) {
                let old = list_at(vi, v, k);
                let j = choose|j: int| 0 <= j < old.len() && old[j] == x;
                assert(list_at(next, v, k)[j] == x);
            }
        }
    }
}

pub proof fn theorem_request_var_index(m: BetaMem, v: Var)
    requires
        wf(m),
        index_agrees(m),
    ensures
        index_agrees(request_var_index(m, v)),
        wf(request_var_index(m, v)),
        request_var_index(m, v).indexed_vars.contains(v),
{
    if !m.indexed_vars.contains(v) {
        let r = request_var_index(m, v);
        let vi = m.var_indices;
        assert(!vi.dom().contains(v));
        assert forall|k: Key| #[trigger] list_at(vi, v, k) == Seq::<Tok>::empty() by {}
        lemma_backfill(vi, v, m.tokens, 0);
        let vars2 = m.indexed_vars.push(v);
        assert forall|w: Var| #[trigger] vars2.contains(w) <==> (m.indexed_vars.contains(w) || w == v) by {
            if vars2.contains(w) {
                let j = choose|j: int| 0 <= j < vars2.len() && vars2[j] == w;
                if j < m.indexed_vars.len() {
                    assert(m.indexed_vars[j] == w);
                }
            }
            if m.indexed_vars.contains(w) {
                let j = choose|j: int| 0 <= j < m.indexed_vars.len() && m.indexed_vars[j] == w;
                assert(vars2[j] == w);
            }
            if w == v {
                assert(vars2[m.indexed_vars.len() as int] == w);
            }
        }
        assert(vars2.no_duplicates()) by {
            assert forall|a: int, b: int|
                0 <= a < vars2.len() && 0 <= b < vars2.len() && a != b implies vars2[a]
                != vars2[b] by {
                if a == m.indexed_vars.len() {
                    assert(m.indexed_vars[b] == vars2[b]);
                }
                if b == m.indexed_vars.len() {
                    assert(m.indexed_vars[a] == vars2[a]);
                }
            }
        }
        assert forall|w: Var| #[trigger] r.var_indices.dom().contains(w) implies r.var_indices[w].dom().len()
            > 0 by {
            if w == v {
                let j = choose|j: int| 0 <= j < m.tokens.len() && #[trigger] key_of(m.tokens[j], v) is Some;
                let k = key_of(m.tokens[j], v)->Some_0;
                assert(key_of(m.tokens[j], v) == Some(k));
                assert(has(r.var_indices, v, k));
            } else {
                let k = vi[w].dom().choose();
                assert(vi[w].dom().contains(k));
                assert(has(vi, w, k));
                assert(has(r.var_indices, w, k));
            }
        }
        assert forall|w: Var, k: Key| #[trigger] has(r.var_indices, w, k) implies list_at(
            r.var_indices,
            w,
            k,
        ).len() > 0 && list_at(r.var_indices, w, k).no_duplicates() by {
            if w == v {
                let j = choose|j: int|
                    0 <= j < m.tokens.len() && #[trigger] key_of(m.tokens[j], v) == Some(k);
                assert(m.tokens[j] == m.tokens[j]);
                assert(list_at(r.var_indices, v, k).contains(m.tokens[j]));
            }
        }
        assert forall|w: Var, k: Key, x: Tok| r.indexed_vars.contains(w) implies (#[trigger] list_at(
            r.var_indices,
            w,
            k,
        ).contains(x) <==> (r.tokens.contains(x) && key_of(x, w) == Some(k))) by {
            if w == v {
                if m.tokens.contains(x) {
                    let j = choose|j: int| 0 <= j < m.tokens.len() && m.tokens[j] == x;
                    assert(m.tokens[j] == x);
                }
            }
        }
    }
}

pub proof fn theorem_request_var_index_empty(m: BetaMem, v: Var)
    requires
        wf(m),
        index_agrees(m),
        m.tokens.len() == 0,
    ensures
        index_agrees(request_var_index_empty(m, v)),
        request_var_index_empty(m, v).indexed_vars.contains(v),
{
    let r = request_var_index_empty(m, v);
    if !m.indexed_vars.contains(v) {
        assert(r.indexed_vars[m.indexed_vars.len() as int] == v);
        assert forall|w: Var| #[trigger] r.indexed_vars.contains(w) implies (m.indexed_vars.contains(w)
            || w == v) by {
            let j = choose|j: int| 0 <= j < r.indexed_vars.len() && r.indexed_vars[j] == w;
            if j < m.indexed_vars.len() {
                assert(m.indexed_vars[j] == w);
            }
        }
        assert forall|w: Var| #[trigger] m.indexed_vars.contains(w) implies r.indexed_vars.contains(
            w,
        ) by {
            let j = choose|j: int| 0 <= j < m.indexed_vars.len() && m.indexed_vars[j] == w;
            assert(r.indexed_vars[j] == w);
        }
        assert forall|k: Key, x: Tok| !(#[trigger] list_at(m.var_indices, v, k).contains(x)) by {}
    }
}

} // verus!
