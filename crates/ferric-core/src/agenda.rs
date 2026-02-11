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
        let (key, &id) = self.ordering.iter().next()?;
        let key = key.clone();

        self.ordering.remove(&key);
        self.id_to_key.remove(&id);

        let activation = self.activations.remove(id)?;

        // Clean up token reverse index
        if let Some(acts) = self.token_to_activations.get_mut(&activation.token) {
            acts.retain(|aid| *aid != id);
            if acts.is_empty() {
                self.token_to_activations.remove(&activation.token);
            }
        }

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
