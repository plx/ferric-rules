//! NCC (Negated Conjunctive Condition) memory: tracks subnetwork results for NCC nodes.
//!
//! In Rete, NCC implements `(not (and (pattern1) (pattern2) ...))` by building
//! a "subnetwork" of join nodes that evaluate the conjunction. An NCC node in
//! the main beta network watches the subnetwork's results:
//!
//! - When a parent token enters the NCC node, we check the subnetwork results
//! - If NO results exist for this parent (result count = 0), the token is
//!   "unblocked" and propagates downstream via a pass-through token
//! - If ANY results exist (count > 0), the token is "blocked" and does not propagate
//!
//! When subnetwork results are added/removed, we update the result count:
//! - Count transitions 0→N: block the parent (retract its pass-through)
//! - Count transitions N→0: unblock the parent (create pass-through)
//!
//! ## NCC partner
//!
//! The NCC partner node sits at the bottom of the subnetwork. When a token
//! reaches the partner, it signals the NCC node to increment the result count
//! for the corresponding parent token.
//!
//! ## Phase 2 implementation
//!
//! - Pass 010: NCC node, NCC partner, and NCC memory

use std::collections::HashMap;

use crate::token::TokenId;

/// Unique identifier for an NCC memory.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct NccMemoryId(pub u32);

/// NCC memory tracks the relationship between parent tokens and subnetwork results.
///
/// For each parent token entering the NCC node, we track how many subnetwork
/// result tokens exist. When the count is 0, the parent token is "unblocked"
/// and has a pass-through token propagated downstream. When count > 0, the
/// parent is "blocked."
pub struct NccMemory {
    pub id: NccMemoryId,
    /// Parent token → count of subnetwork result tokens
    result_count: HashMap<TokenId, usize>,
    /// Subnetwork result token → NCC parent token it blocks.
    result_owner: HashMap<TokenId, TokenId>,
    /// Parent token → pass-through token (when unblocked, count == 0)
    unblocked: HashMap<TokenId, TokenId>,
}

impl NccMemory {
    /// Create a new, empty NCC memory.
    #[must_use]
    pub fn new(id: NccMemoryId) -> Self {
        Self {
            id,
            result_count: HashMap::new(),
            result_owner: HashMap::new(),
            unblocked: HashMap::new(),
        }
    }

    /// Increment the result count for a parent token.
    ///
    /// Returns the new count.
    pub fn increment_results(&mut self, parent_token_id: TokenId) -> usize {
        let count = self.result_count.entry(parent_token_id).or_insert(0);
        *count += 1;
        *count
    }

    /// Add a concrete subnetwork result token for a parent token.
    ///
    /// Returns `(old_count, new_count)` for the parent token.
    pub fn add_result(
        &mut self,
        parent_token_id: TokenId,
        result_token_id: TokenId,
    ) -> (usize, usize) {
        if let Some(existing_parent) = self.result_owner.get(&result_token_id) {
            let current = self.result_count(*existing_parent);
            return (current, current);
        }

        let old_count = self.result_count(parent_token_id);
        let new_count = self.increment_results(parent_token_id);
        self.result_owner.insert(result_token_id, parent_token_id);
        (old_count, new_count)
    }

    /// Decrement the result count for a parent token.
    ///
    /// Returns the new count. Caller should check if count went to 0 (unblocked).
    pub fn decrement_results(&mut self, parent_token_id: TokenId) -> usize {
        if let Some(count) = self.result_count.get_mut(&parent_token_id) {
            if *count > 0 {
                *count -= 1;
            }
            let new_count = *count;
            if new_count == 0 {
                self.result_count.remove(&parent_token_id);
            }
            new_count
        } else {
            0
        }
    }

    /// Remove a concrete subnetwork result token.
    ///
    /// Returns `(parent_token_id, new_count)` if the token was tracked.
    pub fn remove_result(&mut self, result_token_id: TokenId) -> Option<(TokenId, usize)> {
        let parent_token_id = self.result_owner.remove(&result_token_id)?;
        let new_count = self.decrement_results(parent_token_id);
        Some((parent_token_id, new_count))
    }

    /// Get the current result count for a parent token.
    ///
    /// Returns 0 if the parent token is not tracked.
    #[must_use]
    pub fn result_count(&self, parent_token_id: TokenId) -> usize {
        self.result_count
            .get(&parent_token_id)
            .copied()
            .unwrap_or(0)
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

    /// Check if a parent token is currently blocked.
    ///
    /// A parent is blocked if it has a non-zero result count.
    #[must_use]
    pub fn is_blocked(&self, parent_token_id: TokenId) -> bool {
        self.result_count(parent_token_id) > 0
    }

    /// Remove all tracking for a parent token (cleanup on parent retraction).
    pub fn remove_parent_token(&mut self, parent_token_id: TokenId) {
        self.result_count.remove(&parent_token_id);
        self.result_owner
            .retain(|_, owner_parent| *owner_parent != parent_token_id);
        self.unblocked.remove(&parent_token_id);
    }

    /// Clear all state from this NCC memory.
    pub fn clear(&mut self) {
        self.result_count.clear();
        self.result_owner.clear();
        self.unblocked.clear();
    }

    /// Check if the NCC memory has no entries.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.result_count.is_empty() && self.result_owner.is_empty() && self.unblocked.is_empty()
    }

