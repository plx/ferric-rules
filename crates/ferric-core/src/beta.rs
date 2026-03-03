//! Beta network and beta memory.
//!
//! The beta network is the second stage of the Rete algorithm. It performs
//! joins between alpha memories (facts) and beta memories (partial matches/tokens).

use rustc_hash::{FxHashMap as HashMap, FxHashSet as HashSet};
use smallvec::SmallVec;

use crate::alpha::{AlphaMemoryId, SlotIndex};
use crate::binding::{BindingSet, VarId};
use crate::exists::{ExistsMemory, ExistsMemoryId};
use crate::ncc::{NccMemory, NccMemoryId};
use crate::negative::{NegativeMemory, NegativeMemoryId};
use crate::token::NodeId;
use crate::token::TokenId;
use crate::value::AtomKey;

type FanoutNodes = SmallVec<[NodeId; 4]>;

/// Rule priority in CLIPS.
///
/// Higher salience values indicate higher priority. The default salience is 0.
/// This newtype prevents accidental mixing with other `i32` values.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Salience(i32);

impl Salience {
    pub const DEFAULT: Self = Self(0);

    #[must_use]
    pub const fn new(val: i32) -> Self {
        Self(val)
    }

    #[must_use]
    pub const fn get(self) -> i32 {
        self.0
    }
}

/// A join test compares a variable binding from the left (token) with
/// a slot value from the right (fact).
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct JoinTest {
    pub alpha_slot: SlotIndex,
    pub beta_var: VarId,
    pub test_type: JoinTestType,
}

/// The type of join test to perform.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum JoinTestType {
    Equal,
    NotEqual,
    GreaterThan,
    LessThan,
    GreaterOrEqual,
    LessOrEqual,
    /// Lexeme equality (currently string-only) comparison.
    LexEqual,
    /// Lexeme inequality (currently string-only) comparison.
    LexNotEqual,
    /// Lexeme greater-than (currently string-only) comparison.
    LexGreaterThan,
    /// Lexeme less-than (currently string-only) comparison.
    LexLessThan,
    /// Lexeme greater-or-equal (currently string-only) comparison.
    LexGreaterOrEqual,
    /// Lexeme less-or-equal (currently string-only) comparison.
    LexLessOrEqual,
    /// Compare against the left binding plus an integer offset.
    EqualOffset(i64),
    /// Compare against the left binding plus an integer offset.
    NotEqualOffset(i64),
    /// Compare against the left binding plus an integer offset.
    GreaterThanOffset(i64),
    /// Compare against the left binding plus an integer offset.
    LessThanOffset(i64),
    /// Compare against the left binding plus an integer offset.
    GreaterOrEqualOffset(i64),
    /// Compare against the left binding plus an integer offset.
    LessOrEqualOffset(i64),
}

/// Unique identifier for a beta memory.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct BetaMemoryId(pub u32);

/// Beta memory: stores tokens (partial matches).
///
/// Each beta memory is associated with a join node or other beta node
/// and holds the tokens that have successfully matched up to that point.
///
/// Optionally maintains variable-binding indices for O(1) right-activation
/// lookups, mirroring the alpha memory's slot-based indexing.
pub struct BetaMemory {
    pub id: BetaMemoryId,
    tokens: HashSet<TokenId>,
    /// Variable indices: `VarId` → `AtomKey` → set of `TokenId`s with that binding value.
    /// Enables O(1) lookup during right activation instead of full parent-token scans.
    var_indices: HashMap<VarId, HashMap<AtomKey, SmallVec<[TokenId; 4]>>>,
    /// Which variables are currently indexed. Survives `clear()` (like alpha memory's
    /// `indexed_slots`), since the index configuration is a compile-time decision.
    indexed_vars: SmallVec<[VarId; 4]>,
}

impl BetaMemory {
    /// Create a new, empty beta memory.
    #[must_use]
    pub fn new(id: BetaMemoryId) -> Self {
        Self {
            id,
            tokens: HashSet::default(),
            var_indices: HashMap::default(),
            indexed_vars: SmallVec::new(),
        }
    }

    /// Request indexing on a particular variable binding.
    ///
    /// Called during compilation when a child join node has an equality test on
    /// `var_id`. Idempotent — duplicate requests are ignored. No backfill is needed
    /// because beta memories are always empty at compile time.
    pub fn request_var_index(&mut self, var_id: VarId) {
        if !self.indexed_vars.contains(&var_id) {
            self.indexed_vars.push(var_id);
        }
    }

    /// Returns `true` if the given variable has been requested for indexing.
    #[must_use]
    pub fn is_var_indexed(&self, var_id: VarId) -> bool {
        self.indexed_vars.contains(&var_id)
    }

    /// Lookup tokens by variable binding value.
    ///
    /// Returns the set of tokens whose binding for `var_id` matches `key`,
    /// or `None` if no tokens match (or the variable is not indexed).
    #[must_use]
    pub fn lookup_by_var(&self, var_id: VarId, key: &AtomKey) -> Option<&SmallVec<[TokenId; 4]>> {
        self.var_indices.get(&var_id)?.get(key)
    }

    /// Insert a token into the memory.
    ///
    /// If the token is already present, this is a no-op.
    pub fn insert(&mut self, token_id: TokenId) {
        self.tokens.insert(token_id);
    }

    /// Insert a token with its bindings, updating variable indices.
    ///
    /// If the memory has indexed variables, extracts the corresponding binding
    /// values and adds the token to the appropriate index entries.
    pub fn insert_indexed(&mut self, token_id: TokenId, bindings: &BindingSet) {
        self.tokens.insert(token_id);
        for &var_id in &self.indexed_vars {
            if let Some(value) = bindings.get(var_id) {
                if let Some(key) = AtomKey::from_value(value) {
                    self.var_indices
                        .entry(var_id)
                        .or_default()
                        .entry(key)
                        .or_default()
                        .push(token_id);
                }
            }
        }
    }

    /// Remove a token from the memory.
    ///
    /// If the token is not present, this is a no-op.
    pub fn remove(&mut self, token_id: TokenId) {
        self.tokens.remove(&token_id);
    }

    /// Remove a token with its bindings, updating variable indices.
    ///
    /// Mirrors `insert_indexed`: removes the token from all variable index entries
    /// that correspond to its binding values.
    pub fn remove_indexed(&mut self, token_id: TokenId, bindings: &BindingSet) {
        self.tokens.remove(&token_id);
        for &var_id in &self.indexed_vars {
            if let Some(value) = bindings.get(var_id) {
                if let Some(key) = AtomKey::from_value(value) {
                    if let Some(key_map) = self.var_indices.get_mut(&var_id) {
                        if let Some(token_vec) = key_map.get_mut(&key) {
                            token_vec.retain(|tid| *tid != token_id);
                            if token_vec.is_empty() {
                                key_map.remove(&key);
                            }
                        }
                    }
                }
            }
        }
    }

    /// Check if the memory contains a specific token.
    #[must_use]
    pub fn contains(&self, token_id: TokenId) -> bool {
        self.tokens.contains(&token_id)
    }

    /// Iterate over all tokens in the memory.
    pub fn iter(&self) -> impl Iterator<Item = TokenId> + '_ {
        self.tokens.iter().copied()
    }

    /// Check if the memory is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.tokens.is_empty()
    }

    /// Return the number of tokens in the memory.
    #[must_use]
    pub fn len(&self) -> usize {
        self.tokens.len()
    }

    /// Clear all tokens and index entries from this memory.
    ///
    /// Preserves `indexed_vars` (the index configuration), since that is a
    /// compile-time decision. Only runtime data is cleared.
    pub fn clear(&mut self) {
        self.tokens.clear();
        self.var_indices.clear();
    }
}

/// Simple identifier for rules.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct RuleId(pub u32);

