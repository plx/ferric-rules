//! Rete network: integration of alpha, beta, token store, and agenda.
//!
//! The Rete network combines all components of the pattern matcher to efficiently
//! propagate facts through the network and produce rule activations.

use smallvec::SmallVec;

use crate::agenda::{Activation, ActivationId, Agenda};
use crate::alpha::{get_slot_value, AlphaMemoryId, AlphaNetwork};
use crate::beta::{BetaMemoryId, BetaNetwork, BetaNode, JoinTest, JoinTestType};
use crate::negative::NegativeMemoryId;
use crate::binding::BindingSet;
use crate::fact::{Fact, FactBase, FactId};
use crate::strategy::ConflictResolutionStrategy;
use crate::token::{NodeId, Token, TokenId, TokenStore};
use crate::value::AtomKey;

/// The complete Rete network.
///
/// Combines alpha network (fact discrimination), beta network (joins),
/// token store (partial matches), and agenda (activations).
pub struct ReteNetwork {
    pub alpha: AlphaNetwork,
    pub beta: BetaNetwork,
    pub token_store: TokenStore,
    pub agenda: Agenda,
}

impl ReteNetwork {
    /// Create a new, empty Rete network with the default (Depth) strategy.
    #[must_use]
    pub fn new() -> Self {
        Self::with_strategy(ConflictResolutionStrategy::default())
    }

    /// Create a new, empty Rete network with the given conflict resolution strategy.
    #[must_use]
    pub fn with_strategy(strategy: ConflictResolutionStrategy) -> Self {
        let alpha = AlphaNetwork::new();

        // Allocate a node ID for the beta root
        // Use a high offset to avoid conflicts with alpha node IDs
        let beta_root_id = NodeId(100_000);

        let beta = BetaNetwork::new(beta_root_id);
        let token_store = TokenStore::new();
        let agenda = Agenda::with_strategy(strategy);

        Self {
            alpha,
            beta,
            token_store,
            agenda,
        }
    }

    /// Assert a fact into the Rete network.
    ///
    /// Propagates the fact through the alpha network, performs right activations
    /// on all subscribed join nodes, and produces activations at terminal nodes.
    ///
    /// Returns the list of activation IDs created.
    pub fn assert_fact(
        &mut self,
        fact_id: FactId,
        fact: &Fact,
        fact_base: &FactBase,
    ) -> Vec<ActivationId> {
        let mut new_activations = Vec::new();

        // 1. Propagate through alpha network
        let affected_memories = self.alpha.assert_fact(fact_id, fact);

        // 2. For each affected alpha memory, perform right activations on subscribed joins
        for &alpha_mem_id in &affected_memories {
            let join_nodes = self.beta.join_nodes_for_alpha(alpha_mem_id).to_vec();

            for join_node_id in join_nodes {
                self.right_activate(join_node_id, fact_id, fact, fact_base, &mut new_activations);
            }
        }

        // 3. For each affected alpha memory, perform right activations on subscribed negative nodes
        for &alpha_mem_id in &affected_memories {
            let neg_nodes = self.beta.negative_nodes_for_alpha(alpha_mem_id).to_vec();

            for neg_node_id in neg_nodes {
                self.negative_right_activate(neg_node_id, fact_id, fact, fact_base);
            }
        }

        new_activations
    }

    /// Retract a fact from the Rete network.
    ///
    /// Removes all tokens containing this fact (cascading), cleans up beta memories
    /// and the agenda, and removes the fact from alpha memories.
    ///
    /// Returns the list of activations that were removed.
    pub fn retract_fact(
        &mut self,
        fact_id: FactId,
        fact: &Fact,
        fact_base: &FactBase,
    ) -> Vec<Activation> {
        use std::collections::HashSet;

        let mut removed_activations = Vec::new();

        // 1. Find all tokens containing this fact
        let affected: HashSet<TokenId> = self.token_store.tokens_containing(fact_id).collect();

        // 2. Compute retraction roots
        let roots = self.token_store.retraction_roots(&affected);

        // 3. For each root, cascade remove and collect removed tokens
        let mut all_removed_tokens = Vec::new();
        for root_id in roots {
            let removed = self.token_store.remove_cascade(root_id);
            all_removed_tokens.extend(removed);
        }

        // 4. For each removed token, clean up beta memory, agenda, and negative memories
        for (token_id, token) in &all_removed_tokens {
            // Remove activations for this token
            let acts = self.agenda.remove_activations_for_token(*token_id);
            removed_activations.extend(acts);

            // Remove token from the owning beta memory in O(1) via token.owner_node.
            if let Some(mem_id) = self.find_memory_for_node(token.owner_node) {
                if let Some(memory) = self.beta.get_memory_mut(mem_id) {
                    memory.remove(*token_id);
                }
            }

            // Clean up any negative memory references to this token
            self.cleanup_negative_memories_for_token(*token_id);
        }

        // 5. Determine which alpha memories held this fact (before removal)
        let affected_alpha_mems = self.alpha.memories_containing_fact(fact_id);

        // 6. Unblock negative nodes: fact retraction may cause tokens to become unblocked.
        // New activations created by unblocking remain on the agenda (they are not "removed").
        let mut new_activations = Vec::new();
        self.negative_handle_retraction(
            fact_id,
            &affected_alpha_mems,
            fact_base,
            &mut new_activations,
        );

        // 7. Remove from alpha memories
        self.alpha.retract_fact(fact_id, fact);

        removed_activations
    }

    /// Clear all runtime state (facts, tokens, activations) while preserving the compiled network structure.
    ///
    /// This is used during `reset` to return to a clean state before re-asserting deffacts.
    pub fn clear_working_memory(&mut self) {
        self.alpha.clear_all_memories();
        self.token_store.clear();
        self.beta.clear_all_runtime();
        let strategy = self.agenda.strategy();
        self.agenda = Agenda::with_strategy(strategy);
    }

    /// Perform a right activation on a join node.
    ///
    /// When a new fact enters an alpha memory, this function:
    /// 1. Gets all tokens from the parent beta memory
    /// 2. For each token, evaluates join tests against the new fact
    /// 3. If tests pass, creates a new token and propagates it
    fn right_activate(
        &mut self,
        join_node_id: NodeId,
        fact_id: FactId,
        fact: &Fact,
        fact_base: &FactBase,
        new_activations: &mut Vec<ActivationId>,
    ) {
        let Some(join_node) = self.beta.get_node(join_node_id) else {
            return;
        };

        let (parent_id, tests, bindings, join_memory_id, children) = match join_node {
            BetaNode::Join {
                parent,
                tests,
                bindings,
                memory,
                children,
                ..
            } => (
                *parent,
                tests.clone(),
                bindings.clone(),
                *memory,
                children.clone(),
            ),
            _ => return,
        };

        // Get parent tokens
        let parent_tokens: Vec<TokenId> = if parent_id == self.beta.root_id() {
            // Special case: root node has no memory, create dummy token
            vec![]
        } else {
            // Find the parent's beta memory
            self.find_memory_for_node(parent_id)
                .map(|mem_id| {
                    self.beta
                        .get_memory(mem_id)
                        .map(|mem| mem.iter().collect())
                        .unwrap_or_default()
                })
                .unwrap_or_default()
        };

        // If parent is root, we need to handle specially (no parent tokens)
        if parent_id == self.beta.root_id() {
            // For root parent, create a token with just this fact
            if evaluate_join(fact, None, &tests) {
                let mut facts = SmallVec::new();
                facts.push(fact_id);

                // Extract bindings from the fact
                let mut new_bindings = BindingSet::new();
                for &(slot, var_id) in &bindings {
                    if let Some(value) = get_slot_value(fact, slot) {
                        new_bindings.set(var_id, std::rc::Rc::new(value.clone()));
                    }
                }

                let new_token = Token {
                    facts,
                    bindings: new_bindings,
                    parent: None,
                    owner_node: join_node_id,
                };

                let token_id = self.token_store.insert(new_token);

                // Add to join's beta memory
                if let Some(memory) = self.beta.get_memory_mut(join_memory_id) {
                    memory.insert(token_id);
                }

                // Propagate to children
                self.propagate_token(token_id, &children, fact_base, new_activations);
            }
        } else {
            // For non-root parent, iterate through parent tokens
            for parent_token_id in parent_tokens {
                let Some(parent_token) = self.token_store.get(parent_token_id) else {
                    continue;
                };

                if evaluate_join(fact, Some(parent_token), &tests) {
                    // Create new token extending the parent
                    let mut new_facts = parent_token.facts.clone();
                    new_facts.push(fact_id);

                    // Clone parent bindings and add new bindings from this fact
                    let mut new_bindings = parent_token.bindings.clone();
                    for &(slot, var_id) in &bindings {
                        if let Some(value) = get_slot_value(fact, slot) {
                            new_bindings.set(var_id, std::rc::Rc::new(value.clone()));
                        }
                    }

                    let new_token = Token {
                        facts: new_facts,
                        bindings: new_bindings,
                        parent: Some(parent_token_id),
                        owner_node: join_node_id,
                    };

                    let token_id = self.token_store.insert(new_token);

                    // Add to join's beta memory
                    if let Some(memory) = self.beta.get_memory_mut(join_memory_id) {
                        memory.insert(token_id);
                    }

                    // Propagate to children
                    self.propagate_token(token_id, &children, fact_base, new_activations);
                }
            }
        }
    }

