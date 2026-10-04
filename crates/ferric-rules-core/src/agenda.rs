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
use crate::tracing_support::ferric_span;

slotmap::new_key_type! {
    /// Unique identifier for an activation.
    pub struct ActivationId;
}

/// Creation-order sequence number for agenda ordering.
///
/// Distinct from `Timestamp` (which tracks fact assertion order) — this tracks
/// the order in which activations are added to the agenda. Before exhaustion,
/// the agenda rebases live sequences without changing their relative order;
/// values are not permanent identities across that rebase or a full clear.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
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

/// An outer conditional element's recency: absent matches sort below every fact.
///
/// Zero represents a negative/existential match or dummy root. Facts use their
/// assertion timestamp plus one, keeping a fact asserted at timestamp zero distinct.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct RecencyTag(u64);

impl RecencyTag {
    pub const ABSENT: Self = Self(0);

    /// Encode a fact timestamp. The fact store never assigns `u64::MAX`.
    #[must_use]
    pub const fn from_timestamp(timestamp: Timestamp) -> Option<Self> {
        match timestamp.get().checked_add(1) {
            Some(tag) => Some(Self(tag)),
            None => None,
        }
    }

    /// Recover the fact timestamp, or `None` for an absent match.
    #[must_use]
    pub const fn timestamp(self) -> Option<Timestamp> {
        match self.0.checked_sub(1) {
            Some(timestamp) => Some(Timestamp::new(timestamp)),
            None => None,
        }
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
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Activation {
    pub id: ActivationId,
    pub rule: RuleId,
    pub token: TokenId,
    pub salience: Salience,
    /// CLIPS unsigned 11-bit rule specificity.
    pub complexity: u16,
    pub timestamp: Timestamp,
    pub activation_seq: ActivationSeq,
    /// Recency of outer conditional elements in source order, including absent matches.
    pub recency: SmallVec<[RecencyTag; 4]>,
}

/// Strategy-specific ordering component for agenda keys.
///
/// The ordering is designed so that `BTreeMap` naturally pops the highest-priority
/// activation first (using `pop_first`).
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum StrategyOrd {
    Depth(std::cmp::Reverse<ActivationSeq>), // Newest activation first
    Breadth(ActivationSeq),                  // Oldest activation first
    Lex(std::cmp::Reverse<SmallVec<[RecencyTag; 4]>>), // Lexicographic recency (most recent first)
    Mea {
        first_recency: std::cmp::Reverse<RecencyTag>,
        sorted_recency: std::cmp::Reverse<SmallVec<[RecencyTag; 4]>>,
    },
}

/// The ordering key for agenda activations.
///
/// Provides total ordering across all conflict resolution strategies:
/// salience > strategy-specific ordering > complexity > activation sequence.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct AgendaKey {
    /// Higher salience first (Reverse).
    pub salience: std::cmp::Reverse<Salience>,
    /// Strategy-specific ordering component.
    pub strategy_ord: StrategyOrd,
    /// Higher complexity first after equal LEX/MEA recency.
    pub complexity: std::cmp::Reverse<u16>,
    /// Oldest activation first after equal recency and complexity.
    pub seq: ActivationSeq,
}

/// The agenda: stores and prioritizes rule activations.
///
/// Activations are ordered by salience (priority), strategy-specific ordering,
/// and sequence (tiebreaker). The conflict resolution strategy determines how
/// activations with the same salience are ordered.
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Agenda {
    /// Derived ordering within each rule, allocated only after a focus miss.
    #[cfg_attr(feature = "serde", serde(skip))]
    rule_ordering: Option<RuleOrdering>,
    /// Ordered activations: key -> `ActivationId`.
    #[cfg_attr(feature = "serde", serde(with = "crate::serde_helpers::btree_map"))]
    ordering: BTreeMap<AgendaKey, ActivationId>,
    /// All activations: `ActivationId` -> `Activation`.
    activations: SlotMap<ActivationId, Activation>,
    /// Reverse index: `ActivationId` -> `AgendaKey` for removal.
    id_to_key: SecondaryMap<ActivationId, AgendaKey>,
    /// Reverse index: `TokenId` -> `ActivationId`s for retraction.
    #[cfg_attr(feature = "serde", serde(with = "crate::serde_helpers::fx_hash_map"))]
    token_to_activations: HashMap<TokenId, SmallVec<[ActivationId; 2]>>,
    /// Next activation sequence number.
    next_seq: ActivationSeq,
    /// Conflict resolution strategy.
    strategy: ConflictResolutionStrategy,
}

type RuleOrdering = HashMap<RuleId, BTreeMap<AgendaKey, ActivationId>>;

fn remove_rule_key(index: &mut RuleOrdering, rule: RuleId, key: &AgendaKey) {
    if let Some(entries) = index.get_mut(&rule) {
        entries.remove(key);
        if entries.is_empty() {
            index.remove(&rule);
        }
    }
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
            rule_ordering: None,
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
                StrategyOrd::Depth(std::cmp::Reverse(activation.activation_seq))
            }
            ConflictResolutionStrategy::Breadth => StrategyOrd::Breadth(activation.activation_seq),
            ConflictResolutionStrategy::Lex | ConflictResolutionStrategy::Mea => {
                let mut sorted_recency = activation.recency.clone();
                sorted_recency.sort_unstable_by(|left, right| right.cmp(left));
                if self.strategy == ConflictResolutionStrategy::Lex {
                    StrategyOrd::Lex(std::cmp::Reverse(sorted_recency))
                } else {
                    StrategyOrd::Mea {
                        first_recency: std::cmp::Reverse(
                            activation
                                .recency
                                .first()
                                .copied()
                                .unwrap_or(RecencyTag::ABSENT),
                        ),
                        sorted_recency: std::cmp::Reverse(sorted_recency),
                    }
                }
            }
        };

        AgendaKey {
            salience: std::cmp::Reverse(activation.salience),
            strategy_ord,
            complexity: std::cmp::Reverse(activation.complexity),
            seq: activation.activation_seq,
        }
    }

    /// Add an activation to the agenda.
    ///
    /// The activation's `activation_seq` field will be overwritten with
    /// the next sequence number. Returns the activation ID.
    pub fn add(&mut self, mut activation: Activation) -> ActivationId {
        ferric_span!(trace_span, "agenda_add", rule = ?activation.rule, salience = activation.salience.get());
        // Rebase before exhaustion rather than wrap and make new activations
        // appear old. This preserves relative order, including LEX/MEA ties.
        if self.next_seq.get() == u64::MAX {
            self.rebase_sequences();
        }
        activation.activation_seq = self.next_seq;
        self.next_seq = self.next_seq.next();

        let key = self.build_key(&activation);
        let token = activation.token;
        let rule = activation.rule;
        let id = self.activations.insert_with_key(|id| {
            activation.id = id;
            activation
        });

        self.ordering.insert(key.clone(), id);
        if let Some(index) = &mut self.rule_ordering {
            index.entry(rule).or_default().insert(key.clone(), id);
        }
        self.id_to_key.insert(id, key);

        // Update token reverse index
        self.token_to_activations.entry(token).or_default().push(id);

        id
    }

    /// Reclaim sequence space while preserving the chronology of live matches.
    fn rebase_sequences(&mut self) {
        self.rule_ordering = None;
        let mut chronological: Vec<_> = self
            .activations
            .iter()
            .map(|(id, activation)| (activation.activation_seq, id))
            .collect();
        chronological.sort_unstable_by_key(|&(sequence, _)| sequence);
        for (sequence, &(_, id)) in chronological.iter().enumerate() {
            self.activations[id].activation_seq = ActivationSeq::new(
                u64::try_from(sequence).expect("live activations fit the sequence space"),
            );
        }
        self.next_seq = ActivationSeq::new(
            u64::try_from(chronological.len()).expect("live activations fit the sequence space"),
        );
        self.ordering.clear();
        self.id_to_key.clear();
        for (id, activation) in &self.activations {
            let key = self.build_key(activation);
            self.ordering.insert(key.clone(), id);
            self.id_to_key.insert(id, key);
        }
    }

    /// Pop the highest-priority activation from the agenda.
    ///
    /// Returns `None` if the agenda is empty.
    pub fn pop(&mut self) -> Option<Activation> {
        let (key, id) = self.ordering.pop_first()?;
        self.id_to_key.remove(id);

        let activation = self.activations.remove(id)?;
        if let Some(index) = &mut self.rule_ordering {
            remove_rule_key(index, activation.rule, &key);
        }

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
        let (_, &first) = self.ordering.first_key_value()?;
        if self.activations.get(first).is_some_and(&predicate) {
            return self.pop();
        }
        let mut target_key = None;
        let mut target_id = None;

        for (key, &id) in self.ordering.iter().skip(1) {
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
        if let Some(index) = &mut self.rule_ordering {
            remove_rule_key(index, activation.rule, &key);
        }
        remove_from_token_index(&mut self.token_to_activations, activation.token, id);

        Some(activation)
    }

    /// Pop the highest-priority activation whose rule is eligible.
    ///
    /// Eligibility must depend only on the rule and remain stable during this
    /// call. The predicate may be called in a different order or more than once
    /// for a rule. Activation-specific or stateful predicates use `pop_matching`.
    /// The first eligible entry takes the ordinary pop path; a focus miss builds
    /// a derived per-rule index so later calls inspect rule heads, not every
    /// activation belonging to a dormant rule.
    pub fn pop_matching_rule(&mut self, predicate: impl Fn(RuleId) -> bool) -> Option<Activation> {
        let (_, &first) = self.ordering.first_key_value()?;
        if self
            .activations
            .get(first)
            .is_some_and(|activation| predicate(activation.rule))
        {
            return self.pop();
        }
        let index = self.rule_ordering.get_or_insert_with(|| {
            let mut index = RuleOrdering::default();
            for (key, &id) in &self.ordering {
                if let Some(activation) = self.activations.get(id) {
                    index
                        .entry(activation.rule)
                        .or_default()
                        .insert(key.clone(), id);
                }
            }
            index
        });
        let (key, id) = index
            .iter()
            .filter(|(rule, _)| predicate(**rule))
            .filter_map(|(_, entries)| entries.first_key_value())
            .min_by(|(left, _), (right, _)| left.cmp(right))
            .map(|(key, &id)| (key.clone(), id))?;
        self.ordering.remove(&key);
        self.id_to_key.remove(id);
        let activation = self.activations.remove(id)?;
        remove_rule_key(self.rule_ordering.as_mut().unwrap(), activation.rule, &key);
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
                if let Some(index) = &mut self.rule_ordering {
                    if let Some(activation) = self.activations.get(id) {
                        remove_rule_key(index, activation.rule, &key);
                    }
                }
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
        if let Some(index) = &mut self.rule_ordering {
            index.remove(&rule_id);
        }
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

    /// Inspect activations in conflict-resolution order without consuming them.
    pub fn iter_ordered(&self) -> impl Iterator<Item = &Activation> {
        self.ordering
            .values()
            .filter_map(|id| self.activations.get(*id))
    }

    /// Get the current conflict resolution strategy.
    #[must_use]
    pub fn strategy(&self) -> ConflictResolutionStrategy {
        self.strategy
    }

    /// Change conflict resolution without replacing or recreating activations.
    ///
    /// Activation identity, creation order, recency and complexity survive the
    /// reorder. The per-rule index is rebuilt lazily on the next focus miss.
    pub fn set_strategy(
        &mut self,
        strategy: ConflictResolutionStrategy,
    ) -> ConflictResolutionStrategy {
        let previous = self.strategy;
        if previous != strategy {
            self.strategy = strategy;
            self.rule_ordering = None;
            self.ordering.clear();
            self.id_to_key.clear();
            for (id, activation) in &self.activations {
                let key = self.build_key(activation);
                self.ordering.insert(key.clone(), id);
                self.id_to_key.insert(id, key);
            }
        }
        previous
    }

    /// Clear all activations, preserving the strategy.
    pub fn clear(&mut self) {
        self.rule_ordering = None;
        self.ordering.clear();
        self.activations.clear();
        self.id_to_key.clear();
        self.token_to_activations.clear();
        // Reset next_seq to 0 since this is a full clear
        self.next_seq = ActivationSeq::ZERO;
    }

    /// Verify internal consistency of agenda indices.
    ///
    /// Available in all profiles so dependent crates can run release tests.
    pub fn debug_assert_consistency(&self) {
        self.validate_consistency()
            .expect("inconsistent engine state");
    }

    /// Validate activation identity and ordering indexes without panicking.
    #[doc(hidden)]
    #[allow(clippy::too_many_lines)]
    pub fn validate_consistency(&self) -> Result<(), String> {
        // 1. Every key in ordering references a live activation and a matching reverse key.
        for (key, activation_id) in &self.ordering {
            crate::snapshot::require!(
                self.activations.contains_key(*activation_id),
                "ordering references non-existent activation {activation_id:?}"
            );

            let reverse_key = self.id_to_key.get(*activation_id);
            crate::snapshot::require!(
                reverse_key.is_some(),
                "activation {activation_id:?} missing from id_to_key"
            );
            crate::snapshot::require_eq!(
                reverse_key,
                Some(key),
                "id_to_key mismatch for activation {activation_id:?}"
            );
        }

        // 2. Every reverse key references the same entry in ordering and a live activation.
        for (activation_id, key) in &self.id_to_key {
            crate::snapshot::require!(
                self.activations.contains_key(activation_id),
                "id_to_key references non-existent activation {activation_id:?}"
            );

            let ordered_id = self.ordering.get(key);
            crate::snapshot::require!(
                ordered_id.is_some(),
                "id_to_key key missing from ordering for activation {activation_id:?}"
            );
            crate::snapshot::require_eq!(
                ordered_id,
                Some(&activation_id),
                "ordering mismatch for activation {activation_id:?}"
            );
        }

        // 3. token_to_activations entries are non-empty and point to live activations
        //    whose token field matches the map key.
        for (token_id, activation_ids) in &self.token_to_activations {
            crate::snapshot::require!(
                !activation_ids.is_empty(),
                "token_to_activations contains empty entry for token {token_id:?}"
            );

            let unique: rustc_hash::FxHashSet<_> = activation_ids.iter().collect();
            crate::snapshot::require!(
                unique.len() == activation_ids.len(),
                "duplicate token activation index entry"
            );
            for activation_id in activation_ids {
                let activation = self.activations.get(*activation_id);
                crate::snapshot::require!(
                    activation.is_some(),
                    "token_to_activations references non-existent activation {activation_id:?}"
                );
                crate::snapshot::require_eq!(
                    activation.map(|a| a.token),
                    Some(*token_id),
                    "token_to_activations token mismatch for activation {activation_id:?}"
                );
            }
        }

        // 4. Every live activation appears in both reverse indices.
        for (activation_id, activation) in &self.activations {
            let key = self.id_to_key.get(activation_id);
            crate::snapshot::require!(
                key.is_some(),
                "live activation {activation_id:?} missing from id_to_key"
            );
            if let Some(k) = key {
                crate::snapshot::require_eq!(
                    self.ordering.get(k),
                    Some(&activation_id),
                    "live activation {activation_id:?} missing from ordering"
                );
            }

            let token_acts = self.token_to_activations.get(&activation.token);
            crate::snapshot::require!(
                token_acts.is_some(),
                "live activation {activation_id:?} missing from token_to_activations"
            );
            crate::snapshot::require!(
                token_acts.is_some_and(|ids| ids.contains(&activation_id)),
                "live activation {activation_id:?} not indexed under token {:?}",
                activation.token
            );
        }
        let mut sequences = rustc_hash::FxHashSet::default();
        for (id, activation) in &self.activations {
            crate::snapshot::require_eq!(id, activation.id);
            crate::snapshot::require!(
                sequences.insert(activation.activation_seq),
                "duplicate activation sequence"
            );
            crate::snapshot::require!(
                activation.activation_seq < self.next_seq,
                "activation sequence exceeds next sequence"
            );
            let key = self.build_key(activation);
            crate::snapshot::require!(
                self.id_to_key.get(id) == Some(&key),
                "activation ordering key does not match current strategy"
            );
        }

        if let Some(index) = &self.rule_ordering {
            let mut expected = RuleOrdering::default();
            for (key, &id) in &self.ordering {
                expected
                    .entry(self.activations[id].rule)
                    .or_default()
                    .insert(key.clone(), id);
            }
            crate::snapshot::require_eq!(index, &expected, "rule ordering index mismatch");
        }

        Ok(())
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
    fn sequence_exhaustion_preserves_live_chronology_and_indexes() {
        for strategy in [
            ConflictResolutionStrategy::Depth,
            ConflictResolutionStrategy::Breadth,
            ConflictResolutionStrategy::Lex,
            ConflictResolutionStrategy::Mea,
        ] {
            let mut agenda = Agenda::with_strategy(strategy);
            let mut tokens = SlotMap::<TokenId, ()>::with_key();
            let mut ids = Vec::new();
            agenda.next_seq = ActivationSeq::new(u64::MAX - 2);
            for rule in 0..3 {
                ids.push(agenda.add(Activation {
                    id: ActivationId::default(),
                    rule: RuleId(rule),
                    token: tokens.insert(()),
                    salience: Salience::DEFAULT,
                    timestamp: Timestamp::new(30 - u64::from(rule)),
                    complexity: 0,
                    activation_seq: ActivationSeq::ZERO,
                    recency: SmallVec::new(),
                }));
            }
            agenda.debug_assert_consistency();
            assert_eq!(agenda.next_seq.get(), 3);
            if strategy == ConflictResolutionStrategy::Depth {
                ids.reverse();
            }
            for id in ids {
                assert_eq!(agenda.pop().unwrap().id, id);
                agenda.debug_assert_consistency();
            }
            assert!(agenda.is_empty());
        }
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
            complexity: 0,
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
            complexity: 0,
            activation_seq: ActivationSeq::ZERO,
            recency: SmallVec::new(),
        });

        let id2 = agenda.add(Activation {
            id: ActivationId::default(),
            rule: RuleId(2),
            token: t2,
            salience: Salience::new(10), // Highest
            timestamp: Timestamp::new(100),
            complexity: 0,
            activation_seq: ActivationSeq::ZERO,
            recency: SmallVec::new(),
        });

        let _id3 = agenda.add(Activation {
            id: ActivationId::default(),
            rule: RuleId(3),
            token: t3,
            salience: Salience::new(-5), // Lowest
            timestamp: Timestamp::new(100),
            complexity: 0,
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
            complexity: 0,
            activation_seq: ActivationSeq::ZERO,
            recency: SmallVec::new(),
        });

        let _id2 = agenda.add(Activation {
            id: ActivationId::default(),
            rule: RuleId(2),
            token: t2,
            salience: Salience::DEFAULT,
            timestamp: Timestamp::new(200), // Most recent
            complexity: 0,
            activation_seq: ActivationSeq::ZERO,
            recency: SmallVec::new(),
        });

        let id3 = agenda.add(Activation {
            id: ActivationId::default(),
            rule: RuleId(3),
            token: t3,
            salience: Salience::DEFAULT,
            timestamp: Timestamp::new(150),
            complexity: 0,
            activation_seq: ActivationSeq::ZERO,
            recency: SmallVec::new(),
        });

        assert_eq!(agenda.len(), 3);

        // Newest activation wins even when supported by an older fact.
        let popped = agenda.pop().expect("Should have activation");
        assert_eq!(popped.id, id3);
        assert_eq!(popped.timestamp, Timestamp::new(150));
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
            complexity: 0,
            activation_seq: ActivationSeq::ZERO,
            recency: SmallVec::new(),
        });

        let id2 = agenda.add(Activation {
            id: ActivationId::default(),
            rule: RuleId(2),
            token: t1,
            salience: Salience::new(5),
            timestamp: Timestamp::new(200),
            complexity: 0,
            activation_seq: ActivationSeq::ZERO,
            recency: SmallVec::new(),
        });

        let id3 = agenda.add(Activation {
            id: ActivationId::default(),
            rule: RuleId(3),
            token: t2, // Different token
            salience: Salience::DEFAULT,
            timestamp: Timestamp::new(150),
            complexity: 0,
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
            complexity: 0,
            activation_seq: ActivationSeq::ZERO,
            recency: SmallVec::new(),
        });

        let _id2 = agenda.add(Activation {
            id: ActivationId::default(),
            rule: RuleId(2),
            token: t2,
            salience: Salience::DEFAULT,
            timestamp: Timestamp::new(300), // Most recent
            complexity: 0,
            activation_seq: ActivationSeq::ZERO,
            recency: SmallVec::new(),
        });

        let id3 = agenda.add(Activation {
            id: ActivationId::default(),
            rule: RuleId(3),
            token: t3,
            salience: Salience::DEFAULT,
            timestamp: Timestamp::new(200),
            complexity: 0,
            activation_seq: ActivationSeq::ZERO,
            recency: SmallVec::new(),
        });

        // Depth uses activation creation even when fact recency disagrees.
        let popped = agenda.pop().expect("Should have activation");
        assert_eq!(popped.id, id3);
        assert_eq!(popped.timestamp, Timestamp::new(200));
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
            timestamp: Timestamp::new(400), // Newer fact, oldest activation
            complexity: 0,
            activation_seq: ActivationSeq::ZERO,
            recency: SmallVec::new(),
        });

        let _id2 = agenda.add(Activation {
            id: ActivationId::default(),
            rule: RuleId(2),
            token: t2,
            salience: Salience::DEFAULT,
            timestamp: Timestamp::new(300),
            complexity: 0,
            activation_seq: ActivationSeq::ZERO,
            recency: SmallVec::new(),
        });

        let _id3 = agenda.add(Activation {
            id: ActivationId::default(),
            rule: RuleId(3),
            token: t3,
            salience: Salience::DEFAULT,
            timestamp: Timestamp::new(200),
            complexity: 0,
            activation_seq: ActivationSeq::ZERO,
            recency: SmallVec::new(),
        });

        // Breadth uses activation creation even when fact recency disagrees.
        let popped = agenda.pop().expect("Should have activation");
        assert_eq!(popped.id, id1);
        assert_eq!(popped.timestamp, Timestamp::new(400));
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
            complexity: 0,
            activation_seq: ActivationSeq::ZERO,
            recency: smallvec::smallvec![
                RecencyTag::from_timestamp(Timestamp::new(100)).unwrap(),
                RecencyTag::from_timestamp(Timestamp::new(200)).unwrap(),
                RecencyTag::from_timestamp(Timestamp::new(300)).unwrap()
            ], // [100, 200, 300]
        });

        let id2 = agenda.add(Activation {
            id: ActivationId::default(),
            rule: RuleId(2),
            token: t2,
            salience: Salience::DEFAULT,
            timestamp: Timestamp::new(300),
            complexity: 0,
            activation_seq: ActivationSeq::ZERO,
            recency: smallvec::smallvec![
                RecencyTag::from_timestamp(Timestamp::new(400)).unwrap(),
                RecencyTag::from_timestamp(Timestamp::new(100)).unwrap(),
                RecencyTag::from_timestamp(Timestamp::new(100)).unwrap()
            ], // [400, ...] wins lexicographically
        });

        let _id3 = agenda.add(Activation {
            id: ActivationId::default(),
            rule: RuleId(3),
            token: t3,
            salience: Salience::DEFAULT,
            timestamp: Timestamp::new(300),
            complexity: 0,
            activation_seq: ActivationSeq::ZERO,
            recency: smallvec::smallvec![
                RecencyTag::from_timestamp(Timestamp::new(100)).unwrap(),
                RecencyTag::from_timestamp(Timestamp::new(300)).unwrap(),
                RecencyTag::from_timestamp(Timestamp::new(100)).unwrap()
            ], // [100, 300, ...]
        });

        // LEX compares sorted vectors: [400,100,100] > [300,200,100] > [300,100,100].
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
            complexity: 0,
            activation_seq: ActivationSeq::ZERO,
            recency: smallvec::smallvec![
                RecencyTag::from_timestamp(Timestamp::new(100)).unwrap(),
                RecencyTag::from_timestamp(Timestamp::new(500)).unwrap()
            ], // First pattern: 100
        });

        let id2 = agenda.add(Activation {
            id: ActivationId::default(),
            rule: RuleId(2),
            token: t2,
            salience: Salience::DEFAULT,
            timestamp: Timestamp::new(300),
            complexity: 0,
            activation_seq: ActivationSeq::ZERO,
            recency: smallvec::smallvec![
                RecencyTag::from_timestamp(Timestamp::new(400)).unwrap(),
                RecencyTag::from_timestamp(Timestamp::new(100)).unwrap()
            ], // First pattern: 400 (highest)
        });

        let _id3 = agenda.add(Activation {
            id: ActivationId::default(),
            rule: RuleId(3),
            token: t3,
            salience: Salience::DEFAULT,
            timestamp: Timestamp::new(300),
            complexity: 0,
            activation_seq: ActivationSeq::ZERO,
            recency: smallvec::smallvec![
                RecencyTag::from_timestamp(Timestamp::new(200)).unwrap(),
                RecencyTag::from_timestamp(Timestamp::new(800)).unwrap()
            ], // First pattern: 200
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
            complexity: 0,
            activation_seq: ActivationSeq::ZERO,
            recency: smallvec::smallvec![
                RecencyTag::from_timestamp(Timestamp::new(100)).unwrap(),
                RecencyTag::from_timestamp(Timestamp::new(200)).unwrap(),
                RecencyTag::from_timestamp(Timestamp::new(300)).unwrap()
            ], // First: 100, rest: [200, 300]
        });

        let id2 = agenda.add(Activation {
            id: ActivationId::default(),
            rule: RuleId(2),
            token: t2,
            salience: Salience::DEFAULT,
            timestamp: Timestamp::new(300),
            complexity: 0,
            activation_seq: ActivationSeq::ZERO,
            recency: smallvec::smallvec![
                RecencyTag::from_timestamp(Timestamp::new(100)).unwrap(),
                RecencyTag::from_timestamp(Timestamp::new(500)).unwrap(),
                RecencyTag::from_timestamp(Timestamp::new(100)).unwrap()
            ], // First: 100, rest: [500, 100] wins
        });

        let _id3 = agenda.add(Activation {
            id: ActivationId::default(),
            rule: RuleId(3),
            token: t3,
            salience: Salience::DEFAULT,
            timestamp: Timestamp::new(300),
            complexity: 0,
            activation_seq: ActivationSeq::ZERO,
            recency: smallvec::smallvec![
                RecencyTag::from_timestamp(Timestamp::new(100)).unwrap(),
                RecencyTag::from_timestamp(Timestamp::new(300)).unwrap(),
                RecencyTag::from_timestamp(Timestamp::new(200)).unwrap()
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
            complexity: 0,
            activation_seq: ActivationSeq::ZERO,
            recency: SmallVec::new(),
        });

        let id2 = agenda.add(Activation {
            id: ActivationId::default(),
            rule: RuleId(2),
            token: t2,
            salience: Salience::new(10), // Higher salience wins
            timestamp: Timestamp::new(100),
            complexity: 0,
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
            complexity: 0,
            activation_seq: ActivationSeq::ZERO,
            recency: SmallVec::new(),
        });

        let _id2 = agenda.add(Activation {
            id: ActivationId::default(),
            rule: RuleId(2),
            token: t2,
            salience: Salience::DEFAULT,
            timestamp: Timestamp::new(100), // Lower timestamp (would win in breadth, but salience dominates)
            complexity: 0,
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
            complexity: 0,
            activation_seq: ActivationSeq::ZERO,
            recency: smallvec::smallvec![
                RecencyTag::from_timestamp(Timestamp::new(500)).unwrap(),
                RecencyTag::from_timestamp(Timestamp::new(500)).unwrap(),
                RecencyTag::from_timestamp(Timestamp::new(500)).unwrap()
            ], // Higher recency vector
        });

        let id2 = agenda.add(Activation {
            id: ActivationId::default(),
            rule: RuleId(2),
            token: t2,
            salience: Salience::new(8), // Higher salience wins
            timestamp: Timestamp::new(100),
            complexity: 0,
            activation_seq: ActivationSeq::ZERO,
            recency: smallvec::smallvec![
                RecencyTag::from_timestamp(Timestamp::new(100)).unwrap(),
                RecencyTag::from_timestamp(Timestamp::new(100)).unwrap(),
                RecencyTag::from_timestamp(Timestamp::new(100)).unwrap()
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
            complexity: 0,
            activation_seq: ActivationSeq::ZERO,
            recency: smallvec::smallvec![
                RecencyTag::from_timestamp(Timestamp::new(500)).unwrap(),
                RecencyTag::from_timestamp(Timestamp::new(500)).unwrap()
            ], // Higher first recency
        });

        let id2 = agenda.add(Activation {
            id: ActivationId::default(),
            rule: RuleId(2),
            token: t2,
            salience: Salience::new(3), // Higher salience wins
            timestamp: Timestamp::new(100),
            complexity: 0,
            activation_seq: ActivationSeq::ZERO,
            recency: smallvec::smallvec![
                RecencyTag::from_timestamp(Timestamp::new(100)).unwrap(),
                RecencyTag::from_timestamp(Timestamp::new(100)).unwrap()
            ],
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
            complexity: 0,
            activation_seq: ActivationSeq::ZERO, // Will be 0
            recency: SmallVec::new(),
        });

        let _id2 = agenda.add(Activation {
            id: ActivationId::default(),
            rule: RuleId(2),
            token: t2,
            salience: Salience::DEFAULT,
            timestamp: Timestamp::new(100),
            complexity: 0,
            activation_seq: ActivationSeq::ZERO, // Will be 1
            recency: SmallVec::new(),
        });

        let id3 = agenda.add(Activation {
            id: ActivationId::default(),
            rule: RuleId(3),
            token: t3,
            salience: Salience::DEFAULT,
            timestamp: Timestamp::new(100),
            complexity: 0,
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

        let id1 = agenda.add(Activation {
            id: ActivationId::default(),
            rule: RuleId(1),
            token: t1,
            salience: Salience::DEFAULT,
            timestamp: Timestamp::new(100),
            complexity: 0,
            activation_seq: ActivationSeq::ZERO,
            recency: SmallVec::new(),
        });

        let _id2 = agenda.add(Activation {
            id: ActivationId::default(),
            rule: RuleId(2),
            token: t2,
            salience: Salience::DEFAULT,
            timestamp: Timestamp::new(100),
            complexity: 0,
            activation_seq: ActivationSeq::ZERO,
            recency: SmallVec::new(),
        });

        let _id3 = agenda.add(Activation {
            id: ActivationId::default(),
            rule: RuleId(3),
            token: t3,
            salience: Salience::DEFAULT,
            timestamp: Timestamp::new(100),
            complexity: 0,
            activation_seq: ActivationSeq::ZERO,
            recency: SmallVec::new(),
        });

        let popped = agenda.pop().expect("Should have activation");
        assert_eq!(popped.id, id1);
        assert_eq!(popped.activation_seq, ActivationSeq::ZERO);
    }

    #[test]
    fn activation_seq_breaks_ties_lex() {
        let mut agenda = Agenda::with_strategy(ConflictResolutionStrategy::Lex);
        let t1 = make_token_id();
        let t2 = make_token_id();
        let t3 = make_token_id();

        let id1 = agenda.add(Activation {
            id: ActivationId::default(),
            rule: RuleId(1),
            token: t1,
            salience: Salience::DEFAULT,
            timestamp: Timestamp::new(100),
            complexity: 0,
            activation_seq: ActivationSeq::ZERO,
            recency: smallvec::smallvec![
                RecencyTag::from_timestamp(Timestamp::new(100)).unwrap(),
                RecencyTag::from_timestamp(Timestamp::new(200)).unwrap()
            ],
        });

        let _id2 = agenda.add(Activation {
            id: ActivationId::default(),
            rule: RuleId(2),
            token: t2,
            salience: Salience::DEFAULT,
            timestamp: Timestamp::new(100),
            complexity: 0,
            activation_seq: ActivationSeq::ZERO,
            recency: smallvec::smallvec![
                RecencyTag::from_timestamp(Timestamp::new(100)).unwrap(),
                RecencyTag::from_timestamp(Timestamp::new(200)).unwrap()
            ], // Same recency
        });

        let _id3 = agenda.add(Activation {
            id: ActivationId::default(),
            rule: RuleId(3),
            token: t3,
            salience: Salience::DEFAULT,
            timestamp: Timestamp::new(100),
            complexity: 0,
            activation_seq: ActivationSeq::ZERO,
            recency: smallvec::smallvec![
                RecencyTag::from_timestamp(Timestamp::new(100)).unwrap(),
                RecencyTag::from_timestamp(Timestamp::new(200)).unwrap()
            ], // Same recency
        });

        let popped = agenda.pop().expect("Should have activation");
        assert_eq!(popped.id, id1);
        assert_eq!(popped.activation_seq, ActivationSeq::ZERO);
    }

    #[test]
    fn activation_seq_breaks_ties_mea() {
        let mut agenda = Agenda::with_strategy(ConflictResolutionStrategy::Mea);
        let t1 = make_token_id();
        let t2 = make_token_id();
        let t3 = make_token_id();

        let id1 = agenda.add(Activation {
            id: ActivationId::default(),
            rule: RuleId(1),
            token: t1,
            salience: Salience::DEFAULT,
            timestamp: Timestamp::new(100),
            complexity: 0,
            activation_seq: ActivationSeq::ZERO,
            recency: smallvec::smallvec![
                RecencyTag::from_timestamp(Timestamp::new(100)).unwrap(),
                RecencyTag::from_timestamp(Timestamp::new(200)).unwrap()
            ],
        });

        let _id2 = agenda.add(Activation {
            id: ActivationId::default(),
            rule: RuleId(2),
            token: t2,
            salience: Salience::DEFAULT,
            timestamp: Timestamp::new(100),
            complexity: 0,
            activation_seq: ActivationSeq::ZERO,
            recency: smallvec::smallvec![
                RecencyTag::from_timestamp(Timestamp::new(100)).unwrap(),
                RecencyTag::from_timestamp(Timestamp::new(200)).unwrap()
            ], // Same recency
        });

        let _id3 = agenda.add(Activation {
            id: ActivationId::default(),
            rule: RuleId(3),
            token: t3,
            salience: Salience::DEFAULT,
            timestamp: Timestamp::new(100),
            complexity: 0,
            activation_seq: ActivationSeq::ZERO,
            recency: smallvec::smallvec![
                RecencyTag::from_timestamp(Timestamp::new(100)).unwrap(),
                RecencyTag::from_timestamp(Timestamp::new(200)).unwrap()
            ], // Same recency
        });

        let popped = agenda.pop().expect("Should have activation");
        assert_eq!(popped.id, id1);
        assert_eq!(popped.activation_seq, ActivationSeq::ZERO);
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
            complexity: 0,
            activation_seq: ActivationSeq::ZERO,
            recency: SmallVec::new(),
        });

        let id2 = agenda.add(Activation {
            id: ActivationId::default(),
            rule: RuleId(2),
            token: t2,
            salience: Salience::new(10), // highest salience
            timestamp: Timestamp::new(100),
            complexity: 0,
            activation_seq: ActivationSeq::ZERO,
            recency: SmallVec::new(),
        });

        agenda.add(Activation {
            id: ActivationId::default(),
            rule: RuleId(2),
            token: t3,
            salience: Salience::new(5),
            timestamp: Timestamp::new(100),
            complexity: 0,
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
            complexity: 0,
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
            complexity: 0,
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
            complexity: 0,
            activation_seq: ActivationSeq::ZERO,
            recency: SmallVec::new(),
        });
        let id2_depth = agenda_depth.add(Activation {
            id: ActivationId::default(),
            rule: RuleId(2),
            token: t2,
            salience: Salience::DEFAULT,
            timestamp: Timestamp::new(200), // Most recent
            complexity: 0,
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
            complexity: 0,
            activation_seq: ActivationSeq::ZERO,
            recency: SmallVec::new(),
        });
        let _id2_breadth = agenda_breadth.add(Activation {
            id: ActivationId::default(),
            rule: RuleId(2),
            token: t2,
            salience: Salience::DEFAULT,
            timestamp: Timestamp::new(200),
            complexity: 0,
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
            complexity: 0,
            activation_seq: ActivationSeq::ZERO,
            recency: SmallVec::new(),
        });

        let _id2 = agenda.add(Activation {
            id: ActivationId::default(),
            rule: RuleId(2),
            token: t2,
            salience: Salience::DEFAULT,
            timestamp: Timestamp::new(200),
            complexity: 0,
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
            complexity: 0,
            activation_seq: ActivationSeq::ZERO,
            recency: SmallVec::new(),
        });

        let _id2 = agenda.add(Activation {
            id: ActivationId::default(),
            rule: RuleId(2),
            token: t2,
            salience: Salience::DEFAULT,
            timestamp: Timestamp::new(200),
            complexity: 0,
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
            complexity: 0,
            activation_seq: ActivationSeq::ZERO,
            recency: smallvec::smallvec![
                RecencyTag::from_timestamp(Timestamp::new(100)).unwrap(),
                RecencyTag::from_timestamp(Timestamp::new(200)).unwrap()
            ],
        });

        let _id2 = agenda.add(Activation {
            id: ActivationId::default(),
            rule: RuleId(2),
            token: t2,
            salience: Salience::DEFAULT,
            timestamp: Timestamp::new(200),
            complexity: 0,
            activation_seq: ActivationSeq::ZERO,
            recency: smallvec::smallvec![
                RecencyTag::from_timestamp(Timestamp::new(200)).unwrap(),
                RecencyTag::from_timestamp(Timestamp::new(300)).unwrap()
            ],
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
            complexity: 0,
            activation_seq: ActivationSeq::ZERO,
            recency: smallvec::smallvec![
                RecencyTag::from_timestamp(Timestamp::new(100)).unwrap(),
                RecencyTag::from_timestamp(Timestamp::new(200)).unwrap()
            ],
        });

        let _id2 = agenda.add(Activation {
            id: ActivationId::default(),
            rule: RuleId(2),
            token: t2,
            salience: Salience::DEFAULT,
            timestamp: Timestamp::new(200),
            complexity: 0,
            activation_seq: ActivationSeq::ZERO,
            recency: smallvec::smallvec![
                RecencyTag::from_timestamp(Timestamp::new(200)).unwrap(),
                RecencyTag::from_timestamp(Timestamp::new(300)).unwrap()
            ],
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
            complexity: 0,
            activation_seq: ActivationSeq::ZERO,
            recency: SmallVec::new(),
        });
        let _id2 = agenda.add(Activation {
            id: ActivationId::default(),
            rule: RuleId(8),
            token: t2,
            salience: Salience::DEFAULT,
            timestamp: Timestamp::new(20),
            complexity: 0,
            activation_seq: ActivationSeq::ZERO,
            recency: SmallVec::new(),
        });
        let id3 = agenda.add(Activation {
            id: ActivationId::default(),
            rule: RuleId(7),
            token: t3,
            salience: Salience::DEFAULT,
            timestamp: Timestamp::new(30),
            complexity: 0,
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
            complexity: 0,
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

    #[test]
    fn eligible_front_keeps_rule_ordering_unallocated() {
        let (_tokens, tokens) = make_token_pool();
        for strategy in ALL_STRATEGIES {
            let mut agenda = Agenda::with_strategy(strategy);
            for index in 0..20 {
                agenda.add(make_activation(
                    RuleId(0),
                    tokens[index % tokens.len()],
                    Salience::DEFAULT,
                    Timestamp::new(u64::try_from(index).unwrap()),
                ));
            }
            while agenda.pop_matching_rule(|_| true).is_some() {
                assert!(agenda.rule_ordering.is_none());
                agenda.debug_assert_consistency();
            }
        }
    }

    #[test]
    fn activation_predicates_keep_priority_order_and_one_call_per_entry() {
        let (_tokens, tokens) = make_token_pool();
        for target in [2, 1, 9] {
            let mut agenda = Agenda::new();
            for rule in 0..3 {
                agenda.add(make_activation(
                    RuleId(rule),
                    tokens[0],
                    Salience::new(i32::try_from(rule).unwrap()),
                    Timestamp::ZERO,
                ));
            }
            assert!(agenda.pop_matching_rule(|_| false).is_none());
            let calls = std::cell::RefCell::new(Vec::new());
            let selected = agenda.pop_matching(|activation| {
                calls.borrow_mut().push(activation.rule.0);
                activation.rule.0 == target
            });
            assert_eq!(selected.map(|a| a.rule.0), (target < 3).then_some(target));
            assert_eq!(
                *calls.borrow(),
                match target {
                    2 => vec![2],
                    1 => vec![2, 1],
                    _ => vec![2, 1, 0],
                }
            );
            agenda.debug_assert_consistency();
        }
    }

    proptest! {
        #[test]
        fn rule_selection_matches_scans_through_arbitrary_mutations(
            operations in prop::collection::vec((0u8..10, 0u8..5, 0usize..5, 0u8..32, -10i32..10, 0u64..1000), 0..120),
        ) {
            let (_tokens, tokens) = make_token_pool();
            for strategy in ALL_STRATEGIES {
                let mut indexed = Agenda::with_strategy(strategy);
                let mut scanned = Agenda::with_strategy(strategy);
                for &(operation, rule, token, mask, salience, timestamp) in &operations {
                    let rule = RuleId(u32::from(rule));
                    let token = tokens[token];
                    match operation {
                        0..=2 | 8 => {
                            if operation == 8 {
                                indexed.next_seq = ActivationSeq::new(u64::MAX);
                                scanned.next_seq = ActivationSeq::new(u64::MAX);
                            }
                            let mut activation = make_activation(rule, token, Salience::new(salience), Timestamp::new(timestamp));
                            activation.recency = SmallVec::from_slice(&[RecencyTag::from_timestamp(Timestamp::new(timestamp)).unwrap(), RecencyTag::from_timestamp(Timestamp::new(timestamp ^ 7)).unwrap()]);
                            prop_assert_eq!(indexed.add(activation.clone()), scanned.add(activation));
                        }
                        3 => {
                            let eligible = |rule: RuleId| mask & (1 << rule.0) != 0;
                            prop_assert_eq!(indexed.pop_matching_rule(eligible).map(|a| a.id), scanned.pop_matching(|a| eligible(a.rule)).map(|a| a.id));
                        }
                        4 => prop_assert_eq!(indexed.pop().map(|a| a.id), scanned.pop().map(|a| a.id)),
                        5 => prop_assert_eq!(indexed.remove_activations_for_token(token).iter().map(|a| a.id).collect::<Vec<_>>(), scanned.remove_activations_for_token(token).iter().map(|a| a.id).collect::<Vec<_>>()),
                        6 => prop_assert_eq!(indexed.remove_activations_for_rule(rule).iter().map(|a| a.id).collect::<Vec<_>>(), scanned.remove_activations_for_rule(rule).iter().map(|a| a.id).collect::<Vec<_>>()),
                        7 => { indexed.clear(); scanned.clear(); }
                        _ => {
                            let eligible = |a: &Activation| a.timestamp.get() % 2 == u64::from(mask % 2);
                            prop_assert_eq!(indexed.pop_matching(eligible).map(|a| a.id), scanned.pop_matching(eligible).map(|a| a.id));
                        }
                    }
                    // Populate the derived index without changing membership,
                    // so the next mutation must maintain it.
                    prop_assert!(indexed.pop_matching_rule(|_| false).is_none());
                    prop_assert_eq!(indexed.len(), scanned.len());
                    indexed.debug_assert_consistency();
                    scanned.debug_assert_consistency();
                }
            }
        }
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

        /// Exact ties favor the oldest activation except under Depth.
        #[test]
        fn activation_seq_tiebreaker(sal in -10i32..10, ts in 1u64..1000) {
            let (_map, tokens) = make_token_pool();
            for &strategy in &ALL_STRATEGIES {
                let mut agenda = Agenda::with_strategy(strategy);
                let first = agenda.add(make_activation(
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
                    if strategy == ConflictResolutionStrategy::Depth { second } else { first },
                    "strategy {:?}: creation order must follow the selected strategy",
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

#[cfg(test)]
mod clips_ordering_tests {
    use super::*;
    fn fact(timestamp: u64) -> RecencyTag {
        RecencyTag::from_timestamp(Timestamp::new(timestamp)).unwrap()
    }

    fn activation(rule: u32, recency: &[RecencyTag], complexity: u16) -> Activation {
        Activation {
            id: ActivationId::default(),
            rule: RuleId(rule),
            token: TokenId::default(),
            salience: Salience::DEFAULT,
            complexity,
            timestamp: recency
                .iter()
                .filter_map(|tag| tag.timestamp())
                .max()
                .unwrap_or(Timestamp::ZERO),
            activation_seq: ActivationSeq::ZERO,
            recency: SmallVec::from_slice(recency),
        }
    }

    fn drain(agenda: &mut Agenda) -> Vec<u32> {
        std::iter::from_fn(|| agenda.pop().map(|activation| activation.rule.0)).collect()
    }

    #[test]
    fn tags_distinguish_absence_from_first_fact_and_preserve_timestamp_limits() {
        assert!(RecencyTag::ABSENT < fact(0));
        assert_eq!(RecencyTag::ABSENT.timestamp(), None);
        assert_eq!(fact(0).timestamp(), Some(Timestamp::ZERO));
        assert_eq!(
            fact(u64::MAX - 1).timestamp(),
            Some(Timestamp::new(u64::MAX - 1))
        );
        assert_eq!(RecencyTag::from_timestamp(Timestamp::new(u64::MAX)), None);
    }

    #[test]
    fn lex_sorts_the_entire_basis_and_prefers_a_longer_equal_prefix() {
        let mut agenda = Agenda::with_strategy(ConflictResolutionStrategy::Lex);
        agenda.add(activation(1, &[fact(8), fact(1)], 2047));
        agenda.add(activation(2, &[fact(1), fact(9)], 0));
        agenda.add(activation(3, &[fact(9), fact(1), RecencyTag::ABSENT], 0));
        assert_eq!(drain(&mut agenda), vec![3, 2, 1]);
    }

    #[test]
    fn mea_uses_original_first_element_then_the_complete_sorted_basis() {
        let mut agenda = Agenda::with_strategy(ConflictResolutionStrategy::Mea);
        agenda.add(activation(1, &[RecencyTag::ABSENT, fact(100)], 2047));
        agenda.add(activation(2, &[fact(0), fact(9), fact(2)], 0));
        agenda.add(activation(3, &[fact(0), fact(2), fact(10)], 0));
        agenda.add(activation(
            4,
            &[fact(0), fact(10), fact(2), RecencyTag::ABSENT],
            0,
        ));
        assert_eq!(drain(&mut agenda), vec![4, 3, 2, 1]);
    }

    #[test]
    fn complexity_precedes_oldest_tie_and_salience_precedes_recency() {
        for strategy in [
            ConflictResolutionStrategy::Lex,
            ConflictResolutionStrategy::Mea,
        ] {
            let mut agenda = Agenda::with_strategy(strategy);
            agenda.add(activation(1, &[fact(1)], 4));
            agenda.add(activation(2, &[fact(1)], 5));
            agenda.add(activation(3, &[fact(1)], 5));
            let mut salient = activation(4, &[RecencyTag::ABSENT], 0);
            salient.salience = Salience::new(1);
            agenda.add(salient);
            assert_eq!(drain(&mut agenda), vec![4, 2, 3, 1]);
        }
    }

    #[test]
    fn depth_and_breadth_ignore_complexity_and_recency() {
        for (strategy, expected) in [
            (ConflictResolutionStrategy::Depth, vec![3, 2, 1]),
            (ConflictResolutionStrategy::Breadth, vec![1, 2, 3]),
        ] {
            let mut agenda = Agenda::with_strategy(strategy);
            agenda.add(activation(1, &[fact(99)], 2047));
            agenda.add(activation(2, &[fact(3)], 0));
            agenda.add(activation(3, &[RecencyTag::ABSENT], 4));
            assert_eq!(drain(&mut agenda), expected);
        }
    }

    #[test]
    fn canceled_and_recreated_match_follows_older_equal_match_even_after_rebase() {
        for strategy in [
            ConflictResolutionStrategy::Lex,
            ConflictResolutionStrategy::Mea,
        ] {
            let mut agenda = Agenda::with_strategy(strategy);
            agenda.add(activation(1, &[fact(1), RecencyTag::ABSENT], 2));
            agenda.add(activation(2, &[fact(1), RecencyTag::ABSENT], 2));
            // A focus miss also materializes the derived rule index before mutation.
            assert!(agenda.pop_matching_rule(|_| false).is_none());
            agenda.remove_activations_for_rule(RuleId(1));
            agenda.next_seq = ActivationSeq::new(u64::MAX);
            agenda.add(activation(1, &[fact(1), RecencyTag::ABSENT], 2));
            agenda.validate_consistency().unwrap();
            assert_eq!(
                agenda
                    .pop_matching_rule(|rule| rule == RuleId(2))
                    .unwrap()
                    .rule,
                RuleId(2)
            );
            assert_eq!(drain(&mut agenda), vec![1]);
        }
    }

    #[test]
    fn recency_and_complexity_survive_rebuilding_under_another_strategy() {
        let source = [
            activation(1, &[fact(0), fact(9)], 4),
            activation(2, &[fact(8), fact(1)], 5),
            activation(3, &[fact(0), fact(9)], 5),
        ];
        let mut depth = Agenda::new();
        for activation in source {
            depth.add(activation);
        }
        let mut retained: Vec<_> = depth.iter_activations().cloned().collect();
        retained.sort_by_key(|activation| activation.activation_seq);
        for (strategy, expected) in [
            (ConflictResolutionStrategy::Lex, vec![3, 1, 2]),
            (ConflictResolutionStrategy::Mea, vec![2, 3, 1]),
        ] {
            let mut agenda = Agenda::with_strategy(strategy);
            for activation in &retained {
                agenda.add(activation.clone());
            }
            assert_eq!(drain(&mut agenda), expected);
        }
        assert_eq!(retained[0].recency.as_slice(), &[fact(0), fact(9)]);
    }

    #[test]
    fn switching_live_strategy_retains_identity_and_chronology() {
        let mut agenda = Agenda::new();
        let first = agenda.add(activation(1, &[fact(0), fact(9)], 4));
        let removed = agenda.add(activation(9, &[fact(1)], 0));
        let second = agenda.add(activation(2, &[fact(8), fact(1)], 5));
        let third = agenda.add(activation(3, &[fact(0), fact(9)], 5));
        agenda.remove_activations_for_rule(RuleId(9));
        assert!(agenda.get(removed).is_none());
        let next_seq = agenda.next_seq;
        let retained: Vec<_> = [first, second, third]
            .into_iter()
            .map(|id| (id, agenda.get(id).unwrap().clone()))
            .collect();
        for (strategy, expected) in [
            (ConflictResolutionStrategy::Breadth, vec![1, 2, 3]),
            (ConflictResolutionStrategy::Lex, vec![3, 1, 2]),
            (ConflictResolutionStrategy::Mea, vec![2, 3, 1]),
            (ConflictResolutionStrategy::Depth, vec![3, 2, 1]),
        ] {
            // Materialize the focus index before each change, then verify that
            // switching does not leave its old priority keys behind.
            assert!(agenda
                .pop_matching_rule(|rule| rule == RuleId(99))
                .is_none());
            let previous = agenda.strategy();
            assert_eq!(agenda.set_strategy(strategy), previous);
            assert_eq!(agenda.next_seq, next_seq);
            assert_eq!(
                agenda.iter_ordered().map(|a| a.rule.0).collect::<Vec<_>>(),
                expected
            );
            for (id, before) in &retained {
                let after = agenda.get(*id).unwrap();
                assert_eq!(after.id, before.id);
                assert_eq!(after.activation_seq, before.activation_seq);
                assert_eq!(after.recency, before.recency);
                assert_eq!(after.complexity, before.complexity);
            }
            agenda.debug_assert_consistency();
        }
        assert_eq!(
            agenda
                .pop_matching_rule(|rule| rule == RuleId(2))
                .unwrap()
                .id,
            second
        );
        assert_eq!(
            agenda
                .remove_activations_for_token(TokenId::default())
                .len(),
            2
        );
        agenda.debug_assert_consistency();
    }
}