/// A node in the beta network.
///
/// Phase 1 includes only root, join, and terminal nodes.
#[derive(Clone, Debug)]
pub enum BetaNode {
    /// Root node: entry point for all matches.
    Root { children: Vec<NodeId> },
    /// Join node: combines left (parent beta memory) with right (alpha memory).
    Join {
        parent: NodeId,
        alpha_memory: AlphaMemoryId,
        tests: Vec<JoinTest>,
        bindings: Vec<(SlotIndex, VarId)>,
        memory: BetaMemoryId,
        children: Vec<NodeId>,
    },
    /// Terminal node: produces activations for a rule.
    Terminal {
        parent: NodeId,
        rule: RuleId,
        salience: Salience,
    },
    /// Negative node: blocks parent tokens when a matching fact exists.
    Negative {
        parent: NodeId,
        alpha_memory: AlphaMemoryId,
        tests: Vec<JoinTest>,
        memory: BetaMemoryId,
        neg_memory: NegativeMemoryId,
        children: Vec<NodeId>,
    },
    /// NCC node: blocks parent tokens when a subnetwork conjunction has results.
    Ncc {
        parent: NodeId,
        /// The NCC partner node ID (at bottom of subnetwork).
        partner: NodeId,
        memory: BetaMemoryId,
        ncc_memory: NccMemoryId,
        children: Vec<NodeId>,
    },
    /// NCC partner node: sits at bottom of subnetwork, reports results to NCC node.
    NccPartner {
        parent: NodeId,
        /// The NCC node this partner reports to.
        ncc_node: NodeId,
        ncc_memory: NccMemoryId,
    },
    /// Exists node: propagates when at least one supporting fact exists.
    Exists {
        parent: NodeId,
        alpha_memory: AlphaMemoryId,
        tests: Vec<JoinTest>,
        memory: BetaMemoryId,
        exists_memory: ExistsMemoryId,
        children: Vec<NodeId>,
    },
}

/// The beta network.
///
/// Manages beta nodes and beta memories. Coordinates with the alpha network
/// to perform joins and produce activations.
pub struct BetaNetwork {
    nodes: HashMap<NodeId, BetaNode>,
    memories: Vec<BetaMemory>,
    neg_memories: Vec<NegativeMemory>,
    ncc_memories: Vec<NccMemory>,
    exists_memories: Vec<ExistsMemory>,
    root_id: NodeId,
    next_node_id: u32,
    next_memory_id: u32,
    next_neg_memory_id: u32,
    next_ncc_memory_id: u32,
    next_exists_memory_id: u32,
    /// Reverse index: alpha memory -> list of join nodes that subscribe to it.
    alpha_to_joins: HashMap<AlphaMemoryId, FanoutNodes>,
    /// Reverse index: alpha memory -> list of negative nodes that subscribe to it.
    alpha_to_negatives: HashMap<AlphaMemoryId, FanoutNodes>,
    /// Reverse index: alpha memory -> list of exists nodes that subscribe to it.
    alpha_to_exists: HashMap<AlphaMemoryId, FanoutNodes>,
}

impl BetaNetwork {
    /// Create a new beta network with a root node.
    ///
    /// The root node ID is provided by the caller to coordinate with alpha network
    /// node ID allocation.
    #[must_use]
    pub fn new(root_node_id: NodeId) -> Self {
        let mut nodes = HashMap::default();
        nodes.insert(
            root_node_id,
            BetaNode::Root {
                children: Vec::new(),
            },
        );

        // Start beta's internal node counter after the root
        let next_node_id = root_node_id.0 + 1;

        Self {
            nodes,
            memories: Vec::new(),
            neg_memories: Vec::new(),
            ncc_memories: Vec::new(),
            exists_memories: Vec::new(),
            root_id: root_node_id,
            next_node_id,
            next_memory_id: 0,
            next_neg_memory_id: 0,
            next_ncc_memory_id: 0,
            next_exists_memory_id: 0,
            alpha_to_joins: HashMap::default(),
            alpha_to_negatives: HashMap::default(),
            alpha_to_exists: HashMap::default(),
        }
    }

    /// Create a join node as a child of the given parent.
    ///
    /// Returns the new join node's ID and the ID of its associated beta memory.
    #[allow(clippy::cast_possible_truncation)] // Node/memory counts will never reach u32::MAX in practice.
    pub fn create_join_node(
        &mut self,
        parent: NodeId,
        alpha_memory: AlphaMemoryId,
        tests: Vec<JoinTest>,
        bindings: Vec<(SlotIndex, VarId)>,
    ) -> (NodeId, BetaMemoryId) {
        let node_id = NodeId(self.next_node_id);
        self.next_node_id += 1;

        let memory_id = BetaMemoryId(self.next_memory_id);
        self.next_memory_id += 1;

        let node = BetaNode::Join {
            parent,
            alpha_memory,
            tests,
            bindings,
            memory: memory_id,
            children: Vec::new(),
        };

        self.nodes.insert(node_id, node);
        debug_assert_eq!(self.memories.len(), memory_id.0 as usize);
        self.memories.push(BetaMemory::new(memory_id));

        self.attach_child_to_parent(parent, node_id);

        // Register in alpha_to_joins index
        self.alpha_to_joins
            .entry(alpha_memory)
            .or_default()
            .push(node_id);

        (node_id, memory_id)
    }

    /// Create a terminal node as a child of the given parent.
    ///
    /// Returns the new terminal node's ID.
    #[allow(clippy::cast_possible_truncation)] // Node count will never reach u32::MAX in practice.
    pub fn create_terminal_node(
        &mut self,
        parent: NodeId,
        rule: RuleId,
        salience: Salience,
    ) -> NodeId {
        let node_id = NodeId(self.next_node_id);
        self.next_node_id += 1;

        let node = BetaNode::Terminal {
            parent,
            rule,
            salience,
        };

        self.nodes.insert(node_id, node);

        self.attach_child_to_parent(parent, node_id);

        node_id
    }

    /// Create a negative node as a child of the given parent.
    ///
    /// A negative node blocks parent tokens when matching facts exist in the
    /// alpha memory. Returns the node ID, beta memory ID (for unblocked tokens),
    /// and negative memory ID (for blocker tracking).
    #[allow(clippy::cast_possible_truncation)]
    pub fn create_negative_node(
        &mut self,
        parent: NodeId,
        alpha_memory: AlphaMemoryId,
        tests: Vec<JoinTest>,
    ) -> (NodeId, BetaMemoryId, NegativeMemoryId) {
        let node_id = NodeId(self.next_node_id);
        self.next_node_id += 1;

        let memory_id = BetaMemoryId(self.next_memory_id);
        self.next_memory_id += 1;

        let neg_memory_id = NegativeMemoryId(self.next_neg_memory_id);
        self.next_neg_memory_id += 1;

        let node = BetaNode::Negative {
            parent,
            alpha_memory,
            tests,
            memory: memory_id,
            neg_memory: neg_memory_id,
            children: Vec::new(),
        };

        self.nodes.insert(node_id, node);
        debug_assert_eq!(self.memories.len(), memory_id.0 as usize);
        self.memories.push(BetaMemory::new(memory_id));
        debug_assert_eq!(self.neg_memories.len(), neg_memory_id.0 as usize);
        self.neg_memories.push(NegativeMemory::new(neg_memory_id));

        self.attach_child_to_parent(parent, node_id);

        // Register in alpha_to_negatives index
        self.alpha_to_negatives
            .entry(alpha_memory)
            .or_default()
            .push(node_id);

        (node_id, memory_id, neg_memory_id)
    }

    /// Allocate a new NCC memory without creating a node.
    ///
    /// This is used for coordination between NCC nodes and their partners.
    #[allow(clippy::cast_possible_truncation)]
    pub fn allocate_ncc_memory(&mut self) -> NccMemoryId {
        let id = NccMemoryId(self.next_ncc_memory_id);
        self.next_ncc_memory_id += 1;
        debug_assert_eq!(self.ncc_memories.len(), id.0 as usize);
        self.ncc_memories.push(NccMemory::new(id));
        id
    }

