//! Negative node: blocks parent tokens when a matching fact exists.
//!
//! A negative node acts like a join node but with inverted semantics: when a
//! fact matches the pattern, it *blocks* the parent token from propagating
//! downstream (rather than extending the partial match). When the blocking
//! fact is retracted, the parent token becomes unblocked and propagates.
//!
//! ## Blocker tracking
//!
//! Each negative node maintains a map from parent tokens to their set of
//! blocker facts. A parent token with an empty blocker set propagates; one
//! with any blockers does not.
//!
//! ## Pass-through tokens
//!
//! When a parent token is unblocked, the negative node creates a "pass-through"
//! token that copies the parent's facts and bindings. This pass-through token
//! is stored in the negative node's beta memory and propagated to downstream
//! children. When the token becomes blocked, the pass-through is cascade-retracted.
//!
//! ## Phase 2 implementation
//!
//! - Pass 006: Negative node (single-pattern) and blocker tracking
//! - Pass 010: NCC and exists nodes extend this foundation

use rustc_hash::{FxHashMap as HashMap, FxHashSet as HashSet};

use crate::fact::FactId;
use crate::token::TokenId;

/// Unique identifier for a negative memory.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct NegativeMemoryId(pub u32);

/// Negative memory: tracks blocker relationships for a negative node.
///
/// Maintains two views of blocked tokens:
/// - Forward: parent token → set of blocking facts
/// - Reverse: blocking fact → set of parent tokens it blocks
///
/// Also tracks unblocked parent tokens and their pass-through token IDs.
pub struct NegativeMemory {
    pub id: NegativeMemoryId,
    /// Blocked parent tokens → set of blocking facts.
    blocked: HashMap<TokenId, HashSet<FactId>>,
    /// Reverse index: blocking fact → set of parent tokens it blocks.
    fact_to_blocked: HashMap<FactId, HashSet<TokenId>>,
    /// Unblocked parent tokens → their pass-through token IDs.
    unblocked: HashMap<TokenId, TokenId>,
}

impl NegativeMemory {
    /// Create a new, empty negative memory.
    #[must_use]
    pub fn new(id: NegativeMemoryId) -> Self {
        Self {
            id,
            blocked: HashMap::default(),
            fact_to_blocked: HashMap::default(),
            unblocked: HashMap::default(),
        }
    }

    /// Add a fact as a blocker for a parent token.
    pub fn add_blocker(&mut self, parent_token_id: TokenId, fact_id: FactId) {
        self.blocked
            .entry(parent_token_id)
            .or_default()
            .insert(fact_id);
        self.fact_to_blocked
            .entry(fact_id)
            .or_default()
            .insert(parent_token_id);
    }

    /// Remove a fact from a parent token's blocker set.
    ///
    /// Returns `true` if the token is now fully unblocked (blocker set empty).
    pub fn remove_blocker(&mut self, parent_token_id: TokenId, fact_id: FactId) -> bool {
        let mut now_unblocked = false;

        if let Some(blockers) = self.blocked.get_mut(&parent_token_id) {
            blockers.remove(&fact_id);
            if blockers.is_empty() {
                self.blocked.remove(&parent_token_id);
                now_unblocked = true;
            }
        }

        if let Some(tokens) = self.fact_to_blocked.get_mut(&fact_id) {
            tokens.remove(&parent_token_id);
            if tokens.is_empty() {
                self.fact_to_blocked.remove(&fact_id);
            }
        }

        now_unblocked
    }

    /// Clear all blocker and unblocked tracking.
    pub fn clear(&mut self) {
        self.blocked.clear();
        self.fact_to_blocked.clear();
        self.unblocked.clear();
    }

    /// Check if a parent token is blocked.
    #[must_use]
    pub fn is_blocked(&self, parent_token_id: TokenId) -> bool {
        self.blocked.contains_key(&parent_token_id)
    }

