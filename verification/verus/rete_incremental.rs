//! Verus model: incremental Rete maintenance equals declarative matching.
//!
//! Models the positive join chain of `ReteNetwork`
//! (crates/ferric-rules-core/src/rete.rs: `assert_fact`, `right_activate`,
//! `propagate_join_fact`, `emit_join_match`, `left_activate`, `retract_fact`),
//! the `TokenStore` tree of tokens (token.rs), beta memories (beta.rs) and
//! alpha memories (alpha.rs). Each model function cites the production
//! function it follows.
//!
//! What is proved (unbounded: any chain length `n`, any number of facts and
//! tokens, any alpha predicates and join tests, any iteration orders, any
//! fresh-key supply, any finite operation sequence):
//!
//! * `theorem_incremental_equals_declarative`: starting from the state after
//!   `seed_root_token`, after any sequence of assertions and retractions,
//!   for every level `k <= n`
//!   - the fact tuples (`collect_all_facts`) of the tokens in beta memory `k`
//!     are exactly the declarative matches of the first `k` patterns over
//!     the current working memory: tuples `x` of present facts where each
//!     `x[j]` passes the alpha predicate of join `j+1` and its join test
//!     against the data of `x[..j]` (`beta_tuples == matches_k`);
//!   - no two tokens in one memory have the same tuple (no duplicates);
//!   - every token in memory `k >= 1` has its parent in memory `k-1`.
//! * `theorem_assert_delta`: an assertion keeps every existing token (same
//!   id, same contents, same tuple) and the new tokens are exactly the
//!   tokens whose tuple mentions the asserted fact.
//! * `theorem_retract_delta`: a retraction (`tokens_containing`, then
//!   `retraction_roots`, then `remove_cascade` per root) removes exactly the
//!   tokens whose tuple mentions the retracted fact and leaves all other
//!   tokens unchanged.
//! * The invariant `inv` (alpha memories exact; tokens form a level-indexed
//!   tree; each token locally sound; siblings have distinct facts; every
//!   (token, matching fact) pair has a child) holds after every operation
//!   (`lemma_step`), and every intermediate state reached inside
//!   left/right activation satisfies `inv_core` (`grows`). The shared-alpha
//!   case where a token created during an assertion must join the asserted
//!   fact itself, and the pre-propagation capture of right-activation
//!   parents that prevents a second join of the same pair, are covered.
//! * Non-vacuity: `lemma_sched_satisfiable` exhibits oracles meeting the
//!   scheduler hypothesis; `example_shared_alpha_self_join` derives from the
//!   main theorem that `(a ?x) (a ?y)` holds `[7, 7]` after asserting `7`.
//!
//! Abstractions and simplifications (fidelity gaps):
//!
//! * Facts are `nat` ids with data of an arbitrary type `V`; the `FactBase`
//!   is a map. Alpha networks are abstracted by a predicate per alpha memory
//!   (the constant-test discrimination network is not modelled). Join tests
//!   are an arbitrary predicate of (join, parent data, fact data); token
//!   bindings are abstracted as the sequence of the data of the facts on the
//!   token's chain (`Tok.data`), which determines the bindings. Sequence
//!   (multifield) joins are not modelled.
//! * Every iteration order (alpha memory facts, beta memory tokens, the
//!   order of right activations, which production sorts by node id at
//!   rete.rs:323) is chosen by an oracle `ord(clock, set)` that returns any
//!   duplicate-free enumeration; the clock advances on each use, so any
//!   production order is one instance. Token keys come from an oracle
//!   `fresh(clock, live keys)` returning a non-live key (`SlotMap::insert`).
//! * Index narrowing (`collect_candidate_facts` /
//!   `collect_candidate_parent_tokens`, rete.rs:2637/2665) is modelled by the
//!   full-scan fallback. The indexed path gives the same result only if an
//!   index lookup returns every entry that passes the indexed equality test;
//!   that index property is not proved here.
//! * `remove_cascade` is modelled by its effect (the root and all of its
//!   descendants); its stack loop and the `fact_to_tokens` /
//!   `parent_to_children` indexes are not modelled here. `retraction_roots`
//!   and `tokens_containing` are modelled by the sets they compute.
//! * Retraction is modelled by its final state (removal order inside
//!   `retract_fact` is irrelevant for a chain of joins).
//! * Not modelled: terminal nodes and the agenda (beta memory `n` is what the
//!   terminal sees), activations, predicate/negative/NCC/exists nodes,
//!   NCC batching, rule installation/removal, `clear_working_memory`.
//! * Ghost state: `St.root` and `St.clock`.
//!
//! Trusted base: Verus, Z3 and vstd. No `assume`, `admit`,
//! `external_body` or other trusted glue.
use vstd::prelude::*;

