//! Phase 2 integration tests: compiled pipeline end-to-end.
//!
//! These tests exercise the full Phase 2 pipeline:
//! parse → Stage 2 interpret → compile → rete assertion → verify activations.
//!
//! Tests are added incrementally as passes land.

#[cfg(test)]
mod tests {
    use crate::test_helpers::{assert_rete_clean, assert_rete_consistent, load_ok, new_utf8_engine};

    // -----------------------------------------------------------------------
    // Pass 004: Rule compilation pipeline
    // -----------------------------------------------------------------------

    #[test]
    fn compiled_rule_produces_activation_on_match() {
        let mut engine = new_utf8_engine();
        let rule_result = load_ok(&mut engine, "(defrule test (person ?x) => (printout t ?x))");
        assert_eq!(rule_result.rules.len(), 1);

        // Verify the rule compiled into executable rete by asserting a matching fact
        let fact_result = load_ok(&mut engine, "(assert (person Alice))");
        let fid = fact_result.asserted_facts[0];
        let fact = engine.fact_base.get(fid).unwrap().fact.clone();
        let activations = engine.rete.assert_fact(fid, &fact, &engine.fact_base);

        assert_eq!(activations.len(), 1, "compiled rule should produce activation");
    }

    #[test]
    fn compiled_rule_activates_on_matching_fact() {
        let mut engine = new_utf8_engine();
        let _result = load_ok(&mut engine, "(defrule greet (person ?x) => (printout t ?x))");

        // Assert a matching fact
        let fact_result = load_ok(&mut engine, "(assert (person Alice))");
        assert_eq!(fact_result.asserted_facts.len(), 1);

        // Propagate the fact through the engine's rete
        let fact_id = fact_result.asserted_facts[0];
        let fact = engine
            .fact_base
            .get(fact_id)
            .expect("fact should exist")
            .fact
            .clone();
        let activations = engine
            .rete
            .assert_fact(fact_id, &fact, &engine.fact_base);

        assert_eq!(activations.len(), 1, "should have one activation for matching fact");
        assert_eq!(engine.rete.agenda.len(), 1);
    }

    #[test]
    fn compiled_rule_does_not_activate_on_non_matching_fact() {
        let mut engine = new_utf8_engine();
        let _result = load_ok(&mut engine, "(defrule greet (person ?x) => (printout t ?x))");

        // Assert a non-matching fact (different relation)
        let fact_result = load_ok(&mut engine, "(assert (animal dog))");
        let fact_id = fact_result.asserted_facts[0];
        let fact = engine
            .fact_base
            .get(fact_id)
            .expect("fact should exist")
            .fact
            .clone();
        let activations = engine
            .rete
            .assert_fact(fact_id, &fact, &engine.fact_base);

        assert!(activations.is_empty(), "non-matching fact should not activate");
        assert!(engine.rete.agenda.is_empty());
    }

    #[test]
    fn compiled_rule_with_constant_test_filters() {
        let mut engine = new_utf8_engine();
        let _result = load_ok(
            &mut engine,
            "(defrule match-red (color red) => (printout t \"found red\"))",
        );

        // Assert multiple facts
        let facts = load_ok(
            &mut engine,
            r"
            (assert (color red))
            (assert (color blue))
            (assert (color green))
        ",
        );

        for &fid in &facts.asserted_facts {
            let fact = engine.fact_base.get(fid).unwrap().fact.clone();
            engine.rete.assert_fact(fid, &fact, &engine.fact_base);
        }

        // Only (color red) should match
        assert_eq!(engine.rete.agenda.len(), 1);
    }

    #[test]
    fn two_rules_share_alpha_path_for_same_pattern() {
        let mut engine = new_utf8_engine();
        let source = r"
            (defrule rule-a (person ?x) => (printout t ?x))
            (defrule rule-b (person ?y) => (assert (found ?y)))
        ";
        let _result = load_ok(&mut engine, source);

        // Assert a fact — should activate both rules
        let facts = load_ok(&mut engine, "(assert (person Alice))");
        let fid = facts.asserted_facts[0];
        let fact = engine.fact_base.get(fid).unwrap().fact.clone();
        let activations = engine.rete.assert_fact(fid, &fact, &engine.fact_base);

        assert_eq!(activations.len(), 2, "both rules should activate");
        assert_eq!(engine.rete.agenda.len(), 2);
    }

    #[test]
    fn multi_pattern_rule_compiles_into_join_chain() {
        let mut engine = new_utf8_engine();
        let source = r"
            (defrule match-pair
                (parent ?x ?y)
                (parent ?y ?z)
                =>
                (assert (grandparent ?x ?z)))
        ";
        let result = load_ok(&mut engine, source);
        assert_eq!(result.rules.len(), 1);

        // Verify the rule compiled: assert a single fact and check
        // it creates activations through the first join.
        // Full variable-binding join tests (verifying ?y matches across
        // patterns) will be tested in Pass 005 after join binding extraction
        // is implemented.
        let f1 = load_ok(&mut engine, "(assert (parent alice bob))");
        let fid = f1.asserted_facts[0];
        let fact = engine.fact_base.get(fid).unwrap().fact.clone();
        let activations = engine.rete.assert_fact(fid, &fact, &engine.fact_base);

        // The fact should reach the first join (for pattern 1: parent ?x ?y)
        // and right-activate the second join (for pattern 2: parent ?y ?z).
        // Without full binding extraction, the exact activation count depends
        // on the Phase 1 join behavior. The key assertion is: no panic, rete
        // is consistent.
        let _ = activations; // activation count depends on binding extraction (Pass 005)
        assert_rete_consistent(engine.rete());
    }

