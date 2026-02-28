//! Exists memory: tracks support counts for existential quantification.
//!
//! An exists node handles `(exists (pattern))` by maintaining a support count
//! for each parent token. When the support count transitions from zero to
//! non-zero, a pass-through token is created and propagated. When it transitions
//! back to zero, the pass-through is retracted.
//!
//! ## Support counting
//!
//! For each parent token, we track the set of facts that provide "support"
//! (i.e., facts that match the exists pattern). The exists node propagates
//! when at least one supporting fact exists.
//!
//! ## Transitions
//!
//! - Support count 0→N (first match): create pass-through token, propagate
//! - Support count N→M (additional matches): no change (still propagated)
//! - Support count N→0 (last match retracted): retract pass-through token
//!
//! ## Phase 2 implementation
//!
//! - Pass 010: Exists node and exists memory

use std::collections::{HashMap, HashSet};

use crate::fact::FactId;
use crate::token::TokenId;

/// Unique identifier for an exists memory.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ExistsMemoryId(pub u32);

/// Exists memory tracks support counts for existential quantification.
///
/// For each parent token entering the exists node, we track which facts
/// provide support. When support transitions from 0→N, a pass-through
/// token is created. When it transitions from N→0, the pass-through
/// is retracted.
pub struct ExistsMemory {
    pub id: ExistsMemoryId,
    /// Parent token → set of supporting fact IDs
    support: HashMap<TokenId, HashSet<FactId>>,
    /// Parent token → pass-through token (when supported, count > 0)
    satisfied: HashMap<TokenId, TokenId>,
    /// Reverse index: fact → parent tokens it supports
    fact_to_parents: HashMap<FactId, HashSet<TokenId>>,
}

impl ExistsMemory {
    /// Create a new, empty exists memory.
    #[must_use]
    pub fn new(id: ExistsMemoryId) -> Self {
        Self {
            id,
            support: HashMap::new(),
            satisfied: HashMap::new(),
            fact_to_parents: HashMap::new(),
        }
    }

    /// Add a fact as support for a parent token.
    ///
    /// Returns the new support count. Caller should check if count went from 0→1.
    pub fn add_support(&mut self, parent_token_id: TokenId, fact_id: FactId) -> usize {
        self.support
            .entry(parent_token_id)
            .or_default()
            .insert(fact_id);
        self.fact_to_parents
            .entry(fact_id)
            .or_default()
            .insert(parent_token_id);
        self.support.get(&parent_token_id).map_or(0, HashSet::len)
    }

    /// Remove a fact from a parent token's support set.
    ///
    /// Returns (`new_count`, `was_removed`). Caller should check if count went to 0.
    pub fn remove_support(&mut self, parent_token_id: TokenId, fact_id: FactId) -> (usize, bool) {
        let mut was_removed = false;

        if let Some(facts) = self.support.get_mut(&parent_token_id) {
            was_removed = facts.remove(&fact_id);
            if facts.is_empty() {
                self.support.remove(&parent_token_id);
            }
        }

        if let Some(parents) = self.fact_to_parents.get_mut(&fact_id) {
            parents.remove(&parent_token_id);
            if parents.is_empty() {
                self.fact_to_parents.remove(&fact_id);
            }
        }

        let new_count = self.support_count(parent_token_id);
        (new_count, was_removed)
    }

    /// Get the current support count for a parent token.
    ///
    /// Returns 0 if the parent token has no support.
    #[must_use]
    pub fn support_count(&self, parent_token_id: TokenId) -> usize {
        self.support.get(&parent_token_id).map_or(0, HashSet::len)
    }

    /// Get all parent tokens supported by a specific fact.
    ///
    /// Used during fact retraction to find affected parent tokens.
    pub fn parents_supported_by(&self, fact_id: FactId) -> Vec<TokenId> {
        self.fact_to_parents
            .get(&fact_id)
            .map(|tokens| tokens.iter().copied().collect())
            .unwrap_or_default()
    }

