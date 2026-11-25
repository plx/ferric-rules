//! Verus model: end-to-end Rete activation maintenance for four rule shapes.
//!
//! Models `ReteNetwork::assert_fact` / `retract_fact` /
//! `clear_working_memory` (crates/ferric-rules-core/src/rete.rs) on the
//! network the compiler builds for one of
//!
//!   Single: `(a ?x) =>`
//!   Join:   `(a ?x) (b ?x) =>`
//!   Not:    `(a ?x) (not (b ?x)) =>`
//!   Exists: `(a ?x) (exists (b ?x)) =>`
//!
//! and proves, without bounds on facts, values or sequence length, that after
//! any sequence of assertions, retractions and resets the agenda holds
//! exactly the activations of a naive oracle computed from the working memory
//! alone, each exactly once (the agenda is a multiset whose count for a fact
//! tuple is 1 if the oracle accepts it and 0 otherwise):
//! `theorem_run_matches_oracle`. The proof goes through a network invariant
//! (`inv`) that also pins down the contents of every memory: alpha memories,
//! both join memories, the negative memory's blocked/unblocked parents and
//! blocker sets, and the exists memory's support sets and satisfied parents.
//! Lemmas: `lemma_empty_inv` (compilation / `clear_working_memory`),
//! `lemma_assert_a_preserves`, `lemma_assert_b_preserves`,
//! `lemma_retract_a_preserves`, `lemma_retract_b_preserves`,
//! `lemma_step_preserves`.
//!
//! Trusted base: Verus, Z3 and vstd. No `assume`, `admit` or external bodies.
//! Mutation checks run during development (each made verification fail):
//! activating every matching exists parent instead of only 0 -> 1 ones
//! (duplicate activation), unblocking every parent that loses a blocker
//! instead of only those left with none, keeping second-join tokens of a
//! retracted `b` fact.
//!
//! Abstractions and simplifications (fidelity gaps):
//! - Facts are `(relation, value)` pairs keyed by `nat` fact ids; the
//!   working memory is the `FactBase` contents. Engine preconditions are
//!   built into `step`: an asserted id is fresh (`FactBase` issues new ids),
//!   and a retracted id is present (the engine rejects unknown handles).
//! - Tokens are identified by their fact tuple. With a single root token and
//!   test-free first join this is a bijection: the first-join token of `a`
//!   fact `p` is `p`, a second-join token is `(p, b)`, and a negative/exists
//!   pass-through token is identified by its parent `p`. Removing "all tokens
//!   containing `f` and their descendants" (`tokens_containing` +
//!   `remove_cascade`) is therefore "all tuples mentioning `f`".
//! - Loops over parents or facts in the production code act on distinct
//!   parents independently, so each loop is modelled by its aggregate
//!   effect (a set/map comprehension) instead of an iteration order.
//! - Not modelled: block-order chronology (`next_block_order`, primary
//!   blockers), reverse indices (`fact_to_blocked`, `fact_to_parents`,
//!   `TokenStore` indices), activation ordering/recency/salience, binding
//!   sets and the alpha/beta join *indices* (lookups are modelled by the
//!   filter they implement), predicates, NCC, auto-focus.
//! - One rule per network: node sharing between rules (the compiler's
//!   alpha-path and join caches) is not modelled.
//! - Known gap: there is no mechanical link to the production code. Bounded
//!   Kani checking of the same oracle on the production `ReteNetwork` was
//!   measured as intractable (see
//!   crates/ferric-rules-core/src/rete/kani_e2e_proofs.rs), so fidelity rests
//!   on the line-by-line correspondence documented on each spec function.
use vstd::multiset::Multiset;
use vstd::prelude::*;

