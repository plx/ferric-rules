//! Verus model of `TokenStore` (crates/ferric-rules-core/src/token.rs).
//!
//! What is proved (unbounded: any number of tokens, any tree shape, any
//! hash-set iteration order):
//!
//! * `StoreModel::wf` is an invariant strictly stronger than the production
//!   `validate_consistency` checks (`lemma_wf_implies_validate`): it adds the
//!   reverse direction of both indexes (every indexed token really carries
//!   that fact / that parent) and ghost acyclicity data.
//! * `insert` and `insert_sequence_match` preserve `wf` under a caller
//!   contract (fresh id; a parent, if any, is live; sequence matches carry a
//!   fact), and their `debug_assert!(inserted)` checks hold. The contract is
//!   an assumption: `TokenStore::insert` neither documents nor enforces a
//!   live parent (inserting a child of a removed parent creates a
//!   `parent_to_children` entry for a dead key, which
//!   `validate_consistency` rejects), and that every caller in rete.rs
//!   respects it is not verified here.
//! * `remove` preserves `wf` for every id (live or not), returns the token
//!   exactly when the id was live, removes exactly that token, and leaves
//!   every other token unchanged.
//! * `remove_cascade(root)`, for a live root, terminates and
//!   - removes exactly the set `R` of tokens that is the least set containing
//!     `root` and closed under "is a child of" in the pre-state (that is,
//!     exactly the descendants of `root`): `root ∈ R`, every pre-state child
//!     of a member of `R` is in `R`, every member other than `root` has its
//!     parent in `R`, and `R ⊆ X` for every child-closed `X` containing root;
//!   - returns each removed token exactly once, paired with its pre-state
//!     value; leaves all other tokens untouched; and preserves `wf`.
//! * Fact retraction as performed by `ReteNetwork::retract_fact`
//!   (crates/ferric-rules-core/src/rete.rs:377-386: `tokens_containing`,
//!   then `retraction_roots`, then one `remove_cascade` per root, in any
//!   order): every root is still live when its cascade starts (so the
//!   `debug_assert!` in `remove_cascade` holds), `wf` is preserved, every
//!   removed token is reported once with its pre-state value, and the removed
//!   tokens are exactly the live tokens whose live parent chain (including
//!   the token itself) contains a token holding the retracted fact
//!   (`lemma_retract_fact`).
//! * `retraction_roots` (its outer loop over the affected set in any
//!   hash-set order, with the parent walk as `has_affected_ancestor`)
//!   returns a duplicate-free enumeration of exactly the affected tokens
//!   with no affected ancestor (`lemma_retraction_roots`); composed with the
//!   previous item, `lemma_retract_fact_end_to_end` covers
//!   `tokens_containing` + `retraction_roots` + cascades for every hash-set
//!   order of both sets.
//! * `collect_all_facts(t)` (both loops: the parent walk that pushes every
//!   visited id, including a dangling one, and the reversed scan) returns
//!   exactly the facts of the live tokens on the parent chain of `t`, root
//!   first, skipping pass-through tokens, for any id and any well-formed
//!   store, including orphans left by non-cascading `remove`
//!   (`lemma_collect_all_facts` against the independent recursive
//!   specification `chain_facts`; `lemma_chain_facts_members` gives the
//!   membership form in terms of `reaches`).
//!
//! Trusted base and abstractions:
//!
//! * `SlotMap<TokenId, Token>` is modelled as `Map<T, TokenModel<T, F>>`; the
//!   slot map's key freshness is the precondition `!issued.contains(id)` of
//!   `insert` (slotmap returns a key never returned before, modulo version
//!   wrap-around after 2^31 reuses of one slot, which is not modelled).
//! * `FxHashMap<_, FxHashSet<TokenId>>` indexes are modelled as
//!   `Map<_, Set<T>>`; `HashSet::is_empty` as `len() == 0`.
//! * `sequence_matches` is modelled by its key set (the lengths payload is
//!   never inspected by the store).
//! * `bindings` and `owner_node` are omitted: the store never reads them.
//! * The children iteration order of `stack.extend(children.iter())` is an
//!   arbitrary enumeration `order(parent, children)` that only has to list
//!   exactly the members of the set (duplicates allowed); every theorem
//!   holds for every such enumeration.
//! * Ghost fields `issued`, `rank`, `next_rank` record every id ever issued
//!   and an insertion rank; they exist only in the model and express the
//!   acyclicity that the production store inherits from inserting parents
//!   before children.
//! * `Vec` / `SmallVec` stack and result buffers are `Seq`s. Iteration over
//!   a `HashSet` (`for &token_id in affected` in `retraction_roots`) is an
//!   arbitrary duplicate-free enumeration of the set (`affected_order`);
//!   `lemma_retract_fact` alone takes any duplicate-free enumeration of
//!   `root_set` as the roots.
//! * The `while` parent walks (in `retraction_roots` and
//!   `collect_all_facts`) are modelled as recursive spec functions that
//!   follow the loop body branch by branch; they terminate because ranks
//!   strictly decrease along live parent links.
//!
//! Known gaps: `clear` is trivial and not modelled; the rest of
//! `ReteNetwork::retract_fact` (alpha memories, agenda, negative/NCC
//! bookkeeping) is outside this model; whether every `TokenStore::insert`
//! call in rete.rs satisfies `insert_pre` (fresh id; parent live) is not
//! verified here. No TRUSTED glue, `assume`, `admit` or `external_body` is
//! used.
use vstd::prelude::*;