    #[test]
    fn compiled_rule_retract_removes_activation() {
        let mut engine = new_utf8_engine();
        let _result = load_ok(&mut engine, "(defrule test (item ?x) => (printout t ?x))");

        // Assert then retract
        let facts = load_ok(&mut engine, "(assert (item foo))");
        let fid = facts.asserted_facts[0];
        let fact = engine.fact_base.get(fid).unwrap().fact.clone();
        engine.rete.assert_fact(fid, &fact, &engine.fact_base);
        assert_eq!(engine.rete.agenda.len(), 1);

        // Retract
        engine.rete.retract_fact(fid, &fact);
        assert_rete_clean(engine.rete());
    }

    #[test]
    fn multiple_facts_multiple_activations_compiled() {
        let mut engine = new_utf8_engine();
        let _result = load_ok(&mut engine, "(defrule test (item ?x) => (printout t ?x))");

        let facts = load_ok(
            &mut engine,
            r"
            (assert (item a))
            (assert (item b))
            (assert (item c))
        ",
        );

        for &fid in &facts.asserted_facts {
            let fact = engine.fact_base.get(fid).unwrap().fact.clone();
            engine.rete.assert_fact(fid, &fact, &engine.fact_base);
        }

        assert_eq!(engine.rete.agenda.len(), 3);
        assert_rete_consistent(engine.rete());
    }

    #[test]
    fn deffacts_asserted_facts_activate_compiled_rules() {
        let mut engine = new_utf8_engine();
        let source = r"
            (defrule match-animal (animal ?x) => (printout t ?x))
            (deffacts startup (animal dog) (animal cat) (animal bird))
        ";
        let result = load_ok(&mut engine, source);

        assert_eq!(result.rules.len(), 1);
        assert_eq!(result.asserted_facts.len(), 3);

        // Propagate deffacts through the compiled rete
        for &fid in &result.asserted_facts {
            let fact = engine.fact_base.get(fid).unwrap().fact.clone();
            engine.rete.assert_fact(fid, &fact, &engine.fact_base);
        }

        assert_eq!(engine.rete.agenda.len(), 3);
        assert_rete_consistent(engine.rete());
    }

    #[test]
    fn compiled_rule_with_not_equal_via_compiler_api() {
        // NOTE: The Stage 2 interpreter does not yet combine bare connective
        // tokens (e.g., ~red parsed as two atoms: ~ and red) into
        // Constraint::Not. This test exercises NotEqual constant tests
        // directly through the compiler API.
        use ferric_core::{
            AlphaEntryType, AtomKey, CompilablePattern, CompilableRule, ConstantTest,
            ConstantTestType, ReteCompiler, ReteNetwork, SlotIndex,
        };

        let mut engine = new_utf8_engine();
        let red_sym = engine.intern_symbol("red").unwrap();
        let color_sym = engine.intern_symbol("color").unwrap();

        let mut rete = ReteNetwork::new();
        let mut compiler = ReteCompiler::new();

        let rule = CompilableRule {
            rule_id: compiler.allocate_rule_id(),
            salience: 0,
            patterns: vec![CompilablePattern {
                entry_type: AlphaEntryType::OrderedRelation(color_sym),
                constant_tests: vec![ConstantTest {
                    slot: SlotIndex::Ordered(0),
                    test_type: ConstantTestType::NotEqual(AtomKey::Symbol(red_sym)),
                }],
                variable_slots: vec![],
            }],
        };

        compiler.compile_rule(&mut rete, &rule).unwrap();

        // Assert facts via engine, propagate through standalone rete
        let facts = load_ok(
            &mut engine,
            r"
            (assert (color red))
            (assert (color blue))
            (assert (color green))
        ",
        );

        for &fid in &facts.asserted_facts {
            let fact = engine.fact_base.get(fid).unwrap().fact.clone();
            rete.assert_fact(fid, &fact, &engine.fact_base);
        }

        // blue and green should match, red should not
        assert_eq!(rete.agenda.len(), 2);
    }

    // -----------------------------------------------------------------------
    // Planned test areas for later passes:
    // -----------------------------------------------------------------------
    // - negative pattern behavior under assert/retract
    // - NCC behavior under conjunction match/unmatch
    // - exists behavior under support add/remove
    // - agenda strategy ordering in multi-rule programs
    // - .clp fixture loading and verification
    // - forall_vacuous_truth regression shape (Phase 3 plug-in)
}