verus! {

broadcast use {
    vstd::set::group_set_lemmas,
    vstd::map::group_map_lemmas,
    vstd::seq::group_seq_axioms,
    vstd::set_lib::range_set_properties,
    vstd::seq_lib::lemma_seq_contains_after_push,
    vstd::seq_lib::lemma_seq_empty_contains_nothing,
    vstd::iset::group_iset_lemmas,
};

/// `FactId` (crates/ferric-rules-core/src/fact.rs:174, a SlotMap key).
pub type FactId = nat;

/// `TokenId` (crates/ferric-rules-core/src/token.rs:20, a SlotMap key).
pub type TokenId = nat;

/// Compiled network shape: the root (level 0) followed by joins `1..=n`,
/// each the only child of the previous one (`BetaNode::Root` / `BetaNode::Join`,
/// crates/ferric-rules-core/src/beta.rs:396-418). Join `k` reads alpha memory
/// `alpha_of(k)` (`BetaNode::Join.alpha_memory`); several joins may read the
/// same alpha memory (`BetaNetwork::alpha_to_joins` fan-out).
#[verifier::reject_recursive_types(V)]
pub struct Net<V> {
    pub n: nat,
    /// `AlphaNetwork.memories` ids.
    pub alphas: Set<nat>,
    pub alpha_of: spec_fn(nat) -> nat,
    /// Whether the constant-test path into alpha memory `a` accepts fact data
    /// (`AlphaNetwork::assert_fact`, crates/ferric-rules-core/src/alpha.rs:609).
    pub alpha: spec_fn(nat, V) -> bool,
    /// `evaluate_join(fact, Some(parent), tests)` of join `k`
    /// (crates/ferric-rules-core/src/rete.rs:2690), over the parent token's
    /// data (abstracting its bindings) and the fact's data.
    pub test: spec_fn(nat, Seq<V>, V) -> bool,
}

pub open spec fn net_wf<V>(net: Net<V>) -> bool {
    forall|k: nat| 1 <= k <= net.n ==> net.alphas.contains(#[trigger] (net.alpha_of)(k))
}

/// Scheduler oracles: every iteration order (`ord`) and every fresh token key
/// (`fresh`), indexed by a clock that advances on each use.
pub struct Sched {
    pub ord: spec_fn(nat, Set<nat>) -> Seq<nat>,
    pub fresh: spec_fn(nat, Set<nat>) -> nat,
}

/// `ord` enumerates its set exactly once (HashSet / OrderedSet iteration);
/// `fresh` never returns a live key (`SlotMap::insert`).
pub open spec fn sched_ok(sc: Sched) -> bool {
    &&& forall|c: nat, s: Set<nat>| #[trigger] (sc.ord)(c, s).no_duplicates()
    &&& forall|c: nat, s: Set<nat>, x: nat| #[trigger] (sc.ord)(c, s).contains(x) <==> s.contains(x)
    &&& forall|c: nat, s: Set<nat>| !s.contains(#[trigger] (sc.fresh)(c, s))
}

/// `Token` (crates/ferric-rules-core/src/token.rs:31); `data` abstracts
/// `bindings` (the data of every fact on the token's chain), `owner` is the
/// level of `owner_node`.
pub struct Tok<V> {
    pub fact: Option<FactId>,
    pub data: Seq<V>,
    pub parent: Option<TokenId>,
    pub owner: nat,
}

/// Runtime state: `FactBase` contents (`wm`), `AlphaMemory.facts` per alpha
/// memory (`amem`), `TokenStore.tokens` (`tokens`), `BetaMemory.tokens` per
/// level (`mem`). Ghost: `root` (the token made by `seed_root_token`,
/// crates/ferric-rules-core/src/rete.rs:710) and `clock` (scheduler time).
pub struct St<V> {
    pub wm: Map<FactId, V>,
    pub amem: Map<nat, Set<FactId>>,
    pub tokens: Map<TokenId, Tok<V>>,
    pub mem: Map<nat, Set<TokenId>>,
    pub root: TokenId,
    pub clock: nat,
}

// ---------------------------------------------------------------------------
// Production algorithm (positive join chain)
// ---------------------------------------------------------------------------
/// Models `ReteNetwork::propagate_join_fact` (crates/ferric-rules-core/src/rete.rs:1057),
/// non-sequence branch: skip a parent that is gone, reject when the join
/// tests fail, otherwise emit one token.
pub open spec fn join_fact<V>(
    net: Net<V>,
    sc: Sched,
    s: St<V>,
    k: nat,
    p: TokenId,
    g: FactId,
) -> St<V>
    decreases net.n - k, 3nat, 0nat,
{
    if !(1 <= k <= net.n) {
        s
    } else if !s.tokens.contains_key(p) {
        s
    } else if !(net.test)(k, s.tokens[p].data, s.wm[g]) {
        s
    } else {
        emit(net, sc, s, k, p, g)
    }
}

/// Models `ReteNetwork::emit_join_match` (crates/ferric-rules-core/src/rete.rs:1121):
/// build the child token (parent bindings extended by the fact), insert it in
/// the token store (`TokenStore::insert`, token.rs:80) and in the join's beta
/// memory (`BetaMemory::insert_indexed`, beta.rs:293), then
/// `propagate_token` (rete.rs:2359) to the only child: the next join is
/// left-activated (`propagate_to_child`, rete.rs:2440); after the last join
/// the child is the terminal node (activations are not modelled).
pub open spec fn emit<V>(net: Net<V>, sc: Sched, s: St<V>, k: nat, p: TokenId, g: FactId) -> St<V>
    decreases net.n - k, 2nat, 0nat,
{
    if !(1 <= k <= net.n) {
        s
    } else {
        let t = (sc.fresh)(s.clock, s.tokens.dom());
        let tok = Tok {
            fact: Some(g),
            data: s.tokens[p].data.push(s.wm[g]),
            parent: Some(p),
            owner: k,
        };
        let s1 = St {
            tokens: s.tokens.insert(t, tok),
            mem: s.mem.insert(k, s.mem[k].insert(t)),
            clock: s.clock + 1,
            ..s
        };
        if k < net.n {
            left_activate(net, sc, s1, k + 1, t)
        } else {
            s1
        }
    }
}

/// Models `ReteNetwork::left_activate` (crates/ferric-rules-core/src/rete.rs:1162):
/// a missing parent token returns early; otherwise the candidate facts of the
/// join's alpha memory (`collect_candidate_facts`, rete.rs:2637, full-scan
/// path) are enumerated in scheduler order and each is offered to
/// `propagate_join_fact`.
pub open spec fn left_activate<V>(net: Net<V>, sc: Sched, s: St<V>, k: nat, t: TokenId) -> St<V>
    decreases net.n - k, 5nat, 0nat,
{
    if !(1 <= k <= net.n) {
        s
    } else if !s.tokens.contains_key(t) {
        s
    } else {
        let fs = (sc.ord)(s.clock, s.amem[(net.alpha_of)(k)]);
        left_loop(net, sc, St { clock: s.clock + 1, ..s }, k, t, fs)
    }
}

/// The `for fact_id in fact_ids` loop of `left_activate`
/// (crates/ferric-rules-core/src/rete.rs:1188-1201), including the
/// `fact_base.get(fact_id) else continue` guard.
pub open spec fn left_loop<V>(
    net: Net<V>,
    sc: Sched,
    s: St<V>,
    k: nat,
    t: TokenId,
    fs: Seq<FactId>,
) -> St<V>
    decreases net.n - k, 4nat, fs.len(),
{
    if !(1 <= k <= net.n) || fs.len() == 0 {
        s
    } else {
        let s1 = if s.wm.contains_key(fs[0]) {
            join_fact(net, sc, s, k, t, fs[0])
        } else {
            s
        };
        left_loop(net, sc, s1, k, t, fs.drop_first())
    }
}

/// Models `ReteNetwork::right_activate` (crates/ferric-rules-core/src/rete.rs:1023):
/// the loop over the parent tokens captured before propagation.
pub open spec fn right_loop<V>(
    net: Net<V>,
    sc: Sched,
    s: St<V>,
    k: nat,
    ps: Seq<TokenId>,
    f: FactId,
) -> St<V>
    decreases ps.len(),
{
    if ps.len() == 0 {
        s
    } else {
        right_loop(net, sc, join_fact(net, sc, s, k, ps[0], f), k, ps.drop_first(), f)
    }
}

/// The `for activation in right_activations` loop of `ReteNetwork::assert_fact`
/// (crates/ferric-rules-core/src/rete.rs:324-352), `RightActivation::Join` arm.
pub open spec fn ra_loop<V>(
    net: Net<V>,
    sc: Sched,
    s: St<V>,
    ras: Seq<(nat, Seq<TokenId>)>,
    f: FactId,
) -> St<V>
    decreases ras.len(),
{
    if ras.len() == 0 {
        s
    } else {
        ra_loop(net, sc, right_loop(net, sc, s, ras[0].0, ras[0].1, f), ras.drop_first(), f)
    }
}

/// Models `AlphaNetwork::assert_fact` (crates/ferric-rules-core/src/alpha.rs:609)
/// with `AlphaMemory::insert` (alpha.rs:196): the fact enters every memory
/// whose path accepts it.
pub open spec fn alpha_assert<V>(net: Net<V>, s: St<V>, f: FactId) -> St<V> {
    St {
        amem: Map::new(
            s.amem.dom(),
            |a: nat|
                if (net.alpha)(a, s.wm[f]) {
                    s.amem[a].insert(f)
                } else {
                    s.amem[a]
                },
        ),
        ..s
    }
}

/// The join nodes subscribed (`BetaNetwork::join_nodes_for_alpha`,
/// crates/ferric-rules-core/src/beta.rs:1368) to an alpha memory that
/// accepted the fact (rete.rs:296-301).
pub open spec fn affected_joins<V>(net: Net<V>, v: V) -> Set<nat> {
    Set::<nat>::range(1, net.n + 1).filter(|k: nat| (net.alpha)((net.alpha_of)(k), v))
}

/// `RightActivation::Join(node, self.right_parent_candidates(node, fact))`
/// (crates/ferric-rules-core/src/rete.rs:298-301, 995): every join's parent
/// tokens are captured from its parent beta memory before any propagation.
pub open spec fn captured<V>(sc: Sched, s: St<V>, joins: Seq<nat>, c: nat) -> Seq<
    (nat, Seq<TokenId>),
> {
    Seq::new(
        joins.len(),
        |i: int| (joins[i], (sc.ord)((c + i) as nat, s.mem[(joins[i] - 1) as nat])),
    )
}

/// Models `ReteNetwork::assert_fact` (crates/ferric-rules-core/src/rete.rs:281)
/// for a chain of joins. `s.wm` already holds `f` (the engine inserts the fact
/// into the `FactBase` first: crates/ferric-rules-runtime/src/engine.rs:77).
/// The order of right activations (production: node id descending,
/// rete.rs:323) is a scheduler choice.
pub open spec fn assert_fact<V>(net: Net<V>, sc: Sched, s: St<V>, f: FactId) -> St<V> {
    let s1 = alpha_assert(net, s, f);
    let joins = (sc.ord)(s1.clock, affected_joins(net, s1.wm[f]));
    let c = s1.clock + 1;
    ra_loop(net, sc, St { clock: c + joins.len(), ..s1 }, captured(sc, s1, joins, c), f)
}

/// The ancestor `d` parent links above `t` (the walk of
/// `TokenStore::retraction_roots`, crates/ferric-rules-core/src/token.rs:243).
pub open spec fn anc<V>(toks: Map<TokenId, Tok<V>>, t: TokenId, d: nat) -> TokenId
    decreases d,
{
    if d == 0 {
        t
    } else {
        anc(toks, toks[t].parent->0, (d - 1) as nat)
    }
}

/// Models `TokenStore::tokens_containing` (crates/ferric-rules-core/src/token.rs:223):
/// the `fact_to_tokens` index lists the tokens whose own fact is `f`.
pub open spec fn tokens_containing<V>(toks: Map<TokenId, Tok<V>>, f: FactId) -> Set<TokenId> {
    toks.dom().filter(|t: TokenId| toks[t].fact == Some(f))
}

/// Models the per-token test of `TokenStore::retraction_roots`
/// (crates/ferric-rules-core/src/token.rs:243): no ancestor on the parent
/// chain is affected.
pub open spec fn is_retraction_root<V>(
    toks: Map<TokenId, Tok<V>>,
    affected: Set<TokenId>,
    t: TokenId,
) -> bool {
    &&& affected.contains(t)
    &&& forall|d: nat| 1 <= d <= toks[t].owner ==> !affected.contains(#[trigger] anc(toks, t, d))
}

/// Models `TokenStore::retraction_roots` (crates/ferric-rules-core/src/token.rs:243).
pub open spec fn retraction_roots<V>(toks: Map<TokenId, Tok<V>>, affected: Set<TokenId>) -> Set<
    TokenId,
> {
    affected.filter(|t: TokenId| is_retraction_root(toks, affected, t))
}

/// The tokens removed by `TokenStore::remove_cascade(root)` for every root
/// (crates/ferric-rules-core/src/token.rs:168; loop at rete.rs:383-386),
/// specified by its effect: each root and all of its descendants.
pub open spec fn cascade<V>(toks: Map<TokenId, Tok<V>>, roots: Set<TokenId>) -> Set<TokenId> {
    toks.dom().filter(
        |t: TokenId| exists|d: nat| d <= toks[t].owner && #[trigger] roots.contains(anc(toks, t, d)),
    )
}

pub open spec fn removed_by<V>(toks: Map<TokenId, Tok<V>>, f: FactId) -> Set<TokenId> {
    cascade(toks, retraction_roots(toks, tokens_containing(toks, f)))
}

/// Models `ReteNetwork::retract_fact` (crates/ferric-rules-core/src/rete.rs:361)
/// for a chain of joins, followed by the engine's `FactBase::retract`
/// (crates/ferric-rules-runtime/src/engine.rs:1108-1114): steps 1-3 (tokens
/// containing the fact, retraction roots, cascades), removal of each removed
/// token from its owner's beta memory (`remove_indexed`, rete.rs:438), and
/// `AlphaNetwork::retract_fact` (alpha.rs:634; removing `f` from a memory
/// that does not hold it is a no-op).
pub open spec fn retract_fact<V>(net: Net<V>, s: St<V>, f: FactId) -> St<V> {
    let rm = removed_by(s.tokens, f);
    St {
        tokens: s.tokens.remove_keys(rm),
        mem: Map::new(s.mem.dom(), |k: nat| s.mem[k].difference(rm)),
        amem: Map::new(s.amem.dom(), |a: nat| s.amem[a].remove(f)),
        wm: s.wm.remove(f),
        ..s
    }
}

/// Models `TokenStore::collect_all_facts` (crates/ferric-rules-core/src/token.rs:201)
/// for a token `k` levels deep: the facts on its parent chain, root first.
pub open spec fn facts<V>(toks: Map<TokenId, Tok<V>>, t: TokenId, k: nat) -> Seq<FactId>
    decreases k,
{
    if k == 0 {
        Seq::empty()
    } else {
        facts(toks, toks[t].parent->0, (k - 1) as nat).push(toks[t].fact->0)
    }
}

pub open spec fn tuple<V>(toks: Map<TokenId, Tok<V>>, t: TokenId) -> Seq<FactId> {
    facts(toks, t, toks[t].owner)
}

// ---------------------------------------------------------------------------
// Invariants
// ---------------------------------------------------------------------------
pub open spec fn jmatch<V>(net: Net<V>, wm: Map<FactId, V>, k: nat, d: Seq<V>, g: FactId) -> bool {
    &&& wm.contains_key(g)
    &&& (net.alpha)((net.alpha_of)(k), wm[g])
    &&& (net.test)(k, d, wm[g])
}

pub open spec fn is_child<V>(toks: Map<TokenId, Tok<V>>, c: TokenId, p: TokenId, g: FactId) -> bool {
    &&& toks.contains_key(c)
    &&& toks[c].parent == Some(p)
    &&& toks[c].fact == Some(g)
}

pub open spec fn has_child<V>(toks: Map<TokenId, Tok<V>>, p: TokenId, g: FactId) -> bool {
    exists|c: TokenId| #[trigger] is_child(toks, c, p, g)
}

pub open spec fn root_tok<V>() -> Tok<V> {
    Tok { fact: None, data: Seq::empty(), parent: None, owner: 0 }
}

pub open spec fn inv_struct<V>(net: Net<V>, s: St<V>) -> bool {
    &&& forall|k: nat| k <= net.n ==> #[trigger] s.mem.contains_key(k)
    &&& forall|k: nat, t: TokenId|
        k <= net.n && #[trigger] s.mem[k].contains(t) ==> s.tokens.contains_key(t)
            && s.tokens[t].owner == k
    &&& forall|t: TokenId| #[trigger]
        s.tokens.contains_key(t) ==> s.tokens[t].owner <= net.n && s.mem[s.tokens[t].owner].contains(
            t,
        )
    &&& s.tokens.contains_key(s.root)
    &&& s.tokens[s.root] == root_tok::<V>()
    &&& forall|t: TokenId| #[trigger]
        s.tokens.contains_key(t) && s.tokens[t].owner == 0 ==> t == s.root
    &&& forall|t: TokenId| #[trigger]
        s.tokens.contains_key(t) && s.tokens[t].owner > 0 ==> s.tokens[t].fact is Some
    // Triggered on `.parent` (not on liveness) so that the parent chain is not
    // followed by the solver without bound.
    &&& forall|t: TokenId|
        #![trigger s.tokens[t].parent]
        s.tokens.contains_key(t) && s.tokens[t].owner > 0 ==> {
            &&& s.tokens[t].parent is Some
            &&& s.tokens.contains_key(s.tokens[t].parent->0)
            &&& s.tokens[s.tokens[t].parent->0].owner + 1 == s.tokens[t].owner
        }
}

pub open spec fn inv_sound<V>(net: Net<V>, s: St<V>) -> bool {
    forall|t: TokenId|
        #![trigger s.tokens[t].parent]
        s.tokens.contains_key(t) && s.tokens[t].owner > 0 ==> {
            &&& jmatch(
                net,
                s.wm,
                s.tokens[t].owner,
                s.tokens[s.tokens[t].parent->0].data,
                s.tokens[t].fact->0,
            )
            &&& s.tokens[t].data == s.tokens[s.tokens[t].parent->0].data.push(
                s.wm[s.tokens[t].fact->0],
            )
        }
}

pub open spec fn inv_unique<V>(s: St<V>) -> bool {
    forall|c1: TokenId, c2: TokenId, p: TokenId, g: FactId|
        #[trigger] is_child(s.tokens, c1, p, g) && #[trigger] is_child(s.tokens, c2, p, g) ==> c1
            == c2
}

pub open spec fn inv_alpha<V>(net: Net<V>, s: St<V>) -> bool {
    &&& forall|a: nat| #[trigger] net.alphas.contains(a) ==> s.amem.contains_key(a)
    &&& forall|a: nat, g: FactId|
        s.amem.contains_key(a) ==> (#[trigger] s.amem[a].contains(g) <==> (s.wm.contains_key(g)
            && (net.alpha)(a, s.wm[g])))
}

pub open spec fn inv_core<V>(net: Net<V>, s: St<V>) -> bool {
    &&& inv_struct(net, s)
    &&& inv_sound(net, s)
    &&& inv_unique(s)
    &&& inv_alpha(net, s)
}

pub open spec fn complete_node<V>(
    net: Net<V>,
    wm: Map<FactId, V>,
    toks: Map<TokenId, Tok<V>>,
    u: TokenId,
) -> bool {
    toks[u].owner < net.n ==> forall|g: FactId|
        #![trigger has_child(toks, u, g)]
        jmatch(net, wm, toks[u].owner + 1, toks[u].data, g) ==> has_child(toks, u, g)
}

pub open spec fn inv_complete<V>(net: Net<V>, s: St<V>) -> bool {
    forall|u: TokenId| #[trigger] s.tokens.contains_key(u) ==> complete_node(net, s.wm, s.tokens, u)
}

pub open spec fn inv<V>(net: Net<V>, s: St<V>) -> bool {
    inv_core(net, s) && inv_complete(net, s)
}

pub open spec fn tok_ext<V>(a: Map<TokenId, Tok<V>>, b: Map<TokenId, Tok<V>>) -> bool {
    forall|t: TokenId| #[trigger] a.contains_key(t) ==> b.contains_key(t) && b[t] == a[t]
}

pub open spec fn extends<V>(s: St<V>, s2: St<V>) -> bool {
    &&& s2.wm == s.wm
    &&& s2.amem == s.amem
    &&& s2.root == s.root
    &&& tok_ext(s.tokens, s2.tokens)
}

pub open spec fn grows<V>(net: Net<V>, s: St<V>, s2: St<V>) -> bool {
    &&& inv_core(net, s2)
    &&& extends(s, s2)
    &&& forall|u: TokenId| #[trigger]
        s2.tokens.contains_key(u) && !s.tokens.contains_key(u) ==> complete_node(
            net,
            s2.wm,
            s2.tokens,
            u,
        )
}

pub open spec fn no_children<V>(s: St<V>, t: TokenId) -> bool {
    forall|c: TokenId|
        #![trigger s.tokens[c].parent]
        s.tokens.contains_key(c) ==> s.tokens[c].parent != Some(t)
}

// ---------------------------------------------------------------------------
// Monotonicity
// ---------------------------------------------------------------------------
pub proof fn lemma_has_child_mono<V>(s: St<V>, s2: St<V>)
    requires
        tok_ext(s.tokens, s2.tokens),
    ensures
        forall|p: TokenId, g: FactId| #[trigger] has_child(s.tokens, p, g) ==> has_child(s2.tokens, p, g),
{
    assert forall|p: TokenId, g: FactId| #[trigger] has_child(s.tokens, p, g) implies has_child(
        s2.tokens,
        p,
        g,
    ) by {
        let c = choose|c: TokenId| #[trigger] is_child(s.tokens, c, p, g);
        assert(is_child(s2.tokens, c, p, g));
    }
}

pub proof fn lemma_complete_mono<V>(net: Net<V>, s: St<V>, s2: St<V>, u: TokenId)
    requires
        extends(s, s2),
        s.tokens.contains_key(u),
        complete_node(net, s.wm, s.tokens, u),
    ensures
        complete_node(net, s2.wm, s2.tokens, u),
{
    lemma_has_child_mono(s, s2);
    if s2.tokens[u].owner < net.n {
        assert forall|g: FactId| jmatch(net, s2.wm, s2.tokens[u].owner + 1, s2.tokens[u].data, g) implies has_child(
            s2.tokens,
            u,
            g,
        ) by {
            assert(jmatch(net, s.wm, s.tokens[u].owner + 1, s.tokens[u].data, g));
            assert(has_child(s.tokens, u, g));
        }
    }
}

pub proof fn lemma_grows_trans<V>(net: Net<V>, s: St<V>, s1: St<V>, s2: St<V>)
    requires
        grows(net, s, s1),
        grows(net, s1, s2),
    ensures
        grows(net, s, s2),
{
    assert forall|u: TokenId| #[trigger]
        s2.tokens.contains_key(u) && !s.tokens.contains_key(u) implies complete_node(
        net,
        s2.wm,
        s2.tokens,
        u,
    ) by {
        if s1.tokens.contains_key(u) {
            lemma_complete_mono(net, s1, s2, u);
        }
    }
}

pub proof fn lemma_grows_refl<V>(net: Net<V>, s: St<V>)
    requires
        inv_core(net, s),
    ensures
        grows(net, s, s),
{
}

// ---------------------------------------------------------------------------
// Left activation (mutually recursive with join_fact / emit)
// ---------------------------------------------------------------------------
pub proof fn lemma_join_fact<V>(
    net: Net<V>,
    sc: Sched,
    s: St<V>,
    k: nat,
    p: TokenId,
    g: FactId,
)
    requires
        net_wf(net),
        sched_ok(sc),
        inv_core(net, s),
        1 <= k <= net.n,
        s.tokens.contains_key(p),
        s.tokens[p].owner + 1 == k,
        s.amem[(net.alpha_of)(k)].contains(g),
        !has_child(s.tokens, p, g),
    ensures
        grows(net, s, join_fact(net, sc, s, k, p, g)),
        jmatch(net, s.wm, k, s.tokens[p].data, g) ==> has_child(join_fact(net, sc, s, k, p, g).tokens, p, g),
        forall|q: TokenId, h: FactId|
            s.tokens.contains_key(q) && #[trigger] has_child(join_fact(net, sc, s, k, p, g).tokens, q, h)
                ==> has_child(s.tokens, q, h) || (q == p && h == g),
    decreases net.n - k, 3nat, 0nat,
{
    if !(net.test)(k, s.tokens[p].data, s.wm[g]) {
        lemma_grows_refl(net, s);
    } else {
        lemma_emit(net, sc, s, k, p, g);
    }
}

pub proof fn lemma_emit<V>(net: Net<V>, sc: Sched, s: St<V>, k: nat, p: TokenId, g: FactId)
    requires
        net_wf(net),
        sched_ok(sc),
        inv_core(net, s),
        1 <= k <= net.n,
        s.tokens.contains_key(p),
        s.tokens[p].owner + 1 == k,
        s.amem[(net.alpha_of)(k)].contains(g),
        !has_child(s.tokens, p, g),
        (net.test)(k, s.tokens[p].data, s.wm[g]),
    ensures
        grows(net, s, emit(net, sc, s, k, p, g)),
        has_child(emit(net, sc, s, k, p, g).tokens, p, g),
        forall|q: TokenId, h: FactId|
            s.tokens.contains_key(q) && #[trigger] has_child(emit(net, sc, s, k, p, g).tokens, q, h)
                ==> has_child(s.tokens, q, h) || (q == p && h == g),
    decreases net.n - k, 2nat, 0nat,
{
    let t = (sc.fresh)(s.clock, s.tokens.dom());
    let tok = Tok { fact: Some(g), data: s.tokens[p].data.push(s.wm[g]), parent: Some(p), owner: k };
    let s1 = St {
        tokens: s.tokens.insert(t, tok),
        mem: s.mem.insert(k, s.mem[k].insert(t)),
        clock: s.clock + 1,
        ..s
    };
    assert(!s.tokens.contains_key(t));
    assert(net.alphas.contains((net.alpha_of)(k)));
    // s1 satisfies the core invariant
    assert(inv_struct(net, s1)) by {
        assert forall|k2: nat, t2: TokenId|
            k2 <= net.n && #[trigger] s1.mem[k2].contains(t2) implies s1.tokens.contains_key(t2)
            && s1.tokens[t2].owner == k2 by {
            if k2 != k {
                assert(s.mem[k2].contains(t2));
            } else if t2 != t {
                assert(s.mem[k].contains(t2));
            }
        }
    }
    assert(inv_sound(net, s1));
    assert(inv_unique(s1)) by {
        assert forall|c1: TokenId, c2: TokenId, q: TokenId, h: FactId|
            #[trigger] is_child(s1.tokens, c1, q, h) && #[trigger] is_child(s1.tokens, c2, q, h) implies c1
            == c2 by {
            if c1 == t && c2 != t {
                assert(is_child(s.tokens, c2, p, g));
            } else if c2 == t && c1 != t {
                assert(is_child(s.tokens, c1, p, g));
            } else if c1 != t && c2 != t {
                assert(is_child(s.tokens, c1, q, h));
                assert(is_child(s.tokens, c2, q, h));
            }
        }
    }
    assert(inv_alpha(net, s1));
    assert(extends(s, s1));
    assert(is_child(s1.tokens, t, p, g));
    assert(has_child(s1.tokens, p, g));
    assert forall|q: TokenId, h: FactId|
        s.tokens.contains_key(q) && #[trigger] has_child(s1.tokens, q, h) implies has_child(s.tokens, q, h) || (q
        == p && h == g) by {
        let c = choose|c: TokenId| #[trigger] is_child(s1.tokens, c, q, h);
        if c != t {
            assert(is_child(s.tokens, c, q, h));
        }
    }
    if k < net.n {
        assert(no_children(s1, t));
        lemma_left_activate(net, sc, s1, k + 1, t);
        let s3 = left_activate(net, sc, s1, k + 1, t);
        assert(s3 == emit(net, sc, s, k, p, g));
        lemma_has_child_mono(s1, s3);
        assert forall|u: TokenId| #[trigger]
            s3.tokens.contains_key(u) && !s.tokens.contains_key(u) implies complete_node(
            net,
            s3.wm,
            s3.tokens,
            u,
        ) by {
            if s1.tokens.contains_key(u) {
                assert(u == t);
            }
        }
        assert forall|q: TokenId, h: FactId|
            s.tokens.contains_key(q) && #[trigger] has_child(s3.tokens, q, h) implies has_child(s.tokens, q, h)
            || (q == p && h == g) by {
            assert(s1.tokens.contains_key(q));
            assert(has_child(s1.tokens, q, h) || q == t);
        }
    } else {
        assert(s1 == emit(net, sc, s, k, p, g));
        assert forall|u: TokenId| #[trigger]
            s1.tokens.contains_key(u) && !s.tokens.contains_key(u) implies complete_node(
            net,
            s1.wm,
            s1.tokens,
            u,
        ) by {
            assert(u == t);
        }
    }
}

pub proof fn lemma_left_activate<V>(net: Net<V>, sc: Sched, s: St<V>, k: nat, t: TokenId)
    requires
        net_wf(net),
        sched_ok(sc),
        inv_core(net, s),
        1 <= k <= net.n,
        s.tokens.contains_key(t),
        s.tokens[t].owner + 1 == k,
        no_children(s, t),
    ensures
        grows(net, s, left_activate(net, sc, s, k, t)),
        complete_node(net, left_activate(net, sc, s, k, t).wm, left_activate(net, sc, s, k, t).tokens, t),
        forall|q: TokenId, h: FactId|
            s.tokens.contains_key(q) && #[trigger] has_child(left_activate(net, sc, s, k, t).tokens, q, h)
                ==> has_child(s.tokens, q, h) || q == t,
    decreases net.n - k, 5nat, 0nat,
{
    let a = (net.alpha_of)(k);
    let fs = (sc.ord)(s.clock, s.amem[a]);
    let s0 = St { clock: s.clock + 1, ..s };
    assert(s0.tokens == s.tokens && s0.mem == s.mem && s0.wm == s.wm && s0.amem == s.amem
        && s0.root == s.root);
    assert(inv_core(net, s0));
    assert(net.alphas.contains(a));
    assert forall|g: FactId| #[trigger] fs.contains(g) implies !has_child(s0.tokens, t, g) by {
        if has_child(s0.tokens, t, g) {
            let c = choose|c: TokenId| #[trigger] is_child(s0.tokens, c, t, g);
            assert(s.tokens.contains_key(c));
        }
    }
    lemma_left_loop(net, sc, s0, k, t, fs);
    let s2 = left_loop(net, sc, s0, k, t, fs);
    assert(s2 == left_activate(net, sc, s, k, t));
    assert forall|g: FactId|
        #![trigger has_child(s2.tokens, t, g)]
        jmatch(net, s2.wm, s2.tokens[t].owner + 1, s2.tokens[t].data, g) implies has_child(s2.tokens, t, g) by {
        assert(s.amem[a].contains(g));
        assert(fs.contains(g));
    }
}

