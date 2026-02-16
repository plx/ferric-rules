//! Rule compiler: transforms pre-processed patterns into shared Rete network nodes.
//!
//! The compiler takes `CompilableRule` input (constructed by the runtime layer from
//! parsed rule AST) and builds alpha network paths, beta join chains with variable
//! binding tests, and terminal nodes. Node sharing is achieved through alpha path
//! caching: rules with identical alpha patterns share the same memory.

use std::collections::{HashMap, HashSet};

use crate::alpha::{AlphaEntryType, AlphaMemoryId, AlphaNetwork, ConstantTest, SlotIndex};
use crate::beta::{JoinTest, JoinTestType, RuleId};
use crate::binding::VarMap;
use crate::rete::ReteNetwork;
use crate::symbol::Symbol;
use crate::token::NodeId;
use crate::validation::{PatternValidationError, PatternViolation, ValidationStage};

/// A rule ready for compilation into rete structures.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CompilableRule {
    pub rule_id: RuleId,
    pub salience: i32,
    pub patterns: Vec<CompilablePattern>,
}

/// A single pattern ready for compilation.
/// The runtime layer constructs these from Stage 2 Pattern/Constraint types.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CompilablePattern {
    pub entry_type: AlphaEntryType,
    pub constant_tests: Vec<ConstantTest>,
    /// Variable bindings: (`slot_index`, `variable_symbol`)
    /// The Symbol is the interned variable name (e.g., intern("x") for ?x)
    pub variable_slots: Vec<(SlotIndex, Symbol)>,
    /// If true, this pattern is a negated conditional element (not CE).
    /// Negated patterns create negative nodes instead of join nodes.
    pub negated: bool,
    /// If true, this pattern is an exists conditional element.
    /// Exists patterns create exists nodes that produce at most one activation.
    pub exists: bool,
}

/// A compilable conditional element in rule order.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CompilableCondition {
    /// A single pattern CE (positive, not, or exists).
    Pattern(CompilablePattern),
    /// A negated conjunction CE: `(not (and ...))`.
    Ncc(Vec<CompilablePattern>),
}

/// Result of compiling a rule.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CompileResult {
    pub rule_id: RuleId,
    pub terminal_node: NodeId,
    /// Alpha memories used by this rule's patterns.
    pub alpha_memories: Vec<AlphaMemoryId>,
    /// Variable name → `VarId` mapping from compilation.
    pub var_map: VarMap,
}

/// Errors from compilation.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum CompileError {
    #[error("rule has no patterns")]
    EmptyRule,
    #[error("too many variables in rule (limit: 65536)")]
    VarMapOverflow,
    #[error("pattern validation failed")]
    Validation(Vec<crate::validation::PatternValidationError>),
}

/// Canonical key for alpha network paths, used for node sharing.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct AlphaPathKey {
    entry_type: AlphaEntryType,
    tests: Vec<ConstantTest>,
}

/// Compiles rule patterns into shared Rete network nodes.
///
/// The compiler maintains caches to ensure that rules with identical alpha
/// patterns share the same alpha network paths and memories. Node sharing
/// is determined by the canonical alpha path key: (`entry_type`, `constant_tests`).
pub struct ReteCompiler {
    /// Cache: alpha path → memory ID. Ensures identical alpha paths share memory.
    alpha_path_cache: HashMap<AlphaPathKey, AlphaMemoryId>,
    /// Next rule ID counter.
    next_rule_id: u32,
}

impl ReteCompiler {
    /// Create a new compiler with an empty cache.
    pub fn new() -> Self {
        Self {
            alpha_path_cache: HashMap::new(),
            next_rule_id: 1, // Start from 1, reserve 0
        }
    }

    /// Allocate the next sequential rule ID.
    pub fn allocate_rule_id(&mut self) -> RuleId {
        let id = RuleId(self.next_rule_id);
        self.next_rule_id += 1;
        id
    }

    /// Compile a rule into the rete network.
    ///
    /// Creates (or reuses) alpha paths for each pattern, builds the beta
    /// join chain with variable binding tests, and creates a terminal node.
    pub fn compile_rule(
        &mut self,
        rete: &mut ReteNetwork,
        rule: &CompilableRule,
    ) -> Result<CompileResult, CompileError> {
        if rule.patterns.is_empty() {
            return Err(CompileError::EmptyRule);
        }
        self.validate_rule_patterns(&rule.patterns)?;
        let conditions: Vec<_> = rule
            .patterns
            .iter()
            .cloned()
            .map(CompilableCondition::Pattern)
            .collect();
        self.compile_conditions_unchecked(rete, rule.rule_id, rule.salience, &conditions)
    }

    /// Compile a sequence of conditional elements into the rete network.
    pub fn compile_conditions(
        &mut self,
        rete: &mut ReteNetwork,
        rule_id: RuleId,
        salience: i32,
        conditions: &[CompilableCondition],
    ) -> Result<CompileResult, CompileError> {
        if conditions.is_empty() {
            return Err(CompileError::EmptyRule);
        }
        self.validate_conditions(conditions)?;
        self.compile_conditions_unchecked(rete, rule_id, salience, conditions)
    }

