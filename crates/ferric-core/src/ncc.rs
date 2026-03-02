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

use rustc_hash::FxHashMap as HashMap;

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
            result_count: HashMap::default(),
            result_owner: HashMap::default(),
            unblocked: HashMap::default(),
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
        let mut per_parent_results: HashMap<TokenId, usize> = HashMap::default();
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

#[cfg(test)]
mod proptests {
    use super::*;
    use proptest::prelude::*;
    use slotmap::SlotMap;

    // ---------------------------------------------------------------------------
    // Operation enum and shadow model
    // ---------------------------------------------------------------------------

    /// An operation that can be applied to an `NccMemory`.
    ///
    /// Indices into pre-allocated token pools (0..5) are used to refer to both
    /// parent tokens and result tokens, keeping the operation space finite.
    #[derive(Clone, Debug)]
    enum Op {
        /// Add a subnetwork result token (`result_idx`) for a parent token (`parent_idx`).
        AddResult {
            parent_idx: usize,
            result_idx: usize,
        },
        /// Remove a subnetwork result token by index.
        RemoveResult { result_idx: usize },
        /// Record a parent token as unblocked with a pass-through token.
        ///
        /// Only applied when the parent has `result_count == 0` to keep the
        /// blocked/unblocked invariant intact.
        SetUnblocked {
            parent_idx: usize,
            passthrough_idx: usize,
        },
        /// Remove the unblocked entry for a parent token.
        RemoveUnblocked { parent_idx: usize },
        /// Remove all tracking for a parent token (parent retraction).
        RemoveParentToken { parent_idx: usize },
        /// Clear everything.
        Clear,
    }

    /// Shadow model that mirrors the semantics of `NccMemory` using simple
    /// index-keyed maps, so we can verify against the real implementation.
    #[derive(Default)]
    struct Model {
        /// `parent_idx` to number of result tokens pointing to it
        result_count: std::collections::HashMap<usize, usize>,
        /// `result_idx` to `parent_idx` it blocks
        result_owner: std::collections::HashMap<usize, usize>,
        /// `parent_idx` to `passthrough_idx` (when unblocked, count == 0)
        unblocked: std::collections::HashMap<usize, usize>,
    }

    impl Model {
        fn result_count(&self, parent_idx: usize) -> usize {
            self.result_count.get(&parent_idx).copied().unwrap_or(0)
        }

        fn is_blocked(&self, parent_idx: usize) -> bool {
            self.result_count(parent_idx) > 0
        }

        fn get_passthrough(&self, parent_idx: usize) -> Option<usize> {
            self.unblocked.get(&parent_idx).copied()
        }

        fn add_result(&mut self, parent_idx: usize, result_idx: usize) -> (usize, usize) {
            // If this result token is already tracked, return current count unchanged.
            if self.result_owner.contains_key(&result_idx) {
                let existing_parent = self.result_owner[&result_idx];
                let cur = self.result_count(existing_parent);
                return (cur, cur);
            }
            let old_count = self.result_count(parent_idx);
            *self.result_count.entry(parent_idx).or_insert(0) += 1;
            self.result_owner.insert(result_idx, parent_idx);
            let new_count = self.result_count(parent_idx);
            // A newly-blocked parent must lose its unblocked entry
            if old_count == 0 && new_count > 0 {
                self.unblocked.remove(&parent_idx);
            }
            (old_count, new_count)
        }

        fn remove_result(&mut self, result_idx: usize) -> Option<(usize, usize)> {
            let parent_idx = self.result_owner.remove(&result_idx)?;
            let count = self.result_count.get_mut(&parent_idx)?;
            *count -= 1;
            let new_count = *count;
            if new_count == 0 {
                self.result_count.remove(&parent_idx);
            }
            Some((parent_idx, new_count))
        }

        fn remove_parent(&mut self, parent_idx: usize) {
            self.result_count.remove(&parent_idx);
            self.result_owner.retain(|_, owner| *owner != parent_idx);
            self.unblocked.remove(&parent_idx);
        }