verus! {

broadcast use {vstd::set::group_set_lemmas, vstd::map::group_map_lemmas};

/// Models `Token` (crates/ferric-rules-core/src/token.rs:31), keeping the
/// two fields the store reads.
pub struct TokenModel<T, F> {
    pub fact: Option<F>,
    pub parent: Option<T>,
}

/// Models `TokenStore` (crates/ferric-rules-core/src/token.rs:47) plus ghost
/// fields `issued`, `rank`, `next_rank`.
pub struct StoreModel<T, F> {
    pub tokens: Map<T, TokenModel<T, F>>,
    pub sequence_matches: Set<T>,
    pub fact_to_tokens: Map<F, Set<T>>,
    pub parent_to_children: Map<T, Set<T>>,
    pub issued: Set<T>,
    pub rank: Map<T, nat>,
    pub next_rank: nat,
}

/// `order` lists exactly the members of each set (any order, duplicates
/// allowed).
pub open spec fn enumerates<T>(order: spec_fn(T, Set<T>) -> Seq<T>) -> bool {
    forall|p: T, s: Set<T>, c: T| #[trigger] order(p, s).contains(c) <==> s.contains(c)
}

/// Remove `id` from the set stored under `key`, dropping the entry when the
/// set becomes empty (the `get_mut` / `remove` / `is_empty` / `remove`
/// pattern used twice in crates/ferric-rules-core/src/token.rs:128 fn remove).
pub open spec fn remove_from_index<K, T>(index: Map<K, Set<T>>, key: K, id: T) -> Map<K, Set<T>> {
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

/// `index.entry(key).or_default().insert(id)`.
pub open spec fn add_to_index<K, T>(index: Map<K, Set<T>>, key: K, id: T) -> Map<K, Set<T>> {
    index.insert(
        key,
        if index.contains_key(key) {
            index[key]
        } else {
            Set::empty()
        }.insert(id),
    )
}

impl<T, F> StoreModel<T, F> {
    pub open spec fn parent_of(self, t: T) -> Option<T> {
        self.tokens[t].parent
    }

    /// The production checks of
    /// crates/ferric-rules-core/src/token.rs:302 fn validate_consistency.
    pub open spec fn validate(self) -> bool {
        // sequence match metadata references live tokens with a fact
        &&& forall|t: T| #[trigger]
            self.sequence_matches.contains(t) ==> self.tokens.contains_key(t) && self.tokens[t].fact is Some
        // 1. fact_to_tokens members exist
        &&& forall|f: F, t: T|
            #[trigger] self.fact_to_tokens.contains_key(f) && #[trigger] self.fact_to_tokens[f].contains(
                t,
            ) ==> self.tokens.contains_key(t)
        // 2. parent_to_children keys and members exist
        &&& forall|p: T| #[trigger] self.parent_to_children.contains_key(p) ==> self.tokens.contains_key(p)
        &&& forall|p: T, c: T|
            #[trigger] self.parent_to_children.contains_key(p)
                && #[trigger] self.parent_to_children[p].contains(c) ==> self.tokens.contains_key(c)
        // 3. every token's fact is indexed
        &&& forall|t: T| #[trigger]
            self.tokens.contains_key(t) && self.tokens[t].fact is Some ==> {
                &&& self.fact_to_tokens.contains_key(self.tokens[t].fact->0)
                &&& self.fact_to_tokens[self.tokens[t].fact->0].contains(t)
            }
        // 4. every token with a live parent is in its parent's children set
        &&& forall|t: T| #[trigger]
            self.tokens.contains_key(t) && self.tokens[t].parent is Some && self.tokens.contains_key(self.tokens[t].parent->0)
                ==> {
                &&& self.parent_to_children.contains_key(self.tokens[t].parent->0)
                &&& self.parent_to_children[self.tokens[t].parent->0].contains(t)
            }
        // 5. no empty sets
        &&& forall|f: F| #[trigger]
            self.fact_to_tokens.contains_key(f) ==> self.fact_to_tokens[f].len() > 0
        &&& forall|p: T| #[trigger]
            self.parent_to_children.contains_key(p) ==> self.parent_to_children[p].len() > 0
    }

    /// The model invariant: `validate` plus exact reverse indexes and ghost
    /// acyclicity.
    pub open spec fn wf(self) -> bool {
        &&& self.validate()
        // fact_to_tokens is exact
        &&& forall|f: F, t: T|
            #[trigger] self.fact_to_tokens.contains_key(f) && #[trigger] self.fact_to_tokens[f].contains(
                t,
            ) ==> self.tokens[t].fact == Some(f)
        // parent_to_children is exact
        &&& forall|p: T, c: T|
            #[trigger] self.parent_to_children.contains_key(p)
                && #[trigger] self.parent_to_children[p].contains(c) ==> self.tokens[c].parent
                == Some(p)
        // ghost: issued ids and insertion ranks (parents rank below children)
        &&& forall|t: T| #[trigger] self.tokens.contains_key(t) ==> self.issued.contains(t)
        &&& forall|t: T| #[trigger]
            self.tokens.contains_key(t) && self.tokens[t].parent is Some ==> {
                &&& self.issued.contains(self.tokens[t].parent->0)
                &&& self.rank[self.tokens[t].parent->0] < self.rank[t]
            }
        &&& forall|t: T| #[trigger]
            self.issued.contains(t) ==> self.rank.contains_key(t) && self.rank[t] < self.next_rank
    }

    // -----------------------------------------------------------------------
    // Operations
    // -----------------------------------------------------------------------
    /// Models crates/ferric-rules-core/src/token.rs:80 fn insert; `id` is the
    /// key returned by `self.tokens.insert(token)`.
    pub open spec fn insert(self, token: TokenModel<T, F>, id: T) -> Self {
        let tokens = self.tokens.insert(id, token);
        let fact_to_tokens = match token.fact {
            Some(fact_id) => add_to_index(self.fact_to_tokens, fact_id, id),
            None => self.fact_to_tokens,
        };
        let parent_to_children = match token.parent {
            Some(parent_id) => add_to_index(self.parent_to_children, parent_id, id),
            None => self.parent_to_children,
        };
        StoreModel {
            tokens,
            sequence_matches: self.sequence_matches,
            fact_to_tokens,
            parent_to_children,
            issued: self.issued.insert(id),
            rank: self.rank.insert(id, self.next_rank),
            next_rank: self.next_rank + 1,
        }
    }

    /// The two `debug_assert!(inserted, ..)` checks in `insert`.
    pub open spec fn insert_debug_asserts(self, token: TokenModel<T, F>, id: T) -> bool {
        &&& (token.fact is Some && self.fact_to_tokens.contains_key(token.fact->0))
            ==> !self.fact_to_tokens[token.fact->0].contains(id)
        &&& (token.parent is Some && self.parent_to_children.contains_key(token.parent->0))
            ==> !self.parent_to_children[token.parent->0].contains(id)
    }

    /// Caller contract for `insert`: slotmap key freshness, and a parent (if
    /// any) that is live. Assumed, not enforced by the production API (see
    /// the module documentation).
    pub open spec fn insert_pre(self, token: TokenModel<T, F>, id: T) -> bool {
        &&& !self.issued.contains(id)
        &&& token.parent is Some ==> self.tokens.contains_key(token.parent->0)
    }

    /// Models crates/ferric-rules-core/src/token.rs:106 fn insert_sequence_match.
    pub open spec fn insert_sequence_match(self, token: TokenModel<T, F>, id: T) -> Self {
        let inserted = self.insert(token, id);
        StoreModel { sequence_matches: inserted.sequence_matches.insert(id), ..inserted }
    }

    /// Models crates/ferric-rules-core/src/token.rs:128 fn remove.
    pub open spec fn remove(self, id: T) -> (Self, Option<TokenModel<T, F>>) {
        if !self.tokens.contains_key(id) {
            // `self.tokens.remove(id)?`
            (self, None)
        } else {
            let token = self.tokens[id];
            let tokens = self.tokens.remove(id);
            let sequence_matches = if !(self.sequence_matches.len() == 0) {
                self.sequence_matches.remove(id)
            } else {
                self.sequence_matches
            };
            let fact_to_tokens = match token.fact {
                Some(fact_id) => remove_from_index(self.fact_to_tokens, fact_id, id),
                None => self.fact_to_tokens,
            };
            let parent_to_children1 = match token.parent {
                Some(parent_id) => remove_from_index(self.parent_to_children, parent_id, id),
                None => self.parent_to_children,
            };
            // `self.parent_to_children.remove(&id)` orphans the children.
            let parent_to_children = parent_to_children1.remove(id);
            (
                StoreModel {
                    tokens,
                    sequence_matches,
                    fact_to_tokens,
                    parent_to_children,
                    issued: self.issued,
                    rank: self.rank,
                    next_rank: self.next_rank,
                },
                Some(token),
            )
        }
    }

    /// One iteration of the `while let Some(id) = stack.pop()` loop in
    /// crates/ferric-rules-core/src/token.rs:168 fn remove_cascade.
    pub open spec fn cascade_step(
        self,
        stack: Seq<T>,
        removed: Seq<(T, TokenModel<T, F>)>,
        order: spec_fn(T, Set<T>) -> Seq<T>,
    ) -> (Self, Seq<T>, Seq<(T, TokenModel<T, F>)>)
        recommends
            stack.len() > 0,
    {
        let id = stack.last();
        let popped = stack.drop_last();
        // Collect children before removing the token.
        let stack2 = if self.parent_to_children.contains_key(id) {
            popped + order(id, self.parent_to_children[id])
        } else {
            popped
        };
        let (next, token) = self.remove(id);
        let removed2 = match token {
            Some(token) => removed.push((id, token)),
            None => removed,
        };
        (next, stack2, removed2)
    }

    /// The whole loop; defined (and terminating) for well-formed stores.
    pub open spec fn cascade_loop(
        self,
        stack: Seq<T>,
        removed: Seq<(T, TokenModel<T, F>)>,
        order: spec_fn(T, Set<T>) -> Seq<T>,
    ) -> (Self, Seq<(T, TokenModel<T, F>)>)
        decreases self.tokens.len(), stack.len(),
        when self.wf()
    {
        if stack.len() == 0 {
            (self, removed)
        } else {
            let (next, stack2, removed2) = self.cascade_step(stack, removed, order);
            proof {
                let id = stack.last();
                if self.tokens.contains_key(id) {
                    assert(next.tokens.len() < self.tokens.len()) by {
                        assert(next.tokens.dom() =~= self.tokens.dom().remove(id));
                    }
                } else {
                    assert(!self.parent_to_children.contains_key(id));
                    assert(stack2.len() < stack.len());
                }
            }
            next.cascade_loop(stack2, removed2, order)
        }
    }

    /// Models crates/ferric-rules-core/src/token.rs:168 fn remove_cascade.
    pub open spec fn remove_cascade(self, root: T, order: spec_fn(T, Set<T>) -> Seq<T>) -> (
        Self,
        Seq<(T, TokenModel<T, F>)>,
    ) {
        self.cascade_loop(seq![root], Seq::empty(), order)
    }

    /// `x` contains every pre-state child of each of its members.
    pub open spec fn child_closed(self, x: Set<T>) -> bool {
        forall|t: T, c: T|
            #![trigger x.contains(t), self.tokens[c]]
            x.contains(t) && self.tokens.contains_key(c) && self.tokens[c].parent == Some(t) ==> x.contains(c)
    }

    // -----------------------------------------------------------------------
    // Proofs
    // -----------------------------------------------------------------------
    pub proof fn lemma_wf_implies_validate(self)
        requires
            self.wf(),
        ensures
            self.validate(),
    {
    }

    pub proof fn lemma_insert(self, token: TokenModel<T, F>, id: T)
        requires
            self.wf(),
            self.insert_pre(token, id),
        ensures
            self.insert_debug_asserts(token, id),
            self.insert(token, id).wf(),
            self.insert(token, id).tokens == self.tokens.insert(id, token),
            token.fact is Some ==> self.insert_sequence_match(token, id).wf(),
    {
        let next = self.insert(token, id);
        assert(!self.tokens.contains_key(id));
        assert forall|t: T| #[trigger]
            next.tokens.contains_key(t) && next.tokens[t].parent is Some && next.tokens.contains_key(next.tokens[t].parent->0)
                implies {
            &&& next.parent_to_children.contains_key(next.tokens[t].parent->0)
            &&& next.parent_to_children[next.tokens[t].parent->0].contains(t)
        } by {
            if t != id {
                assert(self.tokens.contains_key(t));
                let p = self.tokens[t].parent->0;
                assert(self.issued.contains(p));
                assert(p != id);
                assert(self.tokens.contains_key(p));
            }
        }
        assert forall|t: T| #[trigger]
            next.tokens.contains_key(t) && next.tokens[t].parent is Some implies {
            &&& next.issued.contains(next.tokens[t].parent->0)
            &&& next.rank[next.tokens[t].parent->0] < next.rank[t]
        } by {
            if t != id {
                assert(self.tokens.contains_key(t));
                let p = self.tokens[t].parent->0;
                assert(self.issued.contains(p));
                assert(p != id);
            } else {
                let p = token.parent->0;
                assert(self.issued.contains(p));
                assert(p != id);
            }
        }
        assert forall|f: F, t: T|
            #[trigger] next.fact_to_tokens.contains_key(f) && #[trigger] next.fact_to_tokens[f].contains(
                t,
            ) implies next.tokens[t].fact == Some(f) && next.tokens.contains_key(t) by {
            if t != id {
                assert(self.fact_to_tokens.contains_key(f) && self.fact_to_tokens[f].contains(t));
            }
        }
        assert forall|p: T, c: T|
            #[trigger] next.parent_to_children.contains_key(p)
                && #[trigger] next.parent_to_children[p].contains(c) implies next.tokens[c].parent
            == Some(p) && next.tokens.contains_key(c) by {
            if c != id {
                assert(self.parent_to_children.contains_key(p)
                    && self.parent_to_children[p].contains(c));
            }
        }
        assert forall|f: F| #[trigger]
            next.fact_to_tokens.contains_key(f) implies next.fact_to_tokens[f].len() > 0 by {
            if token.fact == Some(f) {
                assert(next.fact_to_tokens[f].contains(id));
            }
        }
        assert forall|p: T| #[trigger]
            next.parent_to_children.contains_key(p) implies next.parent_to_children[p].len() > 0 by {
            if token.parent == Some(p) {
                assert(next.parent_to_children[p].contains(id));
            }
        }
        assert forall|p: T| #[trigger] next.parent_to_children.contains_key(p) implies next.tokens.contains_key(
            p,
        ) by {}
        assert(next.wf());
        if token.fact is Some {
            let seq_next = self.insert_sequence_match(token, id);
            assert(seq_next.wf());
        }
    }

    pub proof fn lemma_remove(self, id: T)
        requires
            self.wf(),
        ensures
            self.remove(id).0.wf(),
            self.remove(id).1 == (if self.tokens.contains_key(id) {
                Some(self.tokens[id])
            } else {
                None
            }),
            self.remove(id).0.tokens == self.tokens.remove(id),
            self.remove(id).0.issued == self.issued,
            self.remove(id).0.rank == self.rank,
            self.remove(id).0.next_rank == self.next_rank,
    {
        if !self.tokens.contains_key(id) {
            assert(self.tokens.remove(id) =~= self.tokens);
            return ;
        }
        let token = self.tokens[id];
        let next = self.remove(id).0;
        // `id` never appears in an index entry other than its own.
        assert forall|f: F| #[trigger]
            next.fact_to_tokens.contains_key(f) implies !next.fact_to_tokens[f].contains(id)
            && next.fact_to_tokens[f].len() > 0 && self.fact_to_tokens.contains_key(f) && (
        next.fact_to_tokens[f] == (if token.fact == Some(f) {
            self.fact_to_tokens[f].remove(id)
        } else {
            self.fact_to_tokens[f]
        })) by {
            if self.fact_to_tokens[f].contains(id) {
                assert(token.fact == Some(f));
            }
        }
        assert forall|p: T| #[trigger]
            next.parent_to_children.contains_key(p) implies !next.parent_to_children[p].contains(id)
            && next.parent_to_children[p].len() > 0 && self.parent_to_children.contains_key(p)
            && p != id && (next.parent_to_children[p] == (if token.parent == Some(p) {
            self.parent_to_children[p].remove(id)
        } else {
            self.parent_to_children[p]
        })) by {
            if self.parent_to_children[p].contains(id) {
                assert(token.parent == Some(p));
            }
        }
        assert forall|f: F, t: T|
            #[trigger] next.fact_to_tokens.contains_key(f) && #[trigger] next.fact_to_tokens[f].contains(
                t,
            ) implies next.tokens[t].fact == Some(f) && next.tokens.contains_key(t) by {
            assert(self.fact_to_tokens[f].contains(t));
        }
        assert forall|p: T, c: T|
            #[trigger] next.parent_to_children.contains_key(p)
                && #[trigger] next.parent_to_children[p].contains(c) implies next.tokens[c].parent
            == Some(p) && next.tokens.contains_key(c) && next.tokens.contains_key(p) by {
            assert(self.parent_to_children[p].contains(c));
        }
        assert forall|t: T| #[trigger]
            next.tokens.contains_key(t) && next.tokens[t].fact is Some implies {
            &&& next.fact_to_tokens.contains_key(next.tokens[t].fact->0)
            &&& next.fact_to_tokens[next.tokens[t].fact->0].contains(t)
        } by {
            assert(self.tokens.contains_key(t));
            let f = self.tokens[t].fact->0;
            assert(self.fact_to_tokens[f].contains(t));
            if token.fact == Some(f) {
                assert(self.fact_to_tokens[f].remove(id).contains(t));
            }
        }
        assert forall|t: T| #[trigger]
            next.tokens.contains_key(t) && next.tokens[t].parent is Some && next.tokens.contains_key(next.tokens[t].parent->0)
                implies {
            &&& next.parent_to_children.contains_key(next.tokens[t].parent->0)
            &&& next.parent_to_children[next.tokens[t].parent->0].contains(t)
        } by {
            assert(self.tokens.contains_key(t));
            let p = self.tokens[t].parent->0;
            assert(self.tokens.contains_key(p));
            assert(self.parent_to_children[p].contains(t));
            if token.parent == Some(p) {
                assert(self.parent_to_children[p].remove(id).contains(t));
            }
        }
        assert forall|t: T| #[trigger]
            next.sequence_matches.contains(t) implies next.tokens.contains_key(t) && next.tokens[t].fact is Some by {
            assert(self.sequence_matches.contains(t));
        }
        assert(next.tokens =~= self.tokens.remove(id));
        assert(next.wf());
    }

    /// Loop invariant of `remove_cascade` relative to the pre-state `s0`.
    pub open spec fn cascade_inv(
        self,
        s0: Self,
        root: T,
        stack: Seq<T>,
        removed: Seq<(T, TokenModel<T, F>)>,
    ) -> bool {
        let r = removed.map_values(|e: (T, TokenModel<T, F>)| e.0);
        &&& self.wf()
        &&& self.issued == s0.issued
        &&& self.rank == s0.rank
        &&& self.next_rank == s0.next_rank
        &&& r.no_duplicates()
        // removed entries are pre-state tokens paired with their values
        &&& forall|i: int|
            0 <= i < removed.len() ==> s0.tokens.contains_key(#[trigger] removed[i].0) && removed[i].1
                == s0.tokens[removed[i].0]
        // the current tokens are the pre-state tokens minus the removed ones
        &&& forall|t: T| #[trigger] self.tokens.contains_key(t) <==> s0.tokens.contains_key(t) && !r.contains(t)
        &&& forall|t: T| #[trigger] self.tokens.contains_key(t) ==> self.tokens[t] == s0.tokens[t]
        // stack members are pre-state tokens whose parent was removed (or root)
        &&& forall|i: int|
            0 <= i < stack.len() ==> s0.tokens.contains_key(#[trigger] stack[i]) && (stack[i] == root || (
            s0.tokens[stack[i]].parent is Some && r.contains(s0.tokens[stack[i]].parent->0)))
        // every removed token other than root has its parent removed
        &&& forall|i: int|
            0 <= i < r.len() ==> #[trigger] r[i] == root || (s0.tokens[r[i]].parent is Some
                && r.contains(s0.tokens[r[i]].parent->0))
        // children of removed tokens are removed or pending on the stack
        &&& forall|t: T, c: T|
            #![trigger r.contains(t), s0.tokens[c]]
            r.contains(t) && s0.tokens.contains_key(c) && s0.tokens[c].parent == Some(t) ==> r.contains(c)
                || stack.contains(c)
        // the root is removed, or this is the first iteration
        &&& r.contains(root) || (removed.len() == 0 && stack == seq![root])
    }

    pub proof fn lemma_cascade_step(
        self,
        s0: Self,
        root: T,
        stack: Seq<T>,
        removed: Seq<(T, TokenModel<T, F>)>,
        order: spec_fn(T, Set<T>) -> Seq<T>,
    )
        requires
            enumerates(order),
            self.cascade_inv(s0, root, stack, removed),
            stack.len() > 0,
        ensures
            ({
                let (next, stack2, removed2) = self.cascade_step(stack, removed, order);
                next.cascade_inv(s0, root, stack2, removed2)
            }),
    {
        let id = stack.last();
        let popped = stack.drop_last();
        let r = removed.map_values(|e: (T, TokenModel<T, F>)| e.0);
        let (next, stack2, removed2) = self.cascade_step(stack, removed, order);
        let r2 = removed2.map_values(|e: (T, TokenModel<T, F>)| e.0);
        self.lemma_remove(id);
        assert(s0.tokens.contains_key(stack[stack.len() - 1]));
        assert forall|c: T| stack.contains(c) && c != id implies popped.contains(c) by {
            let i = choose|i: int| 0 <= i < stack.len() && stack[i] == c;
            assert(popped[i] == c);
        }
        if self.tokens.contains_key(id) {
            assert(!r.contains(id));
            assert(r2 =~= r.push(id));
            assert(r2.no_duplicates()) by {
                assert forall|i: int, j: int|
                    0 <= i < r2.len() && 0 <= j < r2.len() && i != j implies r2[i] != r2[j] by {
                    if i == r.len() {
                        assert(r[j] == r2[j]);
                    } else if j == r.len() {
                        assert(r[i] == r2[i]);
                    }
                }
            }
            assert forall|t: T| r.contains(t) implies r2.contains(t) by {
                let i = choose|i: int| 0 <= i < r.len() && r[i] == t;
                assert(r2[i] == t);
            }
            assert(r2.contains(id)) by {
                assert(r2[r.len() as int] == id);
            }
            let pushed = if self.parent_to_children.contains_key(id) {
                order(id, self.parent_to_children[id])
            } else {
                Seq::<T>::empty()
            };
            assert(stack2 =~= popped + pushed);
            // Children pushed now are live tokens whose parent is `id`.
            assert forall|c: T| #[trigger]
                pushed.contains(c) implies s0.tokens.contains_key(c) && s0.tokens[c].parent == Some(
                id,
            ) by {
                if self.parent_to_children.contains_key(id) {
                    assert(self.parent_to_children[id].contains(c));
                    assert(self.tokens.contains_key(c));
                }
            }
            assert forall|i: int| 0 <= i < stack2.len() implies s0.tokens.contains_key(
                #[trigger] stack2[i],
            ) && (stack2[i] == root || (s0.tokens[stack2[i]].parent is Some && r2.contains(
                s0.tokens[stack2[i]].parent->0,
            ))) by {
                if i < popped.len() {
                    assert(stack2[i] == stack[i]);
                } else {
                    let c = stack2[i];
                    assert(pushed.contains(c)) by {
                        assert(pushed[i - popped.len()] == c);
                    }
                }
            }
            assert forall|i: int| 0 <= i < r2.len() implies #[trigger] r2[i] == root || (
            s0.tokens[r2[i]].parent is Some && r2.contains(s0.tokens[r2[i]].parent->0)) by {
                if i < r.len() {
                    assert(r2[i] == r[i]);
                } else {
                    assert(r2[i] == id);
                    assert(stack[stack.len() - 1] == id);
                }
            }
            assert forall|t: T, c: T|
                #![trigger r2.contains(t), s0.tokens[c]]
                r2.contains(t) && s0.tokens.contains_key(c) && s0.tokens[c].parent == Some(t)
                    implies r2.contains(c) || stack2.contains(c) by {
                if t == id {
                    if !r.contains(c) {
                        assert(self.tokens.contains_key(c));
                        assert(self.tokens[c].parent == Some(id));
                        assert(self.parent_to_children.contains_key(id));
                        assert(self.parent_to_children[id].contains(c));
                        assert(pushed.contains(c));
                        let k = choose|k: int| 0 <= k < pushed.len() && #[trigger] pushed[k] == c;
                        assert(stack2[popped.len() + k] == c);
                    }
                } else {
                    assert(r.contains(t)) by {
                        let i = choose|i: int| 0 <= i < r2.len() && r2[i] == t;
                        assert(i < r.len());
                        assert(r[i] == t);
                    }
                    if stack.contains(c) && c != id {
                        assert(popped.contains(c));
                        let k = choose|k: int| 0 <= k < popped.len() && popped[k] == c;
                        assert(stack2[k] == c);
                    }
                }
            }
            assert forall|i: int| 0 <= i < removed2.len() implies s0.tokens.contains_key(
                #[trigger] removed2[i].0,
            ) && removed2[i].1 == s0.tokens[removed2[i].0] by {
                if i < removed.len() {
                    assert(removed2[i] == removed[i]);
                }
            }
            assert forall|t: T| #[trigger]
                next.tokens.contains_key(t) <==> s0.tokens.contains_key(t) && !r2.contains(t) by {
                if r2.contains(t) && !r.contains(t) {
                    assert(t == id) by {
                        let i = choose|i: int| 0 <= i < r2.len() && r2[i] == t;
                        if i < r.len() {
                            assert(r[i] == t);
                        }
                    }
                }
            }
        } else {
            assert(r.contains(id));
            assert(!self.parent_to_children.contains_key(id));
            assert(stack2 =~= popped);
            assert(removed2 == removed);
            assert forall|t: T, c: T|
                #![trigger r2.contains(t), s0.tokens[c]]
                r2.contains(t) && s0.tokens.contains_key(c) && s0.tokens[c].parent == Some(t)
                    implies r2.contains(c) || stack2.contains(c) by {
                if stack.contains(c) && c != id {
                    assert(popped.contains(c));
                }
            }
            assert forall|i: int| 0 <= i < stack2.len() implies s0.tokens.contains_key(
                #[trigger] stack2[i],
            ) && (stack2[i] == root || (s0.tokens[stack2[i]].parent is Some && r2.contains(
                s0.tokens[stack2[i]].parent->0,
            ))) by {
                assert(stack2[i] == stack[i]);
            }
            if !r.contains(root) {
                assert(stack[0] == root);
            }
        }
    }

    pub proof fn lemma_cascade_loop(
        self,
        s0: Self,
        root: T,
        stack: Seq<T>,
        removed: Seq<(T, TokenModel<T, F>)>,
        order: spec_fn(T, Set<T>) -> Seq<T>,
    )
        requires
            enumerates(order),
            self.cascade_inv(s0, root, stack, removed),
        ensures
            ({
                let (fin, rem) = self.cascade_loop(stack, removed, order);
                fin.cascade_inv(s0, root, Seq::empty(), rem)
            }),
        decreases self.tokens.len(), stack.len(),
    {
        if stack.len() > 0 {
            self.lemma_cascade_step(s0, root, stack, removed, order);
            let (next, stack2, removed2) = self.cascade_step(stack, removed, order);
            let id = stack.last();
            if self.tokens.contains_key(id) {
                assert(next.tokens.dom() =~= self.tokens.dom().remove(id));
            } else {
                assert(!self.parent_to_children.contains_key(id));
            }
            next.lemma_cascade_loop(s0, root, stack2, removed2, order);
        } else {
            assert(stack =~= Seq::<T>::empty());
        }
    }

    /// Minimality: every child-closed set containing the stack and the
    /// removed tokens contains every token the loop removes.
    pub proof fn lemma_cascade_least(
        self,
        s0: Self,
        root: T,
        stack: Seq<T>,
        removed: Seq<(T, TokenModel<T, F>)>,
        order: spec_fn(T, Set<T>) -> Seq<T>,
        x: Set<T>,
    )
        requires
            enumerates(order),
            self.cascade_inv(s0, root, stack, removed),
            s0.child_closed(x),
            forall|i: int| 0 <= i < stack.len() ==> x.contains(#[trigger] stack[i]),
            forall|i: int| 0 <= i < removed.len() ==> x.contains(#[trigger] removed[i].0),
        ensures
            ({
                let (fin, rem) = self.cascade_loop(stack, removed, order);
                forall|i: int| 0 <= i < rem.len() ==> x.contains(#[trigger] rem[i].0)
            }),
        decreases self.tokens.len(), stack.len(),
    {
        if stack.len() > 0 {
            self.lemma_cascade_step(s0, root, stack, removed, order);
            let (next, stack2, removed2) = self.cascade_step(stack, removed, order);
            let id = stack.last();
            let popped = stack.drop_last();
            assert(x.contains(stack[stack.len() - 1]));
            if self.tokens.contains_key(id) {
                assert(next.tokens.dom() =~= self.tokens.dom().remove(id));
                let pushed = if self.parent_to_children.contains_key(id) {
                    order(id, self.parent_to_children[id])
                } else {
                    Seq::<T>::empty()
                };
                assert(stack2 =~= popped + pushed);
                assert forall|i: int| 0 <= i < stack2.len() implies x.contains(
                    #[trigger] stack2[i],
                ) by {
                    if i < popped.len() {
                        assert(stack2[i] == stack[i]);
                    } else {
                        let c = stack2[i];
                        assert(pushed[i - popped.len()] == c);
                        assert(pushed.contains(c));
                        assert(self.parent_to_children.contains_key(id));
                        assert(self.parent_to_children[id].contains(c));
                        assert(self.tokens.contains_key(c));
                        assert(s0.tokens[c].parent == Some(id));
                    }
                }
                assert forall|i: int| 0 <= i < removed2.len() implies x.contains(
                    #[trigger] removed2[i].0,
                ) by {
                    if i < removed.len() {
                        assert(removed2[i] == removed[i]);
                    }
                }
            } else {
                assert(!self.parent_to_children.contains_key(id));
                assert(removed2 == removed);
                assert forall|i: int| 0 <= i < stack2.len() implies x.contains(
                    #[trigger] stack2[i],
                ) by {
                    assert(stack2[i] == stack[i]);
                }
            }
            next.lemma_cascade_least(s0, root, stack2, removed2, order, x);
            assert(self.cascade_loop(stack, removed, order) == next.cascade_loop(
                stack2,
                removed2,
                order,
            ));
        } else {
            assert(self.cascade_loop(stack, removed, order) == (self, removed));
        }
    }

    /// Main theorem for `remove_cascade`.
    pub proof fn lemma_remove_cascade(self, root: T, order: spec_fn(T, Set<T>) -> Seq<T>)
        requires
            self.wf(),
            self.tokens.contains_key(root),
            enumerates(order),
        ensures
            ({
                let (fin, removed) = self.remove_cascade(root, order);
                let r = removed.map_values(|e: (T, TokenModel<T, F>)| e.0);
                &&& fin.wf()
                // each removed token is reported once, with its pre-state value
                &&& r.no_duplicates()
                &&& forall|i: int|
                    0 <= i < removed.len() ==> self.tokens.contains_key(#[trigger] removed[i].0)
                        && removed[i].1 == self.tokens[removed[i].0]
                // exactly the reported tokens disappear; the rest are untouched
                &&& forall|t: T| #[trigger]
                    fin.tokens.contains_key(t) <==> self.tokens.contains_key(t) && !r.contains(t)
                &&& forall|t: T| #[trigger]
                    fin.tokens.contains_key(t) ==> fin.tokens[t] == self.tokens[t]
                // the reported set is the descendant closure of `root`
                &&& r.contains(root)
                &&& self.child_closed(r.to_set())
                &&& forall|i: int|
                    0 <= i < r.len() ==> #[trigger] r[i] == root || (self.tokens[r[i]].parent is Some
                        && r.contains(self.tokens[r[i]].parent->0))
                &&& forall|x: Set<T>|
                    x.contains(root) && #[trigger] self.child_closed(x) ==> r.to_set().subset_of(x)
            }),
    {
        let stack = seq![root];
        let removed = Seq::<(T, TokenModel<T, F>)>::empty();
        let r0 = removed.map_values(|e: (T, TokenModel<T, F>)| e.0);
        assert(r0 =~= Seq::<T>::empty());
        assert(self.cascade_inv(self, root, stack, removed));
        self.lemma_cascade_loop(self, root, stack, removed, order);
        let (fin, rem) = self.remove_cascade(root, order);
        let r = rem.map_values(|e: (T, TokenModel<T, F>)| e.0);
        assert(fin.cascade_inv(self, root, Seq::empty(), rem));
        assert forall|t: T, c: T|
            #![trigger r.to_set().contains(t), self.tokens[c]]
            r.to_set().contains(t) && self.tokens.contains_key(c) && self.tokens[c].parent == Some(
                t,
            ) implies r.to_set().contains(c) by {
            assert(r.contains(t));
            assert(!Seq::<T>::empty().contains(c));
        }
        assert forall|x: Set<T>|
            x.contains(root) && #[trigger] self.child_closed(x) implies r.to_set().subset_of(x) by {
            self.lemma_cascade_least(self, root, stack, removed, order, x);
            assert forall|t: T| #[trigger] r.to_set().contains(t) implies x.contains(t) by {
                let i = choose|i: int| 0 <= i < r.len() && r[i] == t;
                assert(rem[i].0 == t);
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Fact retraction: tokens_containing + retraction_roots + cascades
// ---------------------------------------------------------------------------
impl<T, F> StoreModel<T, F> {
    /// `t` reaches `target` by following live parent links (`t == target`
    /// counts). Ranks strictly decrease along live parent links.
    pub open spec fn reaches(self, t: T, target: T) -> bool
        decreases self.rank[t],
        when self.wf()
    {
        if !self.tokens.contains_key(t) {
            false
        } else if t == target {
            true
        } else {
            match self.tokens[t].parent {
                None => false,
                Some(p) => self.reaches(p, target),
            }
        }
    }

    /// The parent walk of crates/ferric-rules-core/src/token.rs:243
    /// fn retraction_roots: `true` when the walk meets an affected parent.
    pub open spec fn has_affected_ancestor(self, t: T, affected: Set<T>) -> bool
        decreases self.rank[t],
        when self.wf()
    {
        // `while let Some(token) = self.tokens.get(current)`
        if !self.tokens.contains_key(t) {
            false
        } else {
            match self.tokens[t].parent {
                None => false,
                Some(p) => if affected.contains(p) {
                    true
                } else {
                    self.has_affected_ancestor(p, affected)
                },
            }
        }
    }

    /// Models crates/ferric-rules-core/src/token.rs:223 fn tokens_containing.
    pub open spec fn tokens_containing(self, fact: F) -> Set<T> {
        if self.fact_to_tokens.contains_key(fact) {
            self.fact_to_tokens[fact]
        } else {
            Set::empty()
        }
    }

    /// Models the result set of crates/ferric-rules-core/src/token.rs:243
    /// fn retraction_roots (the production `Vec` lists it in hash-set order).
    pub open spec fn root_set(self, affected: Set<T>) -> Set<T> {
        affected.filter(|t: T| !self.has_affected_ancestor(t, affected))
    }

    /// The `for root_id in roots { remove_cascade(root_id) }` loop of
    /// crates/ferric-rules-core/src/rete.rs:384 (`ReteNetwork::retract_fact`).
    pub open spec fn cascade_all(self, roots: Seq<T>, order: spec_fn(T, Set<T>) -> Seq<T>) -> (
        Self,
        Seq<(T, TokenModel<T, F>)>,
    )
        decreases roots.len(),
    {
        if roots.len() == 0 {
            (self, Seq::empty())
        } else {
            let (next, first) = self.remove_cascade(roots[0], order);
            let (fin, rest) = next.cascade_all(roots.drop_first(), order);
            (fin, first + rest)
        }
    }

    /// Every root is live when its cascade starts (the `debug_assert!` in
    /// `remove_cascade`).
    pub open spec fn cascade_all_roots_live(
        self,
        roots: Seq<T>,
        order: spec_fn(T, Set<T>) -> Seq<T>,
    ) -> bool
        decreases roots.len(),
    {
        roots.len() > 0 ==> {
            &&& self.tokens.contains_key(roots[0])
            &&& self.remove_cascade(roots[0], order).0.cascade_all_roots_live(
                roots.drop_first(),
                order,
            )
        }
    }

    /// `other` is `self` with some tokens removed and nothing else changed
    /// that the walks read.
    pub open spec fn substore_of(self, other: Self) -> bool {
        &&& self.wf()
        &&& other.wf()
        &&& self.rank == other.rank
        &&& self.issued == other.issued
        &&& forall|t: T| #[trigger]
            self.tokens.contains_key(t) ==> other.tokens.contains_key(t) && self.tokens[t]
                == other.tokens[t]
    }

    pub proof fn lemma_reaches_rank(self, t: T, target: T)
        requires
            self.wf(),
            self.reaches(t, target),
        ensures
            self.tokens.contains_key(t),
            self.tokens.contains_key(target),
            t != target ==> self.rank[target] < self.rank[t],
        decreases self.rank[t],
    {
        if t != target {
            let p = self.tokens[t].parent->0;
            self.lemma_reaches_rank(p, target);
        }
    }

    pub proof fn lemma_reaches_trans(self, a: T, b: T, c: T)
        requires
            self.wf(),
            self.reaches(a, b),
            self.reaches(b, c),
        ensures
            self.reaches(a, c),
        decreases self.rank[a],
    {
        self.lemma_reaches_rank(b, c);
        if a != b {
            let p = self.tokens[a].parent->0;
            self.lemma_reaches_trans(p, b, c);
        }
    }

    /// Two ancestors of the same token are related.
    pub proof fn lemma_reaches_linear(self, m: T, a: T, b: T)
        requires
            self.wf(),
            self.reaches(m, a),
            self.reaches(m, b),
        ensures
            self.reaches(a, b) || self.reaches(b, a),
        decreases self.rank[m],
    {
        if m != a && m != b {
            let p = self.tokens[m].parent->0;
            self.lemma_reaches_linear(p, a, b);
        }
    }

    /// A strict affected ancestor is found by the root walk.
    pub proof fn lemma_strict_reach_is_affected_ancestor(self, x: T, y: T, affected: Set<T>)
        requires
            self.wf(),
            self.reaches(x, y),
            x != y,
            affected.contains(y),
        ensures
            self.has_affected_ancestor(x, affected),
        decreases self.rank[x],
    {
        let p = self.tokens[x].parent->0;
        if !affected.contains(p) {
            self.lemma_strict_reach_is_affected_ancestor(p, y, affected);
        }
    }

    /// The root walk only stops at an affected ancestor it reaches.
    pub proof fn lemma_affected_ancestor_reaches(self, t: T, affected: Set<T>)
        requires
            self.wf(),
            self.has_affected_ancestor(t, affected),
            forall|a: T| #[trigger] affected.contains(a) ==> self.tokens.contains_key(a),
        ensures
            exists|a: T| #[trigger]
                affected.contains(a) && a != t && self.reaches(t, a),
        decreases self.rank[t],
    {
        let p = self.tokens[t].parent->0;
        assert(self.rank[p] < self.rank[t]);
        if affected.contains(p) {
            assert(self.reaches(p, p));
            assert(self.reaches(t, p));
        } else {
            self.lemma_affected_ancestor_reaches(p, affected);
            let a = choose|a: T| #[trigger] affected.contains(a) && a != p && self.reaches(p, a);
            self.lemma_reaches_rank(p, a);
            assert(self.reaches(t, a));
        }
    }

    /// Every affected token reaches a root.
    pub proof fn lemma_reaches_some_root(self, t: T, affected: Set<T>)
        requires
            self.wf(),
            affected.contains(t),
            forall|a: T| #[trigger] affected.contains(a) ==> self.tokens.contains_key(a),
        ensures
            exists|r: T| #[trigger] self.root_set(affected).contains(r) && self.reaches(t, r),
        decreases self.rank[t],
    {
        if !self.has_affected_ancestor(t, affected) {
            assert(self.root_set(affected).contains(t));
            assert(self.reaches(t, t));
        } else {
            self.lemma_affected_ancestor_reaches(t, affected);
            let a = choose|a: T| #[trigger] affected.contains(a) && a != t && self.reaches(t, a);
            self.lemma_reaches_rank(t, a);
            self.lemma_reaches_some_root(a, affected);
            let r = choose|r: T| #[trigger] self.root_set(affected).contains(r) && self.reaches(a, r);
            self.lemma_reaches_trans(t, a, r);
        }
    }

    /// A child-closed set containing `target` contains everything reaching it.
    pub proof fn lemma_closed_contains_reaching(self, x: Set<T>, t: T, target: T)
        requires
            self.wf(),
            self.child_closed(x),
            x.contains(target),
            self.reaches(t, target),
        ensures
            x.contains(t),
        decreases self.rank[t],
    {
        if t != target {
            let p = self.tokens[t].parent->0;
            self.lemma_closed_contains_reaching(x, p, target);
        }
    }

    /// Walks in a substore are walks in the larger store.
    pub proof fn lemma_substore_reaches(self, other: Self, t: T, target: T)
        requires
            self.substore_of(other),
            self.reaches(t, target),
        ensures
            other.reaches(t, target),
        decreases self.rank[t],
    {
        if t != target {
            let p = self.tokens[t].parent->0;
            self.lemma_substore_reaches(other, p, target);
        }
    }

    /// A walk in the larger store that is not a walk in the substore passes
    /// through a token missing from the substore.
    pub proof fn lemma_substore_reaches_back(self, other: Self, t: T, target: T)
        requires
            self.substore_of(other),
            other.reaches(t, target),
            !self.reaches(t, target),
        ensures
            exists|m: T| #[trigger]
                other.reaches(t, m) && other.reaches(m, target) && !self.tokens.contains_key(m),
        decreases other.rank[t],
    {
        assert(other.reaches(target, target)) by {
            other.lemma_reaches_rank(t, target);
        }
        if !self.tokens.contains_key(t) {
            assert(other.reaches(t, t));
        } else {
            let p = other.tokens[t].parent->0;
            self.lemma_substore_reaches_back(other, p, target);
            let m = choose|m: T| #[trigger]
                other.reaches(p, m) && other.reaches(m, target) && !self.tokens.contains_key(m);
            other.lemma_reaches_rank(p, m);
            assert(other.reaches(t, m));
        }
    }

    /// `x` child-closed in a store stays child-closed in a substore.
    pub proof fn lemma_substore_closed(self, other: Self, x: Set<T>)
        requires
            self.substore_of(other),
            other.child_closed(x),
        ensures
            self.child_closed(x),
    {
        assert forall|t: T, c: T|
            #![trigger x.contains(t), self.tokens[c]]
            x.contains(t) && self.tokens.contains_key(c) && self.tokens[c].parent == Some(t)
                implies x.contains(c) by {
            assert(other.tokens.contains_key(c));
            assert(other.tokens[c] == self.tokens[c]);
        }
    }

    /// The removal performed by one cascade, restated for the retraction proof.
    pub proof fn lemma_cascade_facts(self, root: T, order: spec_fn(T, Set<T>) -> Seq<T>)
        requires
            self.wf(),
            self.tokens.contains_key(root),
            enumerates(order),
        ensures
            ({
                let (fin, removed) = self.remove_cascade(root, order);
                let r = removed.map_values(|e: (T, TokenModel<T, F>)| e.0);
                &&& fin.substore_of(self)
                &&& r.no_duplicates()
                &&& forall|i: int|
                    0 <= i < removed.len() ==> self.tokens.contains_key(#[trigger] removed[i].0)
                        && removed[i].1 == self.tokens[removed[i].0]
                &&& forall|t: T| #[trigger]
                    fin.tokens.contains_key(t) <==> self.tokens.contains_key(t) && !r.contains(t)
                // exactly the tokens reaching the root disappear
                &&& forall|t: T| #[trigger] r.contains(t) <==> self.reaches(t, root)
            }),
    {
        self.lemma_remove_cascade(root, order);
        let (fin, removed) = self.remove_cascade(root, order);
        let r = removed.map_values(|e: (T, TokenModel<T, F>)| e.0);
        assert(fin.rank == self.rank && fin.issued == self.issued) by {
            self.lemma_cascade_loop(self, root, seq![root], Seq::empty(), order);
            assert(Seq::<(T, TokenModel<T, F>)>::empty().map_values(
                |e: (T, TokenModel<T, F>)| e.0,
            ) =~= Seq::<T>::empty());
        }
        assert forall|t: T| #[trigger] r.contains(t) <==> self.reaches(t, root) by {
            if self.reaches(t, root) {
                self.lemma_closed_contains_reaching(r.to_set(), t, root);
            }
            if r.contains(t) {
                self.lemma_removed_reach_root(r, root, t);
            }
        }
    }

    /// Every token a cascade removes reaches its root.
    pub proof fn lemma_removed_reach_root(self, r: Seq<T>, root: T, t: T)
        requires
            self.wf(),
            r.contains(t),
            forall|x: T| #[trigger] r.contains(x) ==> self.tokens.contains_key(x),
            forall|i: int|
                0 <= i < r.len() ==> #[trigger] r[i] == root || (self.tokens[r[i]].parent is Some
                    && r.contains(self.tokens[r[i]].parent->0)),
        ensures
            self.reaches(t, root),
        decreases self.rank[t],
    {
        let i = choose|i: int| 0 <= i < r.len() && r[i] == t;
        assert(r[i] == t);
        if t != root {
            let p = self.tokens[t].parent->0;
            assert(self.rank[p] < self.rank[t]);
            self.lemma_removed_reach_root(r, root, p);
        }
    }

    /// Invariant of the retraction loop after cascading the roots in `done`,
    /// relative to the pre-state `s0`.
    pub open spec fn retract_inv(
        self,
        s0: Self,
        done: Seq<T>,
        removed: Seq<(T, TokenModel<T, F>)>,
    ) -> bool {
        let r = removed.map_values(|e: (T, TokenModel<T, F>)| e.0);
        &&& self.substore_of(s0)
        &&& r.no_duplicates()
        &&& forall|i: int|
            0 <= i < removed.len() ==> s0.tokens.contains_key(#[trigger] removed[i].0)
                && removed[i].1 == s0.tokens[removed[i].0]
        &&& forall|t: T| #[trigger]
            self.tokens.contains_key(t) <==> s0.tokens.contains_key(t) && !r.contains(t)
        // removed tokens reach a processed root, and everything reaching a
        // processed root is removed
        &&& forall|t: T| #[trigger]
            r.contains(t) <==> exists|j: int| 0 <= j < done.len() && s0.reaches(t, #[trigger] done[j])
    }

    /// The roots handed to the loop: distinct members of the root set.
    pub open spec fn roots_ok(self, affected: Set<T>, roots: Seq<T>) -> bool {
        &&& roots.no_duplicates()
        &&& forall|i: int| 0 <= i < roots.len() ==> self.root_set(affected).contains(#[trigger] roots[i])
        &&& forall|a: T| #[trigger] affected.contains(a) ==> self.tokens.contains_key(a)
    }

    pub proof fn lemma_cascade_all(
        self,
        s0: Self,
        affected: Set<T>,
        done: Seq<T>,
        rest: Seq<T>,
        removed: Seq<(T, TokenModel<T, F>)>,
        order: spec_fn(T, Set<T>) -> Seq<T>,
    )
        requires
            s0.wf(),
            enumerates(order),
            s0.roots_ok(affected, done + rest),
            self.retract_inv(s0, done, removed),
        ensures
            self.cascade_all_roots_live(rest, order),
            ({
                let (fin, rem) = self.cascade_all(rest, order);
                fin.retract_inv(s0, done + rest, removed + rem)
            }),
        decreases rest.len(),
    {
        let all = done + rest;
        if rest.len() == 0 {
            assert(all =~= done);
            assert(removed + Seq::<(T, TokenModel<T, F>)>::empty() =~= removed);
            return ;
        }
        let root = rest[0];
        let k = done.len() as int;
        assert(all[k] == root);
        let r = removed.map_values(|e: (T, TokenModel<T, F>)| e.0);
        // The root is still live: otherwise it would reach an earlier root.
        assert(s0.root_set(affected).contains(root));
        assert(affected.contains(root));
        assert(self.tokens.contains_key(root)) by {
            if r.contains(root) {
                let j = choose|j: int| 0 <= j < done.len() && s0.reaches(root, #[trigger] done[j]);
                let d = done[j];
                assert(all[j] == d);
                assert(d != root);
                assert(s0.root_set(affected).contains(d));
                s0.lemma_strict_reach_is_affected_ancestor(root, d, affected);
            }
        }
        self.lemma_cascade_facts(root, order);
        let (next, rem1) = self.remove_cascade(root, order);
        let r1 = rem1.map_values(|e: (T, TokenModel<T, F>)| e.0);
        let done2 = done.push(root);
        let removed2 = removed + rem1;
        let r2 = removed2.map_values(|e: (T, TokenModel<T, F>)| e.0);
        assert(r2 =~= r + r1);
        assert(rest.drop_first() =~= rest.subrange(1, rest.len() as int));
        assert(done2 + rest.drop_first() =~= all);
        // next is a substore of s0
        assert(next.substore_of(s0)) by {
            assert forall|t: T| #[trigger] next.tokens.contains_key(t) implies s0.tokens.contains_key(
                t,
            ) && next.tokens[t] == s0.tokens[t] by {
                assert(self.tokens.contains_key(t));
            }
        }
        assert(r2.no_duplicates()) by {
            assert forall|i: int, j: int|
                0 <= i < r2.len() && 0 <= j < r2.len() && i != j implies r2[i] != r2[j] by {
                if i < r.len() && j >= r.len() {
                    assert(r1.contains(r2[j])) by {
                        assert(r1[j - r.len()] == r2[j]);
                    }
                    assert(self.tokens.contains_key(r2[j]));
                    assert(r.contains(r2[i])) by {
                        assert(r[i] == r2[i]);
                    }
                } else if j < r.len() && i >= r.len() {
                    assert(r1.contains(r2[i])) by {
                        assert(r1[i - r.len()] == r2[i]);
                    }
                    assert(self.tokens.contains_key(r2[i]));
                    assert(r.contains(r2[j])) by {
                        assert(r[j] == r2[j]);
                    }
                } else if i < r.len() {
                    assert(r2[i] == r[i] && r2[j] == r[j]);
                } else {
                    assert(r2[i] == r1[i - r.len()] && r2[j] == r1[j - r.len()]);
                }
            }
        }
        assert forall|i: int|
            0 <= i < removed2.len() implies s0.tokens.contains_key(#[trigger] removed2[i].0)
            && removed2[i].1 == s0.tokens[removed2[i].0] by {
            if i < removed.len() {
                assert(removed2[i] == removed[i]);
            } else {
                assert(removed2[i] == rem1[i - removed.len()]);
            }
        }
        assert forall|t: T| #[trigger] r2.contains(t) <==> r.contains(t) || r1.contains(t) by {
            if r2.contains(t) {
                let i = choose|i: int| 0 <= i < r2.len() && r2[i] == t;
                if i < r.len() {
                    assert(r[i] == t);
                } else {
                    assert(r1[i - r.len()] == t);
                }
            }
            if r.contains(t) {
                let i = choose|i: int| 0 <= i < r.len() && r[i] == t;
                assert(r2[i] == t);
            }
            if r1.contains(t) {
                let i = choose|i: int| 0 <= i < r1.len() && r1[i] == t;
                assert(r2[r.len() + i] == t);
            }
        }
        assert forall|t: T| #[trigger]
            next.tokens.contains_key(t) <==> s0.tokens.contains_key(t) && !r2.contains(t) by {}
        assert forall|t: T| #[trigger]
            r2.contains(t) <==> exists|j: int|
                0 <= j < done2.len() && s0.reaches(t, #[trigger] done2[j]) by {
            if r1.contains(t) {
                assert(self.reaches(t, root));
                self.lemma_substore_reaches(s0, t, root);
                assert(done2[k] == root);
            }
            if r.contains(t) {
                let j = choose|j: int| 0 <= j < done.len() && s0.reaches(t, #[trigger] done[j]);
                assert(done2[j] == done[j]);
            }
            if exists|j: int| 0 <= j < done2.len() && s0.reaches(t, #[trigger] done2[j]) {
                let j = choose|j: int| 0 <= j < done2.len() && s0.reaches(t, #[trigger] done2[j]);
                if j < k {
                    assert(done2[j] == done[j]);
                    assert(r.contains(t));
                } else {
                    assert(done2[j] == root);
                    if !r.contains(t) && !self.reaches(t, root) {
                        // The walk from `t` would pass a token removed by an
                        // earlier root, making one root reach the other.
                        self.lemma_substore_reaches_back(s0, t, root);
                        let m = choose|m: T| #[trigger]
                            s0.reaches(t, m) && s0.reaches(m, root) && !self.tokens.contains_key(m);
                        s0.lemma_reaches_rank(t, m);
                        assert(r.contains(m));
                        let i = choose|i: int| 0 <= i < done.len() && s0.reaches(m, #[trigger] done[i]);
                        let d = done[i];
                        assert(all[i] == d);
                        assert(d != root);
                        s0.lemma_reaches_linear(m, d, root);
                        assert(s0.root_set(affected).contains(d));
                        if s0.reaches(d, root) {
                            s0.lemma_strict_reach_is_affected_ancestor(d, root, affected);
                        } else {
                            s0.lemma_strict_reach_is_affected_ancestor(root, d, affected);
                        }
                    }
                }
            }
        }
        assert(next.retract_inv(s0, done2, removed2));
        next.lemma_cascade_all(s0, affected, done2, rest.drop_first(), removed2, order);
        let (fin, rem2) = next.cascade_all(rest.drop_first(), order);
        assert(self.cascade_all(rest, order) == (fin, rem1 + rem2));
        assert(removed2 + rem2 =~= removed + (rem1 + rem2));
    }

    /// Fact retraction (`tokens_containing`, `retraction_roots`, one cascade
    /// per root, as in `ReteNetwork::retract_fact`): every cascade root is
    /// live when cascaded, and the removed tokens are exactly the live
    /// tokens whose live parent chain contains a token holding `fact`, each
    /// reported once with its pre-state value.
    pub proof fn lemma_retract_fact(self, fact: F, roots: Seq<T>, order: spec_fn(T, Set<T>) -> Seq<T>)
        requires
            self.wf(),
            enumerates(order),
            roots.no_duplicates(),
            roots.to_set() == self.root_set(self.tokens_containing(fact)),
        ensures
            self.cascade_all_roots_live(roots, order),
            ({
                let (fin, removed) = self.cascade_all(roots, order);
                let r = removed.map_values(|e: (T, TokenModel<T, F>)| e.0);
                &&& fin.wf()
                &&& r.no_duplicates()
                &&& forall|i: int|
                    0 <= i < removed.len() ==> self.tokens.contains_key(#[trigger] removed[i].0)
                        && removed[i].1 == self.tokens[removed[i].0]
                &&& forall|t: T| #[trigger]
                    fin.tokens.contains_key(t) <==> self.tokens.contains_key(t) && !r.contains(t)
                &&& forall|t: T| #[trigger]
                    r.contains(t) <==> exists|a: T| #[trigger]
                        self.tokens_containing(fact).contains(a) && self.reaches(t, a)
            }),
    {
        let affected = self.tokens_containing(fact);
        assert forall|a: T| #[trigger] affected.contains(a) implies self.tokens.contains_key(a)
            && self.tokens[a].fact == Some(fact) by {}
        assert forall|i: int| 0 <= i < roots.len() implies self.root_set(affected).contains(
            #[trigger] roots[i],
        ) by {
            assert(roots.to_set().contains(roots[i]));
        }
        let done = Seq::<T>::empty();
        let removed = Seq::<(T, TokenModel<T, F>)>::empty();
        assert(done + roots =~= roots);
        assert(removed.map_values(|e: (T, TokenModel<T, F>)| e.0) =~= Seq::<T>::empty());
        assert(self.retract_inv(self, done, removed));
        self.lemma_cascade_all(self, affected, done, roots, removed, order);
        let (fin, rem) = self.cascade_all(roots, order);
        assert(removed + rem =~= rem);
        let r = rem.map_values(|e: (T, TokenModel<T, F>)| e.0);
        assert forall|t: T| #[trigger]
            r.contains(t) <==> exists|a: T| #[trigger] affected.contains(a) && self.reaches(t, a) by {
            if r.contains(t) {
                let j = choose|j: int| 0 <= j < roots.len() && self.reaches(t, #[trigger] roots[j]);
                assert(self.root_set(affected).contains(roots[j]));
                assert(affected.contains(roots[j]));
            }
            if exists|a: T| #[trigger] affected.contains(a) && self.reaches(t, a) {
                let a = choose|a: T| #[trigger] affected.contains(a) && self.reaches(t, a);
                self.lemma_reaches_some_root(a, affected);
                let root = choose|root: T| #[trigger]
                    self.root_set(affected).contains(root) && self.reaches(a, root);
                self.lemma_reaches_trans(t, a, root);
                assert(roots.to_set().contains(root));
                let j = choose|j: int| 0 <= j < roots.len() && roots[j] == root;
                assert(self.reaches(t, roots[j]));
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Fact-list reconstruction: collect_all_facts
// ---------------------------------------------------------------------------
impl<T, F> StoreModel<T, F> {
    /// Termination measure of the parent walk in `collect_all_facts`:
    /// `None` < a dead id < a live id (ordered by rank).
    pub open spec fn walk_measure(self, current: Option<T>) -> nat {
        match current {
            None => 0,
            Some(id) => if self.tokens.contains_key(id) {
                self.rank[id] + 2
            } else {
                1
            },
        }
    }

    /// The first loop of crates/ferric-rules-core/src/token.rs:201
    /// fn collect_all_facts:
    /// `while let Some(id) = current { chain.push(id);
    ///   current = self.tokens.get(id).and_then(|t| t.parent); }`.
    pub open spec fn collect_chain(self, current: Option<T>, chain: Seq<T>) -> Seq<T>
        decreases self.walk_measure(current),
        when self.wf()
    {
        match current {
            None => chain,
            Some(id) => {
                let chain2 = chain.push(id);
                // `self.tokens.get(id).and_then(|t| t.parent)`
                let next = if self.tokens.contains_key(id) {
                    self.tokens[id].parent
                } else {
                    None
                };
                proof {
                    if self.tokens.contains_key(id) && next is Some && self.tokens.contains_key(
                        next->0,
                    ) {
                        assert(self.rank[next->0] < self.rank[id]);
                    }
                }
                self.collect_chain(next, chain2)
            },
        }
    }

    /// The second loop of crates/ferric-rules-core/src/token.rs:201
    /// fn collect_all_facts: `for &id in chain.iter().rev()`, pushing the fact
    /// of every live token that has one.
    pub open spec fn collect_facts_rev(self, chain: Seq<T>, facts: Seq<F>) -> Seq<F>
        decreases chain.len(),
    {
        if chain.len() == 0 {
            facts
        } else {
            let id = chain.last();
            // `if let Some(token) = self.tokens.get(id) { if let Some(fact_id) = token.fact`
            let facts2 = if self.tokens.contains_key(id) && self.tokens[id].fact is Some {
                facts.push(self.tokens[id].fact->0)
            } else {
                facts
            };
            self.collect_facts_rev(chain.drop_last(), facts2)
        }
    }

    /// Models crates/ferric-rules-core/src/token.rs:201 fn collect_all_facts.
    pub open spec fn collect_all_facts(self, token_id: T) -> Seq<F> {
        self.collect_facts_rev(self.collect_chain(Some(token_id), Seq::empty()), Seq::empty())
    }

    /// Specification (written independently of the two loops): the facts of
    /// the live tokens on the parent chain of `t`, root first, skipping
    /// pass-through tokens; the walk ends at the first missing token.
    pub open spec fn chain_facts(self, t: T) -> Seq<F>
        decreases self.rank[t],
        when self.wf()
    {
        if !self.tokens.contains_key(t) {
            Seq::empty()
        } else {
            let above = match self.tokens[t].parent {
                None => Seq::empty(),
                Some(p) => self.chain_facts(p),
            };
            match self.tokens[t].fact {
                None => above,
                Some(f) => above.push(f),
            }
        }
    }

    /// The ids the parent walk visits from `current`.
    pub open spec fn walk_ids(self, current: Option<T>) -> Seq<T>
        decreases self.walk_measure(current),
        when self.wf()
    {
        match current {
            None => Seq::empty(),
            Some(id) => {
                let next = if self.tokens.contains_key(id) {
                    self.tokens[id].parent
                } else {
                    None
                };
                proof {
                    if self.tokens.contains_key(id) && next is Some && self.tokens.contains_key(
                        next->0,
                    ) {
                        assert(self.rank[next->0] < self.rank[id]);
                    }
                }
                seq![id] + self.walk_ids(next)
            },
        }
    }

    /// The fact contributed by one chain entry.
    pub open spec fn fact_of(self, id: T) -> Seq<F> {
        if self.tokens.contains_key(id) && self.tokens[id].fact is Some {
            seq![self.tokens[id].fact->0]
        } else {
            Seq::empty()
        }
    }

    /// The facts of a chain, last entry first.
    pub open spec fn rev_facts(self, chain: Seq<T>) -> Seq<F>
        decreases chain.len(),
    {
        if chain.len() == 0 {
            Seq::empty()
        } else {
            self.fact_of(chain.last()) + self.rev_facts(chain.drop_last())
        }
    }

    pub proof fn lemma_collect_chain(self, current: Option<T>, chain: Seq<T>)
        requires
            self.wf(),
        ensures
            self.collect_chain(current, chain) == chain + self.walk_ids(current),
        decreases self.walk_measure(current),
    {
        match current {
            None => {
                assert(chain + Seq::<T>::empty() =~= chain);
            },
            Some(id) => {
                let next = if self.tokens.contains_key(id) {
                    self.tokens[id].parent
                } else {
                    None
                };
                if self.tokens.contains_key(id) && next is Some && self.tokens.contains_key(
                    next->0,
                ) {
                    assert(self.rank[next->0] < self.rank[id]);
                }
                self.lemma_collect_chain(next, chain.push(id));
                assert(chain.push(id) + self.walk_ids(next) =~= chain + (seq![id] + self.walk_ids(
                    next,
                )));
            },
        }
    }

    pub proof fn lemma_collect_facts_rev(self, chain: Seq<T>, facts: Seq<F>)
        ensures
            self.collect_facts_rev(chain, facts) == facts + self.rev_facts(chain),
        decreases chain.len(),
    {
        if chain.len() == 0 {
            assert(facts + Seq::<F>::empty() =~= facts);
        } else {
            let id = chain.last();
            let facts2 = if self.tokens.contains_key(id) && self.tokens[id].fact is Some {
                facts.push(self.tokens[id].fact->0)
            } else {
                facts
            };
            assert(facts2 =~= facts + self.fact_of(id));
            self.lemma_collect_facts_rev(chain.drop_last(), facts2);
            assert(facts2 + self.rev_facts(chain.drop_last()) =~= facts + (self.fact_of(id)
                + self.rev_facts(chain.drop_last())));
        }
    }

    /// Prepending an entry appends its fact.
    pub proof fn lemma_rev_facts_prepend(self, id: T, chain: Seq<T>)
        ensures
            self.rev_facts(seq![id] + chain) == self.rev_facts(chain) + self.fact_of(id),
        decreases chain.len(),
    {
        let whole = seq![id] + chain;
        assert(whole.len() > 0);
        assert(self.rev_facts(whole) == self.fact_of(whole.last()) + self.rev_facts(
            whole.drop_last(),
        ));
        if chain.len() == 0 {
            assert(chain =~= Seq::<T>::empty());
            assert(whole =~= seq![id]);
            assert(whole.drop_last() =~= Seq::<T>::empty());
            assert(self.rev_facts(Seq::<T>::empty()) == Seq::<F>::empty());
            assert(self.fact_of(id) + Seq::<F>::empty() =~= self.fact_of(id));
            assert(Seq::<F>::empty() + self.fact_of(id) =~= self.fact_of(id));
        } else {
            assert(whole.last() == chain.last());
            assert(whole.drop_last() =~= seq![id] + chain.drop_last());
            assert(self.rev_facts(chain) == self.fact_of(chain.last()) + self.rev_facts(
                chain.drop_last(),
            ));
            self.lemma_rev_facts_prepend(id, chain.drop_last());
            assert(self.fact_of(chain.last()) + (self.rev_facts(chain.drop_last()) + self.fact_of(
                id,
            )) =~= (self.fact_of(chain.last()) + self.rev_facts(chain.drop_last()))
                + self.fact_of(id));
        }
    }

    pub proof fn lemma_walk_facts(self, t: T)
        requires
            self.wf(),
        ensures
            self.rev_facts(self.walk_ids(Some(t))) == self.chain_facts(t),
        decreases self.rank[t],
    {
        let next = if self.tokens.contains_key(t) {
            self.tokens[t].parent
        } else {
            None
        };
        assert(self.walk_ids(Some(t)) == seq![t] + self.walk_ids(next));
        self.lemma_rev_facts_prepend(t, self.walk_ids(next));
        match next {
            None => {
                assert(self.walk_ids(next) =~= Seq::<T>::empty());
                assert(self.rev_facts(Seq::<T>::empty()) =~= Seq::<F>::empty());
                assert(Seq::<F>::empty() + self.fact_of(t) =~= self.fact_of(t));
                if self.tokens.contains_key(t) && self.tokens[t].fact is Some {
                    assert(self.chain_facts(t) =~= seq![self.tokens[t].fact->0]);
                }
            },
            Some(p) => {
                assert(self.tokens.contains_key(t));
                if self.tokens.contains_key(p) {
                    assert(self.rank[p] < self.rank[t]);
                    self.lemma_walk_facts(p);
                } else {
                    assert(self.walk_ids(Some(p)) == seq![p] + self.walk_ids(None));
                    assert(self.walk_ids(Some(p)) =~= seq![p]);
                    self.lemma_rev_facts_prepend(p, Seq::<T>::empty());
                    assert(seq![p] + Seq::<T>::empty() =~= seq![p]);
                    assert(self.rev_facts(Seq::<T>::empty()) =~= Seq::<F>::empty());
                    assert(self.fact_of(p) =~= Seq::<F>::empty());
                    assert(self.rev_facts(seq![p]) =~= Seq::<F>::empty());
                }
                match self.tokens[t].fact {
                    None => {
                        assert(self.fact_of(t) =~= Seq::<F>::empty());
                        assert(self.chain_facts(p) + Seq::<F>::empty() =~= self.chain_facts(p));
                    },
                    Some(f) => {
                        assert(self.chain_facts(p) + seq![f] =~= self.chain_facts(p).push(f));
                    },
                }
            },
        }
    }

    /// `collect_all_facts` returns exactly the facts of the live tokens on the
    /// parent chain of `token_id`, root first, skipping pass-through tokens,
    /// for every id (live or not) and every well-formed store, including
    /// stores with orphans left by non-cascading `remove`.
    pub proof fn lemma_collect_all_facts(self, token_id: T)
        requires
            self.wf(),
        ensures
            self.collect_all_facts(token_id) == self.chain_facts(token_id),
    {
        self.lemma_collect_chain(Some(token_id), Seq::empty());
        assert(Seq::<T>::empty() + self.walk_ids(Some(token_id)) =~= self.walk_ids(
            Some(token_id),
        ));
        self.lemma_collect_facts_rev(self.walk_ids(Some(token_id)), Seq::empty());
        assert(Seq::<F>::empty() + self.rev_facts(self.walk_ids(Some(token_id))) =~= self.rev_facts(
            self.walk_ids(Some(token_id)),
        ));
        self.lemma_walk_facts(token_id);
    }

    /// The facts returned are exactly those held by tokens that `token_id`
    /// reaches along live parent links (membership form of the theorem).
    pub proof fn lemma_chain_facts_members(self, t: T, f: F)
        requires
            self.wf(),
        ensures
            self.chain_facts(t).contains(f) <==> exists|a: T| #[trigger]
                self.reaches(t, a) && self.tokens[a].fact == Some(f),
        decreases self.rank[t],
    {
        if self.tokens.contains_key(t) {
            let above = match self.tokens[t].parent {
                None => Seq::<F>::empty(),
                Some(p) => self.chain_facts(p),
            };
            if let Some(p) = self.tokens[t].parent {
                if self.tokens.contains_key(p) {
                    assert(self.rank[p] < self.rank[t]);
                }
                self.lemma_chain_facts_members(p, f);
            }
            if self.chain_facts(t).contains(f) {
                if self.tokens[t].fact == Some(f) {
                    assert(self.reaches(t, t));
                } else {
                    let i = choose|i: int| 0 <= i < self.chain_facts(t).len() && self.chain_facts(
                        t,
                    )[i] == f;
                    if self.tokens[t].fact is Some {
                        assert(i < above.len()) by {
                            if i == above.len() as int {
                                assert(self.chain_facts(t)[i] == self.tokens[t].fact->0);
                            }
                        }
                    }
                    assert(above[i] == f);
                    assert(above.contains(f));
                    let p = self.tokens[t].parent->0;
                    let a = choose|a: T| #[trigger] self.reaches(p, a) && self.tokens[a].fact == Some(
                        f,
                    );
                    assert(self.reaches(t, a)) by {
                        self.lemma_reaches_rank(p, a);
                    }
                }
            }
            if exists|a: T| #[trigger] self.reaches(t, a) && self.tokens[a].fact == Some(f) {
                let a = choose|a: T| #[trigger] self.reaches(t, a) && self.tokens[a].fact == Some(f);
                if a == t {
                    assert(self.chain_facts(t) == above.push(f));
                    assert(self.chain_facts(t)[above.len() as int] == f);
                } else {
                    let p = self.tokens[t].parent->0;
                    assert(self.reaches(p, a));
                    assert(above.contains(f));
                    let i = choose|i: int| 0 <= i < above.len() && above[i] == f;
                    match self.tokens[t].fact {
                        None => {},
                        Some(g) => {
                            assert(self.chain_facts(t)[i] == f);
                        },
                    }
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// retraction_roots: the loop itself
// ---------------------------------------------------------------------------
impl<T, F> StoreModel<T, F> {
    /// The outer loop of crates/ferric-rules-core/src/token.rs:243
    /// fn retraction_roots, `for &token_id in affected`, visiting the affected
    /// set in the hash-set order `affected_order`; the inner parent walk is
    /// `has_affected_ancestor` (`is_root` is its negation).
    pub open spec fn retraction_roots_loop(
        self,
        affected: Set<T>,
        affected_order: Seq<T>,
        roots: Seq<T>,
    ) -> Seq<T>
        decreases affected_order.len(),
    {
        if affected_order.len() == 0 {
            roots
        } else {
            let token_id = affected_order[0];
            let is_root = !self.has_affected_ancestor(token_id, affected);
            let roots2 = if is_root {
                roots.push(token_id)
            } else {
                roots
            };
            self.retraction_roots_loop(affected, affected_order.drop_first(), roots2)
        }
    }

    /// Models crates/ferric-rules-core/src/token.rs:243 fn retraction_roots.
    pub open spec fn retraction_roots(self, affected: Set<T>, affected_order: Seq<T>) -> Seq<T> {
        self.retraction_roots_loop(affected, affected_order, Seq::empty())
    }

    pub proof fn lemma_retraction_roots_loop(
        self,
        affected: Set<T>,
        done: Seq<T>,
        affected_order: Seq<T>,
        roots: Seq<T>,
    )
        requires
            self.wf(),
            (done + affected_order).no_duplicates(),
            roots.no_duplicates(),
            forall|x: T| #[trigger]
                roots.contains(x) <==> done.contains(x) && !self.has_affected_ancestor(
                    x,
                    affected,
                ),
        ensures
            ({
                let result = self.retraction_roots_loop(affected, affected_order, roots);
                &&& result.no_duplicates()
                &&& forall|x: T| #[trigger]
                    result.contains(x) <==> (done + affected_order).contains(x)
                        && !self.has_affected_ancestor(x, affected)
            }),
        decreases affected_order.len(),
    {
        if affected_order.len() == 0 {
            assert(done + affected_order =~= done);
        } else {
            let token_id = affected_order[0];
            let roots2 = if !self.has_affected_ancestor(token_id, affected) {
                roots.push(token_id)
            } else {
                roots
            };
            let done2 = done.push(token_id);
            let rest = affected_order.drop_first();
            assert(done2 + rest =~= done + affected_order);
            assert(!done.contains(token_id)) by {
                if done.contains(token_id) {
                    let i = choose|i: int| 0 <= i < done.len() && done[i] == token_id;
                    assert((done + affected_order)[i] == (done + affected_order)[done.len() as int]);
                }
            }
            assert(roots2.no_duplicates()) by {
                if !self.has_affected_ancestor(token_id, affected) {
                    assert(!roots.contains(token_id));
                    assert forall|i: int, j: int|
                        0 <= i < roots2.len() && 0 <= j < roots2.len() && i != j implies roots2[i]
                        != roots2[j] by {
                        if i == roots.len() && roots2[j] == token_id {
                            assert(roots[j] == token_id);
                        }
                        if j == roots.len() && roots2[i] == token_id {
                            assert(roots[i] == token_id);
                        }
                    }
                }
            }
            assert forall|x: T| #[trigger]
                roots2.contains(x) <==> done2.contains(x) && !self.has_affected_ancestor(
                    x,
                    affected,
                ) by {
                if roots2.contains(x) && x != token_id {
                    let i = choose|i: int| 0 <= i < roots2.len() && roots2[i] == x;
                    assert(roots[i] == x);
                }
                if roots.contains(x) {
                    let i = choose|i: int| 0 <= i < roots.len() && roots[i] == x;
                    assert(roots2[i] == x);
                }
                if done2.contains(x) && x != token_id {
                    let i = choose|i: int| 0 <= i < done2.len() && done2[i] == x;
                    assert(done[i] == x);
                }
                if done.contains(x) {
                    let i = choose|i: int| 0 <= i < done.len() && done[i] == x;
                    assert(done2[i] == x);
                }
                if x == token_id {
                    assert(done2[done.len() as int] == x);
                    if !self.has_affected_ancestor(token_id, affected) {
                        assert(roots2[roots.len() as int] == x);
                    }
                }
            }
            self.lemma_retraction_roots_loop(affected, done2, rest, roots2);
        }
    }

    /// `retraction_roots`, for any duplicate-free enumeration of the affected
    /// set, returns a duplicate-free enumeration of `root_set(affected)`.
    pub proof fn lemma_retraction_roots(self, affected: Set<T>, affected_order: Seq<T>)
        requires
            self.wf(),
            affected_order.no_duplicates(),
            affected_order.to_set() == affected,
        ensures
            self.retraction_roots(affected, affected_order).no_duplicates(),
            self.retraction_roots(affected, affected_order).to_set() == self.root_set(affected),
    {
        let done = Seq::<T>::empty();
        assert(done + affected_order =~= affected_order);
        self.lemma_retraction_roots_loop(affected, done, affected_order, Seq::empty());
        let result = self.retraction_roots(affected, affected_order);
        assert(result.to_set() =~= self.root_set(affected)) by {
            assert forall|x: T| #[trigger] result.to_set().contains(x) <==> self.root_set(affected).contains(
                x,
            ) by {
                assert(affected_order.to_set().contains(x) <==> affected_order.contains(x));
            }
        }
    }

    /// `ReteNetwork::retract_fact` (crates/ferric-rules-core/src/rete.rs:377-386)
    /// end to end: `tokens_containing` collected into a set, `retraction_roots`
    /// over any hash-set order of it, then one `remove_cascade` per root.
    pub proof fn lemma_retract_fact_end_to_end(
        self,
        fact: F,
        affected_order: Seq<T>,
        order: spec_fn(T, Set<T>) -> Seq<T>,
    )
        requires
            self.wf(),
            enumerates(order),
            affected_order.no_duplicates(),
            affected_order.to_set() == self.tokens_containing(fact),
        ensures
            ({
                let roots = self.retraction_roots(self.tokens_containing(fact), affected_order);
                let (fin, removed) = self.cascade_all(roots, order);
                let r = removed.map_values(|e: (T, TokenModel<T, F>)| e.0);
                &&& self.cascade_all_roots_live(roots, order)
                &&& fin.wf()
                &&& r.no_duplicates()
                &&& forall|i: int|
                    0 <= i < removed.len() ==> self.tokens.contains_key(#[trigger] removed[i].0)
                        && removed[i].1 == self.tokens[removed[i].0]
                &&& forall|t: T| #[trigger]
                    fin.tokens.contains_key(t) <==> self.tokens.contains_key(t) && !r.contains(t)
                &&& forall|t: T| #[trigger]
                    r.contains(t) <==> exists|a: T| #[trigger]
                        self.tokens_containing(fact).contains(a) && self.reaches(t, a)
            }),
    {
        let affected = self.tokens_containing(fact);
        self.lemma_retraction_roots(affected, affected_order);
        let roots = self.retraction_roots(affected, affected_order);
        self.lemma_retract_fact(fact, roots, order);
    }
}

} // verus!