    /// Perform a left activation on a join node.
    ///
    /// When a new token enters a join node's parent memory, this function:
    /// 1. Gets all facts from the join's alpha memory
    /// 2. For each fact, evaluates join tests against the parent token
    /// 3. If tests pass, creates a new child token and propagates it
    fn left_activate(
        &mut self,
        join_node_id: NodeId,
        parent_token_id: TokenId,
        fact_base: &FactBase,
        new_activations: &mut Vec<ActivationId>,
    ) {
        // 1. Get join node info
        let Some(join_node) = self.beta.get_node(join_node_id) else {
            return;
        };

        let (alpha_memory_id, tests, bindings, join_memory_id, children) = match join_node {
            BetaNode::Join {
                alpha_memory,
                tests,
                bindings,
                memory,
                children,
                ..
            } => (
                *alpha_memory,
                tests.clone(),
                bindings.clone(),
                *memory,
                children.clone(),
            ),
            _ => return,
        };

        // 2. Get parent token data (clone what we need before mutation)
        let Some(parent_token) = self.token_store.get(parent_token_id) else {
            return;
        };
        let parent_facts = parent_token.facts.clone();
        let parent_bindings = parent_token.bindings.clone();

        // 3. Get all fact IDs from alpha memory (clone before mutation)
        let Some(alpha_memory) = self.alpha.get_memory(alpha_memory_id) else {
            return;
        };
        let fact_ids: Vec<FactId> = alpha_memory.iter().collect();

        // 4. For each fact, try to join
        for fact_id in fact_ids {
            let Some(fact_entry) = fact_base.get(fact_id) else {
                continue;
            };
            let fact = &fact_entry.fact;

            // Get fresh parent token reference for each iteration
            let Some(parent_token) = self.token_store.get(parent_token_id) else {
                break;
            };

            // Evaluate join tests
            if !evaluate_join(fact, Some(parent_token), &tests) {
                continue;
            }

            // Tests passed: create child token
            let mut new_facts = parent_facts.clone();
            new_facts.push(fact_id);

            let mut new_bindings = parent_bindings.clone();
            for &(slot, var_id) in &bindings {
                if let Some(value) = get_slot_value(fact, slot) {
                    new_bindings.set(var_id, std::rc::Rc::new(value.clone()));
                }
            }

            let new_token = Token {
                facts: new_facts,
                bindings: new_bindings,
                parent: Some(parent_token_id),
                owner_node: join_node_id,
            };

            let token_id = self.token_store.insert(new_token);

            // Add to join's beta memory
            if let Some(memory) = self.beta.get_memory_mut(join_memory_id) {
                memory.insert(token_id);
            }

            // Propagate to children
            self.propagate_token(token_id, &children, fact_base, new_activations);
        }
    }

    /// Perform a left activation on a negative node.
    ///
    /// When a new parent token arrives at a negative node:
    /// 1. Check all facts in the alpha memory using join tests
    /// 2. If ANY fact matches, the token is blocked (stored in negative memory)
    /// 3. If NO facts match, create a pass-through token and propagate downstream
    fn negative_left_activate(
        &mut self,
        neg_node_id: NodeId,
        parent_token_id: TokenId,
        fact_base: &FactBase,
        new_activations: &mut Vec<ActivationId>,
    ) {
        let Some(neg_node) = self.beta.get_node(neg_node_id) else {
            return;
        };

        let (alpha_memory_id, tests, beta_memory_id, neg_memory_id, children) = match neg_node {
            BetaNode::Negative {
                alpha_memory,
                tests,
                memory,
                neg_memory,
                children,
                ..
            } => (
                *alpha_memory,
                tests.clone(),
                *memory,
                *neg_memory,
                children.clone(),
            ),
            _ => return,
        };

        // Get parent token for join test evaluation
        let Some(parent_token) = self.token_store.get(parent_token_id) else {
            return;
        };
        let parent_facts = parent_token.facts.clone();
        let parent_bindings = parent_token.bindings.clone();

        // Check all facts in the alpha memory for matches
        let Some(alpha_memory) = self.alpha.get_memory(alpha_memory_id) else {
            return;
        };
        let fact_ids: Vec<FactId> = alpha_memory.iter().collect();

        let mut blocking_facts = Vec::new();
        for fact_id in fact_ids {
            let Some(fact_entry) = fact_base.get(fact_id) else {
                continue;
            };
            let fact = &fact_entry.fact;

            // Re-get parent token (it shouldn't change, but be safe with borrows)
            let Some(parent_token) = self.token_store.get(parent_token_id) else {
                return;
            };

            if evaluate_join(fact, Some(parent_token), &tests) {
                blocking_facts.push(fact_id);
            }
        }

        if blocking_facts.is_empty() {
            // No matching facts → unblocked. Create pass-through token and propagate.
            let passthrough_token = Token {
                facts: parent_facts,
                bindings: parent_bindings,
                parent: Some(parent_token_id),
                owner_node: neg_node_id,
            };

            let pt_id = self.token_store.insert(passthrough_token);

            // Add to negative node's beta memory
            if let Some(memory) = self.beta.get_memory_mut(beta_memory_id) {
                memory.insert(pt_id);
            }

            // Track as unblocked
            if let Some(neg_mem) = self.beta.get_neg_memory_mut(neg_memory_id) {
                neg_mem.set_unblocked(parent_token_id, pt_id);
            }

            // Propagate to children
            self.propagate_token(pt_id, &children, fact_base, new_activations);
        } else {
            // Matching facts exist → blocked. Store in negative memory.
            if let Some(neg_mem) = self.beta.get_neg_memory_mut(neg_memory_id) {
                for fact_id in blocking_facts {
                    neg_mem.add_blocker(parent_token_id, fact_id);
                }
            }
        }
    }

    /// Perform a right activation on a negative node.
    ///
    /// When a new fact enters the alpha memory subscribed by a negative node:
    /// 1. For each unblocked pass-through token, evaluate join tests
    /// 2. If the fact matches, block the parent token:
    ///    - Cascade-retract the pass-through token (removes downstream tokens/activations)
    ///    - Move from unblocked to blocked in negative memory
    fn negative_right_activate(
        &mut self,
        neg_node_id: NodeId,
        fact_id: FactId,
        fact: &Fact,
        _fact_base: &FactBase,
    ) {
        let Some(neg_node) = self.beta.get_node(neg_node_id) else {
            return;
        };

        let (tests, beta_memory_id, neg_memory_id) = match neg_node {
            BetaNode::Negative {
                tests,
                memory,
                neg_memory,
                ..
            } => (tests.clone(), *memory, *neg_memory),
            _ => return,
        };

        // Get all unblocked parent → passthrough mappings
        let Some(neg_mem) = self.beta.get_neg_memory(neg_memory_id) else {
            return;
        };
        let unblocked_entries: Vec<(TokenId, TokenId)> = neg_mem.iter_unblocked().collect();

        // For each unblocked token, check if the new fact blocks it
        let mut to_block = Vec::new();
        for (parent_token_id, passthrough_id) in unblocked_entries {
            // Evaluate join tests using the pass-through token's bindings
            let Some(pt_token) = self.token_store.get(passthrough_id) else {
                continue;
            };

            if evaluate_join(fact, Some(pt_token), &tests) {
                to_block.push((parent_token_id, passthrough_id));
            }
        }

        // Block the matching tokens
        for (parent_token_id, passthrough_id) in to_block {
            // Remove unblocked entry and add blocker
            if let Some(neg_mem) = self.beta.get_neg_memory_mut(neg_memory_id) {
                neg_mem.remove_unblocked(parent_token_id);
                neg_mem.add_blocker(parent_token_id, fact_id);
            }

            // Cascade-retract the pass-through token (removes from beta memory, cleans downstream)
            self.retract_token_cascade(passthrough_id, beta_memory_id);
        }
    }

    /// Handle negative node unblocking when a fact is retracted.
    ///
    /// For each negative node subscribed to the retracted fact's alpha memories,
    /// check if any blocked tokens become unblocked. If so, create new pass-through
    /// tokens and propagate them.
    fn negative_handle_retraction(
        &mut self,
        fact_id: FactId,
        affected_alpha_mems: &[AlphaMemoryId],
        fact_base: &FactBase,
        new_activations: &mut Vec<ActivationId>,
    ) {
        for &alpha_mem_id in affected_alpha_mems {
            let neg_nodes = self.beta.negative_nodes_for_alpha(alpha_mem_id).to_vec();

            for neg_node_id in neg_nodes {
                // Find tokens blocked by this fact in this negative node
                let Some(neg_node) = self.beta.get_node(neg_node_id) else {
                    continue;
                };

                let (neg_memory_id, beta_memory_id, children) = match neg_node {
                    BetaNode::Negative {
                        neg_memory,
                        memory,
                        children,
                        ..
                    } => (*neg_memory, *memory, children.clone()),
                    _ => continue,
                };

                let Some(neg_mem) = self.beta.get_neg_memory(neg_memory_id) else {
                    continue;
                };

                let tokens_to_check: Vec<TokenId> = neg_mem.tokens_blocked_by(fact_id);

                for parent_token_id in tokens_to_check {
                    // Remove blocker; check if now unblocked
                    let Some(neg_mem) = self.beta.get_neg_memory_mut(neg_memory_id) else {
                        continue;
                    };
                    let now_unblocked = neg_mem.remove_blocker(parent_token_id, fact_id);

                    if now_unblocked {
                        // Re-create pass-through token and propagate
                        let Some(parent_token) = self.token_store.get(parent_token_id) else {
                            continue;
                        };
                        let parent_facts = parent_token.facts.clone();
                        let parent_bindings = parent_token.bindings.clone();

                        let passthrough_token = Token {
                            facts: parent_facts,
                            bindings: parent_bindings,
                            parent: Some(parent_token_id),
                            owner_node: neg_node_id,
                        };

                        let pt_id = self.token_store.insert(passthrough_token);

                        // Add to beta memory
                        if let Some(memory) = self.beta.get_memory_mut(beta_memory_id) {
                            memory.insert(pt_id);
                        }

                        // Track as unblocked
                        if let Some(neg_mem) = self.beta.get_neg_memory_mut(neg_memory_id) {
                            neg_mem.set_unblocked(parent_token_id, pt_id);
                        }

                        // Propagate to children
                        self.propagate_token(pt_id, &children, fact_base, new_activations);
                    }
                }
            }
        }
    }