    /// Create an NCC node as a child of the given parent.
    ///
    /// An NCC node blocks parent tokens when the subnetwork (ending at the partner)
    /// produces any results. Returns the node ID, beta memory ID (for unblocked tokens),
    /// and NCC memory ID (for result tracking).
    ///
    /// The NCC memory must be allocated separately using `allocate_ncc_memory()`.
    #[allow(clippy::cast_possible_truncation)]
    pub fn create_ncc_node(
        &mut self,
        parent: NodeId,
        partner: NodeId,
        ncc_memory_id: NccMemoryId,
    ) -> (NodeId, BetaMemoryId) {
        let node_id = NodeId(self.next_node_id);
        self.next_node_id += 1;

        let memory_id = BetaMemoryId(self.next_memory_id);
        self.next_memory_id += 1;

        let node = BetaNode::Ncc {
            parent,
            partner,
            memory: memory_id,
            ncc_memory: ncc_memory_id,
            children: Vec::new(),
        };

        self.nodes.insert(node_id, node);
        debug_assert_eq!(self.memories.len(), memory_id.0 as usize);
        self.memories.push(BetaMemory::new(memory_id));

        self.attach_child_to_parent(parent, node_id);

        (node_id, memory_id)
    }

    /// Create an NCC partner node as a child of the given parent.
    ///
    /// The NCC partner sits at the bottom of the subnetwork and reports results
    /// to the NCC node. It has no children and no beta memory (it doesn't propagate
    /// downstream in the normal sense).
    ///
    /// Returns the partner node ID.
    #[allow(clippy::cast_possible_truncation)]
    pub fn create_ncc_partner(
        &mut self,
        parent: NodeId,
        ncc_node_id: NodeId,
        ncc_memory_id: NccMemoryId,
    ) -> NodeId {
        let node_id = NodeId(self.next_node_id);
        self.next_node_id += 1;

        let node = BetaNode::NccPartner {
            parent,
            ncc_node: ncc_node_id,
            ncc_memory: ncc_memory_id,
        };

        self.nodes.insert(node_id, node);

        self.attach_child_to_parent(parent, node_id);

        node_id
    }

    /// Update the partner pointer of an existing NCC node.
    ///
    /// This is used when the NCC node is created before the subnetwork bottom
    /// (and thus partner node) is known.
    pub fn set_ncc_partner(&mut self, ncc_node_id: NodeId, partner_id: NodeId) {
        if let Some(BetaNode::Ncc { partner, .. }) = self.nodes.get_mut(&ncc_node_id) {
            *partner = partner_id;
        }
    }

    /// Create an exists node as a child of the given parent.
    ///
    /// An exists node propagates when at least one supporting fact exists in the
    /// alpha memory. Returns the node ID, beta memory ID (for pass-through tokens),
    /// and exists memory ID (for support tracking).
    #[allow(clippy::cast_possible_truncation)]
    pub fn create_exists_node(
        &mut self,
        parent: NodeId,
        alpha_memory: AlphaMemoryId,
        tests: Vec<JoinTest>,
    ) -> (NodeId, BetaMemoryId, ExistsMemoryId) {
        let node_id = NodeId(self.next_node_id);
        self.next_node_id += 1;

        let memory_id = BetaMemoryId(self.next_memory_id);
        self.next_memory_id += 1;

        let exists_memory_id = ExistsMemoryId(self.next_exists_memory_id);
        self.next_exists_memory_id += 1;

        let node = BetaNode::Exists {
            parent,
            alpha_memory,
            tests,
            memory: memory_id,
            exists_memory: exists_memory_id,
            children: Vec::new(),
        };

        self.nodes.insert(node_id, node);
        debug_assert_eq!(self.memories.len(), memory_id.0 as usize);
        self.memories.push(BetaMemory::new(memory_id));
        debug_assert_eq!(self.exists_memories.len(), exists_memory_id.0 as usize);
        self.exists_memories
            .push(ExistsMemory::new(exists_memory_id));

        self.attach_child_to_parent(parent, node_id);

        // Register in alpha_to_exists index
        self.alpha_to_exists
            .entry(alpha_memory)
            .or_default()
            .push(node_id);

        (node_id, memory_id, exists_memory_id)
    }

    /// Get a beta node by ID.
    #[must_use]
    pub fn get_node(&self, id: NodeId) -> Option<&BetaNode> {
        self.nodes.get(&id)
    }

    /// Get a beta memory by ID.
    #[must_use]
    pub fn get_memory(&self, id: BetaMemoryId) -> Option<&BetaMemory> {
        self.memory(id)
    }

    /// Get a mutable reference to a beta memory by ID.
    pub fn get_memory_mut(&mut self, id: BetaMemoryId) -> Option<&mut BetaMemory> {
        self.memory_mut(id)
    }

    /// Get the root node ID.
    #[must_use]
    pub fn root_id(&self) -> NodeId {
        self.root_id
    }

    /// Get the beta memory ID associated with a node.
    ///
    /// Returns `None` for root nodes or nodes without a beta memory.
    #[must_use]
    pub fn memory_id_for_node(&self, node_id: NodeId) -> Option<BetaMemoryId> {
        match self.get_node(node_id)? {
            BetaNode::Join { memory, .. }
            | BetaNode::Negative { memory, .. }
            | BetaNode::Ncc { memory, .. }
            | BetaNode::Exists { memory, .. } => Some(*memory),
            _ => None,
        }
    }

    /// Get a negative memory by ID.
    #[must_use]
    pub fn get_neg_memory(&self, id: NegativeMemoryId) -> Option<&NegativeMemory> {
        self.neg_memory(id)
    }

    /// Get a mutable reference to a negative memory by ID.
    pub fn get_neg_memory_mut(&mut self, id: NegativeMemoryId) -> Option<&mut NegativeMemory> {
        self.neg_memory_mut(id)
    }

    /// Get an NCC memory by ID.
    #[must_use]
    pub fn get_ncc_memory(&self, id: NccMemoryId) -> Option<&NccMemory> {
        self.ncc_memory(id)
    }

    /// Get a mutable reference to an NCC memory by ID.
    pub fn get_ncc_memory_mut(&mut self, id: NccMemoryId) -> Option<&mut NccMemory> {
        self.ncc_memory_mut(id)
    }

    /// Get an exists memory by ID.
    #[must_use]
    pub fn get_exists_memory(&self, id: ExistsMemoryId) -> Option<&ExistsMemory> {
        self.exists_memory(id)
    }

    /// Get a mutable reference to an exists memory by ID.
    pub fn get_exists_memory_mut(&mut self, id: ExistsMemoryId) -> Option<&mut ExistsMemory> {
        self.exists_memory_mut(id)
    }

    /// Get the list of join nodes that subscribe to a given alpha memory.
    #[must_use]
    pub fn join_nodes_for_alpha(&self, alpha_mem: AlphaMemoryId) -> &[NodeId] {
        self.alpha_to_joins
            .get(&alpha_mem)
            .map_or(&[], |v| v.as_slice())
    }

    /// Get the list of negative nodes that subscribe to a given alpha memory.
    #[must_use]
    pub fn negative_nodes_for_alpha(&self, alpha_mem: AlphaMemoryId) -> &[NodeId] {
        self.alpha_to_negatives
            .get(&alpha_mem)
            .map_or(&[], |v| v.as_slice())
    }

    /// Get the list of exists nodes that subscribe to a given alpha memory.
    #[must_use]
    pub fn exists_nodes_for_alpha(&self, alpha_mem: AlphaMemoryId) -> &[NodeId] {
        self.alpha_to_exists
            .get(&alpha_mem)
            .map_or(&[], |v| v.as_slice())
    }

    /// Clear all runtime state from beta and negative memories, preserving network structure.
    pub fn clear_all_runtime(&mut self) {
        for memory in &mut self.memories {
            memory.clear();
        }
        for neg_memory in &mut self.neg_memories {
            neg_memory.clear();
        }
        for ncc_memory in &mut self.ncc_memories {
            ncc_memory.clear();
        }
        for exists_memory in &mut self.exists_memories {
            exists_memory.clear();
        }
    }

    /// Allocate a new node ID without creating a node.
    ///
    /// This is exposed for coordination with other ID allocation (e.g., `ReteNetwork`).
    #[allow(clippy::cast_possible_truncation)] // Node count will never reach u32::MAX in practice.
    pub fn allocate_node_id(&mut self) -> NodeId {
        let id = NodeId(self.next_node_id);
        self.next_node_id += 1;
        id
    }

