//! Integration tests for the full pipeline: parser → loader → engine → Rete → activation.
//!
//! These tests exercise the complete flow from CLIPS source code to rule activations.

#[cfg(test)]
mod tests {
    use ferric_core::beta::RuleId;
    use ferric_core::{
        AlphaEntryType, AtomKey, ConstantTest, ConstantTestType, ReteNetwork, SlotIndex,
        StringEncoding,
    };

    use crate::engine::Engine;
    use crate::EngineConfig;

    #[test]
    fn integration_parse_load_assert_match() {
        // Parse and load a simple fact
        let mut engine = Engine::new(EngineConfig::utf8());
        let source = "(assert (person Alice 25))";

        let result = engine.load_str(source);
        assert!(result.is_ok());
        let load_result = result.unwrap();

        // Should have 1 asserted fact, 0 rules
        assert_eq!(load_result.asserted_facts.len(), 1);
        assert_eq!(load_result.rules.len(), 0);
        assert!(load_result.warnings.is_empty());

        // Build a Rete network to match "person" facts
        let mut rete = ReteNetwork::new();
        let person_sym = engine
            .symbol_table
            .intern_symbol("person", StringEncoding::Utf8)
            .unwrap();

        // Alpha: entry node for (person ...) -> alpha memory
        let entry_type = AlphaEntryType::OrderedRelation(person_sym);
        let entry_node = rete.alpha.create_entry_node(entry_type);
        let alpha_mem_id = rete.alpha.create_memory(entry_node);

        // Beta: root -> join (no tests) -> terminal (rule 1)
        let root_id = rete.beta.root_id();
        let (join_id, _join_mem_id) = rete.beta.create_join_node(root_id, alpha_mem_id, vec![]);
        let rule_id = RuleId(1);
        let _terminal_id = rete.beta.create_terminal_node(join_id, rule_id);

        // Assert the loaded fact into the Rete network
        let fact_id = load_result.asserted_facts[0];
        let fact = engine.fact_base.get(fact_id).expect("Fact should exist");

        let activations = rete.assert_fact(fact_id, &fact.fact, &engine.fact_base);

        // Verify one activation is produced
        assert_eq!(activations.len(), 1);
        assert_eq!(rete.agenda.len(), 1);

        let act = rete.agenda.pop().unwrap();
        assert_eq!(act.rule, rule_id);
    }

    #[test]
    fn integration_retract_removes_activation() {
        // Load a fact
        let mut engine = Engine::new(EngineConfig::utf8());
        let source = "(assert (person Bob 30))";

        let result = engine.load_str(source).unwrap();
        assert_eq!(result.asserted_facts.len(), 1);

        // Build a Rete network matching "person" facts
        let mut rete = ReteNetwork::new();
        let person_sym = engine
            .symbol_table
            .intern_symbol("person", StringEncoding::Utf8)
            .unwrap();

        let entry_type = AlphaEntryType::OrderedRelation(person_sym);
        let entry_node = rete.alpha.create_entry_node(entry_type);
        let alpha_mem_id = rete.alpha.create_memory(entry_node);

        let root_id = rete.beta.root_id();
        let (join_id, _join_mem_id) = rete.beta.create_join_node(root_id, alpha_mem_id, vec![]);
        let rule_id = RuleId(1);
        let _terminal_id = rete.beta.create_terminal_node(join_id, rule_id);

        // Assert the loaded fact into the Rete
        let fact_id = result.asserted_facts[0];
        let fact = engine
            .fact_base
            .get(fact_id)
            .expect("Fact should exist")
            .fact
            .clone();

        let activations = rete.assert_fact(fact_id, &fact, &engine.fact_base);
        assert_eq!(activations.len(), 1);
        assert_eq!(rete.agenda.len(), 1);

        // Retract the fact from both FactBase and Rete
        engine.fact_base.retract(fact_id).expect("Retract failed");
        let removed = rete.retract_fact(fact_id, &fact);

        // Verify the activation is removed
        assert_eq!(removed.len(), 1);
        assert!(rete.agenda.is_empty());

        // Verify no tokens remain for this fact
        let tokens_for_fact: Vec<_> = rete.token_store.tokens_containing(fact_id).collect();
        assert!(tokens_for_fact.is_empty());
    }

    #[test]
    fn integration_multiple_facts_multiple_activations() {
        // Load multiple facts
        let mut engine = Engine::new(EngineConfig::utf8());
        let source = r"
            (assert (person Alice))
            (assert (person Bob))
            (assert (person Carol))
        ";

        let result = engine.load_str(source).unwrap();
        assert_eq!(result.asserted_facts.len(), 3);

        // Build a Rete network matching "person" facts
        let mut rete = ReteNetwork::new();
        let person_sym = engine
            .symbol_table
            .intern_symbol("person", StringEncoding::Utf8)
            .unwrap();

        let entry_type = AlphaEntryType::OrderedRelation(person_sym);
        let entry_node = rete.alpha.create_entry_node(entry_type);
        let alpha_mem_id = rete.alpha.create_memory(entry_node);

        let root_id = rete.beta.root_id();
        let (join_id, _join_mem_id) = rete.beta.create_join_node(root_id, alpha_mem_id, vec![]);
        let rule_id = RuleId(1);
        let _terminal_id = rete.beta.create_terminal_node(join_id, rule_id);

        // Assert each fact into the Rete
        let mut all_activations = Vec::new();
        for &fact_id in &result.asserted_facts {
            let fact = engine.fact_base.get(fact_id).expect("Fact should exist");
            let acts = rete.assert_fact(fact_id, &fact.fact, &engine.fact_base);
            all_activations.extend(acts);
        }

        // Verify 3 activations are produced
        assert_eq!(all_activations.len(), 3);
        assert_eq!(rete.agenda.len(), 3);
    }