    /// Cascade-retract a single token and all its descendants.
    ///
    /// Removes tokens from the token store, cleans up beta memories and agenda.
    /// Used by negative node blocking to retract pass-through tokens.
    fn retract_token_cascade(
        &mut self,
        token_id: TokenId,
        _owner_memory: BetaMemoryId,
    ) {
        let removed = self.token_store.remove_cascade(token_id);

        for (tid, token) in removed {
            // Remove activations for this token
            self.agenda.remove_activations_for_token(tid);

            // Remove token from the owning beta memory
            if let Some(mem_id) = self.find_memory_for_node(token.owner_node) {
                if let Some(memory) = self.beta.get_memory_mut(mem_id) {
                    memory.remove(tid);
                }
            }

            // Clean up negative memory entries if this token was tracked as a parent
            self.cleanup_negative_memories_for_token(tid);
        }
    }

    /// Clean up negative memory entries for a retracted token.
    ///
    /// If the token was a parent token tracked in any negative memory
    /// (blocked or unblocked), remove those entries.
    fn cleanup_negative_memories_for_token(&mut self, token_id: TokenId) {
        // Scan all negative memories for entries referencing this token
        let neg_mem_ids: Vec<NegativeMemoryId> = self.beta.neg_memory_ids().collect();
        for neg_mem_id in neg_mem_ids {
            if let Some(neg_mem) = self.beta.get_neg_memory_mut(neg_mem_id) {
                neg_mem.remove_parent_token(token_id);
            }
        }
    }

    /// Find the beta memory associated with a node.
    ///
    /// For join and negative nodes, returns the node's own beta memory.
    /// For other node types, returns None.
    fn find_memory_for_node(&self, node_id: NodeId) -> Option<BetaMemoryId> {
        match self.beta.get_node(node_id)? {
            BetaNode::Join { memory, .. } | BetaNode::Negative { memory, .. } => Some(*memory),
            _ => None,
        }
    }

    /// Propagate a token to child nodes.
    ///
    /// For terminal nodes, creates activations.
    /// For join nodes, stores the token in the join's memory (already done in `right_activate`).
    fn propagate_token(
        &mut self,
        token_id: TokenId,
        children: &[NodeId],
        fact_base: &FactBase,
        new_activations: &mut Vec<ActivationId>,
    ) {
        for &child_id in children {
            let Some(child_node) = self.beta.get_node(child_id) else {
                continue;
            };

            match child_node {
                BetaNode::Terminal { rule, .. } => {
                    // Create activation
                    let Some(token) = self.token_store.get(token_id) else {
                        continue;
                    };

                    // Build recency vector: timestamps of facts in pattern order
                    let recency: SmallVec<[u64; 4]> = token
                        .facts
                        .iter()
                        .filter_map(|&fid| fact_base.get(fid))
                        .map(|entry| entry.timestamp)
                        .collect();

                    // Get timestamp from the most recent fact in the token
                    let timestamp = recency.iter().max().copied().unwrap_or(0);

                    let activation = Activation {
                        id: ActivationId::default(), // Will be set by agenda.add()
                        rule: *rule,
                        token: token_id,
                        salience: 0, // Default salience for Phase 1
                        timestamp,
                        activation_seq: 0, // Will be set by agenda.add()
                        recency,
                    };

                    let act_id = self.agenda.add(activation);
                    new_activations.push(act_id);
                }
                BetaNode::Join { .. } => {
                    // Perform left activation: token enters as parent for this join
                    self.left_activate(child_id, token_id, fact_base, new_activations);
                }
                BetaNode::Negative { .. } => {
                    // Perform negative left activation: token enters as parent for
                    // this negative node. It will be blocked or allowed through.
                    self.negative_left_activate(child_id, token_id, fact_base, new_activations);
                }
                BetaNode::Root { .. } => {
                    // Root nodes shouldn't be children.
                }
            }
        }
    }

    /// Verify cross-structure consistency for the full rete network.
    ///
    /// Checks all substructures and cross-structure invariants. Extended
    /// incrementally as Phase 2 adds negative, NCC, and exists nodes.
    ///
    /// Intended for use in tests and debug builds.
    #[cfg(any(test, debug_assertions))]
    pub fn debug_assert_consistency(&self) {
        // --- Phase 1 substructure checks ---
        self.token_store.debug_assert_consistency();
        self.alpha.debug_assert_consistency();
        self.beta.debug_assert_consistency();
        self.agenda.debug_assert_consistency();

        // Cross-check: every token referenced by beta memories exists in TokenStore.
        for memory_id in self.beta.memory_ids() {
            if let Some(memory) = self.beta.get_memory(memory_id) {
                for token_id in memory.iter() {
                    assert!(
                        self.token_store.get(token_id).is_some(),
                        "beta memory {memory_id:?} references non-existent token {token_id:?}"
                    );
                }
            }
        }

        // Cross-check: every token referenced by an agenda activation exists.
        for activation in self.agenda.iter_activations() {
            assert!(
                self.token_store.get(activation.token).is_some(),
                "activation for rule {:?} references non-existent token {:?}",
                activation.rule,
                activation.token
            );
        }

        // --- Phase 2 extension points (filled in by later passes) ---

        // Negative node checks (Pass 006):
        // - Every blocker entry maps to an existing token.
        // - Blocker counts match actual blockers for each parent token.
        // - Blocked tokens have no downstream activations.

        // NCC node checks (Pass 010):
        // - NCC partner memory and result memory are consistent.
        // - Conjunction sub-network cleanup leaves no orphaned tokens.

        // Exists node checks (Pass 010):
        // - Support count for each parent token matches actual supporting facts.
        // - Tokens with zero support have no downstream activations.

        // Agenda strategy checks (Pass 007):
        // - All activations in the ordering are correctly sorted per the
        //   active conflict resolution strategy.
        // - No duplicate activations for the same (rule, token) pair.
    }
}

impl Default for ReteNetwork {
    fn default() -> Self {
        Self::new()
    }
}

