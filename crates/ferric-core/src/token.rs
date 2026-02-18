//! Token storage with reverse indices for efficient retraction.
//!
//! Tokens represent partial matches through the beta network. They have stable
//! identities to support efficient retraction and cascading deletes.

use slotmap::SlotMap;
use smallvec::SmallVec;
use std::collections::{HashMap, HashSet};

use crate::binding::BindingSet;
use crate::fact::FactId;

/// Identifier for a node in the Rete network.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct NodeId(pub u32);

slotmap::new_key_type! {
    /// Unique identifier for a token within the token store.
    pub struct TokenId;
}

fn remove_from_vec_index<K>(
    index: &mut HashMap<K, SmallVec<[TokenId; 4]>>,
    key: K,
    token_id: TokenId,
) where
    K: Copy + Eq + std::hash::Hash,
{
    let mut remove_key = false;
    if let Some(entry) = index.get_mut(&key) {
        entry.retain(|tid| *tid != token_id);
        remove_key = entry.is_empty();
    }

    if remove_key {
        index.remove(&key);
    }
}

/// A token representing a partial match through the beta network.
///
/// Tokens contain the facts matched so far, variable bindings from the match,
/// a reference to the parent token (for join nodes), and the network node that
/// owns this token.
#[derive(Clone, Debug)]
pub struct Token {
    pub facts: SmallVec<[FactId; 4]>,
    pub bindings: BindingSet,
    pub parent: Option<TokenId>,
    pub owner_node: NodeId,
}

/// Token storage with reverse indices for efficient retraction.
///
/// Maintains two reverse indices:
/// - `fact_to_tokens`: Maps `FactId` to all tokens that reference it
/// - `parent_to_children`: Maps parent `TokenId` to all its children
///
/// These indices enable efficient cascading deletion when facts are retracted.
pub struct TokenStore {
    tokens: SlotMap<TokenId, Token>,
    fact_to_tokens: HashMap<FactId, SmallVec<[TokenId; 4]>>,
    parent_to_children: HashMap<TokenId, SmallVec<[TokenId; 4]>>,
}

impl TokenStore {
    /// Create a new, empty token store.
    #[must_use]
    pub fn new() -> Self {
        Self {
            tokens: SlotMap::with_key(),
            fact_to_tokens: HashMap::new(),
            parent_to_children: HashMap::new(),
        }
    }

    /// Insert a token into the store.
    ///
    /// Updates both reverse indices (`fact_to_tokens` and `parent_to_children`).
    /// Deduplicates fact IDs when updating the fact index (same `FactId` may appear
    /// multiple times in `token.facts`).
    ///
    /// Returns the unique `TokenId` assigned to the token.
    pub fn insert(&mut self, token: Token) -> TokenId {
        let parent = token.parent;

        let id = self.tokens.insert(token);

        // Update fact_to_tokens index with deduplication
        // Use a temporary SmallVec to track already-indexed facts for this token
        let mut indexed_facts: SmallVec<[FactId; 4]> = SmallVec::new();
        let token_ref = &self.tokens[id];

        for &fact_id in &token_ref.facts {
            // Only index if we haven't already indexed this fact for this token
            if !indexed_facts.contains(&fact_id) {
                indexed_facts.push(fact_id);
                let entry = self.fact_to_tokens.entry(fact_id).or_default();

                // Debug assert: no duplicate TokenId in the index
                debug_assert!(
                    !entry.contains(&id),
                    "duplicate TokenId in fact_to_tokens index"
                );

                entry.push(id);
            }
        }

        // Update parent_to_children index
        if let Some(parent_id) = parent {
            let entry = self.parent_to_children.entry(parent_id).or_default();

            // Debug assert: no duplicate TokenId in the index
            debug_assert!(
                !entry.contains(&id),
                "duplicate TokenId in parent_to_children index"
            );

            entry.push(id);
        }

        id
    }