pub proof fn lemma_left_loop<V>(
    net: Net<V>,
    sc: Sched,
    s: St<V>,
    k: nat,
    t: TokenId,
    fs: Seq<FactId>,
)
    requires
        net_wf(net),
        sched_ok(sc),
        inv_core(net, s),
        1 <= k <= net.n,
        s.tokens.contains_key(t),
        s.tokens[t].owner + 1 == k,
        fs.no_duplicates(),
        forall|g: FactId| #[trigger] fs.contains(g) ==> s.amem[(net.alpha_of)(k)].contains(g),
        forall|g: FactId| #[trigger] fs.contains(g) ==> !has_child(s.tokens, t, g),
    ensures
        grows(net, s, left_loop(net, sc, s, k, t, fs)),
        forall|g: FactId|
            #![trigger fs.contains(g)]
            fs.contains(g) && jmatch(net, s.wm, k, s.tokens[t].data, g) ==> has_child(
                left_loop(net, sc, s, k, t, fs).tokens,
                t,
                g,
            ),
        forall|q: TokenId, h: FactId|
            s.tokens.contains_key(q) && #[trigger] has_child(left_loop(net, sc, s, k, t, fs).tokens, q, h)
                ==> has_child(s.tokens, q, h) || (q == t && fs.contains(h)),
    decreases net.n - k, 4nat, fs.len(),
{
    if fs.len() == 0 {
        lemma_grows_refl(net, s);
    } else {
        let g0 = fs[0];
        let rest = fs.drop_first();
        assert(fs.contains(g0));
        let s1 = if s.wm.contains_key(g0) {
            join_fact(net, sc, s, k, t, g0)
        } else {
            s
        };
        assert(s.amem[(net.alpha_of)(k)].contains(g0));
        assert(s.wm.contains_key(g0));
        lemma_join_fact(net, sc, s, k, t, g0);
        assert forall|g: FactId| #[trigger] rest.contains(g) implies s1.amem[(
        net.alpha_of)(k)].contains(g) && !has_child(s1.tokens, t, g) by {
            let i = choose|i: int| 0 <= i < rest.len() && rest[i] == g;
            assert(fs[i + 1] == g);
            assert(fs.contains(g));
            assert(g != g0);
        }
        assert(rest.no_duplicates()) by {
            assert forall|i: int, j: int|
                0 <= i < rest.len() && 0 <= j < rest.len() && i != j implies rest[i] != rest[j] by {
                assert(fs[i + 1] != fs[j + 1]);
            }
        }
        lemma_left_loop(net, sc, s1, k, t, rest);
        let s2 = left_loop(net, sc, s1, k, t, rest);
        assert(s2 == left_loop(net, sc, s, k, t, fs));
        lemma_grows_trans(net, s, s1, s2);
        lemma_has_child_mono(s1, s2);
        assert forall|g: FactId|
            #![trigger fs.contains(g)]
            fs.contains(g) && jmatch(net, s.wm, k, s.tokens[t].data, g) implies has_child(s2.tokens, t, g) by {
            if g != g0 {
                let i = choose|i: int| 0 <= i < fs.len() && fs[i] == g;
                assert(rest[i - 1] == g);
                assert(rest.contains(g));
                assert(jmatch(net, s1.wm, k, s1.tokens[t].data, g));
            }
        }
        assert forall|q: TokenId, h: FactId|
            s.tokens.contains_key(q) && #[trigger] has_child(s2.tokens, q, h) implies has_child(s.tokens, q, h)
            || (q == t && fs.contains(h)) by {
            assert(s1.tokens.contains_key(q));
            if has_child(s1.tokens, q, h) {
            } else {
                assert(q == t && rest.contains(h));
                let i = choose|i: int| 0 <= i < rest.len() && rest[i] == h;
                assert(fs[i + 1] == h);
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Right activation and assertion
// ---------------------------------------------------------------------------
pub proof fn lemma_right_loop<V>(
    net: Net<V>,
    sc: Sched,
    s: St<V>,
    k: nat,
    ps: Seq<TokenId>,
    f: FactId,
)
    requires
        net_wf(net),
        sched_ok(sc),
        inv_core(net, s),
        1 <= k <= net.n,
        ps.no_duplicates(),
        s.amem[(net.alpha_of)(k)].contains(f),
        forall|p: TokenId| #[trigger]
            ps.contains(p) ==> s.tokens.contains_key(p) && s.tokens[p].owner + 1 == k
                && !has_child(s.tokens, p, f),
    ensures
        grows(net, s, right_loop(net, sc, s, k, ps, f)),
        forall|p: TokenId|
            #![trigger ps.contains(p)]
            ps.contains(p) && jmatch(net, s.wm, k, s.tokens[p].data, f) ==> has_child(
                right_loop(net, sc, s, k, ps, f).tokens,
                p,
                f,
            ),
        forall|q: TokenId, h: FactId|
            s.tokens.contains_key(q) && #[trigger] has_child(
                right_loop(net, sc, s, k, ps, f).tokens,
                q,
                h,
            ) ==> has_child(s.tokens, q, h) || (h == f && ps.contains(q)),
    decreases ps.len(),
{
    if ps.len() == 0 {
        lemma_grows_refl(net, s);
    } else {
        let p0 = ps[0];
        let rest = ps.drop_first();
        assert(ps.contains(p0));
        lemma_join_fact(net, sc, s, k, p0, f);
        let s1 = join_fact(net, sc, s, k, p0, f);
        assert forall|p: TokenId| #[trigger] rest.contains(p) implies s1.tokens.contains_key(p)
            && s1.tokens[p].owner + 1 == k && !has_child(s1.tokens, p, f) by {
            let i = choose|i: int| 0 <= i < rest.len() && rest[i] == p;
            assert(ps[i + 1] == p);
            assert(ps.contains(p));
            assert(p != p0);
        }
        assert(rest.no_duplicates()) by {
            assert forall|i: int, j: int|
                0 <= i < rest.len() && 0 <= j < rest.len() && i != j implies rest[i] != rest[j] by {
                assert(ps[i + 1] != ps[j + 1]);
            }
        }
        lemma_right_loop(net, sc, s1, k, rest, f);
        let s2 = right_loop(net, sc, s1, k, rest, f);
        assert(s2 == right_loop(net, sc, s, k, ps, f));
        lemma_grows_trans(net, s, s1, s2);
        lemma_has_child_mono(s1, s2);
        assert forall|p: TokenId|
            #![trigger ps.contains(p)]
            ps.contains(p) && jmatch(net, s.wm, k, s.tokens[p].data, f) implies has_child(
            s2.tokens,
            p,
            f,
        ) by {
            if p != p0 {
                let i = choose|i: int| 0 <= i < ps.len() && ps[i] == p;
                assert(rest[i - 1] == p);
                assert(rest.contains(p));
            }
        }
        assert forall|q: TokenId, h: FactId|
            s.tokens.contains_key(q) && #[trigger] has_child(s2.tokens, q, h) implies has_child(
            s.tokens,
            q,
            h,
        ) || (h == f && ps.contains(q)) by {
            assert(s1.tokens.contains_key(q));
            if has_child(s1.tokens, q, h) {
            } else {
                assert(h == f && rest.contains(q));
                let i = choose|i: int| 0 <= i < rest.len() && rest[i] == q;
                assert(ps[i + 1] == q);
            }
        }
    }
}