    /// Iterate over all beta memory IDs.
    pub fn memory_ids(&self) -> impl Iterator<Item = BetaMemoryId> + '_ {
        self.memories.iter().map(|memory| memory.id)
    }

    /// Iterate over all negative memory IDs.
    pub fn neg_memory_ids(&self) -> impl Iterator<Item = NegativeMemoryId> + '_ {
        self.neg_memories.iter().map(|memory| memory.id)
    }

    /// Iterate over all NCC memory IDs.
    pub fn ncc_memory_ids(&self) -> impl Iterator<Item = NccMemoryId> + '_ {
        self.ncc_memories.iter().map(|memory| memory.id)
    }

    /// Find the NCC node that owns the given NCC memory.
    #[must_use]
    pub fn ncc_node_for_memory(&self, ncc_memory_id: NccMemoryId) -> Option<NodeId> {
        self.nodes.iter().find_map(|(node_id, node)| match node {
            BetaNode::Ncc { ncc_memory, .. } if *ncc_memory == ncc_memory_id => Some(*node_id),
            _ => None,
        })
    }

    /// Iterate over all exists memory IDs.
    pub fn exists_memory_ids(&self) -> impl Iterator<Item = ExistsMemoryId> + '_ {
        self.exists_memories.iter().map(|memory| memory.id)
    }

    fn attach_child_to_parent(&mut self, parent: NodeId, child_id: NodeId) {
        if let Some(parent_node) = self.nodes.get_mut(&parent) {
            match parent_node {
                BetaNode::Root { children }
                | BetaNode::Join { children, .. }
                | BetaNode::Negative { children, .. }
                | BetaNode::Ncc { children, .. }
                | BetaNode::Exists { children, .. } => {
                    children.push(child_id);
                }
                BetaNode::Terminal { .. } | BetaNode::NccPartner { .. } => {
                    // Terminal and NccPartner nodes cannot have children
                }
            }
        }
    }

    /// Verify internal consistency of the beta network.
    ///
    /// This method is gated behind `test` or `debug_assertions` and will panic
    /// if any inconsistencies are detected.
    #[cfg(any(test, debug_assertions))]
    #[allow(clippy::too_many_lines)]
    pub fn debug_assert_consistency(&self) {
        // Check 1: All node IDs in children fields exist in nodes map
        for (node_id, node) in &self.nodes {
            let children = match node {
                BetaNode::Root { children }
                | BetaNode::Join { children, .. }
                | BetaNode::Negative { children, .. }
                | BetaNode::Ncc { children, .. }
                | BetaNode::Exists { children, .. } => children,
                BetaNode::Terminal { .. } | BetaNode::NccPartner { .. } => continue,
            };

            for child_id in children {
                assert!(
                    self.nodes.contains_key(child_id),
                    "Node {node_id:?} has non-existent child {child_id:?}"
                );
            }
        }

        // Check 2: All parent references point to existing nodes
        for (node_id, node) in &self.nodes {
            let parent = match node {
                BetaNode::Join { parent, .. }
                | BetaNode::Terminal { parent, .. }
                | BetaNode::Negative { parent, .. }
                | BetaNode::Ncc { parent, .. }
                | BetaNode::NccPartner { parent, .. }
                | BetaNode::Exists { parent, .. } => *parent,
                BetaNode::Root { .. } => continue,
            };

            assert!(
                self.nodes.contains_key(&parent),
                "Node {node_id:?} has non-existent parent {parent:?}"
            );
        }

        // Check 3: All memory IDs in join/negative/ncc/exists nodes exist in memories map
        for (node_id, node) in &self.nodes {
            match node {
                BetaNode::Join { memory, .. } => {
                    assert!(
                        self.memory(*memory).is_some(),
                        "Join node {node_id:?} references non-existent memory {memory:?}"
                    );
                }
                BetaNode::Negative {
                    memory, neg_memory, ..
                } => {
                    assert!(
                        self.memory(*memory).is_some(),
                        "Negative node {node_id:?} references non-existent beta memory {memory:?}"
                    );
                    assert!(
                        self.neg_memory(*neg_memory).is_some(),
                        "Negative node {node_id:?} references non-existent negative memory {neg_memory:?}"
                    );
                }
                BetaNode::Ncc {
                    memory,
                    ncc_memory,
                    partner,
                    ..
                } => {
                    assert!(
                        self.memory(*memory).is_some(),
                        "NCC node {node_id:?} references non-existent beta memory {memory:?}"
                    );
                    assert!(
                        self.ncc_memory(*ncc_memory).is_some(),
                        "NCC node {node_id:?} references non-existent NCC memory {ncc_memory:?}"
                    );
                    assert!(
                        self.nodes.contains_key(partner),
                        "NCC node {node_id:?} references non-existent partner node {partner:?}"
                    );
                }
                BetaNode::NccPartner {
                    ncc_node,
                    ncc_memory,
                    ..
                } => {
                    assert!(
                        self.nodes.contains_key(ncc_node),
                        "NCC partner node {node_id:?} references non-existent NCC node {ncc_node:?}"
                    );
                    assert!(
                        self.ncc_memory(*ncc_memory).is_some(),
                        "NCC partner node {node_id:?} references non-existent NCC memory {ncc_memory:?}"
                    );
                }
                BetaNode::Exists {
                    memory,
                    exists_memory,
                    ..
                } => {
                    assert!(
                        self.memory(*memory).is_some(),
                        "Exists node {node_id:?} references non-existent beta memory {memory:?}"
                    );
                    assert!(
                        self.exists_memory(*exists_memory).is_some(),
                        "Exists node {node_id:?} references non-existent exists memory {exists_memory:?}"
                    );
                }
                _ => {}
            }
        }

        // Check 4: All join nodes referenced in alpha_to_joins exist in nodes map
        for (alpha_mem_id, join_nodes) in &self.alpha_to_joins {
            for join_node_id in join_nodes {
                assert!(
                    self.nodes.contains_key(join_node_id),
                    "Alpha memory {alpha_mem_id:?} references non-existent join node {join_node_id:?}"
                );

                assert!(
                    matches!(self.nodes.get(join_node_id), Some(BetaNode::Join { .. })),
                    "Alpha memory {alpha_mem_id:?} references node {join_node_id:?} which is not a join node"
                );
            }
        }

        // Check 4b: All negative nodes referenced in alpha_to_negatives exist
        for (alpha_mem_id, neg_nodes) in &self.alpha_to_negatives {
            for neg_node_id in neg_nodes {
                assert!(
                    self.nodes.contains_key(neg_node_id),
                    "Alpha memory {alpha_mem_id:?} references non-existent negative node {neg_node_id:?}"
                );

                assert!(
                    matches!(
                        self.nodes.get(neg_node_id),
                        Some(BetaNode::Negative { .. })
                    ),
                    "Alpha memory {alpha_mem_id:?} references node {neg_node_id:?} which is not a negative node"
                );
            }
        }

        // Check 4c: All exists nodes referenced in alpha_to_exists exist
        for (alpha_mem_id, exists_nodes) in &self.alpha_to_exists {
            for exists_node_id in exists_nodes {
                assert!(
                    self.nodes.contains_key(exists_node_id),
                    "Alpha memory {alpha_mem_id:?} references non-existent exists node {exists_node_id:?}"
                );

                assert!(
                    matches!(
                        self.nodes.get(exists_node_id),
                        Some(BetaNode::Exists { .. })
                    ),
                    "Alpha memory {alpha_mem_id:?} references node {exists_node_id:?} which is not an exists node"
                );
            }
        }

        // Check 5: Root node exists and is actually a Root variant
        assert!(
            self.nodes.contains_key(&self.root_id),
            "Root node {:?} does not exist in nodes map",
            self.root_id
        );

        assert!(
            matches!(self.nodes.get(&self.root_id), Some(BetaNode::Root { .. })),
            "Root node {:?} is not a Root variant",
            self.root_id
        );

        // Check 6: All negative memories are internally consistent
        for neg_mem in &self.neg_memories {
            neg_mem.debug_assert_consistency();
        }

        // Check 7: All NCC memories are internally consistent
        for ncc_mem in &self.ncc_memories {
            ncc_mem.debug_assert_consistency();
        }

        // Check 8: All exists memories are internally consistent
        for exists_mem in &self.exists_memories {
            exists_mem.debug_assert_consistency();
        }
    }

    fn memory(&self, id: BetaMemoryId) -> Option<&BetaMemory> {
        self.memories.get(id.0 as usize)
    }

    fn memory_mut(&mut self, id: BetaMemoryId) -> Option<&mut BetaMemory> {
        self.memories.get_mut(id.0 as usize)
    }

    fn neg_memory(&self, id: NegativeMemoryId) -> Option<&NegativeMemory> {
        self.neg_memories.get(id.0 as usize)
    }

    fn neg_memory_mut(&mut self, id: NegativeMemoryId) -> Option<&mut NegativeMemory> {
        self.neg_memories.get_mut(id.0 as usize)
    }

    fn ncc_memory(&self, id: NccMemoryId) -> Option<&NccMemory> {
        self.ncc_memories.get(id.0 as usize)
    }

    fn ncc_memory_mut(&mut self, id: NccMemoryId) -> Option<&mut NccMemory> {
        self.ncc_memories.get_mut(id.0 as usize)
    }

    fn exists_memory(&self, id: ExistsMemoryId) -> Option<&ExistsMemory> {
        self.exists_memories.get(id.0 as usize)
    }

    fn exists_memory_mut(&mut self, id: ExistsMemoryId) -> Option<&mut ExistsMemory> {
        self.exists_memories.get_mut(id.0 as usize)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn beta_memory_new_is_empty() {
        let mem = BetaMemory::new(BetaMemoryId(0));
        assert!(mem.is_empty());
        assert_eq!(mem.len(), 0);
    }

    #[test]
    fn beta_memory_insert_and_contains() {
        use slotmap::SlotMap;

        let mut mem = BetaMemory::new(BetaMemoryId(0));
        let mut temp_map: SlotMap<TokenId, ()> = SlotMap::with_key();
        let t1 = temp_map.insert(());
        let t2 = temp_map.insert(());

        mem.insert(t1);
        assert!(mem.contains(t1));
        assert!(!mem.contains(t2));
        assert_eq!(mem.len(), 1);

        mem.insert(t2);
        assert!(mem.contains(t1));
        assert!(mem.contains(t2));
        assert_eq!(mem.len(), 2);

        // Insert duplicate
        mem.insert(t1);
        assert_eq!(mem.len(), 2);
    }

    #[test]
    fn beta_memory_remove() {
        use slotmap::SlotMap;

        let mut mem = BetaMemory::new(BetaMemoryId(0));
        let mut temp_map: SlotMap<TokenId, ()> = SlotMap::with_key();
        let t1 = temp_map.insert(());

        mem.insert(t1);
        assert!(mem.contains(t1));

        mem.remove(t1);
        assert!(!mem.contains(t1));
        assert!(mem.is_empty());

        // Remove non-existent
        mem.remove(t1);
        assert!(mem.is_empty());
    }

    #[test]
    fn beta_network_create_join_and_terminal() {
        let root = NodeId(100);
        let mut net = BetaNetwork::new(root);

        assert_eq!(net.root_id(), root);
        assert!(net.get_node(root).is_some());

        // Create a join node
        let alpha_mem = AlphaMemoryId(5);
        let tests = vec![JoinTest {
            alpha_slot: SlotIndex::Ordered(0),
            beta_var: VarId(0),
            test_type: JoinTestType::Equal,
        }];

        let (join_id, mem_id) = net.create_join_node(root, alpha_mem, tests.clone(), vec![]);

        // Verify join node was created
        let join_node = net.get_node(join_id).expect("Join node should exist");
        if let BetaNode::Join {
            parent,
            alpha_memory,
            tests: node_tests,
            bindings,
            memory,
            children,
        } = join_node
        {
            assert_eq!(*parent, root);
            assert_eq!(*alpha_memory, alpha_mem);
            assert_eq!(node_tests, &tests);
            assert!(bindings.is_empty(), "No bindings in this test");
            assert_eq!(*memory, mem_id);
            assert!(children.is_empty());
        } else {
            panic!("Expected Join node");
        }

        // Verify memory was created
        let memory = net.get_memory(mem_id).expect("Memory should exist");
        assert!(memory.is_empty());
        assert_eq!(memory.id, mem_id);

        // Verify root has join as child
        if let Some(BetaNode::Root { children }) = net.get_node(root) {
            assert_eq!(children.len(), 1);
            assert_eq!(children[0], join_id);
        } else {
            panic!("Expected Root node");
        }

        // Verify alpha_to_joins index
        let joins = net.join_nodes_for_alpha(alpha_mem);
        assert_eq!(joins.len(), 1);
        assert_eq!(joins[0], join_id);

        // Create a terminal node
        let rule = RuleId(42);
        let terminal_id = net.create_terminal_node(join_id, rule, Salience::DEFAULT);

        let terminal_node = net
            .get_node(terminal_id)
            .expect("Terminal node should exist");
        if let BetaNode::Terminal {
            parent,
            rule: node_rule,
            salience,
        } = terminal_node
        {
            assert_eq!(*parent, join_id);
            assert_eq!(*node_rule, rule);
            assert_eq!(*salience, Salience::DEFAULT);
        } else {
            panic!("Expected Terminal node");
        }

        // Verify join has terminal as child
        if let Some(BetaNode::Join { children, .. }) = net.get_node(join_id) {
            assert_eq!(children.len(), 1);
            assert_eq!(children[0], terminal_id);
        } else {
            panic!("Expected Join node");
        }
    }
}

