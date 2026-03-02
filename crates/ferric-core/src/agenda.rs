//! Agenda: manages rule activations in priority order.
//!
//! The agenda tracks which rules are ready to fire and determines the order
//! in which they execute based on salience and conflict resolution strategy.

use rustc_hash::FxHashMap as HashMap;
use slotmap::{SecondaryMap, SlotMap};
use smallvec::SmallVec;
use std::collections::BTreeMap;

use crate::beta::{RuleId, Salience};
use crate::fact::Timestamp;
use crate::strategy::ConflictResolutionStrategy;
use crate::token::TokenId;

slotmap::new_key_type! {
    /// Unique identifier for an activation.
    pub struct ActivationId;
}

/// Monotonically increasing sequence number for agenda ordering tiebreaks.
///
/// Distinct from `Timestamp` (which tracks fact assertion order) — this tracks
/// the order in which activations are added to the agenda.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ActivationSeq(u64);

impl ActivationSeq {
    pub const ZERO: Self = Self(0);

    #[must_use]
    pub const fn new(val: u64) -> Self {
        Self(val)
    }

    #[must_use]
    pub const fn get(self) -> u64 {
        self.0
    }

    #[must_use]
    pub fn next(self) -> Self {
        Self(self.0 + 1)
    }
}

fn remove_from_token_index(
    token_index: &mut HashMap<TokenId, SmallVec<[ActivationId; 2]>>,
    token_id: TokenId,
    activation_id: ActivationId,
) {
    let mut remove_entry = false;
    if let Some(acts) = token_index.get_mut(&token_id) {
        acts.retain(|aid| *aid != activation_id);
        remove_entry = acts.is_empty();
    }

    if remove_entry {
        token_index.remove(&token_id);
    }
}

/// An activation: a rule that is ready to fire with a specific token.
#[derive(Clone, Debug)]
pub struct Activation {
    pub id: ActivationId,
    pub rule: RuleId,
    pub token: TokenId,
    pub salience: Salience,
    pub timestamp: Timestamp,
    pub activation_seq: ActivationSeq,
    /// Recency vector: timestamps of facts in pattern order (for LEX/MEA strategies).
    pub recency: SmallVec<[Timestamp; 4]>,
}

/// Strategy-specific ordering component for agenda keys.
///
/// The ordering is designed so that `BTreeMap` naturally pops the highest-priority
/// activation first (using `pop_first`).
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum StrategyOrd {
    Depth(std::cmp::Reverse<Timestamp>), // Higher timestamp first
    Breadth(Timestamp),                  // Lower timestamp first
    Lex(std::cmp::Reverse<SmallVec<[Timestamp; 4]>>), // Lexicographic recency (most recent first)
    Mea {
        first_recency: std::cmp::Reverse<Timestamp>,
        rest_recency: std::cmp::Reverse<SmallVec<[Timestamp; 4]>>,
    },
}

/// The ordering key for agenda activations.
///
/// Provides total ordering across all conflict resolution strategies:
/// salience > strategy-specific ordering > activation sequence.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct AgendaKey {
    /// Higher salience first (Reverse).
    pub salience: std::cmp::Reverse<Salience>,
    /// Strategy-specific ordering component.
    pub strategy_ord: StrategyOrd,
    /// Higher seq first (Reverse) — final tiebreaker.
    pub seq: std::cmp::Reverse<ActivationSeq>,
}

/// The agenda: stores and prioritizes rule activations.
///
/// Activations are ordered by salience (priority), strategy-specific ordering,
/// and sequence (tiebreaker). The conflict resolution strategy determines how
/// activations with the same salience are ordered.
pub struct Agenda {
    /// Ordered activations: key -> `ActivationId`.
    ordering: BTreeMap<AgendaKey, ActivationId>,
    /// All activations: `ActivationId` -> `Activation`.
    activations: SlotMap<ActivationId, Activation>,
    /// Reverse index: `ActivationId` -> `AgendaKey` for removal.
    id_to_key: SecondaryMap<ActivationId, AgendaKey>,
    /// Reverse index: `TokenId` -> `ActivationId`s for retraction.
    token_to_activations: HashMap<TokenId, SmallVec<[ActivationId; 2]>>,
    /// Next activation sequence number.
    next_seq: ActivationSeq,
    /// Conflict resolution strategy.
    strategy: ConflictResolutionStrategy,
}

impl Agenda {
    /// Create a new, empty agenda with the default (Depth) strategy.
    #[must_use]
    pub fn new() -> Self {
        Self::with_strategy(ConflictResolutionStrategy::default())
    }

    /// Create a new, empty agenda with the given conflict resolution strategy.
    #[must_use]
    pub fn with_strategy(strategy: ConflictResolutionStrategy) -> Self {
        Self {
            ordering: BTreeMap::new(),
            activations: SlotMap::with_key(),
            id_to_key: SecondaryMap::new(),
            token_to_activations: HashMap::default(),
            next_seq: ActivationSeq::ZERO,
            strategy,
        }
    }

    /// Build an `AgendaKey` for the given activation.
    ///
    /// The key is constructed based on the agenda's conflict resolution strategy.
    fn build_key(&self, activation: &Activation) -> AgendaKey {
        let strategy_ord = match self.strategy {
            ConflictResolutionStrategy::Depth => {
                StrategyOrd::Depth(std::cmp::Reverse(activation.timestamp))
            }
            ConflictResolutionStrategy::Breadth => StrategyOrd::Breadth(activation.timestamp),
            ConflictResolutionStrategy::Lex => {
                StrategyOrd::Lex(std::cmp::Reverse(activation.recency.clone()))
            }
            ConflictResolutionStrategy::Mea => {
                let first_recency = activation
                    .recency
                    .first()
                    .copied()
                    .unwrap_or(Timestamp::ZERO);
                let rest_recency: SmallVec<[Timestamp; 4]> =
                    activation.recency.iter().skip(1).copied().collect();
                StrategyOrd::Mea {
                    first_recency: std::cmp::Reverse(first_recency),
                    rest_recency: std::cmp::Reverse(rest_recency),
                }
            }
        };

        AgendaKey {
            salience: std::cmp::Reverse(activation.salience),
            strategy_ord,
            seq: std::cmp::Reverse(activation.activation_seq),
        }
    }

    /// Add an activation to the agenda.
    ///
    /// The activation's `activation_seq` field will be overwritten with
    /// the next sequence number. Returns the activation ID.
    pub fn add(&mut self, mut activation: Activation) -> ActivationId {
        activation.activation_seq = self.next_seq;
        self.next_seq = self.next_seq.next();

        let key = self.build_key(&activation);
        let token = activation.token;
        let id = self.activations.insert_with_key(|id| {
            activation.id = id;
            activation
        });

        self.ordering.insert(key.clone(), id);
        self.id_to_key.insert(id, key);

        // Update token reverse index
        self.token_to_activations.entry(token).or_default().push(id);

        id
    }

    /// Pop the highest-priority activation from the agenda.
    ///
    /// Returns `None` if the agenda is empty.
    pub fn pop(&mut self) -> Option<Activation> {
        let (_, id) = self.ordering.pop_first()?;
        self.id_to_key.remove(id);

        let activation = self.activations.remove(id)?;

        // Clean up token reverse index
        remove_from_token_index(&mut self.token_to_activations, activation.token, id);

        Some(activation)
    }

    /// Pop the highest-priority activation matching the given predicate.
    ///
    /// Scans from highest to lowest priority and returns the first activation
    /// for which `predicate` returns `true`. Returns `None` if no matching
    /// activation exists.
    pub fn pop_matching(&mut self, predicate: impl Fn(&Activation) -> bool) -> Option<Activation> {
        let mut target_key = None;
        let mut target_id = None;

        for (key, &id) in &self.ordering {
            if let Some(activation) = self.activations.get(id) {
                if predicate(activation) {
                    target_key = Some(key.clone());
                    target_id = Some(id);
                    break;
                }
            }
        }

        let key = target_key?;
        let id = target_id?;

        self.ordering.remove(&key);
        self.id_to_key.remove(id);
        let activation = self.activations.remove(id)?;
        remove_from_token_index(&mut self.token_to_activations, activation.token, id);

        Some(activation)
    }

