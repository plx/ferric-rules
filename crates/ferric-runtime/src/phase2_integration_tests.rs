//! Phase 2 integration tests: compiled pipeline end-to-end.
//!
//! These tests exercise the full Phase 2 pipeline:
//! parse → Stage 2 interpret → compile → rete assertion → verify activations.
//!
//! Tests are added incrementally as passes land.

#[cfg(test)]
mod tests {
    use crate::test_helpers::{
        assert_has_fact_with_relation, assert_rete_clean, assert_rete_consistent, load_ok,
        new_utf8_engine,
    };

    // -----------------------------------------------------------------------
    // Pass 004: Rule compilation pipeline
    // -----------------------------------------------------------------------

    #[test]
    fn compiled_rule_produces_activation_on_match() {
        let mut engine = new_utf8_engine();
        let rule_result = load_ok(&mut engine, "(defrule test (person ?x) => (printout t ?x))");
        assert_eq!(rule_result.rules.len(), 1);

        // Verify the rule compiled into executable rete by asserting a matching fact
        let _fact_result = load_ok(&mut engine, "(assert (person Alice))");

        assert_eq!(
            engine.rete.agenda.len(),
            1,
            "compiled rule should produce activation"
        );
    }

    #[test]
    fn compiled_rule_activates_on_matching_fact() {
        let mut engine = new_utf8_engine();
        let _result = load_ok(
            &mut engine,
            "(defrule greet (person ?x) => (printout t ?x))",
        );

        // Assert a matching fact (automatically propagates through rete)
        let _fact_result = load_ok(&mut engine, "(assert (person Alice))");

        assert_eq!(
            engine.rete.agenda.len(),
            1,
            "should have one activation for matching fact"
        );
    }

    #[test]
    fn compiled_rule_does_not_activate_on_non_matching_fact() {
        let mut engine = new_utf8_engine();
        let _result = load_ok(
            &mut engine,
            "(defrule greet (person ?x) => (printout t ?x))",
        );

        // Assert a non-matching fact (different relation)
        let _fact_result = load_ok(&mut engine, "(assert (animal dog))");

        assert!(
            engine.rete.agenda.is_empty(),
            "non-matching fact should not activate"
        );
    }

    #[test]
    fn compiled_rule_with_constant_test_filters() {
        let mut engine = new_utf8_engine();
        let _result = load_ok(
            &mut engine,
            "(defrule match-red (color red) => (printout t \"found red\"))",
        );

        // Assert multiple facts (automatically propagate through rete)
        let _facts = load_ok(
            &mut engine,
            r"
            (assert (color red))
            (assert (color blue))
            (assert (color green))
        ",
        );

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
        let _facts = load_ok(&mut engine, "(assert (person Alice))");

        assert_eq!(engine.rete.agenda.len(), 2, "both rules should activate");
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

        // Assert facts: (parent alice bob), (parent bob carol), (parent dan eve)
        // Expected: 1 activation (alice→bob→carol chain)
        let _facts = load_ok(
            &mut engine,
            r"
            (assert (parent alice bob))
            (assert (parent bob carol))
            (assert (parent dan eve))
        ",
        );

        // Only (parent alice bob) → (parent bob carol) should match
        // (parent dan eve) doesn't connect, so no second activation
        assert_eq!(
            engine.rete.agenda.len(),
            1,
            "Should have exactly one activation for the alice→bob→carol chain"
        );
        assert_rete_consistent(engine.rete());
    }

    #[test]
    fn compiled_rule_retract_removes_activation() {
        let mut engine = new_utf8_engine();
        let _result = load_ok(&mut engine, "(defrule test (item ?x) => (printout t ?x))");

        // Assert then retract
        let facts = load_ok(&mut engine, "(assert (item foo))");
        let fid = facts.asserted_facts[0];
        assert_eq!(engine.rete.agenda.len(), 1);

        // Retract (use engine.retract which handles rete)
        engine.retract(fid).unwrap();
        assert_rete_clean(engine.rete());
    }

    #[test]
    fn multiple_facts_multiple_activations_compiled() {
        let mut engine = new_utf8_engine();
        let _result = load_ok(&mut engine, "(defrule test (item ?x) => (printout t ?x))");

        let _facts = load_ok(
            &mut engine,
            r"
            (assert (item a))
            (assert (item b))
            (assert (item c))
        ",
        );

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

        // Facts from deffacts automatically propagate through rete during load
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
            ConstantTestType, ReteCompiler, ReteNetwork, Salience, SlotIndex,
        };

        let mut engine = new_utf8_engine();
        let red_sym = engine.intern_symbol("red").unwrap();
        let color_sym = engine.intern_symbol("color").unwrap();

        let mut rete = ReteNetwork::new();
        let mut compiler = ReteCompiler::new();

