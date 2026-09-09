//! Alpha network and alpha memory.
//!
//! The alpha network is the first stage of the Rete algorithm. It discriminates
//! facts by type (template or ordered relation) and applies constant tests.

use crate::ordered_set::OrderedSet;
use rustc_hash::{FxHashMap as HashMap, FxHashSet as HashSet};
use slotmap::SparseSecondaryMap;
use smallvec::SmallVec;
use std::cmp::Ordering;

use crate::fact::{Fact, FactBase, FactId, TemplateId};
use crate::symbol::Symbol;
use crate::token::NodeId;
use crate::tracing_support::ferric_span;
use crate::value::{AtomKey, Value};

/// A simple way to reference a field in a fact.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum SlotIndex {
    /// Ordered fact field by position (0-based).
    Ordered(usize),
    /// Template fact slot by position (0-based).
    Template(usize),
}

/// Alpha entry type: identifies facts by their type (template or ordered relation).
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum AlphaEntryType {
    Template(TemplateId),
    OrderedRelation(Symbol),
}

/// Unique identifier for an alpha memory.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct AlphaMemoryId(pub u32);

/// A constant test applied to a fact or one of its slots.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct ConstantTest {
    /// Slot inspected by value tests; ignored by ordered field-count tests.
    pub slot: SlotIndex,
    pub test_type: ConstantTestType,
}

/// The type of constant test to perform.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum ConstantTestType {
    Equal(AtomKey),
    NotEqual(AtomKey),
    /// Disjunctive equality: slot value must equal any one of the keys.
    EqualAny(Vec<AtomKey>),
    /// Numeric greater-than comparison against an atom key.
    GreaterThan(AtomKey),
    /// Numeric less-than comparison against an atom key.
    LessThan(AtomKey),
    /// Numeric greater-than-or-equal comparison against an atom key.
    GreaterOrEqual(AtomKey),
    /// Numeric less-than-or-equal comparison against an atom key.
    LessOrEqual(AtomKey),
    /// Slot equality: current slot value must equal another slot value.
    EqualSlot(SlotIndex),
    /// Slot inequality: current slot value must not equal another slot value.
    NotEqualSlot(SlotIndex),
    /// Numeric slot equality against another slot plus integer offset.
    EqualSlotOffset(SlotIndex, i64),
    /// Numeric slot inequality against another slot plus integer offset.
    NotEqualSlotOffset(SlotIndex, i64),
    /// Numeric slot greater-than against another slot plus integer offset.
    GreaterThanSlotOffset(SlotIndex, i64),
    /// Numeric slot less-than against another slot plus integer offset.
    LessThanSlotOffset(SlotIndex, i64),
    /// Numeric slot greater-or-equal against another slot plus integer offset.
    GreaterOrEqualSlotOffset(SlotIndex, i64),
    /// Numeric slot less-or-equal against another slot plus integer offset.
    LessOrEqualSlotOffset(SlotIndex, i64),
    /// Ordered fact cardinality, independent of any individual slot.
    /// An absent maximum permits a variable-length multifield match.
    OrderedFieldCount {
        min: usize,
        max: Option<usize>,
    },
}

/// An alpha network node.
///
/// Entry nodes discriminate by fact type; constant test nodes apply tests to slots.
/// Nodes may have an attached alpha memory that stores matching facts.
#[derive(Clone, Debug)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum AlphaNode {
    Entry {
        entry_type: AlphaEntryType,
        children: Vec<NodeId>,
        memory: Option<AlphaMemoryId>,
    },
    ConstantTest {
        test: ConstantTest,
        children: Vec<NodeId>,
        memory: Option<AlphaMemoryId>,
    },
    RuntimePredicate {
        condition: crate::rete::RuntimeCondition,
        children: Vec<NodeId>,
        memory: Option<AlphaMemoryId>,
    },
}

impl AlphaNode {
    #[must_use]
    fn memory(&self) -> Option<AlphaMemoryId> {
        match self {
            Self::Entry { memory, .. }
            | Self::ConstantTest { memory, .. }
            | Self::RuntimePredicate { memory, .. } => *memory,
        }
    }

    fn memory_mut(&mut self) -> &mut Option<AlphaMemoryId> {
        match self {
            Self::Entry { memory, .. }
            | Self::ConstantTest { memory, .. }
            | Self::RuntimePredicate { memory, .. } => memory,
        }
    }

    #[must_use]
    fn children(&self) -> &[NodeId] {
        match self {
            Self::Entry { children, .. }
            | Self::ConstantTest { children, .. }
            | Self::RuntimePredicate { children, .. } => children,
        }
    }

    fn children_mut(&mut self) -> &mut Vec<NodeId> {
        match self {
            Self::Entry { children, .. }
            | Self::ConstantTest { children, .. }
            | Self::RuntimePredicate { children, .. } => children,
        }
    }
}

/// Alpha memory: stores facts that match a particular alpha network path.
///
/// Maintains a set of facts and optional slot indices for efficient lookup
/// by slot value.
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct AlphaMemory {
    pub id: AlphaMemoryId,
    pub(crate) facts: OrderedSet<FactId>,
    /// Slot indices: `SlotIndex` -> `AtomKey` -> `FactId`s with that key in that slot.
    #[cfg_attr(
        feature = "serde",
        serde(with = "crate::serde_helpers::fx_hash_map_of_fx_hash_map")
    )]
    pub(crate) slot_indices: HashMap<SlotIndex, HashMap<AtomKey, OrderedSet<FactId>>>,
    /// Which slots are currently indexed.
    pub(crate) indexed_slots: SmallVec<[SlotIndex; 4]>,
}

impl AlphaMemory {
    /// Create a new, empty alpha memory.
    #[must_use]
    pub fn new(id: AlphaMemoryId) -> Self {
        Self {
            id,
            facts: OrderedSet::default(),
            slot_indices: HashMap::default(),
            indexed_slots: SmallVec::new(),
        }
    }

    /// Insert a fact into the memory.
    ///
    /// If the fact is already present, this is a no-op.
    /// Updates all requested slot indices.
    pub fn insert(&mut self, fact_id: FactId, fact: &Fact) {
        if !self.facts.insert(fact_id) {
            // Fact already present, no-op
            return;
        }

        // Update all indexed slots
        for &slot in &self.indexed_slots {
            if let Some(value) = get_slot_value(fact, slot) {
                if let Some(key) = AtomKey::from_value(value) {
                    self.slot_indices
                        .entry(slot)
                        .or_default()
                        .entry(key)
                        .or_default()
                        .insert(fact_id);
                }
            }
        }
    }

    /// Remove a fact from the memory.
    ///
    /// Updates all slot indices and prunes empty entries eagerly.
    pub fn remove(&mut self, fact_id: FactId, fact: &Fact) {
        if !self.facts.remove(&fact_id) {
            // Fact not present, no-op
            return;
        }

        // Update all indexed slots
        for &slot in &self.indexed_slots {
            if let Some(value) = get_slot_value(fact, slot) {
                if let Some(key) = AtomKey::from_value(value) {
                    remove_from_slot_index(&mut self.slot_indices, slot, &key, fact_id);
                }
            }
        }
    }

    /// Request indexing on a particular slot.
    ///
    /// Backfills existing facts into the index.
    pub fn request_index(&mut self, slot: SlotIndex, fact_base: &FactBase) {
        if self.indexed_slots.contains(&slot) {
            // Already indexed
            return;
        }

        self.indexed_slots.push(slot);

        // Backfill existing facts
        for &fact_id in &self.facts {
            if let Some(entry) = fact_base.get(fact_id) {
                if let Some(value) = get_slot_value(&entry.fact, slot) {
                    if let Some(key) = AtomKey::from_value(value) {
                        self.slot_indices
                            .entry(slot)
                            .or_default()
                            .entry(key)
                            .or_default()
                            .insert(fact_id);
                    }
                }
            }
        }
    }