    /// Check whether any activation matches the given predicate.
    pub fn has_matching(&self, predicate: impl Fn(&Activation) -> bool) -> bool {
        self.activations.values().any(predicate)
    }

    /// Remove all activations for a given token.
    ///
    /// Returns the removed activations.
    pub fn remove_activations_for_token(&mut self, token_id: TokenId) -> Vec<Activation> {
        let Some(act_ids) = self.token_to_activations.remove(&token_id) else {
            return Vec::new();
        };

        let mut removed = Vec::new();

        for id in act_ids {
            if let Some(key) = self.id_to_key.remove(id) {
                self.ordering.remove(&key);
            }

            if let Some(activation) = self.activations.remove(id) {
                removed.push(activation);
            }
        }

        removed
    }

    /// Remove all activations for a given rule.
    ///
    /// Returns the removed activations.
    pub fn remove_activations_for_rule(&mut self, rule_id: RuleId) -> Vec<Activation> {
        let act_ids: Vec<ActivationId> = self
            .activations
            .iter()
            .filter_map(|(id, activation)| (activation.rule == rule_id).then_some(id))
            .collect();

        let mut removed = Vec::with_capacity(act_ids.len());
        for id in act_ids {
            if let Some(key) = self.id_to_key.remove(id) {
                self.ordering.remove(&key);
            }

            if let Some(activation) = self.activations.remove(id) {
                remove_from_token_index(&mut self.token_to_activations, activation.token, id);
                removed.push(activation);
            }
        }

        removed
    }

    /// Return the number of activations in the agenda.
    #[must_use]
    pub fn len(&self) -> usize {
        self.activations.len()
    }

    /// Check if the agenda is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.activations.is_empty()
    }

    /// Get an activation by ID.
    #[must_use]
    pub fn get(&self, id: ActivationId) -> Option<&Activation> {
        self.activations.get(id)
    }

    /// Iterate over all activations.
    pub fn iter_activations(&self) -> impl Iterator<Item = &Activation> {
        self.activations.values()
    }

    /// Get the current conflict resolution strategy.
    #[must_use]
    pub fn strategy(&self) -> ConflictResolutionStrategy {
        self.strategy
    }

    /// Clear all activations, preserving the strategy.
    pub fn clear(&mut self) {
        self.ordering.clear();
        self.activations.clear();
        self.id_to_key.clear();
        self.token_to_activations.clear();
        // Reset next_seq to 0 since this is a full clear
        self.next_seq = ActivationSeq::ZERO;
    }

    /// Verify internal consistency of agenda indices.
    ///
    /// Intended for use in tests and debug builds.
    #[cfg(any(test, debug_assertions))]
    pub fn debug_assert_consistency(&self) {
        // 1. Every key in ordering references a live activation and a matching reverse key.
        for (key, activation_id) in &self.ordering {
            assert!(
                self.activations.contains_key(*activation_id),
                "ordering references non-existent activation {activation_id:?}"
            );

            let reverse_key = self.id_to_key.get(*activation_id);
            assert!(
                reverse_key.is_some(),
                "activation {activation_id:?} missing from id_to_key"
            );
            assert_eq!(
                reverse_key,
                Some(key),
                "id_to_key mismatch for activation {activation_id:?}"
            );
        }

        // 2. Every reverse key references the same entry in ordering and a live activation.
        for (activation_id, key) in &self.id_to_key {
            assert!(
                self.activations.contains_key(activation_id),
                "id_to_key references non-existent activation {activation_id:?}"
            );

            let ordered_id = self.ordering.get(key);
            assert!(
                ordered_id.is_some(),
                "id_to_key key missing from ordering for activation {activation_id:?}"
            );
            assert_eq!(
                ordered_id,
                Some(&activation_id),
                "ordering mismatch for activation {activation_id:?}"
            );
        }

        // 3. token_to_activations entries are non-empty and point to live activations
        //    whose token field matches the map key.
        for (token_id, activation_ids) in &self.token_to_activations {
            assert!(
                !activation_ids.is_empty(),
                "token_to_activations contains empty entry for token {token_id:?}"
            );

            for activation_id in activation_ids {
                let activation = self.activations.get(*activation_id);
                assert!(
                    activation.is_some(),
                    "token_to_activations references non-existent activation {activation_id:?}"
                );
                assert_eq!(
                    activation.map(|a| a.token),
                    Some(*token_id),
                    "token_to_activations token mismatch for activation {activation_id:?}"
                );
            }
        }

        // 4. Every live activation appears in both reverse indices.
        for (activation_id, activation) in &self.activations {
            let key = self.id_to_key.get(activation_id);
            assert!(
                key.is_some(),
                "live activation {activation_id:?} missing from id_to_key"
            );
            if let Some(k) = key {
                assert_eq!(
                    self.ordering.get(k),
                    Some(&activation_id),
                    "live activation {activation_id:?} missing from ordering"
                );
            }

            let token_acts = self.token_to_activations.get(&activation.token);
            assert!(
                token_acts.is_some(),
                "live activation {activation_id:?} missing from token_to_activations"
            );
            assert!(
                token_acts.is_some_and(|ids| ids.contains(&activation_id)),
                "live activation {activation_id:?} not indexed under token {:?}",
                activation.token
            );
        }
    }
}

impl Default for Agenda {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::beta::Salience;
    use crate::fact::Timestamp;
    use slotmap::SlotMap;

    fn make_token_id() -> TokenId {
        let mut temp: SlotMap<TokenId, ()> = SlotMap::with_key();
        temp.insert(())
    }

    #[test]
    fn agenda_new_is_empty() {
        let agenda = Agenda::new();
        assert!(agenda.is_empty());
        assert_eq!(agenda.len(), 0);
    }

    #[test]
    fn agenda_add_and_pop() {
        let mut agenda = Agenda::new();
        let token = make_token_id();

        let activation = Activation {
            id: ActivationId::default(), // Will be overwritten
            rule: RuleId(1),
            token,
            salience: Salience::DEFAULT,
            timestamp: Timestamp::new(100),
            activation_seq: ActivationSeq::ZERO, // Will be overwritten
            recency: SmallVec::new(),
        };

        let id = agenda.add(activation);
        assert_eq!(agenda.len(), 1);

        let popped = agenda.pop().expect("Should have activation");
        assert_eq!(popped.id, id);
        assert_eq!(popped.rule, RuleId(1));
        assert_eq!(popped.token, token);
        assert_eq!(popped.activation_seq, ActivationSeq::ZERO);

        assert!(agenda.is_empty());
    }

    #[test]
    fn agenda_pop_highest_salience_first() {
        let mut agenda = Agenda::new();
        let t1 = make_token_id();
        let t2 = make_token_id();
        let t3 = make_token_id();

        // Add activations with different saliences
        let _id1 = agenda.add(Activation {
            id: ActivationId::default(),
            rule: RuleId(1),
            token: t1,
            salience: Salience::DEFAULT,
            timestamp: Timestamp::new(100),
            activation_seq: ActivationSeq::ZERO,
            recency: SmallVec::new(),
        });

        let id2 = agenda.add(Activation {
            id: ActivationId::default(),
            rule: RuleId(2),
            token: t2,
            salience: Salience::new(10), // Highest
            timestamp: Timestamp::new(100),
            activation_seq: ActivationSeq::ZERO,
            recency: SmallVec::new(),
        });

        let _id3 = agenda.add(Activation {
            id: ActivationId::default(),
            rule: RuleId(3),
            token: t3,
            salience: Salience::new(-5), // Lowest
            timestamp: Timestamp::new(100),
            activation_seq: ActivationSeq::ZERO,
            recency: SmallVec::new(),
        });

        assert_eq!(agenda.len(), 3);

        // Pop should return highest salience first (id2)
        let popped = agenda.pop().expect("Should have activation");
        assert_eq!(popped.id, id2);
        assert_eq!(popped.salience, Salience::new(10));
    }