#[cfg(test)]
mod proptests {
    use super::*;
    use proptest::prelude::*;
    use slotmap::SlotMap;

    // ===========================================================================
    // BetaMemory property tests
    // ===========================================================================

    /// Shadow model for `BetaMemory`: a `HashSet` of token indices (0..5).
    #[derive(Default)]
    struct BetaModel {
        tokens: std::collections::HashSet<usize>,
    }

    impl BetaModel {
        fn insert(&mut self, idx: usize) {
            self.tokens.insert(idx);
        }

        fn remove(&mut self, idx: usize) {
            self.tokens.remove(&idx);
        }

        fn contains(&self, idx: usize) -> bool {
            self.tokens.contains(&idx)
        }

        fn len(&self) -> usize {
            self.tokens.len()
        }

        fn is_empty(&self) -> bool {
            self.tokens.is_empty()
        }
    }

    /// An operation that can be applied to a `BetaMemory`.
    #[derive(Clone, Debug)]
    enum BetaOp {
        Insert(usize),
        Remove(usize),
    }

    fn beta_op_strategy() -> impl Strategy<Value = BetaOp> {
        prop_oneof![
            (0..5_usize).prop_map(BetaOp::Insert),
            (0..5_usize).prop_map(BetaOp::Remove),
        ]
    }

    fn apply_beta_op(op: &BetaOp, mem: &mut BetaMemory, model: &mut BetaModel, tokens: &[TokenId]) {
        match *op {
            BetaOp::Insert(idx) => {
                mem.insert(tokens[idx]);
                model.insert(idx);
            }
            BetaOp::Remove(idx) => {
                mem.remove(tokens[idx]);
                model.remove(idx);
            }
        }
    }

