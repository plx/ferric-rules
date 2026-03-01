//! Token storage with reverse indices for efficient retraction.
//!
//! Tokens represent partial matches through the beta network. They have stable
//! identities to support efficient retraction and cascading deletes.

use rustc_hash::{FxHashMap as HashMap, FxHashSet as HashSet};
use slotmap::SlotMap;
use smallvec::SmallVec;

use crate::binding::BindingSet;
use crate::fact::FactId;

/// Identifier for a node in the Rete network.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct NodeId(pub u32);

slotmap::new_key_type! {
    /// Unique identifier for a token within the token store.
    pub struct TokenId;
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
    fact_to_tokens: HashMap<FactId, HashSet<TokenId>>,
    parent_to_children: HashMap<TokenId, HashSet<TokenId>>,
}

impl TokenStore {
    /// Create a new, empty token store.
    #[must_use]
    pub fn new() -> Self {
        Self {
            tokens: SlotMap::with_key(),
            fact_to_tokens: HashMap::default(),
            parent_to_children: HashMap::default(),
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
                let inserted = self.fact_to_tokens.entry(fact_id).or_default().insert(id);

                // Debug assert: no duplicate TokenId in the index
                debug_assert!(inserted, "duplicate TokenId in fact_to_tokens index");
            }
        }