    #[test]
    fn agenda_pop_most_recent_first_at_same_salience() {
        let mut agenda = Agenda::new();
        let t1 = make_token_id();
        let t2 = make_token_id();
        let t3 = make_token_id();

        // Add activations with same salience, different timestamps
        let _id1 = agenda.add(Activation {
            id: ActivationId::default(),
            rule: RuleId(1),
            token: t1,
            salience: Salience::DEFAULT,
            timestamp: Timestamp::new(100),
            activation_seq: ActivationSeq::ZERO,
            recency: SmallVec::new(),
        });

        let id2 = agenda.add(Activation {
            id: ActivationId::default(),
            rule: RuleId(2),
            token: t2,
            salience: Salience::DEFAULT,
            timestamp: Timestamp::new(200), // Most recent
            activation_seq: ActivationSeq::ZERO,
            recency: SmallVec::new(),
        });

        let _id3 = agenda.add(Activation {
            id: ActivationId::default(),
            rule: RuleId(3),
            token: t3,
            salience: Salience::DEFAULT,
            timestamp: Timestamp::new(150),
            activation_seq: ActivationSeq::ZERO,
            recency: SmallVec::new(),
        });

        assert_eq!(agenda.len(), 3);

        // Pop should return most recent timestamp first (id2)
        let popped = agenda.pop().expect("Should have activation");
        assert_eq!(popped.id, id2);
        assert_eq!(popped.timestamp, Timestamp::new(200));
    }

    #[test]
    fn agenda_remove_activations_for_token() {
        let mut agenda = Agenda::new();
        let mut temp: SlotMap<TokenId, ()> = SlotMap::with_key();
        let t1 = temp.insert(());
        let t2 = temp.insert(());

        // Add multiple activations for the same token
        let id1 = agenda.add(Activation {
            id: ActivationId::default(),
            rule: RuleId(1),
            token: t1,
            salience: Salience::DEFAULT,
            timestamp: Timestamp::new(100),
            activation_seq: ActivationSeq::ZERO,
            recency: SmallVec::new(),
        });

        let id2 = agenda.add(Activation {
            id: ActivationId::default(),
            rule: RuleId(2),
            token: t1,
            salience: Salience::new(5),
            timestamp: Timestamp::new(200),
            activation_seq: ActivationSeq::ZERO,
            recency: SmallVec::new(),
        });

        let id3 = agenda.add(Activation {
            id: ActivationId::default(),
            rule: RuleId(3),
            token: t2, // Different token
            salience: Salience::DEFAULT,
            timestamp: Timestamp::new(150),
            activation_seq: ActivationSeq::ZERO,
            recency: SmallVec::new(),
        });

        assert_eq!(agenda.len(), 3);

        // Remove activations for t1
        let removed = agenda.remove_activations_for_token(t1);
        assert_eq!(removed.len(), 2);

        let removed_ids: Vec<_> = removed.iter().map(|a| a.id).collect();
        assert!(removed_ids.contains(&id1));
        assert!(removed_ids.contains(&id2));

        // Only t2's activation should remain
        assert_eq!(agenda.len(), 1);
        let remaining = agenda.pop().expect("Should have activation");
        assert_eq!(remaining.id, id3);
        assert!(agenda.is_empty());
    }

    // -----------------------------------------------------------------------
    // Conflict resolution strategy tests
    // -----------------------------------------------------------------------

    #[test]
    fn depth_most_recent_first() {
        let mut agenda = Agenda::with_strategy(ConflictResolutionStrategy::Depth);
        let t1 = make_token_id();
        let t2 = make_token_id();
        let t3 = make_token_id();

        // Add activations with same salience, different timestamps
        let _id1 = agenda.add(Activation {
            id: ActivationId::default(),
            rule: RuleId(1),
            token: t1,
            salience: Salience::DEFAULT,
            timestamp: Timestamp::new(100),
            activation_seq: ActivationSeq::ZERO,
            recency: SmallVec::new(),
        });

        let id2 = agenda.add(Activation {
            id: ActivationId::default(),
            rule: RuleId(2),
            token: t2,
            salience: Salience::DEFAULT,
            timestamp: Timestamp::new(300), // Most recent
            activation_seq: ActivationSeq::ZERO,
            recency: SmallVec::new(),
        });

        let _id3 = agenda.add(Activation {
            id: ActivationId::default(),
            rule: RuleId(3),
            token: t3,
            salience: Salience::DEFAULT,
            timestamp: Timestamp::new(200),
            activation_seq: ActivationSeq::ZERO,
            recency: SmallVec::new(),
        });

        // Depth strategy: most recent timestamp first
        let popped = agenda.pop().expect("Should have activation");
        assert_eq!(popped.id, id2);
        assert_eq!(popped.timestamp, Timestamp::new(300));
    }

    #[test]
    fn breadth_oldest_first() {
        let mut agenda = Agenda::with_strategy(ConflictResolutionStrategy::Breadth);
        let t1 = make_token_id();
        let t2 = make_token_id();
        let t3 = make_token_id();

        // Add activations with same salience, different timestamps
        let id1 = agenda.add(Activation {
            id: ActivationId::default(),
            rule: RuleId(1),
            token: t1,
            salience: Salience::DEFAULT,
            timestamp: Timestamp::new(100), // Oldest
            activation_seq: ActivationSeq::ZERO,
            recency: SmallVec::new(),
        });

        let _id2 = agenda.add(Activation {
            id: ActivationId::default(),
            rule: RuleId(2),
            token: t2,
            salience: Salience::DEFAULT,
            timestamp: Timestamp::new(300),
            activation_seq: ActivationSeq::ZERO,
            recency: SmallVec::new(),
        });

        let _id3 = agenda.add(Activation {
            id: ActivationId::default(),
            rule: RuleId(3),
            token: t3,
            salience: Salience::DEFAULT,
            timestamp: Timestamp::new(200),
            activation_seq: ActivationSeq::ZERO,
            recency: SmallVec::new(),
        });

        // Breadth strategy: oldest timestamp first
        let popped = agenda.pop().expect("Should have activation");
        assert_eq!(popped.id, id1);
        assert_eq!(popped.timestamp, Timestamp::new(100));
    }

    #[test]
    fn lex_compares_recency_vectors() {
        let mut agenda = Agenda::with_strategy(ConflictResolutionStrategy::Lex);
        let t1 = make_token_id();
        let t2 = make_token_id();
        let t3 = make_token_id();

        // Add activations with same salience, different recency vectors
        let _id1 = agenda.add(Activation {
            id: ActivationId::default(),
            rule: RuleId(1),
            token: t1,
            salience: Salience::DEFAULT,
            timestamp: Timestamp::new(300),
            activation_seq: ActivationSeq::ZERO,
            recency: smallvec::smallvec![
                Timestamp::new(100),
                Timestamp::new(200),
                Timestamp::new(300)
            ], // [100, 200, 300]
        });

        let id2 = agenda.add(Activation {
            id: ActivationId::default(),
            rule: RuleId(2),
            token: t2,
            salience: Salience::DEFAULT,
            timestamp: Timestamp::new(300),
            activation_seq: ActivationSeq::ZERO,
            recency: smallvec::smallvec![
                Timestamp::new(400),
                Timestamp::new(100),
                Timestamp::new(100)
            ], // [400, ...] wins lexicographically
        });

        let _id3 = agenda.add(Activation {
            id: ActivationId::default(),
            rule: RuleId(3),
            token: t3,
            salience: Salience::DEFAULT,
            timestamp: Timestamp::new(300),
            activation_seq: ActivationSeq::ZERO,
            recency: smallvec::smallvec![
                Timestamp::new(100),
                Timestamp::new(300),
                Timestamp::new(100)
            ], // [100, 300, ...]
        });

        // LEX strategy: lexicographic comparison of recency vectors (most recent first per position)
        // [400, ...] > [100, 300, ...] > [100, 200, ...]
        let popped = agenda.pop().expect("Should have activation");
        assert_eq!(popped.id, id2);
    }