    /// Get all parent tokens blocked by a specific fact.
    pub fn tokens_blocked_by(&self, fact_id: FactId) -> Vec<TokenId> {
        self.fact_to_blocked
            .get(&fact_id)
            .map(|tokens| tokens.iter().copied().collect())
            .unwrap_or_default()
    }

    /// Record a parent token as unblocked with its pass-through token.
    pub fn set_unblocked(&mut self, parent_token_id: TokenId, passthrough_token_id: TokenId) {
        self.unblocked.insert(parent_token_id, passthrough_token_id);
    }

    /// Get the pass-through token for an unblocked parent token.
    #[must_use]
    pub fn get_passthrough(&self, parent_token_id: TokenId) -> Option<TokenId> {
        self.unblocked.get(&parent_token_id).copied()
    }

    /// Remove the unblocked entry for a parent token.
    ///
    /// Returns the pass-through token ID if it was unblocked.
    pub fn remove_unblocked(&mut self, parent_token_id: TokenId) -> Option<TokenId> {
        self.unblocked.remove(&parent_token_id)
    }

    /// Check if a parent token is tracked as unblocked.
    #[must_use]
    pub fn is_unblocked(&self, parent_token_id: TokenId) -> bool {
        self.unblocked.contains_key(&parent_token_id)
    }

    /// Remove all tracking for a parent token (cleanup on parent retraction).
    ///
    /// Removes from both blocked and unblocked tracking.
    pub fn remove_parent_token(&mut self, parent_token_id: TokenId) {
        // Remove from unblocked
        self.unblocked.remove(&parent_token_id);

        // Remove from blocked (and clean up reverse index)
        if let Some(blockers) = self.blocked.remove(&parent_token_id) {
            for fact_id in blockers {
                if let Some(tokens) = self.fact_to_blocked.get_mut(&fact_id) {
                    tokens.remove(&parent_token_id);
                    if tokens.is_empty() {
                        self.fact_to_blocked.remove(&fact_id);
                    }
                }
            }
        }
    }