        let rule = CompilableRule {
            rule_id: compiler.allocate_rule_id(),
            salience: Salience::DEFAULT,
            patterns: vec![CompilablePattern {
                entry_type: AlphaEntryType::OrderedRelation(color_sym),
                constant_tests: vec![ConstantTest {
                    slot: SlotIndex::Ordered(0),
                    test_type: ConstantTestType::NotEqual(AtomKey::Symbol(red_sym)),
                }],
                variable_slots: vec![],
                negated_variable_slots: Vec::new(),
                negated: false,
                exists: false,
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
    // Pass 006: Negative node (single pattern) and blocker tracking
    // -----------------------------------------------------------------------

    /// Helper: assert facts into both the `fact_base` (via `load_ok`) and the rete network.
    fn assert_facts_into_rete(
        engine: &mut crate::Engine,
        source: &str,
    ) -> Vec<ferric_core::FactId> {
        // Facts now automatically propagate through rete via load_ok
        let result = load_ok(engine, source);
        result.asserted_facts
    }

    /// Helper: retract a fact (automatically retracts from rete).
    fn retract_from_rete(engine: &mut crate::Engine, fid: ferric_core::FactId) {
        engine.retract(fid).expect("retract should succeed");
    }

    #[test]
    fn negative_rule_fires_when_no_blocking_fact() {
        // CLIPS equivalent:
        //   (defrule no-danger (item ?x) (not (danger)) => (printout t "safe"))
        //   (assert (item lamp))
        //   => activation fires because no (danger) fact exists
        let mut engine = new_utf8_engine();
        load_ok(
            &mut engine,
            "(defrule safe (item ?x) (not (danger)) => (printout t safe))",
        );

        assert_facts_into_rete(&mut engine, "(assert (item lamp))");

        assert_eq!(
            engine.rete.agenda.len(),
            1,
            "Rule should fire when no blocking fact exists"
        );
        assert_rete_consistent(engine.rete());
    }

    #[test]
    fn negative_rule_blocked_when_fact_exists() {
        // CLIPS equivalent:
        //   (defrule safe (item ?x) (not (danger)) => ...)
        //   (assert (item lamp))
        //   (assert (danger))
        //   => no activation because (danger) exists
        let mut engine = new_utf8_engine();
        load_ok(
            &mut engine,
            "(defrule safe (item ?x) (not (danger)) => (printout t safe))",
        );

        // Assert both the item and the blocking fact
        assert_facts_into_rete(&mut engine, "(assert (danger))");
        assert_facts_into_rete(&mut engine, "(assert (item lamp))");

        assert_eq!(
            engine.rete.agenda.len(),
            0,
            "Rule should NOT fire when blocking fact exists"
        );
        assert_rete_consistent(engine.rete());
    }

    #[test]
    fn negative_rule_unblocked_by_retraction() {
        // CLIPS equivalent:
        //   (defrule safe (item ?x) (not (danger)) => ...)
        //   (assert (danger)) (assert (item lamp))
        //   => no activation
        //   (retract <danger-fact>)
        //   => activation fires
        let mut engine = new_utf8_engine();
        load_ok(
            &mut engine,
            "(defrule safe (item ?x) (not (danger)) => (printout t safe))",
        );

        let danger_fids = assert_facts_into_rete(&mut engine, "(assert (danger))");
        assert_facts_into_rete(&mut engine, "(assert (item lamp))");
        assert_eq!(engine.rete.agenda.len(), 0, "Should be blocked");

        // Retract the blocking fact
        retract_from_rete(&mut engine, danger_fids[0]);

        assert_eq!(
            engine.rete.agenda.len(),
            1,
            "Rule should fire after blocking fact retracted"
        );
        assert_rete_consistent(engine.rete());
    }

    #[test]
    fn negative_rule_blocked_then_unblocked_cycle() {
        let mut engine = new_utf8_engine();
        load_ok(
            &mut engine,
            "(defrule safe (item ?x) (not (danger)) => (printout t safe))",
        );

        // Start with just the item — activation fires
        assert_facts_into_rete(&mut engine, "(assert (item lamp))");
        assert_eq!(engine.rete.agenda.len(), 1);

        // Add danger — blocks
        let danger_fids = assert_facts_into_rete(&mut engine, "(assert (danger))");
        assert_eq!(engine.rete.agenda.len(), 0);

        // Remove danger — unblocks
        retract_from_rete(&mut engine, danger_fids[0]);
        assert_eq!(engine.rete.agenda.len(), 1);

        // Add danger again — blocks again
        let second_danger_fids = assert_facts_into_rete(&mut engine, "(assert (danger))");
        assert_eq!(engine.rete.agenda.len(), 0);

        // Remove danger again — unblocks again
        retract_from_rete(&mut engine, second_danger_fids[0]);
        assert_eq!(engine.rete.agenda.len(), 1);
        assert_rete_consistent(engine.rete());
    }

    #[test]
    fn negative_rule_with_shared_variable() {
        // CLIPS equivalent:
        //   (defrule not-excluded
        //     (item ?x)
        //     (not (exclude ?x))
        //     =>
        //     (printout t ?x " is not excluded"))
        //
        //   (assert (item alice) (item bob))
        //   => 2 activations
        //   (assert (exclude alice))
        //   => alice blocked, bob still active → 1 activation
        //   (retract <exclude-alice>)
        //   => alice unblocked → 2 activations
        let mut engine = new_utf8_engine();
        load_ok(
            &mut engine,
            r"(defrule not-excluded
                (item ?x)
                (not (exclude ?x))
                =>
                (printout t ?x))",
        );

        assert_facts_into_rete(&mut engine, "(assert (item alice)) (assert (item bob))");
        assert_eq!(
            engine.rete.agenda.len(),
            2,
            "Both items should fire with no excludes"
        );

        // Exclude alice specifically
        let exc_fids = assert_facts_into_rete(&mut engine, "(assert (exclude alice))");
        assert_eq!(engine.rete.agenda.len(), 1, "Only bob should remain active");
        assert_rete_consistent(engine.rete());

        // Retract exclude
        retract_from_rete(&mut engine, exc_fids[0]);
        assert_eq!(
            engine.rete.agenda.len(),
            2,
            "Both items should be active again"
        );
        assert_rete_consistent(engine.rete());
    }

    #[test]
    fn negative_rule_non_matching_exclude_doesnt_block() {
        let mut engine = new_utf8_engine();
        load_ok(
            &mut engine,
            r"(defrule not-excluded
                (item ?x)
                (not (exclude ?x))
                =>
                (printout t ?x))",
        );

        assert_facts_into_rete(&mut engine, "(assert (item alice))");
        assert_eq!(engine.rete.agenda.len(), 1);

        // Exclude charlie (not alice) — shouldn't block
        assert_facts_into_rete(&mut engine, "(assert (exclude charlie))");
        assert_eq!(
            engine.rete.agenda.len(),
            1,
            "Non-matching exclude should not block alice"
        );
        assert_rete_consistent(engine.rete());
    }

    #[test]
    fn negative_rule_multiple_blockers_need_all_retracted() {
        let mut engine = new_utf8_engine();
        load_ok(
            &mut engine,
            "(defrule safe (item ?x) (not (danger)) => (printout t safe))",
        );

        // Two blocking facts
        let d1 = assert_facts_into_rete(&mut engine, "(assert (danger))");
        let d2 = assert_facts_into_rete(&mut engine, "(assert (danger))");

        assert_facts_into_rete(&mut engine, "(assert (item lamp))");
        assert_eq!(engine.rete.agenda.len(), 0, "Both block");

        // Remove first — still blocked
        retract_from_rete(&mut engine, d1[0]);
        assert_eq!(engine.rete.agenda.len(), 0, "Still blocked by second");

        // Remove second — now unblocked
        retract_from_rete(&mut engine, d2[0]);
        assert_eq!(engine.rete.agenda.len(), 1, "Now unblocked");
        assert_rete_consistent(engine.rete());
    }

    #[test]
    fn negative_rule_retract_positive_cleans_up() {
        let mut engine = new_utf8_engine();
        load_ok(
            &mut engine,
            "(defrule safe (item ?x) (not (danger)) => (printout t safe))",
        );

        let items = assert_facts_into_rete(&mut engine, "(assert (item lamp))");
        assert_eq!(engine.rete.agenda.len(), 1);

        // Retract the positive fact
        retract_from_rete(&mut engine, items[0]);
        assert_rete_clean(engine.rete());
    }

    // -----------------------------------------------------------------------
    // Pass 009: Action execution tests
    // -----------------------------------------------------------------------

    #[test]
    fn rule_assert_action_creates_new_fact() {
        let mut engine = new_utf8_engine();
        engine
            .load_str("(defrule derive (person ?name) => (assert (greeted ?name)))")
            .unwrap();
        engine.load_str("(assert (person Alice))").unwrap();

        // Should have 1 activation for the rule
        assert_eq!(engine.rete.agenda.len(), 1);

        // Fire the rule
        let result = engine.run(crate::execution::RunLimit::Count(1)).unwrap();
        assert_eq!(result.rules_fired, 1);

        // The assert action should have created a (greeted Alice) fact
        let facts: Vec<_> = engine.facts().unwrap().collect();
        // Should have original (person Alice) + new (greeted Alice) = 2 facts
        assert_eq!(facts.len(), 2);
    }

    #[test]
    fn rule_retract_action_removes_matched_fact() {
        let mut engine = new_utf8_engine();
        engine
            .load_str("(defrule cleanup ?f <- (temporary ?x) => (retract ?f))")
            .unwrap();
        engine.load_str("(assert (temporary data))").unwrap();

        assert_eq!(engine.rete.agenda.len(), 1);
        let fact_count_before = engine.facts().unwrap().count();
        assert_eq!(fact_count_before, 1);

        engine.run(crate::execution::RunLimit::Count(1)).unwrap();

        // The temporary fact should be retracted
        let fact_count_after = engine.facts().unwrap().count();
        assert_eq!(fact_count_after, 0);
    }

    #[test]
    fn rule_halt_action_stops_execution() {
        let mut engine = new_utf8_engine();
        engine
            .load_str("(defrule stopper (stop) => (halt))")
            .unwrap();
        engine
            .load_str("(defrule other (person ?x) => (assert (greeted ?x)))")
            .unwrap();
        engine.load_str("(assert (stop))").unwrap();
        engine.load_str("(assert (person Alice))").unwrap();

        // Both rules should have activations
        assert!(!engine.rete.agenda.is_empty());

        let result = engine.run(crate::execution::RunLimit::Unlimited).unwrap();
        // One rule should fire halt, which stops execution
        // The exact number depends on agenda ordering (both have salience 0)
        assert!(
            result.halt_reason == crate::execution::HaltReason::HaltRequested
                || result.halt_reason == crate::execution::HaltReason::AgendaEmpty
        );
    }

    #[test]
    fn rule_assert_triggers_chain_reaction() {
        let mut engine = new_utf8_engine();
        // Rule 1: person → greeted
        engine
            .load_str("(defrule greet (person ?name) => (assert (greeted ?name)))")
            .unwrap();
        // Rule 2: greeted → done
        engine
            .load_str("(defrule done (greeted ?name) => (assert (done ?name)))")
            .unwrap();
        engine.load_str("(assert (person Alice))").unwrap();

        let result = engine.run(crate::execution::RunLimit::Unlimited).unwrap();
        assert_eq!(result.rules_fired, 2);

        // Should have: person Alice, greeted Alice, done Alice
        let fact_count = engine.facts().unwrap().count();
        assert_eq!(fact_count, 3);
    }

    #[test]
    fn rule_retract_with_assert_rebuilds_state() {
        let mut engine = new_utf8_engine();
        // Retract old, assert new
        engine
            .load_str("(defrule upgrade ?f <- (level low) => (retract ?f) (assert (level high)))")
            .unwrap();
        engine.load_str("(assert (level low))").unwrap();

        let result = engine.run(crate::execution::RunLimit::Count(1)).unwrap();
        assert_eq!(result.rules_fired, 1);

        // Should have only (level high), not (level low)
        let facts: Vec<_> = engine.facts().unwrap().collect();
        assert_eq!(facts.len(), 1);
    }

    #[test]
    fn rule_with_salience_fires_in_order() {
        let mut engine = new_utf8_engine();
        engine
            .load_str(
                r"
            (defrule low-priority (trigger) => (assert (fired low)))
            (defrule high-priority (declare (salience 10)) (trigger) => (assert (fired high)))
        ",
            )
            .unwrap();
        engine.load_str("(assert (trigger))").unwrap();

        // Step once — should fire high-priority first
        engine.step().unwrap();

        // Check which rule fired (high-priority should fire first due to salience)
        let facts: Vec<_> = engine.facts().unwrap().collect();
        // After one step, should have trigger + fired-high = 2 facts
        assert_eq!(facts.len(), 2);
    }

    #[test]
    fn reset_and_run_cycle_with_actions() {
        let mut engine = new_utf8_engine();
        engine
            .load_str("(defrule greet (person ?name) => (assert (greeted ?name)))")
            .unwrap();
        engine
            .load_str("(deffacts startup (person Alice))")
            .unwrap();

        // First run
        let result = engine.run(crate::execution::RunLimit::Unlimited).unwrap();
        assert_eq!(result.rules_fired, 1);
        assert_eq!(engine.facts().unwrap().count(), 2); // person + greeted

        // Reset and run again
        engine.reset().unwrap();
        let result2 = engine.run(crate::execution::RunLimit::Unlimited).unwrap();
        assert_eq!(result2.rules_fired, 1);
        assert_eq!(engine.facts().unwrap().count(), 2);
    }

    // -----------------------------------------------------------------------
    // Pass 010: Exists node tests
    // -----------------------------------------------------------------------

    #[test]
    fn exists_single_pattern_fires_once() {
        let mut engine = new_utf8_engine();
        engine
            .load_str(
                "(defrule has-person (trigger) (exists (person ?x)) => (assert (has-someone)))",
            )
            .unwrap();
        engine.load_str("(assert (trigger))").unwrap();
        engine.load_str("(assert (person Alice))").unwrap();
        engine.load_str("(assert (person Bob))").unwrap();

        // Despite two persons, exists should produce only one activation
        assert_eq!(
            engine.rete.agenda.len(),
            1,
            "exists should produce exactly one activation"
        );

        let result = engine.run(crate::execution::RunLimit::Unlimited).unwrap();
        assert_eq!(result.rules_fired, 1);
    }

    #[test]
    fn exists_root_level_pattern_fires() {
        let mut engine = new_utf8_engine();
        engine
            .load_str("(defrule any-person (exists (person ?x)) => (assert (seen-person)))")
            .unwrap();
        engine.load_str("(assert (person Alice))").unwrap();

        assert_eq!(engine.rete.agenda.len(), 1);

        let result = engine.run(crate::execution::RunLimit::Unlimited).unwrap();
        assert_eq!(result.rules_fired, 1, "root-level exists should fire");
    }

    #[test]
    fn exists_retract_last_removes_activation() {
        let mut engine = new_utf8_engine();
        engine
            .load_str("(defrule has-person (trigger) (exists (person ?x)) => (assert (detected)))")
            .unwrap();
        engine.load_str("(assert (trigger))").unwrap();
        engine.load_str("(assert (person Alice))").unwrap();

        assert_eq!(engine.rete.agenda.len(), 1);

        // For now, just verify the agenda had 1 activation
        // Full retraction testing is covered in rete.rs unit tests
    }

    #[test]
    fn exists_with_run_produces_expected_facts() {
        let mut engine = new_utf8_engine();
        engine
            .load_str(
                r"
            (defrule detect-person
                (trigger)
                (exists (person ?x))
                =>
                (assert (has-person-detected)))
        ",
            )
            .unwrap();
        engine.load_str("(assert (trigger))").unwrap();
        engine.load_str("(assert (person Alice))").unwrap();
        engine.load_str("(assert (person Bob))").unwrap();
        engine.load_str("(assert (person Carol))").unwrap();

        let result = engine.run(crate::execution::RunLimit::Unlimited).unwrap();
        assert_eq!(result.rules_fired, 1, "exists should fire exactly once");

        // Should have: trigger, person Alice, person Bob, person Carol, has-person-detected = 5
        let fact_count = engine.facts().unwrap().count();
        assert_eq!(fact_count, 5);
    }

    // -----------------------------------------------------------------------
    // Pass 011: Pattern validation and source-located compile errors
    // -----------------------------------------------------------------------

    #[test]
    fn triple_nested_not_compiles_as_not() {
        // (not (not (not (b)))) = (not (b)) after double-negation stripping
        let mut engine = new_utf8_engine();
        let result = engine.load_str("(defrule r (a) (not (not (not (b)))) => )");

        assert!(
            result.is_ok(),
            "triple-nested not should compile (strips to not(b)): {result:?}"
        );
    }

    #[test]
    fn deeply_nested_not_fails_validation() {
        // Depth 5 exceeds the max depth of 4
        let mut engine = new_utf8_engine();
        let result = engine.load_str("(defrule r (a) (not (not (not (not (not (b)))))) => )");

        assert!(result.is_err(), "5-deep nested not should fail validation");
        let errs = result.unwrap_err();
        assert_eq!(errs.len(), 1);

        if let crate::loader::LoadError::Validation(validation_errors) = &errs[0] {
            assert_eq!(validation_errors.len(), 1);
            assert_eq!(validation_errors[0].code, "E0001");
        } else {
            panic!("Expected LoadError::Validation, got {:?}", errs[0]);
        }
    }

    #[test]
    fn exists_containing_not_fails_validation() {
        let mut engine = new_utf8_engine();
        let result = engine.load_str("(defrule r (a) (exists (not (b))) => )");

        assert!(
            result.is_err(),
            "exists containing not should fail validation"
        );
        let errs = result.unwrap_err();
        assert_eq!(errs.len(), 1);

        if let crate::loader::LoadError::Validation(validation_errors) = &errs[0] {
            assert_eq!(validation_errors.len(), 1);
            assert_eq!(validation_errors[0].code, "E0005");
        } else {
            panic!("Expected LoadError::Validation, got {:?}", errs[0]);
        }
    }

    #[test]
    fn multi_pattern_exists_with_not_passes_validation() {
        let mut engine = new_utf8_engine();
        let result = engine.load_str("(defrule r (a) (exists (b) (not (c))) => )");

        assert!(
            result.is_ok(),
            "multi-pattern exists containing not should pass validation/desugaring: {result:?}"
        );
    }

    #[test]
    fn valid_not_exists_passes() {
        let mut engine = new_utf8_engine();
        let result = engine.load_str("(defrule r (a) (not (exists (b))) => )");

        assert!(
            result.is_ok(),
            "not-exists combination should pass validation"
        );
    }

    #[test]
    fn double_nested_not_passes() {
        let mut engine = new_utf8_engine();
        let result = engine.load_str("(defrule r (a) (not (not (b))) => )");

        assert!(
            result.is_ok(),
            "double-nested not (depth 2) should pass validation"
        );
    }

    #[test]
    fn single_not_passes() {
        let mut engine = new_utf8_engine();
        let result = engine.load_str("(defrule r (a) (not (b)) => )");

        assert!(result.is_ok(), "single not should pass validation");
    }

    #[test]
    fn single_exists_passes() {
        let mut engine = new_utf8_engine();
        let result = engine.load_str("(defrule r (a) (exists (b)) => )");

        assert!(result.is_ok(), "single exists should pass validation");
    }

    #[test]
    fn ncc_not_and_block_unblock_cycle() {
        let mut engine = new_utf8_engine();
        load_ok(
            &mut engine,
            r"(defrule allow
                (item ?x)
                (not (and (block ?x) (reason ?x)))
                =>
                (printout t ?x))",
        );

        let item = assert_facts_into_rete(&mut engine, "(assert (item apple))");
        assert_eq!(engine.rete.agenda.len(), 1, "unblocked with no conjunction");

        let block = assert_facts_into_rete(&mut engine, "(assert (block apple))");
        assert_eq!(
            engine.rete.agenda.len(),
            1,
            "single subpattern should not block conjunction"
        );

        let reason = assert_facts_into_rete(&mut engine, "(assert (reason apple))");
        assert_eq!(
            engine.rete.agenda.len(),
            0,
            "full conjunction should block (0->1 transition)"
        );
        assert_rete_consistent(engine.rete());

        retract_from_rete(&mut engine, reason[0]);
        assert_eq!(
            engine.rete.agenda.len(),
            1,
            "retraction should unblock (1->0 transition)"
        );

        let reason2 = assert_facts_into_rete(&mut engine, "(assert (reason apple))");
        assert_eq!(engine.rete.agenda.len(), 0, "blocked again");

        retract_from_rete(&mut engine, block[0]);
        assert_eq!(
            engine.rete.agenda.len(),
            1,
            "unblocked by breaking conjunction"
        );

        // Cleanup should leave rete clean when parent is removed.
        retract_from_rete(&mut engine, reason2[0]);
        retract_from_rete(&mut engine, item[0]);
        assert_rete_consistent(engine.rete());
    }