    #[test]
    fn mea_first_pattern_recency_dominates() {
        let mut agenda = Agenda::with_strategy(ConflictResolutionStrategy::Mea);
        let t1 = make_token_id();
        let t2 = make_token_id();
        let t3 = make_token_id();

        // Add activations with same salience, different first-pattern recency
        let _id1 = agenda.add(Activation {
            id: ActivationId::default(),
            rule: RuleId(1),
            token: t1,
            salience: Salience::DEFAULT,
            timestamp: Timestamp::new(300),
            activation_seq: ActivationSeq::ZERO,
            recency: smallvec::smallvec![Timestamp::new(100), Timestamp::new(500)], // First pattern: 100
        });

        let id2 = agenda.add(Activation {
            id: ActivationId::default(),
            rule: RuleId(2),
            token: t2,
            salience: Salience::DEFAULT,
            timestamp: Timestamp::new(300),
            activation_seq: ActivationSeq::ZERO,
            recency: smallvec::smallvec![Timestamp::new(400), Timestamp::new(100)], // First pattern: 400 (highest)
        });

        let _id3 = agenda.add(Activation {
            id: ActivationId::default(),
            rule: RuleId(3),
            token: t3,
            salience: Salience::DEFAULT,
            timestamp: Timestamp::new(300),
            activation_seq: ActivationSeq::ZERO,
            recency: smallvec::smallvec![Timestamp::new(200), Timestamp::new(800)], // First pattern: 200
        });

        // MEA strategy: first pattern recency dominates
        let popped = agenda.pop().expect("Should have activation");
        assert_eq!(popped.id, id2);
    }

    #[test]
    fn mea_falls_back_to_lex_on_tie() {
        let mut agenda = Agenda::with_strategy(ConflictResolutionStrategy::Mea);
        let t1 = make_token_id();
        let t2 = make_token_id();
        let t3 = make_token_id();

        // Add activations with same salience and same first-pattern recency
        let _id1 = agenda.add(Activation {
            id: ActivationId::default(),
            rule: RuleId(1),
            token: t1,
            salience: Salience::DEFAULT,
            timestamp: Timestamp::new(300),
            activation_seq: ActivationSeq::ZERO,
            recency: smallvec::smallvec![
                Timestamp::new(100),
                Timestamp::new(200),
                Timestamp::new(300)
            ], // First: 100, rest: [200, 300]
        });

        let id2 = agenda.add(Activation {
            id: ActivationId::default(),
            rule: RuleId(2),
            token: t2,
            salience: Salience::DEFAULT,
            timestamp: Timestamp::new(300),
            activation_seq: ActivationSeq::ZERO,
            recency: smallvec::smallvec![
                Timestamp::new(100),
                Timestamp::new(500),
                Timestamp::new(100)
            ], // First: 100, rest: [500, 100] wins
        });

        let _id3 = agenda.add(Activation {
            id: ActivationId::default(),
            rule: RuleId(3),
            token: t3,
            salience: Salience::DEFAULT,
            timestamp: Timestamp::new(300),
            activation_seq: ActivationSeq::ZERO,
            recency: smallvec::smallvec![
                Timestamp::new(100),
                Timestamp::new(300),
                Timestamp::new(200)
            ], // First: 100, rest: [300, 200]
        });

        // MEA strategy: same first-pattern recency (100), so LEX tiebreak on rest
        // [500, 100] > [300, 200] > [200, 300]
        let popped = agenda.pop().expect("Should have activation");
        assert_eq!(popped.id, id2);
    }

    #[test]
    fn salience_dominates_all_strategies_depth() {
        let mut agenda = Agenda::with_strategy(ConflictResolutionStrategy::Depth);
        let t1 = make_token_id();
        let t2 = make_token_id();

        let _id1 = agenda.add(Activation {
            id: ActivationId::default(),
            rule: RuleId(1),
            token: t1,
            salience: Salience::DEFAULT,
            timestamp: Timestamp::new(500), // Higher timestamp
            activation_seq: ActivationSeq::ZERO,
            recency: SmallVec::new(),
        });

        let id2 = agenda.add(Activation {
            id: ActivationId::default(),
            rule: RuleId(2),
            token: t2,
            salience: Salience::new(10), // Higher salience wins
            timestamp: Timestamp::new(100),
            activation_seq: ActivationSeq::ZERO,
            recency: SmallVec::new(),
        });

        let popped = agenda.pop().expect("Should have activation");
        assert_eq!(popped.id, id2);
        assert_eq!(popped.salience, Salience::new(10));
    }

    #[test]
    fn salience_dominates_all_strategies_breadth() {
        let mut agenda = Agenda::with_strategy(ConflictResolutionStrategy::Breadth);
        let t1 = make_token_id();
        let t2 = make_token_id();

        let id1 = agenda.add(Activation {
            id: ActivationId::default(),
            rule: RuleId(1),
            token: t1,
            salience: Salience::new(5), // Higher salience wins
            timestamp: Timestamp::new(500),
            activation_seq: ActivationSeq::ZERO,
            recency: SmallVec::new(),
        });

        let _id2 = agenda.add(Activation {
            id: ActivationId::default(),
            rule: RuleId(2),
            token: t2,
            salience: Salience::DEFAULT,
            timestamp: Timestamp::new(100), // Lower timestamp (would win in breadth, but salience dominates)
            activation_seq: ActivationSeq::ZERO,
            recency: SmallVec::new(),
        });

        let popped = agenda.pop().expect("Should have activation");
        assert_eq!(popped.id, id1);
        assert_eq!(popped.salience, Salience::new(5));
    }

    #[test]
    fn salience_dominates_all_strategies_lex() {
        let mut agenda = Agenda::with_strategy(ConflictResolutionStrategy::Lex);
        let t1 = make_token_id();
        let t2 = make_token_id();

        let _id1 = agenda.add(Activation {
            id: ActivationId::default(),
            rule: RuleId(1),
            token: t1,
            salience: Salience::DEFAULT,
            timestamp: Timestamp::new(100),
            activation_seq: ActivationSeq::ZERO,
            recency: smallvec::smallvec![
                Timestamp::new(500),
                Timestamp::new(500),
                Timestamp::new(500)
            ], // Higher recency vector
        });

        let id2 = agenda.add(Activation {
            id: ActivationId::default(),
            rule: RuleId(2),
            token: t2,
            salience: Salience::new(8), // Higher salience wins
            timestamp: Timestamp::new(100),
            activation_seq: ActivationSeq::ZERO,
            recency: smallvec::smallvec![
                Timestamp::new(100),
                Timestamp::new(100),
                Timestamp::new(100)
            ],
        });

        let popped = agenda.pop().expect("Should have activation");
        assert_eq!(popped.id, id2);
        assert_eq!(popped.salience, Salience::new(8));
    }

    #[test]
    fn salience_dominates_all_strategies_mea() {
        let mut agenda = Agenda::with_strategy(ConflictResolutionStrategy::Mea);
        let t1 = make_token_id();
        let t2 = make_token_id();

        let _id1 = agenda.add(Activation {
            id: ActivationId::default(),
            rule: RuleId(1),
            token: t1,
            salience: Salience::new(-1),
            timestamp: Timestamp::new(100),
            activation_seq: ActivationSeq::ZERO,
            recency: smallvec::smallvec![Timestamp::new(500), Timestamp::new(500)], // Higher first recency
        });

        let id2 = agenda.add(Activation {
            id: ActivationId::default(),
            rule: RuleId(2),
            token: t2,
            salience: Salience::new(3), // Higher salience wins
            timestamp: Timestamp::new(100),
            activation_seq: ActivationSeq::ZERO,
            recency: smallvec::smallvec![Timestamp::new(100), Timestamp::new(100)],
        });

        let popped = agenda.pop().expect("Should have activation");
        assert_eq!(popped.id, id2);
        assert_eq!(popped.salience, Salience::new(3));
    }