    /// Check if the negative memory has no entries.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.blocked.is_empty() && self.fact_to_blocked.is_empty() && self.unblocked.is_empty()
    }

    /// Get the number of blocked parent tokens.
    #[must_use]
    pub fn blocked_count(&self) -> usize {
        self.blocked.len()
    }

    /// Get the number of unblocked parent tokens.
    #[must_use]
    pub fn unblocked_count(&self) -> usize {
        self.unblocked.len()
    }

    /// Iterate over all unblocked parent token → pass-through token pairs.
    pub fn iter_unblocked(&self) -> impl Iterator<Item = (TokenId, TokenId)> + '_ {
        self.unblocked.iter().map(|(&parent, &pt)| (parent, pt))
    }

    /// Verify internal consistency of the negative memory.
    #[cfg(any(test, debug_assertions))]
    pub fn debug_assert_consistency(&self) {
        // Check 1: forward and reverse blocker indices are consistent
        for (&token_id, blockers) in &self.blocked {
            assert!(
                !blockers.is_empty(),
                "NegativeMemory {:?}: empty blocker set for token {token_id:?}",
                self.id
            );
            for &fact_id in blockers {
                let tokens = self.fact_to_blocked.get(&fact_id);
                assert!(
                    tokens.is_some_and(|t| t.contains(&token_id)),
                    "NegativeMemory {:?}: token {token_id:?} blocked by fact {fact_id:?} but reverse index missing",
                    self.id
                );
            }
        }

        for (&fact_id, tokens) in &self.fact_to_blocked {
            assert!(
                !tokens.is_empty(),
                "NegativeMemory {:?}: empty reverse set for fact {fact_id:?}",
                self.id
            );
            for &token_id in tokens {
                let blockers = self.blocked.get(&token_id);
                assert!(
                    blockers.is_some_and(|b| b.contains(&fact_id)),
                    "NegativeMemory {:?}: reverse index says fact {fact_id:?} blocks token {token_id:?} but forward missing",
                    self.id
                );
            }
        }

        // Check 2: no token is both blocked and unblocked
        for parent_token_id in self.unblocked.keys() {
            assert!(
                !self.blocked.contains_key(parent_token_id),
                "NegativeMemory {:?}: parent token {parent_token_id:?} is both blocked and unblocked",
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
    fn new_negative_memory_is_empty() {
        let mem = NegativeMemory::new(NegativeMemoryId(0));
        assert!(mem.is_empty());
        assert_eq!(mem.blocked_count(), 0);
        assert_eq!(mem.unblocked_count(), 0);
    }

    #[test]
    fn add_single_blocker() {
        let mut mem = NegativeMemory::new(NegativeMemoryId(0));
        let tokens = make_token_ids(1);
        let facts = make_fact_ids(1);

        mem.add_blocker(tokens[0], facts[0]);

        assert!(mem.is_blocked(tokens[0]));
        assert_eq!(mem.blocked_count(), 1);

        let blocked_by = mem.tokens_blocked_by(facts[0]);
        assert_eq!(blocked_by.len(), 1);
        assert!(blocked_by.contains(&tokens[0]));

        mem.debug_assert_consistency();
    }

    #[test]
    fn add_multiple_blockers_for_same_token() {
        let mut mem = NegativeMemory::new(NegativeMemoryId(0));
        let tokens = make_token_ids(1);
        let facts = make_fact_ids(3);

        mem.add_blocker(tokens[0], facts[0]);
        mem.add_blocker(tokens[0], facts[1]);
        mem.add_blocker(tokens[0], facts[2]);

        assert!(mem.is_blocked(tokens[0]));
        assert_eq!(mem.blocked_count(), 1);

        mem.debug_assert_consistency();
    }

    #[test]
    fn remove_blocker_returns_false_when_still_blocked() {
        let mut mem = NegativeMemory::new(NegativeMemoryId(0));
        let tokens = make_token_ids(1);
        let facts = make_fact_ids(2);

        mem.add_blocker(tokens[0], facts[0]);
        mem.add_blocker(tokens[0], facts[1]);

        let unblocked = mem.remove_blocker(tokens[0], facts[0]);
        assert!(!unblocked);
        assert!(mem.is_blocked(tokens[0]));

        mem.debug_assert_consistency();
    }

    #[test]
    fn remove_last_blocker_returns_true() {
        let mut mem = NegativeMemory::new(NegativeMemoryId(0));
        let tokens = make_token_ids(1);
        let facts = make_fact_ids(1);

        mem.add_blocker(tokens[0], facts[0]);

        let unblocked = mem.remove_blocker(tokens[0], facts[0]);
        assert!(unblocked);
        assert!(!mem.is_blocked(tokens[0]));
        assert!(mem.is_empty());

        mem.debug_assert_consistency();
    }

    #[test]
    fn unblocked_tracking() {
        let mut mem = NegativeMemory::new(NegativeMemoryId(0));
        let tokens = make_token_ids(2);

        mem.set_unblocked(tokens[0], tokens[1]);

        assert!(mem.is_unblocked(tokens[0]));
        assert_eq!(mem.get_passthrough(tokens[0]), Some(tokens[1]));
        assert_eq!(mem.unblocked_count(), 1);

        mem.debug_assert_consistency();
    }

    #[test]
    fn remove_unblocked_returns_passthrough() {
        let mut mem = NegativeMemory::new(NegativeMemoryId(0));
        let tokens = make_token_ids(2);

        mem.set_unblocked(tokens[0], tokens[1]);

        let pt = mem.remove_unblocked(tokens[0]);
        assert_eq!(pt, Some(tokens[1]));
        assert!(!mem.is_unblocked(tokens[0]));
        assert!(mem.is_empty());
    }

    #[test]
    fn remove_parent_token_cleans_blocked() {
        let mut mem = NegativeMemory::new(NegativeMemoryId(0));
        let tokens = make_token_ids(1);
        let facts = make_fact_ids(2);

        mem.add_blocker(tokens[0], facts[0]);
        mem.add_blocker(tokens[0], facts[1]);

        mem.remove_parent_token(tokens[0]);

        assert!(!mem.is_blocked(tokens[0]));
        assert!(mem.tokens_blocked_by(facts[0]).is_empty());
        assert!(mem.tokens_blocked_by(facts[1]).is_empty());
        assert!(mem.is_empty());

        mem.debug_assert_consistency();
    }

    #[test]
    fn remove_parent_token_cleans_unblocked() {
        let mut mem = NegativeMemory::new(NegativeMemoryId(0));
        let tokens = make_token_ids(2);

        mem.set_unblocked(tokens[0], tokens[1]);

        mem.remove_parent_token(tokens[0]);

        assert!(!mem.is_unblocked(tokens[0]));
        assert!(mem.is_empty());
    }

    #[test]
    fn same_fact_blocks_multiple_tokens() {
        let mut mem = NegativeMemory::new(NegativeMemoryId(0));
        let tokens = make_token_ids(3);
        let facts = make_fact_ids(1);

        mem.add_blocker(tokens[0], facts[0]);
        mem.add_blocker(tokens[1], facts[0]);
        mem.add_blocker(tokens[2], facts[0]);

        let blocked_by = mem.tokens_blocked_by(facts[0]);
        assert_eq!(blocked_by.len(), 3);

        mem.debug_assert_consistency();
    }

    #[test]
    fn iter_unblocked_returns_all_entries() {
        let mut mem = NegativeMemory::new(NegativeMemoryId(0));
        let tokens = make_token_ids(4);

        mem.set_unblocked(tokens[0], tokens[1]);
        mem.set_unblocked(tokens[2], tokens[3]);

        let entries: Vec<_> = mem.iter_unblocked().collect();
        assert_eq!(entries.len(), 2);

        mem.debug_assert_consistency();
    }
}

#[cfg(test)]
mod proptests {
    use super::*;
    use proptest::prelude::*;
    use slotmap::SlotMap;

    proptest! {
        /// Adding and then removing all blockers returns to empty state.
        #[test]
        fn add_then_remove_all_blockers_is_clean(
            blocker_count in 1..20_usize
        ) {
            let mut mem = NegativeMemory::new(NegativeMemoryId(0));
            let mut token_map: SlotMap<TokenId, ()> = SlotMap::with_key();
            let mut fact_map: SlotMap<FactId, ()> = SlotMap::with_key();

            let token = token_map.insert(());
            let facts: Vec<FactId> = (0..blocker_count).map(|_| fact_map.insert(())).collect();

            for &fact in &facts {
                mem.add_blocker(token, fact);
            }

            prop_assert!(mem.is_blocked(token));
            prop_assert_eq!(mem.blocked_count(), 1);

            for (i, &fact) in facts.iter().enumerate() {
                let unblocked = mem.remove_blocker(token, fact);
                if i < facts.len() - 1 {
                    prop_assert!(!unblocked, "should still be blocked with {} blockers remaining", facts.len() - i - 1);
                } else {
                    prop_assert!(unblocked, "should be unblocked after removing last blocker");
                }
            }

            prop_assert!(mem.is_empty());
            mem.debug_assert_consistency();
        }

        /// `remove_parent_token` always results in a consistent state.
        #[test]
        fn remove_parent_always_consistent(
            blocker_count in 0..10_usize,
            has_unblocked in any::<bool>()
        ) {
            let mut mem = NegativeMemory::new(NegativeMemoryId(0));
            let mut token_map: SlotMap<TokenId, ()> = SlotMap::with_key();
            let mut fact_map: SlotMap<FactId, ()> = SlotMap::with_key();

            let parent = token_map.insert(());
            let passthrough = token_map.insert(());

            if has_unblocked {
                mem.set_unblocked(parent, passthrough);
            } else {
                for _ in 0..blocker_count {
                    let fact = fact_map.insert(());
                    mem.add_blocker(parent, fact);
                }
            }

            mem.remove_parent_token(parent);

            prop_assert!(!mem.is_blocked(parent));
            prop_assert!(!mem.is_unblocked(parent));
            mem.debug_assert_consistency();
        }
    }
}
