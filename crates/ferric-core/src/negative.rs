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
use smallvec::SmallVec;

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
    pub fn tokens_blocked_by(&self, fact_id: FactId) -> SmallVec<[TokenId; 4]> {
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

    // ---------------------------------------------------------------------------
    // Operation enum and shadow model
    // ---------------------------------------------------------------------------

    /// Operations that can be applied to a `NegativeMemory`.
    #[derive(Clone, Debug)]
    enum Op {
        AddBlocker {
            token_idx: usize,
            fact_idx: usize,
        },
        RemoveBlocker {
            token_idx: usize,
            fact_idx: usize,
        },
        SetUnblocked {
            token_idx: usize,
            passthrough_idx: usize,
        },
        RemoveUnblocked {
            token_idx: usize,
        },
        RemoveParent {
            token_idx: usize,
        },
    }

    /// Shadow model that mirrors the expected state of `NegativeMemory`.
    #[derive(Default)]
    struct Model {
        /// `token_idx` -> set of `fact_idxs` blocking it
        blocked: HashMap<usize, HashSet<usize>>,
        /// `token_idx` -> passthrough `token_idx`
        unblocked: HashMap<usize, usize>,
    }

    impl Model {
        fn apply(&mut self, op: &Op) {
            match op {
                Op::AddBlocker {
                    token_idx,
                    fact_idx,
                } => {
                    // Mirror the guard in apply_op: skip when already unblocked.
                    if !self.unblocked.contains_key(token_idx) {
                        self.blocked
                            .entry(*token_idx)
                            .or_default()
                            .insert(*fact_idx);
                    }
                }
                Op::RemoveBlocker {
                    token_idx,
                    fact_idx,
                } => {
                    if let Some(facts) = self.blocked.get_mut(token_idx) {
                        facts.remove(fact_idx);
                        if facts.is_empty() {
                            self.blocked.remove(token_idx);
                        }
                    }
                }
                Op::SetUnblocked {
                    token_idx,
                    passthrough_idx,
                } => {
                    // Mirror the guard in apply_op: skip when already blocked.
                    if !self.blocked.contains_key(token_idx) {
                        self.unblocked.insert(*token_idx, *passthrough_idx);
                    }
                }
                Op::RemoveUnblocked { token_idx } => {
                    self.unblocked.remove(token_idx);
                }
                Op::RemoveParent { token_idx } => {
                    self.blocked.remove(token_idx);
                    self.unblocked.remove(token_idx);
                }
            }
        }

        fn is_blocked(&self, token_idx: usize) -> bool {
            self.blocked.contains_key(&token_idx)
        }

        fn is_empty(&self) -> bool {
            self.blocked.is_empty() && self.unblocked.is_empty()
        }
    }

    // ---------------------------------------------------------------------------
    // Strategy helpers
    // ---------------------------------------------------------------------------

    const POOL: usize = 5;

    fn op_strategy() -> impl Strategy<Value = Op> {
        prop_oneof![
            // AddBlocker
            (0..POOL, 0..POOL).prop_map(|(t, f)| Op::AddBlocker {
                token_idx: t,
                fact_idx: f,
            }),
            // RemoveBlocker
            (0..POOL, 0..POOL).prop_map(|(t, f)| Op::RemoveBlocker {
                token_idx: t,
                fact_idx: f,
            }),
            // SetUnblocked (passthrough from same pool but different slot is fine)
            (0..POOL, 0..POOL).prop_map(|(t, p)| Op::SetUnblocked {
                token_idx: t,
                passthrough_idx: p,
            }),
            // RemoveUnblocked
            (0..POOL).prop_map(|t| Op::RemoveUnblocked { token_idx: t }),
            // RemoveParent
            (0..POOL).prop_map(|t| Op::RemoveParent { token_idx: t }),
        ]
    }

    /// Apply an `Op` to a `NegativeMemory`, respecting the mutual-exclusivity
    /// invariant: a token cannot be simultaneously blocked and unblocked.
    ///
    /// - `AddBlocker` is skipped when the token is currently unblocked.
    /// - `SetUnblocked` is skipped when the token is currently blocked.
    ///
    /// Returns `Some(bool)` (the return value of `remove_blocker`) when a
    /// `RemoveBlocker` op was applied; `None` for all other ops.
    fn apply_op(
        mem: &mut NegativeMemory,
        tokens: &[TokenId],
        facts: &[FactId],
        op: &Op,
    ) -> Option<bool> {
        match op {
            Op::AddBlocker {
                token_idx,
                fact_idx,
            } => {
                // Guard: do not add a blocker to an already-unblocked token.
                if !mem.is_unblocked(tokens[*token_idx]) {
                    mem.add_blocker(tokens[*token_idx], facts[*fact_idx]);
                }
                None
            }
            Op::RemoveBlocker {
                token_idx,
                fact_idx,
            } => Some(mem.remove_blocker(tokens[*token_idx], facts[*fact_idx])),
            Op::SetUnblocked {
                token_idx,
                passthrough_idx,
            } => {
                // Guard: do not mark a blocked token as unblocked.
                if !mem.is_blocked(tokens[*token_idx]) {
                    mem.set_unblocked(tokens[*token_idx], tokens[*passthrough_idx]);
                }
                None
            }
            Op::RemoveUnblocked { token_idx } => {
                mem.remove_unblocked(tokens[*token_idx]);
                None
            }
            Op::RemoveParent { token_idx } => {
                mem.remove_parent_token(tokens[*token_idx]);
                None
            }
        }
    }

    // ---------------------------------------------------------------------------
    // Property tests
    // ---------------------------------------------------------------------------

    proptest! {
        /// 1. Arbitrary operations maintain internal consistency after every op.
        #[test]
        fn arbitrary_ops_maintain_consistency(
            ops in proptest::collection::vec(op_strategy(), 0..100)
        ) {
            let mut mem = NegativeMemory::new(NegativeMemoryId(0));
            let mut token_map: SlotMap<TokenId, ()> = SlotMap::with_key();
            let mut fact_map: SlotMap<FactId, ()> = SlotMap::with_key();
            let tokens: Vec<TokenId> = (0..POOL).map(|_| token_map.insert(())).collect();
            let facts: Vec<FactId> = (0..POOL).map(|_| fact_map.insert(())).collect();

            for op in &ops {
                apply_op(&mut mem, &tokens, &facts, op);
                mem.debug_assert_consistency();
            }
        }

        /// 2. Shadow model matches implementation after arbitrary operations.
        #[test]
        fn model_matches_implementation(
            ops in proptest::collection::vec(op_strategy(), 0..100)
        ) {
            let mut mem = NegativeMemory::new(NegativeMemoryId(0));
            let mut model = Model::default();
            let mut token_map: SlotMap<TokenId, ()> = SlotMap::with_key();
            let mut fact_map: SlotMap<FactId, ()> = SlotMap::with_key();
            let tokens: Vec<TokenId> = (0..POOL).map(|_| token_map.insert(())).collect();
            let facts: Vec<FactId> = (0..POOL).map(|_| fact_map.insert(())).collect();

            for op in &ops {
                apply_op(&mut mem, &tokens, &facts, op);
                model.apply(op);
            }

            // Verify is_blocked for every token in the pool
            for (i, &token_idx) in tokens.iter().enumerate().take(POOL) {
                prop_assert_eq!(
                    mem.is_blocked(token_idx),
                    model.is_blocked(i),
                    "is_blocked mismatch for token_idx {}", i
                );
            }

            // Verify blocked_count matches the shadow model
            prop_assert_eq!(
                mem.blocked_count(),
                model.blocked.len(),
                "blocked_count mismatch"
            );

            // Verify unblocked_count matches the shadow model
            prop_assert_eq!(
                mem.unblocked_count(),
                model.unblocked.len(),
                "unblocked_count mismatch"
            );

            // Verify is_empty matches
            prop_assert_eq!(
                mem.is_empty(),
                model.is_empty(),
                "is_empty mismatch"
            );
        }

        /// 3. remove_blocker returns true iff the token's blocker set becomes empty.
        #[test]
        fn remove_blocker_return_value_semantics(
            ops in proptest::collection::vec(op_strategy(), 0..100)
        ) {
            let mut mem = NegativeMemory::new(NegativeMemoryId(0));
            let mut model = Model::default();
            let mut token_map: SlotMap<TokenId, ()> = SlotMap::with_key();
            let mut fact_map: SlotMap<FactId, ()> = SlotMap::with_key();
            let tokens: Vec<TokenId> = (0..POOL).map(|_| token_map.insert(())).collect();
            let facts: Vec<FactId> = (0..POOL).map(|_| fact_map.insert(())).collect();

            for op in &ops {
                if let Op::RemoveBlocker { token_idx, fact_idx } = op {
                    // Compute expected return value from shadow model BEFORE applying
                    let expected_unblocked = model
                        .blocked
                        .get(token_idx)
                        .is_some_and(|s| s.contains(fact_idx) && s.len() == 1);

                    let actual = mem.remove_blocker(tokens[*token_idx], facts[*fact_idx]);
                    model.apply(op);

                    prop_assert_eq!(
                        actual,
                        expected_unblocked,
                        "remove_blocker return value wrong for token_idx={}, fact_idx={}", token_idx, fact_idx
                    );
                } else {
                    apply_op(&mut mem, &tokens, &facts, op);
                    model.apply(op);
                }
            }
        }

        /// 4. Adding N blockers then removing them all yields empty state with correct return values.
        #[test]
        fn add_remove_roundtrip(
            fact_indices in proptest::collection::vec(0..POOL, 1..10)
        ) {
            let mut mem = NegativeMemory::new(NegativeMemoryId(0));
            let mut token_map: SlotMap<TokenId, ()> = SlotMap::with_key();
            let mut fact_map: SlotMap<FactId, ()> = SlotMap::with_key();
            let tokens: Vec<TokenId> = (0..POOL).map(|_| token_map.insert(())).collect();
            let facts: Vec<FactId> = (0..POOL).map(|_| fact_map.insert(())).collect();

            // Deduplicate indices so we know exactly how many distinct facts are added
            let mut unique_indices: Vec<usize> = fact_indices.clone();
            unique_indices.sort_unstable();
            unique_indices.dedup();

            for &fi in &unique_indices {
                mem.add_blocker(tokens[0], facts[fi]);
            }

            prop_assert!(mem.is_blocked(tokens[0]));
            prop_assert_eq!(mem.blocked_count(), 1);

            for (i, &fi) in unique_indices.iter().enumerate() {
                let last = i == unique_indices.len() - 1;
                let unblocked = mem.remove_blocker(tokens[0], facts[fi]);
                prop_assert_eq!(
                    unblocked,
                    last,
                    "remove_blocker at step {} (last={}) returned wrong value", i, last
                );
                mem.debug_assert_consistency();
            }

            prop_assert!(mem.is_empty());
        }

        /// 5. Adding the same (token, fact) pair multiple times is idempotent.
        #[test]
        fn idempotent_add_blocker(
            repeats in 1..10_usize,
            token_idx in 0..POOL,
            fact_idx in 0..POOL,
        ) {
            let mut mem = NegativeMemory::new(NegativeMemoryId(0));
            let mut token_map: SlotMap<TokenId, ()> = SlotMap::with_key();
            let mut fact_map: SlotMap<FactId, ()> = SlotMap::with_key();
            let tokens: Vec<TokenId> = (0..POOL).map(|_| token_map.insert(())).collect();
            let facts: Vec<FactId> = (0..POOL).map(|_| fact_map.insert(())).collect();

            for _ in 0..repeats {
                mem.add_blocker(tokens[token_idx], facts[fact_idx]);
            }

            // Regardless of how many times we added, exactly one fact blocks this token
            prop_assert_eq!(mem.blocked_count(), 1);
            // Removing once should return true (last blocker)
            let unblocked = mem.remove_blocker(tokens[token_idx], facts[fact_idx]);
            prop_assert!(unblocked, "expected true after removing only blocker");
            prop_assert!(mem.is_empty());
            mem.debug_assert_consistency();
        }

        /// 6. After remove_parent_token(t), t is neither blocked nor unblocked,
        ///    and tokens_blocked_by returns no reference to t for any fact.
        #[test]
        fn remove_parent_token_completeness(
            blocker_fact_indices in proptest::collection::vec(0..POOL, 0..5),
            is_unblocked_first in any::<bool>(),
            passthrough_idx in 0..POOL,
            target_idx in 0..POOL,
        ) {
            let mut mem = NegativeMemory::new(NegativeMemoryId(0));
            let mut token_map: SlotMap<TokenId, ()> = SlotMap::with_key();
            let mut fact_map: SlotMap<FactId, ()> = SlotMap::with_key();
            let tokens: Vec<TokenId> = (0..POOL).map(|_| token_map.insert(())).collect();
            let facts: Vec<FactId> = (0..POOL).map(|_| fact_map.insert(())).collect();

            if is_unblocked_first {
                mem.set_unblocked(tokens[target_idx], tokens[passthrough_idx]);
            } else {
                for &fi in &blocker_fact_indices {
                    mem.add_blocker(tokens[target_idx], facts[fi]);
                }
            }

            mem.remove_parent_token(tokens[target_idx]);

            prop_assert!(!mem.is_blocked(tokens[target_idx]));
            prop_assert!(!mem.is_unblocked(tokens[target_idx]));

            for (fi, &fact) in facts.iter().enumerate().take(POOL) {
                let blocked_by = mem.tokens_blocked_by(fact);
                prop_assert!(
                    !blocked_by.contains(&tokens[target_idx]),
                    "tokens_blocked_by(fact {fi}) still contains the removed parent"
                );
            }

            mem.debug_assert_consistency();
        }

        /// 7. Removing one parent token with a shared fact blocker does not affect
        ///    other tokens that share the same fact.
        #[test]
        fn remove_parent_with_shared_facts(
            shared_fact_idx in 0..POOL,
            token_a_idx in 0..POOL,
            token_b_idx in 0..POOL,
        ) {
            // Skip when both indices are the same — there's only one token to test
            prop_assume!(token_a_idx != token_b_idx);

            let mut mem = NegativeMemory::new(NegativeMemoryId(0));
            let mut token_map: SlotMap<TokenId, ()> = SlotMap::with_key();
            let mut fact_map: SlotMap<FactId, ()> = SlotMap::with_key();
            let tokens: Vec<TokenId> = (0..POOL).map(|_| token_map.insert(())).collect();
            let facts: Vec<FactId> = (0..POOL).map(|_| fact_map.insert(())).collect();

            // Both tokens blocked by the same fact
            mem.add_blocker(tokens[token_a_idx], facts[shared_fact_idx]);
            mem.add_blocker(tokens[token_b_idx], facts[shared_fact_idx]);

            // Remove token A's parent tracking
            mem.remove_parent_token(tokens[token_a_idx]);

            // Token B should still be blocked by the shared fact
            prop_assert!(
                mem.is_blocked(tokens[token_b_idx]),
                "token_b should still be blocked after token_a removal"
            );
            let still_blocked = mem.tokens_blocked_by(facts[shared_fact_idx]);
            prop_assert!(
                still_blocked.contains(&tokens[token_b_idx]),
                "fact should still block token_b"
            );
            prop_assert!(
                !still_blocked.contains(&tokens[token_a_idx]),
                "fact should no longer list token_a"
            );

            mem.debug_assert_consistency();
        }

        /// 8. No empty HashSets remain after operations (verified via consistency check).
        #[test]
        fn empty_set_pruning(
            ops in proptest::collection::vec(op_strategy(), 0..80)
        ) {
            let mut mem = NegativeMemory::new(NegativeMemoryId(0));
            let mut token_map: SlotMap<TokenId, ()> = SlotMap::with_key();
            let mut fact_map: SlotMap<FactId, ()> = SlotMap::with_key();
            let tokens: Vec<TokenId> = (0..POOL).map(|_| token_map.insert(())).collect();
            let facts: Vec<FactId> = (0..POOL).map(|_| fact_map.insert(())).collect();

            for op in &ops {
                apply_op(&mut mem, &tokens, &facts, op);
                // debug_assert_consistency verifies no empty sets exist
                mem.debug_assert_consistency();
            }
        }

        /// 9. No token appears in both blocked and unblocked simultaneously.
        ///    (Validated via debug_assert_consistency; also verified directly here.)
        #[test]
        fn blocked_unblocked_mutual_exclusivity(
            ops in proptest::collection::vec(op_strategy(), 0..100)
        ) {
            let mut mem = NegativeMemory::new(NegativeMemoryId(0));
            let mut token_map: SlotMap<TokenId, ()> = SlotMap::with_key();
            let mut fact_map: SlotMap<FactId, ()> = SlotMap::with_key();
            let tokens: Vec<TokenId> = (0..POOL).map(|_| token_map.insert(())).collect();
            let facts: Vec<FactId> = (0..POOL).map(|_| fact_map.insert(())).collect();

            for op in &ops {
                apply_op(&mut mem, &tokens, &facts, op);
            }

            // Direct check across every pool token
            for (i, &token_idx) in tokens.iter().enumerate().take(POOL) {
                let blocked = mem.is_blocked(token_idx);
                let unblocked = mem.is_unblocked(token_idx);
                prop_assert!(
                    !(blocked && unblocked),
                    "token_idx {i} is simultaneously blocked and unblocked"
                );
            }

            // Consistency check covers the same invariant for all stored tokens
            mem.debug_assert_consistency();
        }

        /// 10. tokens_blocked_by(fact) returns exactly the set of tokens the shadow
        ///     model says are blocked by that fact.
        #[test]
        fn tokens_blocked_by_completeness(
            ops in proptest::collection::vec(op_strategy(), 0..100)
        ) {
            let mut mem = NegativeMemory::new(NegativeMemoryId(0));
            let mut model = Model::default();
            let mut token_map: SlotMap<TokenId, ()> = SlotMap::with_key();
            let mut fact_map: SlotMap<FactId, ()> = SlotMap::with_key();
            let tokens: Vec<TokenId> = (0..POOL).map(|_| token_map.insert(())).collect();
            let facts: Vec<FactId> = (0..POOL).map(|_| fact_map.insert(())).collect();

            for op in &ops {
                apply_op(&mut mem, &tokens, &facts, op);
                model.apply(op);
            }

            for (fi, &fact) in facts.iter().enumerate().take(POOL) {
                // Build expected set from shadow model
                let expected: HashSet<TokenId> = (0..POOL)
                    .filter(|&ti| {
                        model.blocked.get(&ti).is_some_and(|s| s.contains(&fi))
                    })
                    .map(|ti| tokens[ti])
                    .collect();

                let actual: HashSet<TokenId> = mem.tokens_blocked_by(fact).into_iter().collect();

                prop_assert_eq!(
                    actual,
                    expected,
                    "tokens_blocked_by mismatch for fact_idx {}", fi
                );
            }
        }

        /// 11. iter_unblocked returns exactly the entries in the shadow model's unblocked map.
        #[test]
        fn iter_unblocked_completeness(
            ops in proptest::collection::vec(op_strategy(), 0..100)
        ) {
            let mut mem = NegativeMemory::new(NegativeMemoryId(0));
            let mut model = Model::default();
            let mut token_map: SlotMap<TokenId, ()> = SlotMap::with_key();
            let mut fact_map: SlotMap<FactId, ()> = SlotMap::with_key();
            let tokens: Vec<TokenId> = (0..POOL).map(|_| token_map.insert(())).collect();
            let facts: Vec<FactId> = (0..POOL).map(|_| fact_map.insert(())).collect();

            // fact_map must be used to satisfy the borrow checker (pool construction)
            let _ = &facts;

            for op in &ops {
                apply_op(&mut mem, &tokens, &facts, op);
                model.apply(op);
            }

            // Build expected set from shadow model
            let expected: HashSet<(TokenId, TokenId)> = model
                .unblocked
                .iter()
                .map(|(&ti, &pi)| (tokens[ti], tokens[pi]))
                .collect();

            let actual: HashSet<(TokenId, TokenId)> = mem.iter_unblocked().collect();

            prop_assert_eq!(actual, expected, "iter_unblocked entries do not match shadow model");
        }

        /// 12. After clear(), is_empty is true and all counts are zero.
        #[test]
        fn clear_resets_everything(
            ops in proptest::collection::vec(op_strategy(), 0..50)
        ) {
            let mut mem = NegativeMemory::new(NegativeMemoryId(0));
            let mut token_map: SlotMap<TokenId, ()> = SlotMap::with_key();
            let mut fact_map: SlotMap<FactId, ()> = SlotMap::with_key();
            let tokens: Vec<TokenId> = (0..POOL).map(|_| token_map.insert(())).collect();
            let facts: Vec<FactId> = (0..POOL).map(|_| fact_map.insert(())).collect();

            for op in &ops {
                apply_op(&mut mem, &tokens, &facts, op);
            }

            mem.clear();

            prop_assert!(mem.is_empty(), "is_empty should be true after clear");
            prop_assert_eq!(mem.blocked_count(), 0, "blocked_count should be 0 after clear");
            prop_assert_eq!(mem.unblocked_count(), 0, "unblocked_count should be 0 after clear");

            // Consistency check must also pass on the cleared memory
            mem.debug_assert_consistency();

            // iter_unblocked should yield nothing
            let unblocked_entries: Vec<_> = mem.iter_unblocked().collect();
            prop_assert!(unblocked_entries.is_empty(), "iter_unblocked should be empty after clear");

            // No token should appear blocked or unblocked
            for &token_idx in tokens.iter().take(POOL) {
                prop_assert!(!mem.is_blocked(token_idx));
                prop_assert!(!mem.is_unblocked(token_idx));
            }
        }
    }
}