    /// Remove a single token from the store.
    ///
    /// Updates both reverse indices and prunes empty entries.
    /// Does NOT cascade to children.
    ///
    /// Returns the removed token if it existed, or `None` if not found.
    pub fn remove(&mut self, id: TokenId) -> Option<Token> {
        let token = self.tokens.remove(id)?;

        // Clean up fact_to_tokens index with deduplication
        let mut cleaned_facts: SmallVec<[FactId; 4]> = SmallVec::new();
        for &fact_id in &token.facts {
            // Only clean once per distinct fact
            if !cleaned_facts.contains(&fact_id) {
                cleaned_facts.push(fact_id);
                remove_from_vec_index(&mut self.fact_to_tokens, fact_id, id);
            }
        }

        // Clean up parent_to_children index: remove from parent's children list
        if let Some(parent_id) = token.parent {
            remove_from_vec_index(&mut self.parent_to_children, parent_id, id);
        }

        // Also remove the entry where this token is the parent (if it has children)
        // This orphans the children, which is acceptable for non-cascading remove
        self.parent_to_children.remove(&id);

        Some(token)
    }

    /// Remove a token and all its descendants.
    ///
    /// Uses the `parent_to_children` index to efficiently find and remove all
    /// descendants. Uses an iterative stack-based traversal to avoid recursion.
    ///
    /// Returns all removed `(TokenId, Token)` pairs in arbitrary order.
    pub fn remove_cascade(&mut self, root_id: TokenId) -> Vec<(TokenId, Token)> {
        debug_assert!(self.tokens.contains_key(root_id), "root token must exist");

        let mut removed = Vec::new();
        let mut stack = vec![root_id];

        while let Some(id) = stack.pop() {
            // Collect children before removing the token
            if let Some(children) = self.parent_to_children.get(&id) {
                stack.extend(children.iter().copied());
            }

            // Remove the token
            if let Some(token) = self.remove(id) {
                removed.push((id, token));
            }
        }

        removed
    }

    /// Remove all tokens and clear all indices.
    pub fn clear(&mut self) {
        self.tokens.clear();
        self.fact_to_tokens.clear();
        self.parent_to_children.clear();
    }

    /// Return an iterator over all tokens that contain the given fact.
    pub fn tokens_containing(&self, fact_id: FactId) -> impl Iterator<Item = TokenId> + '_ {
        self.fact_to_tokens
            .get(&fact_id)
            .into_iter()
            .flat_map(|tokens| tokens.iter().copied())
    }

    /// Return an iterator over the direct children of a token.
    pub fn children(&self, id: TokenId) -> impl Iterator<Item = TokenId> + '_ {
        self.parent_to_children
            .get(&id)
            .into_iter()
            .flat_map(|children| children.iter().copied())
    }

    /// Given a set of affected tokens, return only those that are retraction roots.
    ///
    /// A token is a retraction root if none of its ancestors are in the affected set.
    /// This prevents cascading from both a token and its ancestor (which would be
    /// redundant and incorrect).
    pub fn retraction_roots(&self, affected: &HashSet<TokenId>) -> Vec<TokenId> {
        let mut roots = Vec::new();

        for &token_id in affected {
            let mut is_root = true;
            let mut current = token_id;

            // Walk up the parent chain
            while let Some(token) = self.tokens.get(current) {
                if let Some(parent_id) = token.parent {
                    if affected.contains(&parent_id) {
                        // An ancestor is also affected, so this is not a root
                        is_root = false;
                        break;
                    }
                    current = parent_id;
                } else {
                    // Reached the top of the chain
                    break;
                }
            }

            if is_root {
                roots.push(token_id);
            }
        }

        roots
    }

    /// Get a reference to a token by ID.
    #[must_use]
    pub fn get(&self, id: TokenId) -> Option<&Token> {
        self.tokens.get(id)
    }

    /// Returns the number of tokens in the store.
    #[must_use]
    pub fn len(&self) -> usize {
        self.tokens.len()
    }

    /// Returns `true` if the store contains no tokens.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.tokens.is_empty()
    }

    /// Verify internal consistency of all indices.
    ///
    /// Intended for use in tests and debug builds.
    #[cfg(any(test, debug_assertions))]
    pub fn debug_assert_consistency(&self) {
        // 1. Every TokenId in fact_to_tokens exists in the tokens SlotMap
        for (fact_id, token_ids) in &self.fact_to_tokens {
            for &token_id in token_ids {
                assert!(
                    self.tokens.contains_key(token_id),
                    "fact_to_tokens references non-existent token {token_id:?} for fact {fact_id:?}"
                );
            }
        }

        // 2. Every TokenId in parent_to_children (both keys and values) exists in tokens
        for (&parent_id, children) in &self.parent_to_children {
            assert!(
                self.tokens.contains_key(parent_id),
                "parent_to_children has non-existent parent {parent_id:?}"
            );
            for &child_id in children {
                assert!(
                    self.tokens.contains_key(child_id),
                    "parent_to_children references non-existent child {child_id:?}"
                );
            }
        }

        // 3. For every token, its facts are correctly reflected in fact_to_tokens
        for (token_id, token) in &self.tokens {
            // Build deduplicated set of facts for this token
            let mut unique_facts: SmallVec<[FactId; 4]> = SmallVec::new();
            for &fact_id in &token.facts {
                if !unique_facts.contains(&fact_id) {
                    unique_facts.push(fact_id);
                }
            }

            // Verify each unique fact has this token in its index
            for &fact_id in &unique_facts {
                let indexed_tokens = self.fact_to_tokens.get(&fact_id);
                assert!(
                    indexed_tokens.is_some(),
                    "token {token_id:?} contains fact {fact_id:?} but fact_to_tokens has no entry"
                );
                assert!(
                    indexed_tokens.unwrap().contains(&token_id),
                    "token {token_id:?} contains fact {fact_id:?} but is not in fact_to_tokens index"
                );
            }
        }

        // 4. For every token with a parent that still exists, the parent's children list contains it
        for (token_id, token) in &self.tokens {
            if let Some(parent_id) = token.parent {
                // Only check if parent still exists (orphaned tokens are allowed after non-cascading remove)
                if self.tokens.contains_key(parent_id) {
                    let children = self.parent_to_children.get(&parent_id);
                    assert!(
                        children.is_some(),
                        "token {token_id:?} has parent {parent_id:?} but parent has no children entry"
                    );
                    assert!(
                        children.unwrap().contains(&token_id),
                        "token {token_id:?} has parent {parent_id:?} but is not in parent's children list"
                    );
                }
            }
        }

        // 5. No empty SmallVecs exist in the index maps
        for (fact_id, tokens) in &self.fact_to_tokens {
            assert!(
                !tokens.is_empty(),
                "fact_to_tokens has empty entry for fact {fact_id:?}"
            );
        }

        for (parent_id, children) in &self.parent_to_children {
            assert!(
                !children.is_empty(),
                "parent_to_children has empty entry for parent {parent_id:?}"
            );
        }
    }
}