    /// Verify internal consistency of the NCC memory.
    #[cfg(any(test, debug_assertions))]
    pub fn debug_assert_consistency(&self) {
        // Check 1: no token is both in result_count (blocked) and unblocked
        for parent_token_id in self.unblocked.keys() {
            assert!(
                !self.result_count.contains_key(parent_token_id),
                "NccMemory {:?}: parent token {parent_token_id:?} is both blocked (count > 0) and unblocked",
                self.id
            );
        }

        // Check 2: all entries in result_count should have count > 0
        for (&token_id, &count) in &self.result_count {
            assert!(
                count > 0,
                "NccMemory {:?}: token {token_id:?} has zero count in result_count map",
                self.id
            );
        }

        // Check 3: each tracked result token points to a parent with a non-zero count.
        let mut per_parent_results: HashMap<TokenId, usize> = HashMap::new();
        for (&result_token, &parent_token) in &self.result_owner {
            let _ = result_token;
            assert!(
                self.result_count.contains_key(&parent_token),
                "NccMemory {:?}: result token references parent {parent_token:?} with no count entry",
                self.id
            );
            *per_parent_results.entry(parent_token).or_insert(0) += 1;
        }

        // Check 4: parent counts match tracked result tokens.
        for (&parent_token, &count) in &self.result_count {
            let tracked = per_parent_results.get(&parent_token).copied().unwrap_or(0);
            assert_eq!(
                tracked, count,
                "NccMemory {:?}: parent {parent_token:?} count mismatch: count={count}, tracked-results={tracked}",
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

    #[test]
    fn new_ncc_memory_is_empty() {
        let mem = NccMemory::new(NccMemoryId(0));
        assert!(mem.is_empty());
        assert_eq!(mem.result_count(make_token_ids(1)[0]), 0);
    }

    #[test]
    fn increment_from_zero_returns_one() {
        let mut mem = NccMemory::new(NccMemoryId(0));
        let tokens = make_token_ids(2);

        let (old_count, new_count) = mem.add_result(tokens[0], tokens[1]);
        assert_eq!(old_count, 0);
        assert_eq!(new_count, 1);
        assert_eq!(mem.result_count(tokens[0]), 1);
        assert!(mem.is_blocked(tokens[0]));

        mem.debug_assert_consistency();
    }

    #[test]
    fn increment_from_one_returns_two() {
        let mut mem = NccMemory::new(NccMemoryId(0));
        let tokens = make_token_ids(3);

        let (_old, _new) = mem.add_result(tokens[0], tokens[1]);
        let (old_count, new_count) = mem.add_result(tokens[0], tokens[2]);
        assert_eq!(old_count, 1);
        assert_eq!(new_count, 2);
        assert_eq!(mem.result_count(tokens[0]), 2);

        mem.debug_assert_consistency();
    }

    #[test]
    fn decrement_from_two_returns_one() {
        let mut mem = NccMemory::new(NccMemoryId(0));
        let tokens = make_token_ids(3);

        let (_old, _new) = mem.add_result(tokens[0], tokens[1]);
        let (_old, _new) = mem.add_result(tokens[0], tokens[2]);

        let removed = mem.remove_result(tokens[2]);
        assert_eq!(removed, Some((tokens[0], 1)));
        assert!(mem.is_blocked(tokens[0]));

        mem.debug_assert_consistency();
    }

    #[test]
    fn decrement_from_one_returns_zero() {
        let mut mem = NccMemory::new(NccMemoryId(0));
        let tokens = make_token_ids(2);

        let (_old, _new) = mem.add_result(tokens[0], tokens[1]);

        let removed = mem.remove_result(tokens[1]);
        assert_eq!(removed, Some((tokens[0], 0)));
        assert!(!mem.is_blocked(tokens[0]));

        mem.debug_assert_consistency();
    }

    #[test]
    fn set_and_get_unblocked() {
        let mut mem = NccMemory::new(NccMemoryId(0));
        let tokens = make_token_ids(2);

        mem.set_unblocked(tokens[0], tokens[1]);

        assert_eq!(mem.get_passthrough(tokens[0]), Some(tokens[1]));
        assert!(!mem.is_empty());

        mem.debug_assert_consistency();
    }

    #[test]
    fn remove_unblocked_returns_passthrough() {
        let mut mem = NccMemory::new(NccMemoryId(0));
        let tokens = make_token_ids(2);

        mem.set_unblocked(tokens[0], tokens[1]);
        let pt = mem.remove_unblocked(tokens[0]);

        assert_eq!(pt, Some(tokens[1]));
        assert_eq!(mem.get_passthrough(tokens[0]), None);
        assert!(mem.is_empty());
    }

    #[test]
    fn remove_parent_token_cleans_everything() {
        let mut mem = NccMemory::new(NccMemoryId(0));
        let tokens = make_token_ids(3);

        let (_old, _new) = mem.add_result(tokens[0], tokens[1]);
        let (_old, _new) = mem.add_result(tokens[0], tokens[2]);

        mem.remove_parent_token(tokens[0]);

        assert_eq!(mem.result_count(tokens[0]), 0);
        assert!(!mem.is_blocked(tokens[0]));
        assert!(mem.is_empty());

        mem.debug_assert_consistency();
    }

    #[test]
    fn remove_parent_token_cleans_unblocked() {
        let mut mem = NccMemory::new(NccMemoryId(0));
        let tokens = make_token_ids(2);

        mem.set_unblocked(tokens[0], tokens[1]);

        mem.remove_parent_token(tokens[0]);

        assert_eq!(mem.get_passthrough(tokens[0]), None);
        assert!(mem.is_empty());
    }

    #[test]
    fn consistency_check_passes_on_valid_state() {
        let mut mem = NccMemory::new(NccMemoryId(0));
        let tokens = make_token_ids(4);

        // Token 0: blocked with count 2
        let (_old, _new) = mem.add_result(tokens[0], tokens[1]);
        let (_old, _new) = mem.add_result(tokens[0], tokens[2]);

        // Token 1: unblocked with passthrough
        mem.set_unblocked(tokens[3], tokens[1]);

        mem.debug_assert_consistency();
    }
}