    /// Request indexing on a particular slot without backfilling.
    ///
    /// Use this at compile time when the alpha memory is known to be empty.
    /// The slot will be indexed for all future `insert()` calls.
    pub fn request_index_empty(&mut self, slot: SlotIndex) {
        assert!(
            self.facts.is_empty(),
            "request_index_empty called on non-empty alpha memory; use request_index() to backfill"
        );
        if !self.indexed_slots.contains(&slot) {
            self.indexed_slots.push(slot);
        }
    }

    /// Returns `true` if the given slot has been requested for indexing.
    #[must_use]
    pub fn is_slot_indexed(&self, slot: SlotIndex) -> bool {
        self.indexed_slots.contains(&slot)
    }

    /// Lookup facts by slot value.
    ///
    /// Returns `None` if the slot is not indexed or the key is not present.
    #[must_use]
    pub fn lookup_by_slot(
        &self,
        slot: SlotIndex,
        key: &AtomKey,
    ) -> Option<impl ExactSizeIterator<Item = FactId> + '_> {
        Some(self.slot_indices.get(&slot)?.get(key)?.iter().copied())
    }

    /// Clear all facts and indices from this memory, preserving its ID and structure.
    pub fn clear(&mut self) {
        self.facts.clear();
        self.slot_indices.clear();
        // indexed_slots stays — it tracks which slots SHOULD be indexed
    }

    /// Iterate over fact IDs in insertion order (oldest first).
    pub fn iter(&self) -> impl Iterator<Item = FactId> + '_ {
        self.facts.iter().copied()
    }

    /// Check if a fact is in this memory.
    #[must_use]
    pub fn contains(&self, fact_id: FactId) -> bool {
        self.facts.contains(&fact_id)
    }

    /// Returns `true` if this memory contains no facts.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.facts.is_empty()
    }

    /// Returns the number of facts in this memory.
    #[must_use]
    pub fn len(&self) -> usize {
        self.facts.len()
    }
}

/// Helper: extract a value from a fact by slot index.
///
/// Returns `None` if the slot index is out of bounds.
#[must_use]
pub fn get_slot_value(fact: &Fact, slot: SlotIndex) -> Option<&Value> {
    match (fact, slot) {
        (Fact::Ordered(ordered), SlotIndex::Ordered(i)) => ordered.fields.get(i),
        (Fact::Template(template), SlotIndex::Template(i)) => template.slots.get(i),
        _ => None,
    }
}

fn remove_from_slot_index(
    slot_indices: &mut HashMap<SlotIndex, HashMap<AtomKey, OrderedSet<FactId>>>,
    slot: SlotIndex,
    key: &AtomKey,
    fact_id: FactId,
) {
    let mut remove_slot = false;
    if let Some(key_map) = slot_indices.get_mut(&slot) {
        if let Some(fact_set) = key_map.get_mut(key) {
            fact_set.remove(&fact_id);
            if fact_set.is_empty() {
                key_map.remove(key);
            }
        }
        remove_slot = key_map.is_empty();
    }

    if remove_slot {
        slot_indices.remove(&slot);
    }
}

/// The alpha network.
///
/// Stores all alpha nodes and memories, and provides methods to propagate facts
/// through the network.
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct AlphaNetwork {
    pub(crate) nodes: Vec<AlphaNode>,
    pub(crate) memories: Vec<AlphaMemory>,
    /// Entry points: `AlphaEntryType` -> `NodeId`.
    #[cfg_attr(feature = "serde", serde(with = "crate::serde_helpers::fx_hash_map"))]
    pub(crate) entry_nodes: HashMap<AlphaEntryType, NodeId>,
    /// Reverse index: which alpha memories contain each fact.
    /// Populated on assertion, pruned on retraction. Eliminates the full
    /// alpha-memory scan in `memories_containing_fact`.
    pub(crate) fact_to_memories: SparseSecondaryMap<FactId, SmallVec<[AlphaMemoryId; 4]>>,
    #[cfg_attr(feature = "serde", serde(skip, default))]
    pub(crate) pending_runtime: Vec<(NodeId, FactId)>,
    pub(crate) next_node_id: u32,
    pub(crate) next_memory_id: u32,
}

impl AlphaNetwork {
    /// Create a new, empty alpha network.
    #[must_use]
    pub fn new() -> Self {
        Self {
            nodes: Vec::new(),
            memories: Vec::new(),
            entry_nodes: HashMap::default(),
            fact_to_memories: SparseSecondaryMap::new(),
            pending_runtime: Vec::new(),
            next_node_id: 0,
            next_memory_id: 0,
        }
    }

    pub(crate) fn take_runtime_requests(&mut self) -> Vec<(NodeId, FactId)> {
        std::mem::take(&mut self.pending_runtime)
    }

    pub(crate) fn create_runtime_predicate_node(
        &mut self,
        parent: NodeId,
        condition: crate::rete::RuntimeCondition,
    ) -> NodeId {
        let id = NodeId(self.next_node_id);
        self.next_node_id += 1;
        self.nodes.push(AlphaNode::RuntimePredicate {
            condition,
            children: Vec::new(),
            memory: None,
        });
        self.node_mut(parent)
            .expect("alpha parent exists")
            .children_mut()
            .push(id);
        id
    }

    pub(crate) fn backfill_runtime_predicate(
        &mut self,
        node: NodeId,
        entry_type: &AlphaEntryType,
        tests: &[ConstantTest],
        facts: &FactBase,
    ) {
        let mut candidates: Vec<_> = facts
            .iter()
            .filter(|(_, entry)| {
                fact_matches_entry_type(&entry.fact, entry_type)
                    && tests.iter().all(|test| evaluate_test(&entry.fact, test))
            })
            .map(|(id, entry)| (entry.timestamp, id))
            .collect();
        candidates.sort_unstable_by_key(|(timestamp, _)| *timestamp);
        self.pending_runtime
            .extend(candidates.into_iter().map(|(_, fact)| (node, fact)));
    }

    pub(crate) fn resolve_runtime_predicate(
        &mut self,
        node: NodeId,
        fact_id: FactId,
        fact: &Fact,
        passed: bool,
    ) -> Option<AlphaMemoryId> {
        let Some(AlphaNode::RuntimePredicate {
            memory: Some(memory),
            ..
        }) = self.node(node)
        else {
            return None;
        };
        let memory = *memory;
        if !passed || self.memory(memory)?.facts.contains(&fact_id) {
            return None;
        }
        self.memory_mut(memory)?.insert(fact_id, fact);
        if let Some(memories) = self.fact_to_memories.get_mut(fact_id) {
            if !memories.contains(&memory) {
                memories.push(memory);
            }
        } else {
            self.fact_to_memories
                .insert(fact_id, SmallVec::from_slice(&[memory]));
        }
        Some(memory)
    }

    pub(crate) fn structural_counts(&self) -> (usize, usize) {
        (self.nodes.len(), self.memories.len())
    }

    /// Fact identities referenced by compiled alpha paths.
    pub fn entry_types(&self) -> impl Iterator<Item = &AlphaEntryType> {
        self.entry_nodes.keys()
    }