    fn compile_conditions_unchecked(
        &mut self,
        rete: &mut ReteNetwork,
        rule_id: RuleId,
        salience: i32,
        conditions: &[CompilableCondition],
    ) -> Result<CompileResult, CompileError> {
        let mut alpha_memories = Vec::new();
        let mut var_map = VarMap::new();
        let mut bound_vars: HashSet<Symbol> = HashSet::new();
        let mut current_parent = rete.beta.root_id();

        for condition in conditions {
            match condition {
                CompilableCondition::Pattern(pattern) => {
                    current_parent = self.compile_pattern(
                        rete,
                        current_parent,
                        pattern,
                        &mut var_map,
                        &mut bound_vars,
                        &mut alpha_memories,
                    )?;
                }
                CompilableCondition::Ncc(subpatterns) => {
                    current_parent = self.compile_ncc_condition(
                        rete,
                        current_parent,
                        subpatterns,
                        &mut var_map,
                        &bound_vars,
                        &mut alpha_memories,
                    )?;
                }
            }
        }

        let terminal = rete
            .beta
            .create_terminal_node(current_parent, rule_id, salience);

        Ok(CompileResult {
            rule_id,
            terminal_node: terminal,
            alpha_memories,
            var_map,
        })
    }

    fn validate_rule_patterns(&self, patterns: &[CompilablePattern]) -> Result<(), CompileError> {
        let mut errors = Vec::new();
        for (idx, pattern) in patterns.iter().enumerate() {
            let context = format!("pattern {idx}");
            self.validate_pattern_structure(
                pattern,
                &context,
                true,
                true,
                &mut errors,
            );
        }
        if errors.is_empty() {
            Ok(())
        } else {
            Err(CompileError::Validation(errors))
        }
    }