        // Update parent_to_children index
        if let Some(parent_id) = parent {
            let inserted = self
                .parent_to_children
                .entry(parent_id)
                .or_default()
                .insert(id);

            // Debug assert: no duplicate TokenId in the index
            debug_assert!(inserted, "duplicate TokenId in parent_to_children index");
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
                if let Some(set) = self.fact_to_tokens.get_mut(&fact_id) {
                    set.remove(&id);
                    if set.is_empty() {
                        self.fact_to_tokens.remove(&fact_id);
                    }
                }
            }
        }

        // Clean up parent_to_children index: remove from parent's children list
        if let Some(parent_id) = token.parent {
            if let Some(set) = self.parent_to_children.get_mut(&parent_id) {
                set.remove(&id);
                if set.is_empty() {
                    self.parent_to_children.remove(&parent_id);
                }
            }
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

        // 5. No empty sets exist in the index maps
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
        let mut affected = HashSet::default();
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

        let mut affected = HashSet::default();
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

    // ---------------------------------------------------------------------------
    // Operation enum
    // ---------------------------------------------------------------------------

    /// One mutation step applied to both the real `TokenStore` and the shadow model.
    #[derive(Clone, Debug)]
    enum Op {
        /// Insert a root token (no parent) referencing `fact_ids[fact_idx % len]`.
        InsertRoot { fact_idx: usize },
        /// Insert a child token whose parent is `live[parent_idx % len]`.
        InsertChild { parent_idx: usize, fact_idx: usize },
        /// Non-cascading remove of `live[idx % len]`.  No-op when store is empty.
        Remove { idx: usize },
        /// Cascading remove starting from `live[idx % len]`.  No-op when store is empty.
        RemoveCascade { idx: usize },
    }

    fn op_strategy() -> impl Strategy<Value = Op> {
        prop_oneof![
            3 => any::<usize>().prop_map(|f| Op::InsertRoot { fact_idx: f }),
            3 => (any::<usize>(), any::<usize>())
                .prop_map(|(p, f)| Op::InsertChild { parent_idx: p, fact_idx: f }),
            2 => any::<usize>().prop_map(|i| Op::Remove { idx: i }),
            1 => any::<usize>().prop_map(|i| Op::RemoveCascade { idx: i }),
        ]
    }

    fn scenario_strategy() -> impl Strategy<Value = Vec<Op>> {
        prop::collection::vec(op_strategy(), 0..100)
    }

    // ---------------------------------------------------------------------------
    // Shadow model
    // ---------------------------------------------------------------------------

    /// Tracks what the `TokenStore` should contain independently of the real
    /// implementation.
    #[derive(Default)]
    struct Model {
        /// Set of currently-live token IDs.
        live: HashSet<TokenId>,
        /// `parent_of`[id] = Some(p) means `id` was inserted with parent `p`.
        parent_of: HashMap<TokenId, Option<TokenId>>,
        /// `facts_of`[id] = the facts vec (may contain duplicates) for that token.
        facts_of: HashMap<TokenId, Vec<FactId>>,
    }

    impl Model {
        fn insert(&mut self, id: TokenId, parent: Option<TokenId>, facts: Vec<FactId>) {
            self.live.insert(id);
            self.parent_of.insert(id, parent);
            self.facts_of.insert(id, facts);
        }

        /// Collect `id` and all its descendants (tokens whose ancestor chain
        /// reaches `id`) from the live set.
        fn subtree(&self, root: TokenId) -> HashSet<TokenId> {
            let mut result = HashSet::default();
            let mut stack = vec![root];
            while let Some(id) = stack.pop() {
                if result.insert(id) {
                    // Push all live tokens whose direct parent is `id`
                    for &candidate in &self.live {
                        if self.parent_of.get(&candidate).copied().flatten() == Some(id) {
                            stack.push(candidate);
                        }
                    }
                }
            }
            result
        }

        /// Remove a single token from the shadow model (non-cascading).
        fn remove(&mut self, id: TokenId) {
            self.live.remove(&id);
            self.parent_of.remove(&id);
            self.facts_of.remove(&id);
        }

        /// Remove `root` and all descendants from the shadow model.
        fn remove_cascade(&mut self, root: TokenId) {
            let to_remove = self.subtree(root);
            for id in to_remove {
                self.remove(id);
            }
        }
    }

    // ---------------------------------------------------------------------------
    // Scenario runner
    // ---------------------------------------------------------------------------

    /// Run a sequence of operations on both a real `TokenStore` and a `Model`.
    ///
    /// Returns the final store, the final model, and the pre-allocated pool of
    /// `FactId` values.
    fn run_scenario(ops: &[Op]) -> (TokenStore, Model) {
        // Pre-allocate a pool of 16 distinct FactIds.
        let mut fact_pool: SlotMap<FactId, ()> = SlotMap::with_key();
        let fact_ids: Vec<FactId> = (0..16).map(|_| fact_pool.insert(())).collect();

        let mut store = TokenStore::new();
        let mut model = Model::default();
        // `live_vec` gives us an ordered snapshot of live IDs so we can index
        // into it with `idx % len`.
        let mut live_vec: Vec<TokenId> = Vec::new();

        #[allow(clippy::cast_possible_truncation)]
        let mut token_count: u32 = 0;

        for op in ops {
            match *op {
                Op::InsertRoot { fact_idx } => {
                    let fid = fact_ids[fact_idx % fact_ids.len()];
                    let t = Token {
                        facts: smallvec::smallvec![fid],
                        bindings: BindingSet::new(),
                        parent: None,
                        owner_node: NodeId(token_count),
                    };
                    token_count += 1;
                    let id = store.insert(t);
                    model.insert(id, None, vec![fid]);
                    live_vec.push(id);
                }
                Op::InsertChild {
                    parent_idx,
                    fact_idx,
                } => {
                    if live_vec.is_empty() {
                        // No live tokens to be a parent — treat as root insert.
                        let fid = fact_ids[fact_idx % fact_ids.len()];
                        let t = Token {
                            facts: smallvec::smallvec![fid],
                            bindings: BindingSet::new(),
                            parent: None,
                            owner_node: NodeId(token_count),
                        };
                        token_count += 1;
                        let id = store.insert(t);
                        model.insert(id, None, vec![fid]);
                        live_vec.push(id);
                    } else {
                        let parent_id = live_vec[parent_idx % live_vec.len()];
                        let fid = fact_ids[fact_idx % fact_ids.len()];
                        let t = Token {
                            facts: smallvec::smallvec![fid],
                            bindings: BindingSet::new(),
                            parent: Some(parent_id),
                            owner_node: NodeId(token_count),
                        };
                        token_count += 1;
                        let id = store.insert(t);
                        model.insert(id, Some(parent_id), vec![fid]);
                        live_vec.push(id);
                    }
                }
                Op::Remove { idx } => {
                    if live_vec.is_empty() {
                        continue;
                    }
                    let pick = idx % live_vec.len();
                    let id = live_vec.swap_remove(pick);
                    store.remove(id);
                    model.remove(id);
                }
                Op::RemoveCascade { idx } => {
                    if live_vec.is_empty() {
                        continue;
                    }
                    let pick = idx % live_vec.len();
                    let root = live_vec[pick];
                    // Compute the full subtree from the model *before* mutating.
                    let subtree = model.subtree(root);
                    store.remove_cascade(root);
                    model.remove_cascade(root);
                    // Rebuild live_vec: remove all IDs that were in the subtree.
                    live_vec.retain(|id| !subtree.contains(id));
                }
            }
        }

        (store, model)
    }

    // ---------------------------------------------------------------------------
    // Property tests
    // ---------------------------------------------------------------------------

    proptest! {
        /// After every operation the internal consistency check passes.
        #[test]
        fn arbitrary_ops_maintain_consistency(ops in scenario_strategy()) {
            // Run op-by-op so we can call consistency check after each step.
            let mut fact_pool: SlotMap<FactId, ()> = SlotMap::with_key();
            let fact_ids: Vec<FactId> = (0..16).map(|_| fact_pool.insert(())).collect();

            let mut store = TokenStore::new();
            let mut model = Model::default();
            let mut live_vec: Vec<TokenId> = Vec::new();

            #[allow(clippy::cast_possible_truncation)]
            let mut token_count: u32 = 0;

            for op in &ops {
                match *op {
                    Op::InsertRoot { fact_idx } => {
                        let fid = fact_ids[fact_idx % fact_ids.len()];
                        let t = Token {
                            facts: smallvec::smallvec![fid],
                            bindings: BindingSet::new(),
                            parent: None,
                            owner_node: NodeId(token_count),
                        };
                        token_count += 1;
                        let id = store.insert(t);
                        model.insert(id, None, vec![fid]);
                        live_vec.push(id);
                    }
                    Op::InsertChild { parent_idx, fact_idx } => {
                        let fid = fact_ids[fact_idx % fact_ids.len()];
                        if live_vec.is_empty() {
                            let t = Token {
                                facts: smallvec::smallvec![fid],
                                bindings: BindingSet::new(),
                                parent: None,
                                owner_node: NodeId(token_count),
                            };
                            token_count += 1;
                            let id = store.insert(t);
                            model.insert(id, None, vec![fid]);
                            live_vec.push(id);
                        } else {
                            let parent_id = live_vec[parent_idx % live_vec.len()];
                            let t = Token {
                                facts: smallvec::smallvec![fid],
                                bindings: BindingSet::new(),
                                parent: Some(parent_id),
                                owner_node: NodeId(token_count),
                            };
                            token_count += 1;
                            let id = store.insert(t);
                            model.insert(id, Some(parent_id), vec![fid]);
                            live_vec.push(id);
                        }
                    }
                    Op::Remove { idx } => {
                        if live_vec.is_empty() { continue; }
                        let pick = idx % live_vec.len();
                        let id = live_vec.swap_remove(pick);
                        store.remove(id);
                        model.remove(id);
                    }
                    Op::RemoveCascade { idx } => {
                        if live_vec.is_empty() { continue; }
                        let pick = idx % live_vec.len();
                        let root = live_vec[pick];
                        let subtree = model.subtree(root);
                        store.remove_cascade(root);
                        model.remove_cascade(root);
                        live_vec.retain(|id| !subtree.contains(id));
                    }
                }
                store.debug_assert_consistency();
            }
        }

        /// Every token we insert is immediately retrievable via `get()`.
        #[test]
        fn insert_get_roundtrip(ops in scenario_strategy()) {
            let mut fact_pool: SlotMap<FactId, ()> = SlotMap::with_key();
            let fact_ids: Vec<FactId> = (0..16).map(|_| fact_pool.insert(())).collect();

            let mut store = TokenStore::new();
            let mut live_vec: Vec<TokenId> = Vec::new();

            #[allow(clippy::cast_possible_truncation)]
            let mut token_count: u32 = 0;

            for op in &ops {
                match *op {
                    Op::InsertRoot { fact_idx } | Op::InsertChild { fact_idx, .. } => {
                        let parent = match *op {
                            Op::InsertChild { parent_idx, .. } if !live_vec.is_empty() => {
                                Some(live_vec[parent_idx % live_vec.len()])
                            }
                            _ => None,
                        };
                        let fid = fact_ids[fact_idx % fact_ids.len()];
                        let t = Token {
                            facts: smallvec::smallvec![fid],
                            bindings: BindingSet::new(),
                            parent,
                            owner_node: NodeId(token_count),
                        };
                        token_count += 1;
                        let id = store.insert(t);
                        // Must be retrievable immediately after insertion.
                        prop_assert!(store.get(id).is_some(), "get() returned None right after insert");
                        live_vec.push(id);
                    }
                    Op::Remove { idx } => {
                        if live_vec.is_empty() { continue; }
                        let pick = idx % live_vec.len();
                        let id = live_vec.swap_remove(pick);
                        store.remove(id);
                        prop_assert!(store.get(id).is_none(), "get() returned Some after remove");
                    }
                    Op::RemoveCascade { idx } => {
                        if live_vec.is_empty() { continue; }
                        let pick = idx % live_vec.len();
                        let root = live_vec[pick];
                        let removed: HashSet<TokenId> = store
                            .remove_cascade(root)
                            .into_iter()
                            .map(|(id, _)| id)
                            .collect();
                        live_vec.retain(|id| !removed.contains(id));
                        for id in &removed {
                            prop_assert!(
                                store.get(*id).is_none(),
                                "get() returned Some for cascade-removed token"
                            );
                        }
                    }
                }
            }
        }

        /// `len()` always matches the number of live tokens in the shadow model.
        #[test]
        fn len_tracks_live_tokens(ops in scenario_strategy()) {
            let (store, model) = run_scenario(&ops);
            prop_assert_eq!(
                store.len(),
                model.live.len(),
                "store.len()={} but shadow model has {} live tokens",
                store.len(),
                model.live.len()
            );
        }

        /// After `remove_cascade(root)`, neither root nor any of its descendants
        /// (as computed by the shadow model) are reachable via `get()`.
        #[test]
        fn cascade_removes_all_descendants(ops in scenario_strategy()) {
            let (mut store, model) = run_scenario(&ops);

            // Pick all root tokens (no parent) from the live set, cascade each
            // one and verify the subtree disappears.
            let roots: Vec<TokenId> = model
                .live
                .iter()
                .filter(|&&id| model.parent_of.get(&id).copied().flatten().is_none())
                .copied()
                .collect();

            for root in roots {
                if store.get(root).is_none() {
                    // Already removed by a previous cascade in this test.
                    continue;
                }
                // Compute expected subtree before cascading.
                let subtree: HashSet<TokenId> = {
                    // Walk live_vec in the store to find all descendants.
                    let mut desc = HashSet::default();
                    let mut stack = vec![root];
                    while let Some(id) = stack.pop() {
                        if desc.insert(id) {
                            let children: Vec<_> = store.children(id).collect();
                            stack.extend(children);
                        }
                    }
                    desc
                };

                store.remove_cascade(root);

                for id in &subtree {
                    prop_assert!(
                        store.get(*id).is_none(),
                        "token {:?} still reachable after cascade from root {:?}",
                        id,
                        root
                    );
                }
                store.debug_assert_consistency();
            }
        }

        /// After `remove_cascade(root)`, tokens outside root's subtree remain
        /// accessible.
        #[test]
        fn cascade_preserves_unrelated_tokens(ops in scenario_strategy()) {
            let (mut store, _model) = run_scenario(&ops);

            // Only proceed if there are at least two independent root subtrees.
            let all_live: Vec<TokenId> = store
                .tokens
                .keys()
                .collect();

            if all_live.len() < 2 {
                return Ok(());
            }

            // Find a root (token with no parent still alive).
            let root = all_live.iter().find(|&&id| {
                store.get(id).and_then(|t| t.parent).is_none()
            });
            let Some(&root) = root else { return Ok(()) };

            // Compute the subtree of that root.
            let subtree: HashSet<TokenId> = {
                let mut desc = HashSet::default();
                let mut stack = vec![root];
                while let Some(id) = stack.pop() {
                    if desc.insert(id) {
                        let children: Vec<_> = store.children(id).collect();
                        stack.extend(children);
                    }
                }
                desc
            };

            // Tokens outside the subtree.
            let unrelated: Vec<TokenId> = all_live
                .iter()
                .filter(|id| !subtree.contains(id))
                .copied()
                .collect();

            store.remove_cascade(root);

            for id in unrelated {
                prop_assert!(
                    store.get(id).is_some(),
                    "token {:?} (outside subtree of {:?}) missing after cascade",
                    id,
                    root
                );
            }
            store.debug_assert_consistency();
        }

        /// For every live token, each of its unique facts maps back to that token
        /// via `tokens_containing()`.  And every entry returned by
        /// `tokens_containing(f)` actually contains `f` in its `facts` vec.
        #[test]
        fn fact_to_tokens_bidirectional(ops in scenario_strategy()) {
            let (store, _model) = run_scenario(&ops);

            // Forward: every live token's unique facts index back to it.
            for (token_id, token) in &store.tokens {
                let mut seen_facts: SmallVec<[FactId; 4]> = SmallVec::new();
                for &fid in &token.facts {
                    if seen_facts.contains(&fid) {
                        continue;
                    }
                    seen_facts.push(fid);
                    let indexed: Vec<_> = store.tokens_containing(fid).collect();
                    prop_assert!(
                        indexed.contains(&token_id),
                        "token {:?} has fact {:?} but tokens_containing() doesn't include it",
                        token_id,
                        fid
                    );
                }
            }

            // Reverse: every entry returned by tokens_containing(f) has f in its facts.
            for (&fact_id, token_ids) in &store.fact_to_tokens {
                for &tid in token_ids {
                    let token = store.get(tid).expect("index references non-existent token");
                    prop_assert!(
                        token.facts.contains(&fact_id),
                        "tokens_containing({:?}) includes {:?} but that token doesn't contain the fact",
                        fact_id,
                        tid
                    );
                }
            }
        }

        /// For every live token whose parent is also alive, the parent's
        /// `children()` includes it.  And every `children(p)` entry has
        /// `parent == Some(p)`.
        #[test]
        fn parent_to_children_bidirectional(ops in scenario_strategy()) {
            let (store, _model) = run_scenario(&ops);

            // Forward: every live token with a live parent appears in the parent's children.
            for (token_id, token) in &store.tokens {
                if let Some(parent_id) = token.parent {
                    if store.get(parent_id).is_some() {
                        let children: Vec<_> = store.children(parent_id).collect();
                        prop_assert!(
                            children.contains(&token_id),
                            "token {:?} has live parent {:?} but is not in children()",
                            token_id,
                            parent_id
                        );
                    }
                }
            }

            // Reverse: every child returned by children(p) has parent == Some(p).
            for (&parent_id, child_ids) in &store.parent_to_children {
                for &cid in child_ids {
                    let child = store.get(cid).expect("index references non-existent child token");
                    prop_assert_eq!(
                        child.parent,
                        Some(parent_id),
                        "children({:?}) includes {:?} but that token's parent is {:?}",
                        parent_id,
                        cid,
                        child.parent
                    );
                }
            }
        }

        /// Every token in the affected set is either a retraction root itself or
        /// a descendant of a root in the returned set.
        #[test]
        fn retraction_roots_coverage(ops in scenario_strategy()) {
            let (store, _model) = run_scenario(&ops);

            if store.is_empty() {
                return Ok(());
            }

            let all_live: HashSet<TokenId> = store.tokens.keys().collect();
            let roots = store.retraction_roots(&all_live);
            let roots_set: HashSet<TokenId> = roots.iter().copied().collect();

            for &affected_id in &all_live {
                // Walk up to see if this token's ancestor chain hits a root.
                let mut current = affected_id;
                let mut covered = false;
                loop {
                    if roots_set.contains(&current) {
                        covered = true;
                        break;
                    }
                    match store.get(current).and_then(|t| t.parent) {
                        Some(parent_id) if store.get(parent_id).is_some() => {
                            current = parent_id;
                        }
                        _ => break,
                    }
                }
                prop_assert!(
                    covered,
                    "token {:?} is not covered by any retraction root",
                    affected_id
                );
            }
        }

        /// No root returned by `retraction_roots` has an ancestor that is also
        /// in the affected set.
        #[test]
        fn retraction_roots_minimality(ops in scenario_strategy()) {
            let (store, _model) = run_scenario(&ops);

            if store.is_empty() {
                return Ok(());
            }

            let all_live: HashSet<TokenId> = store.tokens.keys().collect();
            let roots = store.retraction_roots(&all_live);

            for &root_id in &roots {
                let mut current = root_id;
                while let Some(token) = store.get(current) {
                    match token.parent {
                        Some(parent_id) => {
                            prop_assert!(
                                !all_live.contains(&parent_id),
                                "retraction root {:?} has ancestor {:?} in the affected set",
                                root_id,
                                parent_id
                            );
                            current = parent_id;
                        }
                        None => break,
                    }
                }
            }
        }

        /// A token whose `facts` vec contains the same `FactId` twice still only
        /// appears once in `fact_to_tokens` for that fact.
        #[test]
        fn duplicate_fact_dedup_in_index(count in 1..30_usize) {
            let mut fact_pool: SlotMap<FactId, ()> = SlotMap::with_key();
            let fid = fact_pool.insert(());

            let mut store = TokenStore::new();

            #[allow(clippy::cast_possible_truncation)]
            for i in 0..count {
                // Each token references the same FactId twice.
                let t = Token {
                    facts: smallvec::smallvec![fid, fid],
                    bindings: BindingSet::new(),
                    parent: None,
                    owner_node: NodeId(i as u32),
                };
                store.insert(t);
            }

            // There should be exactly `count` tokens in the index (one per token,
            // no duplicates within a token's entry).
            let indexed: Vec<_> = store.tokens_containing(fid).collect();
            prop_assert_eq!(
                indexed.len(),
                count,
                "expected {} entries in fact_to_tokens but found {}",
                count,
                indexed.len()
            );
            store.debug_assert_consistency();
        }

        /// Removing an already-removed `TokenId` returns `None`
        /// and does not change `len()`.
        #[test]
        fn remove_already_removed_is_noop(count in 1..20_usize) {
            let mut store = TokenStore::new();

            // Insert `count` tokens, then remove the first and try again.
            let mut ids = Vec::new();
            #[allow(clippy::cast_possible_truncation)]
            for i in 0..count {
                let id = store.insert(Token {
                    facts: SmallVec::new(),
                    bindings: BindingSet::new(),
                    parent: None,
                    owner_node: NodeId(i as u32),
                });
                ids.push(id);
            }

            let target = ids[0];
            let first = store.remove(target);
            prop_assert!(first.is_some(), "first remove must return Some");
            store.debug_assert_consistency();

            let before_len = store.len();
            let second = store.remove(target);
            prop_assert!(second.is_none(), "second remove of same ID returned Some");
            prop_assert_eq!(store.len(), before_len, "len changed after removing already-removed ID");
            store.debug_assert_consistency();
        }

        /// When the entire store is a single chain (root → … → leaf) and we
        /// cascade from the root, the store becomes empty.
        #[test]
        fn cascade_from_root_empties_store(depth in 1..20_usize) {
            let mut fact_pool: SlotMap<FactId, ()> = SlotMap::with_key();
            let fact_ids: Vec<FactId> = (0..depth).map(|_| fact_pool.insert(())).collect();

            let mut store = TokenStore::new();
            let mut parent_id: Option<TokenId> = None;
            let mut root_id: Option<TokenId> = None;

            #[allow(clippy::cast_possible_truncation)]
            for (i, &fact_id) in fact_ids.iter().enumerate().take(depth) {
                let t = Token {
                    facts: smallvec::smallvec![fact_id],
                    bindings: BindingSet::new(),
                    parent: parent_id,
                    owner_node: NodeId(i as u32),
                };
                let id = store.insert(t);
                if root_id.is_none() {
                    root_id = Some(id);
                }
                parent_id = Some(id);
            }

            prop_assert_eq!(store.len(), depth);
            store.remove_cascade(root_id.unwrap());
            prop_assert!(store.is_empty(), "store not empty after cascading from chain root");
            store.debug_assert_consistency();
        }
    }
}
