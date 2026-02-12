//! Beta network and beta memory.
//!
//! The beta network is the second stage of the Rete algorithm. It performs
//! joins between alpha memories (facts) and beta memories (partial matches/tokens).

use std::collections::{HashMap, HashSet};

use crate::alpha::{AlphaMemoryId, SlotIndex};
use crate::binding::VarId;
use crate::negative::{NegativeMemory, NegativeMemoryId};
use crate::token::NodeId;
use crate::token::TokenId;

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
}

/// Unique identifier for a beta memory.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct BetaMemoryId(pub u32);

/// Beta memory: stores tokens (partial matches).
///
/// Each beta memory is associated with a join node or other beta node
/// and holds the tokens that have successfully matched up to that point.
pub struct BetaMemory {
    pub id: BetaMemoryId,
    tokens: HashSet<TokenId>,
}

impl BetaMemory {
    /// Create a new, empty beta memory.
    #[must_use]
    pub fn new(id: BetaMemoryId) -> Self {
        Self {
            id,
            tokens: HashSet::new(),
        }
    }

    /// Insert a token into the memory.
    ///
    /// If the token is already present, this is a no-op.
    pub fn insert(&mut self, token_id: TokenId) {
        self.tokens.insert(token_id);
    }

    /// Remove a token from the memory.
    ///
    /// If the token is not present, this is a no-op.
    pub fn remove(&mut self, token_id: TokenId) {
        self.tokens.remove(&token_id);
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

    /// Clear all tokens from this memory.
    pub fn clear(&mut self) {
        self.tokens.clear();
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
    Terminal { parent: NodeId, rule: RuleId, salience: i32 },
    /// Negative node: blocks parent tokens when a matching fact exists.
    Negative {
        parent: NodeId,
        alpha_memory: AlphaMemoryId,
        tests: Vec<JoinTest>,
        memory: BetaMemoryId,
        neg_memory: NegativeMemoryId,
        children: Vec<NodeId>,
    },
}

/// The beta network.
///
/// Manages beta nodes and beta memories. Coordinates with the alpha network
/// to perform joins and produce activations.
pub struct BetaNetwork {
    nodes: HashMap<NodeId, BetaNode>,
    memories: HashMap<BetaMemoryId, BetaMemory>,
    neg_memories: HashMap<NegativeMemoryId, NegativeMemory>,
    root_id: NodeId,
    next_node_id: u32,
    next_memory_id: u32,
    next_neg_memory_id: u32,
    /// Reverse index: alpha memory -> list of join nodes that subscribe to it.
    alpha_to_joins: HashMap<AlphaMemoryId, Vec<NodeId>>,
    /// Reverse index: alpha memory -> list of negative nodes that subscribe to it.
    alpha_to_negatives: HashMap<AlphaMemoryId, Vec<NodeId>>,
}

impl BetaNetwork {
    /// Create a new beta network with a root node.
    ///
    /// The root node ID is provided by the caller to coordinate with alpha network
    /// node ID allocation.
    #[must_use]
    pub fn new(root_node_id: NodeId) -> Self {
        let mut nodes = HashMap::new();
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
            memories: HashMap::new(),
            neg_memories: HashMap::new(),
            root_id: root_node_id,
            next_node_id,
            next_memory_id: 0,
            next_neg_memory_id: 0,
            alpha_to_joins: HashMap::new(),
            alpha_to_negatives: HashMap::new(),
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
        self.memories.insert(memory_id, BetaMemory::new(memory_id));

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
    pub fn create_terminal_node(&mut self, parent: NodeId, rule: RuleId, salience: i32) -> NodeId {
        let node_id = NodeId(self.next_node_id);
        self.next_node_id += 1;

        let node = BetaNode::Terminal { parent, rule, salience };

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
        self.memories.insert(memory_id, BetaMemory::new(memory_id));
        self.neg_memories
            .insert(neg_memory_id, NegativeMemory::new(neg_memory_id));

        self.attach_child_to_parent(parent, node_id);

        // Register in alpha_to_negatives index
        self.alpha_to_negatives
            .entry(alpha_memory)
            .or_default()
            .push(node_id);

        (node_id, memory_id, neg_memory_id)
    }

    /// Get a beta node by ID.
    #[must_use]
    pub fn get_node(&self, id: NodeId) -> Option<&BetaNode> {
        self.nodes.get(&id)
    }

    /// Get a beta memory by ID.
    #[must_use]
    pub fn get_memory(&self, id: BetaMemoryId) -> Option<&BetaMemory> {
        self.memories.get(&id)
    }

    /// Get a mutable reference to a beta memory by ID.
    pub fn get_memory_mut(&mut self, id: BetaMemoryId) -> Option<&mut BetaMemory> {
        self.memories.get_mut(&id)
    }

    /// Get the root node ID.
    #[must_use]
    pub fn root_id(&self) -> NodeId {
        self.root_id
    }

    /// Get a negative memory by ID.
    #[must_use]
    pub fn get_neg_memory(&self, id: NegativeMemoryId) -> Option<&NegativeMemory> {
        self.neg_memories.get(&id)
    }

    /// Get a mutable reference to a negative memory by ID.
    pub fn get_neg_memory_mut(&mut self, id: NegativeMemoryId) -> Option<&mut NegativeMemory> {
        self.neg_memories.get_mut(&id)
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

    /// Clear all runtime state from beta and negative memories, preserving network structure.
    pub fn clear_all_runtime(&mut self) {
        for memory in self.memories.values_mut() {
            memory.clear();
        }
        for neg_memory in self.neg_memories.values_mut() {
            neg_memory.clear();
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
        self.memories.keys().copied()
    }

    /// Iterate over all negative memory IDs.
    pub fn neg_memory_ids(&self) -> impl Iterator<Item = NegativeMemoryId> + '_ {
        self.neg_memories.keys().copied()
    }

    fn attach_child_to_parent(&mut self, parent: NodeId, child_id: NodeId) {
        if let Some(parent_node) = self.nodes.get_mut(&parent) {
            match parent_node {
                BetaNode::Root { children }
                | BetaNode::Join { children, .. }
                | BetaNode::Negative { children, .. } => {
                    children.push(child_id);
                }
                BetaNode::Terminal { .. } => {
                    // Terminal nodes cannot have children
                }
            }
        }
    }

    /// Verify internal consistency of the beta network.
    ///
    /// This method is gated behind `test` or `debug_assertions` and will panic
    /// if any inconsistencies are detected.
    #[cfg(any(test, debug_assertions))]
    pub fn debug_assert_consistency(&self) {
        // Check 1: All node IDs in children fields exist in nodes map
        for (node_id, node) in &self.nodes {
            let children = match node {
                BetaNode::Root { children }
                | BetaNode::Join { children, .. }
                | BetaNode::Negative { children, .. } => children,
                BetaNode::Terminal { .. } => continue,
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
                | BetaNode::Negative { parent, .. } => *parent,
                BetaNode::Root { .. } => continue,
            };

            assert!(
                self.nodes.contains_key(&parent),
                "Node {node_id:?} has non-existent parent {parent:?}"
            );
        }

        // Check 3: All memory IDs in join/negative nodes exist in memories map
        for (node_id, node) in &self.nodes {
            match node {
                BetaNode::Join { memory, .. } => {
                    assert!(
                        self.memories.contains_key(memory),
                        "Join node {node_id:?} references non-existent memory {memory:?}"
                    );
                }
                BetaNode::Negative {
                    memory, neg_memory, ..
                } => {
                    assert!(
                        self.memories.contains_key(memory),
                        "Negative node {node_id:?} references non-existent beta memory {memory:?}"
                    );
                    assert!(
                        self.neg_memories.contains_key(neg_memory),
                        "Negative node {node_id:?} references non-existent negative memory {neg_memory:?}"
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
        for (neg_mem_id, neg_mem) in &self.neg_memories {
            let _ = neg_mem_id;
            neg_mem.debug_assert_consistency();
        }
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
        let terminal_id = net.create_terminal_node(join_id, rule, 0);

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
            assert_eq!(*salience, 0);
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
