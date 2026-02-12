//! Integration tests for the full pipeline: parser → loader → engine → Rete → activation.
//!
//! These tests exercise the complete flow from CLIPS source code to rule activations.
//! Phase 1 tests use manual rete construction. Phase 2 tests will use the
//! compiler pipeline via shared helpers in `test_helpers`.

#[cfg(test)]
mod tests {
    use ferric_core::beta::RuleId;
    use ferric_core::{AtomKey, ConstantTest, ConstantTestType, SlotIndex};

    use crate::test_helpers::{
        assert_facts_into_rete, assert_rete_clean, assert_rete_consistent,
        build_constant_test_rete, build_single_pattern_rete, intern, load_ok, new_utf8_engine,
        retract_one_fact,
    };

    #[test]
    fn integration_parse_load_assert_match() {
        let mut engine = new_utf8_engine();
        let result = load_ok(&mut engine, "(assert (person Alice 25))");

        assert_eq!(result.asserted_facts.len(), 1);
        assert_eq!(result.rules.len(), 0);
        assert!(result.warnings.is_empty());

        let rule_id = RuleId(1);
        let mut rete = build_single_pattern_rete(&mut engine, "person", rule_id);

        let activations = assert_facts_into_rete(&mut rete, &engine, &result.asserted_facts);

        assert_eq!(activations, 1);
        assert_eq!(rete.agenda.len(), 1);

        let act = rete.agenda.pop().unwrap();
        assert_eq!(act.rule, rule_id);
    }

    #[test]
    fn integration_retract_removes_activation() {
        let mut engine = new_utf8_engine();
        let result = load_ok(&mut engine, "(assert (person Bob 30))");
        assert_eq!(result.asserted_facts.len(), 1);

        let rule_id = RuleId(1);
        let mut rete = build_single_pattern_rete(&mut engine, "person", rule_id);
        let fact_id = result.asserted_facts[0];

        let acts = assert_facts_into_rete(&mut rete, &engine, &[fact_id]);
        assert_eq!(acts, 1);
        assert_eq!(rete.agenda.len(), 1);

        let removed = retract_one_fact(&mut rete, &mut engine, fact_id);
        assert_eq!(removed.len(), 1);
        assert_rete_clean(&rete);
    }

    #[test]
    fn integration_multiple_facts_multiple_activations() {
        let mut engine = new_utf8_engine();
        let source = r"
            (assert (person Alice))
            (assert (person Bob))
            (assert (person Carol))
        ";
        let result = load_ok(&mut engine, source);
        assert_eq!(result.asserted_facts.len(), 3);

        let rule_id = RuleId(1);
        let mut rete = build_single_pattern_rete(&mut engine, "person", rule_id);

        let activation_count = assert_facts_into_rete(&mut rete, &engine, &result.asserted_facts);
        assert_eq!(activation_count, 3);
        assert_eq!(rete.agenda.len(), 3);
    }

    #[test]
    fn integration_constant_test_filters_facts() {
        let mut engine = new_utf8_engine();
        let source = r"
            (assert (color red))
            (assert (color blue))
            (assert (color green))
        ";
        let result = load_ok(&mut engine, source);
        assert_eq!(result.asserted_facts.len(), 3);

        let red_sym = intern(&mut engine, "red");
        let red_test = ConstantTest {
            slot: SlotIndex::Ordered(0),
            test_type: ConstantTestType::Equal(AtomKey::Symbol(red_sym)),
        };
        let rule_id = RuleId(1);
        let mut rete = build_constant_test_rete(&mut engine, "color", red_test, rule_id);

        let activation_count = assert_facts_into_rete(&mut rete, &engine, &result.asserted_facts);
        assert_eq!(activation_count, 1);
        assert_eq!(rete.agenda.len(), 1);
    }

    #[test]
    fn integration_loader_and_rete_roundtrip() {
        let mut engine = new_utf8_engine();
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
        let result = load_ok(&mut engine, source);

        assert_eq!(result.asserted_facts.len(), 3);
        assert_eq!(result.rules.len(), 1);

        let rule_id = RuleId(1);
        let mut rete = build_single_pattern_rete(&mut engine, "animal", rule_id);

        let activation_count = assert_facts_into_rete(&mut rete, &engine, &result.asserted_facts);
        assert_eq!(activation_count, 3);
        assert_eq!(rete.agenda.len(), 3);

        let first_act = rete.agenda.pop();
        assert!(first_act.is_some());
        assert_eq!(rete.agenda.len(), 2);

        let rule_def = &result.rules[0];
        assert_eq!(rule_def.name, "match-animal");
        assert_eq!(rule_def.lhs.len(), 1);
        assert_eq!(rule_def.rhs.len(), 1);
    }

    #[test]
    fn integration_assert_retract_cycle_with_consistency_checks() {
        let mut engine = new_utf8_engine();
        let rule_id = RuleId(1);
        let mut rete = build_single_pattern_rete(&mut engine, "item", rule_id);

        // Assert several facts, checking consistency after each
        let mut fact_ids = Vec::new();
        for i in 0..5 {
            let result = load_ok(&mut engine, &format!("(assert (item v{i}))"));
            let fid = result.asserted_facts[0];
            assert_facts_into_rete(&mut rete, &engine, &[fid]);
            fact_ids.push(fid);
            assert_rete_consistent(&rete);
        }
        assert_eq!(rete.agenda.len(), 5);

        // Retract all, checking consistency after each
        for fid in fact_ids {
            retract_one_fact(&mut rete, &mut engine, fid);
            assert_rete_consistent(&rete);
        }

        assert_rete_clean(&rete);
    }
}
