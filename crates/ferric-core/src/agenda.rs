//! Agenda: manages rule activations in priority order.
//!
//! The agenda tracks which rules are ready to fire and determines the order
//! in which they execute based on salience and conflict resolution strategy.

use slotmap::SlotMap;
use smallvec::SmallVec;
use std::collections::{BTreeMap, HashMap};

use crate::beta::RuleId;
use crate::token::TokenId;

slotmap::new_key_type! {
    /// Unique identifier for an activation.
    pub struct ActivationId;
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
    pub salience: i32,
    pub timestamp: u64,
    pub activation_seq: u64,
}

/// The ordering key for agenda activations.
///
/// Phase 1 uses depth strategy: higher salience first, then most recent
/// (higher timestamp) first, then higher activation sequence.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct AgendaKey {
    /// Higher salience first (Reverse).
    pub salience: std::cmp::Reverse<i32>,
    /// Higher timestamp first (Reverse) — depth strategy.
    pub timestamp: std::cmp::Reverse<u64>,
    /// Higher seq first (Reverse) — tiebreaker.
    pub seq: std::cmp::Reverse<u64>,
}

/// The agenda: stores and prioritizes rule activations.
///
/// Activations are ordered by salience (priority), timestamp (recency),
/// and sequence (tiebreaker). The depth strategy fires the most recent
/// activations first within the same salience level.
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
    next_seq: u64,
}

impl Agenda {
    /// Create a new, empty agenda.
    #[must_use]
    pub fn new() -> Self {
        Self {
            ordering: BTreeMap::new(),
            activations: SlotMap::with_key(),
            id_to_key: HashMap::new(),
            token_to_activations: HashMap::new(),
            next_seq: 0,
        }
    }

    /// Add an activation to the agenda.
    ///
    /// The activation's `activation_seq` field will be overwritten with
    /// the next sequence number. Returns the activation ID.
    pub fn add(&mut self, mut activation: Activation) -> ActivationId {
        activation.activation_seq = self.next_seq;
        self.next_seq += 1;

        let key = AgendaKey {
            salience: std::cmp::Reverse(activation.salience),
            timestamp: std::cmp::Reverse(activation.timestamp),
            seq: std::cmp::Reverse(activation.activation_seq),
        };

        let token = activation.token;
        let id = self.activations.insert(activation);

        // Update the activation's ID field
        if let Some(act) = self.activations.get_mut(id) {
            act.id = id;
        }

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
            salience: 0,
            timestamp: 100,
            activation_seq: 0, // Will be overwritten
        };

        let id = agenda.add(activation);
        assert_eq!(agenda.len(), 1);

        let popped = agenda.pop().expect("Should have activation");
        assert_eq!(popped.id, id);
        assert_eq!(popped.rule, RuleId(1));
        assert_eq!(popped.token, token);
        assert_eq!(popped.activation_seq, 0);

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
            salience: 0,
            timestamp: 100,
            activation_seq: 0,
        });

        let id2 = agenda.add(Activation {
            id: ActivationId::default(),
            rule: RuleId(2),
            token: t2,
            salience: 10, // Highest
            timestamp: 100,
            activation_seq: 0,
        });

        let _id3 = agenda.add(Activation {
            id: ActivationId::default(),
            rule: RuleId(3),
            token: t3,
            salience: -5, // Lowest
            timestamp: 100,
            activation_seq: 0,
        });

        assert_eq!(agenda.len(), 3);

        // Pop should return highest salience first (id2)
        let popped = agenda.pop().expect("Should have activation");
        assert_eq!(popped.id, id2);
        assert_eq!(popped.salience, 10);
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
            salience: 0,
            timestamp: 100,
            activation_seq: 0,
        });

        let id2 = agenda.add(Activation {
            id: ActivationId::default(),
            rule: RuleId(2),
            token: t2,
            salience: 0,
            timestamp: 200, // Most recent
            activation_seq: 0,
        });

        let _id3 = agenda.add(Activation {
            id: ActivationId::default(),
            rule: RuleId(3),
            token: t3,
            salience: 0,
            timestamp: 150,
            activation_seq: 0,
        });

        assert_eq!(agenda.len(), 3);

        // Pop should return most recent timestamp first (id2)
        let popped = agenda.pop().expect("Should have activation");
        assert_eq!(popped.id, id2);
        assert_eq!(popped.timestamp, 200);
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
            salience: 0,
            timestamp: 100,
            activation_seq: 0,
        });

        let id2 = agenda.add(Activation {
            id: ActivationId::default(),
            rule: RuleId(2),
            token: t1,
            salience: 5,
            timestamp: 200,
            activation_seq: 0,
        });

        let id3 = agenda.add(Activation {
            id: ActivationId::default(),
            rule: RuleId(3),
            token: t2, // Different token
            salience: 0,
            timestamp: 150,
            activation_seq: 0,
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
}