    #[test]
    fn activation_seq_breaks_ties_depth() {
        let mut agenda = Agenda::with_strategy(ConflictResolutionStrategy::Depth);
        let t1 = make_token_id();
        let t2 = make_token_id();
        let t3 = make_token_id();

        // Add activations with identical salience and timestamp
        let _id1 = agenda.add(Activation {
            id: ActivationId::default(),
            rule: RuleId(1),
            token: t1,
            salience: Salience::DEFAULT,
            timestamp: Timestamp::new(100),
            activation_seq: ActivationSeq::ZERO, // Will be 0
            recency: SmallVec::new(),
        });

        let _id2 = agenda.add(Activation {
            id: ActivationId::default(),
            rule: RuleId(2),
            token: t2,
            salience: Salience::DEFAULT,
            timestamp: Timestamp::new(100),
            activation_seq: ActivationSeq::ZERO, // Will be 1
            recency: SmallVec::new(),
        });

        let id3 = agenda.add(Activation {
            id: ActivationId::default(),
            rule: RuleId(3),
            token: t3,
            salience: Salience::DEFAULT,
            timestamp: Timestamp::new(100),
            activation_seq: ActivationSeq::ZERO, // Will be 2 (highest seq)
            recency: SmallVec::new(),
        });

        // Highest activation_seq should win
        let popped = agenda.pop().expect("Should have activation");
        assert_eq!(popped.id, id3);
        assert_eq!(popped.activation_seq, ActivationSeq::new(2));
    }

    #[test]
    fn activation_seq_breaks_ties_breadth() {
        let mut agenda = Agenda::with_strategy(ConflictResolutionStrategy::Breadth);
        let t1 = make_token_id();
        let t2 = make_token_id();
        let t3 = make_token_id();

        let _id1 = agenda.add(Activation {
            id: ActivationId::default(),
            rule: RuleId(1),
            token: t1,
            salience: Salience::DEFAULT,
            timestamp: Timestamp::new(100),
            activation_seq: ActivationSeq::ZERO,
            recency: SmallVec::new(),
        });

        let _id2 = agenda.add(Activation {
            id: ActivationId::default(),
            rule: RuleId(2),
            token: t2,
            salience: Salience::DEFAULT,
            timestamp: Timestamp::new(100),
            activation_seq: ActivationSeq::ZERO,
            recency: SmallVec::new(),
        });

        let id3 = agenda.add(Activation {
            id: ActivationId::default(),
            rule: RuleId(3),
            token: t3,
            salience: Salience::DEFAULT,
            timestamp: Timestamp::new(100),
            activation_seq: ActivationSeq::ZERO,
            recency: SmallVec::new(),
        });

        let popped = agenda.pop().expect("Should have activation");
        assert_eq!(popped.id, id3);
        assert_eq!(popped.activation_seq, ActivationSeq::new(2));
    }

    #[test]
    fn activation_seq_breaks_ties_lex() {
        let mut agenda = Agenda::with_strategy(ConflictResolutionStrategy::Lex);
        let t1 = make_token_id();
        let t2 = make_token_id();
        let t3 = make_token_id();

        let _id1 = agenda.add(Activation {
            id: ActivationId::default(),
            rule: RuleId(1),
            token: t1,
            salience: Salience::DEFAULT,
            timestamp: Timestamp::new(100),
            activation_seq: ActivationSeq::ZERO,
            recency: smallvec::smallvec![Timestamp::new(100), Timestamp::new(200)],
        });

        let _id2 = agenda.add(Activation {
            id: ActivationId::default(),
            rule: RuleId(2),
            token: t2,
            salience: Salience::DEFAULT,
            timestamp: Timestamp::new(100),
            activation_seq: ActivationSeq::ZERO,
            recency: smallvec::smallvec![Timestamp::new(100), Timestamp::new(200)], // Same recency
        });

        let id3 = agenda.add(Activation {
            id: ActivationId::default(),
            rule: RuleId(3),
            token: t3,
            salience: Salience::DEFAULT,
            timestamp: Timestamp::new(100),
            activation_seq: ActivationSeq::ZERO,
            recency: smallvec::smallvec![Timestamp::new(100), Timestamp::new(200)], // Same recency
        });

        let popped = agenda.pop().expect("Should have activation");
        assert_eq!(popped.id, id3);
        assert_eq!(popped.activation_seq, ActivationSeq::new(2));
    }

    #[test]
    fn activation_seq_breaks_ties_mea() {
        let mut agenda = Agenda::with_strategy(ConflictResolutionStrategy::Mea);
        let t1 = make_token_id();
        let t2 = make_token_id();
        let t3 = make_token_id();

        let _id1 = agenda.add(Activation {
            id: ActivationId::default(),
            rule: RuleId(1),
            token: t1,
            salience: Salience::DEFAULT,
            timestamp: Timestamp::new(100),
            activation_seq: ActivationSeq::ZERO,
            recency: smallvec::smallvec![Timestamp::new(100), Timestamp::new(200)],
        });

        let _id2 = agenda.add(Activation {
            id: ActivationId::default(),
            rule: RuleId(2),
            token: t2,
            salience: Salience::DEFAULT,
            timestamp: Timestamp::new(100),
            activation_seq: ActivationSeq::ZERO,
            recency: smallvec::smallvec![Timestamp::new(100), Timestamp::new(200)], // Same recency
        });

        let id3 = agenda.add(Activation {
            id: ActivationId::default(),
            rule: RuleId(3),
            token: t3,
            salience: Salience::DEFAULT,
            timestamp: Timestamp::new(100),
            activation_seq: ActivationSeq::ZERO,
            recency: smallvec::smallvec![Timestamp::new(100), Timestamp::new(200)], // Same recency
        });

        let popped = agenda.pop().expect("Should have activation");
        assert_eq!(popped.id, id3);
        assert_eq!(popped.activation_seq, ActivationSeq::new(2));
    }

    #[test]
    fn pop_matching_finds_highest_priority_match() {
        let mut agenda = Agenda::new();
        let t1 = make_token_id();
        let t2 = make_token_id();
        let t3 = make_token_id();

        agenda.add(Activation {
            id: ActivationId::default(),
            rule: RuleId(1),
            token: t1,
            salience: Salience::DEFAULT,
            timestamp: Timestamp::new(100),
            activation_seq: ActivationSeq::ZERO,
            recency: SmallVec::new(),
        });

        let id2 = agenda.add(Activation {
            id: ActivationId::default(),
            rule: RuleId(2),
            token: t2,
            salience: Salience::new(10), // highest salience
            timestamp: Timestamp::new(100),
            activation_seq: ActivationSeq::ZERO,
            recency: SmallVec::new(),
        });

        agenda.add(Activation {
            id: ActivationId::default(),
            rule: RuleId(2),
            token: t3,
            salience: Salience::new(5),
            timestamp: Timestamp::new(100),
            activation_seq: ActivationSeq::ZERO,
            recency: SmallVec::new(),
        });

        // Only match rule 2
        let popped = agenda.pop_matching(|a| a.rule == RuleId(2)).unwrap();
        assert_eq!(popped.id, id2);
        assert_eq!(popped.salience, Salience::new(10));
        assert_eq!(agenda.len(), 2);
    }