    /// Reclaim paths no longer consumed by a live beta node. Compaction
    /// preserves fact membership and returns the old-to-new memory mapping.
    pub(crate) fn retain_memories(
        &mut self,
        retained: &HashSet<AlphaMemoryId>,
    ) -> Vec<Option<AlphaMemoryId>> {
        let mut memory_map = vec![None; self.memories.len()];
        let mut memories = Vec::with_capacity(retained.len());
        for mut memory in self.memories.drain(..) {
            if retained.contains(&memory.id) {
                let new_id = AlphaMemoryId(u32::try_from(memories.len()).unwrap());
                memory_map[memory.id.0 as usize] = Some(new_id);
                memory.id = new_id;
                memories.push(memory);
            }
        }
        self.memories = memories;

        // Alpha paths are created parent-first. Keep a node if it owns a live
        // memory or leads to one; no historical rule ownership table is needed.
        let mut keep = vec![false; self.nodes.len()];
        for (index, node) in self.nodes.iter().enumerate().rev() {
            keep[index] = node.memory().is_some_and(|id| retained.contains(&id))
                || node.children().iter().any(|id| keep[id.0 as usize]);
        }
        let mut node_map = vec![None; self.nodes.len()];
        let mut next = 0;
        for (index, &live) in keep.iter().enumerate() {
            if live {
                node_map[index] = Some(NodeId(next));
                next += 1;
            }
        }
        self.nodes = self
            .nodes
            .drain(..)
            .enumerate()
            .filter_map(|(index, mut node)| {
                if !keep[index] {
                    return None;
                }
                *node.children_mut() = node
                    .children()
                    .iter()
                    .filter_map(|id| node_map[id.0 as usize])
                    .collect();
                *node.memory_mut() = node.memory().and_then(|id| memory_map[id.0 as usize]);
                Some(node)
            })
            .collect();
        self.entry_nodes.retain(|_, id| {
            if let Some(new_id) = node_map[id.0 as usize] {
                *id = new_id;
                true
            } else {
                false
            }
        });
        self.fact_to_memories.retain(|_, ids| {
            *ids = ids
                .iter()
                .filter_map(|id| memory_map[id.0 as usize])
                .collect();
            !ids.is_empty()
        });
        self.next_node_id = next;
        self.next_memory_id = u32::try_from(self.memories.len()).unwrap();
        memory_map
    }

    /// Whether a compiled alpha path references this fact type.
    #[must_use]
    pub fn contains_entry(&self, entry_type: &AlphaEntryType) -> bool {
        self.entry_nodes.contains_key(entry_type)
    }

    /// Get or create an entry node for a given entry type.
    ///
    /// Entry nodes are unique per entry type (idempotent).
    #[allow(clippy::cast_possible_truncation)] // Node count will never reach u32::MAX in practice.
    pub fn create_entry_node(&mut self, entry_type: AlphaEntryType) -> NodeId {
        if let Some(&node_id) = self.entry_nodes.get(&entry_type) {
            return node_id;
        }

        let node_id = NodeId(self.next_node_id);
        self.next_node_id += 1;

        let node = AlphaNode::Entry {
            entry_type: entry_type.clone(),
            children: Vec::new(),
            memory: None,
        };

        debug_assert_eq!(self.nodes.len(), node_id.0 as usize);
        self.nodes.push(node);
        self.entry_nodes.insert(entry_type, node_id);

        node_id
    }

    /// Create a constant test node as a child of the given parent node.
    ///
    /// Returns the new node's ID.
    #[allow(clippy::cast_possible_truncation)] // Node count will never reach u32::MAX in practice.
    pub fn create_constant_test_node(&mut self, parent: NodeId, test: ConstantTest) -> NodeId {
        let node_id = NodeId(self.next_node_id);
        self.next_node_id += 1;

        let node = AlphaNode::ConstantTest {
            test,
            children: Vec::new(),
            memory: None,
        };

        debug_assert_eq!(self.nodes.len(), node_id.0 as usize);
        self.nodes.push(node);

        // Add this node as a child of the parent
        if let Some(parent_node) = self.node_mut(parent) {
            parent_node.children_mut().push(node_id);
        }

        node_id
    }

    /// Create and attach a memory to a node.
    ///
    /// Returns the memory ID.
    #[allow(clippy::cast_possible_truncation)] // Memory count will never reach u32::MAX in practice.
    pub fn create_memory(&mut self, node_id: NodeId) -> AlphaMemoryId {
        let memory_id = AlphaMemoryId(self.next_memory_id);
        self.next_memory_id += 1;

        let memory = AlphaMemory::new(memory_id);
        debug_assert_eq!(self.memories.len(), memory_id.0 as usize);
        self.memories.push(memory);

        // Attach to node
        if let Some(node) = self.node_mut(node_id) {
            *node.memory_mut() = Some(memory_id);
        }

        memory_id
    }

    /// Populate a newly created alpha memory from the current working memory.
    ///
    /// This is used when rules are compiled after facts have already been
    /// asserted. It updates both the memory and the fact-to-memory reverse
    /// index without replaying facts through pre-existing alpha descendants.
    pub(crate) fn backfill_memory(
        &mut self,
        memory_id: AlphaMemoryId,
        entry_type: &AlphaEntryType,
        tests: &[ConstantTest],
        fact_base: &FactBase,
    ) {
        debug_assert!(
            self.memory(memory_id).is_some_and(AlphaMemory::is_empty),
            "only newly created alpha memories may be backfilled"
        );

        let mut matching_facts: Vec<(FactId, crate::fact::Timestamp)> = fact_base
            .iter()
            .filter(|(_, entry)| {
                fact_matches_entry_type(&entry.fact, entry_type)
                    && tests.iter().all(|test| evaluate_test(&entry.fact, test))
            })
            .map(|(fact_id, entry)| (fact_id, entry.timestamp))
            .collect();
        matching_facts.sort_unstable_by_key(|(_, timestamp)| *timestamp);

        for (fact_id, _) in matching_facts {
            let entry = fact_base
                .get(fact_id)
                .expect("backfill candidate must remain in the fact base");
            self.memory_mut(memory_id)
                .expect("new alpha memory must exist")
                .insert(fact_id, &entry.fact);

            if let Some(memories) = self.fact_to_memories.get_mut(fact_id) {
                if !memories.contains(&memory_id) {
                    memories.push(memory_id);
                }
            } else {
                self.fact_to_memories
                    .insert(fact_id, SmallVec::from_slice(&[memory_id]));
            }
        }
    }

    /// Get a reference to a memory.
    #[must_use]
    pub fn get_memory(&self, id: AlphaMemoryId) -> Option<&AlphaMemory> {
        self.memory(id)
    }

    /// Get a mutable reference to a memory.
    pub fn get_memory_mut(&mut self, id: AlphaMemoryId) -> Option<&mut AlphaMemory> {
        self.memory_mut(id)
    }

    /// Assert a fact into the alpha network.
    ///
    /// Propagates the fact through the network and returns all memories that accepted it.
    pub fn assert_fact(&mut self, fact_id: FactId, fact: &Fact) -> Vec<AlphaMemoryId> {
        ferric_span!(trace_span, "alpha_assert", fact_id = ?fact_id);
        let entry_type = match fact {
            Fact::Ordered(ordered) => AlphaEntryType::OrderedRelation(ordered.relation),
            Fact::Template(template) => AlphaEntryType::Template(template.template_id),
        };

        let Some(&entry_node) = self.entry_nodes.get(&entry_type) else {
            // No rules match this fact type
            return Vec::new();
        };

        let mut accepted_memories = Vec::new();
        self.propagate(entry_node, fact_id, fact, &mut accepted_memories);
        // Populate reverse index for O(1) retraction lookup
        if !accepted_memories.is_empty() {
            self.fact_to_memories
                .insert(fact_id, SmallVec::from_slice(&accepted_memories));
        }
        accepted_memories
    }