verus! {

broadcast use {
    vstd::set::group_set_lemmas,
    vstd::set_lib::group_set_lib_default,
    vstd::map::group_map_lemmas,
    vstd::multiset::group_multiset_axioms,
    vstd::set::Set::lemma_map_contains,
};

pub enum Rel {
    A,
    B,
}

pub struct FactData {
    pub rel: Rel,
    pub val: int,
}

pub enum Shape {
    Single,
    Join,
    Not,
    Exists,
}

/// `FactBase` contents: fact id -> (relation, value).
pub type Wm = Map<nat, FactData>;

/// Runtime state of the compiled network.
pub struct Net {
    /// `AlphaMemory.facts` of the `(a ...)` path (alpha.rs).
    pub alpha_a: Set<nat>,
    /// `AlphaMemory.facts` of the `(b ...)` path.
    pub alpha_b: Set<nat>,
    /// `BetaMemory.tokens` of the first join `(a ?x)`, by `a` fact.
    pub join1: Set<nat>,
    /// Join shape: `BetaMemory.tokens` of the second join `(b ?x)`.
    pub join2: Set<(nat, nat)>,
    /// Not shape: `NegativeMemory.blocked` (negative.rs), parent -> blockers.
    pub blocked: Map<nat, Set<nat>>,
    /// Not shape: parents in `NegativeMemory.unblocked` (with a pass-through).
    pub unblocked: Set<nat>,
    /// Exists shape: `ExistsMemory.support` (exists.rs), parent -> supports.
    pub support: Map<nat, Set<nat>>,
    /// Exists shape: parents in `ExistsMemory.satisfied`.
    pub satisfied: Set<nat>,
    /// Agenda activations, by fact tuple of the activation token. A
    /// multiset, so a duplicated activation would be visible.
    pub agenda: Multiset<Seq<nat>>,
}

pub open spec fn is_a(wm: Wm, f: nat) -> bool {
    wm.dom().contains(f) && wm[f].rel == Rel::A
}

pub open spec fn is_b(wm: Wm, f: nat) -> bool {
    wm.dom().contains(f) && wm[f].rel == Rel::B
}

/// Some `b` fact in working memory carries `v`.
pub open spec fn some_b(wm: Wm, v: int) -> bool {
    exists|b: nat| #[trigger] is_b(wm, b) && wm[b].val == v
}

/// The naive oracle: the activations a rule of `shape` must have, computed
/// directly from the working memory.
pub open spec fn oracle(shape: Shape, wm: Wm, t: Seq<nat>) -> bool {
    match shape {
        Shape::Single => t.len() == 1 && is_a(wm, t[0]),
        Shape::Join => t.len() == 2 && is_a(wm, t[0]) && is_b(wm, t[1]) && wm[t[0]].val
            == wm[t[1]].val,
        Shape::Not => t.len() == 1 && is_a(wm, t[0]) && !some_b(wm, wm[t[0]].val),
        Shape::Exists => t.len() == 1 && is_a(wm, t[0]) && some_b(wm, wm[t[0]].val),
    }
}

/// Network right after compilation, or after `clear_working_memory`
/// (rete.rs:498): the root token reaches the first join, whose alpha memory
/// is empty, so nothing else is populated.
pub open spec fn empty_net() -> Net {
    Net {
        alpha_a: Set::empty(),
        alpha_b: Set::empty(),
        join1: Set::empty(),
        join2: Set::empty(),
        blocked: Map::empty(),
        unblocked: Set::empty(),
        support: Map::empty(),
        satisfied: Set::empty(),
        agenda: Multiset::empty(),
    }
}

/// Candidate `b` facts of the alpha memory passing the join test `?x = v`
/// (rete.rs:2637 `collect_candidate_facts` + rete.rs:2816 `evaluate_pattern`).
pub open spec fn matching(net: Net, wm: Wm, v: int) -> Set<nat> {
    net.alpha_b.filter(|b: nat| wm[b].val == v)
}

/// First-join tokens passing the join test against value `v`
/// (rete.rs:2665 `collect_candidate_parent_tokens` + rete.rs:2690 `evaluate_join`).
pub open spec fn parents_matching(net: Net, wm: Wm, v: int) -> Set<nat> {
    net.join1.filter(|p: nat| wm[p].val == v)
}

// ---------------------------------------------------------------------
// Assertion
// ---------------------------------------------------------------------
/// Models rete.rs:1162 `left_activate` of the second join for the new
/// first-join token `p`: one token and one activation per matching `b`.
pub open spec fn join2_left_activate(net: Net, wm: Wm, p: nat) -> Net {
    let m = matching(net, wm, wm[p].val);
    Net {
        join2: net.join2.union(m.map(|b: nat| (p, b))),
        agenda: net.agenda.add(Multiset::from_set(m.map(|b: nat| seq![p, b]))),
        ..net
    }
}

/// Models rete.rs:1211 `negative_left_activate`: no blocker -> pass-through
/// token, `set_unblocked`, activation; otherwise `add_blocker` for each.
pub open spec fn negative_left_activate(net: Net, wm: Wm, p: nat) -> Net {
    let blocking = matching(net, wm, wm[p].val);
    if blocking.is_empty() {
        Net { unblocked: net.unblocked.insert(p), agenda: net.agenda.insert(seq![p]), ..net }
    } else {
        Net { blocked: net.blocked.insert(p, blocking), ..net }
    }
}

/// Models rete.rs:2043 `exists_left_activate`: `add_support` for each
/// supporting fact; with support, a pass-through, `set_satisfied`, activation.
pub open spec fn exists_left_activate(net: Net, wm: Wm, p: nat) -> Net {
    let supporting = matching(net, wm, wm[p].val);
    if supporting.is_empty() {
        net
    } else {
        Net {
            support: net.support.insert(p, supporting),
            satisfied: net.satisfied.insert(p),
            agenda: net.agenda.insert(seq![p]),
            ..net
        }
    }
}

/// Models rete.rs:2359 `propagate_token` / rete.rs:2440 `propagate_to_child`
/// of a new first-join token `p` to the first join's only child.
pub open spec fn propagate_from_join1(shape: Shape, net: Net, wm: Wm, p: nat) -> Net {
    match shape {
        Shape::Single => Net { agenda: net.agenda.insert(seq![p]), ..net },
        Shape::Join => join2_left_activate(net, wm, p),
        Shape::Not => negative_left_activate(net, wm, p),
        Shape::Exists => exists_left_activate(net, wm, p),
    }
}

/// Models rete.rs:1023 `right_activate` of the second join for new `b` fact
/// `g`: one token and activation per matching first-join parent.
pub open spec fn join2_right_activate(net: Net, wm: Wm, g: nat) -> Net {
    let ps = parents_matching(net, wm, wm[g].val);
    Net {
        join2: net.join2.union(ps.map(|p: nat| (p, g))),
        agenda: net.agenda.add(Multiset::from_set(ps.map(|p: nat| seq![p, g]))),
        ..net
    }
}

/// Models rete.rs:1321 `negative_right_activate` for new `b` fact `g`:
/// already blocked matching parents record `g` as a further blocker; matching
/// unblocked parents become blocked by `g` and their pass-through is
/// cascade-retracted (rete.rs:1519 `retract_token_cascade`), removing its
/// activation.
pub open spec fn negative_right_activate(net: Net, wm: Wm, g: nat) -> Net {
    let v = wm[g].val;
    let to_block = net.unblocked.filter(|p: nat| wm[p].val == v);
    Net {
        blocked: Map::new(
            net.blocked.dom().union(to_block),
            |p: nat|
                if to_block.contains(p) {
                    set![g]
                } else if wm[p].val == v {
                    net.blocked[p].insert(g)
                } else {
                    net.blocked[p]
                },
        ),
        unblocked: net.unblocked.difference(to_block),
        agenda: net.agenda.filter(|t: Seq<nat>| !(t.len() == 1 && to_block.contains(t[0]))),
        ..net
    }
}

/// Models rete.rs:2152 `exists_right_activate` for new `b` fact `g`: every
/// matching parent gains support; a parent whose count goes 0 -> 1 gets a
/// pass-through, `set_satisfied` and an activation.
pub open spec fn exists_right_activate(net: Net, wm: Wm, g: nat) -> Net {
    let ps = parents_matching(net, wm, wm[g].val);
    let fresh = ps.filter(|p: nat| !net.support.dom().contains(p));
    Net {
        support: Map::new(
            net.support.dom().union(ps),
            |p: nat|
                if ps.contains(p) {
                    if net.support.dom().contains(p) {
                        net.support[p].insert(g)
                    } else {
                        set![g]
                    }
                } else {
                    net.support[p]
                },
        ),
        satisfied: net.satisfied.union(fresh),
        agenda: net.agenda.add(Multiset::from_set(fresh.map(|p: nat| seq![p]))),
        ..net
    }
}

/// Models rete.rs:281 `ReteNetwork::assert_fact(f)`; `wm` already holds `f`
/// (the engine inserts into the `FactBase` first). The alpha network accepts
/// `f` into its relation's memory; `a` facts right-activate the first join
/// (root parent, no tests), `b` facts the rule's second node, if any.
pub open spec fn assert_fact(shape: Shape, net: Net, wm: Wm, f: nat) -> Net {
    if wm[f].rel == Rel::A {
        let n = Net { alpha_a: net.alpha_a.insert(f), join1: net.join1.insert(f), ..net };
        propagate_from_join1(shape, n, wm, f)
    } else {
        let n = Net { alpha_b: net.alpha_b.insert(f), ..net };
        match shape {
            Shape::Single => n,
            Shape::Join => join2_right_activate(n, wm, f),
            Shape::Not => negative_right_activate(n, wm, f),
            Shape::Exists => exists_right_activate(n, wm, f),
        }
    }
}

// ---------------------------------------------------------------------
// Retraction
// ---------------------------------------------------------------------
/// Models rete.rs:1409 `collect_negative_retractions` +
/// rete.rs:1450 `apply_negative_retraction`: `f` leaves every block list it
/// is on (`remove_blocker`); a parent left with none is unblocked with a new
/// pass-through and activation.
pub open spec fn negative_retraction(net: Net, f: nat) -> Net {
    let parents = net.blocked.dom().filter(|p: nat| net.blocked[p].contains(f));
    let freed = parents.filter(|p: nat| net.blocked[p].remove(f).is_empty());
    Net {
        blocked: Map::new(
            net.blocked.dom().difference(freed),
            |p: nat|
                if parents.contains(p) {
                    net.blocked[p].remove(f)
                } else {
                    net.blocked[p]
                },
        ),
        unblocked: net.unblocked.union(freed),
        agenda: net.agenda.add(Multiset::from_set(freed.map(|p: nat| seq![p]))),
        ..net
    }
}

/// Models rete.rs:2255 `exists_handle_retraction`: `f` leaves every support
/// set (`remove_support`); a parent left with none loses its pass-through
/// (`remove_satisfied` + `retract_token_cascade`) and its activation.
pub open spec fn exists_retraction(net: Net, f: nat) -> Net {
    let parents = net.support.dom().filter(|p: nat| net.support[p].contains(f));
    let emptied = parents.filter(|p: nat| net.support[p].remove(f).is_empty());
    Net {
        support: Map::new(
            net.support.dom().difference(emptied),
            |p: nat|
                if parents.contains(p) {
                    net.support[p].remove(f)
                } else {
                    net.support[p]
                },
        ),
        satisfied: net.satisfied.difference(emptied),
        agenda: net.agenda.filter(|t: Seq<nat>| !(t.len() == 1 && emptied.contains(t[0]))),
        ..net
    }
}

/// Models rete.rs:361 `ReteNetwork::retract_fact(f)`; `wm` still holds `f`
/// (the engine retracts from the network first). Tokens containing `f` and
/// their descendants are removed with their activations; removing an `a`
/// fact's first-join token also drops its negative/exists bookkeeping
/// (rete.rs:1552 `cleanup_negative_memories_for_token` ->
/// `remove_parent_token`). Then the alpha memory drops `f` and the
/// negative/exists retraction runs.
pub open spec fn retract_fact(shape: Shape, net: Net, wm: Wm, f: nat) -> Net {
    if wm[f].rel == Rel::A {
        Net {
            alpha_a: net.alpha_a.remove(f),
            join1: net.join1.remove(f),
            join2: net.join2.filter(|pb: (nat, nat)| pb.0 != f),
            blocked: net.blocked.remove(f),
            unblocked: net.unblocked.remove(f),
            support: net.support.remove(f),
            satisfied: net.satisfied.remove(f),
            agenda: net.agenda.filter(|t: Seq<nat>| !(t.len() >= 1 && t[0] == f)),
            ..net
        }
    } else {
        let n = Net {
            alpha_b: net.alpha_b.remove(f),
            join2: net.join2.filter(|pb: (nat, nat)| pb.1 != f),
            agenda: net.agenda.filter(|t: Seq<nat>| !(t.len() == 2 && t[1] == f)),
            ..net
        };
        match shape {
            Shape::Not => negative_retraction(n, f),
            Shape::Exists => exists_retraction(n, f),
            _ => n,
        }
    }
}

// ---------------------------------------------------------------------
// Invariant
// ---------------------------------------------------------------------
pub open spec fn inv(shape: Shape, net: Net, wm: Wm) -> bool {
    &&& forall|f: nat| #[trigger] net.alpha_a.contains(f) <==> is_a(wm, f)
    &&& forall|f: nat| #[trigger] net.alpha_b.contains(f) <==> is_b(wm, f)
    &&& forall|f: nat| #[trigger] net.join1.contains(f) <==> is_a(wm, f)
    &&& forall|p: nat, b: nat|
        #[trigger] net.join2.contains((p, b)) <==> (shape == Shape::Join && is_a(wm, p) && is_b(
            wm,
            b,
        ) && wm[p].val == wm[b].val)
    &&& forall|p: nat|
        #[trigger] net.unblocked.contains(p) <==> (shape == Shape::Not && is_a(wm, p) && !some_b(
            wm,
            wm[p].val,
        ))
    &&& forall|p: nat|
        #[trigger] net.blocked.dom().contains(p) <==> (shape == Shape::Not && is_a(wm, p)
            && some_b(wm, wm[p].val))
    &&& forall|p: nat, b: nat|
        net.blocked.dom().contains(p) ==> (#[trigger] net.blocked[p].contains(b) <==> (is_b(
            wm,
            b,
        ) && wm[b].val == wm[p].val))
    &&& forall|p: nat|
        #[trigger] net.support.dom().contains(p) <==> (shape == Shape::Exists && is_a(wm, p)
            && some_b(wm, wm[p].val))
    &&& forall|p: nat, b: nat|
        net.support.dom().contains(p) ==> (#[trigger] net.support[p].contains(b) <==> (is_b(
            wm,
            b,
        ) && wm[b].val == wm[p].val))
    &&& forall|p: nat| #[trigger]
        net.satisfied.contains(p) <==> net.support.dom().contains(p)
    &&& forall|t: Seq<nat>|
        #[trigger] net.agenda.count(t) == (if oracle(shape, wm, t) {
            1nat
        } else {
            0nat
        })
}