    #[test]
    fn pop_matching_returns_none_when_no_match() {
        let mut agenda = Agenda::new();
        let t1 = make_token_id();

        agenda.add(Activation {
            id: ActivationId::default(),
            rule: RuleId(1),
            token: t1,
            salience: Salience::DEFAULT,
            timestamp: Timestamp::new(100),
            activation_seq: ActivationSeq::ZERO,
            recency: SmallVec::new(),
        });

        let result = agenda.pop_matching(|a| a.rule == RuleId(99));
        assert!(result.is_none());
        assert_eq!(agenda.len(), 1); // Not removed
    }

    #[test]
    fn has_matching_checks_predicate() {
        let mut agenda = Agenda::new();
        let t1 = make_token_id();

        agenda.add(Activation {
            id: ActivationId::default(),
            rule: RuleId(5),
            token: t1,
            salience: Salience::DEFAULT,
            timestamp: Timestamp::new(100),
            activation_seq: ActivationSeq::ZERO,
            recency: SmallVec::new(),
        });

        assert!(agenda.has_matching(|a| a.rule == RuleId(5)));
        assert!(!agenda.has_matching(|a| a.rule == RuleId(99)));
    }

    #[test]
    fn strategy_switch_changes_ordering() {
        let t1 = make_token_id();
        let t2 = make_token_id();

        // Test with Depth: most recent first
        let mut agenda_depth = Agenda::with_strategy(ConflictResolutionStrategy::Depth);
        let _id1_depth = agenda_depth.add(Activation {
            id: ActivationId::default(),
            rule: RuleId(1),
            token: t1,
            salience: Salience::DEFAULT,
            timestamp: Timestamp::new(100),
            activation_seq: ActivationSeq::ZERO,
            recency: SmallVec::new(),
        });
        let id2_depth = agenda_depth.add(Activation {
            id: ActivationId::default(),
            rule: RuleId(2),
            token: t2,
            salience: Salience::DEFAULT,
            timestamp: Timestamp::new(200), // Most recent
            activation_seq: ActivationSeq::ZERO,
            recency: SmallVec::new(),
        });

        let popped_depth = agenda_depth.pop().unwrap();
        assert_eq!(popped_depth.id, id2_depth);
        assert_eq!(popped_depth.timestamp, Timestamp::new(200));

        // Test with Breadth: oldest first
        let mut agenda_breadth = Agenda::with_strategy(ConflictResolutionStrategy::Breadth);
        let id1_breadth = agenda_breadth.add(Activation {
            id: ActivationId::default(),
            rule: RuleId(1),
            token: t1,
            salience: Salience::DEFAULT,
            timestamp: Timestamp::new(100), // Oldest
            activation_seq: ActivationSeq::ZERO,
            recency: SmallVec::new(),
        });
        let _id2_breadth = agenda_breadth.add(Activation {
            id: ActivationId::default(),
            rule: RuleId(2),
            token: t2,
            salience: Salience::DEFAULT,
            timestamp: Timestamp::new(200),
            activation_seq: ActivationSeq::ZERO,
            recency: SmallVec::new(),
        });

        let popped_breadth = agenda_breadth.pop().unwrap();
        assert_eq!(popped_breadth.id, id1_breadth);
        assert_eq!(popped_breadth.timestamp, Timestamp::new(100));
    }

    #[test]
    fn remove_activations_for_token_works_with_depth() {
        let mut agenda = Agenda::with_strategy(ConflictResolutionStrategy::Depth);
        let mut temp: SlotMap<TokenId, ()> = SlotMap::with_key();
        let t1 = temp.insert(());
        let t2 = temp.insert(());

        let id1 = agenda.add(Activation {
            id: ActivationId::default(),
            rule: RuleId(1),
            token: t1,
            salience: Salience::DEFAULT,
            timestamp: Timestamp::new(100),
            activation_seq: ActivationSeq::ZERO,
            recency: SmallVec::new(),
        });

        let _id2 = agenda.add(Activation {
            id: ActivationId::default(),
            rule: RuleId(2),
            token: t2,
            salience: Salience::DEFAULT,
            timestamp: Timestamp::new(200),
            activation_seq: ActivationSeq::ZERO,
            recency: SmallVec::new(),
        });

        let removed = agenda.remove_activations_for_token(t1);
        assert_eq!(removed.len(), 1);
        assert_eq!(removed[0].id, id1);
        assert_eq!(agenda.len(), 1);
    }

    #[test]
    fn remove_activations_for_token_works_with_breadth() {
        let mut agenda = Agenda::with_strategy(ConflictResolutionStrategy::Breadth);
        let mut temp: SlotMap<TokenId, ()> = SlotMap::with_key();
        let t1 = temp.insert(());
        let t2 = temp.insert(());

        let id1 = agenda.add(Activation {
            id: ActivationId::default(),
            rule: RuleId(1),
            token: t1,
            salience: Salience::DEFAULT,
            timestamp: Timestamp::new(100),
            activation_seq: ActivationSeq::ZERO,
            recency: SmallVec::new(),
        });

        let _id2 = agenda.add(Activation {
            id: ActivationId::default(),
            rule: RuleId(2),
            token: t2,
            salience: Salience::DEFAULT,
            timestamp: Timestamp::new(200),
            activation_seq: ActivationSeq::ZERO,
            recency: SmallVec::new(),
        });

        let removed = agenda.remove_activations_for_token(t1);
        assert_eq!(removed.len(), 1);
        assert_eq!(removed[0].id, id1);
        assert_eq!(agenda.len(), 1);
    }

    #[test]
    fn remove_activations_for_token_works_with_lex() {
        let mut agenda = Agenda::with_strategy(ConflictResolutionStrategy::Lex);
        let mut temp: SlotMap<TokenId, ()> = SlotMap::with_key();
        let t1 = temp.insert(());
        let t2 = temp.insert(());

        let id1 = agenda.add(Activation {
            id: ActivationId::default(),
            rule: RuleId(1),
            token: t1,
            salience: Salience::DEFAULT,
            timestamp: Timestamp::new(100),
            activation_seq: ActivationSeq::ZERO,
            recency: smallvec::smallvec![Timestamp::new(100), Timestamp::new(200)],
        });

        let _id2 = agenda.add(Activation {
            id: ActivationId::default(),
            rule: RuleId(2),
            token: t2,
            salience: Salience::DEFAULT,
            timestamp: Timestamp::new(200),
            activation_seq: ActivationSeq::ZERO,
            recency: smallvec::smallvec![Timestamp::new(200), Timestamp::new(300)],
        });

        let removed = agenda.remove_activations_for_token(t1);
        assert_eq!(removed.len(), 1);
        assert_eq!(removed[0].id, id1);
        assert_eq!(agenda.len(), 1);
    }

    #[test]
    fn remove_activations_for_token_works_with_mea() {
        let mut agenda = Agenda::with_strategy(ConflictResolutionStrategy::Mea);
        let mut temp: SlotMap<TokenId, ()> = SlotMap::with_key();
        let t1 = temp.insert(());
        let t2 = temp.insert(());

        let id1 = agenda.add(Activation {
            id: ActivationId::default(),
            rule: RuleId(1),
            token: t1,
            salience: Salience::DEFAULT,
            timestamp: Timestamp::new(100),
            activation_seq: ActivationSeq::ZERO,
            recency: smallvec::smallvec![Timestamp::new(100), Timestamp::new(200)],
        });

        let _id2 = agenda.add(Activation {
            id: ActivationId::default(),
            rule: RuleId(2),
            token: t2,
            salience: Salience::DEFAULT,
            timestamp: Timestamp::new(200),
            activation_seq: ActivationSeq::ZERO,
            recency: smallvec::smallvec![Timestamp::new(200), Timestamp::new(300)],
        });

        let removed = agenda.remove_activations_for_token(t1);
        assert_eq!(removed.len(), 1);
        assert_eq!(removed[0].id, id1);
        assert_eq!(agenda.len(), 1);
    }

