//! Agenda: manages rule activations in priority order.
//!
//! The agenda tracks which rules are ready to fire and determines the order
//! in which they execute based on salience and conflict resolution strategy.

use slotmap::SlotMap;
use smallvec::SmallVec;
use std::collections::{BTreeMap, HashMap};

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
    id_to_key: HashMap<ActivationId, AgendaKey>,
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
            id_to_key: HashMap::new(),
            token_to_activations: HashMap::new(),
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
        self.id_to_key.remove(&id);

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
        self.id_to_key.remove(&id);
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
            if let Some(key) = self.id_to_key.remove(&id) {
                self.ordering.remove(&key);
            }

            if let Some(activation) = self.activations.remove(id) {
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

            let reverse_key = self.id_to_key.get(activation_id);
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
                self.activations.contains_key(*activation_id),
                "id_to_key references non-existent activation {activation_id:?}"
            );

            let ordered_id = self.ordering.get(key);
            assert!(
                ordered_id.is_some(),
                "id_to_key key missing from ordering for activation {activation_id:?}"
            );
            assert_eq!(
                ordered_id,
                Some(activation_id),
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
            let key = self.id_to_key.get(&activation_id);
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
}