pub open spec fn ra_ok<V>(net: Net<V>, s: St<V>, ras: Seq<(nat, Seq<TokenId>)>, f: FactId) -> bool {
    &&& forall|i: int|
        0 <= i < ras.len() ==> 1 <= #[trigger] ras[i].0 <= net.n && ras[i].1.no_duplicates()
            && s.amem[(net.alpha_of)(ras[i].0)].contains(f)
    &&& forall|i: int, p: TokenId|
        0 <= i < ras.len() && #[trigger] ras[i].1.contains(p) ==> s.tokens.contains_key(p)
            && s.tokens[p].owner + 1 == ras[i].0 && !has_child(s.tokens, p, f)
    &&& forall|i: int, j: int| 0 <= i < j < ras.len() ==> #[trigger] ras[i].0 != #[trigger] ras[j].0
}

pub proof fn lemma_ra_loop<V>(
    net: Net<V>,
    sc: Sched,
    s: St<V>,
    ras: Seq<(nat, Seq<TokenId>)>,
    f: FactId,
)
    requires
        net_wf(net),
        sched_ok(sc),
        inv_core(net, s),
        ra_ok(net, s, ras, f),
    ensures
        grows(net, s, ra_loop(net, sc, s, ras, f)),
        forall|i: int, p: TokenId|
            0 <= i < ras.len() && #[trigger] ras[i].1.contains(p) && jmatch(
                net,
                s.wm,
                ras[i].0,
                s.tokens[p].data,
                f,
            ) ==> has_child(ra_loop(net, sc, s, ras, f).tokens, p, f),
    decreases ras.len(),
{
    if ras.len() == 0 {
        lemma_grows_refl(net, s);
    } else {
        let k0 = ras[0].0;
        let ps0 = ras[0].1;
        let rest = ras.drop_first();
        assert(1 <= ras[0].0 <= net.n);
        assert forall|p: TokenId| #[trigger] ps0.contains(p) implies s.tokens.contains_key(p)
            && s.tokens[p].owner + 1 == k0 && !has_child(s.tokens, p, f) by {
            assert(ras[0].1.contains(p));
        }
        lemma_right_loop(net, sc, s, k0, ps0, f);
        let s1 = right_loop(net, sc, s, k0, ps0, f);
        assert(ra_ok(net, s1, rest, f)) by {
            assert forall|i: int| 0 <= i < rest.len() implies 1 <= #[trigger] rest[i].0 <= net.n
                && rest[i].1.no_duplicates() && s1.amem[(net.alpha_of)(rest[i].0)].contains(f) by {
                assert(rest[i] == ras[i + 1]);
            }
            assert forall|i: int, p: TokenId|
                0 <= i < rest.len() && #[trigger] rest[i].1.contains(p) implies s1.tokens.contains_key(
                p,
            ) && s1.tokens[p].owner + 1 == rest[i].0 && !has_child(s1.tokens, p, f) by {
                assert(rest[i] == ras[i + 1]);
                assert(ras[i + 1].1.contains(p));
                assert(ras[0].0 != ras[i + 1].0);
                if ps0.contains(p) {
                    assert(ras[0].1.contains(p));
                }
            }
            assert forall|i: int, j: int| 0 <= i < j < rest.len() implies #[trigger] rest[i].0
                != #[trigger] rest[j].0 by {
                assert(rest[i] == ras[i + 1]);
                assert(rest[j] == ras[j + 1]);
            }
        }
        lemma_ra_loop(net, sc, s1, rest, f);
        let s2 = ra_loop(net, sc, s1, rest, f);
        assert(s2 == ra_loop(net, sc, s, ras, f));
        lemma_grows_trans(net, s, s1, s2);
        lemma_has_child_mono(s1, s2);
        assert forall|i: int, p: TokenId|
            0 <= i < ras.len() && #[trigger] ras[i].1.contains(p) && jmatch(
                net,
                s.wm,
                ras[i].0,
                s.tokens[p].data,
                f,
            ) implies has_child(s2.tokens, p, f) by {
            if i == 0 {
                assert(ps0.contains(p));
            } else {
                assert(rest[i - 1] == ras[i]);
                assert(rest[i - 1].1.contains(p));
            }
        }
    }
}