    #[test]
    fn remove_activations_for_rule_removes_all_matching_entries() {
        let mut agenda = Agenda::new();
        let mut temp: SlotMap<TokenId, ()> = SlotMap::with_key();
        let t1 = temp.insert(());
        let t2 = temp.insert(());
        let t3 = temp.insert(());

        let id1 = agenda.add(Activation {
            id: ActivationId::default(),
            rule: RuleId(7),
            token: t1,
            salience: Salience::DEFAULT,
            timestamp: Timestamp::new(10),
            activation_seq: ActivationSeq::ZERO,
            recency: SmallVec::new(),
        });
        let _id2 = agenda.add(Activation {
            id: ActivationId::default(),
            rule: RuleId(8),
            token: t2,
            salience: Salience::DEFAULT,
            timestamp: Timestamp::new(20),
            activation_seq: ActivationSeq::ZERO,
            recency: SmallVec::new(),
        });
        let id3 = agenda.add(Activation {
            id: ActivationId::default(),
            rule: RuleId(7),
            token: t3,
            salience: Salience::DEFAULT,
            timestamp: Timestamp::new(30),
            activation_seq: ActivationSeq::ZERO,
            recency: SmallVec::new(),
        });

        let removed = agenda.remove_activations_for_rule(RuleId(7));
        let removed_ids: std::collections::HashSet<_> = removed.iter().map(|a| a.id).collect();
        assert_eq!(removed.len(), 2);
        assert!(removed_ids.contains(&id1));
        assert!(removed_ids.contains(&id3));
        assert_eq!(agenda.len(), 1);
        assert!(agenda.iter_activations().all(|a| a.rule == RuleId(8)));
    }
}

#[cfg(test)]
mod proptests {
    use super::*;
    use crate::beta::{RuleId, Salience};
    use crate::fact::Timestamp;
    use crate::strategy::ConflictResolutionStrategy;
    use proptest::prelude::*;
    use slotmap::SlotMap;
    use smallvec::SmallVec;

    // ---------------------------------------------------------------------------
    // Operation enum
    // ---------------------------------------------------------------------------

    /// An abstract mutation that can be applied to an `Agenda`.
    #[derive(Clone, Debug)]
    enum Op {
        Add {
            rule_idx: u8,
            token_idx: u8,
            salience: i32,
            timestamp: u64,
        },
        Pop,
        RemoveForToken {
            token_idx: u8,
        },
        RemoveForRule {
            rule_idx: u8,
        },
    }

    fn op_strategy() -> impl Strategy<Value = Op> {
        prop_oneof![
            4 => (0..5u8, 0..5u8, -10i32..10, 1u64..1000).prop_map(|(r, t, s, ts)| Op::Add {
                rule_idx: r,
                token_idx: t,
                salience: s,
                timestamp: ts,
            }),
            2 => Just(Op::Pop),
            1 => (0..5u8).prop_map(|t| Op::RemoveForToken { token_idx: t }),
            1 => (0..5u8).prop_map(|r| Op::RemoveForRule { rule_idx: r }),
        ]
    }

    // ---------------------------------------------------------------------------
    // Helpers
    // ---------------------------------------------------------------------------

    /// Build a pool of 5 `TokenId`s from a single `SlotMap`.
    fn make_token_pool() -> (SlotMap<TokenId, ()>, Vec<TokenId>) {
        let mut map: SlotMap<TokenId, ()> = SlotMap::with_key();
        let ids: Vec<_> = (0..5).map(|_| map.insert(())).collect();
        (map, ids)
    }

    /// Construct an `Activation` with empty recency (suitable for Depth/Breadth tests).
    fn make_activation(
        rule: RuleId,
        token: TokenId,
        salience: Salience,
        timestamp: Timestamp,
    ) -> Activation {
        Activation {
            id: ActivationId::default(),
            rule,
            token,
            salience,
            timestamp,
            activation_seq: ActivationSeq::ZERO,
            recency: SmallVec::new(),
        }
    }

    /// Apply one `Op` to an agenda, using the provided token pool.
    fn apply_op(agenda: &mut Agenda, op: &Op, tokens: &[TokenId]) {
        match *op {
            Op::Add {
                rule_idx,
                token_idx,
                salience,
                timestamp,
            } => {
                let token = tokens[token_idx as usize % tokens.len()];
                agenda.add(make_activation(
                    RuleId(u32::from(rule_idx)),
                    token,
                    Salience::new(salience),
                    Timestamp::new(timestamp),
                ));
            }
            Op::Pop => {
                let _ = agenda.pop();
            }
            Op::RemoveForToken { token_idx } => {
                let token = tokens[token_idx as usize % tokens.len()];
                let _ = agenda.remove_activations_for_token(token);
            }
            Op::RemoveForRule { rule_idx } => {
                let _ = agenda.remove_activations_for_rule(RuleId(u32::from(rule_idx)));
            }
        }
    }

    const ALL_STRATEGIES: [ConflictResolutionStrategy; 4] = [
        ConflictResolutionStrategy::Depth,
        ConflictResolutionStrategy::Breadth,
        ConflictResolutionStrategy::Lex,
        ConflictResolutionStrategy::Mea,
    ];

    // ---------------------------------------------------------------------------
    // Tests
    // ---------------------------------------------------------------------------