    /// Retract a fact from the alpha network.
    ///
    /// Removes the fact from all alpha memories.
    pub fn retract_fact(&mut self, fact_id: FactId, fact: &Fact) {
        ferric_span!(trace_span, "alpha_retract", fact_id = ?fact_id);
        // Assertion and online backfill record every accepting memory. Removing
        // only those memberships avoids walking unrelated constant-test branches.
        let Some(memories) = self.fact_to_memories.remove(fact_id) else {
            return;
        };
        for memory_id in memories {
            if let Some(memory) = self.memory_mut(memory_id) {
                memory.remove(fact_id, fact);
            }
        }
    }

    /// Get a reference to a node.
    #[must_use]
    pub fn get_node(&self, id: NodeId) -> Option<&AlphaNode> {
        self.node(id)
    }

    /// Return all alpha memory IDs that currently contain the given fact.
    ///
    /// Uses a reverse index populated during assertion for O(1) lookup
    /// rather than scanning all memories.
    pub fn memories_containing_fact(&self, fact_id: FactId) -> Vec<AlphaMemoryId> {
        self.fact_to_memories
            .get(fact_id)
            .map(|mems| mems.to_vec())
            .unwrap_or_default()
    }

    /// Clear all facts from all alpha memories, preserving network structure.
    pub fn clear_all_memories(&mut self) {
        self.pending_runtime.clear();
        for memory in &mut self.memories {
            memory.clear();
        }
        self.fact_to_memories.clear();
    }

    /// Verify internal consistency of the alpha network.
    ///
    /// Panics if any inconsistencies are detected, in any build profile.
    pub fn debug_assert_consistency(&self) {
        self.validate_consistency()
            .expect("inconsistent engine state");
    }

    /// Validate internal indexes without panicking.
    #[doc(hidden)]
    #[allow(clippy::too_many_lines)]
    pub fn validate_consistency(&self) -> Result<(), String> {
        // Check 1: All alpha memory IDs referenced by nodes exist in memories map
        for node in &self.nodes {
            if let Some(mem_id) = node.memory() {
                crate::snapshot::require!(
                    self.memory(mem_id).is_some(),
                    "Node references non-existent memory {mem_id:?}"
                );
            }
        }

        // Check 2: All node IDs in children fields exist in nodes map
        for (index, node) in self.nodes.iter().enumerate() {
            #[allow(clippy::cast_possible_truncation)]
            let node_id = NodeId(index as u32);
            for child_id in node.children() {
                crate::snapshot::require!(
                    self.node(*child_id).is_some(),
                    "Node {node_id:?} has non-existent child {child_id:?}"
                );
            }
        }

        // Check 3: No duplicate children in any node
        for (index, node) in self.nodes.iter().enumerate() {
            #[allow(clippy::cast_possible_truncation)]
            let node_id = NodeId(index as u32);
            let mut seen = HashSet::default();
            for child_id in node.children() {
                crate::snapshot::require!(
                    seen.insert(child_id),
                    "Node {node_id:?} has duplicate child {child_id:?}"
                );
            }
        }

        // Check 4: All facts in slot indices are also in main facts set of memory
        for memory in &self.memories {
            let mem_id = memory.id;
            for key_map in memory.slot_indices.values() {
                for fact_set in key_map.values() {
                    for fact_id in fact_set {
                        crate::snapshot::require!(
                            memory.facts.contains(fact_id),
                            "Memory {mem_id:?} has fact {fact_id:?} in index but not in main facts set"
                        );
                    }
                }
            }
        }
        Ok(())
    }

    /// Recursively propagate a fact through the network starting at a node.
    fn propagate(
        &mut self,
        node_id: NodeId,
        fact_id: FactId,
        fact: &Fact,
        accepted: &mut Vec<AlphaMemoryId>,
    ) {
        if matches!(self.node(node_id), Some(AlphaNode::RuntimePredicate { .. })) {
            self.pending_runtime.push((node_id, fact_id));
            return;
        }
        let Some((memory_id, children)) = self.propagation_plan(node_id, fact) else {
            return;
        };

        // If this node has a memory, insert the fact
        if let Some(mem_id) = memory_id {
            if let Some(memory) = self.memory_mut(mem_id) {
                memory.insert(fact_id, fact);
                accepted.push(mem_id);
            }
        }

        for child_id in children {
            self.propagate(child_id, fact_id, fact, accepted);
        }
    }

    pub(crate) fn propagation_plan(
        &self,
        node_id: NodeId,
        fact: &Fact,
    ) -> Option<(Option<AlphaMemoryId>, Vec<NodeId>)> {
        let node = self.node(node_id)?;
        if let AlphaNode::ConstantTest { test, .. } = node {
            evaluate_test(fact, test).then_some(())?;
        }
        Some((node.memory(), node.children().to_vec()))
    }

    fn node(&self, id: NodeId) -> Option<&AlphaNode> {
        self.nodes.get(id.0 as usize)
    }

    fn node_mut(&mut self, id: NodeId) -> Option<&mut AlphaNode> {
        self.nodes.get_mut(id.0 as usize)
    }

    fn memory(&self, id: AlphaMemoryId) -> Option<&AlphaMemory> {
        self.memories.get(id.0 as usize)
    }

    fn memory_mut(&mut self, id: AlphaMemoryId) -> Option<&mut AlphaMemory> {
        self.memories.get_mut(id.0 as usize)
    }
}

impl Default for AlphaNetwork {
    fn default() -> Self {
        Self::new()
    }
}

fn fact_matches_entry_type(fact: &Fact, entry_type: &AlphaEntryType) -> bool {
    matches!(
        (fact, entry_type),
        (Fact::Ordered(ordered), AlphaEntryType::OrderedRelation(relation))
            if ordered.relation == *relation
    ) || matches!(
        (fact, entry_type),
        (Fact::Template(template), AlphaEntryType::Template(template_id))
            if template.template_id == *template_id
    )
}