    #[test]
    fn integration_constant_test_filters_facts() {
        // Load multiple facts with different values
        let mut engine = Engine::new(EngineConfig::utf8());
        let source = r"
            (assert (color red))
            (assert (color blue))
            (assert (color green))
        ";

        let result = engine.load_str(source).unwrap();
        assert_eq!(result.asserted_facts.len(), 3);

        // Build a Rete network that only matches (color red)
        let mut rete = ReteNetwork::new();
        let color_sym = engine
            .symbol_table
            .intern_symbol("color", StringEncoding::Utf8)
            .unwrap();
        let red_sym = engine
            .symbol_table
            .intern_symbol("red", StringEncoding::Utf8)
            .unwrap();

        // Alpha: entry for (color ...) -> constant test (field 0 = red) -> memory
        let entry_type = AlphaEntryType::OrderedRelation(color_sym);
        let entry_node = rete.alpha.create_entry_node(entry_type);

        let red_test = ConstantTest {
            slot: SlotIndex::Ordered(0),
            test_type: ConstantTestType::Equal(AtomKey::Symbol(red_sym)),
        };
        let test_node = rete.alpha.create_constant_test_node(entry_node, red_test);
        let alpha_mem_id = rete.alpha.create_memory(test_node);

        // Beta: root -> join -> terminal
        let root_id = rete.beta.root_id();
        let (join_id, _join_mem_id) = rete.beta.create_join_node(root_id, alpha_mem_id, vec![]);
        let rule_id = RuleId(1);
        let _terminal_id = rete.beta.create_terminal_node(join_id, rule_id);

        // Assert all facts into the Rete
        let mut all_activations = Vec::new();
        for &fact_id in &result.asserted_facts {
            let fact = engine.fact_base.get(fact_id).expect("Fact should exist");
            let acts = rete.assert_fact(fact_id, &fact.fact, &engine.fact_base);
            all_activations.extend(acts);
        }

        // Verify only 1 activation (for red)
        assert_eq!(all_activations.len(), 1);
        assert_eq!(rete.agenda.len(), 1);
    }

    #[test]
    fn integration_loader_and_rete_roundtrip() {
        // Use loader to parse realistic CLIPS source
        let mut engine = Engine::new(EngineConfig::utf8());
        let source = r"
            (deffacts startup
                (animal dog)
                (animal cat)
                (animal bird))

            (defrule match-animal
                (animal ?x)
                =>
                (printout t ?x crlf))
        ";

        let result = engine.load_str(source).unwrap();

        // Verify the loader produces 3 asserted facts and 1 rule definition
        assert_eq!(result.asserted_facts.len(), 3);
        assert_eq!(result.rules.len(), 1);

        // Build a Rete network matching "animal" facts
        let mut rete = ReteNetwork::new();
        let animal_sym = engine
            .symbol_table
            .intern_symbol("animal", StringEncoding::Utf8)
            .unwrap();

        let entry_type = AlphaEntryType::OrderedRelation(animal_sym);
        let entry_node = rete.alpha.create_entry_node(entry_type);
        let alpha_mem_id = rete.alpha.create_memory(entry_node);

        let root_id = rete.beta.root_id();
        let (join_id, _join_mem_id) = rete.beta.create_join_node(root_id, alpha_mem_id, vec![]);
        let rule_id = RuleId(1);
        let _terminal_id = rete.beta.create_terminal_node(join_id, rule_id);

        // Assert all facts into the Rete
        let mut all_activations = Vec::new();
        for &fact_id in &result.asserted_facts {
            let fact = engine.fact_base.get(fact_id).expect("Fact should exist");
            let acts = rete.assert_fact(fact_id, &fact.fact, &engine.fact_base);
            all_activations.extend(acts);
        }

        // Verify 3 activations
        assert_eq!(all_activations.len(), 3);
        assert_eq!(rete.agenda.len(), 3);

        // Pop one activation and verify agenda has 2 remaining
        let first_act = rete.agenda.pop();
        assert!(first_act.is_some());
        assert_eq!(rete.agenda.len(), 2);

        // Verify the rule definition
        let rule_def = &result.rules[0];
        assert_eq!(rule_def.name, "match-animal");
        assert_eq!(rule_def.lhs.len(), 1);
        assert_eq!(rule_def.rhs.len(), 1);
    }
}
