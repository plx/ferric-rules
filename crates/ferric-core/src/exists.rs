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
        self.support
            .get(&parent_token_id)
            .map_or(0, HashSet::len)
    }

    /// Remove a fact from a parent token's support set.
    ///
    /// Returns (`new_count`, `was_removed`). Caller should check if count went to 0.
    pub fn remove_support(
        &mut self,
        parent_token_id: TokenId,
        fact_id: FactId,
    ) -> (usize, bool) {
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
        self.support
            .get(&parent_token_id)
            .map_or(0, HashSet::len)
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