    proptest! {
        /// Running arbitrary operations under every strategy keeps all four indices
        /// in sync (verified by `debug_assert_consistency`).
        #[test]
        fn arbitrary_ops_maintain_consistency(
            ops in prop::collection::vec(op_strategy(), 0..80)
        ) {
            let (_map, tokens) = make_token_pool();
            for &strategy in &ALL_STRATEGIES {
                let mut agenda = Agenda::with_strategy(strategy);
                for op in &ops {
                    apply_op(&mut agenda, op, &tokens);
                    agenda.debug_assert_consistency();
                }
            }
        }

        /// Adding N activations and then popping N+1 times yields exactly N
        /// `Some` results followed by one `None`, and the agenda is empty.
        #[test]
        fn pop_drains_completely(n in 0usize..30) {
            let (_map, tokens) = make_token_pool();
            let mut agenda = Agenda::new();
            for i in 0..n {
                agenda.add(make_activation(
                    RuleId(0),
                    tokens[i % tokens.len()],
                    Salience::DEFAULT,
                    Timestamp::new(u64::try_from(i + 1).unwrap()),
                ));
            }

            for _ in 0..n {
                prop_assert!(agenda.pop().is_some());
            }
            prop_assert!(agenda.pop().is_none());
            prop_assert!(agenda.is_empty());
        }

        /// Under Depth strategy, successive pops never yield a higher salience
        /// than the previous pop (weak non-increasing salience order).
        #[test]
        fn pop_order_non_increasing_salience(
            entries in prop::collection::vec((-10i32..10, 1u64..1000), 1..20)
        ) {
            let (_map, tokens) = make_token_pool();
            let mut agenda = Agenda::with_strategy(ConflictResolutionStrategy::Depth);
            for (i, (sal, ts)) in entries.iter().enumerate() {
                agenda.add(make_activation(
                    RuleId(0),
                    tokens[i % tokens.len()],
                    Salience::new(*sal),
                    Timestamp::new(*ts),
                ));
            }

            let mut prev_salience: Option<i32> = None;
            while let Some(act) = agenda.pop() {
                let s = act.salience.get();
                if let Some(prev) = prev_salience {
                    prop_assert!(s <= prev, "salience increased: {} -> {}", prev, s);
                }
                prev_salience = Some(s);
            }
        }

        /// Under every strategy, an activation with strictly higher salience always
        /// pops before one with lower salience, regardless of timestamp or recency.
        #[test]
        fn salience_dominates_all_strategies(
            low_ts in 1u64..500,
            high_ts in 1u64..500,
            low_sal in -9i32..0,
            high_sal in 1i32..10,
        ) {
            let (_map, tokens) = make_token_pool();
            for &strategy in &ALL_STRATEGIES {
                let mut agenda = Agenda::with_strategy(strategy);
                // Insert low-salience activation first (would win seq tiebreak)
                agenda.add(make_activation(
                    RuleId(0),
                    tokens[0],
                    Salience::new(low_sal),
                    Timestamp::new(low_ts),
                ));
                // Insert high-salience activation second
                agenda.add(make_activation(
                    RuleId(1),
                    tokens[1],
                    Salience::new(high_sal),
                    Timestamp::new(high_ts),
                ));
                let first = agenda.pop().expect("should have activation");
                prop_assert_eq!(
                    first.salience,
                    Salience::new(high_sal),
                    "strategy {:?}: expected high salience to pop first",
                    strategy,
                );
            }
        }

        /// Under Depth strategy, with equal salience, the activation with the
        /// higher timestamp pops first.
        #[test]
        fn depth_prefers_higher_timestamp(
            ts_a in 1u64..500,
            ts_b in 501u64..1000,
        ) {
            // ts_b > ts_a by construction
            let (_map, tokens) = make_token_pool();
            let mut agenda = Agenda::with_strategy(ConflictResolutionStrategy::Depth);
            agenda.add(make_activation(RuleId(0), tokens[0], Salience::DEFAULT, Timestamp::new(ts_a)));
            agenda.add(make_activation(RuleId(1), tokens[1], Salience::DEFAULT, Timestamp::new(ts_b)));
            let first = agenda.pop().expect("should have activation");
            prop_assert_eq!(first.timestamp, Timestamp::new(ts_b));
        }

        /// Under Breadth strategy, with equal salience, the activation with the
        /// lower timestamp pops first.
        #[test]
        fn breadth_prefers_lower_timestamp(
            ts_a in 1u64..500,
            ts_b in 501u64..1000,
        ) {
            // ts_a < ts_b by construction
            let (_map, tokens) = make_token_pool();
            let mut agenda = Agenda::with_strategy(ConflictResolutionStrategy::Breadth);
            agenda.add(make_activation(RuleId(0), tokens[0], Salience::DEFAULT, Timestamp::new(ts_a)));
            agenda.add(make_activation(RuleId(1), tokens[1], Salience::DEFAULT, Timestamp::new(ts_b)));
            let first = agenda.pop().expect("should have activation");
            prop_assert_eq!(first.timestamp, Timestamp::new(ts_a));
        }

        /// With the same salience and timestamp, the activation added later
        /// (higher activation_seq) pops first (recency-of-addition tiebreaker).
        #[test]
        fn activation_seq_tiebreaker(sal in -10i32..10, ts in 1u64..1000) {
            let (_map, tokens) = make_token_pool();
            for &strategy in &ALL_STRATEGIES {
                let mut agenda = Agenda::with_strategy(strategy);
                let _first = agenda.add(make_activation(
                    RuleId(0),
                    tokens[0],
                    Salience::new(sal),
                    Timestamp::new(ts),
                ));
                let second = agenda.add(make_activation(
                    RuleId(1),
                    tokens[1],
                    Salience::new(sal),
                    Timestamp::new(ts),
                ));
                let popped = agenda.pop().expect("should have activation");
                prop_assert_eq!(
                    popped.id,
                    second,
                    "strategy {:?}: later-added activation should pop first",
                    strategy,
                );
            }
        }

        /// After `remove_activations_for_token`, no remaining activation in the
        /// agenda has that token.
        #[test]
        fn remove_for_token_completeness(
            ops in prop::collection::vec(op_strategy(), 0..40),
            target_token_idx in 0..5u8,
        ) {
            let (_map, tokens) = make_token_pool();
            let mut agenda = Agenda::new();
            for op in &ops {
                apply_op(&mut agenda, op, &tokens);
            }
            let target = tokens[target_token_idx as usize % tokens.len()];
            let _ = agenda.remove_activations_for_token(target);
            agenda.debug_assert_consistency();
            for act in agenda.iter_activations() {
                prop_assert_ne!(
                    act.token,
                    target,
                    "found activation with removed token after remove_activations_for_token",
                );
            }
        }

        /// After `remove_activations_for_rule`, no remaining activation in the
        /// agenda has that rule.
        #[test]
        fn remove_for_rule_completeness(
            ops in prop::collection::vec(op_strategy(), 0..40),
            target_rule_idx in 0..5u8,
        ) {
            let (_map, tokens) = make_token_pool();
            let mut agenda = Agenda::new();
            for op in &ops {
                apply_op(&mut agenda, op, &tokens);
            }
            let target_rule = RuleId(u32::from(target_rule_idx));
            let _ = agenda.remove_activations_for_rule(target_rule);
            agenda.debug_assert_consistency();
            for act in agenda.iter_activations() {
                prop_assert_ne!(
                    act.rule,
                    target_rule,
                    "found activation with removed rule after remove_activations_for_rule",
                );
            }
        }

        /// After `clear()`, the agenda is fully empty.
        #[test]
        fn clear_resets_everything(
            ops in prop::collection::vec(op_strategy(), 0..40)
        ) {
            let (_map, tokens) = make_token_pool();
            let mut agenda = Agenda::new();
            for op in &ops {
                apply_op(&mut agenda, op, &tokens);
            }
            agenda.clear();
            prop_assert!(agenda.is_empty());
            prop_assert_eq!(agenda.len(), 0);
            prop_assert!(agenda.pop().is_none());
            agenda.debug_assert_consistency();
        }

        /// `len()` correctly tracks adds, pops, and removals across arbitrary
        /// operation sequences.
        #[test]
        fn len_tracks_mutations(
            ops in prop::collection::vec(op_strategy(), 0..60)
        ) {
            let (_map, tokens) = make_token_pool();
            let mut agenda = Agenda::new();
            let mut expected_len: usize = 0;

            for op in &ops {
                match op {
                    Op::Add { rule_idx, token_idx, salience, timestamp } => {
                        let token = tokens[*token_idx as usize % tokens.len()];
                        agenda.add(make_activation(
                            RuleId(u32::from(*rule_idx)),
                            token,
                            Salience::new(*salience),
                            Timestamp::new(*timestamp),
                        ));
                        expected_len += 1;
                    }
                    Op::Pop => {
                        let had = agenda.pop().is_some();
                        if had {
                            expected_len -= 1;
                        }
                    }
                    Op::RemoveForToken { token_idx } => {
                        let token = tokens[*token_idx as usize % tokens.len()];
                        let removed = agenda.remove_activations_for_token(token);
                        expected_len -= removed.len();
                    }
                    Op::RemoveForRule { rule_idx } => {
                        let rule = RuleId(u32::from(*rule_idx));
                        let removed = agenda.remove_activations_for_rule(rule);
                        expected_len -= removed.len();
                    }
                }
                prop_assert_eq!(
                    agenda.len(),
                    expected_len,
                    "len() mismatch after op {:?}",
                    op,
                );
            }
        }

        /// Removing activations for a token that has none is a no-op: returns an
        /// empty vec and does not change `len()`.
        #[test]
        fn remove_absent_token_is_noop(n in 0usize..20) {
            let (_map, tokens) = make_token_pool();
            // Use only tokens[0..4] when adding, leaving tokens[4] always absent.
            let absent_token = tokens[4];
            let mut agenda = Agenda::new();
            for i in 0..n {
                agenda.add(make_activation(
                    RuleId(0),
                    tokens[i % 4],
                    Salience::DEFAULT,
                    Timestamp::new(u64::try_from(i + 1).unwrap()),
                ));
            }
            let before_len = agenda.len();
            let removed = agenda.remove_activations_for_token(absent_token);
            prop_assert!(removed.is_empty());
            prop_assert_eq!(agenda.len(), before_len);
            agenda.debug_assert_consistency();
        }

        /// `strategy()` returns the strategy the agenda was constructed with.
        #[test]
        fn strategy_preserved(_seed in 0u8..1) {
            for &strategy in &ALL_STRATEGIES {
                let agenda = Agenda::with_strategy(strategy);
                prop_assert_eq!(agenda.strategy(), strategy);
            }
        }
    }
}
