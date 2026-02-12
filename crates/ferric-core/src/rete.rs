//! Rete network: integration of alpha, beta, token store, and agenda.
//!
//! The Rete network combines all components of the pattern matcher to efficiently
//! propagate facts through the network and produce rule activations.

use smallvec::SmallVec;

use crate::agenda::{Activation, ActivationId, Agenda};
use crate::alpha::{get_slot_value, AlphaNetwork};
use crate::beta::{BetaMemoryId, BetaNetwork, BetaNode, JoinTest, JoinTestType};
use crate::binding::BindingSet;
use crate::fact::{Fact, FactBase, FactId};
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
    /// Create a new, empty Rete network.
    #[must_use]
    pub fn new() -> Self {
        let alpha = AlphaNetwork::new();

        // Allocate a node ID for the beta root
        // Use a high offset to avoid conflicts with alpha node IDs
        let beta_root_id = NodeId(100_000);

        let beta = BetaNetwork::new(beta_root_id);
        let token_store = TokenStore::new();
        let agenda = Agenda::new();

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

        new_activations
    }

    /// Retract a fact from the Rete network.
    ///
    /// Removes all tokens containing this fact (cascading), cleans up beta memories
    /// and the agenda, and removes the fact from alpha memories.
    ///
    /// Returns the list of activations that were removed.
    pub fn retract_fact(&mut self, fact_id: FactId, fact: &Fact) -> Vec<Activation> {
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

        // 4. For each removed token, clean up beta memory and agenda
        for (token_id, token) in all_removed_tokens {
            // Remove activations for this token
            let acts = self.agenda.remove_activations_for_token(token_id);
            removed_activations.extend(acts);

            // Remove token from the owning beta memory in O(1) via token.owner_node.
            if let Some(mem_id) = self.find_memory_for_node(token.owner_node) {
                if let Some(memory) = self.beta.get_memory_mut(mem_id) {
                    memory.remove(token_id);
                }
            }
        }

        // 5. Remove from alpha memories
        self.alpha.retract_fact(fact_id, fact);

        removed_activations
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

    /// Find the beta memory associated with a node.
    ///
    /// For join nodes, returns the node's own memory.
    /// For other node types, returns None.
    fn find_memory_for_node(&self, node_id: NodeId) -> Option<BetaMemoryId> {
        match self.beta.get_node(node_id)? {
            BetaNode::Join { memory, .. } => Some(*memory),
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

                    // Get timestamp from the most recent fact in the token
                    let timestamp = token
                        .facts
                        .iter()
                        .filter_map(|&fid| fact_base.get(fid))
                        .map(|entry| entry.timestamp)
                        .max()
                        .unwrap_or(0);

                    let activation = Activation {
                        id: ActivationId::default(), // Will be set by agenda.add()
                        rule: *rule,
                        token: token_id,
                        salience: 0, // Default salience for Phase 1
                        timestamp,
                        activation_seq: 0, // Will be set by agenda.add()
                    };

                    let act_id = self.agenda.add(activation);
                    new_activations.push(act_id);
                }
                BetaNode::Join { .. } => {
                    // Perform left activation: token enters as parent for this join
                    self.left_activate(child_id, token_id, fact_base, new_activations);
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
        let removed = rete.retract_fact(fact_id, &fact.fact);
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
        rete.retract_fact(retract_id, &retract_fact);

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
                rete.retract_fact(fact_id, &fact);
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
                rete.retract_fact(fact_id, &fact);

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
        let removed = rete.retract_fact(person_fact_id, &person_fact);
        assert_eq!(removed.len(), 1, "Should remove one activation");
        assert!(rete.agenda.is_empty());
        assert!(rete.token_store.is_empty());
        rete.debug_assert_consistency();
    }
}