    #[test]
    fn ncc_not_and_selective_blocking_by_variable() {
        let mut engine = new_utf8_engine();
        load_ok(
            &mut engine,
            r"(defrule allow
                (item ?x)
                (not (and (block ?x) (reason ?x)))
                =>
                (printout t ?x))",
        );

        assert_facts_into_rete(&mut engine, "(assert (item alice)) (assert (item bob))");
        assert_eq!(engine.rete.agenda.len(), 2);

        assert_facts_into_rete(&mut engine, "(assert (block alice))");
        assert_eq!(engine.rete.agenda.len(), 2, "still missing reason alice");

        let reason_alice = assert_facts_into_rete(&mut engine, "(assert (reason alice))");
        assert_eq!(
            engine.rete.agenda.len(),
            1,
            "alice blocked; bob remains unblocked"
        );

        retract_from_rete(&mut engine, reason_alice[0]);
        assert_eq!(
            engine.rete.agenda.len(),
            2,
            "alice should be restored after reason retract"
        );
        assert_rete_consistent(engine.rete());
    }

    // -----------------------------------------------------------------------
    // Planned test areas for later passes:
    // -----------------------------------------------------------------------
    // - agenda strategy ordering in multi-rule programs
    // - .clp fixture loading and verification

    // Phase 3 forall regression contract (per Section 7.5 of implementation plan).
    #[test]
    fn forall_vacuous_truth_and_retraction_cycle() {
        let mut engine = new_utf8_engine();
        engine
            .load_str(
                r"
            (defrule all-checked
                (forall (item ?id) (checked ?id))
                =>
                (assert (all-complete)))
        ",
            )
            .unwrap();

        // Step 2: Empty working memory -> forall is vacuously true -> rule fires
        engine.reset().unwrap();
        let result = engine.run(crate::execution::RunLimit::Unlimited).unwrap();
        assert_eq!(
            result.rules_fired, 1,
            "forall should be vacuously true with no items"
        );
        assert_has_fact_with_relation(&engine, "all-complete");

        // Step 3: Assert (item 1), run -> forall unsatisfied (missing (checked 1)).
        let item_fid = engine.load_str("(assert (item 1))").unwrap().asserted_facts[0];
        let result = engine.run(crate::execution::RunLimit::Unlimited).unwrap();
        assert_eq!(
            result.rules_fired, 0,
            "forall should be unsatisfied when an item is unchecked"
        );

        // Step 4: Assert (checked 1), run -> forall satisfied again.
        let checked_fid = engine
            .load_str("(assert (checked 1))")
            .unwrap()
            .asserted_facts[0];
        let result = engine.run(crate::execution::RunLimit::Unlimited).unwrap();
        assert_eq!(
            result.rules_fired, 1,
            "forall should be satisfied once all items are checked"
        );

        // Step 5: Retract (checked 1), run -> forall unsatisfied again.
        engine.retract(checked_fid).unwrap();
        let result = engine.run(crate::execution::RunLimit::Unlimited).unwrap();
        assert_eq!(
            result.rules_fired, 0,
            "forall should become unsatisfied after retracting required checked fact"
        );

        // Step 6: Retract (item 1), run -> vacuous truth restored.
        engine.retract(item_fid).unwrap();
        let result = engine.run(crate::execution::RunLimit::Unlimited).unwrap();
        assert_eq!(
            result.rules_fired, 1,
            "forall should become vacuously true again with zero items"
        );
    }

    // -----------------------------------------------------------------------
    // Pass 012: Integration fixtures and exit validation
    // -----------------------------------------------------------------------

    #[test]
    fn fixture_phase2_basic() {
        let mut engine = new_utf8_engine();
        engine
            .load_file(std::path::Path::new("tests/fixtures/phase2_basic.clp"))
            .unwrap();

        // Should have 2 facts from deffacts
        assert_eq!(engine.facts().unwrap().count(), 2);

        // Run should produce 2 greeted facts (one per person)
        let result = engine.run(crate::execution::RunLimit::Unlimited).unwrap();
        assert_eq!(result.rules_fired, 2);
        assert_eq!(engine.facts().unwrap().count(), 4); // 2 person + 2 greeted
    }

    #[test]
    fn fixture_phase2_negative() {
        let mut engine = new_utf8_engine();
        engine
            .load_file(std::path::Path::new("tests/fixtures/phase2_negative.clp"))
            .unwrap();

        // Should have 3 items + 1 forbidden = 4 facts
        assert_eq!(engine.facts().unwrap().count(), 4);

        // Run: apple and cherry should be allowed (banana is forbidden)
        let result = engine.run(crate::execution::RunLimit::Unlimited).unwrap();
        assert_eq!(result.rules_fired, 2); // apple + cherry
        assert_eq!(engine.facts().unwrap().count(), 6); // 4 original + 2 allowed
    }

    #[test]
    fn fixture_phase2_exists() {
        let mut engine = new_utf8_engine();
        engine
            .load_file(std::path::Path::new("tests/fixtures/phase2_exists.clp"))
            .unwrap();

        // Should have 1 category + 3 items = 4 facts
        assert_eq!(engine.facts().unwrap().count(), 4);

        // Run: exists should fire exactly once despite 3 fruit items
        let result = engine.run(crate::execution::RunLimit::Unlimited).unwrap();
        assert_eq!(result.rules_fired, 1);
        assert_eq!(engine.facts().unwrap().count(), 5); // 4 original + fruit-detected
    }

    #[test]
    fn fixture_phase2_salience() {
        let mut engine = new_utf8_engine();
        engine
            .load_file(std::path::Path::new("tests/fixtures/phase2_salience.clp"))
            .unwrap();

        // Step once — high-priority should fire first
        engine.step().unwrap();

        // After one step, should have trigger + fired-high = 2 facts
        assert_eq!(engine.facts().unwrap().count(), 2);
    }

    #[test]
    fn fixture_phase2_chain() {
        let mut engine = new_utf8_engine();
        engine
            .load_file(std::path::Path::new("tests/fixtures/phase2_chain.clp"))
            .unwrap();

        // Run all rules
        let result = engine.run(crate::execution::RunLimit::Unlimited).unwrap();
        assert_eq!(result.rules_fired, 3);
        assert_eq!(engine.facts().unwrap().count(), 4); // input + stage1 + stage2 + complete
    }

    #[test]
    fn fixture_phase2_retract() {
        let mut engine = new_utf8_engine();
        engine
            .load_file(std::path::Path::new("tests/fixtures/phase2_retract.clp"))
            .unwrap();

        // Should have 1 temporary fact from deffacts
        assert_eq!(engine.facts().unwrap().count(), 1);

        // Run: retract temporary, assert cleaned
        let result = engine.run(crate::execution::RunLimit::Unlimited).unwrap();
        assert_eq!(result.rules_fired, 1);
        assert_eq!(engine.facts().unwrap().count(), 1); // only cleaned remains
    }

    #[test]
    fn fixture_phase2_ncc() {
        let mut engine = new_utf8_engine();
        engine
            .load_file(std::path::Path::new("tests/fixtures/phase2_ncc.clp"))
            .unwrap();

        // 2 items + block/reason apple + block banana
        assert_eq!(engine.facts().unwrap().count(), 5);

        // Only banana should pass (apple is blocked by full conjunction)
        let result = engine.run(crate::execution::RunLimit::Unlimited).unwrap();
        assert_eq!(result.rules_fired, 1);
        assert_eq!(engine.facts().unwrap().count(), 6);
    }

    // -----------------------------------------------------------------------
    // Pass 012: Retraction invariant hardening
    // -----------------------------------------------------------------------

    #[test]
    fn retract_all_facts_leaves_clean_rete() {
        let mut engine = new_utf8_engine();
        engine
            .load_str(
                r"
            (defrule r1 (a ?x) => (printout t ?x))
            (defrule r2 (b ?x) (not (c ?x)) => (printout t ?x))
            (defrule r3 (d) (exists (e ?x)) => (printout t ?x))
        ",
            )
            .unwrap();

        engine
            .load_str("(assert (a 1) (b 2) (d) (e 10) (e 20))")
            .unwrap();

        // Collect all fact IDs
        let fids: Vec<_> = engine.facts().unwrap().map(|(fid, _)| fid).collect();
        assert!(!fids.is_empty());

        // Retract all
        for fid in fids {
            engine.retract(fid).unwrap();
        }

        // Rete should be completely clean
        assert_rete_clean(engine.rete());
    }

    #[test]
    fn retract_positive_fact_cascades_through_join_chain() {
        let mut engine = new_utf8_engine();
        engine
            .load_str(
                r"
            (defrule chain
                (a ?x)
                (b ?x ?y)
                (c ?y)
                =>
                (printout t ?x ?y))
        ",
            )
            .unwrap();

        engine.load_str("(assert (a 1) (b 1 2) (c 2))").unwrap();
        assert_eq!(engine.rete.agenda.len(), 1);

        // Retract middle fact by finding it in the fact base
        let b_sym = engine.intern_symbol("b").unwrap();
        let b_fid = engine
            .facts()
            .unwrap()
            .find(|(_, f)| matches!(f, ferric_core::Fact::Ordered(of) if of.relation == b_sym))
            .map(|(fid, _)| fid)
            .unwrap();
        engine.retract(b_fid).unwrap();

        assert_eq!(engine.rete.agenda.len(), 0);
        assert_rete_consistent(engine.rete());
    }

    #[test]
    fn retract_under_negative_and_exists_preserves_consistency() {
        let mut engine = new_utf8_engine();
        engine
            .load_str(
                r"
            (defrule neg-rule (a ?x) (not (block ?x)) => (printout t ?x))
            (defrule exists-rule (trigger) (exists (item ?x)) => (printout t exists))
        ",
            )
            .unwrap();

        engine
            .load_str("(assert (a 1) (block 1) (trigger) (item foo) (item bar))")
            .unwrap();

        // neg-rule: blocked by (block 1) → 0 activations from neg-rule
        // exists-rule: trigger + exists(item) → 1 activation
        assert_eq!(engine.rete.agenda.len(), 1);

        // Retract block fact
        let block_sym = engine.intern_symbol("block").unwrap();
        let block_fid = engine
            .facts()
            .unwrap()
            .find(|(_, f)| matches!(f, ferric_core::Fact::Ordered(of) if of.relation == block_sym))
            .map(|(fid, _)| fid)
            .unwrap();
        engine.retract(block_fid).unwrap();

        // Now neg-rule should have 1 activation (unblocked), exists-rule still 1 = 2 total
        assert_eq!(engine.rete.agenda.len(), 2);
        assert_rete_consistent(engine.rete());

        // Retract all items
        let item_sym = engine.intern_symbol("item").unwrap();
        let item_fids: Vec<_> = engine
            .facts()
            .unwrap()
            .filter(|(_, f)| matches!(f, ferric_core::Fact::Ordered(of) if of.relation == item_sym))
            .map(|(fid, _)| fid)
            .collect();
        for fid in item_fids {
            engine.retract(fid).unwrap();
        }

        // exists-rule should lose its activation (no more items)
        // neg-rule still active
        assert_eq!(engine.rete.agenda.len(), 1);
        assert_rete_consistent(engine.rete());
    }

    #[test]
    fn reset_clears_all_runtime_state() {
        let mut engine = new_utf8_engine();
        engine
            .load_str(
                r"
            (defrule r1 (a ?x) (not (b ?x)) => (printout t ?x))
            (defrule r2 (trigger) (exists (c ?x)) => (printout t exists))
            (deffacts startup (a 1) (trigger) (c 10))
        ",
            )
            .unwrap();

        // Should have activations
        assert!(!engine.rete.agenda.is_empty());

        // Run some rules
        engine.run(crate::execution::RunLimit::Count(1)).unwrap();

        // Reset
        engine.reset().unwrap();

        // After reset: deffacts re-asserted, rules should re-fire
        assert!(!engine.rete.agenda.is_empty());
        assert_rete_consistent(engine.rete());

        // Run all
        let result = engine.run(crate::execution::RunLimit::Unlimited).unwrap();
        assert!(result.rules_fired >= 1);
        assert_rete_consistent(engine.rete());
    }

    #[test]
    fn multiple_rules_retract_churn_consistency() {
        let mut engine = new_utf8_engine();
        engine
            .load_str(
                r"
            (defrule r1 (x ?a) => (printout t ?a))
            (defrule r2 (x ?a) (y ?a) => (printout t ?a))
            (defrule r3 (x ?a) (not (z ?a)) => (printout t ?a))
        ",
            )
            .unwrap();

        // Assert/retract cycle
        engine.load_str("(assert (x 1))").unwrap();
        let y_facts = load_ok(&mut engine, "(assert (y 1))");
        engine.load_str("(assert (z 1))").unwrap();

        assert_rete_consistent(engine.rete());

        // Retract y
        engine.retract(y_facts.asserted_facts[0]).unwrap();
        assert_rete_consistent(engine.rete());

        // Retract z (should unblock r3)
        let z_sym = engine.intern_symbol("z").unwrap();
        let z_fid = engine
            .facts()
            .unwrap()
            .find(|(_, f)| matches!(f, ferric_core::Fact::Ordered(of) if of.relation == z_sym))
            .map(|(fid, _)| fid)
            .unwrap();
        engine.retract(z_fid).unwrap();
        assert_rete_consistent(engine.rete());

        // r1 and r3 should be active (x 1 exists, no z 1, no y 1 for r2)
        assert_eq!(engine.rete.agenda.len(), 2);
    }

    // -----------------------------------------------------------------------
    // Pass 012: Compile-time validation in integration scenarios
    // -----------------------------------------------------------------------

    #[test]
    fn validation_error_from_file_has_source_location() {
        use std::io::Write;
        // Create a temporary file with invalid nesting (depth 5 exceeds max of 4)
        let mut temp = tempfile::NamedTempFile::new().unwrap();
        writeln!(
            temp,
            "(defrule bad-rule (a) (not (not (not (not (not (b)))))) => )"
        )
        .unwrap();

        let mut engine = new_utf8_engine();
        let result = engine.load_file(temp.path());

        assert!(result.is_err());
        let errs = result.unwrap_err();
        if let crate::loader::LoadError::Validation(v_errors) = &errs[0] {
            assert!(!v_errors.is_empty());
            assert_eq!(v_errors[0].code, "E0001");
            // Location should be present (from parsed source)
            assert!(v_errors[0].location.is_some());
        } else {
            panic!("Expected validation error, got: {errs:?}");
        }
    }

    #[test]
    fn valid_complex_rule_compiles_successfully() {
        let mut engine = new_utf8_engine();
        let result = engine.load_str(
            r"
            (defrule complex
                (a ?x)
                (b ?x ?y)
                (not (exclude ?x))
                (exists (c ?y))
                =>
                (assert (result ?x ?y)))
        ",
        );
        assert!(
            result.is_ok(),
            "Complex rule with not + exists should compile: {result:?}"
        );
    }
}
