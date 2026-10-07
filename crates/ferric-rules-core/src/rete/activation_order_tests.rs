//! Activation creation order is separate from agenda depth/breadth ranking.
use crate::{
    AlphaEntryType, AtomKey, CompilableCondition, CompilablePattern, ConflictResolutionStrategy,
    ConstantTest, ConstantTestType, Fact, FactBase, FactId, ReteCompiler, ReteNetwork, RuleId,
    Salience, SlotIndex, StringEncoding, SymbolTable, Value,
};

struct Network {
    rete: ReteNetwork,
    compiler: ReteCompiler,
    facts: FactBase,
    symbols: SymbolTable,
}

impl Network {
    fn new(strategy: ConflictResolutionStrategy) -> Self {
        Self {
            rete: ReteNetwork::with_strategy(strategy),
            compiler: ReteCompiler::new(),
            facts: FactBase::new(),
            symbols: SymbolTable::new(),
        }
    }

    fn pattern(&mut self, relation: &str, bind: bool) -> CompilablePattern {
        let relation = self
            .symbols
            .intern_symbol(relation, StringEncoding::Ascii)
            .unwrap();
        let variable_slots = if bind {
            vec![(
                SlotIndex::Ordered(0),
                self.symbols
                    .intern_symbol("x", StringEncoding::Ascii)
                    .unwrap(),
            )]
        } else {
            vec![]
        };
        CompilablePattern {
            entry_type: AlphaEntryType::OrderedRelation(relation),
            constant_tests: vec![],
            sequence: None,
            variable_slots,
            negated_variable_slots: vec![],
            negated: false,
            exists: false,
        }
    }

    fn install(&mut self, id: u32, patterns: Vec<CompilablePattern>) {
        let conditions: Vec<_> = patterns
            .into_iter()
            .map(CompilableCondition::Pattern)
            .collect();
        self.compiler
            .compile_conditions(
                &mut self.rete,
                &self.facts,
                RuleId(id),
                Salience::DEFAULT,
                &conditions,
            )
            .unwrap();
    }

    fn install_conditions(&mut self, id: u32, conditions: &[CompilableCondition]) {
        self.compiler
            .compile_conditions(
                &mut self.rete,
                &self.facts,
                RuleId(id),
                Salience::DEFAULT,
                conditions,
            )
            .unwrap();
    }

    fn assert_with_activations(&mut self, relation: &str, fields: &[i64]) -> usize {
        let relation = self
            .symbols
            .intern_symbol(relation, StringEncoding::Ascii)
            .unwrap();
        let id = self.facts.assert_ordered(
            relation,
            fields.iter().copied().map(Value::Integer).collect(),
        );
        let created = self
            .rete
            .assert_fact(id, &self.facts.get(id).unwrap().fact, &self.facts)
            .len();
        self.rete.debug_assert_consistency();
        created
    }

    fn assert(&mut self, relation: &str, fields: &[i64]) -> FactId {
        let relation = self
            .symbols
            .intern_symbol(relation, StringEncoding::Ascii)
            .unwrap();
        let id = self.facts.assert_ordered(
            relation,
            fields.iter().copied().map(Value::Integer).collect(),
        );
        self.rete
            .assert_fact(id, &self.facts.get(id).unwrap().fact, &self.facts);
        self.rete.debug_assert_consistency();
        id
    }

    fn retract(&mut self, id: FactId) {
        let entry = self.facts.retract(id).unwrap();
        self.rete.retract_fact(id, &entry.fact, &self.facts);
        self.rete.debug_assert_consistency();
    }

    fn drain(&mut self) -> Vec<(u32, Vec<i64>)> {
        let mut results = Vec::new();
        while let Some(activation) = self.rete.agenda.pop() {
            let fields = self
                .rete
                .token_store
                .collect_all_facts(activation.token)
                .iter()
                .flat_map(|id| {
                    let Fact::Ordered(fact) = &self.facts.get(*id).unwrap().fact else {
                        unreachable!()
                    };
                    fact.fields.iter().filter_map(|value| match value {
                        Value::Integer(value) => Some(*value),
                        _ => None,
                    })
                })
                .collect();
            results.push((activation.rule.0, fields));
        }
        results
    }

    fn rules(&mut self) -> Vec<u32> {
        self.drain().into_iter().map(|(rule, _)| rule).collect()
    }
}

#[test]
fn shared_successors_match_depth_and_breadth_for_each_fact() {
    for strategy in [
        ConflictResolutionStrategy::Depth,
        ConflictResolutionStrategy::Breadth,
    ] {
        let mut network = Network::new(strategy);
        let pattern = network.pattern("item", true);
        for id in 1..=3 {
            network.install(id, vec![pattern.clone()]);
        }
        network.assert("item", &[1]);
        network.assert("item", &[2]);
        let expected = if strategy == ConflictResolutionStrategy::Depth {
            vec![
                (1, vec![2]),
                (2, vec![2]),
                (3, vec![2]),
                (1, vec![1]),
                (2, vec![1]),
                (3, vec![1]),
            ]
        } else {
            vec![
                (3, vec![1]),
                (2, vec![1]),
                (1, vec![1]),
                (3, vec![2]),
                (2, vec![2]),
                (1, vec![2]),
            ]
        };
        assert_eq!(network.drain(), expected);
    }
}