impl Default for TokenStore {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Helper to create a minimal token for testing
    fn make_token(facts: Vec<FactId>, parent: Option<TokenId>, owner_node: NodeId) -> Token {
        Token {
            facts: SmallVec::from_vec(facts),
            bindings: BindingSet::new(),
            parent,
            owner_node,
        }
    }

    // Helper to create distinct FactIds for testing
    fn make_fact_ids(n: usize) -> Vec<FactId> {
        let mut temp: SlotMap<FactId, ()> = SlotMap::with_key();
        (0..n).map(|_| temp.insert(())).collect()
    }

    #[test]
    fn new_store_is_empty() {
        let store = TokenStore::new();
        assert!(store.is_empty());
        assert_eq!(store.len(), 0);
    }

    #[test]
    fn insert_and_get() {
        let mut store = TokenStore::new();
        let facts = make_fact_ids(1);
        let token = make_token(vec![facts[0]], None, NodeId(0));

        let id = store.insert(token);

        assert_eq!(store.len(), 1);
        assert!(store.get(id).is_some());
    }

    #[test]
    fn insert_updates_fact_to_tokens_index() {
        let mut store = TokenStore::new();
        let facts = make_fact_ids(2);
        let token = make_token(vec![facts[0], facts[1]], None, NodeId(0));

        let id = store.insert(token);

        let tokens_for_fact0: Vec<_> = store.tokens_containing(facts[0]).collect();
        assert_eq!(tokens_for_fact0.len(), 1);
        assert_eq!(tokens_for_fact0[0], id);

        let tokens_for_fact1: Vec<_> = store.tokens_containing(facts[1]).collect();
        assert_eq!(tokens_for_fact1.len(), 1);
        assert_eq!(tokens_for_fact1[0], id);
    }

    #[test]
    fn insert_updates_parent_to_children_index() {
        let mut store = TokenStore::new();
        let facts = make_fact_ids(2);

        let parent_token = make_token(vec![facts[0]], None, NodeId(0));
        let parent_id = store.insert(parent_token);

        let child_token = make_token(vec![facts[1]], Some(parent_id), NodeId(1));
        let child_id = store.insert(child_token);

        let children: Vec<_> = store.children(parent_id).collect();
        assert_eq!(children.len(), 1);
        assert_eq!(children[0], child_id);
    }