    /// Record a parent token as satisfied with its pass-through token.
    pub fn set_satisfied(&mut self, parent_token_id: TokenId, passthrough_token_id: TokenId) {
        self.satisfied.insert(parent_token_id, passthrough_token_id);
    }

    /// Get the pass-through token for a satisfied parent token.
    #[must_use]
    pub fn get_passthrough(&self, parent_token_id: TokenId) -> Option<TokenId> {
        self.satisfied.get(&parent_token_id).copied()
    }

    /// Remove the satisfied entry for a parent token.
    ///
    /// Returns the pass-through token ID if it was satisfied.
    pub fn remove_satisfied(&mut self, parent_token_id: TokenId) -> Option<TokenId> {
        self.satisfied.remove(&parent_token_id)
    }

    /// Check if a parent token is currently satisfied (has support).
    #[must_use]
    pub fn is_satisfied(&self, parent_token_id: TokenId) -> bool {
        self.satisfied.contains_key(&parent_token_id)
    }

    /// Remove all tracking for a parent token (cleanup on parent retraction).
    pub fn remove_parent_token(&mut self, parent_token_id: TokenId) {
        // Remove from satisfied
        self.satisfied.remove(&parent_token_id);

        // Remove from support (and clean up reverse index)
        if let Some(facts) = self.support.remove(&parent_token_id) {
            for fact_id in facts {
                if let Some(parents) = self.fact_to_parents.get_mut(&fact_id) {
                    parents.remove(&parent_token_id);
                    if parents.is_empty() {
                        self.fact_to_parents.remove(&fact_id);
                    }
                }
            }
        }
    }

    /// Clear all state from this exists memory.
    pub fn clear(&mut self) {
        self.support.clear();
        self.satisfied.clear();
        self.fact_to_parents.clear();
    }