pub proof fn lemma_empty_inv(shape: Shape)
    ensures
        inv(shape, empty_net(), Map::empty()),
{
}

// ---------------------------------------------------------------------
// Preservation
// ---------------------------------------------------------------------
pub proof fn lemma_assert_a_preserves(shape: Shape, net: Net, wm: Wm, f: nat, v: int)
    requires
        inv(shape, net, wm),
        !wm.dom().contains(f),
    ensures
        ({
            let wm2 = wm.insert(f, FactData { rel: Rel::A, val: v });
            inv(shape, assert_fact(shape, net, wm2, f), wm2)
        }),
{
    let wm2 = wm.insert(f, FactData { rel: Rel::A, val: v });
    let n = Net { alpha_a: net.alpha_a.insert(f), join1: net.join1.insert(f), ..net };
    let post = assert_fact(shape, net, wm2, f);
    assert(forall|b: nat| is_b(wm2, b) <==> is_b(wm, b));
    assert(forall|b: nat| is_b(wm, b) ==> wm2[b] == wm[b]);
    assert(forall|x: int| some_b(wm2, x) <==> some_b(wm, x)) by {
        assert forall|x: int| some_b(wm2, x) implies some_b(wm, x) by {
            let b = choose|b: nat| #[trigger] is_b(wm2, b) && wm2[b].val == x;
            assert(is_b(wm, b) && wm[b].val == x);
        }
        assert forall|x: int| some_b(wm, x) implies some_b(wm2, x) by {
            let b = choose|b: nat| #[trigger] is_b(wm, b) && wm[b].val == x;
            assert(is_b(wm2, b) && wm2[b].val == x);
        }
    }
    let m = matching(n, wm2, v);
    assert(forall|b: nat| #[trigger] m.contains(b) <==> (is_b(wm, b) && wm[b].val == v));
    assert(m.is_empty() <==> !some_b(wm, v)) by {
        if some_b(wm, v) {
            let b = choose|b: nat| #[trigger] is_b(wm, b) && wm[b].val == v;
            assert(m.contains(b));
        } else {
            assert(m =~= Set::empty());
        }
    }
    match shape {
        Shape::Join => {
            assert forall|p: nat, b: nat| #[trigger]
                post.join2.contains((p, b)) <==> (is_a(wm2, p) && is_b(wm2, b) && wm2[p].val
                    == wm2[b].val) by {
                if p == f && is_b(wm, b) && wm[b].val == v {
                    assert(m.contains(b));
                    assert((p, b) == (|b: nat| (f, b))(b));
                }
            }
            assert forall|t: Seq<nat>| #[trigger]
                post.agenda.count(t) == (if oracle(shape, wm2, t) { 1nat } else { 0nat }) by {
                if oracle(shape, wm2, t) && t[0] == f {
                    assert(m.contains(t[1]));
                    assert(t =~= (|b: nat| seq![f, b])(t[1]));
                }
            }
        },
        Shape::Single => {
            assert forall|t: Seq<nat>| #[trigger]
                post.agenda.count(t) == (if oracle(shape, wm2, t) { 1nat } else { 0nat }) by {
                if oracle(shape, wm2, t) && t[0] == f {
                    assert(t =~= seq![f]);
                }
            }
        },
        Shape::Not => {
            assert forall|t: Seq<nat>| #[trigger]
                post.agenda.count(t) == (if oracle(shape, wm2, t) { 1nat } else { 0nat }) by {
                if oracle(shape, wm2, t) && t[0] == f {
                    assert(t =~= seq![f]);
                }
            }
        },
        Shape::Exists => {
            assert forall|t: Seq<nat>| #[trigger]
                post.agenda.count(t) == (if oracle(shape, wm2, t) { 1nat } else { 0nat }) by {
                if oracle(shape, wm2, t) && t[0] == f {
                    assert(t =~= seq![f]);
                }
            }
        },
    }
}

pub proof fn lemma_assert_b_preserves(shape: Shape, net: Net, wm: Wm, f: nat, v: int)
    requires
        inv(shape, net, wm),
        !wm.dom().contains(f),
    ensures
        ({
            let wm2 = wm.insert(f, FactData { rel: Rel::B, val: v });
            inv(shape, assert_fact(shape, net, wm2, f), wm2)
        }),
{
    let wm2 = wm.insert(f, FactData { rel: Rel::B, val: v });
    let n = Net { alpha_b: net.alpha_b.insert(f), ..net };
    let post = assert_fact(shape, net, wm2, f);
    assert(forall|p: nat| #[trigger] is_a(wm2, p) <==> is_a(wm, p));
    assert(forall|p: nat| wm.dom().contains(p) ==> #[trigger] wm2[p] == wm[p]);
    assert(forall|b: nat| #[trigger] is_b(wm2, b) <==> (is_b(wm, b) || b == f));
    assert forall|x: int| #[trigger] some_b(wm2, x) <==> (some_b(wm, x) || x == v) by {
        if some_b(wm2, x) && x != v {
            let b = choose|b: nat| #[trigger] is_b(wm2, b) && wm2[b].val == x;
            assert(is_b(wm, b) && wm[b].val == x);
        }
        if some_b(wm, x) {
            let b = choose|b: nat| #[trigger] is_b(wm, b) && wm[b].val == x;
            assert(is_b(wm2, b) && wm2[b].val == x);
        }
        if x == v {
            assert(is_b(wm2, f) && wm2[f].val == x);
        }
    }
    let ps = parents_matching(n, wm2, v);
    assert(forall|p: nat| #[trigger] ps.contains(p) <==> (is_a(wm, p) && wm[p].val == v));
    match shape {
        Shape::Single => {},
        Shape::Join => {
            assert forall|p: nat, b: nat| #[trigger]
                post.join2.contains((p, b)) <==> (is_a(wm2, p) && is_b(wm2, b) && wm2[p].val
                    == wm2[b].val) by {
                if b == f && is_a(wm, p) && wm[p].val == v {
                    assert(ps.contains(p));
                    assert((p, b) == (|p: nat| (p, f))(p));
                }
            }
            assert forall|t: Seq<nat>| #[trigger]
                post.agenda.count(t) == (if oracle(shape, wm2, t) { 1nat } else { 0nat }) by {
                if oracle(shape, wm2, t) && t[1] == f {
                    assert(ps.contains(t[0]));
                    assert(t =~= (|p: nat| seq![p, f])(t[0]));
                }
            }
        },
        Shape::Not => {
            let to_block = n.unblocked.filter(|p: nat| wm2[p].val == v);
            assert forall|p: nat, b: nat|
                post.blocked.dom().contains(p) implies (#[trigger] post.blocked[p].contains(b)
                <==> (is_b(wm2, b) && wm2[b].val == wm2[p].val)) by {
                if to_block.contains(p) {
                    if is_b(wm2, b) && wm2[b].val == wm2[p].val && b != f {
                        assert(some_b(wm, v));
                    }
                }
            }
        },
        Shape::Exists => {
            let fresh = ps.filter(|p: nat| !n.support.dom().contains(p));
            assert forall|p: nat, b: nat|
                post.support.dom().contains(p) implies (#[trigger] post.support[p].contains(b)
                <==> (is_b(wm2, b) && wm2[b].val == wm2[p].val)) by {
                if ps.contains(p) && !net.support.dom().contains(p) {
                    if is_b(wm2, b) && wm2[b].val == wm2[p].val && b != f {
                        assert(some_b(wm, v));
                    }
                }
            }
            assert forall|t: Seq<nat>| #[trigger]
                post.agenda.count(t) == (if oracle(shape, wm2, t) { 1nat } else { 0nat }) by {
                if oracle(shape, wm2, t) && net.agenda.count(t) == 0 {
                    assert(fresh.contains(t[0]));
                    assert(t =~= (|p: nat| seq![p])(t[0]));
                }
            }
        },
    }
}

pub proof fn lemma_retract_a_preserves(shape: Shape, net: Net, wm: Wm, f: nat)
    requires
        inv(shape, net, wm),
        is_a(wm, f),
    ensures
        inv(shape, retract_fact(shape, net, wm, f), wm.remove(f)),
{
    let wm2 = wm.remove(f);
    let post = retract_fact(shape, net, wm, f);
    assert(forall|p: nat| #[trigger] is_a(wm2, p) <==> (is_a(wm, p) && p != f));
    assert(forall|p: nat| wm2.dom().contains(p) ==> #[trigger] wm2[p] == wm[p]);
    assert(forall|b: nat| #[trigger] is_b(wm2, b) <==> is_b(wm, b));
    assert forall|x: int| #[trigger] some_b(wm2, x) <==> some_b(wm, x) by {
        if some_b(wm2, x) {
            let b = choose|b: nat| #[trigger] is_b(wm2, b) && wm2[b].val == x;
            assert(is_b(wm, b) && wm[b].val == x);
        }
        if some_b(wm, x) {
            let b = choose|b: nat| #[trigger] is_b(wm, b) && wm[b].val == x;
            assert(is_b(wm2, b) && wm2[b].val == x);
        }
    }
    assert forall|t: Seq<nat>| #[trigger]
        post.agenda.count(t) == (if oracle(shape, wm2, t) { 1nat } else { 0nat }) by {
        if oracle(shape, wm2, t) {
            assert(oracle(shape, wm, t));
        }
    }
}

pub proof fn lemma_retract_b_preserves(shape: Shape, net: Net, wm: Wm, f: nat)
    requires
        inv(shape, net, wm),
        is_b(wm, f),
    ensures
        inv(shape, retract_fact(shape, net, wm, f), wm.remove(f)),
{
    let wm2 = wm.remove(f);
    let v = wm[f].val;
    let post = retract_fact(shape, net, wm, f);
    let n = Net {
        alpha_b: net.alpha_b.remove(f),
        join2: net.join2.filter(|pb: (nat, nat)| pb.1 != f),
        agenda: net.agenda.filter(|t: Seq<nat>| !(t.len() == 2 && t[1] == f)),
        ..net
    };
    assert(forall|p: nat| #[trigger] is_a(wm2, p) <==> is_a(wm, p));
    assert(forall|p: nat| wm2.dom().contains(p) ==> #[trigger] wm2[p] == wm[p]);
    assert(forall|b: nat| #[trigger] is_b(wm2, b) <==> (is_b(wm, b) && b != f));
    match shape {
        Shape::Single => {},
        Shape::Join => {
            assert forall|t: Seq<nat>| #[trigger]
                post.agenda.count(t) == (if oracle(shape, wm2, t) { 1nat } else { 0nat }) by {
                if oracle(shape, wm2, t) {
                    assert(oracle(shape, wm, t));
                }
            }
        },
        Shape::Not => {
            let parents = n.blocked.dom().filter(|p: nat| n.blocked[p].contains(f));
            let freed = parents.filter(|p: nat| n.blocked[p].remove(f).is_empty());
            // A blocked parent keeps a blocker iff a `b` other than `f` matches it.
            assert forall|p: nat| #[trigger]
                net.blocked.dom().contains(p) implies (some_b(wm2, wm[p].val) <==> !(
                parents.contains(p) && net.blocked[p].remove(f).is_empty())) by {
                if some_b(wm2, wm[p].val) {
                    let b = choose|b: nat| #[trigger] is_b(wm2, b) && wm2[b].val == wm[p].val;
                    assert(net.blocked[p].remove(f).contains(b));
                } else {
                    assert(some_b(wm, wm[p].val));
                    let b = choose|b: nat| #[trigger] is_b(wm, b) && wm[b].val == wm[p].val;
                    if b != f {
                        assert(is_b(wm2, b) && wm2[b].val == wm[p].val);
                    }
                    assert(net.blocked[p].contains(f));
                    assert forall|x: nat| !net.blocked[p].remove(f).contains(x) by {
                        if net.blocked[p].remove(f).contains(x) {
                            assert(is_b(wm2, x) && wm2[x].val == wm[p].val);
                        }
                    }
                    assert(net.blocked[p].remove(f) =~= Set::empty());
                }
            }
            assert forall|p: nat| #[trigger]
                is_a(wm, p) && !net.blocked.dom().contains(p) implies !some_b(wm2, wm[p].val) by {
                if some_b(wm2, wm[p].val) {
                    let b = choose|b: nat| #[trigger] is_b(wm2, b) && wm2[b].val == wm[p].val;
                    assert(is_b(wm, b) && wm[b].val == wm[p].val);
                }
            }
            assert(forall|p: nat| #[trigger] post.unblocked.contains(p) <==> (is_a(wm2, p)
                && !some_b(wm2, wm2[p].val)));
            assert(forall|p: nat| #[trigger] post.blocked.dom().contains(p) <==> (is_a(wm2, p)
                && some_b(wm2, wm2[p].val)));
            assert(forall|p: nat, b: nat| post.blocked.dom().contains(p) ==> (
            #[trigger] post.blocked[p].contains(b) <==> (is_b(wm2, b) && wm2[b].val
                == wm2[p].val)));
            assert forall|t: Seq<nat>| #[trigger]
                post.agenda.count(t) == (if oracle(shape, wm2, t) { 1nat } else { 0nat }) by {
                if oracle(shape, wm2, t) && net.agenda.count(t) == 0 {
                    assert(net.blocked.dom().contains(t[0]));
                    assert(freed.contains(t[0]));
                    assert(t =~= (|p: nat| seq![p])(t[0]));
                }
            }
        },
        Shape::Exists => {
            let parents = n.support.dom().filter(|p: nat| n.support[p].contains(f));
            let emptied = parents.filter(|p: nat| n.support[p].remove(f).is_empty());
            assert forall|p: nat| #[trigger]
                net.support.dom().contains(p) implies (some_b(wm2, wm[p].val) <==> !(
                parents.contains(p) && net.support[p].remove(f).is_empty())) by {
                if some_b(wm2, wm[p].val) {
                    let b = choose|b: nat| #[trigger] is_b(wm2, b) && wm2[b].val == wm[p].val;
                    assert(net.support[p].remove(f).contains(b));
                } else {
                    assert(some_b(wm, wm[p].val));
                    let b = choose|b: nat| #[trigger] is_b(wm, b) && wm[b].val == wm[p].val;
                    if b != f {
                        assert(is_b(wm2, b) && wm2[b].val == wm[p].val);
                    }
                    assert(net.support[p].contains(f));
                    assert forall|x: nat| !net.support[p].remove(f).contains(x) by {
                        if net.support[p].remove(f).contains(x) {
                            assert(is_b(wm2, x) && wm2[x].val == wm[p].val);
                        }
                    }
                    assert(net.support[p].remove(f) =~= Set::empty());
                }
            }
            assert forall|p: nat| #[trigger]
                is_a(wm, p) && !net.support.dom().contains(p) implies !some_b(wm2, wm[p].val) by {
                if some_b(wm2, wm[p].val) {
                    let b = choose|b: nat| #[trigger] is_b(wm2, b) && wm2[b].val == wm[p].val;
                    assert(is_b(wm, b) && wm[b].val == wm[p].val);
                }
            }
            assert(forall|p: nat| #[trigger] post.support.dom().contains(p) <==> (is_a(wm2, p)
                && some_b(wm2, wm2[p].val)));
            assert(forall|p: nat, b: nat| post.support.dom().contains(p) ==> (
            #[trigger] post.support[p].contains(b) <==> (is_b(wm2, b) && wm2[b].val
                == wm2[p].val)));
            assert(forall|p: nat| #[trigger] post.satisfied.contains(p) <==> post.support.dom().contains(p));
            assert(forall|t: Seq<nat>| #[trigger] post.agenda.count(t) == (if oracle(shape, wm2, t) { 1nat } else { 0nat }));
        },
    }
}

// ---------------------------------------------------------------------
// Operation sequences
// ---------------------------------------------------------------------
pub enum Op {
    /// `Engine::assert`: a fresh fact id enters the `FactBase`, then the network.
    Assert(nat, FactData),
    /// `Engine::retract`: network first, then the `FactBase`.
    Retract(nat),
    /// `Engine::reset`: a new `FactBase` and `clear_working_memory`.
    Clear,
}

pub open spec fn step(shape: Shape, st: (Net, Wm), op: Op) -> (Net, Wm) {
    match op {
        Op::Assert(f, d) => if st.1.dom().contains(f) {
            st
        } else {
            let wm2 = st.1.insert(f, d);
            (assert_fact(shape, st.0, wm2, f), wm2)
        },
        Op::Retract(f) => if st.1.dom().contains(f) {
            (retract_fact(shape, st.0, st.1, f), st.1.remove(f))
        } else {
            st
        },
        Op::Clear => (empty_net(), Map::empty()),
    }
}

pub open spec fn run(shape: Shape, ops: Seq<Op>) -> (Net, Wm)
    decreases ops.len(),
{
    if ops.len() == 0 {
        (empty_net(), Map::empty())
    } else {
        step(shape, run(shape, ops.drop_last()), ops.last())
    }
}

pub proof fn lemma_step_preserves(shape: Shape, st: (Net, Wm), op: Op)
    requires
        inv(shape, st.0, st.1),
    ensures
        inv(shape, step(shape, st, op).0, step(shape, st, op).1),
{
    match op {
        Op::Assert(f, d) => {
            if !st.1.dom().contains(f) {
                match d.rel {
                    Rel::A => lemma_assert_a_preserves(shape, st.0, st.1, f, d.val),
                    Rel::B => lemma_assert_b_preserves(shape, st.0, st.1, f, d.val),
                }
                assert(d == FactData { rel: d.rel, val: d.val });
            }
        },
        Op::Retract(f) => {
            if st.1.dom().contains(f) {
                match st.1[f].rel {
                    Rel::A => lemma_retract_a_preserves(shape, st.0, st.1, f),
                    Rel::B => lemma_retract_b_preserves(shape, st.0, st.1, f),
                }
            }
        },
        Op::Clear => lemma_empty_inv(shape),
    }
}

/// Main theorem: after any operation sequence, the network invariant holds,
/// so in particular the agenda is exactly the naive oracle's activation set.
pub proof fn theorem_run_matches_oracle(shape: Shape, ops: Seq<Op>)
    ensures
        inv(shape, run(shape, ops).0, run(shape, ops).1),
        forall|t: Seq<nat>| #[trigger]
            run(shape, ops).0.agenda.count(t) == (if oracle(shape, run(shape, ops).1, t) {
                1nat
            } else {
                0nat
            }),
    decreases ops.len(),
{
    if ops.len() == 0 {
        lemma_empty_inv(shape);
    } else {
        theorem_run_matches_oracle(shape, ops.drop_last());
        lemma_step_preserves(shape, run(shape, ops.drop_last()), ops.last());
    }
}

} // verus!