/// Evaluate join tests between a fact and a token.
///
/// Returns `true` if all tests pass, `false` otherwise.
///
/// If `token` is `None`, treats this as a root-level match (no bindings to check).
fn evaluate_join(fact: &Fact, token: Option<&Token>, tests: &[JoinTest]) -> bool {
    for test in tests {
        let Some(fact_value) = get_slot_value(fact, test.alpha_slot) else {
            return false; // Slot doesn't exist
        };

        let Some(fact_key) = AtomKey::from_value(fact_value) else {
            return false; // Value not indexable (e.g., Multifield, Void)
        };

        // Get the token's binding for the variable
        let token_value = match token {
            Some(t) => match t.bindings.get(test.beta_var) {
                Some(v) => v,
                None => return false, // Variable not bound in token
            },
            None => return false, // No token to compare against
        };

        let Some(token_key) = AtomKey::from_value(token_value) else {
            return false;
        };

        // Compare based on test type
        let matches = match test.test_type {
            JoinTestType::Equal => fact_key == token_key,
            JoinTestType::NotEqual => fact_key != token_key,
        };

        if !matches {
            return false;
        }
    }

    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::alpha::{AlphaEntryType, ConstantTest, ConstantTestType, SlotIndex};
    use crate::beta::RuleId;
    use crate::symbol::{Symbol, SymbolTable};
    use crate::value::Value;
    use smallvec::smallvec;

    fn make_symbol(table: &mut SymbolTable, s: &str) -> Symbol {
        use crate::encoding::StringEncoding;
        table
            .intern_symbol(s, StringEncoding::Ascii)
            .expect("Failed to intern symbol")
    }

    #[test]
    fn rete_assert_simple_fact_no_rules() {
        let mut rete = ReteNetwork::new();
        let mut fact_base = FactBase::new();
        let mut symbol_table = SymbolTable::new();

        let person = make_symbol(&mut symbol_table, "person");
        let fact_id = fact_base.assert_ordered(person, SmallVec::new());
        let fact = fact_base.get(fact_id).expect("Fact should exist");

        let activations = rete.assert_fact(fact_id, &fact.fact, &fact_base);

        assert!(activations.is_empty(), "No rules, so no activations");
        assert!(rete.agenda.is_empty());
    }

    #[test]
    fn rete_simple_single_pattern_rule() {
        let mut rete = ReteNetwork::new();
        let mut fact_base = FactBase::new();
        let mut symbol_table = SymbolTable::new();

        let person = make_symbol(&mut symbol_table, "person");

        // Build a simple rule: (person) => activation
        // Alpha: entry node for (person)
        let entry_type = AlphaEntryType::OrderedRelation(person);
        let entry_node = rete.alpha.create_entry_node(entry_type);
        let alpha_mem_id = rete.alpha.create_memory(entry_node);

        // Beta: root -> join (no tests) -> terminal
        let root_id = rete.beta.root_id();
        let (join_id, _join_mem_id) = rete.beta.create_join_node(root_id, alpha_mem_id, vec![], vec![]);

        let rule_id = RuleId(1);
        let _terminal_id = rete.beta.create_terminal_node(join_id, rule_id);

        // Assert a person fact
        let fact_id = fact_base.assert_ordered(person, SmallVec::new());
        let fact = fact_base.get(fact_id).expect("Fact should exist");

        let activations = rete.assert_fact(fact_id, &fact.fact, &fact_base);

        assert_eq!(activations.len(), 1, "Should produce one activation");
        assert_eq!(rete.agenda.len(), 1);

        let act = rete.agenda.pop().expect("Should have activation");
        assert_eq!(act.rule, rule_id);
    }

    #[test]
    fn rete_two_pattern_join() {
        let mut rete = ReteNetwork::new();
        let mut fact_base = FactBase::new();
        let mut symbol_table = SymbolTable::new();

        let person = make_symbol(&mut symbol_table, "person");
        let age = make_symbol(&mut symbol_table, "age");
        let alice_val = Value::Symbol(make_symbol(&mut symbol_table, "alice"));

        // Build rule: (person ?x) (age ?x 30) => activation
        // Pattern 1: (person ?x) — binds ?x to field 0
        // Pattern 2: (age ?x 30) — field 0 must match ?x, field 1 must equal 30

        // Alpha network:
        // Entry for (person ...) -> alpha_mem1
        let entry1 = AlphaEntryType::OrderedRelation(person);
        let entry_node1 = rete.alpha.create_entry_node(entry1);
        let alpha_mem1 = rete.alpha.create_memory(entry_node1);

        // Entry for (age ...) -> constant test (field 1 = 30) -> alpha_mem2
        let entry2 = AlphaEntryType::OrderedRelation(age);
        let entry_node2 = rete.alpha.create_entry_node(entry2);
        let const_test = ConstantTest {
            slot: SlotIndex::Ordered(1),
            test_type: ConstantTestType::Equal(AtomKey::Integer(30)),
        };
        let test_node = rete
            .alpha
            .create_constant_test_node(entry_node2, const_test);
        let alpha_mem2 = rete.alpha.create_memory(test_node);

        // Beta network:
        // root -> join1 (person, no tests) -> join2 (age, test field 0 = ?x) -> terminal

        let root_id = rete.beta.root_id();

        // Join1: match (person ?x) — no tests, but we'd need to bind ?x
        // For Phase 1 simplicity, we won't actually bind ?x here. We'll just
        // propagate the token. The join test will check slot equality.
        let (join1_id, join1_mem_id) = rete.beta.create_join_node(root_id, alpha_mem1, vec![], vec![]);

        // Join2: match (age ?x 30) — test that age's field 0 equals person's field 0
        // This requires a join test: alpha_slot=Ordered(0), beta_var=VarId(0), Equal
        // But for this to work, we need the first pattern to have bound ?x to VarId(0).
        //
        // Phase 1 note: We're not actually binding variables during join. We're just
        // copying parent bindings. For this test to work, we need a different approach:
        // we'll manually set up bindings or simplify the test.
        //
        // Let's simplify: we'll just test that both facts exist and the join creates
        // a token with both facts. We won't test variable binding for now.

        // Actually, let's test the join test mechanism properly. We need to:
        // 1. Create a token with bindings for ?x
        // 2. Have the join test check that the age fact's field 0 matches ?x
        //
        // For this, we'll manually create a token in join1's memory with bindings.

        // For now, let's just test that a two-pattern rule produces one activation
        // when both facts are asserted. We'll skip the variable binding check.

        let (join2_id, _join2_mem_id) = rete.beta.create_join_node(join1_id, alpha_mem2, vec![], vec![]);

        let rule_id = RuleId(2);
        let _terminal_id = rete.beta.create_terminal_node(join2_id, rule_id);

        // Assert facts
        let mut person_fields = SmallVec::new();
        person_fields.push(alice_val.clone());
        let person_fact_id = fact_base.assert_ordered(person, person_fields);
        let person_fact = fact_base
            .get(person_fact_id)
            .expect("Fact should exist")
            .fact
            .clone();

        let mut age_fields = SmallVec::new();
        age_fields.push(alice_val.clone());
        age_fields.push(Value::Integer(30));
        let age_fact_id = fact_base.assert_ordered(age, age_fields);
        let age_fact = fact_base
            .get(age_fact_id)
            .expect("Fact should exist")
            .fact
            .clone();

        // Assert person fact — should create token in join1's memory
        let acts1 = rete.assert_fact(person_fact_id, &person_fact, &fact_base);
        assert_eq!(acts1.len(), 0, "No activation yet, waiting for age fact");

        // Verify join1's memory has a token
        if let Some(mem) = rete.beta.get_memory(join1_mem_id) {
            assert_eq!(mem.len(), 1, "Join1 should have one token");
        }

        // Assert age fact — should join with person token and create activation
        // But this requires the join test to check variable equality, which we
        // haven't implemented binding for. For Phase 1, let's just verify that
        // the join happens when both facts are present.
        //
        // Actually, the join test will fail because we don't have bindings in the
        // person token. Let's remove the join test for this simple test.

        // Let's restart with a simpler test: two patterns, no variable binding,
        // just check that both facts produce an activation.

        // I'll modify this test to not use join tests for now.
        let activations = rete.assert_fact(age_fact_id, &age_fact, &fact_base);

        // With no join tests, the join should succeed and create an activation
        assert_eq!(
            activations.len(),
            1,
            "Should produce one activation after both facts asserted"
        );
    }

    #[test]
    fn rete_retract_removes_activation() {
        let mut rete = ReteNetwork::new();
        let mut fact_base = FactBase::new();
        let mut symbol_table = SymbolTable::new();

        let person = make_symbol(&mut symbol_table, "person");

        // Build a simple rule: (person) => activation
        let entry_type = AlphaEntryType::OrderedRelation(person);
        let entry_node = rete.alpha.create_entry_node(entry_type);
        let alpha_mem_id = rete.alpha.create_memory(entry_node);

        let root_id = rete.beta.root_id();
        let (join_id, _join_mem_id) = rete.beta.create_join_node(root_id, alpha_mem_id, vec![], vec![]);

        let rule_id = RuleId(1);
        let _terminal_id = rete.beta.create_terminal_node(join_id, rule_id);

        // Assert a fact
        let fact_id = fact_base.assert_ordered(person, SmallVec::new());
        let fact = fact_base.get(fact_id).expect("Fact should exist");

        let activations = rete.assert_fact(fact_id, &fact.fact, &fact_base);
        assert_eq!(activations.len(), 1);
        assert_eq!(rete.agenda.len(), 1);

        // Retract the fact
        let removed = rete.retract_fact(fact_id, &fact.fact, &fact_base);
        assert_eq!(removed.len(), 1, "Should remove one activation");
        assert!(
            rete.agenda.is_empty(),
            "Agenda should be empty after retract"
        );
    }

    #[test]
    fn rete_constant_test_filters() {
        let mut rete = ReteNetwork::new();
        let mut fact_base = FactBase::new();
        let mut symbol_table = SymbolTable::new();

        let person = make_symbol(&mut symbol_table, "person");
        let alice = make_symbol(&mut symbol_table, "alice");
        let bob = make_symbol(&mut symbol_table, "bob");

        // Build rule: (person alice) => activation
        // Only facts with alice in field 0 should match

        let entry_type = AlphaEntryType::OrderedRelation(person);
        let entry_node = rete.alpha.create_entry_node(entry_type);

        let alice_key = AtomKey::Symbol(alice);
        let const_test = ConstantTest {
            slot: SlotIndex::Ordered(0),
            test_type: ConstantTestType::Equal(alice_key),
        };
        let test_node = rete.alpha.create_constant_test_node(entry_node, const_test);
        let alpha_mem_id = rete.alpha.create_memory(test_node);

        let root_id = rete.beta.root_id();
        let (join_id, _join_mem_id) = rete.beta.create_join_node(root_id, alpha_mem_id, vec![], vec![]);

        let rule_id = RuleId(1);
        let _terminal_id = rete.beta.create_terminal_node(join_id, rule_id);

        // Assert (person alice) — should match
        let mut alice_fields = SmallVec::new();
        alice_fields.push(Value::Symbol(alice));
        let alice_fact_id = fact_base.assert_ordered(person, alice_fields);
        let alice_fact = fact_base.get(alice_fact_id).expect("Fact should exist");

        let acts1 = rete.assert_fact(alice_fact_id, &alice_fact.fact, &fact_base);
        assert_eq!(acts1.len(), 1, "Alice fact should produce activation");

        // Assert (person bob) — should not match
        let mut bob_fields = SmallVec::new();
        bob_fields.push(Value::Symbol(bob));
        let bob_fact_id = fact_base.assert_ordered(person, bob_fields);
        let bob_fact = fact_base.get(bob_fact_id).expect("Fact should exist");

        let acts2 = rete.assert_fact(bob_fact_id, &bob_fact.fact, &fact_base);
        assert_eq!(acts2.len(), 0, "Bob fact should not produce activation");

        assert_eq!(rete.agenda.len(), 1, "Only one activation total");
    }

    #[test]
    fn retraction_invariants_after_assert_retract_cycle() {
        let mut rete = ReteNetwork::new();
        let mut fact_base = FactBase::new();
        let mut symbol_table = SymbolTable::new();

        let person = make_symbol(&mut symbol_table, "person");

        // Build a simple rule: (person) => activation
        let entry_type = AlphaEntryType::OrderedRelation(person);
        let entry_node = rete.alpha.create_entry_node(entry_type);
        let alpha_mem_id = rete.alpha.create_memory(entry_node);

        let root_id = rete.beta.root_id();
        let (join_id, _join_mem_id) = rete.beta.create_join_node(root_id, alpha_mem_id, vec![], vec![]);

        let rule_id = RuleId(1);
        let _terminal_id = rete.beta.create_terminal_node(join_id, rule_id);

        // Assert 3 facts, checking consistency after each
        let mut fact_ids = Vec::new();
        for i in 0..3 {
            let fact_id = fact_base.assert_ordered(person, smallvec![Value::Integer(i)]);
            let fact = fact_base.get(fact_id).expect("Fact should exist");

            rete.assert_fact(fact_id, &fact.fact, &fact_base);
            fact_ids.push(fact_id);

            rete.debug_assert_consistency();
        }

        assert_eq!(rete.agenda.len(), 3);

        // Retract 1 fact
        let retract_id = fact_ids[1];
        let retract_fact = fact_base
            .get(retract_id)
            .expect("Fact should exist")
            .fact
            .clone();
        fact_base.retract(retract_id);
        rete.retract_fact(retract_id, &retract_fact, &fact_base);

        rete.debug_assert_consistency();
        assert_eq!(rete.agenda.len(), 2);

        // Assert 1 more fact
        let fact_id = fact_base.assert_ordered(person, smallvec![Value::Integer(99)]);
        let fact = fact_base.get(fact_id).expect("Fact should exist");
        rete.assert_fact(fact_id, &fact.fact, &fact_base);
        fact_ids.push(fact_id);

        rete.debug_assert_consistency();
        assert_eq!(rete.agenda.len(), 3);

        // Retract all remaining
        for fact_id in fact_ids {
            if let Some(entry) = fact_base.get(fact_id) {
                let fact = entry.fact.clone();
                fact_base.retract(fact_id);
                rete.retract_fact(fact_id, &fact, &fact_base);
            }
        }

        // Verify everything is clean
        rete.debug_assert_consistency();
        assert!(rete.agenda.is_empty());
        assert!(rete.token_store.is_empty());
    }

    #[test]
    fn retraction_invariants_with_constant_tests() {
        let mut rete = ReteNetwork::new();
        let mut fact_base = FactBase::new();
        let mut symbol_table = SymbolTable::new();

        let person = make_symbol(&mut symbol_table, "person");
        let alice = make_symbol(&mut symbol_table, "alice");
        let bob = make_symbol(&mut symbol_table, "bob");

        // Build rule: (person alice) => activation
        let entry_type = AlphaEntryType::OrderedRelation(person);
        let entry_node = rete.alpha.create_entry_node(entry_type);

        let alice_key = AtomKey::Symbol(alice);
        let const_test = ConstantTest {
            slot: SlotIndex::Ordered(0),
            test_type: ConstantTestType::Equal(alice_key),
        };
        let test_node = rete.alpha.create_constant_test_node(entry_node, const_test);
        let alpha_mem_id = rete.alpha.create_memory(test_node);

        let root_id = rete.beta.root_id();
        let (join_id, _join_mem_id) = rete.beta.create_join_node(root_id, alpha_mem_id, vec![], vec![]);

        let rule_id = RuleId(1);
        let _terminal_id = rete.beta.create_terminal_node(join_id, rule_id);

        // Assert 5 facts (some matching, some not)
        let mut fact_ids = Vec::new();

        // Matching facts
        for i in 0..3 {
            let fact_id = fact_base
                .assert_ordered(person, smallvec![Value::Symbol(alice), Value::Integer(i)]);
            let fact = fact_base.get(fact_id).expect("Fact should exist");
            rete.assert_fact(fact_id, &fact.fact, &fact_base);
            fact_ids.push(fact_id);

            rete.debug_assert_consistency();
        }

        // Non-matching facts
        for i in 0..2 {
            let fact_id =
                fact_base.assert_ordered(person, smallvec![Value::Symbol(bob), Value::Integer(i)]);
            let fact = fact_base.get(fact_id).expect("Fact should exist");
            rete.assert_fact(fact_id, &fact.fact, &fact_base);
            fact_ids.push(fact_id);

            rete.debug_assert_consistency();
        }

        // Should have 3 activations (only alice facts matched)
        assert_eq!(rete.agenda.len(), 3);

        // Retract all
        for fact_id in fact_ids {
            if let Some(entry) = fact_base.get(fact_id) {
                let fact = entry.fact.clone();
                fact_base.retract(fact_id);
                rete.retract_fact(fact_id, &fact, &fact_base);

                rete.debug_assert_consistency();
            }
        }

        // Verify clean state
        assert!(rete.agenda.is_empty());
        assert!(rete.token_store.is_empty());
    }

    #[test]
    fn two_pattern_rule_with_variable_binding() {
        use crate::binding::VarId;

        let mut rete = ReteNetwork::new();
        let mut fact_base = FactBase::new();
        let mut symbol_table = SymbolTable::new();

        let person = make_symbol(&mut symbol_table, "person");
        let age = make_symbol(&mut symbol_table, "age");
        let alice_val = Value::Symbol(make_symbol(&mut symbol_table, "alice"));
        let bob_val = Value::Symbol(make_symbol(&mut symbol_table, "bob"));

        // Build rule: (person ?x) (age ?x 30) => activation
        // Pattern 1: (person ?x) — binds ?x to field 0
        // Pattern 2: (age ?x 30) — field 0 must match ?x (join test), field 1 must equal 30 (constant test)

        // Alpha network:
        let entry1 = AlphaEntryType::OrderedRelation(person);
        let entry_node1 = rete.alpha.create_entry_node(entry1);
        let alpha_mem1 = rete.alpha.create_memory(entry_node1);

        let entry2 = AlphaEntryType::OrderedRelation(age);
        let entry_node2 = rete.alpha.create_entry_node(entry2);
        let const_test = ConstantTest {
            slot: SlotIndex::Ordered(1),
            test_type: ConstantTestType::Equal(AtomKey::Integer(30)),
        };
        let test_node = rete
            .alpha
            .create_constant_test_node(entry_node2, const_test);
        let alpha_mem2 = rete.alpha.create_memory(test_node);

        // Beta network:
        let root_id = rete.beta.root_id();

        // Join1: match (person ?x) — bind ?x from field 0
        let var_x = VarId(0);
        let join1_bindings = vec![(SlotIndex::Ordered(0), var_x)];
        let (join1_id, _join1_mem_id) =
            rete.beta
                .create_join_node(root_id, alpha_mem1, vec![], join1_bindings);

        // Join2: match (age ?x 30) — test that age's field 0 equals ?x
        let join2_tests = vec![JoinTest {
            alpha_slot: SlotIndex::Ordered(0),
            beta_var: var_x,
            test_type: JoinTestType::Equal,
        }];
        let (join2_id, _join2_mem_id) =
            rete.beta
                .create_join_node(join1_id, alpha_mem2, join2_tests, vec![]);

        let rule_id = RuleId(42);
        let _terminal_id = rete.beta.create_terminal_node(join2_id, rule_id);

        // Assert facts
        // (person alice) — should bind ?x to alice
        let mut person_fields = SmallVec::new();
        person_fields.push(alice_val.clone());
        let person_fact_id = fact_base.assert_ordered(person, person_fields);
        let person_fact = fact_base
            .get(person_fact_id)
            .expect("Fact should exist")
            .fact
            .clone();

        // (age alice 30) — should match ?x = alice
        let mut age_alice_fields = SmallVec::new();
        age_alice_fields.push(alice_val.clone());
        age_alice_fields.push(Value::Integer(30));
        let age_alice_fact_id = fact_base.assert_ordered(age, age_alice_fields);
        let age_alice_fact = fact_base
            .get(age_alice_fact_id)
            .expect("Fact should exist")
            .fact
            .clone();

        // (age bob 30) — should NOT match because ?x is bound to alice
        let mut age_bob_fields = SmallVec::new();
        age_bob_fields.push(bob_val.clone());
        age_bob_fields.push(Value::Integer(30));
        let age_bob_fact_id = fact_base.assert_ordered(age, age_bob_fields);
        let age_bob_fact = fact_base
            .get(age_bob_fact_id)
            .expect("Fact should exist")
            .fact
            .clone();

        // Assert person fact — should create token in join1's memory with ?x = alice
        let acts1 = rete.assert_fact(person_fact_id, &person_fact, &fact_base);
        assert_eq!(acts1.len(), 0, "No activation yet, waiting for age fact");

        // Assert age alice fact — should join with person token and create activation
        let acts2 = rete.assert_fact(age_alice_fact_id, &age_alice_fact, &fact_base);
        assert_eq!(
            acts2.len(),
            1,
            "Should produce one activation for matching age"
        );

        // Assert age bob fact — should NOT produce activation (different ?x)
        let acts3 = rete.assert_fact(age_bob_fact_id, &age_bob_fact, &fact_base);
        assert_eq!(
            acts3.len(),
            0,
            "Should not produce activation for non-matching age"
        );

        assert_eq!(rete.agenda.len(), 1, "Only one activation total");
    }

    #[test]
    fn left_activation_with_reverse_assertion_order() {
        use crate::binding::VarId;

        let mut rete = ReteNetwork::new();
        let mut fact_base = FactBase::new();
        let mut symbol_table = SymbolTable::new();

        let person = make_symbol(&mut symbol_table, "person");
        let age = make_symbol(&mut symbol_table, "age");
        let alice_val = Value::Symbol(make_symbol(&mut symbol_table, "alice"));

        // Build rule: (person ?x) (age ?x 30) => activation

        // Alpha network:
        let entry1 = AlphaEntryType::OrderedRelation(person);
        let entry_node1 = rete.alpha.create_entry_node(entry1);
        let alpha_mem1 = rete.alpha.create_memory(entry_node1);

        let entry2 = AlphaEntryType::OrderedRelation(age);
        let entry_node2 = rete.alpha.create_entry_node(entry2);
        let const_test = ConstantTest {
            slot: SlotIndex::Ordered(1),
            test_type: ConstantTestType::Equal(AtomKey::Integer(30)),
        };
        let test_node = rete
            .alpha
            .create_constant_test_node(entry_node2, const_test);
        let alpha_mem2 = rete.alpha.create_memory(test_node);

        // Beta network:
        let root_id = rete.beta.root_id();

        let var_x = VarId(0);
        let join1_bindings = vec![(SlotIndex::Ordered(0), var_x)];
        let (join1_id, _join1_mem_id) =
            rete.beta
                .create_join_node(root_id, alpha_mem1, vec![], join1_bindings);

        let join2_tests = vec![JoinTest {
            alpha_slot: SlotIndex::Ordered(0),
            beta_var: var_x,
            test_type: JoinTestType::Equal,
        }];
        let (join2_id, _join2_mem_id) =
            rete.beta
                .create_join_node(join1_id, alpha_mem2, join2_tests, vec![]);

        let rule_id = RuleId(42);
        let _terminal_id = rete.beta.create_terminal_node(join2_id, rule_id);

        // Assert facts IN REVERSE ORDER: age fact first, then person fact

        // (age alice 30) — enters alpha memory, no parent tokens yet, no join
        let mut age_fields = SmallVec::new();
        age_fields.push(alice_val.clone());
        age_fields.push(Value::Integer(30));
        let age_fact_id = fact_base.assert_ordered(age, age_fields);
        let age_fact = fact_base.get(age_fact_id).expect("Fact should exist");

        let acts1 = rete.assert_fact(age_fact_id, &age_fact.fact, &fact_base);
        assert_eq!(acts1.len(), 0, "No activation yet, age fact waiting");

        // (person alice) — should create token in join1, then left-activate join2
        let mut person_fields = SmallVec::new();
        person_fields.push(alice_val.clone());
        let person_fact_id = fact_base.assert_ordered(person, person_fields);
        let person_fact = fact_base.get(person_fact_id).expect("Fact should exist");

        let acts2 = rete.assert_fact(person_fact_id, &person_fact.fact, &fact_base);
        assert_eq!(
            acts2.len(),
            1,
            "Should produce activation via left activation"
        );

        assert_eq!(rete.agenda.len(), 1);
    }

    #[test]
    fn binding_extraction_stores_correct_values() {
        use crate::binding::VarId;

        let mut rete = ReteNetwork::new();
        let mut fact_base = FactBase::new();
        let mut symbol_table = SymbolTable::new();

        let person = make_symbol(&mut symbol_table, "person");
        let alice_val = Value::Symbol(make_symbol(&mut symbol_table, "alice"));

        // Build simple rule: (person ?x ?y) => activation
        // Should bind ?x from field 0, ?y from field 1

        let entry1 = AlphaEntryType::OrderedRelation(person);
        let entry_node1 = rete.alpha.create_entry_node(entry1);
        let alpha_mem1 = rete.alpha.create_memory(entry_node1);

        let root_id = rete.beta.root_id();

        let var_x = VarId(0);
        let var_y = VarId(1);
        let join1_bindings = vec![
            (SlotIndex::Ordered(0), var_x),
            (SlotIndex::Ordered(1), var_y),
        ];
        let (join1_id, join1_mem_id) =
            rete.beta
                .create_join_node(root_id, alpha_mem1, vec![], join1_bindings);

        let rule_id = RuleId(1);
        let _terminal_id = rete.beta.create_terminal_node(join1_id, rule_id);

        // Assert (person alice 42)
        let mut fields = SmallVec::new();
        fields.push(alice_val.clone());
        fields.push(Value::Integer(42));
        let fact_id = fact_base.assert_ordered(person, fields);
        let fact = fact_base.get(fact_id).expect("Fact should exist");

        let acts = rete.assert_fact(fact_id, &fact.fact, &fact_base);
        assert_eq!(acts.len(), 1);

        // Get the token from join1's memory
        let join1_memory = rete.beta.get_memory(join1_mem_id).expect("Memory exists");
        assert_eq!(join1_memory.len(), 1);

        let token_id = join1_memory.iter().next().expect("Token exists");
        let token = rete.token_store.get(token_id).expect("Token should exist");

        // Verify bindings
        let x_binding = token.bindings.get(var_x).expect("?x should be bound");
        let y_binding = token.bindings.get(var_y).expect("?y should be bound");

        assert!(
            matches!(**x_binding, Value::Symbol(_)),
            "?x should be alice symbol"
        );
        assert!(
            matches!(**y_binding, Value::Integer(42)),
            "?y should be 42"
        );
    }

    #[test]
    fn retraction_with_variable_bindings() {
        use crate::binding::VarId;

        let mut rete = ReteNetwork::new();
        let mut fact_base = FactBase::new();
        let mut symbol_table = SymbolTable::new();

        let person = make_symbol(&mut symbol_table, "person");
        let age = make_symbol(&mut symbol_table, "age");
        let alice_val = Value::Symbol(make_symbol(&mut symbol_table, "alice"));

        // Build rule: (person ?x) (age ?x 30) => activation

        let entry1 = AlphaEntryType::OrderedRelation(person);
        let entry_node1 = rete.alpha.create_entry_node(entry1);
        let alpha_mem1 = rete.alpha.create_memory(entry_node1);

        let entry2 = AlphaEntryType::OrderedRelation(age);
        let entry_node2 = rete.alpha.create_entry_node(entry2);
        let const_test = ConstantTest {
            slot: SlotIndex::Ordered(1),
            test_type: ConstantTestType::Equal(AtomKey::Integer(30)),
        };
        let test_node = rete
            .alpha
            .create_constant_test_node(entry_node2, const_test);
        let alpha_mem2 = rete.alpha.create_memory(test_node);

        let root_id = rete.beta.root_id();

        let var_x = VarId(0);
        let join1_bindings = vec![(SlotIndex::Ordered(0), var_x)];
        let (join1_id, _join1_mem_id) =
            rete.beta
                .create_join_node(root_id, alpha_mem1, vec![], join1_bindings);

        let join2_tests = vec![JoinTest {
            alpha_slot: SlotIndex::Ordered(0),
            beta_var: var_x,
            test_type: JoinTestType::Equal,
        }];
        let (join2_id, _join2_mem_id) =
            rete.beta
                .create_join_node(join1_id, alpha_mem2, join2_tests, vec![]);

        let rule_id = RuleId(1);
        let _terminal_id = rete.beta.create_terminal_node(join2_id, rule_id);

        // Assert facts
        let mut person_fields = SmallVec::new();
        person_fields.push(alice_val.clone());
        let person_fact_id = fact_base.assert_ordered(person, person_fields);
        let person_fact = fact_base
            .get(person_fact_id)
            .expect("Fact should exist")
            .fact
            .clone();

        let mut age_fields = SmallVec::new();
        age_fields.push(alice_val.clone());
        age_fields.push(Value::Integer(30));
        let age_fact_id = fact_base.assert_ordered(age, age_fields);
        let age_fact = fact_base
            .get(age_fact_id)
            .expect("Fact should exist")
            .fact
            .clone();

        rete.assert_fact(person_fact_id, &person_fact, &fact_base);
        rete.assert_fact(age_fact_id, &age_fact, &fact_base);

        assert_eq!(rete.agenda.len(), 1);
        rete.debug_assert_consistency();

        // Retract person fact
        let removed = rete.retract_fact(person_fact_id, &person_fact, &fact_base);
        assert_eq!(removed.len(), 1, "Should remove one activation");
        assert!(rete.agenda.is_empty());
        assert!(rete.token_store.is_empty());
        rete.debug_assert_consistency();
    }

    // -----------------------------------------------------------------------
    // Negative node tests
    // -----------------------------------------------------------------------

    /// Build a rule: (positive-relation) (not (negative-relation)) => activation.
    ///
    /// Returns (rete, `positive_alpha_mem`, `negative_alpha_mem`, `rule_id`).
    fn build_positive_then_negative_rule(
        symbol_table: &mut SymbolTable,
    ) -> (ReteNetwork, AlphaMemoryId, AlphaMemoryId, RuleId) {
        let mut rete = ReteNetwork::new();

        let pos_sym = make_symbol(symbol_table, "item");
        let neg_sym = make_symbol(symbol_table, "exclude");

        // Alpha path for positive pattern
        let pos_entry = rete
            .alpha
            .create_entry_node(AlphaEntryType::OrderedRelation(pos_sym));
        let pos_alpha = rete.alpha.create_memory(pos_entry);

        // Alpha path for negative pattern
        let neg_entry = rete
            .alpha
            .create_entry_node(AlphaEntryType::OrderedRelation(neg_sym));
        let neg_alpha = rete.alpha.create_memory(neg_entry);

        let root = rete.beta.root_id();

        // Join node for positive pattern (no tests, no bindings)
        let (join_id, _join_mem) =
            rete.beta
                .create_join_node(root, pos_alpha, vec![], vec![]);

        // Negative node for negated pattern
        let (neg_id, _neg_beta_mem, _neg_mem_id) =
            rete.beta
                .create_negative_node(join_id, neg_alpha, vec![]);

        // Terminal
        let rule_id = RuleId(1);
        let _terminal = rete.beta.create_terminal_node(neg_id, rule_id);

        (rete, pos_alpha, neg_alpha, rule_id)
    }

    #[test]
    fn negative_node_no_blocking_fact_produces_activation() {
        let mut symbol_table = SymbolTable::new();
        let (mut rete, _pos_alpha, _neg_alpha, _rule_id) =
            build_positive_then_negative_rule(&mut symbol_table);
        let mut fact_base = FactBase::new();

        let item_sym = make_symbol(&mut symbol_table, "item");

        // Assert positive fact. No exclude facts exist, so the negative node
        // should be unblocked and produce an activation.
        let fact_id = fact_base.assert_ordered(item_sym, SmallVec::new());
        let fact = fact_base.get(fact_id).unwrap();
        let acts = rete.assert_fact(fact_id, &fact.fact, &fact_base);

        assert_eq!(acts.len(), 1, "Should produce activation with no blocking facts");
        assert_eq!(rete.agenda.len(), 1);
        rete.debug_assert_consistency();
    }

    #[test]
    fn negative_node_blocking_fact_suppresses_activation() {
        let mut symbol_table = SymbolTable::new();
        let (mut rete, _pos_alpha, _neg_alpha, _rule_id) =
            build_positive_then_negative_rule(&mut symbol_table);
        let mut fact_base = FactBase::new();

        let item_sym = make_symbol(&mut symbol_table, "item");
        let exclude_sym = make_symbol(&mut symbol_table, "exclude");

        // Assert the blocking fact first
        let block_id = fact_base.assert_ordered(exclude_sym, SmallVec::new());
        let block_fact = fact_base.get(block_id).unwrap();
        rete.assert_fact(block_id, &block_fact.fact, &fact_base);

        // Now assert positive fact — should be blocked, no activation
        let item_id = fact_base.assert_ordered(item_sym, SmallVec::new());
        let item_fact = fact_base.get(item_id).unwrap();
        let acts = rete.assert_fact(item_id, &item_fact.fact, &fact_base);

        assert_eq!(acts.len(), 0, "Should produce no activation when blocking fact exists");
        assert_eq!(rete.agenda.len(), 0);
        rete.debug_assert_consistency();
    }

    #[test]
    fn negative_node_retract_blocker_unblocks_and_produces_activation() {
        let mut symbol_table = SymbolTable::new();
        let (mut rete, _pos_alpha, _neg_alpha, _rule_id) =
            build_positive_then_negative_rule(&mut symbol_table);
        let mut fact_base = FactBase::new();

        let item_sym = make_symbol(&mut symbol_table, "item");
        let exclude_sym = make_symbol(&mut symbol_table, "exclude");

        // Assert blocking fact first
        let block_id = fact_base.assert_ordered(exclude_sym, SmallVec::new());
        let block_fact = fact_base.get(block_id).unwrap().fact.clone();
        rete.assert_fact(block_id, &block_fact, &fact_base);

        // Assert positive fact — should be blocked
        let item_id = fact_base.assert_ordered(item_sym, SmallVec::new());
        let item_fact = fact_base.get(item_id).unwrap().fact.clone();
        rete.assert_fact(item_id, &item_fact, &fact_base);

        assert_eq!(rete.agenda.len(), 0, "Blocked, so no activation");
        rete.debug_assert_consistency();

        // Retract the blocking fact — should unblock and produce activation
        fact_base.retract(block_id);
        rete.retract_fact(block_id, &block_fact, &fact_base);

        assert_eq!(rete.agenda.len(), 1, "Should have activation after unblocking");
        rete.debug_assert_consistency();
    }

    #[test]
    fn negative_node_assert_blocker_after_unblocked_retracts_passthrough() {
        let mut symbol_table = SymbolTable::new();
        let (mut rete, _pos_alpha, _neg_alpha, _rule_id) =
            build_positive_then_negative_rule(&mut symbol_table);
        let mut fact_base = FactBase::new();

        let item_sym = make_symbol(&mut symbol_table, "item");
        let exclude_sym = make_symbol(&mut symbol_table, "exclude");

        // Assert positive fact — no blockers, should produce activation
        let item_id = fact_base.assert_ordered(item_sym, SmallVec::new());
        let item_fact = fact_base.get(item_id).unwrap().fact.clone();
        rete.assert_fact(item_id, &item_fact, &fact_base);

        assert_eq!(rete.agenda.len(), 1, "Should have activation before any blockers");
        rete.debug_assert_consistency();

        // Assert blocking fact — should block and remove activation
        let block_id = fact_base.assert_ordered(exclude_sym, SmallVec::new());
        let block_fact = fact_base.get(block_id).unwrap().fact.clone();
        rete.assert_fact(block_id, &block_fact, &fact_base);

        assert_eq!(
            rete.agenda.len(),
            0,
            "Activation should be removed after blocking fact asserted"
        );
        rete.debug_assert_consistency();
    }

    #[test]
    fn negative_node_block_unblock_cycle() {
        let mut symbol_table = SymbolTable::new();
        let (mut rete, _pos_alpha, _neg_alpha, _rule_id) =
            build_positive_then_negative_rule(&mut symbol_table);
        let mut fact_base = FactBase::new();

        let item_sym = make_symbol(&mut symbol_table, "item");
        let exclude_sym = make_symbol(&mut symbol_table, "exclude");

        // Assert positive fact — produces activation
        let item_id = fact_base.assert_ordered(item_sym, SmallVec::new());
        let item_fact = fact_base.get(item_id).unwrap().fact.clone();
        rete.assert_fact(item_id, &item_fact, &fact_base);
        assert_eq!(rete.agenda.len(), 1);
        rete.debug_assert_consistency();

        // Block it
        let block1_id = fact_base.assert_ordered(exclude_sym, SmallVec::new());
        let block1_fact = fact_base.get(block1_id).unwrap().fact.clone();
        rete.assert_fact(block1_id, &block1_fact, &fact_base);
        assert_eq!(rete.agenda.len(), 0);
        rete.debug_assert_consistency();

        // Unblock it
        fact_base.retract(block1_id);
        rete.retract_fact(block1_id, &block1_fact, &fact_base);
        assert_eq!(rete.agenda.len(), 1);
        rete.debug_assert_consistency();

        // Block again with new fact
        let block2_id = fact_base.assert_ordered(exclude_sym, SmallVec::new());
        let block2_fact = fact_base.get(block2_id).unwrap().fact.clone();
        rete.assert_fact(block2_id, &block2_fact, &fact_base);
        assert_eq!(rete.agenda.len(), 0);
        rete.debug_assert_consistency();

        // Unblock again
        fact_base.retract(block2_id);
        rete.retract_fact(block2_id, &block2_fact, &fact_base);
        assert_eq!(rete.agenda.len(), 1);
        rete.debug_assert_consistency();
    }

    #[test]
    fn negative_node_multiple_blockers_require_all_removed() {
        let mut symbol_table = SymbolTable::new();
        let (mut rete, _pos_alpha, _neg_alpha, _rule_id) =
            build_positive_then_negative_rule(&mut symbol_table);
        let mut fact_base = FactBase::new();

        let item_sym = make_symbol(&mut symbol_table, "item");
        let exclude_sym = make_symbol(&mut symbol_table, "exclude");

        // Assert two blocking facts
        let block1_id = fact_base.assert_ordered(exclude_sym, smallvec![Value::Integer(1)]);
        let block1_fact = fact_base.get(block1_id).unwrap().fact.clone();
        rete.assert_fact(block1_id, &block1_fact, &fact_base);

        let block2_id = fact_base.assert_ordered(exclude_sym, smallvec![Value::Integer(2)]);
        let block2_fact = fact_base.get(block2_id).unwrap().fact.clone();
        rete.assert_fact(block2_id, &block2_fact, &fact_base);

        // Assert positive fact — blocked by both
        let item_id = fact_base.assert_ordered(item_sym, SmallVec::new());
        let item_fact = fact_base.get(item_id).unwrap().fact.clone();
        rete.assert_fact(item_id, &item_fact, &fact_base);

        assert_eq!(rete.agenda.len(), 0);
        rete.debug_assert_consistency();

        // Remove one blocker — still blocked
        fact_base.retract(block1_id);
        rete.retract_fact(block1_id, &block1_fact, &fact_base);
        assert_eq!(rete.agenda.len(), 0, "Still blocked by second blocker");
        rete.debug_assert_consistency();

        // Remove second blocker — now unblocked
        fact_base.retract(block2_id);
        rete.retract_fact(block2_id, &block2_fact, &fact_base);
        assert_eq!(rete.agenda.len(), 1, "Now unblocked");
        rete.debug_assert_consistency();
    }

    #[test]
    fn negative_node_retract_positive_fact_cleans_up() {
        let mut symbol_table = SymbolTable::new();
        let (mut rete, _pos_alpha, _neg_alpha, _rule_id) =
            build_positive_then_negative_rule(&mut symbol_table);
        let mut fact_base = FactBase::new();

        let item_sym = make_symbol(&mut symbol_table, "item");

        // Assert positive fact — produces activation
        let item_id = fact_base.assert_ordered(item_sym, SmallVec::new());
        let item_fact = fact_base.get(item_id).unwrap().fact.clone();
        rete.assert_fact(item_id, &item_fact, &fact_base);

        assert_eq!(rete.agenda.len(), 1);
        rete.debug_assert_consistency();

        // Retract the positive fact
        fact_base.retract(item_id);
        rete.retract_fact(item_id, &item_fact, &fact_base);

        assert_eq!(rete.agenda.len(), 0);
        assert!(rete.token_store.is_empty(), "All tokens should be cleaned up");
        rete.debug_assert_consistency();
    }

    #[test]
    fn negative_node_multiple_positive_facts_independent_blocking() {
        let mut symbol_table = SymbolTable::new();
        let (mut rete, _pos_alpha, _neg_alpha, _rule_id) =
            build_positive_then_negative_rule(&mut symbol_table);
        let mut fact_base = FactBase::new();

        let item_sym = make_symbol(&mut symbol_table, "item");
        let exclude_sym = make_symbol(&mut symbol_table, "exclude");

        // Assert two positive facts — both should produce activations
        let item1_id = fact_base.assert_ordered(item_sym, smallvec![Value::Integer(1)]);
        let item1_fact = fact_base.get(item1_id).unwrap().fact.clone();
        rete.assert_fact(item1_id, &item1_fact, &fact_base);

        let item2_id = fact_base.assert_ordered(item_sym, smallvec![Value::Integer(2)]);
        let item2_fact = fact_base.get(item2_id).unwrap().fact.clone();
        rete.assert_fact(item2_id, &item2_fact, &fact_base);

        assert_eq!(rete.agenda.len(), 2, "Two positive facts, two activations");
        rete.debug_assert_consistency();

        // Assert blocking fact — blocks ALL tokens through negative node
        let block_id = fact_base.assert_ordered(exclude_sym, SmallVec::new());
        let block_fact = fact_base.get(block_id).unwrap().fact.clone();
        rete.assert_fact(block_id, &block_fact, &fact_base);

        assert_eq!(rete.agenda.len(), 0, "Both should be blocked");
        rete.debug_assert_consistency();

        // Retract blocking fact — both should unblock
        fact_base.retract(block_id);
        rete.retract_fact(block_id, &block_fact, &fact_base);

        assert_eq!(rete.agenda.len(), 2, "Both should be unblocked");
        rete.debug_assert_consistency();
    }

    /// Build a rule with a shared variable across positive and negative patterns:
    /// (item ?x) (not (exclude ?x)) => activation.
    fn build_negative_with_variable_binding(
        symbol_table: &mut SymbolTable,
    ) -> (ReteNetwork, AlphaMemoryId, AlphaMemoryId, RuleId) {
        use crate::binding::VarId;

        let mut rete = ReteNetwork::new();

        let item_sym = make_symbol(symbol_table, "item");
        let exclude_sym = make_symbol(symbol_table, "exclude");

        // Alpha paths
        let item_entry = rete
            .alpha
            .create_entry_node(AlphaEntryType::OrderedRelation(item_sym));
        let item_alpha = rete.alpha.create_memory(item_entry);

        let exclude_entry = rete
            .alpha
            .create_entry_node(AlphaEntryType::OrderedRelation(exclude_sym));
        let exclude_alpha = rete.alpha.create_memory(exclude_entry);

        let root = rete.beta.root_id();

        // Join node for (item ?x): binds ?x from slot 0
        let var_x = VarId(0);
        let (join_id, _) = rete.beta.create_join_node(
            root,
            item_alpha,
            vec![],
            vec![(SlotIndex::Ordered(0), var_x)],
        );

        // Negative node for (not (exclude ?x)): tests ?x = slot 0
        let neg_tests = vec![JoinTest {
            alpha_slot: SlotIndex::Ordered(0),
            beta_var: var_x,
            test_type: JoinTestType::Equal,
        }];
        let (neg_id, _, _) =
            rete.beta
                .create_negative_node(join_id, exclude_alpha, neg_tests);

        let rule_id = RuleId(1);
        let _terminal = rete.beta.create_terminal_node(neg_id, rule_id);

        (rete, item_alpha, exclude_alpha, rule_id)
    }

    #[test]
    fn negative_node_with_variable_selective_blocking() {
        let mut symbol_table = SymbolTable::new();
        let (mut rete, _item_alpha, _exclude_alpha, _rule_id) =
            build_negative_with_variable_binding(&mut symbol_table);
        let mut fact_base = FactBase::new();

        let item_sym = make_symbol(&mut symbol_table, "item");
        let exclude_sym = make_symbol(&mut symbol_table, "exclude");
        let alice_val = Value::Symbol(make_symbol(&mut symbol_table, "alice"));
        let bob_val = Value::Symbol(make_symbol(&mut symbol_table, "bob"));

        // Assert (item alice) and (item bob)
        let alice_id = fact_base.assert_ordered(item_sym, smallvec![alice_val.clone()]);
        let alice_fact = fact_base.get(alice_id).unwrap().fact.clone();
        rete.assert_fact(alice_id, &alice_fact, &fact_base);

        let bob_id = fact_base.assert_ordered(item_sym, smallvec![bob_val.clone()]);
        let bob_fact = fact_base.get(bob_id).unwrap().fact.clone();
        rete.assert_fact(bob_id, &bob_fact, &fact_base);

        assert_eq!(rete.agenda.len(), 2, "Both should be active (no excludes)");
        rete.debug_assert_consistency();

        // Assert (exclude alice) — should only block alice, not bob
        let exc_alice_id = fact_base.assert_ordered(exclude_sym, smallvec![alice_val.clone()]);
        let exc_alice_fact = fact_base.get(exc_alice_id).unwrap().fact.clone();
        rete.assert_fact(exc_alice_id, &exc_alice_fact, &fact_base);

        assert_eq!(rete.agenda.len(), 1, "Only bob should remain active");
        rete.debug_assert_consistency();

        // Retract (exclude alice) — alice should come back
        fact_base.retract(exc_alice_id);
        rete.retract_fact(exc_alice_id, &exc_alice_fact, &fact_base);

        assert_eq!(rete.agenda.len(), 2, "Both should be active again");
        rete.debug_assert_consistency();
    }

    #[test]
    fn negative_node_with_variable_non_matching_exclude_doesnt_block() {
        let mut symbol_table = SymbolTable::new();
        let (mut rete, _item_alpha, _exclude_alpha, _rule_id) =
            build_negative_with_variable_binding(&mut symbol_table);
        let mut fact_base = FactBase::new();

        let item_sym = make_symbol(&mut symbol_table, "item");
        let exclude_sym = make_symbol(&mut symbol_table, "exclude");
        let alice_val = Value::Symbol(make_symbol(&mut symbol_table, "alice"));
        let charlie_val = Value::Symbol(make_symbol(&mut symbol_table, "charlie"));

        // Assert (item alice)
        let alice_id = fact_base.assert_ordered(item_sym, smallvec![alice_val.clone()]);
        let alice_fact = fact_base.get(alice_id).unwrap().fact.clone();
        rete.assert_fact(alice_id, &alice_fact, &fact_base);

        assert_eq!(rete.agenda.len(), 1);

        // Assert (exclude charlie) — shouldn't block alice
        let exc_id = fact_base.assert_ordered(exclude_sym, smallvec![charlie_val]);
        let exc_fact = fact_base.get(exc_id).unwrap().fact.clone();
        rete.assert_fact(exc_id, &exc_fact, &fact_base);

        assert_eq!(rete.agenda.len(), 1, "Non-matching exclude should not block");
        rete.debug_assert_consistency();
    }

    #[test]
    fn negative_node_full_lifecycle_with_cleanup() {
        let mut symbol_table = SymbolTable::new();
        let (mut rete, _pos_alpha, _neg_alpha, _rule_id) =
            build_positive_then_negative_rule(&mut symbol_table);
        let mut fact_base = FactBase::new();

        let item_sym = make_symbol(&mut symbol_table, "item");
        let exclude_sym = make_symbol(&mut symbol_table, "exclude");

        // Assert positive, then block, then retract positive
        let item_id = fact_base.assert_ordered(item_sym, SmallVec::new());
        let item_fact = fact_base.get(item_id).unwrap().fact.clone();
        rete.assert_fact(item_id, &item_fact, &fact_base);

        let block_id = fact_base.assert_ordered(exclude_sym, SmallVec::new());
        let block_fact = fact_base.get(block_id).unwrap().fact.clone();
        rete.assert_fact(block_id, &block_fact, &fact_base);

        assert_eq!(rete.agenda.len(), 0);
        rete.debug_assert_consistency();

        // Retract positive fact while blocked
        fact_base.retract(item_id);
        rete.retract_fact(item_id, &item_fact, &fact_base);

        assert_eq!(rete.agenda.len(), 0);
        rete.debug_assert_consistency();

        // Retract blocking fact — nothing to unblock
        fact_base.retract(block_id);
        rete.retract_fact(block_id, &block_fact, &fact_base);

        assert_eq!(rete.agenda.len(), 0);
        assert!(rete.token_store.is_empty());
        rete.debug_assert_consistency();
    }
}