    /// Check if the exists memory has no entries.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.support.is_empty() && self.satisfied.is_empty() && self.fact_to_parents.is_empty()
    }

    /// Verify internal consistency of the exists memory.
    #[cfg(any(test, debug_assertions))]
    pub fn debug_assert_consistency(&self) {
        // Check 1: forward and reverse support indices are consistent
        for (&token_id, facts) in &self.support {
            assert!(
                !facts.is_empty(),
                "ExistsMemory {:?}: empty support set for token {token_id:?}",
                self.id
            );
            for &fact_id in facts {
                let parents = self.fact_to_parents.get(&fact_id);
                assert!(
                    parents.is_some_and(|p| p.contains(&token_id)),
                    "ExistsMemory {:?}: token {token_id:?} supported by fact {fact_id:?} but reverse index missing",
                    self.id
                );
            }
        }

        for (&fact_id, parents) in &self.fact_to_parents {
            assert!(
                !parents.is_empty(),
                "ExistsMemory {:?}: empty reverse set for fact {fact_id:?}",
                self.id
            );
            for &token_id in parents {
                let facts = self.support.get(&token_id);
                assert!(
                    facts.is_some_and(|f| f.contains(&fact_id)),
                    "ExistsMemory {:?}: reverse index says fact {fact_id:?} supports token {token_id:?} but forward missing",
                    self.id
                );
            }
        }

        // Check 2: satisfied tokens should have non-empty support
        for parent_token_id in self.satisfied.keys() {
            assert!(
                self.support.get(parent_token_id).map_or(0, HashSet::len) > 0,
                "ExistsMemory {:?}: parent token {parent_token_id:?} is satisfied but has no support",
                self.id
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use slotmap::SlotMap;

    fn make_token_ids(n: usize) -> Vec<TokenId> {
        let mut temp: SlotMap<TokenId, ()> = SlotMap::with_key();
        (0..n).map(|_| temp.insert(())).collect()
    }

    fn make_fact_ids(n: usize) -> Vec<FactId> {
        let mut temp: SlotMap<FactId, ()> = SlotMap::with_key();
        (0..n).map(|_| temp.insert(())).collect()
    }

    #[test]
    fn new_exists_memory_is_empty() {
        let mem = ExistsMemory::new(ExistsMemoryId(0));
        assert!(mem.is_empty());
        assert_eq!(mem.support_count(make_token_ids(1)[0]), 0);
    }

    #[test]
    fn add_support_from_zero_returns_one() {
        let mut mem = ExistsMemory::new(ExistsMemoryId(0));
        let tokens = make_token_ids(1);
        let facts = make_fact_ids(1);

        let count = mem.add_support(tokens[0], facts[0]);
        assert_eq!(count, 1);
        assert_eq!(mem.support_count(tokens[0]), 1);

        mem.debug_assert_consistency();
    }

    #[test]
    fn add_support_from_one_returns_two() {
        let mut mem = ExistsMemory::new(ExistsMemoryId(0));
        let tokens = make_token_ids(1);
        let facts = make_fact_ids(2);

        mem.add_support(tokens[0], facts[0]);
        let count = mem.add_support(tokens[0], facts[1]);
        assert_eq!(count, 2);

        mem.debug_assert_consistency();
    }

    #[test]
    fn remove_support_from_two_returns_one() {
        let mut mem = ExistsMemory::new(ExistsMemoryId(0));
        let tokens = make_token_ids(1);
        let facts = make_fact_ids(2);

        mem.add_support(tokens[0], facts[0]);
        mem.add_support(tokens[0], facts[1]);

        let (count, removed) = mem.remove_support(tokens[0], facts[0]);
        assert_eq!(count, 1);
        assert!(removed);

        mem.debug_assert_consistency();
    }

    #[test]
    fn remove_support_from_one_returns_zero() {
        let mut mem = ExistsMemory::new(ExistsMemoryId(0));
        let tokens = make_token_ids(1);
        let facts = make_fact_ids(1);

        mem.add_support(tokens[0], facts[0]);

        let (count, removed) = mem.remove_support(tokens[0], facts[0]);
        assert_eq!(count, 0);
        assert!(removed);
        assert!(mem.is_empty());

        mem.debug_assert_consistency();
    }

    #[test]
    fn parents_supported_by_returns_all_parents() {
        let mut mem = ExistsMemory::new(ExistsMemoryId(0));
        let tokens = make_token_ids(3);
        let facts = make_fact_ids(1);

        mem.add_support(tokens[0], facts[0]);
        mem.add_support(tokens[1], facts[0]);
        mem.add_support(tokens[2], facts[0]);

        let parents = mem.parents_supported_by(facts[0]);
        assert_eq!(parents.len(), 3);

        mem.debug_assert_consistency();
    }

    #[test]
    fn set_and_get_satisfied() {
        let mut mem = ExistsMemory::new(ExistsMemoryId(0));
        let tokens = make_token_ids(3);
        let facts = make_fact_ids(1);

        // Add support first (satisfied requires support)
        mem.add_support(tokens[0], facts[0]);
        mem.set_satisfied(tokens[0], tokens[1]);

        assert!(mem.is_satisfied(tokens[0]));
        assert_eq!(mem.get_passthrough(tokens[0]), Some(tokens[1]));

        mem.debug_assert_consistency();
    }

    #[test]
    fn remove_satisfied_returns_passthrough() {
        let mut mem = ExistsMemory::new(ExistsMemoryId(0));
        let tokens = make_token_ids(3);
        let facts = make_fact_ids(1);

        mem.add_support(tokens[0], facts[0]);
        mem.set_satisfied(tokens[0], tokens[1]);

        let pt = mem.remove_satisfied(tokens[0]);
        assert_eq!(pt, Some(tokens[1]));
        assert!(!mem.is_satisfied(tokens[0]));
    }

    #[test]
    fn remove_parent_token_cleans_everything() {
        let mut mem = ExistsMemory::new(ExistsMemoryId(0));
        let tokens = make_token_ids(2);
        let facts = make_fact_ids(2);

        mem.add_support(tokens[0], facts[0]);
        mem.add_support(tokens[0], facts[1]);
        mem.set_satisfied(tokens[0], tokens[1]);

        mem.remove_parent_token(tokens[0]);

        assert_eq!(mem.support_count(tokens[0]), 0);
        assert!(!mem.is_satisfied(tokens[0]));
        assert!(mem.parents_supported_by(facts[0]).is_empty());
        assert!(mem.parents_supported_by(facts[1]).is_empty());

        mem.debug_assert_consistency();
    }

    #[test]
    fn consistency_check_passes_on_valid_state() {
        let mut mem = ExistsMemory::new(ExistsMemoryId(0));
        let tokens = make_token_ids(3);
        let facts = make_fact_ids(2);

        // Token 0: supported by fact 0, satisfied
        mem.add_support(tokens[0], facts[0]);
        mem.set_satisfied(tokens[0], tokens[2]);

        // Token 1: supported by facts 0 and 1, not yet satisfied
        mem.add_support(tokens[1], facts[0]);
        mem.add_support(tokens[1], facts[1]);

        mem.debug_assert_consistency();
    }
}

#[cfg(test)]
mod proptests {
    use super::*;
    use proptest::prelude::*;
    use slotmap::SlotMap;

    // ---------------------------------------------------------------------------
    // Operation enum and shadow model
    // ---------------------------------------------------------------------------

    /// An operation that can be applied to an `ExistsMemory`.
    #[derive(Clone, Debug)]
    enum Op {
        AddSupport {
            token_idx: usize,
            fact_idx: usize,
        },
        RemoveSupport {
            token_idx: usize,
            fact_idx: usize,
        },
        SetSatisfied {
            token_idx: usize,
            passthrough_idx: usize,
        },
        RemoveSatisfied {
            token_idx: usize,
        },
        RemoveParent {
            token_idx: usize,
        },
    }

    /// Shadow model that mirrors the semantics of `ExistsMemory` using simple
    /// index-keyed maps so we can verify against the real implementation.
    #[derive(Default)]
    struct Model {
        /// `token_idx` → set of `fact_idxs` providing support
        support: std::collections::HashMap<usize, std::collections::HashSet<usize>>,
        /// `token_idx` → `passthrough_idx` (when satisfied)
        satisfied: std::collections::HashMap<usize, usize>,
    }

    impl Model {
        fn add_support(&mut self, token_idx: usize, fact_idx: usize) -> usize {
            self.support.entry(token_idx).or_default().insert(fact_idx);
            self.support
                .get(&token_idx)
                .map_or(0, std::collections::HashSet::len)
        }

        fn remove_support(&mut self, token_idx: usize, fact_idx: usize) -> (usize, bool) {
            let was_removed;
            if let Some(facts) = self.support.get_mut(&token_idx) {
                was_removed = facts.remove(&fact_idx);
                if facts.is_empty() {
                    self.support.remove(&token_idx);
                    // A token with no support cannot remain satisfied.
                    self.satisfied.remove(&token_idx);
                }
            } else {
                was_removed = false;
            }
            let count = self
                .support
                .get(&token_idx)
                .map_or(0, std::collections::HashSet::len);
            (count, was_removed)
        }

        fn remove_parent(&mut self, token_idx: usize) {
            self.support.remove(&token_idx);
            self.satisfied.remove(&token_idx);
        }

        fn support_count(&self, token_idx: usize) -> usize {
            self.support
                .get(&token_idx)
                .map_or(0, std::collections::HashSet::len)
        }

        fn is_satisfied(&self, token_idx: usize) -> bool {
            self.satisfied.contains_key(&token_idx)
        }

        fn is_empty(&self) -> bool {
            self.support.is_empty() && self.satisfied.is_empty()
        }

        /// Reverse index: for a given `fact_idx`, which `token_idxs` does the
        /// shadow model say are supported by it?
        fn parents_for_fact(&self, fact_idx: usize) -> std::collections::HashSet<usize> {
            let mut result = std::collections::HashSet::new();
            for (&token_idx, facts) in &self.support {
                if facts.contains(&fact_idx) {
                    result.insert(token_idx);
                }
            }
            result
        }
    }

    /// Strategy for generating a sequence of operations over pools of 5 tokens
    /// and 5 facts.
    fn op_strategy() -> impl Strategy<Value = Op> {
        prop_oneof![
            (0..5_usize, 0..5_usize).prop_map(|(t, f)| Op::AddSupport {
                token_idx: t,
                fact_idx: f
            }),
            (0..5_usize, 0..5_usize).prop_map(|(t, f)| Op::RemoveSupport {
                token_idx: t,
                fact_idx: f
            }),
            (0..5_usize, 0..5_usize).prop_map(|(t, p)| Op::SetSatisfied {
                token_idx: t,
                passthrough_idx: p
            }),
            (0..5_usize).prop_map(|t| Op::RemoveSatisfied { token_idx: t }),
            (0..5_usize).prop_map(|t| Op::RemoveParent { token_idx: t }),
        ]
    }

    // ---------------------------------------------------------------------------
    // Helper: build token/fact pools and apply an op to both model and memory
    // ---------------------------------------------------------------------------

    fn apply_op(
        op: &Op,
        mem: &mut ExistsMemory,
        model: &mut Model,
        tokens: &[TokenId],
        facts: &[FactId],
    ) {
        match *op {
            Op::AddSupport {
                token_idx,
                fact_idx,
            } => {
                mem.add_support(tokens[token_idx], facts[fact_idx]);
                model.add_support(token_idx, fact_idx);
            }
            Op::RemoveSupport {
                token_idx,
                fact_idx,
            } => {
                let (new_count, _) = mem.remove_support(tokens[token_idx], facts[fact_idx]);
                model.remove_support(token_idx, fact_idx);
                // When support drops to 0, a well-behaved caller always removes
                // the satisfied entry (mirroring real Rete node behavior). We do
                // this here so the invariant is maintained across all op sequences.
                if new_count == 0 {
                    mem.remove_satisfied(tokens[token_idx]);
                    // Shadow model already cleared satisfied in remove_support above.
                }
            }
            Op::SetSatisfied {
                token_idx,
                passthrough_idx,
            } => {
                // Only apply when the token has non-zero support (invariant).
                if model.support_count(token_idx) > 0 {
                    mem.set_satisfied(tokens[token_idx], tokens[passthrough_idx]);
                    model.satisfied.insert(token_idx, passthrough_idx);
                }
            }
            Op::RemoveSatisfied { token_idx } => {
                mem.remove_satisfied(tokens[token_idx]);
                model.satisfied.remove(&token_idx);
            }
            Op::RemoveParent { token_idx } => {
                mem.remove_parent_token(tokens[token_idx]);
                model.remove_parent(token_idx);
            }
        }
    }

    // ---------------------------------------------------------------------------
    // Property tests
    // ---------------------------------------------------------------------------

    proptest! {
        /// Running arbitrary sequences of operations never violates internal
        /// consistency as reported by `debug_assert_consistency`.
        #[test]
        fn arbitrary_ops_maintain_consistency(
            ops in proptest::collection::vec(op_strategy(), 0..100)
        ) {
            let mut token_map: SlotMap<TokenId, ()> = SlotMap::with_key();
            let mut fact_map: SlotMap<FactId, ()> = SlotMap::with_key();
            let tokens: Vec<TokenId> = (0..5).map(|_| token_map.insert(())).collect();
            let facts: Vec<FactId> = (0..5).map(|_| fact_map.insert(())).collect();

            let mut mem = ExistsMemory::new(ExistsMemoryId(0));
            let mut model = Model::default();

            for op in &ops {
                apply_op(op, &mut mem, &mut model, &tokens, &facts);
                mem.debug_assert_consistency();
            }
        }

        /// After random operations, the memory state matches the shadow model.
        #[test]
        fn model_matches_implementation(
            ops in proptest::collection::vec(op_strategy(), 0..100)
        ) {
            let mut token_map: SlotMap<TokenId, ()> = SlotMap::with_key();
            let mut fact_map: SlotMap<FactId, ()> = SlotMap::with_key();
            let tokens: Vec<TokenId> = (0..5).map(|_| token_map.insert(())).collect();
            let facts: Vec<FactId> = (0..5).map(|_| fact_map.insert(())).collect();

            let mut mem = ExistsMemory::new(ExistsMemoryId(0));
            let mut model = Model::default();

            for op in &ops {
                apply_op(op, &mut mem, &mut model, &tokens, &facts);
            }

            // Verify support_count and is_satisfied match the shadow model.
            for (idx, &token_idx) in tokens.iter().enumerate().take(5) {
                prop_assert_eq!(
                    mem.support_count(token_idx),
                    model.support_count(idx),
                    "support_count mismatch for token index {}",
                    idx
                );
                prop_assert_eq!(
                    mem.is_satisfied(token_idx),
                    model.is_satisfied(idx),
                    "is_satisfied mismatch for token index {}",
                    idx
                );
            }

            // Verify is_empty matches.
            prop_assert_eq!(mem.is_empty(), model.is_empty());
        }

        /// `add_support` returns the correct new count (shadow model size after
        /// insert).
        #[test]
        fn add_support_count_accuracy(
            prior_facts in proptest::collection::vec(0..5_usize, 0..5),
            new_fact in 0..5_usize,
            token_idx in 0..5_usize,
        ) {
            let mut token_map: SlotMap<TokenId, ()> = SlotMap::with_key();
            let mut fact_map: SlotMap<FactId, ()> = SlotMap::with_key();
            let tokens: Vec<TokenId> = (0..5).map(|_| token_map.insert(())).collect();
            let facts: Vec<FactId> = (0..5).map(|_| fact_map.insert(())).collect();

            let mut mem = ExistsMemory::new(ExistsMemoryId(0));
            let mut model = Model::default();

            for &f in &prior_facts {
                mem.add_support(tokens[token_idx], facts[f]);
                model.add_support(token_idx, f);
            }

            let expected = {
                let mut s = model.support.entry(token_idx).or_default().clone();
                s.insert(new_fact);
                s.len()
            };
            let actual = mem.add_support(tokens[token_idx], facts[new_fact]);

            prop_assert_eq!(actual, expected);
        }

        /// `remove_support` returns the correct count and `was_removed` flag.
        #[test]
        fn remove_support_count_and_was_removed(
            prior_facts in proptest::collection::vec(0..5_usize, 0..5),
            remove_fact in 0..5_usize,
            token_idx in 0..5_usize,
        ) {
            let mut token_map: SlotMap<TokenId, ()> = SlotMap::with_key();
            let mut fact_map: SlotMap<FactId, ()> = SlotMap::with_key();
            let tokens: Vec<TokenId> = (0..5).map(|_| token_map.insert(())).collect();
            let facts: Vec<FactId> = (0..5).map(|_| fact_map.insert(())).collect();

            let mut mem = ExistsMemory::new(ExistsMemoryId(0));
            let mut model = Model::default();

            for &f in &prior_facts {
                mem.add_support(tokens[token_idx], facts[f]);
                model.add_support(token_idx, f);
            }

            let (expected_count, expected_removed) = model.remove_support(token_idx, remove_fact);
            let (actual_count, actual_removed) = mem.remove_support(tokens[token_idx], facts[remove_fact]);

            prop_assert_eq!(actual_count, expected_count);
            prop_assert_eq!(actual_removed, expected_removed);
            mem.debug_assert_consistency();
        }

        /// Adding the same (token, fact) pair twice doesn't increase support_count.
        #[test]
        fn add_support_idempotent_on_duplicate(
            token_idx in 0..5_usize,
            fact_idx in 0..5_usize,
        ) {
            let mut token_map: SlotMap<TokenId, ()> = SlotMap::with_key();
            let mut fact_map: SlotMap<FactId, ()> = SlotMap::with_key();
            let tokens: Vec<TokenId> = (0..5).map(|_| token_map.insert(())).collect();
            let facts: Vec<FactId> = (0..5).map(|_| fact_map.insert(())).collect();

            let mut mem = ExistsMemory::new(ExistsMemoryId(0));

            mem.add_support(tokens[token_idx], facts[fact_idx]);
            let count_after_first = mem.support_count(tokens[token_idx]);

            mem.add_support(tokens[token_idx], facts[fact_idx]);
            let count_after_second = mem.support_count(tokens[token_idx]);

            prop_assert_eq!(count_after_first, count_after_second,
                "duplicate add_support must not increase count");
            mem.debug_assert_consistency();
        }

        /// When support transitions from 0→1, `add_support` returns 1.
        #[test]
        fn zero_to_one_transition(
            token_idx in 0..5_usize,
            fact_idx in 0..5_usize,
        ) {
            let mut token_map: SlotMap<TokenId, ()> = SlotMap::with_key();
            let mut fact_map: SlotMap<FactId, ()> = SlotMap::with_key();
            let tokens: Vec<TokenId> = (0..5).map(|_| token_map.insert(())).collect();
            let facts: Vec<FactId> = (0..5).map(|_| fact_map.insert(())).collect();

            let mut mem = ExistsMemory::new(ExistsMemoryId(0));

            prop_assume!(mem.support_count(tokens[token_idx]) == 0);
            let count = mem.add_support(tokens[token_idx], facts[fact_idx]);
            prop_assert_eq!(count, 1);
        }

        /// When the last supporting fact is removed, support_count drops to 0
        /// and the token is no longer tracked in the support map.
        #[test]
        fn n_to_zero_transition(
            fact_count in 1..10_usize,
        ) {
            let mut token_map: SlotMap<TokenId, ()> = SlotMap::with_key();
            let mut fact_map: SlotMap<FactId, ()> = SlotMap::with_key();
            let token = token_map.insert(());
            let facts: Vec<FactId> = (0..fact_count).map(|_| fact_map.insert(())).collect();

            let mut mem = ExistsMemory::new(ExistsMemoryId(0));

            for &f in &facts {
                mem.add_support(token, f);
            }
            prop_assert_eq!(mem.support_count(token), fact_count);

            // Remove all but the last
            for &f in facts.iter().take(fact_count - 1) {
                mem.remove_support(token, f);
                prop_assert!(mem.support_count(token) > 0);
            }

            // Remove the last one
            let (final_count, was_removed) = mem.remove_support(token, facts[fact_count - 1]);
            prop_assert_eq!(final_count, 0);
            prop_assert!(was_removed);
            prop_assert!(mem.is_empty(), "memory should be empty after last support removed");
            mem.debug_assert_consistency();
        }

        /// After `remove_parent_token(t)`, support_count is 0, `is_satisfied` is
        /// false, and `parents_supported_by(f)` no longer includes `t`.
        #[test]
        fn remove_parent_token_completeness(
            fact_count in 0..5_usize,
            mark_satisfied in any::<bool>(),
        ) {
            let mut token_map: SlotMap<TokenId, ()> = SlotMap::with_key();
            let mut fact_map: SlotMap<FactId, ()> = SlotMap::with_key();
            let parent = token_map.insert(());
            let passthrough = token_map.insert(());
            let facts: Vec<FactId> = (0..fact_count).map(|_| fact_map.insert(())).collect();

            let mut mem = ExistsMemory::new(ExistsMemoryId(0));

            for &f in &facts {
                mem.add_support(parent, f);
            }
            if mark_satisfied && fact_count > 0 {
                mem.set_satisfied(parent, passthrough);
            }

            mem.remove_parent_token(parent);

            prop_assert_eq!(mem.support_count(parent), 0);
            prop_assert!(!mem.is_satisfied(parent));
            for &f in &facts {
                let parents = mem.parents_supported_by(f);
                prop_assert!(!parents.contains(&parent),
                    "parents_supported_by should not include the removed parent");
            }
            mem.debug_assert_consistency();
        }

        /// When multiple tokens share a supporting fact, removing one parent
        /// does not affect the others.
        #[test]
        fn remove_parent_with_bystanders(
            bystander_count in 1..5_usize,
        ) {
            let mut token_map: SlotMap<TokenId, ()> = SlotMap::with_key();
            let mut fact_map: SlotMap<FactId, ()> = SlotMap::with_key();
            let shared_fact = fact_map.insert(());

            let target = token_map.insert(());
            let bystanders: Vec<TokenId> = (0..bystander_count).map(|_| token_map.insert(())).collect();

            let mut mem = ExistsMemory::new(ExistsMemoryId(0));

            // All tokens share the same fact
            mem.add_support(target, shared_fact);
            for &b in &bystanders {
                mem.add_support(b, shared_fact);
            }

            mem.remove_parent_token(target);

            // Bystanders should still be supported by shared_fact
            for &b in &bystanders {
                prop_assert_eq!(mem.support_count(b), 1,
                    "bystander support_count should still be 1");
            }
            let parents = mem.parents_supported_by(shared_fact);
            prop_assert_eq!(parents.len(), bystander_count,
                "all bystanders should still appear in parents_supported_by");
            prop_assert!(!parents.contains(&target),
                "removed parent should not appear in parents_supported_by");
            mem.debug_assert_consistency();
        }

        /// For every fact in the model, `parents_supported_by(fact)` matches the
        /// shadow model's reverse index.
        #[test]
        fn parents_supported_by_consistency(
            ops in proptest::collection::vec(op_strategy(), 0..80)
        ) {
            let mut token_map: SlotMap<TokenId, ()> = SlotMap::with_key();
            let mut fact_map: SlotMap<FactId, ()> = SlotMap::with_key();
            let tokens: Vec<TokenId> = (0..5).map(|_| token_map.insert(())).collect();
            let facts: Vec<FactId> = (0..5).map(|_| fact_map.insert(())).collect();

            let mut mem = ExistsMemory::new(ExistsMemoryId(0));
            let mut model = Model::default();

            for op in &ops {
                apply_op(op, &mut mem, &mut model, &tokens, &facts);
            }

            for (fact_idx, &fact) in facts.iter().enumerate().take(5) {
                let expected: std::collections::HashSet<usize> = model.parents_for_fact(fact_idx);
                let actual_vec = mem.parents_supported_by(fact);
                let actual: std::collections::HashSet<usize> = actual_vec
                    .iter()
                    .map(|&tid| tokens.iter().position(|&t| t == tid).unwrap())
                    .collect();
                prop_assert_eq!(
                    actual, expected,
                    "parents_supported_by mismatch for fact index {}",
                    fact_idx
                );
            }
        }

        /// `debug_assert_consistency` validates the satisfied-requires-support
        /// invariant. We verify that by always maintaining it in ops and checking
        /// consistency after every operation.
        #[test]
        fn satisfied_requires_support(
            ops in proptest::collection::vec(op_strategy(), 0..100)
        ) {
            let mut token_map: SlotMap<TokenId, ()> = SlotMap::with_key();
            let mut fact_map: SlotMap<FactId, ()> = SlotMap::with_key();
            let tokens: Vec<TokenId> = (0..5).map(|_| token_map.insert(())).collect();
            let facts: Vec<FactId> = (0..5).map(|_| fact_map.insert(())).collect();

            let mut mem = ExistsMemory::new(ExistsMemoryId(0));
            let mut model = Model::default();

            for op in &ops {
                apply_op(op, &mut mem, &mut model, &tokens, &facts);
                // debug_assert_consistency will panic (fail) if the invariant
                // is broken.
                mem.debug_assert_consistency();
            }
        }

        /// After `clear()`, `is_empty()` is true regardless of prior state.
        #[test]
        fn clear_resets_everything(
            ops in proptest::collection::vec(op_strategy(), 0..50)
        ) {
            let mut token_map: SlotMap<TokenId, ()> = SlotMap::with_key();
            let mut fact_map: SlotMap<FactId, ()> = SlotMap::with_key();
            let tokens: Vec<TokenId> = (0..5).map(|_| token_map.insert(())).collect();
            let facts: Vec<FactId> = (0..5).map(|_| fact_map.insert(())).collect();

            let mut mem = ExistsMemory::new(ExistsMemoryId(0));
            let mut model = Model::default();

            for op in &ops {
                apply_op(op, &mut mem, &mut model, &tokens, &facts);
            }

            mem.clear();
            prop_assert!(mem.is_empty(), "is_empty must be true after clear()");
            mem.debug_assert_consistency();
        }
    }
}