/// Evaluate a constant test against a fact.
fn evaluate_test(fact: &Fact, test: &ConstantTest) -> bool {
    match (&test.test_type, get_slot_value(fact, test.slot)) {
        (ConstantTestType::OrderedFieldCount { min, max }, _) => {
            matches!(fact, Fact::Ordered(ordered)
                if ordered.fields.len() >= *min
                    && max.map_or(true, |max| ordered.fields.len() <= max))
        }
        (_, None) => false,
        (ConstantTestType::Equal(test_key), Some(slot_value)) => {
            atom_key_matches(slot_value, |slot_key| slot_key == test_key)
        }
        (ConstantTestType::NotEqual(test_key), Some(slot_value)) => {
            atom_key_matches(slot_value, |slot_key| slot_key != test_key)
        }
        (ConstantTestType::EqualAny(keys), Some(slot_value)) => {
            atom_key_matches(slot_value, |slot_key| keys.contains(slot_key))
        }
        (ConstantTestType::GreaterThan(test_key), Some(slot_value)) => {
            compare_test_key(slot_value, test_key, |ord| matches!(ord, Ordering::Greater))
        }
        (ConstantTestType::LessThan(test_key), Some(slot_value)) => {
            compare_test_key(slot_value, test_key, |ord| matches!(ord, Ordering::Less))
        }
        (ConstantTestType::GreaterOrEqual(test_key), Some(slot_value)) => {
            compare_test_key(slot_value, test_key, |ord| {
                matches!(ord, Ordering::Greater | Ordering::Equal)
            })
        }
        (ConstantTestType::LessOrEqual(test_key), Some(slot_value)) => {
            compare_test_key(slot_value, test_key, |ord| {
                matches!(ord, Ordering::Less | Ordering::Equal)
            })
        }
        (ConstantTestType::EqualSlot(other_slot), Some(slot_value)) => {
            compare_other_slot(fact, *other_slot, |other_value| {
                slot_value.structural_eq(other_value)
            })
        }
        (ConstantTestType::NotEqualSlot(other_slot), Some(slot_value)) => {
            compare_other_slot(fact, *other_slot, |other_value| {
                !slot_value.structural_eq(other_value)
            })
        }
        (ConstantTestType::EqualSlotOffset(other_slot, offset), Some(slot_value)) => {
            compare_other_slot(fact, *other_slot, |other_value| {
                compare_offset(slot_value, other_value, *offset, |ord| {
                    matches!(ord, Ordering::Equal)
                })
            })
        }
        (ConstantTestType::NotEqualSlotOffset(other_slot, offset), Some(slot_value)) => {
            compare_other_slot(fact, *other_slot, |other_value| {
                !compare_offset(slot_value, other_value, *offset, |ord| {
                    matches!(ord, Ordering::Equal)
                })
            })
        }
        (ConstantTestType::GreaterThanSlotOffset(other_slot, offset), Some(slot_value)) => {
            compare_other_slot(fact, *other_slot, |other_value| {
                compare_offset(slot_value, other_value, *offset, |ord| {
                    matches!(ord, Ordering::Greater)
                })
            })
        }
        (ConstantTestType::LessThanSlotOffset(other_slot, offset), Some(slot_value)) => {
            compare_other_slot(fact, *other_slot, |other_value| {
                compare_offset(slot_value, other_value, *offset, |ord| {
                    matches!(ord, Ordering::Less)
                })
            })
        }
        (ConstantTestType::GreaterOrEqualSlotOffset(other_slot, offset), Some(slot_value)) => {
            compare_other_slot(fact, *other_slot, |other_value| {
                compare_offset(slot_value, other_value, *offset, |ord| {
                    matches!(ord, Ordering::Greater | Ordering::Equal)
                })
            })
        }
        (ConstantTestType::LessOrEqualSlotOffset(other_slot, offset), Some(slot_value)) => {
            compare_other_slot(fact, *other_slot, |other_value| {
                compare_offset(slot_value, other_value, *offset, |ord| {
                    matches!(ord, Ordering::Less | Ordering::Equal)
                })
            })
        }
    }
}

fn atom_key_matches<F>(value: &Value, predicate: F) -> bool
where
    F: FnOnce(&AtomKey) -> bool,
{
    let Some(slot_key) = AtomKey::from_value(value) else {
        return false;
    };
    predicate(&slot_key)
}

fn compare_test_key<F>(slot_value: &Value, test_key: &AtomKey, predicate: F) -> bool
where
    F: FnOnce(Ordering) -> bool,
{
    let rhs = test_key.to_value();
    numeric_compare_matches(slot_value, &rhs, predicate)
}

fn compare_other_slot<F>(fact: &Fact, other_slot: SlotIndex, predicate: F) -> bool
where
    F: FnOnce(&Value) -> bool,
{
    let Some(other_value) = get_slot_value(fact, other_slot) else {
        return false;
    };
    predicate(other_value)
}

fn compare_offset<F>(lhs: &Value, rhs: &Value, offset: i64, predicate: F) -> bool
where
    F: FnOnce(Ordering) -> bool,
{
    let Some(adjusted_rhs) = add_integer_offset(rhs, offset) else {
        return false;
    };
    numeric_compare_matches(lhs, &adjusted_rhs, predicate)
}

fn numeric_compare_matches<F>(lhs: &Value, rhs: &Value, predicate: F) -> bool
where
    F: FnOnce(Ordering) -> bool,
{
    compare_numeric_values(lhs, rhs).is_some_and(predicate)
}

#[allow(clippy::cast_precision_loss)]
fn add_integer_offset(value: &Value, offset: i64) -> Option<Value> {
    match value {
        Value::Integer(i) => i.checked_add(offset).map(Value::Integer),
        Value::Float(f) => Some(Value::Float(*f + offset as f64)),
        _ => None,
    }
}