        fn is_empty(&self) -> bool {
            self.result_count.is_empty()
                && self.result_owner.is_empty()
                && self.unblocked.is_empty()
        }
    }

    /// Strategy that produces individual `Op` values over pools of 5 tokens.
    fn op_strategy() -> impl Strategy<Value = Op> {
        prop_oneof![
            // Weighted toward mutations that are likely to be interesting
            3 => (0..5_usize, 0..5_usize).prop_map(|(p, r)| Op::AddResult { parent_idx: p, result_idx: r }),
            2 => (0..5_usize).prop_map(|r| Op::RemoveResult { result_idx: r }),
            2 => (0..5_usize, 0..5_usize).prop_map(|(p, pt)| Op::SetUnblocked { parent_idx: p, passthrough_idx: pt }),
            2 => (0..5_usize).prop_map(|p| Op::RemoveUnblocked { parent_idx: p }),
            2 => (0..5_usize).prop_map(|p| Op::RemoveParentToken { parent_idx: p }),
            1 => Just(Op::Clear),
        ]
    }

    /// Apply an operation to both the real `NccMemory` and the shadow `Model`.
    ///
    /// Where the operation has a precondition (e.g., `SetUnblocked` requires
    /// count == 0), we check the model first and skip if the precondition is unmet.
    fn apply_op(op: &Op, mem: &mut NccMemory, model: &mut Model, tokens: &[TokenId]) {
        match *op {
            Op::AddResult {
                parent_idx,
                result_idx,
            } => {
                // A result token cannot be its own parent in a well-formed Rete network,
                // but the memory doesn't enforce this; apply unconditionally.
                let (old_count, _new_count) =
                    mem.add_result(tokens[parent_idx], tokens[result_idx]);
                model.add_result(parent_idx, result_idx);
                // When the parent transitions from 0→N results it becomes blocked.
                // A blocked parent cannot simultaneously be unblocked; the Rete engine
                // is responsible for removing the unblocked entry on this transition.
                // We mirror that behavior here so both stay consistent.
                if old_count == 0 {
                    mem.remove_unblocked(tokens[parent_idx]);
                    // The shadow model already handles this in add_result.
                }
            }
            Op::RemoveResult { result_idx } => {
                mem.remove_result(tokens[result_idx]);
                model.remove_result(result_idx);
            }
            Op::SetUnblocked {
                parent_idx,
                passthrough_idx,
            } => {
                // Invariant: only set unblocked when result_count == 0.
                if !model.is_blocked(parent_idx) {
                    mem.set_unblocked(tokens[parent_idx], tokens[passthrough_idx]);
                    model.unblocked.insert(parent_idx, passthrough_idx);
                }
            }
            Op::RemoveUnblocked { parent_idx } => {
                mem.remove_unblocked(tokens[parent_idx]);
                model.unblocked.remove(&parent_idx);
            }
            Op::RemoveParentToken { parent_idx } => {
                mem.remove_parent_token(tokens[parent_idx]);
                model.remove_parent(parent_idx);
            }
            Op::Clear => {
                mem.clear();
                *model = Model::default();
            }
        }
    }

    // ---------------------------------------------------------------------------
    // Property tests
    // ---------------------------------------------------------------------------