    #[test]
    fn insert_dedup_fact_index() {
        let mut store = TokenStore::new();
        let facts = make_fact_ids(1);

        // Token with the same FactId appearing twice
        let token = make_token(vec![facts[0], facts[0]], None, NodeId(0));
        let id = store.insert(token);

        // Should only appear once in the index
        let tokens_for_fact: Vec<_> = store.tokens_containing(facts[0]).collect();
        assert_eq!(tokens_for_fact.len(), 1);
        assert_eq!(tokens_for_fact[0], id);

        // The index entry should only contain the token once
        let entry = store.fact_to_tokens.get(&facts[0]).unwrap();
        assert_eq!(entry.len(), 1);
    }

    #[test]
    fn remove_single_token() {
        let mut store = TokenStore::new();
        let facts = make_fact_ids(1);
        let token = make_token(vec![facts[0]], None, NodeId(0));

        let id = store.insert(token);
        assert_eq!(store.len(), 1);

        let removed = store.remove(id);
        assert!(removed.is_some());
        assert_eq!(store.len(), 0);
        assert!(store.get(id).is_none());
    }

    #[test]
    fn remove_cleans_fact_index() {
        let mut store = TokenStore::new();
        let facts = make_fact_ids(1);
        let token = make_token(vec![facts[0]], None, NodeId(0));

        let id = store.insert(token);
        store.remove(id);

        let tokens_for_fact: Vec<_> = store.tokens_containing(facts[0]).collect();
        assert!(tokens_for_fact.is_empty());
    }

    #[test]
    fn remove_cleans_parent_index() {
        let mut store = TokenStore::new();
        let facts = make_fact_ids(2);

        let parent_token = make_token(vec![facts[0]], None, NodeId(0));
        let parent_id = store.insert(parent_token);

        let child_token = make_token(vec![facts[1]], Some(parent_id), NodeId(1));
        let child_id = store.insert(child_token);

        store.remove(child_id);

        let children: Vec<_> = store.children(parent_id).collect();
        assert!(children.is_empty());
    }

    #[test]
    fn remove_prunes_empty_entries() {
        let mut store = TokenStore::new();
        let facts = make_fact_ids(1);
        let token = make_token(vec![facts[0]], None, NodeId(0));

        let id = store.insert(token);
        assert!(store.fact_to_tokens.contains_key(&facts[0]));

        store.remove(id);

        // Empty entry should be pruned
        assert!(!store.fact_to_tokens.contains_key(&facts[0]));
    }

    #[test]
    fn remove_nonexistent_returns_none() {
        let mut store = TokenStore::new();
        let mut temp: SlotMap<TokenId, ()> = SlotMap::with_key();
        let fake_id = temp.insert(());

        let result = store.remove(fake_id);
        assert!(result.is_none());
    }

    #[test]
    fn remove_cascade_removes_subtree() {
        let mut store = TokenStore::new();
        let facts = make_fact_ids(4);

        // Build a tree:
        //     t0
        //    /  \
        //   t1   t2
        //   |
        //   t3

        let t0 = store.insert(make_token(vec![facts[0]], None, NodeId(0)));
        let t1 = store.insert(make_token(vec![facts[1]], Some(t0), NodeId(1)));
        let t2 = store.insert(make_token(vec![facts[2]], Some(t0), NodeId(2)));
        let t3 = store.insert(make_token(vec![facts[3]], Some(t1), NodeId(3)));

        assert_eq!(store.len(), 4);

        // Cascade from t1 should remove t1 and t3, but not t0 or t2
        let removed = store.remove_cascade(t1);

        assert_eq!(removed.len(), 2);
        assert_eq!(store.len(), 2);
        assert!(store.get(t0).is_some());
        assert!(store.get(t1).is_none());
        assert!(store.get(t2).is_some());
        assert!(store.get(t3).is_none());
    }

    #[test]
    fn remove_cascade_returns_all_removed() {
        let mut store = TokenStore::new();
        let facts = make_fact_ids(3);

        let t0 = store.insert(make_token(vec![facts[0]], None, NodeId(0)));
        let t1 = store.insert(make_token(vec![facts[1]], Some(t0), NodeId(1)));
        let t2 = store.insert(make_token(vec![facts[2]], Some(t0), NodeId(2)));

        let removed = store.remove_cascade(t0);

        assert_eq!(removed.len(), 3);

        let removed_ids: HashSet<_> = removed.iter().map(|(id, _)| *id).collect();
        assert!(removed_ids.contains(&t0));
        assert!(removed_ids.contains(&t1));
        assert!(removed_ids.contains(&t2));
    }