    proptest! {
        /// After arbitrary insert/remove ops, `contains`, `len`, and `is_empty`
        /// all match the shadow model (a plain `HashSet`).
        #[test]
        fn beta_memory_model_matches_implementation(
            ops in proptest::collection::vec(beta_op_strategy(), 0..100)
        ) {
            let mut token_map: SlotMap<TokenId, ()> = SlotMap::with_key();
            let tokens: Vec<TokenId> = (0..5).map(|_| token_map.insert(())).collect();

            let mut mem = BetaMemory::new(BetaMemoryId(0));
            let mut model = BetaModel::default();

            for op in &ops {
                apply_beta_op(op, &mut mem, &mut model, &tokens);
            }

            // contains matches for every token in the pool
            for (idx, &tok) in tokens.iter().enumerate() {
                prop_assert_eq!(
                    mem.contains(tok),
                    model.contains(idx),
                    "contains mismatch for token index {}",
                    idx
                );
            }

            // len and is_empty are consistent
            prop_assert_eq!(mem.len(), model.len(), "len mismatch");
            prop_assert_eq!(mem.is_empty(), model.is_empty(), "is_empty mismatch");
        }

        /// Inserting the same token twice is idempotent: `len` does not increase.
        #[test]
        fn beta_memory_insert_idempotent(token_idx in 0..5_usize) {
            let mut token_map: SlotMap<TokenId, ()> = SlotMap::with_key();
            let tokens: Vec<TokenId> = (0..5).map(|_| token_map.insert(())).collect();

            let mut mem = BetaMemory::new(BetaMemoryId(0));
            mem.insert(tokens[token_idx]);
            let len_after_first = mem.len();

            mem.insert(tokens[token_idx]);
            let len_after_second = mem.len();

            prop_assert_eq!(len_after_first, len_after_second,
                "duplicate insert must not increase len");
            prop_assert!(mem.contains(tokens[token_idx]),
                "token must be present after insert");
        }

        /// Removing a token that is not present is a no-op: `len` is unchanged.
        #[test]
        fn beta_memory_remove_missing_is_noop(
            present_idx in 0..4_usize,
            absent_idx in 4..5_usize,
        ) {
            let mut token_map: SlotMap<TokenId, ()> = SlotMap::with_key();
            let tokens: Vec<TokenId> = (0..5).map(|_| token_map.insert(())).collect();

            let mut mem = BetaMemory::new(BetaMemoryId(0));
            mem.insert(tokens[present_idx]);

            let len_before = mem.len();
            mem.remove(tokens[absent_idx]); // absent_idx not inserted
            let len_after = mem.len();

            prop_assert_eq!(len_before, len_after,
                "removing absent token must not change len");
        }

        /// After `clear()`, the memory is always empty.
        #[test]
        fn beta_memory_clear_resets_everything(
            ops in proptest::collection::vec(beta_op_strategy(), 0..50)
        ) {
            let mut token_map: SlotMap<TokenId, ()> = SlotMap::with_key();
            let tokens: Vec<TokenId> = (0..5).map(|_| token_map.insert(())).collect();

            let mut mem = BetaMemory::new(BetaMemoryId(0));
            let mut model = BetaModel::default();

            for op in &ops {
                apply_beta_op(op, &mut mem, &mut model, &tokens);
            }

            mem.clear();

            prop_assert!(mem.is_empty(), "is_empty must be true after clear()");
            prop_assert_eq!(mem.len(), 0, "len must be 0 after clear()");
        }
    }

    // ===========================================================================
    // BetaMemory variable-index property tests
    // ===========================================================================

    use crate::binding::BindingSet;
    use crate::value::{AtomKey, Value};
    use std::rc::Rc;

    /// Shadow model for indexed `BetaMemory`: tracks both the token set and a
    /// per-variable, per-key set of token IDs.
    #[derive(Default)]
    struct IndexedBetaModel {
        tokens: std::collections::HashSet<usize>,
        /// `var_idx` -> `key_idx` -> set of token indices
        var_index: std::collections::HashMap<
            usize,
            std::collections::HashMap<i64, std::collections::HashSet<usize>>,
        >,
    }

    impl IndexedBetaModel {
        fn insert(&mut self, idx: usize, key_val: i64, indexed_var: usize) {
            self.tokens.insert(idx);
            self.var_index
                .entry(indexed_var)
                .or_default()
                .entry(key_val)
                .or_default()
                .insert(idx);
        }

        fn remove(&mut self, idx: usize, key_val: i64, indexed_var: usize) {
            self.tokens.remove(&idx);
            if let Some(key_map) = self.var_index.get_mut(&indexed_var) {
                if let Some(set) = key_map.get_mut(&key_val) {
                    set.remove(&idx);
                    if set.is_empty() {
                        key_map.remove(&key_val);
                    }
                }
            }
        }

        fn lookup(&self, var_idx: usize, key_val: i64) -> std::collections::HashSet<usize> {
            self.var_index
                .get(&var_idx)
                .and_then(|m| m.get(&key_val))
                .cloned()
                .unwrap_or_default()
        }
    }

    /// An operation for the indexed beta memory tests.
    #[derive(Clone, Debug)]
    enum IndexedBetaOp {
        Insert { token_idx: usize, key_val: i64 },
        Remove { token_idx: usize },
    }

    fn indexed_beta_op_strategy() -> impl Strategy<Value = IndexedBetaOp> {
        prop_oneof![
            (0..5_usize, 0..4_i64).prop_map(|(t, k)| IndexedBetaOp::Insert {
                token_idx: t,
                key_val: k,
            }),
            (0..5_usize).prop_map(|t| IndexedBetaOp::Remove { token_idx: t }),
        ]
    }

    /// Create a `BindingSet` with a single integer binding at `VarId(var_idx)`.
    fn make_bindings(var_idx: u16, key_val: i64) -> BindingSet {
        let mut bs = BindingSet::new();
        bs.set(VarId(var_idx), Rc::new(Value::Integer(key_val)));
        bs
    }