#[allow(clippy::cast_precision_loss)]
fn compare_numeric_values(lhs: &Value, rhs: &Value) -> Option<Ordering> {
    match (lhs, rhs) {
        (Value::Integer(a), Value::Integer(b)) => Some(a.cmp(b)),
        (Value::Integer(a), Value::Float(b)) => (*a as f64).partial_cmp(b),
        (Value::Float(a), Value::Integer(b)) => a.partial_cmp(&(*b as f64)),
        (Value::Float(a), Value::Float(b)) => a.partial_cmp(b),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fact::OrderedFact;
    use crate::string::FerricString;
    use crate::symbol::SymbolTable;
    use crate::StringEncoding;
    use slotmap::SlotMap;
    use smallvec::smallvec;

    // --- AlphaMemory tests ---

    #[test]
    fn alpha_memory_new_is_empty() {
        let mem = AlphaMemory::new(AlphaMemoryId(0));
        assert!(mem.is_empty());
        assert_eq!(mem.len(), 0);
    }

    #[test]
    fn alpha_memory_insert_and_contains() {
        let mut mem = AlphaMemory::new(AlphaMemoryId(0));
        let mut fact_base = FactBase::new();
        let mut table = SymbolTable::new();

        let rel = table.intern_symbol("test", StringEncoding::Ascii).unwrap();
        let fact_id = fact_base.assert_ordered(rel, smallvec![Value::Integer(42)]);
        let fact = fact_base.get(fact_id).unwrap();

        mem.insert(fact_id, &fact.fact);

        assert_eq!(mem.len(), 1);
        assert!(mem.contains(fact_id));
    }

    #[test]
    fn alpha_memory_remove() {
        let mut mem = AlphaMemory::new(AlphaMemoryId(0));
        let mut fact_base = FactBase::new();
        let mut table = SymbolTable::new();

        let rel = table.intern_symbol("test", StringEncoding::Ascii).unwrap();
        let fact_id = fact_base.assert_ordered(rel, smallvec![Value::Integer(42)]);
        let fact = fact_base.get(fact_id).unwrap();

        mem.insert(fact_id, &fact.fact);
        assert_eq!(mem.len(), 1);

        mem.remove(fact_id, &fact.fact);
        assert_eq!(mem.len(), 0);
        assert!(!mem.contains(fact_id));
    }

    #[test]
    fn alpha_memory_remove_prunes_empty_indices() {
        let mut mem = AlphaMemory::new(AlphaMemoryId(0));
        let mut fact_base = FactBase::new();
        let mut table = SymbolTable::new();

        let rel = table.intern_symbol("test", StringEncoding::Ascii).unwrap();
        let fact_id = fact_base.assert_ordered(rel, smallvec![Value::Integer(42)]);
        let fact = fact_base.get(fact_id).unwrap();

        // Request index on slot 0
        mem.request_index(SlotIndex::Ordered(0), &fact_base);
        mem.insert(fact_id, &fact.fact);

        // Verify index exists
        assert!(mem.slot_indices.contains_key(&SlotIndex::Ordered(0)));

        // Remove fact
        mem.remove(fact_id, &fact.fact);

        // Index should be pruned
        assert!(!mem.slot_indices.contains_key(&SlotIndex::Ordered(0)));
    }

    #[test]
    fn alpha_memory_request_index_backfills_existing() {
        let mut mem = AlphaMemory::new(AlphaMemoryId(0));
        let mut fact_base = FactBase::new();
        let mut table = SymbolTable::new();

        let rel = table.intern_symbol("test", StringEncoding::Ascii).unwrap();
        let fact_id1 = fact_base.assert_ordered(rel, smallvec![Value::Integer(42)]);
        let fact_id2 = fact_base.assert_ordered(rel, smallvec![Value::Integer(99)]);

        let fact1 = fact_base.get(fact_id1).unwrap();
        let fact2 = fact_base.get(fact_id2).unwrap();

        // Insert facts before indexing
        mem.insert(fact_id1, &fact1.fact);
        mem.insert(fact_id2, &fact2.fact);

        // Request index - should backfill
        mem.request_index(SlotIndex::Ordered(0), &fact_base);

        // Both facts should be indexed
        let key_42 = AtomKey::Integer(42);
        let key_99 = AtomKey::Integer(99);

        assert!(mem.lookup_by_slot(SlotIndex::Ordered(0), &key_42).is_some());
        assert!(mem.lookup_by_slot(SlotIndex::Ordered(0), &key_99).is_some());
    }

    #[test]
    fn alpha_memory_lookup_by_slot() {
        let mut mem = AlphaMemory::new(AlphaMemoryId(0));
        let mut fact_base = FactBase::new();
        let mut table = SymbolTable::new();

        let rel = table.intern_symbol("test", StringEncoding::Ascii).unwrap();

        // Request index first
        mem.request_index(SlotIndex::Ordered(0), &fact_base);

        let fact_id1 = fact_base.assert_ordered(rel, smallvec![Value::Integer(42)]);
        let fact_id2 = fact_base.assert_ordered(rel, smallvec![Value::Integer(42)]);
        let fact_id3 = fact_base.assert_ordered(rel, smallvec![Value::Integer(99)]);

        let fact1 = fact_base.get(fact_id1).unwrap();
        let fact2 = fact_base.get(fact_id2).unwrap();
        let fact3 = fact_base.get(fact_id3).unwrap();

        mem.insert(fact_id1, &fact1.fact);
        mem.insert(fact_id2, &fact2.fact);
        mem.insert(fact_id3, &fact3.fact);

        let key_42 = AtomKey::Integer(42);
        let matching_facts: Vec<_> = mem
            .lookup_by_slot(SlotIndex::Ordered(0), &key_42)
            .unwrap()
            .collect();

        assert_eq!(matching_facts.len(), 2);
        assert!(matching_facts.contains(&fact_id1));
        assert!(matching_facts.contains(&fact_id2));
    }

    #[test]
    fn alpha_memory_lookup_nonexistent_slot_returns_none() {
        let mem = AlphaMemory::new(AlphaMemoryId(0));
        let key = AtomKey::Integer(42);

        assert!(mem.lookup_by_slot(SlotIndex::Ordered(0), &key).is_none());
    }

    // --- AlphaNetwork tests ---

    #[test]
    fn alpha_network_create_entry_node() {
        let mut network = AlphaNetwork::new();
        let mut table = SymbolTable::new();
        let rel = table.intern_symbol("test", StringEncoding::Ascii).unwrap();

        let entry_type = AlphaEntryType::OrderedRelation(rel);
        let node_id = network.create_entry_node(entry_type.clone());

        assert!(network.get_node(node_id).is_some());
        assert_eq!(network.entry_nodes.get(&entry_type), Some(&node_id));
    }

    #[test]
    fn alpha_network_create_entry_node_is_idempotent() {
        let mut network = AlphaNetwork::new();
        let mut table = SymbolTable::new();
        let rel = table.intern_symbol("test", StringEncoding::Ascii).unwrap();

        let entry_type = AlphaEntryType::OrderedRelation(rel);
        let node_id1 = network.create_entry_node(entry_type.clone());
        let node_id2 = network.create_entry_node(entry_type);

        assert_eq!(node_id1, node_id2);
    }

    #[test]
    fn alpha_network_create_constant_test() {
        let mut network = AlphaNetwork::new();
        let mut table = SymbolTable::new();
        let rel = table.intern_symbol("test", StringEncoding::Ascii).unwrap();

        let entry_type = AlphaEntryType::OrderedRelation(rel);
        let entry_node = network.create_entry_node(entry_type);

        let test = ConstantTest {
            slot: SlotIndex::Ordered(0),
            test_type: ConstantTestType::Equal(AtomKey::Integer(42)),
        };

        let test_node = network.create_constant_test_node(entry_node, test);

        assert!(network.get_node(test_node).is_some());

        // Verify it's a child of the entry node
        if let Some(AlphaNode::Entry { children, .. }) = network.get_node(entry_node) {
            assert_eq!(children.len(), 1);
            assert_eq!(children[0], test_node);
        } else {
            panic!("Expected Entry node");
        }
    }

    #[test]
    fn alpha_network_assert_fact_reaches_memory() {
        let mut network = AlphaNetwork::new();
        let mut fact_base = FactBase::new();
        let mut table = SymbolTable::new();

        let rel = table
            .intern_symbol("person", StringEncoding::Ascii)
            .unwrap();
        let entry_type = AlphaEntryType::OrderedRelation(rel);

        let entry_node = network.create_entry_node(entry_type);
        let mem_id = network.create_memory(entry_node);

        let fact_id = fact_base.assert_ordered(rel, smallvec![Value::Integer(42)]);
        let fact = fact_base.get(fact_id).unwrap();

        let accepted = network.assert_fact(fact_id, &fact.fact);

        assert_eq!(accepted.len(), 1);
        assert_eq!(accepted[0], mem_id);

        let memory = network.get_memory(mem_id).unwrap();
        assert!(memory.contains(fact_id));
    }

    #[test]
    fn alpha_network_assert_fact_constant_test_filters() {
        let mut network = AlphaNetwork::new();
        let mut fact_base = FactBase::new();
        let mut table = SymbolTable::new();

        let rel = table
            .intern_symbol("person", StringEncoding::Ascii)
            .unwrap();
        let entry_type = AlphaEntryType::OrderedRelation(rel);

        let entry_node = network.create_entry_node(entry_type);

        let test = ConstantTest {
            slot: SlotIndex::Ordered(0),
            test_type: ConstantTestType::Equal(AtomKey::Integer(42)),
        };

        let test_node = network.create_constant_test_node(entry_node, test);
        let mem_id = network.create_memory(test_node);

        // Fact that matches the test
        let fact_id1 = fact_base.assert_ordered(rel, smallvec![Value::Integer(42)]);
        let fact1 = fact_base.get(fact_id1).unwrap();

        let accepted1 = network.assert_fact(fact_id1, &fact1.fact);
        assert_eq!(accepted1.len(), 1);
        assert_eq!(accepted1[0], mem_id);

        // Fact that doesn't match the test
        let fact_id2 = fact_base.assert_ordered(rel, smallvec![Value::Integer(99)]);
        let fact2 = fact_base.get(fact_id2).unwrap();

        let accepted2 = network.assert_fact(fact_id2, &fact2.fact);
        assert!(accepted2.is_empty());

        let memory = network.get_memory(mem_id).unwrap();
        assert!(memory.contains(fact_id1));
        assert!(!memory.contains(fact_id2));
    }

    #[test]
    fn alpha_network_assert_fact_no_matching_entry_returns_empty() {
        let mut network = AlphaNetwork::new();
        let mut fact_base = FactBase::new();
        let mut table = SymbolTable::new();

        let rel = table
            .intern_symbol("person", StringEncoding::Ascii)
            .unwrap();
        let fact_id = fact_base.assert_ordered(rel, smallvec![Value::Integer(42)]);
        let fact = fact_base.get(fact_id).unwrap();

        // No entry node created, so no memories should accept the fact
        let accepted = network.assert_fact(fact_id, &fact.fact);
        assert!(accepted.is_empty());
    }

    #[test]
    fn alpha_network_retract_fact_removes_from_memories() {
        let mut network = AlphaNetwork::new();
        let mut fact_base = FactBase::new();
        let mut table = SymbolTable::new();

        let rel = table
            .intern_symbol("person", StringEncoding::Ascii)
            .unwrap();
        let entry_type = AlphaEntryType::OrderedRelation(rel);

        let entry_node = network.create_entry_node(entry_type);
        let mem_id = network.create_memory(entry_node);

        let fact_id = fact_base.assert_ordered(rel, smallvec![Value::Integer(42)]);
        let fact = fact_base.get(fact_id).unwrap();

        network.assert_fact(fact_id, &fact.fact);

        let memory = network.get_memory(mem_id).unwrap();
        assert!(memory.contains(fact_id));

        network.retract_fact(fact_id, &fact.fact);

        let memory = network.get_memory(mem_id).unwrap();
        assert!(!memory.contains(fact_id));
    }

    #[test]
    fn alpha_network_multiple_memories_with_different_tests() {
        let mut network = AlphaNetwork::new();
        let mut fact_base = FactBase::new();
        let mut table = SymbolTable::new();

        let rel = table
            .intern_symbol("person", StringEncoding::Ascii)
            .unwrap();
        let entry_type = AlphaEntryType::OrderedRelation(rel);

        let entry_node = network.create_entry_node(entry_type);

        // First test: slot 0 == 42
        let test1 = ConstantTest {
            slot: SlotIndex::Ordered(0),
            test_type: ConstantTestType::Equal(AtomKey::Integer(42)),
        };
        let test_node1 = network.create_constant_test_node(entry_node, test1);
        let mem_id1 = network.create_memory(test_node1);

        // Second test: slot 0 == 99
        let test2 = ConstantTest {
            slot: SlotIndex::Ordered(0),
            test_type: ConstantTestType::Equal(AtomKey::Integer(99)),
        };
        let test_node2 = network.create_constant_test_node(entry_node, test2);
        let mem_id2 = network.create_memory(test_node2);

        // Assert fact with slot 0 = 42
        let fact_id = fact_base.assert_ordered(rel, smallvec![Value::Integer(42)]);
        let fact = fact_base.get(fact_id).unwrap();

        let accepted = network.assert_fact(fact_id, &fact.fact);

        // Should only reach mem1, not mem2
        assert_eq!(accepted.len(), 1);
        assert_eq!(accepted[0], mem_id1);

        let mem1 = network.get_memory(mem_id1).unwrap();
        let mem2 = network.get_memory(mem_id2).unwrap();

        assert!(mem1.contains(fact_id));
        assert!(!mem2.contains(fact_id));
    }

    // --- Test helpers for get_slot_value ---

    #[test]
    fn get_slot_value_ordered_fact() {
        let mut table = SymbolTable::new();
        let rel = table.intern_symbol("test", StringEncoding::Ascii).unwrap();
        let ordered = OrderedFact {
            relation: rel,
            fields: smallvec![Value::Integer(42), Value::Integer(99)],
        };
        let fact = Fact::Ordered(ordered);

        assert!(matches!(
            get_slot_value(&fact, SlotIndex::Ordered(0)),
            Some(Value::Integer(42))
        ));
        assert!(matches!(
            get_slot_value(&fact, SlotIndex::Ordered(1)),
            Some(Value::Integer(99))
        ));
        assert!(get_slot_value(&fact, SlotIndex::Ordered(2)).is_none());
    }

    #[test]
    fn get_slot_value_template_fact() {
        let mut temp: SlotMap<TemplateId, ()> = SlotMap::with_key();
        let template_id = temp.insert(());

        let template = crate::fact::TemplateFact {
            template_id,
            slots: Box::new([Value::Integer(42), Value::Integer(99)]),
        };
        let fact = Fact::Template(template);

        assert!(matches!(
            get_slot_value(&fact, SlotIndex::Template(0)),
            Some(Value::Integer(42))
        ));
        assert!(matches!(
            get_slot_value(&fact, SlotIndex::Template(1)),
            Some(Value::Integer(99))
        ));
        assert!(get_slot_value(&fact, SlotIndex::Template(2)).is_none());
    }

    #[test]
    fn get_slot_value_mismatched_slot_type_returns_none() {
        let mut table = SymbolTable::new();
        let rel = table.intern_symbol("test", StringEncoding::Ascii).unwrap();
        let ordered = OrderedFact {
            relation: rel,
            fields: smallvec![Value::Integer(42)],
        };
        let fact = Fact::Ordered(ordered);

        // Try to access with Template slot index
        assert!(get_slot_value(&fact, SlotIndex::Template(0)).is_none());
    }

    // --- Test constant test evaluation ---

    #[test]
    fn ordered_field_count_tests_enforce_exact_and_minimum_cardinality() {
        let mut table = SymbolTable::new();
        let relation = table.intern_symbol("test", StringEncoding::Ascii).unwrap();
        for field_count in 0..=3 {
            let fact = Fact::Ordered(OrderedFact {
                relation,
                fields: (0..field_count).map(|_| Value::Integer(42)).collect(),
            });
            for min in 0..=3 {
                for max in [Some(min), None] {
                    let test = ConstantTest {
                        slot: SlotIndex::Ordered(0),
                        test_type: ConstantTestType::OrderedFieldCount { min, max },
                    };
                    let expected = if max.is_some() {
                        field_count == min
                    } else {
                        field_count >= min
                    };
                    assert_eq!(
                        evaluate_test(&fact, &test),
                        expected,
                        "field count {field_count}, bounds {min}..={max:?}"
                    );
                }
            }
        }
    }

    #[test]
    fn ordered_field_count_tests_reject_template_facts() {
        let mut templates: SlotMap<TemplateId, ()> = SlotMap::with_key();
        let fact = Fact::Template(crate::fact::TemplateFact {
            template_id: templates.insert(()),
            slots: Box::new([Value::Integer(42)]),
        });
        let test = ConstantTest {
            slot: SlotIndex::Template(0),
            test_type: ConstantTestType::OrderedFieldCount { min: 1, max: None },
        };
        assert!(!evaluate_test(&fact, &test));
    }

    #[test]
    fn constant_test_equal_passes() {
        let mut table = SymbolTable::new();
        let rel = table.intern_symbol("test", StringEncoding::Ascii).unwrap();
        let ordered = OrderedFact {
            relation: rel,
            fields: smallvec![Value::Integer(42)],
        };
        let fact = Fact::Ordered(ordered);

        let test = ConstantTest {
            slot: SlotIndex::Ordered(0),
            test_type: ConstantTestType::Equal(AtomKey::Integer(42)),
        };

        assert!(evaluate_test(&fact, &test));
    }

    #[test]
    fn constant_test_equal_fails() {
        let mut table = SymbolTable::new();
        let rel = table.intern_symbol("test", StringEncoding::Ascii).unwrap();
        let ordered = OrderedFact {
            relation: rel,
            fields: smallvec![Value::Integer(99)],
        };
        let fact = Fact::Ordered(ordered);

        let test = ConstantTest {
            slot: SlotIndex::Ordered(0),
            test_type: ConstantTestType::Equal(AtomKey::Integer(42)),
        };

        assert!(!evaluate_test(&fact, &test));
    }

    #[test]
    fn constant_test_not_equal_passes() {
        let mut table = SymbolTable::new();
        let rel = table.intern_symbol("test", StringEncoding::Ascii).unwrap();
        let ordered = OrderedFact {
            relation: rel,
            fields: smallvec![Value::Integer(99)],
        };
        let fact = Fact::Ordered(ordered);

        let test = ConstantTest {
            slot: SlotIndex::Ordered(0),
            test_type: ConstantTestType::NotEqual(AtomKey::Integer(42)),
        };

        assert!(evaluate_test(&fact, &test));
    }

    #[test]
    fn constant_test_not_equal_fails() {
        let mut table = SymbolTable::new();
        let rel = table.intern_symbol("test", StringEncoding::Ascii).unwrap();
        let ordered = OrderedFact {
            relation: rel,
            fields: smallvec![Value::Integer(42)],
        };
        let fact = Fact::Ordered(ordered);

        let test = ConstantTest {
            slot: SlotIndex::Ordered(0),
            test_type: ConstantTestType::NotEqual(AtomKey::Integer(42)),
        };

        assert!(!evaluate_test(&fact, &test));
    }

    #[test]
    fn constant_test_on_void_fails() {
        let mut table = SymbolTable::new();
        let rel = table.intern_symbol("test", StringEncoding::Ascii).unwrap();
        let ordered = OrderedFact {
            relation: rel,
            fields: smallvec![Value::Void],
        };
        let fact = Fact::Ordered(ordered);

        let test = ConstantTest {
            slot: SlotIndex::Ordered(0),
            test_type: ConstantTestType::Equal(AtomKey::Integer(42)),
        };

        // Void can't be converted to AtomKey, so test should fail
        assert!(!evaluate_test(&fact, &test));
    }

    #[test]
    fn constant_test_on_string() {
        let mut table = SymbolTable::new();
        let rel = table.intern_symbol("test", StringEncoding::Ascii).unwrap();
        let s = FerricString::new("hello", StringEncoding::Ascii).unwrap();
        let ordered = OrderedFact {
            relation: rel,
            fields: smallvec![Value::String(s.clone())],
        };
        let fact = Fact::Ordered(ordered);

        let test = ConstantTest {
            slot: SlotIndex::Ordered(0),
            test_type: ConstantTestType::Equal(AtomKey::String(s)),
        };

        assert!(evaluate_test(&fact, &test));
    }

    #[test]
    fn constant_test_equal_slot_passes() {
        let mut table = SymbolTable::new();
        let rel = table.intern_symbol("pair", StringEncoding::Ascii).unwrap();
        let ordered = OrderedFact {
            relation: rel,
            fields: smallvec![Value::Integer(7), Value::Integer(7)],
        };
        let fact = Fact::Ordered(ordered);

        let test = ConstantTest {
            slot: SlotIndex::Ordered(1),
            test_type: ConstantTestType::EqualSlot(SlotIndex::Ordered(0)),
        };

        assert!(evaluate_test(&fact, &test));
    }

    #[test]
    fn constant_test_not_equal_slot_passes() {
        let mut table = SymbolTable::new();
        let rel = table.intern_symbol("pair", StringEncoding::Ascii).unwrap();
        let ordered = OrderedFact {
            relation: rel,
            fields: smallvec![Value::Integer(7), Value::Integer(8)],
        };
        let fact = Fact::Ordered(ordered);

        let test = ConstantTest {
            slot: SlotIndex::Ordered(1),
            test_type: ConstantTestType::NotEqualSlot(SlotIndex::Ordered(0)),
        };

        assert!(evaluate_test(&fact, &test));
    }

    #[test]
    fn constant_test_equal_slot_offset_passes() {
        let mut table = SymbolTable::new();
        let rel = table.intern_symbol("pair", StringEncoding::Ascii).unwrap();
        let ordered = OrderedFact {
            relation: rel,
            fields: smallvec![Value::Integer(7), Value::Integer(8)],
        };
        let fact = Fact::Ordered(ordered);

        let test = ConstantTest {
            slot: SlotIndex::Ordered(1),
            test_type: ConstantTestType::EqualSlotOffset(SlotIndex::Ordered(0), 1),
        };

        assert!(evaluate_test(&fact, &test));
    }

    #[test]
    fn constant_test_greater_than_slot_offset_passes() {
        let mut table = SymbolTable::new();
        let rel = table.intern_symbol("pair", StringEncoding::Ascii).unwrap();
        let ordered = OrderedFact {
            relation: rel,
            fields: smallvec![Value::Integer(10), Value::Integer(8)],
        };
        let fact = Fact::Ordered(ordered);

        let test = ConstantTest {
            slot: SlotIndex::Ordered(0),
            test_type: ConstantTestType::GreaterThanSlotOffset(SlotIndex::Ordered(1), 1),
        };

        assert!(evaluate_test(&fact, &test));
    }

    #[test]
    fn constant_test_greater_than_numeric_passes() {
        let mut table = SymbolTable::new();
        let rel = table.intern_symbol("test", StringEncoding::Ascii).unwrap();
        let ordered = OrderedFact {
            relation: rel,
            fields: smallvec![Value::Integer(7)],
        };
        let fact = Fact::Ordered(ordered);

        let test = ConstantTest {
            slot: SlotIndex::Ordered(0),
            test_type: ConstantTestType::GreaterThan(AtomKey::Integer(5)),
        };

        assert!(evaluate_test(&fact, &test));
    }

    #[test]
    fn constant_test_less_or_equal_numeric_fails_on_non_numeric() {
        let mut table = SymbolTable::new();
        let rel = table.intern_symbol("test", StringEncoding::Ascii).unwrap();
        let sym = table.intern_symbol("alpha", StringEncoding::Ascii).unwrap();
        let ordered = OrderedFact {
            relation: rel,
            fields: smallvec![Value::Symbol(sym)],
        };
        let fact = Fact::Ordered(ordered);

        let test = ConstantTest {
            slot: SlotIndex::Ordered(0),
            test_type: ConstantTestType::LessOrEqual(AtomKey::Integer(5)),
        };

        assert!(!evaluate_test(&fact, &test));
    }
}

#[cfg(test)]
mod proptests {
    use super::*;
    use crate::symbol::SymbolTable;
    use crate::StringEncoding;
    use proptest::prelude::*;
    use smallvec::smallvec;

    proptest! {
        #[test]
        fn insert_and_remove_n_facts_leaves_memory_empty(count in 1..50_usize) {
            let mut mem = AlphaMemory::new(AlphaMemoryId(0));
            let mut fact_base = FactBase::new();
            let mut table = SymbolTable::new();

            let rel = table.intern_symbol("test", StringEncoding::Ascii).unwrap();
            let mut fact_ids = Vec::new();

            // Insert N facts
            #[allow(clippy::cast_possible_wrap)] // count is always < 50 in this test
            for i in 0..count {
                let fact_id = fact_base.assert_ordered(rel, smallvec![Value::Integer(i as i64)]);
                let fact = fact_base.get(fact_id).unwrap();
                mem.insert(fact_id, &fact.fact);
                fact_ids.push(fact_id);
            }

            prop_assert_eq!(mem.len(), count);

            // Remove all facts
            for fact_id in fact_ids {
                let fact = fact_base.get(fact_id).unwrap();
                mem.remove(fact_id, &fact.fact);
            }

            prop_assert!(mem.is_empty());
        }

        #[test]
        fn assert_fact_always_reaches_matching_entry_memory(values in prop::collection::vec(any::<i64>(), 1..20)) {
            let mut network = AlphaNetwork::new();
            let mut fact_base = FactBase::new();
            let mut table = SymbolTable::new();

            let rel = table.intern_symbol("test", StringEncoding::Ascii).unwrap();
            let entry_type = AlphaEntryType::OrderedRelation(rel);

            let entry_node = network.create_entry_node(entry_type);
            let mem_id = network.create_memory(entry_node);

            for value in values {
                let fact_id = fact_base.assert_ordered(rel, smallvec![Value::Integer(value)]);
                let fact = fact_base.get(fact_id).unwrap();

                let accepted = network.assert_fact(fact_id, &fact.fact);

                // Should always reach the entry memory
                prop_assert_eq!(accepted.len(), 1);
                prop_assert_eq!(accepted[0], mem_id);

                let memory = network.get_memory(mem_id).unwrap();
                prop_assert!(memory.contains(fact_id));
            }
        }
    }
}