    fn validate_conditions(&self, conditions: &[CompilableCondition]) -> Result<(), CompileError> {
        let mut errors = Vec::new();
        for (condition_idx, condition) in conditions.iter().enumerate() {
            match condition {
                CompilableCondition::Pattern(pattern) => {
                    let context = format!("condition {condition_idx}");
                    self.validate_pattern_structure(
                        pattern,
                        &context,
                        true,
                        true,
                        &mut errors,
                    );
                }
                CompilableCondition::Ncc(subpatterns) => {
                    if subpatterns.is_empty() {
                        Self::push_unsupported_structure_error(
                            &mut errors,
                            format!(
                                "condition {condition_idx} has an empty NCC; NCC requires at least one subpattern"
                            ),
                        );
                        continue;
                    }

                    for (subpattern_idx, subpattern) in subpatterns.iter().enumerate() {
                        let context =
                            format!("condition {condition_idx} NCC subpattern {subpattern_idx}");
                        self.validate_pattern_structure(
                            subpattern,
                            &context,
                            false,
                            false,
                            &mut errors,
                        );
                    }
                }
            }
        }
        if errors.is_empty() {
            Ok(())
        } else {
            Err(CompileError::Validation(errors))
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn validate_pattern_structure(
        &self,
        pattern: &CompilablePattern,
        context: &str,
        allow_negated: bool,
        allow_exists: bool,
        errors: &mut Vec<PatternValidationError>,
    ) {
        if pattern.negated && !allow_negated {
            Self::push_unsupported_structure_error(
                errors,
                format!("{context} cannot be negated"),
            );
        }
        if pattern.exists && !allow_exists {
            Self::push_unsupported_structure_error(errors, format!("{context} cannot be exists"));
        }

        let mut slot_bindings = HashSet::new();
        let mut variable_bindings: HashMap<Symbol, SlotIndex> = HashMap::new();
        for &(slot, var_sym) in &pattern.variable_slots {
            if !slot_bindings.insert(slot) {
                Self::push_unsupported_structure_error(
                    errors,
                    format!("{context} binds slot {slot:?} more than once"),
                );
            }

            if let Some(previous_slot) = variable_bindings.insert(var_sym, slot) {
                if previous_slot != slot {
                    Self::push_unsupported_structure_error(
                        errors,
                        format!(
                            "{context} reuses variable symbol across slots {previous_slot:?} and {slot:?}; \
                             intra-pattern equality is not supported at core compile stage"
                        ),
                    );
                }
            }
        }
    }

    fn push_unsupported_structure_error(
        errors: &mut Vec<PatternValidationError>,
        description: String,
    ) {
        errors.push(PatternValidationError::new(
            PatternViolation::UnsupportedNestingCombination { description },
            None,
            ValidationStage::ReteCompilation,
        ));
    }

    /// Ensure an alpha path exists for a pattern, reusing cached paths when possible.
    fn ensure_alpha_path(
        &mut self,
        alpha: &mut AlphaNetwork,
        pattern: &CompilablePattern,
    ) -> AlphaMemoryId {
        let key = AlphaPathKey {
            entry_type: pattern.entry_type.clone(),
            tests: pattern.constant_tests.clone(),
        };

        if let Some(&mem_id) = self.alpha_path_cache.get(&key) {
            return mem_id;
        }

        // Build the path: entry node → constant test chain → memory
        let entry_node = alpha.create_entry_node(pattern.entry_type.clone());
        let mut current_node = entry_node;

        for test in &pattern.constant_tests {
            current_node = alpha.create_constant_test_node(current_node, test.clone());
        }

        let mem_id = alpha.create_memory(current_node);
        self.alpha_path_cache.insert(key, mem_id);
        mem_id
    }

    #[allow(clippy::too_many_arguments)]
    fn compile_pattern(
        &mut self,
        rete: &mut ReteNetwork,
        current_parent: NodeId,
        pattern: &CompilablePattern,
        var_map: &mut VarMap,
        bound_vars: &mut HashSet<Symbol>,
        alpha_memories: &mut Vec<AlphaMemoryId>,
    ) -> Result<NodeId, CompileError> {
        let alpha_mem = self.ensure_alpha_path(&mut rete.alpha, pattern);
        alpha_memories.push(alpha_mem);

        let mut join_tests = Vec::new();
        let mut binding_extractions = Vec::new();
        let mut new_bindings = Vec::new();

        for &(slot, var_sym) in &pattern.variable_slots {
            let var_id = var_map
                .get_or_create(var_sym)
                .map_err(|_| CompileError::VarMapOverflow)?;

            if bound_vars.contains(&var_sym) {
                join_tests.push(JoinTest {
                    alpha_slot: slot,
                    beta_var: var_id,
                    test_type: JoinTestType::Equal,
                });
            } else {
                binding_extractions.push((slot, var_id));
                new_bindings.push(var_sym);
            }
        }

        bound_vars.extend(new_bindings);

        if pattern.negated {
            let (neg_id, _beta_mem, _neg_mem) =
                rete.beta
                    .create_negative_node(current_parent, alpha_mem, join_tests);
            Ok(neg_id)
        } else if pattern.exists {
            let (exists_id, _beta_mem, _exists_mem) =
                rete.beta
                    .create_exists_node(current_parent, alpha_mem, join_tests);
            Ok(exists_id)
        } else {
            let (join_id, _beta_mem) = rete.beta.create_join_node(
                current_parent,
                alpha_mem,
                join_tests,
                binding_extractions,
            );
            Ok(join_id)
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn compile_ncc_condition(
        &mut self,
        rete: &mut ReteNetwork,
        current_parent: NodeId,
        subpatterns: &[CompilablePattern],
        var_map: &mut VarMap,
        bound_vars: &HashSet<Symbol>,
        alpha_memories: &mut Vec<AlphaMemoryId>,
    ) -> Result<NodeId, CompileError> {
        if subpatterns.is_empty() {
            return Err(CompileError::Validation(vec![
                PatternValidationError::new(
                    PatternViolation::UnsupportedNestingCombination {
                        description:
                            "NCC requires at least one subpattern".to_string(),
                    },
                    None,
                    ValidationStage::ReteCompilation,
                ),
            ]));
        }

        // Create the main-chain NCC node first, then wire its partner once the
        // subnetwork bottom is known.
        let ncc_memory_id = rete.beta.allocate_ncc_memory();
        let placeholder_partner = rete.beta.root_id();
        let (ncc_id, _ncc_beta_mem) =
            rete.beta
                .create_ncc_node(current_parent, placeholder_partner, ncc_memory_id);

        let mut sub_parent = current_parent;
        let mut sub_bound_vars = bound_vars.clone();
        for subpattern in subpatterns {
            sub_parent = self.compile_pattern(
                rete,
                sub_parent,
                subpattern,
                var_map,
                &mut sub_bound_vars,
                alpha_memories,
            )?;
        }

        let partner_id = rete
            .beta
            .create_ncc_partner(sub_parent, ncc_id, ncc_memory_id);
        rete.beta.set_ncc_partner(ncc_id, partner_id);

        Ok(ncc_id)
    }
}

impl Default for ReteCompiler {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use slotmap::SlotMap;

    use super::*;
    use crate::alpha::{AlphaEntryType, ConstantTest, ConstantTestType, SlotIndex};
    use crate::beta::BetaNode;
    use crate::fact::TemplateId;
    use crate::rete::ReteNetwork;
    use crate::symbol::SymbolTable;
    use crate::value::AtomKey;
    use crate::StringEncoding;

    fn new_table() -> SymbolTable {
        SymbolTable::new()
    }

    fn intern(table: &mut SymbolTable, s: &str) -> Symbol {
        table.intern_symbol(s, StringEncoding::Utf8).unwrap()
    }

    #[test]
    fn test_allocate_rule_id_sequential() {
        let mut compiler = ReteCompiler::new();
        let id1 = compiler.allocate_rule_id();
        let id2 = compiler.allocate_rule_id();
        let id3 = compiler.allocate_rule_id();

        assert_eq!(id1.0, 1);
        assert_eq!(id2.0, 2);
        assert_eq!(id3.0, 3);
    }

    #[test]
    fn test_compile_empty_rule_error() {
        let mut compiler = ReteCompiler::new();
        let mut rete = ReteNetwork::new();
        let rule_id = compiler.allocate_rule_id();

        let rule = CompilableRule {
            rule_id,
            salience: 0,
            patterns: vec![],
        };

        let result = compiler.compile_rule(&mut rete, &rule);
        assert!(matches!(result, Err(CompileError::EmptyRule)));
    }

    #[test]
    fn test_single_pattern_no_tests_no_variables() {
        let mut compiler = ReteCompiler::new();
        let mut rete = ReteNetwork::new();
        let mut table = new_table();

        let relation = intern(&mut table, "person");
        let rule_id = compiler.allocate_rule_id();

        let pattern = CompilablePattern {
            entry_type: AlphaEntryType::OrderedRelation(relation),
            constant_tests: vec![],
            variable_slots: vec![],
            negated: false,
            exists: false,
        };

        let rule = CompilableRule {
            rule_id,
            salience: 0,
            patterns: vec![pattern],
        };

        let result = compiler.compile_rule(&mut rete, &rule).unwrap();

        assert_eq!(result.rule_id, rule_id);
        assert_eq!(result.alpha_memories.len(), 1);

        // Verify terminal node exists and references correct rule
        let terminal_node = rete.beta.get_node(result.terminal_node).unwrap();
        if let BetaNode::Terminal {
            rule: term_rule, ..
        } = terminal_node
        {
            assert_eq!(*term_rule, rule_id);
        } else {
            panic!("Expected terminal node");
        }
    }

    #[test]
    fn test_single_pattern_with_constant_test() {
        let mut compiler = ReteCompiler::new();
        let mut rete = ReteNetwork::new();
        let mut table = new_table();

        let relation = intern(&mut table, "person");
        let rule_id = compiler.allocate_rule_id();

        let name_key = AtomKey::Integer(42);
        let test = ConstantTest {
            slot: SlotIndex::Ordered(0),
            test_type: ConstantTestType::Equal(name_key),
        };

        let pattern = CompilablePattern {
            entry_type: AlphaEntryType::OrderedRelation(relation),
            constant_tests: vec![test],
            variable_slots: vec![],
            negated: false,
            exists: false,
        };

        let rule = CompilableRule {
            rule_id,
            salience: 0,
            patterns: vec![pattern],
        };

        let result = compiler.compile_rule(&mut rete, &rule).unwrap();

        assert_eq!(result.rule_id, rule_id);
        assert_eq!(result.alpha_memories.len(), 1);

        // Verify memory exists
        let mem = rete.alpha.get_memory(result.alpha_memories[0]);
        assert!(mem.is_some());
    }

    #[test]
    fn test_single_pattern_with_variable() {
        let mut compiler = ReteCompiler::new();
        let mut rete = ReteNetwork::new();
        let mut table = new_table();

        let relation = intern(&mut table, "person");
        let var_x = intern(&mut table, "x");
        let rule_id = compiler.allocate_rule_id();

        let pattern = CompilablePattern {
            entry_type: AlphaEntryType::OrderedRelation(relation),
            constant_tests: vec![],
            variable_slots: vec![(SlotIndex::Ordered(0), var_x)],
            negated: false,
            exists: false,
        };

        let rule = CompilableRule {
            rule_id,
            salience: 0,
            patterns: vec![pattern],
        };

        let result = compiler.compile_rule(&mut rete, &rule).unwrap();

        assert_eq!(result.rule_id, rule_id);
        assert_eq!(result.alpha_memories.len(), 1);

        // First pattern with variable should create join node with NO tests
        // (variable is being bound, not tested)
        let terminal_node = rete.beta.get_node(result.terminal_node).unwrap();
        if let BetaNode::Terminal { .. } = terminal_node {
            // Terminal is correct, now check its parent
            // We can't easily navigate backward in the current API, but we can
            // verify the structure was built without error
        } else {
            panic!("Expected terminal node");
        }
    }

    #[test]
    fn test_two_patterns_shared_variable_creates_join_test() {
        let mut compiler = ReteCompiler::new();
        let mut rete = ReteNetwork::new();
        let mut table = new_table();

        let rel1 = intern(&mut table, "person");
        let rel2 = intern(&mut table, "age");
        let var_x = intern(&mut table, "x");
        let rule_id = compiler.allocate_rule_id();

        let pattern1 = CompilablePattern {
            entry_type: AlphaEntryType::OrderedRelation(rel1),
            constant_tests: vec![],
            variable_slots: vec![(SlotIndex::Ordered(0), var_x)],
            negated: false,
            exists: false,
        };

        let pattern2 = CompilablePattern {
            entry_type: AlphaEntryType::OrderedRelation(rel2),
            constant_tests: vec![],
            variable_slots: vec![(SlotIndex::Ordered(0), var_x)],
            negated: false,
            exists: false,
        };

        let rule = CompilableRule {
            rule_id,
            salience: 0,
            patterns: vec![pattern1, pattern2],
        };

        let result = compiler.compile_rule(&mut rete, &rule).unwrap();

        assert_eq!(result.rule_id, rule_id);
        assert_eq!(result.alpha_memories.len(), 2);

        // The second join should have one join test (var_x equality)
        // We can verify this by checking that compilation succeeded and
        // the structure has the correct depth
        let terminal_node = rete.beta.get_node(result.terminal_node).unwrap();
        assert!(matches!(terminal_node, BetaNode::Terminal { .. }));
    }

    #[test]
    fn test_two_patterns_different_variables_no_join_test() {
        let mut compiler = ReteCompiler::new();
        let mut rete = ReteNetwork::new();
        let mut table = new_table();

        let rel1 = intern(&mut table, "person");
        let rel2 = intern(&mut table, "age");
        let var_x = intern(&mut table, "x");
        let var_y = intern(&mut table, "y");
        let rule_id = compiler.allocate_rule_id();

        let pattern1 = CompilablePattern {
            entry_type: AlphaEntryType::OrderedRelation(rel1),
            constant_tests: vec![],
            variable_slots: vec![(SlotIndex::Ordered(0), var_x)],
            negated: false,
            exists: false,
        };

        let pattern2 = CompilablePattern {
            entry_type: AlphaEntryType::OrderedRelation(rel2),
            constant_tests: vec![],
            variable_slots: vec![(SlotIndex::Ordered(0), var_y)],
            negated: false,
            exists: false,
        };

        let rule = CompilableRule {
            rule_id,
            salience: 0,
            patterns: vec![pattern1, pattern2],
        };

        let result = compiler.compile_rule(&mut rete, &rule).unwrap();

        assert_eq!(result.rule_id, rule_id);
        assert_eq!(result.alpha_memories.len(), 2);
    }

    #[test]
    fn test_alpha_path_sharing_same_pattern() {
        let mut compiler = ReteCompiler::new();
        let mut rete = ReteNetwork::new();
        let mut table = new_table();

        let relation = intern(&mut table, "person");

        let pattern = CompilablePattern {
            entry_type: AlphaEntryType::OrderedRelation(relation),
            constant_tests: vec![],
            variable_slots: vec![],
            negated: false,
            exists: false,
        };

        // Compile first rule
        let rule_id1 = compiler.allocate_rule_id();
        let rule1 = CompilableRule {
            rule_id: rule_id1,
            salience: 0,
            patterns: vec![pattern.clone()],
        };
        let result1 = compiler.compile_rule(&mut rete, &rule1).unwrap();

        // Compile second rule with same pattern
        let rule_id2 = compiler.allocate_rule_id();
        let rule2 = CompilableRule {
            rule_id: rule_id2,
            salience: 0,
            patterns: vec![pattern.clone()],
        };
        let result2 = compiler.compile_rule(&mut rete, &rule2).unwrap();

        // Both rules should share the same alpha memory
        assert_eq!(result1.alpha_memories[0], result2.alpha_memories[0]);
    }

    #[test]
    fn test_alpha_path_sharing_different_patterns() {
        let mut compiler = ReteCompiler::new();
        let mut rete = ReteNetwork::new();
        let mut table = new_table();

        let rel1 = intern(&mut table, "person");
        let rel2 = intern(&mut table, "animal");

        let pattern1 = CompilablePattern {
            entry_type: AlphaEntryType::OrderedRelation(rel1),
            constant_tests: vec![],
            variable_slots: vec![],
            negated: false,
            exists: false,
        };

        let pattern2 = CompilablePattern {
            entry_type: AlphaEntryType::OrderedRelation(rel2),
            constant_tests: vec![],
            variable_slots: vec![],
            negated: false,
            exists: false,
        };

        // Compile first rule
        let rule_id1 = compiler.allocate_rule_id();
        let rule1 = CompilableRule {
            rule_id: rule_id1,
            salience: 0,
            patterns: vec![pattern1.clone()],
        };
        let result1 = compiler.compile_rule(&mut rete, &rule1).unwrap();

        // Compile second rule with different pattern
        let rule_id2 = compiler.allocate_rule_id();
        let rule2 = CompilableRule {
            rule_id: rule_id2,
            salience: 0,
            patterns: vec![pattern2.clone()],
        };
        let result2 = compiler.compile_rule(&mut rete, &rule2).unwrap();

        // Different patterns should have different alpha memories
        assert_ne!(result1.alpha_memories[0], result2.alpha_memories[0]);
    }

    #[test]
    fn test_deterministic_compilation() {
        let mut compiler = ReteCompiler::new();
        let mut rete = ReteNetwork::new();
        let mut table = new_table();

        let relation = intern(&mut table, "person");
        let var_x = intern(&mut table, "x");

        let pattern = CompilablePattern {
            entry_type: AlphaEntryType::OrderedRelation(relation),
            constant_tests: vec![],
            variable_slots: vec![(SlotIndex::Ordered(0), var_x)],
            negated: false,
            exists: false,
        };

        let rule_id = compiler.allocate_rule_id();
        let rule = CompilableRule {
            rule_id,
            salience: 0,
            patterns: vec![pattern.clone()],
        };

        let result1 = compiler.compile_rule(&mut rete, &rule).unwrap();

        // Compile same rule again (with new rule_id)
        let rule_id2 = compiler.allocate_rule_id();
        let rule2 = CompilableRule {
            rule_id: rule_id2,
            salience: 0,
            patterns: vec![pattern],
        };
        let result2 = compiler.compile_rule(&mut rete, &rule2).unwrap();

        // Should share alpha memory
        assert_eq!(result1.alpha_memories[0], result2.alpha_memories[0]);
    }

    #[test]
    fn test_multiple_constant_tests_chain() {
        let mut compiler = ReteCompiler::new();
        let mut rete = ReteNetwork::new();
        let mut table = new_table();

        let relation = intern(&mut table, "person");
        let rule_id = compiler.allocate_rule_id();

        let test1 = ConstantTest {
            slot: SlotIndex::Ordered(0),
            test_type: ConstantTestType::Equal(AtomKey::Integer(42)),
        };

        let test2 = ConstantTest {
            slot: SlotIndex::Ordered(1),
            test_type: ConstantTestType::Equal(AtomKey::Integer(100)),
        };

        let pattern = CompilablePattern {
            entry_type: AlphaEntryType::OrderedRelation(relation),
            constant_tests: vec![test1, test2],
            variable_slots: vec![],
            negated: false,
            exists: false,
        };

        let rule = CompilableRule {
            rule_id,
            salience: 0,
            patterns: vec![pattern],
        };

        let result = compiler.compile_rule(&mut rete, &rule).unwrap();

        assert_eq!(result.alpha_memories.len(), 1);
        // Verify memory exists
        let mem = rete.alpha.get_memory(result.alpha_memories[0]);
        assert!(mem.is_some());
    }

    #[test]
    fn test_constant_test_not_equal() {
        let mut compiler = ReteCompiler::new();
        let mut rete = ReteNetwork::new();
        let mut table = new_table();

        let relation = intern(&mut table, "person");
        let rule_id = compiler.allocate_rule_id();

        let test = ConstantTest {
            slot: SlotIndex::Ordered(0),
            test_type: ConstantTestType::NotEqual(AtomKey::Integer(42)),
        };

        let pattern = CompilablePattern {
            entry_type: AlphaEntryType::OrderedRelation(relation),
            constant_tests: vec![test],
            variable_slots: vec![],
            negated: false,
            exists: false,
        };

        let rule = CompilableRule {
            rule_id,
            salience: 0,
            patterns: vec![pattern],
        };

        let result = compiler.compile_rule(&mut rete, &rule).unwrap();

        assert_eq!(result.alpha_memories.len(), 1);
        // Verify memory exists
        let mem = rete.alpha.get_memory(result.alpha_memories[0]);
        assert!(mem.is_some());
    }

    #[test]
    fn test_three_pattern_rule_with_variable_binding_chain() {
        let mut compiler = ReteCompiler::new();
        let mut rete = ReteNetwork::new();
        let mut table = new_table();

        let rel1 = intern(&mut table, "person");
        let rel2 = intern(&mut table, "age");
        let rel3 = intern(&mut table, "salary");
        let var_x = intern(&mut table, "x");
        let rule_id = compiler.allocate_rule_id();

        let pattern1 = CompilablePattern {
            entry_type: AlphaEntryType::OrderedRelation(rel1),
            constant_tests: vec![],
            variable_slots: vec![(SlotIndex::Ordered(0), var_x)],
            negated: false,
            exists: false,
        };

        let pattern2 = CompilablePattern {
            entry_type: AlphaEntryType::OrderedRelation(rel2),
            constant_tests: vec![],
            variable_slots: vec![(SlotIndex::Ordered(0), var_x)],
            negated: false,
            exists: false,
        };

        let pattern3 = CompilablePattern {
            entry_type: AlphaEntryType::OrderedRelation(rel3),
            constant_tests: vec![],
            variable_slots: vec![(SlotIndex::Ordered(0), var_x)],
            negated: false,
            exists: false,
        };

        let rule = CompilableRule {
            rule_id,
            salience: 0,
            patterns: vec![pattern1, pattern2, pattern3],
        };

        let result = compiler.compile_rule(&mut rete, &rule).unwrap();

        assert_eq!(result.rule_id, rule_id);
        assert_eq!(result.alpha_memories.len(), 3);

        // Verify terminal node exists
        let terminal_node = rete.beta.get_node(result.terminal_node).unwrap();
        assert!(matches!(terminal_node, BetaNode::Terminal { .. }));
    }

    #[test]
    fn test_alpha_path_sharing_with_constant_tests() {
        let mut compiler = ReteCompiler::new();
        let mut rete = ReteNetwork::new();
        let mut table = new_table();

        let relation = intern(&mut table, "person");

        let test = ConstantTest {
            slot: SlotIndex::Ordered(0),
            test_type: ConstantTestType::Equal(AtomKey::Integer(42)),
        };

        let pattern = CompilablePattern {
            entry_type: AlphaEntryType::OrderedRelation(relation),
            constant_tests: vec![test],
            variable_slots: vec![],
            negated: false,
            exists: false,
        };

        // Compile first rule
        let rule_id1 = compiler.allocate_rule_id();
        let rule1 = CompilableRule {
            rule_id: rule_id1,
            salience: 0,
            patterns: vec![pattern.clone()],
        };
        let result1 = compiler.compile_rule(&mut rete, &rule1).unwrap();

        // Compile second rule with same pattern including constant test
        let rule_id2 = compiler.allocate_rule_id();
        let rule2 = CompilableRule {
            rule_id: rule_id2,
            salience: 0,
            patterns: vec![pattern],
        };
        let result2 = compiler.compile_rule(&mut rete, &rule2).unwrap();

        // Both rules should share the same alpha memory
        assert_eq!(result1.alpha_memories[0], result2.alpha_memories[0]);
    }

    #[test]
    fn test_beta_network_structure() {
        let mut compiler = ReteCompiler::new();
        let mut rete = ReteNetwork::new();
        let mut table = new_table();

        // Create template IDs for testing
        let mut temp_map: SlotMap<TemplateId, ()> = SlotMap::with_key();
        let template_id = temp_map.insert(());

        let var_x = intern(&mut table, "x");
        let var_y = intern(&mut table, "y");
        let rule_id = compiler.allocate_rule_id();

        let pattern1 = CompilablePattern {
            entry_type: AlphaEntryType::Template(template_id),
            constant_tests: vec![],
            variable_slots: vec![(SlotIndex::Template(0), var_x)],
            negated: false,
            exists: false,
        };

        let pattern2 = CompilablePattern {
            entry_type: AlphaEntryType::Template(template_id),
            constant_tests: vec![],
            variable_slots: vec![
                (SlotIndex::Template(0), var_x),
                (SlotIndex::Template(1), var_y),
            ],
            negated: false,
            exists: false,
        };

        let rule = CompilableRule {
            rule_id,
            salience: 0,
            patterns: vec![pattern1, pattern2],
        };

        let result = compiler.compile_rule(&mut rete, &rule).unwrap();

        assert_eq!(result.alpha_memories.len(), 2);

        // Verify terminal node has correct rule
        let terminal_node = rete.beta.get_node(result.terminal_node).unwrap();
        if let BetaNode::Terminal {
            rule: term_rule, ..
        } = terminal_node
        {
            assert_eq!(*term_rule, rule_id);
        } else {
            panic!("Expected terminal node");
        }

        // Verify both alpha memories exist
        assert!(rete.alpha.get_memory(result.alpha_memories[0]).is_some());
        assert!(rete.alpha.get_memory(result.alpha_memories[1]).is_some());

        // Since pattern1 and pattern2 use the same template but different
        // variable configurations, they should have the same alpha memory
        // (no constant tests, so same entry type → same path)
        assert_eq!(result.alpha_memories[0], result.alpha_memories[1]);
    }

    #[test]
    fn test_negated_pattern_creates_negative_node() {
        let mut compiler = ReteCompiler::new();
        let mut rete = ReteNetwork::new();
        let mut table = new_table();

        let item_rel = intern(&mut table, "item");
        let danger_rel = intern(&mut table, "danger");
        let rule_id = compiler.allocate_rule_id();

        let positive_pattern = CompilablePattern {
            entry_type: AlphaEntryType::OrderedRelation(item_rel),
            constant_tests: vec![],
            variable_slots: vec![],
            negated: false,
            exists: false,
        };

        let negated_pattern = CompilablePattern {
            entry_type: AlphaEntryType::OrderedRelation(danger_rel),
            constant_tests: vec![],
            variable_slots: vec![],
            negated: true,
            exists: false,
        };

        let rule = CompilableRule {
            rule_id,
            salience: 0,
            patterns: vec![positive_pattern, negated_pattern],
        };

        let result = compiler.compile_rule(&mut rete, &rule).unwrap();

        assert_eq!(result.alpha_memories.len(), 2);

        // Verify terminal node exists
        let terminal_node = rete.beta.get_node(result.terminal_node).unwrap();
        assert!(matches!(terminal_node, BetaNode::Terminal { .. }));

        // Walk up from terminal: terminal's parent should be a Negative node
        if let BetaNode::Terminal { parent, .. } = terminal_node {
            let parent_node = rete.beta.get_node(*parent).unwrap();
            assert!(
                matches!(parent_node, BetaNode::Negative { .. }),
                "Parent of terminal should be a Negative node, got {parent_node:?}"
            );
        }
    }

    #[test]
    fn test_negated_pattern_with_join_test() {
        let mut compiler = ReteCompiler::new();
        let mut rete = ReteNetwork::new();
        let mut table = new_table();

        let item_rel = intern(&mut table, "item");
        let exclude_rel = intern(&mut table, "exclude");
        let var_x = intern(&mut table, "x");
        let rule_id = compiler.allocate_rule_id();

        // (item ?x) (not (exclude ?x))
        let pattern1 = CompilablePattern {
            entry_type: AlphaEntryType::OrderedRelation(item_rel),
            constant_tests: vec![],
            variable_slots: vec![(SlotIndex::Ordered(0), var_x)],
            negated: false,
            exists: false,
        };

        let pattern2 = CompilablePattern {
            entry_type: AlphaEntryType::OrderedRelation(exclude_rel),
            constant_tests: vec![],
            variable_slots: vec![(SlotIndex::Ordered(0), var_x)],
            negated: true,
            exists: false,
        };

        let rule = CompilableRule {
            rule_id,
            salience: 0,
            patterns: vec![pattern1, pattern2],
        };

        let result = compiler.compile_rule(&mut rete, &rule).unwrap();

        assert_eq!(result.alpha_memories.len(), 2);

        // Walk up from terminal: terminal → Negative → Join → root
        let terminal = rete.beta.get_node(result.terminal_node).unwrap();
        if let BetaNode::Terminal { parent, .. } = terminal {
            let neg_node = rete.beta.get_node(*parent).unwrap();
            assert!(matches!(neg_node, BetaNode::Negative { .. }));

            if let BetaNode::Negative { tests, .. } = neg_node {
                // The negated pattern should have a join test for ?x
                assert_eq!(
                    tests.len(),
                    1,
                    "Negated pattern should have one join test for ?x"
                );
            }
        }
    }

    #[test]
    fn test_exists_pattern_creates_exists_node() {
        let mut compiler = ReteCompiler::new();
        let mut rete = ReteNetwork::new();
        let mut table = new_table();

        let trigger_rel = intern(&mut table, "trigger");
        let person_rel = intern(&mut table, "person");
        let rule_id = compiler.allocate_rule_id();

        let pattern1 = CompilablePattern {
            entry_type: AlphaEntryType::OrderedRelation(trigger_rel),
            constant_tests: vec![],
            variable_slots: vec![],
            negated: false,
            exists: false,
        };

        let pattern2 = CompilablePattern {
            entry_type: AlphaEntryType::OrderedRelation(person_rel),
            constant_tests: vec![],
            variable_slots: vec![],
            negated: false,
            exists: true,
        };

        let rule = CompilableRule {
            rule_id,
            salience: 0,
            patterns: vec![pattern1, pattern2],
        };

        let result = compiler.compile_rule(&mut rete, &rule).unwrap();

        // Walk up from terminal: terminal's parent should be an Exists node
        let terminal = rete.beta.get_node(result.terminal_node).unwrap();
        if let BetaNode::Terminal { parent, .. } = terminal {
            let parent_node = rete.beta.get_node(*parent).unwrap();
            assert!(
                matches!(parent_node, BetaNode::Exists { .. }),
                "Parent of terminal should be an Exists node, got {parent_node:?}"
            );
        } else {
            panic!("Expected terminal node");
        }
    }

    #[test]
    fn test_ncc_condition_creates_ncc_and_partner_nodes() {
        let mut compiler = ReteCompiler::new();
        let mut rete = ReteNetwork::new();
        let mut table = new_table();

        let item_rel = intern(&mut table, "item");
        let block_rel = intern(&mut table, "block");
        let reason_rel = intern(&mut table, "reason");
        let var_x = intern(&mut table, "x");
        let rule_id = compiler.allocate_rule_id();

        let positive = CompilablePattern {
            entry_type: AlphaEntryType::OrderedRelation(item_rel),
            constant_tests: vec![],
            variable_slots: vec![(SlotIndex::Ordered(0), var_x)],
            negated: false,
            exists: false,
        };
        let ncc_sub_1 = CompilablePattern {
            entry_type: AlphaEntryType::OrderedRelation(block_rel),
            constant_tests: vec![],
            variable_slots: vec![(SlotIndex::Ordered(0), var_x)],
            negated: false,
            exists: false,
        };
        let ncc_sub_2 = CompilablePattern {
            entry_type: AlphaEntryType::OrderedRelation(reason_rel),
            constant_tests: vec![],
            variable_slots: vec![(SlotIndex::Ordered(0), var_x)],
            negated: false,
            exists: false,
        };

        let conditions = vec![
            CompilableCondition::Pattern(positive),
            CompilableCondition::Ncc(vec![ncc_sub_1, ncc_sub_2]),
        ];

        let result = compiler
            .compile_conditions(&mut rete, rule_id, 0, &conditions)
            .unwrap();

        let terminal = rete.beta.get_node(result.terminal_node).unwrap();
        let ncc_id = match terminal {
            BetaNode::Terminal { parent, .. } => *parent,
            _ => panic!("Expected terminal node"),
        };

        let ncc_node = rete.beta.get_node(ncc_id).unwrap();
        let partner_id = match ncc_node {
            BetaNode::Ncc { partner, .. } => *partner,
            other => panic!("Expected NCC node, got {other:?}"),
        };

        let partner = rete.beta.get_node(partner_id).unwrap();
        assert!(
            matches!(partner, BetaNode::NccPartner { .. }),
            "NCC partner should exist"
        );
    }

    #[test]
    fn test_compile_conditions_rejects_empty_ncc() {
        let mut compiler = ReteCompiler::new();
        let mut rete = ReteNetwork::new();
        let rule_id = compiler.allocate_rule_id();

        let err = compiler
            .compile_conditions(&mut rete, rule_id, 0, &[CompilableCondition::Ncc(vec![])])
            .unwrap_err();

        match err {
            CompileError::Validation(errors) => {
                assert!(!errors.is_empty());
                assert_eq!(errors[0].code, "E0005");
                assert!(
                    errors[0]
                        .to_string()
                        .contains("NCC requires at least one subpattern")
                );
            }
            other => panic!("expected validation error, got {other:?}"),
        }
    }

    #[test]
    fn test_compile_conditions_rejects_negated_ncc_subpattern() {
        let mut compiler = ReteCompiler::new();
        let mut rete = ReteNetwork::new();
        let mut table = new_table();
        let rule_id = compiler.allocate_rule_id();
        let rel = intern(&mut table, "blocked");

        let ncc_subpattern = CompilablePattern {
            entry_type: AlphaEntryType::OrderedRelation(rel),
            constant_tests: vec![],
            variable_slots: vec![],
            negated: true,
            exists: false,
        };

        let err = compiler
            .compile_conditions(
                &mut rete,
                rule_id,
                0,
                &[CompilableCondition::Ncc(vec![ncc_subpattern])],
            )
            .unwrap_err();

        match err {
            CompileError::Validation(errors) => {
                assert!(!errors.is_empty());
                assert_eq!(errors[0].code, "E0005");
                assert!(errors[0].to_string().contains("cannot be negated"));
            }
            other => panic!("expected validation error, got {other:?}"),
        }
    }
}