    proptest! {
        /// After arbitrary insert_indexed/remove_indexed ops, the variable index
        /// matches a shadow model. Every token in the index is also in the main
        /// token set, and every token in the main set with a matching binding
        /// appears in the index.
        #[test]
        fn beta_memory_var_index_tracks_tokens(
            ops in proptest::collection::vec(indexed_beta_op_strategy(), 0..80)
        ) {
            let mut token_map: SlotMap<TokenId, ()> = SlotMap::with_key();
            let tokens: Vec<TokenId> = (0..5).map(|_| token_map.insert(())).collect();

            let indexed_var = VarId(0);
            let mut mem = BetaMemory::new(BetaMemoryId(0));
            mem.request_var_index(indexed_var);

            let mut model = IndexedBetaModel::default();

            // Track the last key value each token was inserted with (for removal)
            let mut token_keys: std::collections::HashMap<usize, i64> = std::collections::HashMap::new();

            for op in &ops {
                match *op {
                    IndexedBetaOp::Insert { token_idx, key_val } => {
                        // If already present with a different key, remove first
                        if let Some(&old_key) = token_keys.get(&token_idx) {
                            if model.tokens.contains(&token_idx) {
                                let old_bindings = make_bindings(0, old_key);
                                mem.remove_indexed(tokens[token_idx], &old_bindings);
                                model.remove(token_idx, old_key, 0);
                            }
                        }
                        let bindings = make_bindings(0, key_val);
                        mem.insert_indexed(tokens[token_idx], &bindings);
                        model.insert(token_idx, key_val, 0);
                        token_keys.insert(token_idx, key_val);
                    }
                    IndexedBetaOp::Remove { token_idx } => {
                        if let Some(&stored_key) = token_keys.get(&token_idx) {
                            if model.tokens.contains(&token_idx) {
                                let bindings = make_bindings(0, stored_key);
                                mem.remove_indexed(tokens[token_idx], &bindings);
                                model.remove(token_idx, stored_key, 0);
                                token_keys.remove(&token_idx);
                            }
                        }
                    }
                }
            }

            // Verify: for each key value, lookup matches model
            for key_val in 0..4_i64 {
                let atom_key = AtomKey::Integer(key_val);
                let actual: std::collections::HashSet<TokenId> = mem
                    .lookup_by_var(indexed_var, &atom_key)
                    .map(|sv| sv.iter().copied().collect())
                    .unwrap_or_default();
                let expected_indices = model.lookup(0, key_val);
                let expected: std::collections::HashSet<TokenId> =
                    expected_indices.iter().map(|&idx| tokens[idx]).collect();
                prop_assert_eq!(
                    actual, expected,
                    "lookup_by_var mismatch for key_val={}",
                    key_val
                );
            }

            // Verify: every token in the index is in the main set
            for key_val in 0..4_i64 {
                let atom_key = AtomKey::Integer(key_val);
                if let Some(indexed_tokens) = mem.lookup_by_var(indexed_var, &atom_key) {
                    for &tid in indexed_tokens {
                        prop_assert!(
                            mem.contains(tid),
                            "indexed token {:?} not in main token set (key={})",
                            tid,
                            key_val
                        );
                    }
                }
            }
        }

        /// `lookup_by_var` returns the same result as a full scan+filter.
        #[test]
        fn beta_memory_lookup_matches_scan(
            key_vals in proptest::collection::vec(0..4_i64, 5..=5)
        ) {
            let mut token_map: SlotMap<TokenId, ()> = SlotMap::with_key();
            let tokens: Vec<TokenId> = (0..5).map(|_| token_map.insert(())).collect();

            let indexed_var = VarId(0);
            let mut mem = BetaMemory::new(BetaMemoryId(0));
            mem.request_var_index(indexed_var);

            // Build a map from TokenId -> key_val for brute-force scanning
            let mut tid_to_key: std::collections::HashMap<TokenId, i64> =
                std::collections::HashMap::new();

            // Insert 5 tokens with potentially overlapping keys
            for (i, &kv) in key_vals.iter().enumerate() {
                let bindings = make_bindings(0, kv);
                mem.insert_indexed(tokens[i], &bindings);
                tid_to_key.insert(tokens[i], kv);
            }

            // For each possible key, verify lookup matches a brute-force scan
            for query_key in 0..4_i64 {
                let atom_key = AtomKey::Integer(query_key);

                // Indexed lookup
                let indexed_result: std::collections::HashSet<TokenId> = mem
                    .lookup_by_var(indexed_var, &atom_key)
                    .map(|sv| sv.iter().copied().collect())
                    .unwrap_or_default();

                // Brute-force scan using tid_to_key map
                let scan_result: std::collections::HashSet<TokenId> = mem
                    .iter()
                    .filter(|tid| tid_to_key.get(tid) == Some(&query_key))
                    .collect();

                prop_assert_eq!(
                    indexed_result, scan_result,
                    "index lookup vs scan mismatch for key={}",
                    query_key
                );
            }
        }

        /// After `clear()`, `indexed_vars` is preserved but lookups return empty.
        #[test]
        fn beta_memory_clear_preserves_indexed_vars(
            key_vals in proptest::collection::vec(0..4_i64, 1..5)
        ) {
            let mut token_map: SlotMap<TokenId, ()> = SlotMap::with_key();
            let tokens: Vec<TokenId> = (0..5).map(|_| token_map.insert(())).collect();

            let indexed_var = VarId(0);
            let mut mem = BetaMemory::new(BetaMemoryId(0));
            mem.request_var_index(indexed_var);

            for (i, &kv) in key_vals.iter().enumerate() {
                let bindings = make_bindings(0, kv);
                mem.insert_indexed(tokens[i], &bindings);
            }

            mem.clear();

            // Index config preserved
            prop_assert!(mem.is_var_indexed(indexed_var),
                "indexed_vars must survive clear()");

            // Lookups return empty
            for kv in 0..4_i64 {
                let atom_key = AtomKey::Integer(kv);
                let result = mem.lookup_by_var(indexed_var, &atom_key);
                prop_assert!(
                    result.is_none() || result.unwrap().is_empty(),
                    "lookup must return empty after clear() for key={}",
                    kv
                );
            }
        }

        /// `request_var_index` is idempotent: calling it twice doesn't duplicate.
        #[test]
        fn beta_memory_request_var_index_idempotent(var_id in 0..10_u16) {
            let mut mem = BetaMemory::new(BetaMemoryId(0));
            let vid = VarId(var_id);

            mem.request_var_index(vid);
            prop_assert!(mem.is_var_indexed(vid));

            mem.request_var_index(vid);
            prop_assert!(mem.is_var_indexed(vid));

            // indexed_vars should have exactly one entry for this var
            let count = mem.indexed_vars.iter().filter(|&&v| v == vid).count();
            prop_assert_eq!(count, 1, "duplicate entry in indexed_vars after double request");
        }
    }

    // ===========================================================================
    // BetaNetwork property tests
    // ===========================================================================

    /// An operation that creates a new node in the `BetaNetwork`.
    ///
    /// All parents are drawn from `created_node_ids` (already-created nodes)
    /// or the root. Alpha memories are drawn from a pre-allocated pool.
    #[allow(clippy::enum_variant_names)] // All variants are "make X node" operations; the prefix is intentional
    #[derive(Clone, Debug)]
    enum NetOp {
        /// Create a join node attached to parent at `parent_slot` in `created_nodes`.
        MakeJoin {
            alpha_idx: usize,
            parent_slot: usize,
        },
        /// Create a terminal node attached to the parent at `parent_slot`.
        MakeTerminal { parent_slot: usize, rule_id: u32 },
        /// Create a negative node attached to parent at `parent_slot`.
        MakeNegative {
            alpha_idx: usize,
            parent_slot: usize,
        },
        /// Create an exists node attached to parent at `parent_slot`.
        MakeExists {
            alpha_idx: usize,
            parent_slot: usize,
        },
    }

    fn net_op_strategy() -> impl Strategy<Value = NetOp> {
        prop_oneof![
            3 => (0..4_usize, 0..8_usize).prop_map(|(a, p)| NetOp::MakeJoin { alpha_idx: a, parent_slot: p }),
            2 => (0..8_usize, 0..100_u32).prop_map(|(p, r)| NetOp::MakeTerminal { parent_slot: p, rule_id: r }),
            2 => (0..4_usize, 0..8_usize).prop_map(|(a, p)| NetOp::MakeNegative { alpha_idx: a, parent_slot: p }),
            2 => (0..4_usize, 0..8_usize).prop_map(|(a, p)| NetOp::MakeExists { alpha_idx: a, parent_slot: p }),
        ]
    }