#[test]
fn shared_suffix_retains_parent_recency_order() {
    let mut network = Network::new(ConflictResolutionStrategy::Depth);
    let prefix = network.pattern("p", true);
    let suffix = network.pattern("item", false);
    for id in 1..=2 {
        network.install(id, vec![prefix.clone(), suffix.clone()]);
    }
    network.assert("p", &[1]);
    network.assert("p", &[2]);
    network.assert("item", &[]);
    assert_eq!(
        network.drain(),
        vec![(1, vec![1]), (2, vec![1]), (1, vec![2]), (2, vec![2])]
    );
}

#[test]
fn shared_alpha_paths_keep_each_successor_group_in_creation_order() {
    let mut network = Network::new(ConflictResolutionStrategy::Depth);
    let generic = network.pattern("item", true);
    let mut literal = network.pattern("item", false);
    literal.constant_tests.push(ConstantTest {
        slot: SlotIndex::Ordered(0),
        test_type: ConstantTestType::Equal(AtomKey::Integer(1)),
    });
    for (id, pattern) in [
        (1, generic.clone()),
        (2, literal.clone()),
        (3, generic),
        (4, literal),
    ] {
        network.install(id, vec![pattern]);
    }
    network.assert("item", &[1]);
    assert_eq!(network.rules(), vec![1, 3, 2, 4]);
}

#[test]
fn independently_compiled_or_variants_share_successor_order() {
    let mut network = Network::new(ConflictResolutionStrategy::Depth);
    let a = network.pattern("a", true);
    let b = network.pattern("b", true);
    network.install(1, vec![a.clone()]);
    network.install(2, vec![b]);
    network.install(3, vec![a]);
    network.assert("a", &[1]);
    network.assert("b", &[1]);
    assert_eq!(network.rules(), vec![2, 1, 3]);
}

#[test]
fn positive_and_exists_right_notifications_share_one_chronology() {
    for exists in [[true, false, true], [false, true, false]] {
        let mut network = Network::new(ConflictResolutionStrategy::Depth);
        for ((id, relation), exists) in [(1, "a"), (2, "b"), (3, "c")].into_iter().zip(exists) {
            let prefix = network.pattern(relation, false);
            let mut suffix = network.pattern("item", false);
            suffix.exists = exists;
            network.install(id, vec![prefix, suffix]);
        }
        for relation in ["a", "b", "c", "item"] {
            network.assert(relation, &[]);
        }
        assert_eq!(network.rules(), vec![1, 2, 3]);
    }
}

#[test]
fn reset_root_visitation_is_reversed_exactly_once() {
    for strategy in [
        ConflictResolutionStrategy::Depth,
        ConflictResolutionStrategy::Breadth,
    ] {
        let mut network = Network::new(strategy);
        for id in 1..=3 {
            network.install(id, vec![]);
        }
        network.rete.clear_working_memory();
        let expected = if strategy == ConflictResolutionStrategy::Depth {
            vec![1, 2, 3]
        } else {
            vec![3, 2, 1]
        };
        assert_eq!(network.rules(), expected);
        network.rete.debug_assert_consistency();
    }
}

#[test]
fn reloaded_rule_uses_new_node_chronology_even_when_rule_id_is_reused() {
    let mut network = Network::new(ConflictResolutionStrategy::Depth);
    let item = network.pattern("item", true);
    network.install(1, vec![item.clone()]);
    network.install(2, vec![item.clone()]);
    let first = network.assert("item", &[1]);
    assert_eq!(network.rules(), vec![1, 2]);
    network
        .compiler
        .remove_rules(&mut network.rete, &[RuleId(1)]);
    network.install(1, vec![item]);
    assert_eq!(
        network.rules(),
        vec![1],
        "online install must not replay the surviving rule"
    );
    network.retract(first);
    network.assert("item", &[2]);
    assert_eq!(network.rules(), vec![2, 1]);
}

#[test]
fn unified_right_pass_keeps_prepropagation_positive_parent_capture() {
    let mut network = Network::new(ConflictResolutionStrategy::Depth);
    let item = network.pattern("item", false);
    network.install(1, vec![item.clone(), item]);
    network.assert("item", &[1]);
    assert_eq!(network.drain(), vec![(1, vec![1, 1])]);
    network.assert("item", &[2]);
    assert_eq!(
        network.drain().len(),
        3,
        "the new fact produces precisely the three new ordered pairs"
    );
}

#[test]
fn nested_ncc_waits_for_shared_subnetwork_entry() {
    for strategy in [
        ConflictResolutionStrategy::Depth,
        ConflictResolutionStrategy::Breadth,
    ] {
        let mut network = Network::new(strategy);
        let [a, b, c] = ["a", "b", "c"].map(|relation| network.pattern(relation, false));
        // helper: (a) (b) (c); nested: (not (and (a) (not (and (b) (c))))).
        network.install(1, vec![a.clone(), b.clone(), c.clone()]);
        network.install_conditions(
            2,
            &[CompilableCondition::Ncc(vec![
                CompilableCondition::Pattern(a),
                CompilableCondition::Ncc(vec![
                    CompilableCondition::Pattern(b),
                    CompilableCondition::Pattern(c),
                ]),
            ])],
        );
        network.rete.clear_working_memory();
        network.assert("b", &[]);
        network.assert("c", &[]);
        assert_eq!(network.rules(), vec![2]);

        // The inner subnetwork shares helper's (b) join, so (a) must reach
        // it before the inner NCC decides. Otherwise a transient inner
        // pass-through retracts and recreates the fired outer match.
        assert_eq!(
            network.assert_with_activations("a", &[]),
            1,
            "only helper is activated; nested is neither added nor removed"
        );
        assert_eq!(network.rules(), vec![1]);
    }
}