    #[test]
    fn remove_cascade_on_leaf_removes_just_leaf() {
        let mut store = TokenStore::new();
        let facts = make_fact_ids(2);

        let t0 = store.insert(make_token(vec![facts[0]], None, NodeId(0)));
        let t1 = store.insert(make_token(vec![facts[1]], Some(t0), NodeId(1)));

        let removed = store.remove_cascade(t1);

        assert_eq!(removed.len(), 1);
        assert_eq!(removed[0].0, t1);
        assert!(store.get(t0).is_some());
    }

    #[test]
    fn tokens_containing_returns_correct_set() {
        let mut store = TokenStore::new();
        let facts = make_fact_ids(3);

        let t0 = store.insert(make_token(vec![facts[0], facts[1]], None, NodeId(0)));
        let t1 = store.insert(make_token(vec![facts[1], facts[2]], None, NodeId(1)));
        let _t2 = store.insert(make_token(vec![facts[2]], None, NodeId(2)));

        // fact[1] should be in t0 and t1
        let tokens: Vec<_> = store.tokens_containing(facts[1]).collect();
        assert_eq!(tokens.len(), 2);
        assert!(tokens.contains(&t0));
        assert!(tokens.contains(&t1));
    }

    #[test]
    fn children_returns_direct_children_only() {
        let mut store = TokenStore::new();
        let facts = make_fact_ids(3);

        // Build: t0 -> t1 -> t2
        let t0 = store.insert(make_token(vec![facts[0]], None, NodeId(0)));
        let t1 = store.insert(make_token(vec![facts[1]], Some(t0), NodeId(1)));
        let _t2 = store.insert(make_token(vec![facts[2]], Some(t1), NodeId(2)));

        // t0's children should only be t1, not t2
        let children: Vec<_> = store.children(t0).collect();
        assert_eq!(children.len(), 1);
        assert_eq!(children[0], t1);
    }

    #[test]
    fn retraction_roots_filters_to_minimal_set() {
        let mut store = TokenStore::new();
        let facts = make_fact_ids(4);

        // Build: t0 -> t1 -> t2, and separate t3
        let t0 = store.insert(make_token(vec![facts[0]], None, NodeId(0)));
        let t1 = store.insert(make_token(vec![facts[1]], Some(t0), NodeId(1)));
        let t2 = store.insert(make_token(vec![facts[2]], Some(t1), NodeId(2)));
        let t3 = store.insert(make_token(vec![facts[3]], None, NodeId(3)));

        // If t0, t1, and t2 are all affected, only t0 should be a root
        // t3 is independent, so it's also a root
        let mut affected = HashSet::new();
        affected.insert(t0);
        affected.insert(t1);
        affected.insert(t2);
        affected.insert(t3);

        let roots = store.retraction_roots(&affected);

        assert_eq!(roots.len(), 2);
        assert!(roots.contains(&t0));
        assert!(roots.contains(&t3));
    }

    #[test]
    fn retraction_roots_all_independent_are_all_roots() {
        let mut store = TokenStore::new();
        let facts = make_fact_ids(3);

        let t0 = store.insert(make_token(vec![facts[0]], None, NodeId(0)));
        let t1 = store.insert(make_token(vec![facts[1]], None, NodeId(1)));
        let t2 = store.insert(make_token(vec![facts[2]], None, NodeId(2)));

        let mut affected = HashSet::new();
        affected.insert(t0);
        affected.insert(t1);
        affected.insert(t2);

        let roots = store.retraction_roots(&affected);

        assert_eq!(roots.len(), 3);
        assert!(roots.contains(&t0));
        assert!(roots.contains(&t1));
        assert!(roots.contains(&t2));
    }

    #[test]
    fn consistency_check_passes_on_valid_store() {
        let mut store = TokenStore::new();
        let facts = make_fact_ids(3);

        let t0 = store.insert(make_token(vec![facts[0]], None, NodeId(0)));
        let t1 = store.insert(make_token(vec![facts[1]], Some(t0), NodeId(1)));
        let _t2 = store.insert(make_token(vec![facts[2]], Some(t1), NodeId(2)));

        // Should not panic
        store.debug_assert_consistency();
    }