    proptest! {
        /// Arbitrary sequences of node-creation operations maintain the structural
        /// consistency invariants verified by `debug_assert_consistency`.
        ///
        /// This verifies: parent/child references are valid, memory IDs are valid,
        /// alpha reverse indices are accurate, and the root node is always present.
        #[test]
        fn beta_network_arbitrary_ops_maintain_consistency(
            ops in proptest::collection::vec(net_op_strategy(), 0..30)
        ) {
            // Pre-allocate a small pool of alpha memory IDs.
            // The real alpha network isn't involved here; we only test structural
            // consistency of the beta network itself.
            let alpha_mems = [
                AlphaMemoryId(0),
                AlphaMemoryId(1),
                AlphaMemoryId(2),
                AlphaMemoryId(3),
            ];

            let root = NodeId(0);
            let mut net = BetaNetwork::new(root);

            // Track all created node IDs so we can use them as parents.
            // The root is always a valid parent.
            let mut created_nodes: Vec<NodeId> = vec![root];

            for op in &ops {
                // Resolve the parent slot modulo current length (always >= 1 because
                // root is always present).
                let num_nodes = created_nodes.len();

                match *op {
                    NetOp::MakeJoin { alpha_idx, parent_slot } => {
                        let parent = created_nodes[parent_slot % num_nodes];
                        let alpha = alpha_mems[alpha_idx];
                        let (node_id, _mem_id) =
                            net.create_join_node(parent, alpha, vec![], vec![]);
                        // Verify the new node immediately exists in the network
                        prop_assert!(net.get_node(node_id).is_some(),
                            "newly created join node must exist");
                        // Verify the alpha reverse index was updated
                        prop_assert!(
                            net.join_nodes_for_alpha(alpha).contains(&node_id),
                            "join node must appear in alpha_to_joins"
                        );
                        created_nodes.push(node_id);
                    }
                    NetOp::MakeTerminal { parent_slot, rule_id } => {
                        let parent = created_nodes[parent_slot % num_nodes];
                        let node_id =
                            net.create_terminal_node(parent, RuleId(rule_id), Salience::DEFAULT);
                        prop_assert!(net.get_node(node_id).is_some(),
                            "newly created terminal node must exist");
                        // Terminal nodes are not added to created_nodes as parents
                        // because they cannot have children (attach_child_to_parent
                        // is a no-op for Terminal/NccPartner nodes).
                    }
                    NetOp::MakeNegative { alpha_idx, parent_slot } => {
                        let parent = created_nodes[parent_slot % num_nodes];
                        let alpha = alpha_mems[alpha_idx];
                        let (node_id, _mem_id, _neg_mem_id) =
                            net.create_negative_node(parent, alpha, vec![]);
                        prop_assert!(net.get_node(node_id).is_some(),
                            "newly created negative node must exist");
                        // Verify the alpha reverse index was updated
                        prop_assert!(
                            net.negative_nodes_for_alpha(alpha).contains(&node_id),
                            "negative node must appear in alpha_to_negatives"
                        );
                        created_nodes.push(node_id);
                    }
                    NetOp::MakeExists { alpha_idx, parent_slot } => {
                        let parent = created_nodes[parent_slot % num_nodes];
                        let alpha = alpha_mems[alpha_idx];
                        let (node_id, _mem_id, _exists_mem_id) =
                            net.create_exists_node(parent, alpha, vec![]);
                        prop_assert!(net.get_node(node_id).is_some(),
                            "newly created exists node must exist");
                        // Verify the alpha reverse index was updated
                        prop_assert!(
                            net.exists_nodes_for_alpha(alpha).contains(&node_id),
                            "exists node must appear in alpha_to_exists"
                        );
                        created_nodes.push(node_id);
                    }
                }

                // Full structural consistency check after every operation.
                net.debug_assert_consistency();
            }

            // All created nodes must still be present in the network.
            for &node_id in &created_nodes {
                prop_assert!(net.get_node(node_id).is_some(),
                    "node {node_id:?} must still exist after all ops");
            }
        }

        /// Every join node created for a given alpha memory appears exactly once
        /// in `join_nodes_for_alpha`.
        #[test]
        fn join_nodes_for_alpha_tracks_all_joins(
            alpha_idxs in proptest::collection::vec(0..4_usize, 1..10)
        ) {
            let alpha_mems = [
                AlphaMemoryId(0),
                AlphaMemoryId(1),
                AlphaMemoryId(2),
                AlphaMemoryId(3),
            ];

            let root = NodeId(0);
            let mut net = BetaNetwork::new(root);

            // Track how many join nodes we create per alpha memory.
            let mut expected_counts = [0_usize; 4];

            for &a_idx in &alpha_idxs {
                let alpha = alpha_mems[a_idx];
                net.create_join_node(root, alpha, vec![], vec![]);
                expected_counts[a_idx] += 1;
            }

            // Verify counts match.
            for (a_idx, &expected) in expected_counts.iter().enumerate() {
                let actual = net.join_nodes_for_alpha(alpha_mems[a_idx]).len();
                prop_assert_eq!(
                    actual, expected,
                    "join_nodes_for_alpha count mismatch for alpha_idx {}",
                    a_idx
                );
            }

            net.debug_assert_consistency();
        }

        /// Every negative node created for a given alpha memory appears in
        /// `negative_nodes_for_alpha`.
        #[test]
        fn negative_nodes_for_alpha_tracks_all_negatives(
            alpha_idxs in proptest::collection::vec(0..4_usize, 1..10)
        ) {
            let alpha_mems = [
                AlphaMemoryId(0),
                AlphaMemoryId(1),
                AlphaMemoryId(2),
                AlphaMemoryId(3),
            ];

            let root = NodeId(0);
            let mut net = BetaNetwork::new(root);

            let mut expected_counts = [0_usize; 4];

            for &a_idx in &alpha_idxs {
                let alpha = alpha_mems[a_idx];
                net.create_negative_node(root, alpha, vec![]);
                expected_counts[a_idx] += 1;
            }

            for (a_idx, &expected) in expected_counts.iter().enumerate() {
                let actual = net.negative_nodes_for_alpha(alpha_mems[a_idx]).len();
                prop_assert_eq!(
                    actual, expected,
                    "negative_nodes_for_alpha count mismatch for alpha_idx {}",
                    a_idx
                );
            }

            net.debug_assert_consistency();
        }

        /// Every exists node created for a given alpha memory appears in
        /// `exists_nodes_for_alpha`.
        #[test]
        fn exists_nodes_for_alpha_tracks_all_exists(
            alpha_idxs in proptest::collection::vec(0..4_usize, 1..10)
        ) {
            let alpha_mems = [
                AlphaMemoryId(0),
                AlphaMemoryId(1),
                AlphaMemoryId(2),
                AlphaMemoryId(3),
            ];

            let root = NodeId(0);
            let mut net = BetaNetwork::new(root);

            let mut expected_counts = [0_usize; 4];

            for &a_idx in &alpha_idxs {
                let alpha = alpha_mems[a_idx];
                net.create_exists_node(root, alpha, vec![]);
                expected_counts[a_idx] += 1;
            }

            for (a_idx, &expected) in expected_counts.iter().enumerate() {
                let actual = net.exists_nodes_for_alpha(alpha_mems[a_idx]).len();
                prop_assert_eq!(
                    actual, expected,
                    "exists_nodes_for_alpha count mismatch for alpha_idx {}",
                    a_idx
                );
            }

            net.debug_assert_consistency();
        }

        /// Parent-child relationships are always bi-directional: a node listed as
        /// the parent of a child should list that child in its `children` vec.
        #[test]
        fn parent_child_relationship_bidirectional(
            ops in proptest::collection::vec(net_op_strategy(), 0..20)
        ) {
            let alpha_mems = [
                AlphaMemoryId(0),
                AlphaMemoryId(1),
                AlphaMemoryId(2),
                AlphaMemoryId(3),
            ];

            let root = NodeId(0);
            let mut net = BetaNetwork::new(root);
            let mut created_nodes: Vec<NodeId> = vec![root];

            for op in &ops {
                let num_nodes = created_nodes.len();
                let (node_id_opt, parent_id) = match *op {
                    NetOp::MakeJoin { alpha_idx, parent_slot } => {
                        let parent = created_nodes[parent_slot % num_nodes];
                        let (node_id, _) =
                            net.create_join_node(parent, alpha_mems[alpha_idx], vec![], vec![]);
                        created_nodes.push(node_id);
                        (Some(node_id), parent)
                    }
                    NetOp::MakeTerminal { parent_slot, rule_id } => {
                        let parent = created_nodes[parent_slot % num_nodes];
                        let node_id =
                            net.create_terminal_node(parent, RuleId(rule_id), Salience::DEFAULT);
                        (Some(node_id), parent)
                    }
                    NetOp::MakeNegative { alpha_idx, parent_slot } => {
                        let parent = created_nodes[parent_slot % num_nodes];
                        let (node_id, _, _) =
                            net.create_negative_node(parent, alpha_mems[alpha_idx], vec![]);
                        created_nodes.push(node_id);
                        (Some(node_id), parent)
                    }
                    NetOp::MakeExists { alpha_idx, parent_slot } => {
                        let parent = created_nodes[parent_slot % num_nodes];
                        let (node_id, _, _) =
                            net.create_exists_node(parent, alpha_mems[alpha_idx], vec![]);
                        created_nodes.push(node_id);
                        (Some(node_id), parent)
                    }
                };

                // If a node was created, verify the parent's children list contains it.
                // (Terminal nodes don't get added as children of Terminal/NccPartner parents,
                //  but attach_child_to_parent is always called — it just silently skips
                //  those node types. We only assert for non-terminal parents.)
                if let Some(node_id) = node_id_opt {
                    if let Some(parent_node) = net.get_node(parent_id) {
                        let children: Option<&Vec<NodeId>> = match parent_node {
                            BetaNode::Root { children }
                            | BetaNode::Join { children, .. }
                            | BetaNode::Negative { children, .. }
                            | BetaNode::Ncc { children, .. }
                            | BetaNode::Exists { children, .. } => Some(children),
                            BetaNode::Terminal { .. } | BetaNode::NccPartner { .. } => None,
                        };
                        if let Some(children) = children {
                            prop_assert!(
                                children.contains(&node_id),
                                "parent {parent_id:?} children list does not contain child {node_id:?}"
                            );
                        }
                    }
                }
            }
        }
    }
}