pub proof fn lemma_assert_fact<V>(net: Net<V>, sc: Sched, s: St<V>, f: FactId, v: V)
    requires
        net_wf(net),
        sched_ok(sc),
        inv(net, s),
        !s.wm.contains_key(f),
    ensures
        ({
            let s2 = assert_fact(net, sc, St { wm: s.wm.insert(f, v), ..s }, f);
            &&& inv(net, s2)
            &&& s2.wm == s.wm.insert(f, v)
            &&& s2.root == s.root
            &&& forall|t: TokenId| #[trigger]
                s.tokens.contains_key(t) ==> s2.tokens.contains_key(t) && s2.tokens[t]
                    == s.tokens[t]
        }),
{
    let sa = St { wm: s.wm.insert(f, v), ..s };
    let s1 = alpha_assert(net, sa, f);
    assert(s1.wm[f] == v);
    // Old tokens never carry the new fact.
    assert forall|t: TokenId| #[trigger]
        s.tokens.contains_key(t) && s.tokens[t].owner > 0 implies s.tokens[t].fact->0 != f by {
        assert(s.tokens.contains_key(s.tokens[t].parent->0));
        assert(s.wm.contains_key(s.tokens[t].fact->0));
    }
    assert(inv_core(net, s1)) by {
        assert(inv_struct(net, s1));
        assert forall|t: TokenId|
            #![trigger s1.tokens[t].parent]
            s1.tokens.contains_key(t) && s1.tokens[t].owner > 0 implies {
            &&& jmatch(
                net,
                s1.wm,
                s1.tokens[t].owner,
                s1.tokens[s1.tokens[t].parent->0].data,
                s1.tokens[t].fact->0,
            )
            &&& s1.tokens[t].data == s1.tokens[s1.tokens[t].parent->0].data.push(
                s1.wm[s1.tokens[t].fact->0],
            )
        } by {
            assert(s.tokens[t].fact->0 != f);
        }
        assert(inv_sound(net, s1));
        assert(inv_unique(s1));
        assert forall|a: nat, g: FactId| s1.amem.contains_key(a) implies (
        #[trigger] s1.amem[a].contains(g) <==> (s1.wm.contains_key(g) && (net.alpha)(
            a,
            s1.wm[g],
        ))) by {
            if g != f {
                assert(s.amem.contains_key(a));
                assert(s.amem[a].contains(g) <==> s1.amem[a].contains(g));
            }
        }
        assert(inv_alpha(net, s1));
    }
    let joins = (sc.ord)(s1.clock, affected_joins(net, v));
    let c = s1.clock + 1;
    let ras = captured(sc, s1, joins, c);
    let s1c = St { clock: c + joins.len(), ..s1 };
    assert(inv_core(net, s1c));
    assert(ra_ok(net, s1c, ras, f)) by {
        assert forall|i: int| 0 <= i < ras.len() implies 1 <= #[trigger] ras[i].0 <= net.n
            && ras[i].1.no_duplicates() && s1c.amem[(net.alpha_of)(ras[i].0)].contains(f) by {
            let k = joins[i];
            assert(joins.contains(k));
            assert(affected_joins(net, v).contains(k));
            assert(net.alphas.contains((net.alpha_of)(k)));
        }
        assert forall|i: int, p: TokenId|
            0 <= i < ras.len() && #[trigger] ras[i].1.contains(p) implies s1c.tokens.contains_key(p)
            && s1c.tokens[p].owner + 1 == ras[i].0 && !has_child(s1c.tokens, p, f) by {
            let k = joins[i];
            assert(joins.contains(k));
            assert(affected_joins(net, v).contains(k));
            assert(s1.mem[(k - 1) as nat].contains(p));
            if has_child(s1c.tokens, p, f) {
                let ch = choose|ch: TokenId| #[trigger] is_child(s1c.tokens, ch, p, f);
                assert(s.tokens[ch].parent == Some(p));
                assert(s.tokens[ch].owner > 0);
            }
        }
        assert forall|i: int, j: int| 0 <= i < j < ras.len() implies #[trigger] ras[i].0
            != #[trigger] ras[j].0 by {
            assert(joins.no_duplicates());
            assert(ras[i].0 == joins[i]);
            assert(ras[j].0 == joins[j]);
            assert(joins[i] != joins[j]);
        }
    }
    lemma_ra_loop(net, sc, s1c, ras, f);
    let s2 = ra_loop(net, sc, s1c, ras, f);
    assert(s2 == assert_fact(net, sc, sa, f));
    lemma_has_child_mono(s, s2);
    assert forall|u: TokenId| #[trigger] s2.tokens.contains_key(u) implies complete_node(
        net,
        s2.wm,
        s2.tokens,
        u,
    ) by {
        if s.tokens.contains_key(u) && s2.tokens[u].owner < net.n {
            let j = s.tokens[u].owner;
            assert forall|g: FactId|
                #![trigger has_child(s2.tokens, u, g)]
                jmatch(net, s2.wm, j + 1, s2.tokens[u].data, g) implies has_child(
                s2.tokens,
                u,
                g,
            ) by {
                if g != f {
                    assert(complete_node(net, s.wm, s.tokens, u));
                    assert(jmatch(net, s.wm, j + 1, s.tokens[u].data, g));
                    assert(has_child(s.tokens, u, g));
                } else {
                    assert(affected_joins(net, v).contains(j + 1));
                    assert(joins.contains(j + 1));
                    let i = choose|i: int| 0 <= i < joins.len() && joins[i] == j + 1;
                    assert(ras[i].0 == j + 1);
                    assert(s1.mem[j].contains(u));
                    assert(ras[i].1.contains(u));
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Token tuples, ancestors, retraction
// ---------------------------------------------------------------------------
pub proof fn lemma_parent<V>(net: Net<V>, s: St<V>, t: TokenId)
    requires
        inv_struct(net, s),
        s.tokens.contains_key(t),
        s.tokens[t].owner > 0,
    ensures
        s.tokens[t].parent is Some,
        s.tokens[t].fact is Some,
        s.tokens.contains_key(s.tokens[t].parent->0),
        s.tokens[s.tokens[t].parent->0].owner + 1 == s.tokens[t].owner,
        tuple(s.tokens, t) == tuple(s.tokens, s.tokens[t].parent->0).push(s.tokens[t].fact->0),
{
    assert(s.tokens[t].parent is Some);
}

pub proof fn lemma_facts_len<V>(toks: Map<TokenId, Tok<V>>, t: TokenId, k: nat)
    ensures
        facts(toks, t, k).len() == k,
    decreases k,
{
    if k > 0 {
        lemma_facts_len(toks, toks[t].parent->0, (k - 1) as nat);
    }
}

pub proof fn lemma_anc<V>(net: Net<V>, s: St<V>, t: TokenId, d: nat)
    requires
        inv_struct(net, s),
        s.tokens.contains_key(t),
        d <= s.tokens[t].owner,
    ensures
        s.tokens.contains_key(anc(s.tokens, t, d)),
        s.tokens[anc(s.tokens, t, d)].owner + d == s.tokens[t].owner,
    decreases d,
{
    if d > 0 {
        lemma_parent(net, s, t);
        lemma_anc(net, s, s.tokens[t].parent->0, (d - 1) as nat);
    }
}

pub proof fn lemma_tuple_index<V>(net: Net<V>, s: St<V>, t: TokenId, j: int)
    requires
        inv_struct(net, s),
        s.tokens.contains_key(t),
        0 <= j < s.tokens[t].owner,
    ensures
        tuple(s.tokens, t)[j] == s.tokens[anc(
            s.tokens,
            t,
            (s.tokens[t].owner - 1 - j) as nat,
        )].fact->0,
    decreases s.tokens[t].owner,
{
    lemma_parent(net, s, t);
    let p = s.tokens[t].parent->0;
    lemma_facts_len(s.tokens, p, s.tokens[p].owner);
    if j < s.tokens[p].owner {
        lemma_tuple_index(net, s, p, j);
        let d = (s.tokens[t].owner - 1 - j) as nat;
        assert(anc(s.tokens, t, d) == anc(s.tokens, p, (d - 1) as nat));
    }
}

pub proof fn lemma_removed_iff<V>(net: Net<V>, s: St<V>, f: FactId, t: TokenId)
    requires
        inv_struct(net, s),
        s.tokens.contains_key(t),
    ensures
        removed_by(s.tokens, f).contains(t) <==> tuple(s.tokens, t).contains(f),
    decreases s.tokens[t].owner,
{
    let toks = s.tokens;
    let aff = tokens_containing(toks, f);
    let roots = retraction_roots(toks, aff);
    lemma_facts_len(toks, t, toks[t].owner);
    if removed_by(toks, f).contains(t) {
        let d = choose|d: nat| d <= toks[t].owner && #[trigger] roots.contains(anc(toks, t, d));
        let r = anc(toks, t, d);
        lemma_anc(net, s, t, d);
        assert(aff.contains(r));
        if toks[r].owner == 0 {
            assert(r == s.root);
        }
        let j = toks[t].owner - 1 - d;
        lemma_tuple_index(net, s, t, j);
        assert(tuple(toks, t)[j] == f);
    }
    if tuple(toks, t).contains(f) {
        if toks[t].owner == 0 {
            assert(tuple(toks, t) =~= Seq::<FactId>::empty());
        }
        lemma_parent(net, s, t);
        let p = toks[t].parent->0;
        if tuple(toks, p).contains(f) {
            lemma_removed_iff(net, s, f, p);
            let d = choose|d: nat| d <= toks[p].owner && #[trigger] roots.contains(anc(toks, p, d));
            assert(anc(toks, t, d + 1) == anc(toks, p, d));
            assert(roots.contains(anc(toks, t, d + 1)));
        } else {
            assert(toks[t].fact == Some(f));
            assert(aff.contains(t));
            assert forall|d: nat| 1 <= d <= toks[t].owner implies !aff.contains(
                #[trigger] anc(toks, t, d),
            ) by {
                assert(anc(toks, t, d) == anc(toks, p, (d - 1) as nat));
                lemma_anc(net, s, p, (d - 1) as nat);
                let a = anc(toks, p, (d - 1) as nat);
                if d == toks[t].owner {
                    assert(a == s.root);
                } else {
                    let j = toks[p].owner - d;
                    lemma_tuple_index(net, s, p, j);
                    lemma_facts_len(toks, p, toks[p].owner);
                    assert(tuple(toks, p)[j] == toks[a].fact->0);
                    if toks[a].fact == Some(f) {
                        assert(tuple(toks, p).contains(f));
                    }
                }
            }
            assert(is_retraction_root(toks, aff, t));
            assert(roots.contains(t));
            assert(anc(toks, t, 0) == t);
            assert(roots.contains(anc(toks, t, 0)));
        }
    }
}

pub proof fn lemma_retract_fact<V>(net: Net<V>, s: St<V>, f: FactId)
    requires
        net_wf(net),
        inv(net, s),
        s.wm.contains_key(f),
    ensures
        ({
            let s2 = retract_fact(net, s, f);
            &&& inv(net, s2)
            &&& s2.wm == s.wm.remove(f)
            &&& s2.root == s.root
            &&& forall|t: TokenId| #[trigger]
                s2.tokens.contains_key(t) <==> (s.tokens.contains_key(t) && !tuple(
                    s.tokens,
                    t,
                ).contains(f))
            &&& forall|t: TokenId| #[trigger]
                s2.tokens.contains_key(t) ==> s2.tokens[t] == s.tokens[t]
        }),
{
    let rm = removed_by(s.tokens, f);
    let s2 = retract_fact(net, s, f);
    assert forall|t: TokenId| #[trigger] s.tokens.contains_key(t) implies (rm.contains(t)
        <==> tuple(s.tokens, t).contains(f)) by {
        lemma_removed_iff(net, s, f, t);
    }
    assert forall|t: TokenId| #[trigger]
        s2.tokens.contains_key(t) <==> (s.tokens.contains_key(t) && !tuple(s.tokens, t).contains(
            f,
        )) by {
        if s.tokens.contains_key(t) {
            assert(rm.contains(t) <==> tuple(s.tokens, t).contains(f));
        }
    }
    // A surviving token's parent survives and its own fact is not `f`.
    assert forall|t: TokenId| #[trigger]
        s2.tokens.contains_key(t) && s2.tokens[t].owner > 0 implies s2.tokens.contains_key(
        s.tokens[t].parent->0,
    ) && s.tokens[t].fact->0 != f by {
        lemma_parent(net, s, t);
        let p = s.tokens[t].parent->0;
        assert(s.tokens.contains_key(p));
    }
    assert(inv_struct(net, s2)) by {
        assert(tuple(s.tokens, s.root) =~= Seq::<FactId>::empty());
        assert(s2.tokens.contains_key(s.root));
        assert forall|k: nat, t: TokenId|
            k <= net.n && #[trigger] s2.mem[k].contains(t) implies s2.tokens.contains_key(t)
            && s2.tokens[t].owner == k by {
            assert(s.mem[k].contains(t));
        }
        assert forall|t: TokenId|
            #![trigger s2.tokens[t].parent]
            s2.tokens.contains_key(t) && s2.tokens[t].owner > 0 implies {
            &&& s2.tokens[t].parent is Some
            &&& s2.tokens.contains_key(s2.tokens[t].parent->0)
            &&& s2.tokens[s2.tokens[t].parent->0].owner + 1 == s2.tokens[t].owner
        } by {
            lemma_parent(net, s, t);
        }
    }
    assert(inv_sound(net, s2)) by {
        assert forall|t: TokenId|
            #![trigger s2.tokens[t].parent]
            s2.tokens.contains_key(t) && s2.tokens[t].owner > 0 implies {
            &&& jmatch(
                net,
                s2.wm,
                s2.tokens[t].owner,
                s2.tokens[s2.tokens[t].parent->0].data,
                s2.tokens[t].fact->0,
            )
            &&& s2.tokens[t].data == s2.tokens[s2.tokens[t].parent->0].data.push(
                s2.wm[s2.tokens[t].fact->0],
            )
        } by {
            lemma_parent(net, s, t);
        }
    }
    assert(inv_unique(s2)) by {
        assert forall|c1: TokenId, c2: TokenId, p: TokenId, g: FactId|
            #[trigger] is_child(s2.tokens, c1, p, g) && #[trigger] is_child(
                s2.tokens,
                c2,
                p,
                g,
            ) implies c1 == c2 by {
            assert(is_child(s.tokens, c1, p, g));
            assert(is_child(s.tokens, c2, p, g));
        }
    }
    assert(inv_alpha(net, s2)) by {
        assert forall|a: nat, g: FactId| s2.amem.contains_key(a) implies (
        #[trigger] s2.amem[a].contains(g) <==> (s2.wm.contains_key(g) && (net.alpha)(
            a,
            s2.wm[g],
        ))) by {
            assert(s.amem.contains_key(a));
            if g != f {
                assert(s.amem[a].contains(g) <==> s2.amem[a].contains(g));
            }
        }
    }
    assert forall|u: TokenId| #[trigger] s2.tokens.contains_key(u) implies complete_node(
        net,
        s2.wm,
        s2.tokens,
        u,
    ) by {
        if s2.tokens[u].owner < net.n {
            assert forall|g: FactId|
                #![trigger has_child(s2.tokens, u, g)]
                jmatch(net, s2.wm, s2.tokens[u].owner + 1, s2.tokens[u].data, g) implies has_child(
                s2.tokens,
                u,
                g,
            ) by {
                assert(g != f);
                assert(jmatch(net, s.wm, s.tokens[u].owner + 1, s.tokens[u].data, g));
                assert(complete_node(net, s.wm, s.tokens, u));
                assert(has_child(s.tokens, u, g));
                let c = choose|c: TokenId| #[trigger] is_child(s.tokens, c, u, g);
                if s.tokens[c].owner == 0 {
                    assert(c == s.root);
                }
                lemma_parent(net, s, c);
                assert(!tuple(s.tokens, c).contains(f));
                assert(is_child(s2.tokens, c, u, g));
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Declarative semantics
// ---------------------------------------------------------------------------
pub open spec fn data_of<V>(wm: Map<FactId, V>, x: Seq<FactId>) -> Seq<V> {
    Seq::new(x.len(), |i: int| wm[x[i]])
}

pub open spec fn is_match<V>(net: Net<V>, wm: Map<FactId, V>, x: Seq<FactId>) -> bool {
    forall|j: int|
        0 <= j < x.len() ==> {
            &&& wm.contains_key(#[trigger] x[j])
            &&& (net.alpha)((net.alpha_of)((j + 1) as nat), wm[x[j]])
            &&& (net.test)((j + 1) as nat, data_of(wm, x.take(j)), wm[x[j]])
        }
}

pub open spec fn matches_k<V>(net: Net<V>, wm: Map<FactId, V>, k: nat) -> ISet<Seq<FactId>> {
    ISet::new(|x: Seq<FactId>| x.len() == k && is_match(net, wm, x))
}

pub open spec fn beta_tuples<V>(s: St<V>, k: nat) -> ISet<Seq<FactId>> {
    ISet::new(
        |x: Seq<FactId>| exists|t: TokenId| #[trigger] s.mem[k].contains(t) && tuple(s.tokens, t) == x,
    )
}

pub open spec fn declarative_ok<V>(net: Net<V>, s: St<V>) -> bool {
    &&& forall|k: nat| k <= net.n ==> #[trigger] beta_tuples(s, k) == matches_k(net, s.wm, k)
    &&& forall|k: nat, t1: TokenId, t2: TokenId|
        k <= net.n && #[trigger] s.mem[k].contains(t1) && #[trigger] s.mem[k].contains(t2)
            && tuple(s.tokens, t1) == tuple(s.tokens, t2) ==> t1 == t2
    &&& forall|k: nat, t: TokenId|
        1 <= k <= net.n && #[trigger] s.mem[k].contains(t) ==> s.tokens[t].parent is Some
            && s.mem[(k - 1) as nat].contains(s.tokens[t].parent->0)
}

pub proof fn lemma_is_match_push<V>(net: Net<V>, wm: Map<FactId, V>, x: Seq<FactId>, g: FactId)
    ensures
        is_match(net, wm, x.push(g)) <==> (is_match(net, wm, x) && jmatch(
            net,
            wm,
            x.len() + 1,
            data_of(wm, x),
            g,
        )),
{
    let y = x.push(g);
    assert(y.take(x.len() as int) =~= x);
    assert forall|j: int| 0 <= j < x.len() implies #[trigger] y.take(j) =~= x.take(j) && y[j]
        == x[j] by {}
    if is_match(net, wm, y) {
        assert(y[x.len() as int] == g);
        assert forall|j: int| 0 <= j < x.len() implies {
            &&& wm.contains_key(#[trigger] x[j])
            &&& (net.alpha)((net.alpha_of)((j + 1) as nat), wm[x[j]])
            &&& (net.test)((j + 1) as nat, data_of(wm, x.take(j)), wm[x[j]])
        } by {
            assert(y[j] == x[j]);
        }
    }
    if is_match(net, wm, x) && jmatch(net, wm, x.len() + 1, data_of(wm, x), g) {
        assert forall|j: int| 0 <= j < y.len() implies {
            &&& wm.contains_key(#[trigger] y[j])
            &&& (net.alpha)((net.alpha_of)((j + 1) as nat), wm[y[j]])
            &&& (net.test)((j + 1) as nat, data_of(wm, y.take(j)), wm[y[j]])
        } by {
            if j < x.len() {
                assert(x[j] == y[j]);
            }
        }
    }
}

pub proof fn lemma_sound<V>(net: Net<V>, s: St<V>, t: TokenId)
    requires
        inv_core(net, s),
        s.tokens.contains_key(t),
    ensures
        tuple(s.tokens, t).len() == s.tokens[t].owner,
        is_match(net, s.wm, tuple(s.tokens, t)),
        s.tokens[t].data == data_of(s.wm, tuple(s.tokens, t)),
    decreases s.tokens[t].owner,
{
    lemma_facts_len(s.tokens, t, s.tokens[t].owner);
    if s.tokens[t].owner == 0 {
        assert(t == s.root);
        assert(data_of(s.wm, tuple(s.tokens, t)) =~= Seq::<V>::empty());
    } else {
        lemma_parent(net, s, t);
        let p = s.tokens[t].parent->0;
        let g = s.tokens[t].fact->0;
        lemma_sound(net, s, p);
        lemma_is_match_push(net, s.wm, tuple(s.tokens, p), g);
        assert(data_of(s.wm, tuple(s.tokens, p).push(g)) =~= data_of(
            s.wm,
            tuple(s.tokens, p),
        ).push(s.wm[g]));
    }
}

pub proof fn lemma_unique<V>(net: Net<V>, s: St<V>, t1: TokenId, t2: TokenId)
    requires
        inv_core(net, s),
        s.tokens.contains_key(t1),
        s.tokens.contains_key(t2),
        tuple(s.tokens, t1) == tuple(s.tokens, t2),
    ensures
        t1 == t2,
    decreases s.tokens[t1].owner,
{
    lemma_facts_len(s.tokens, t1, s.tokens[t1].owner);
    lemma_facts_len(s.tokens, t2, s.tokens[t2].owner);
    if s.tokens[t1].owner == 0 {
        assert(t1 == s.root && t2 == s.root);
    } else {
        lemma_parent(net, s, t1);
        lemma_parent(net, s, t2);
        let p1 = s.tokens[t1].parent->0;
        let p2 = s.tokens[t2].parent->0;
        let x1 = tuple(s.tokens, p1);
        let x2 = tuple(s.tokens, p2);
        let g1 = s.tokens[t1].fact->0;
        let g2 = s.tokens[t2].fact->0;
        assert(x1.push(g1)[x1.len() as int] == x2.push(g2)[x2.len() as int]);
        assert(x1 =~= x2) by {
            assert forall|i: int| 0 <= i < x1.len() implies x1[i] == x2[i] by {
                assert(x1.push(g1)[i] == x2.push(g2)[i]);
            }
        }
        lemma_unique(net, s, p1, p2);
        assert(is_child(s.tokens, t1, p1, g1));
        assert(is_child(s.tokens, t2, p1, g1));
    }
}

pub proof fn lemma_complete<V>(net: Net<V>, s: St<V>, x: Seq<FactId>) -> (t: TokenId)
    requires
        inv(net, s),
        x.len() <= net.n,
        is_match(net, s.wm, x),
    ensures
        s.tokens.contains_key(t),
        s.tokens[t].owner == x.len(),
        tuple(s.tokens, t) == x,
    decreases x.len(),
{
    if x.len() == 0 {
        assert(tuple(s.tokens, s.root) =~= x);
        s.root
    } else {
        let pre = x.drop_last();
        let g = x.last();
        assert(x =~= pre.push(g));
        lemma_is_match_push(net, s.wm, pre, g);
        let p = lemma_complete(net, s, pre);
        lemma_sound(net, s, p);
        assert(complete_node(net, s.wm, s.tokens, p));
        assert(jmatch(net, s.wm, s.tokens[p].owner + 1, s.tokens[p].data, g));
        assert(has_child(s.tokens, p, g));
        let c = choose|c: TokenId| #[trigger] is_child(s.tokens, c, p, g);
        if s.tokens[c].owner == 0 {
            assert(c == s.root);
        }
        lemma_parent(net, s, c);
        c
    }
}

pub proof fn lemma_declarative<V>(net: Net<V>, s: St<V>)
    requires
        inv(net, s),
    ensures
        declarative_ok(net, s),
{
    assert forall|k: nat| k <= net.n implies #[trigger] beta_tuples(s, k) == matches_k(
        net,
        s.wm,
        k,
    ) by {
        assert forall|x: Seq<FactId>|
            #[trigger] beta_tuples(s, k).contains(x) <==> matches_k(net, s.wm, k).contains(x) by {
            if beta_tuples(s, k).contains(x) {
                let t = choose|t: TokenId| #[trigger] s.mem[k].contains(t) && tuple(s.tokens, t) == x;
                lemma_sound(net, s, t);
            }
            if matches_k(net, s.wm, k).contains(x) {
                let t = lemma_complete(net, s, x);
                assert(s.mem[k].contains(t));
            }
        }
        assert(beta_tuples(s, k) =~= matches_k(net, s.wm, k));
    }
    assert forall|k: nat, t1: TokenId, t2: TokenId|
        k <= net.n && #[trigger] s.mem[k].contains(t1) && #[trigger] s.mem[k].contains(t2)
            && tuple(s.tokens, t1) == tuple(s.tokens, t2) implies t1 == t2 by {
        lemma_unique(net, s, t1, t2);
    }
    assert forall|k: nat, t: TokenId|
        1 <= k <= net.n && #[trigger] s.mem[k].contains(t) implies s.tokens[t].parent is Some
        && s.mem[(k - 1) as nat].contains(s.tokens[t].parent->0) by {
        lemma_parent(net, s, t);
    }
}

// ---------------------------------------------------------------------------
// Initial state, operation sequences, main theorem
// ---------------------------------------------------------------------------
/// Empty working memory after `ReteNetwork::seed_root_token`
/// (crates/ferric-rules-core/src/rete.rs:710): the root memory holds the one
/// empty-prefix token, every other memory is empty.
pub open spec fn init<V>(net: Net<V>, root: TokenId) -> St<V> {
    St {
        wm: Map::empty(),
        amem: Map::new(net.alphas, |a: nat| Set::<FactId>::empty()),
        tokens: Map::empty().insert(root, root_tok()),
        mem: Map::new(
            Set::<nat>::range(0, net.n + 1),
            |k: nat|
                if k == 0 {
                    Set::<TokenId>::empty().insert(root)
                } else {
                    Set::<TokenId>::empty()
                },
        ),
        root,
        clock: 0,
    }
}

pub enum Op<V> {
    Assert(FactId, V),
    Retract(FactId),
}

/// One engine operation. An assertion carries a fresh fact id (`FactBase`
/// issues SlotMap keys; an id already present is treated as a no-op, which
/// the engine never issues); retracting an absent id is rejected by the
/// engine (`EngineError::FactNotFound`, engine.rs:1096-1107), modelled as a
/// no-op.
pub open spec fn step<V>(net: Net<V>, sc: Sched, s: St<V>, op: Op<V>) -> St<V> {
    match op {
        Op::Assert(f, v) => if s.wm.contains_key(f) {
            s
        } else {
            assert_fact(net, sc, St { wm: s.wm.insert(f, v), ..s }, f)
        },
        Op::Retract(f) => if s.wm.contains_key(f) {
            retract_fact(net, s, f)
        } else {
            s
        },
    }
}

pub open spec fn run<V>(net: Net<V>, sc: Sched, s: St<V>, ops: Seq<Op<V>>) -> St<V>
    decreases ops.len(),
{
    if ops.len() == 0 {
        s
    } else {
        run(net, sc, step(net, sc, s, ops[0]), ops.drop_first())
    }
}

pub proof fn lemma_init<V>(net: Net<V>, root: TokenId)
    requires
        net_wf(net),
    ensures
        inv(net, init(net, root)),
{
    let s = init(net, root);
    assert(inv_struct(net, s)) by {
        assert forall|k: nat, t: TokenId|
            k <= net.n && #[trigger] s.mem[k].contains(t) implies s.tokens.contains_key(t)
            && s.tokens[t].owner == k by {
            if k > 0 {
                assert(!s.mem[k].contains(t));
            }
        }
    }
    assert(inv_unique(s));
    assert forall|u: TokenId| #[trigger] s.tokens.contains_key(u) implies complete_node(
        net,
        s.wm,
        s.tokens,
        u,
    ) by {}
}

pub proof fn lemma_step<V>(net: Net<V>, sc: Sched, s: St<V>, op: Op<V>)
    requires
        net_wf(net),
        sched_ok(sc),
        inv(net, s),
    ensures
        inv(net, step(net, sc, s, op)),
{
    match op {
        Op::Assert(f, v) => {
            if !s.wm.contains_key(f) {
                lemma_assert_fact(net, sc, s, f, v);
            }
        },
        Op::Retract(f) => {
            if s.wm.contains_key(f) {
                lemma_retract_fact(net, s, f);
            }
        },
    }
}

pub proof fn lemma_run<V>(net: Net<V>, sc: Sched, s: St<V>, ops: Seq<Op<V>>)
    requires
        net_wf(net),
        sched_ok(sc),
        inv(net, s),
    ensures
        inv(net, run(net, sc, s, ops)),
    decreases ops.len(),
{
    if ops.len() > 0 {
        lemma_step(net, sc, s, ops[0]);
        lemma_run(net, sc, step(net, sc, s, ops[0]), ops.drop_first());
    }
}

/// Main theorem: after any finite sequence of assertions and retractions,
/// every beta memory of the chain holds exactly one token per declarative
/// match of its prefix, and every token's parent is in the previous memory.
pub proof fn theorem_incremental_equals_declarative<V>(
    net: Net<V>,
    sc: Sched,
    root: TokenId,
    ops: Seq<Op<V>>,
)
    requires
        net_wf(net),
        sched_ok(sc),
    ensures
        declarative_ok(net, run(net, sc, init(net, root), ops)),
{
    lemma_init(net, root);
    lemma_run(net, sc, init(net, root), ops);
    lemma_declarative(net, run(net, sc, init(net, root), ops));
}

// ---------------------------------------------------------------------------
// Per-operation deltas
// ---------------------------------------------------------------------------
pub proof fn lemma_tuple_ext<V>(net: Net<V>, s: St<V>, toks2: Map<TokenId, Tok<V>>, t: TokenId)
    requires
        inv_struct(net, s),
        tok_ext(s.tokens, toks2),
        s.tokens.contains_key(t),
    ensures
        tuple(toks2, t) == tuple(s.tokens, t),
    decreases s.tokens[t].owner,
{
    if s.tokens[t].owner > 0 {
        lemma_parent(net, s, t);
        lemma_tuple_ext(net, s, toks2, s.tokens[t].parent->0);
    }
}

pub proof fn lemma_is_match_frame<V>(
    net: Net<V>,
    wm: Map<FactId, V>,
    f: FactId,
    v: V,
    x: Seq<FactId>,
)
    requires
        !x.contains(f),
        is_match(net, wm.insert(f, v), x),
    ensures
        is_match(net, wm, x),
{
    let wm2 = wm.insert(f, v);
    assert forall|j: int| 0 <= j < x.len() implies {
        &&& wm.contains_key(#[trigger] x[j])
        &&& (net.alpha)((net.alpha_of)((j + 1) as nat), wm[x[j]])
        &&& (net.test)((j + 1) as nat, data_of(wm, x.take(j)), wm[x[j]])
    } by {
        assert(x[j] != f);
        assert(data_of(wm2, x.take(j)) =~= data_of(wm, x.take(j))) by {
            assert forall|i: int| 0 <= i < j implies #[trigger] x.take(j)[i] != f by {
                assert(x.take(j)[i] == x[i]);
            }
        }
    }
}

/// Assertion keeps every existing token (same identity, same contents) and
/// adds exactly the tokens whose fact tuple mentions the asserted fact.
pub proof fn theorem_assert_delta<V>(net: Net<V>, sc: Sched, s: St<V>, f: FactId, v: V)
    requires
        net_wf(net),
        sched_ok(sc),
        inv(net, s),
        !s.wm.contains_key(f),
    ensures
        ({
            let s2 = step(net, sc, s, Op::Assert(f, v));
            &&& inv(net, s2)
            &&& s2.wm == s.wm.insert(f, v)
            &&& forall|t: TokenId| #[trigger]
                s.tokens.contains_key(t) ==> s2.tokens.contains_key(t) && s2.tokens[t]
                    == s.tokens[t] && tuple(s2.tokens, t) == tuple(s.tokens, t)
            &&& forall|u: TokenId| #[trigger]
                s2.tokens.contains_key(u) ==> (!s.tokens.contains_key(u) <==> tuple(
                    s2.tokens,
                    u,
                ).contains(f))
        }),
{
    lemma_assert_fact(net, sc, s, f, v);
    let s2 = step(net, sc, s, Op::Assert(f, v));
    assert forall|t: TokenId| #[trigger] s.tokens.contains_key(t) implies tuple(s2.tokens, t)
        == tuple(s.tokens, t) && !tuple(s.tokens, t).contains(f) by {
        lemma_tuple_ext(net, s, s2.tokens, t);
        lemma_sound(net, s, t);
        let x = tuple(s.tokens, t);
        if x.contains(f) {
            let j = choose|j: int| 0 <= j < x.len() && x[j] == f;
            assert(s.wm.contains_key(x[j]));
        }
    }
    assert forall|u: TokenId| #[trigger] s2.tokens.contains_key(u) implies (!s.tokens.contains_key(
        u,
    ) <==> tuple(s2.tokens, u).contains(f)) by {
        if !s.tokens.contains_key(u) && !tuple(s2.tokens, u).contains(f) {
            let x = tuple(s2.tokens, u);
            lemma_sound(net, s2, u);
            lemma_is_match_frame(net, s.wm, f, v, x);
            let t = lemma_complete(net, s, x);
            lemma_unique(net, s2, t, u);
        }
    }
}

/// Retraction removes exactly the tokens whose fact tuple mentions the
/// retracted fact and leaves every other token untouched.
pub proof fn theorem_retract_delta<V>(net: Net<V>, sc: Sched, s: St<V>, f: FactId)
    requires
        net_wf(net),
        inv(net, s),
        s.wm.contains_key(f),
    ensures
        ({
            let s2 = step(net, sc, s, Op::Retract(f));
            &&& inv(net, s2)
            &&& s2.wm == s.wm.remove(f)
            &&& forall|t: TokenId| #[trigger]
                s2.tokens.contains_key(t) <==> (s.tokens.contains_key(t) && !tuple(
                    s.tokens,
                    t,
                ).contains(f))
            &&& forall|t: TokenId| #[trigger]
                s2.tokens.contains_key(t) ==> s2.tokens[t] == s.tokens[t]
        }),
{
    lemma_retract_fact(net, s, f);
}

// ---------------------------------------------------------------------------
// Non-vacuity
// ---------------------------------------------------------------------------
pub proof fn lemma_fresh_exists(s: Set<nat>)
    ensures
        exists|x: nat| !s.contains(x),
{
    if forall|x: nat| s.contains(x) {
        let r = Set::<nat>::range(0, s.len() + 1);
        assert(r.subset_of(s));
        vstd::set_lib::lemma_len_subset(r, s);
    }
}

/// The scheduler hypothesis is satisfiable (so the theorems are not vacuous):
/// `Set::to_seq` is a valid iteration order and "least unused id" a valid
/// fresh-key supply.
pub proof fn lemma_sched_satisfiable() -> (sc: Sched)
    ensures
        sched_ok(sc),
{
    let sc = Sched {
        ord: |c: nat, s: Set<nat>| s.to_seq(),
        fresh: |c: nat, s: Set<nat>| choose|x: nat| !s.contains(x),
    };
    assert forall|c: nat, s: Set<nat>| #[trigger] (sc.ord)(c, s).no_duplicates() by {
        s.lemma_to_seq_no_duplicates();
    }
    assert forall|c: nat, s: Set<nat>, x: nat| #[trigger] (sc.ord)(c, s).contains(x)
        <==> s.contains(x) by {
        s.lemma_to_seq_to_set_id();
        s.to_seq().to_set_ensures();
    }
    assert forall|c: nat, s: Set<nat>| !s.contains(#[trigger] (sc.fresh)(c, s)) by {
        lemma_fresh_exists(s);
    }
    sc
}