    #[test]
    fn consistency_check_after_insert_and_remove() {
        let mut store = TokenStore::new();
        let facts = make_fact_ids(5);

        let t0 = store.insert(make_token(vec![facts[0]], None, NodeId(0)));
        let t1 = store.insert(make_token(vec![facts[1]], Some(t0), NodeId(1)));
        let t2 = store.insert(make_token(vec![facts[2]], Some(t0), NodeId(2)));
        let _t3 = store.insert(make_token(vec![facts[3]], Some(t1), NodeId(3)));
        let _t4 = store.insert(make_token(vec![facts[4]], Some(t2), NodeId(4)));

        store.debug_assert_consistency();

        // Remove t1 (should clean up t1's references but leave t3)
        store.remove(t1);
        store.debug_assert_consistency();

        // Cascade from t2
        store.remove_cascade(t2);
        store.debug_assert_consistency();
    }
}

#[cfg(test)]
mod proptests {
    use super::*;
    use proptest::prelude::*;

    proptest! {
        #[test]
        fn insert_n_tokens_maintains_consistency(count in 1..50_usize) {
            let mut store = TokenStore::new();
            let mut temp_facts: SlotMap<FactId, ()> = SlotMap::with_key();

            #[allow(clippy::cast_possible_truncation)]
            for i in 0..count {
                let fact_id = temp_facts.insert(());
                let token = Token {
                    facts: smallvec::smallvec![fact_id],
                    bindings: BindingSet::new(),
                    parent: None,
                    owner_node: NodeId(i as u32),
                };
                store.insert(token);
            }

            prop_assert_eq!(store.len(), count);
            store.debug_assert_consistency();
        }

        #[test]
        fn insert_then_remove_cascade_leaves_empty_store(depth in 1..10_usize) {
            let mut store = TokenStore::new();
            let mut temp_facts: SlotMap<FactId, ()> = SlotMap::with_key();

            // Build a chain: t0 -> t1 -> t2 -> ... -> t[depth-1]
            let mut parent_id = None;
            let mut root_id = None;

            #[allow(clippy::cast_possible_truncation)]
            for i in 0..depth {
                let fact_id = temp_facts.insert(());
                let token = Token {
                    facts: smallvec::smallvec![fact_id],
                    bindings: BindingSet::new(),
                    parent: parent_id,
                    owner_node: NodeId(i as u32),
                };
                let id = store.insert(token);

                if i == 0 {
                    root_id = Some(id);
                }
                parent_id = Some(id);
            }

            prop_assert_eq!(store.len(), depth);

            // Cascade from root should remove everything
            store.remove_cascade(root_id.unwrap());
            prop_assert!(store.is_empty());
            store.debug_assert_consistency();
        }

        #[test]
        fn retraction_roots_never_returns_ancestors_of_other_affected_tokens(
            // Generate random tree structure
            ops in prop::collection::vec((0..10_usize, any::<bool>()), 1..30)
        ) {
            let mut store = TokenStore::new();
            let mut temp_facts: SlotMap<FactId, ()> = SlotMap::with_key();
            let mut token_ids = Vec::new();

            // Build tokens with random parent relationships
            #[allow(clippy::cast_possible_truncation)]
            for (parent_idx, has_parent) in ops {
                let fact_id = temp_facts.insert(());
                let parent = if has_parent && !token_ids.is_empty() {
                    let idx = parent_idx % token_ids.len();
                    Some(token_ids[idx])
                } else {
                    None
                };

                let token = Token {
                    facts: smallvec::smallvec![fact_id],
                    bindings: BindingSet::new(),
                    parent,
                    owner_node: NodeId(token_ids.len() as u32),
                };
                let id = store.insert(token);
                token_ids.push(id);
            }

            if token_ids.is_empty() {
                return Ok(());
            }

            // Pick a random subset as affected
            let affected: HashSet<_> = token_ids.iter()
                .enumerate()
                .filter(|(i, _)| i % 2 == 0)
                .map(|(_, &id)| id)
                .collect();

            if affected.is_empty() {
                return Ok(());
            }

            let roots = store.retraction_roots(&affected);

            // Verify: for each root, none of its ancestors should be in affected
            for &root_id in &roots {
                let mut current = root_id;
                while let Some(token) = store.get(current) {
                    if let Some(parent_id) = token.parent {
                        prop_assert!(
                            !affected.contains(&parent_id),
                            "root {:?} has ancestor {:?} in affected set",
                            root_id,
                            parent_id
                        );
                        current = parent_id;
                    } else {
                        break;
                    }
                }
            }
        }
    }
}
