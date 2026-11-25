//! Kani proof harnesses for `token` (see docs/verification.md).
//!
//! What is proved, on the production code:
//!
//! * `unlinked_token_lifecycle`: for tokens without facts or parents, and
//!   for both removal orders, `remove` returns the token exactly once (a
//!   second removal and `get` return `None`), the other token is untouched,
//!   a cascade from a childless token removes exactly that token, a reused
//!   slot yields an id distinct from both removed ids (stale ids stay dead),
//!   and both reverse indexes stay empty.
//! * `collect_all_facts_stops_at_dangling_parent`: for a single token
//!   whose parent was removed, and any fact or none, `collect_all_facts`
//!   returns exactly that token's fact (the walk stops at the dangling
//!   parent). The slot map is populated directly; this function reads
//!   nothing else. Chains of two or more tokens (symbolic or concrete
//!   shapes) ran out of memory at 7 GB, so the general chain walk is not
//!   covered.
//!
//! `validate_consistency` is not called by these harnesses: with symbolic
//! store contents its error-message formatting (`format!` with `Debug`)
//! made symbolic execution run out of memory. The invariant it checks is
//! implied by the model invariant `wf`
//! (verification/verus/core_stores_token_store.rs).
//!
//! Harness technique: tokens returned to the harness and the store itself
//! are `mem::forget`-ed at the end; their drop glue (through `BindingSet`
//! and `Value`) is not part of any property here.
//!
//! Known gaps: nothing here populates `fact_to_tokens` /
//! `parent_to_children`. Every populated hashbrown table makes symbolic
//! execution very expensive; even a single parent/child pair cascaded from
//! the parent was not attempted to completion (the comparable one-entry
//! fact-index harness ran for over eleven minutes). Invariant preservation
//! by `insert`,
//! `remove` and `remove_cascade` for arbitrary stores, "cascade removes
//! exactly the descendants", and "fact retraction (`tokens_containing` +
//! `retraction_roots` + cascades) removes exactly the tokens whose parent
//! chain holds the fact" are proved unboundedly on a model in
//! verification/verus/core_stores_token_store.rs. `retraction_roots`,
//! `sequence_matches` metadata and `clear` are not exercised by Kani.

use super::*;
use slotmap::KeyData;

fn token(fact: Option<FactId>, parent: Option<TokenId>) -> Token {
    Token {
        fact,
        bindings: BindingSet::new(),
        parent,
        owner_node: NodeId(0),
    }
}

fn fact_id(raw: u64) -> FactId {
    FactId::from(KeyData::from_ffi(raw))
}

/// One unlinked-token lifecycle for a concrete removal order.
fn unlinked_lifecycle(remove_first: bool) {
    let mut store = TokenStore::new();
    // Replace (without dropping, see the module documentation) the default
    // slot map with one that never reallocates in this harness.
    std::mem::forget(std::mem::replace(
        &mut store.tokens,
        SlotMap::with_capacity_and_key(4),
    ));
    let a = store.insert(token(None, None));
    let b = store.insert(token(None, None));
    assert_ne!(a, b);
    let (gone, kept) = if remove_first { (a, b) } else { (b, a) };
    let removed = store.remove(gone);
    assert!(removed.is_some());
    std::mem::forget(removed);
    let again = store.remove(gone);
    assert!(again.is_none());
    std::mem::forget(again);
    assert!(store.get(gone).is_none());
    assert!(store.get(kept).is_some());
    assert_eq!(store.len(), 1);
    let cascade = store.remove_cascade(kept);
    assert_eq!(cascade.len(), 1);
    assert_eq!(cascade[0].0, kept);
    std::mem::forget(cascade);
    assert!(store.is_empty());
    let c = store.insert(token(None, None));
    assert!(c != a && c != b);
    assert!(store.get(a).is_none() && store.get(b).is_none());
    assert_eq!(store.len(), 1);
    assert!(store.fact_to_tokens.is_empty() && store.parent_to_children.is_empty());
    std::mem::forget(store);
}

/// Tokens without facts or parents: exact, idempotent removal.
#[kani::proof]
#[kani::unwind(4)]
fn unlinked_token_lifecycle() {
    unlinked_lifecycle(true);
    unlinked_lifecycle(false);
}

fn any_fact_or_none() -> Option<FactId> {
    if kani::any() {
        Some(fact_id(1 + u64::from(kani::any::<bool>())))
    } else {
        None
    }
}

/// Check `collect_all_facts` on a chain of `len` tokens whose root has no
/// parent or (`dangling_end`) a removed parent; returns the number of facts.
fn check_chain(len: usize, dangling_end: bool) -> usize {
    let mut store = TokenStore::new();
    // Replace (without dropping, see the module documentation) the default
    // slot map with one that never reallocates in this harness.
    std::mem::forget(std::mem::replace(
        &mut store.tokens,
        SlotMap::with_capacity_and_key(4),
    ));
    let dangling = store.tokens.insert(token(None, None));
    std::mem::forget(store.tokens.remove(dangling));
    // facts[0] belongs to the root, facts[len - 1] to the leaf.
    let facts = [any_fact_or_none(), any_fact_or_none()];
    let mut parent = if dangling_end { Some(dangling) } else { None };
    let mut i = 0;
    while i < len {
        parent = Some(store.tokens.insert(token(facts[i], parent)));
        i += 1;
    }
    // Oracle: the facts present on the chain, root first.
    let mut expected: [Option<FactId>; 2] = [None; 2];
    let mut expected_len = 0;
    let mut j = 0;
    while j < len {
        if let Some(fact) = facts[j] {
            expected[expected_len] = Some(fact);
            expected_len += 1;
        }
        j += 1;
    }
    let actual = store.collect_all_facts(parent.unwrap());
    assert_eq!(actual.len(), expected_len);
    let mut k = 0;
    while k < expected_len {
        assert_eq!(Some(actual[k]), expected[k]);
        k += 1;
    }
    std::mem::forget(store);
    expected_len
}

/// `collect_all_facts` on a single token whose parent is dangling (removed)
/// returns exactly that token's fact, if any. (Chains of two tokens ran out
/// of memory at 7 GB.)
#[kani::proof]
#[kani::unwind(3)]
fn collect_all_facts_stops_at_dangling_parent() {
    let facts = check_chain(1, true);
    kani::cover!(facts == 1);
    kani::cover!(facts == 0);
}