    proptest! {
        /// Running arbitrary op sequences never violates internal consistency
        /// as reported by `debug_assert_consistency`.
        #[test]
        fn arbitrary_ops_maintain_consistency(
            ops in proptest::collection::vec(op_strategy(), 0..100)
        ) {
            let mut token_map: SlotMap<TokenId, ()> = SlotMap::with_key();
            let tokens: Vec<TokenId> = (0..5).map(|_| token_map.insert(())).collect();

            let mut mem = NccMemory::new(NccMemoryId(0));
            let mut model = Model::default();

            for op in &ops {
                apply_op(op, &mut mem, &mut model, &tokens);
                // Panics (and thus fails the property test) if any invariant is violated.
                mem.debug_assert_consistency();
            }
        }

        /// After random op sequences, `result_count`, `is_blocked`, and
        /// `get_passthrough` all match the shadow model.
        #[test]
        fn model_matches_implementation(
            ops in proptest::collection::vec(op_strategy(), 0..100)
        ) {
            let mut token_map: SlotMap<TokenId, ()> = SlotMap::with_key();
            let tokens: Vec<TokenId> = (0..5).map(|_| token_map.insert(())).collect();

            let mut mem = NccMemory::new(NccMemoryId(0));
            let mut model = Model::default();

            for op in &ops {
                apply_op(op, &mut mem, &mut model, &tokens);
            }

            for idx in 0..5 {
                // result_count matches the shadow model
                prop_assert_eq!(
                    mem.result_count(tokens[idx]),
                    model.result_count(idx),
                    "result_count mismatch for token index {}",
                    idx
                );
                // is_blocked is consistent with result_count > 0
                prop_assert_eq!(
                    mem.is_blocked(tokens[idx]),
                    model.is_blocked(idx),
                    "is_blocked mismatch for token index {}",
                    idx
                );
                // get_passthrough matches the shadow model
                let expected_pt = model.get_passthrough(idx).map(|pt_idx| tokens[pt_idx]);
                prop_assert_eq!(
                    mem.get_passthrough(tokens[idx]),
                    expected_pt,
                    "get_passthrough mismatch for token index {}",
                    idx
                );
            }

            // is_empty matches
            prop_assert_eq!(
                mem.is_empty(),
                model.is_empty(),
                "is_empty mismatch"
            );
        }

        /// `add_result` returns the correct (old_count, new_count) pair.
        ///
        /// The old count is the result_count before the call; the new count is
        /// old_count + 1 (unless the result token was already tracked, in which
        /// case both values equal the current count).
        #[test]
        fn add_result_count_accuracy(
            prior_results in proptest::collection::vec((0..5_usize, 1..5_usize), 0..5),
            new_result_idx in 1..5_usize,
            parent_idx in 0..1_usize,
        ) {
            // parent uses index 0; results use indices 1-4 so they can't alias parent.
            let mut token_map: SlotMap<TokenId, ()> = SlotMap::with_key();
            let tokens: Vec<TokenId> = (0..5).map(|_| token_map.insert(())).collect();

            let mut mem = NccMemory::new(NccMemoryId(0));
            let mut model = Model::default();

            // Add prior results (all share the same parent for simplicity)
            for &(_, r_idx) in &prior_results {
                mem.add_result(tokens[parent_idx], tokens[r_idx]);
                model.add_result(parent_idx, r_idx);
            }

            let old_count_expected = model.result_count(parent_idx);
            let (old_count_actual, new_count_actual) =
                mem.add_result(tokens[parent_idx], tokens[new_result_idx]);

            prop_assert_eq!(
                old_count_actual, old_count_expected,
                "add_result old_count wrong"
            );

            // If the result was already tracked, new_count equals old_count.
            let already_tracked = model.result_owner.contains_key(&new_result_idx);
            if already_tracked {
                prop_assert_eq!(
                    new_count_actual, old_count_expected,
                    "add_result new_count should equal old when already tracked"
                );
            } else {
                prop_assert_eq!(
                    new_count_actual,
                    old_count_expected + 1,
                    "add_result new_count should be old + 1 for fresh result"
                );
            }

            mem.debug_assert_consistency();
        }

        /// A token cannot be simultaneously blocked (result_count > 0) and
        /// recorded in the unblocked map.
        #[test]
        fn blocked_and_unblocked_mutually_exclusive(
            ops in proptest::collection::vec(op_strategy(), 0..100)
        ) {
            let mut token_map: SlotMap<TokenId, ()> = SlotMap::with_key();
            let tokens: Vec<TokenId> = (0..5).map(|_| token_map.insert(())).collect();

            let mut mem = NccMemory::new(NccMemoryId(0));
            let mut model = Model::default();

            for op in &ops {
                apply_op(op, &mut mem, &mut model, &tokens);

                // For every token, it must not be both blocked and unblocked.
                for (idx, &tok) in tokens.iter().enumerate() {
                    let blocked = mem.is_blocked(tok);
                    let has_passthrough = mem.get_passthrough(tok).is_some();
                    prop_assert!(
                        !(blocked && has_passthrough),
                        "token index {} is both blocked and has a passthrough token",
                        idx
                    );
                }
            }
        }

        /// After `remove_parent_token(p)`, result_count is 0, is_blocked is false,
        /// and get_passthrough returns None for that parent.
        #[test]
        fn remove_parent_token_completeness(
            result_count in 0..5_usize,
            mark_unblocked in any::<bool>(),
        ) {
            let mut token_map: SlotMap<TokenId, ()> = SlotMap::with_key();
            let tokens: Vec<TokenId> = (0..10).map(|_| token_map.insert(())).collect();
            // tokens[0] = parent, tokens[1..=5] = results, tokens[6] = passthrough

            let mut mem = NccMemory::new(NccMemoryId(0));

            // Add `result_count` distinct result tokens for the parent.
            for i in 1..=result_count {
                mem.add_result(tokens[0], tokens[i]);
            }

            // Optionally mark it unblocked (only valid when result_count == 0).
            if mark_unblocked && result_count == 0 {
                mem.set_unblocked(tokens[0], tokens[6]);
            }

            mem.remove_parent_token(tokens[0]);

            prop_assert_eq!(mem.result_count(tokens[0]), 0,
                "result_count must be 0 after remove_parent_token");
            prop_assert!(!mem.is_blocked(tokens[0]),
                "is_blocked must be false after remove_parent_token");
            prop_assert_eq!(mem.get_passthrough(tokens[0]), None,
                "get_passthrough must be None after remove_parent_token");

            mem.debug_assert_consistency();
        }

        /// After `clear()`, `is_empty()` is true regardless of prior state.
        #[test]
        fn clear_resets_everything(
            ops in proptest::collection::vec(op_strategy(), 0..50)
        ) {
            let mut token_map: SlotMap<TokenId, ()> = SlotMap::with_key();
            let tokens: Vec<TokenId> = (0..5).map(|_| token_map.insert(())).collect();

            let mut mem = NccMemory::new(NccMemoryId(0));
            let mut model = Model::default();

            for op in &ops {
                apply_op(op, &mut mem, &mut model, &tokens);
            }

            mem.clear();

            prop_assert!(mem.is_empty(), "is_empty must be true after clear()");
            mem.debug_assert_consistency();
        }

        /// `remove_result` returns the correct (parent_token_id, new_count) pair.
        #[test]
        fn remove_result_returns_correct_values(
            result_idxs in proptest::collection::vec(1..5_usize, 1..5),
            remove_idx in 1..5_usize,
        ) {
            let mut token_map: SlotMap<TokenId, ()> = SlotMap::with_key();
            let tokens: Vec<TokenId> = (0..5).map(|_| token_map.insert(())).collect();
            // tokens[0] = parent, tokens[1..4] = results

            let mut mem = NccMemory::new(NccMemoryId(0));
            let mut model = Model::default();

            for &r_idx in &result_idxs {
                mem.add_result(tokens[0], tokens[r_idx]);
                model.add_result(0, r_idx);
            }

            let actual = mem.remove_result(tokens[remove_idx]);
            let expected = model.remove_result(remove_idx);

            match (actual, expected) {
                (Some((actual_parent, actual_count)), Some((_, expected_count))) => {
                    // The parent should be tokens[0]
                    prop_assert_eq!(actual_parent, tokens[0],
                        "remove_result parent should be tokens[0]");
                    prop_assert_eq!(actual_count, expected_count,
                        "remove_result new_count mismatch");
                }
                (None, None) => { /* both say "not found" — correct */ }
                _ => {
                    prop_assert!(false, "remove_result presence mismatch: actual={actual:?}");
                }
            }

            mem.debug_assert_consistency();
        }
    }
}