/// A concrete two-join chain `(a ?x) (a ?y)` whose joins share one alpha
/// memory: after asserting a single fact `7`, the second beta memory holds
/// the self-join tuple `[7, 7]` (exactly once). This exercises the subtle
/// path in `assert_fact` where the new fact must be joined with a token that
/// was itself created by this assertion.
pub proof fn example_shared_alpha_self_join<V>(sc: Sched, root: TokenId, v: V)
    requires
        sched_ok(sc),
{
    let net = Net::<V> {
        n: 2,
        alphas: Set::<nat>::empty().insert(0),
        alpha_of: |k: nat| 0nat,
        alpha: |a: nat, d: V| true,
        test: |k: nat, d: Seq<V>, e: V| true,
    };
    let ops = seq![Op::Assert(7nat, v)];
    lemma_init(net, root);
    let s0 = init(net, root);
    lemma_assert_fact(net, sc, s0, 7, v);
    let s = run(net, sc, s0, ops);
    assert(ops.drop_first() =~= Seq::<Op<V>>::empty());
    reveal_with_fuel(run, 2);
    assert(s == step(net, sc, s0, ops[0]));
    assert(s.wm == Map::<FactId, V>::empty().insert(7, v));
    theorem_incremental_equals_declarative(net, sc, root, ops);
    let x = seq![7nat, 7nat];
    assert(x.take(0) =~= Seq::<FactId>::empty());
    assert(is_match(net, s.wm, x));
    assert(matches_k(net, s.wm, 2).contains(x));
    assert(beta_tuples(s, 2).contains(x));
    let t = choose|t: TokenId| #[trigger] s.mem[2].contains(t) && tuple(s.tokens, t) == x;
    assert(s.mem[2].contains(t) && tuple(s.tokens, t) == x);
}

} // verus!
