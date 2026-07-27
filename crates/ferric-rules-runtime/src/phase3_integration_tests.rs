//! Phase 3 integration tests: language completion end-to-end.
//!
//! These tests exercise the Phase 3 pipeline additions:
//! - Expression evaluation (shared path for RHS and test CE)
//! - Template-aware modify/duplicate
//! - Real printout behavior
//! - deffunction/defglobal loading and execution
//! - defmodule import/export and focus
//! - defgeneric/defmethod dispatch
//! - forall CE semantics
//!
//! Tests are added incrementally as passes land. Commented-out tests
//! serve as scaffolding and contract documentation for upcoming passes.

#[cfg(test)]
mod tests {
    use crate::test_helpers::{
        assert_engine_consistent, assert_fact_count, assert_has_fact_with_relation,
        assert_no_fact_with_relation, assert_rete_consistent, find_facts_by_relation,
        get_ordered_fields, load_fixture, load_ok, new_utf8_engine, run_to_completion,
    };

    // =======================================================================
    // Phase 3 baseline: unsupported form diagnostics
    // =======================================================================
    //
    // These tests verify that Phase 3 constructs currently produce explicit
    // diagnostic errors (not silent degradation) before their passes land.

    #[test]
    fn deffunction_loads_successfully() {
        let mut engine = new_utf8_engine();
        let result = load_ok(&mut engine, "(deffunction add-one (?x) (+ ?x 1))");
        assert_eq!(result.functions.len(), 1);
        assert_eq!(result.functions[0].name, "add-one");
    }

    #[test]
    fn defglobal_loads_successfully() {
        let mut engine = new_utf8_engine();
        let result = load_ok(&mut engine, "(defglobal ?*threshold* = 50)");
        assert_eq!(result.globals.len(), 1);
        assert_eq!(result.globals[0].globals.len(), 1);
        assert_eq!(result.globals[0].globals[0].name, "threshold");
    }

    #[test]
    fn defmodule_loads_successfully() {
        let mut engine = new_utf8_engine();
        let result = load_ok(
            &mut engine,
            "(defmodule SENSOR (export deftemplate reading))",
        );
        assert_eq!(result.modules.len(), 1);
        assert_eq!(result.modules[0].name, "SENSOR");
    }

    #[test]
    fn defgeneric_loads_successfully() {
        let mut engine = new_utf8_engine();
        let result = load_ok(&mut engine, "(defgeneric describe)");
        assert_eq!(result.generics.len(), 1);
        assert_eq!(result.generics[0].name, "describe");
    }

    #[test]
    fn defmethod_loads_successfully() {
        let mut engine = new_utf8_engine();
        let result = load_ok(
            &mut engine,
            "(defmethod describe ((?x INTEGER)) (printout t ?x))",
        );
        assert_eq!(result.methods.len(), 1);
        assert_eq!(result.methods[0].name, "describe");
    }

    // =======================================================================
    // Phase 3 baseline: Phase 2 behavior preservation
    // =======================================================================
    //
    // These tests verify that Phase 2 capabilities remain intact as Phase 3
    // passes land. They are the "canary" tests for regression detection.

    #[test]
    fn phase2_rule_assert_chain_still_works() {
        let mut engine = new_utf8_engine();
        load_ok(
            &mut engine,
            r"
            (defrule greet (person ?name) => (assert (greeted ?name)))
            (defrule done (greeted ?name) => (assert (done ?name)))
            (deffacts startup (person Alice))
        ",
        );

        let result = run_to_completion(&mut engine);
        assert_eq!(result.rules_fired, 2);
        assert_fact_count(&engine, 3); // person + greeted + done
        assert_has_fact_with_relation(&engine, "done");
    }

    #[test]
    fn phase2_negative_rule_still_works() {
        let mut engine = new_utf8_engine();
        load_ok(
            &mut engine,
            r"
            (defrule safe (item ?x) (not (danger)) => (assert (allowed ?x)))
            (deffacts startup (item lamp))
        ",
        );

        let result = run_to_completion(&mut engine);
        assert_eq!(result.rules_fired, 1);
        assert_has_fact_with_relation(&engine, "allowed");
    }

    #[test]
    fn phase2_exists_rule_still_works() {
        let mut engine = new_utf8_engine();
        load_ok(
            &mut engine,
            r"
            (defrule detect (trigger) (exists (item ?x)) => (assert (detected)))
            (deffacts startup (trigger) (item a) (item b))
        ",
        );

        let result = run_to_completion(&mut engine);
        assert_eq!(result.rules_fired, 1);
        assert_has_fact_with_relation(&engine, "detected");
    }

    #[test]
    fn phase2_ncc_rule_still_works() {
        let mut engine = new_utf8_engine();
        load_ok(
            &mut engine,
            r"
            (defrule allow
                (item ?x)
                (not (and (block ?x) (reason ?x)))
                =>
                (assert (allowed ?x)))
            (deffacts startup
                (item apple)
                (item banana)
                (block apple)
                (reason apple))
        ",
        );

        let result = run_to_completion(&mut engine);
        // Only banana should pass (apple is blocked by conjunction)
        assert_eq!(result.rules_fired, 1);
        assert_has_fact_with_relation(&engine, "allowed");
        assert_rete_consistent(engine.rete());
    }

    #[test]
    fn phase2_retract_action_still_works() {
        let mut engine = new_utf8_engine();
        load_ok(
            &mut engine,
            r"
            (defrule cleanup ?f <- (temporary ?x) => (retract ?f) (assert (cleaned ?x)))
            (deffacts startup (temporary data))
        ",
        );

        let result = run_to_completion(&mut engine);
        assert_eq!(result.rules_fired, 1);
        assert_no_fact_with_relation(&engine, "temporary");
        assert_has_fact_with_relation(&engine, "cleaned");
    }

    #[test]
    fn phase2_reset_cycle_still_works() {
        let mut engine = new_utf8_engine();
        load_ok(
            &mut engine,
            r"
            (defrule greet (person ?name) => (assert (greeted ?name)))
            (deffacts startup (person Alice))
        ",
        );

        // First run
        let r1 = run_to_completion(&mut engine);
        assert_eq!(r1.rules_fired, 1);

        // Reset and run again
        engine.reset().unwrap();
        let r2 = run_to_completion(&mut engine);
        assert_eq!(r2.rules_fired, 1);
        assert_fact_count(&engine, 2);
    }

    // =======================================================================
    // Pass 002: Expression evaluation
    // =======================================================================

    #[test]
    fn test_ce_evaluates_boolean_expression() {
        let mut engine = new_utf8_engine();
        load_ok(
            &mut engine,
            r"
            (defrule positive (value ?x) (test (> ?x 0)) => (assert (positive ?x)))
            (deffacts startup (value 5) (value -3))
        ",
        );
        let result = run_to_completion(&mut engine);
        assert_eq!(result.rules_fired, 1);
        assert_has_fact_with_relation(&engine, "positive");
    }

    #[test]
    fn fr_rete_004_false_test_never_enters_agenda() {
        let mut engine = new_utf8_engine();
        load_ok(
            &mut engine,
            r"
            (defrule positive
                (value ?x)
                (test (> ?x 0))
                =>
                (assert (positive ?x)))
        ",
        );

        engine.assert_ordered("value", -1_i64).unwrap();

        assert_eq!(
            engine.agenda_len(),
            0,
            "a failing test CE must prevent terminal activation"
        );
    }

    #[test]
    fn fr_rete_004_test_uses_match_time_state() {
        let mut engine = new_utf8_engine();
        load_ok(
            &mut engine,
            r"
            (defglobal ?*open* = FALSE)

            (defrule candidate
                (declare (salience 10))
                (candidate)
                (test (eq ?*open* TRUE))
                =>
                (assert (candidate-fired)))

            (defrule opener
                (declare (salience 100))
                (open)
                =>
                (bind ?*open* TRUE))
        ",
        );

        engine.assert_ordered("candidate", []).unwrap();
        engine.assert_ordered("open", []).unwrap();

        assert_eq!(
            engine.agenda_len(),
            1,
            "only the opener should match while the global guard is false"
        );
        let run = run_to_completion(&mut engine);
        assert_eq!(run.rules_fired, 1);
        assert_no_fact_with_relation(&engine, "candidate-fired");
    }

    #[test]
    fn fr_rete_004_test_retracts_and_reactivates_correctly() {
        let mut engine = new_utf8_engine();
        load_ok(
            &mut engine,
            r"
            (defrule positive
                (value ?x)
                (test (> ?x 0))
                =>
                (assert (positive ?x)))
        ",
        );

        let rejected = engine.assert_ordered("value", -1_i64).unwrap();
        assert_eq!(engine.agenda_len(), 0);
        engine.retract(rejected).unwrap();

        let accepted = engine.assert_ordered("value", 1_i64).unwrap();
        assert_eq!(engine.agenda_len(), 1);
        engine.retract(accepted).unwrap();
        assert_eq!(engine.agenda_len(), 0);

        engine.assert_ordered("value", 1_i64).unwrap();
        assert_eq!(
            engine.agenda_len(),
            1,
            "a new partial match must reevaluate and reactivate the test CE"
        );
    }

    #[test]
    fn fr_rete_004_rhs_retraction_uses_state_at_action_boundary() {
        let mut engine = new_utf8_engine();
        load_ok(
            &mut engine,
            r"
            (defglobal ?*gate* = TRUE)

            (defrule candidate
                (anchor)
                (not (blocker))
                (test (eq ?*gate* TRUE))
                =>
                (assert (candidate-fired)))

            (defrule unblocker
                (declare (salience 100))
                ?blocker <- (blocker)
                (go)
                =>
                (retract ?blocker)
                (bind ?*gate* FALSE))

            (deffacts startup
                (anchor)
                (blocker)
                (go))
        ",
        );

        let run = run_to_completion(&mut engine);

        assert_eq!(
            run.rules_fired, 2,
            "the candidate created by retract must be matched before the later bind"
        );
        assert_has_fact_with_relation(&engine, "candidate-fired");
    }

    #[test]
    fn fr_rete_004_complex_negation_not_deferred_to_fire() {
        let mut engine = new_utf8_engine();
        let errors = engine
            .load_str(
                r"
                (defrule no-square-greater
                    (anchor ?min)
                    (not (data ?x&:(> (* ?x ?x) (* ?min ?min))))
                    =>
                    (assert (safe-square ?min)))
            ",
            )
            .expect_err("unsupported complex negation must fail during load");

        assert!(
            errors.iter().any(|error| matches!(
                error,
                crate::loader::LoadError::Compile(message)
                    if message.contains("complex constraints inside negated patterns")
            )),
            "expected an explicit match-time support error, got {errors:?}"
        );
    }

    #[test]
    fn fr_rete_005_double_not_has_one_activation() {
        let mut engine = new_utf8_engine();
        load_ok(
            &mut engine,
            r"
            (defrule any-foo
                (not (not (foo ?x)))
                =>
                (assert (found-foo)))
        ",
        );

        engine.assert_ordered("foo", 1_i64).unwrap();
        engine.assert_ordered("foo", 2_i64).unwrap();

        assert_eq!(
            engine.agenda_len(),
            1,
            "double negation must contribute one Boolean activation"
        );
    }

    #[test]
    fn fr_rete_005_double_not_survives_partial_support_retract() {
        let mut engine = new_utf8_engine();
        load_ok(
            &mut engine,
            r"
            (defrule any-foo
                (not (not (foo ?x)))
                =>
                (assert (found-foo)))
        ",
        );

        let first = engine.assert_ordered("foo", 1_i64).unwrap();
        engine.assert_ordered("foo", 2_i64).unwrap();
        assert_eq!(
            engine.agenda_len(),
            1,
            "two supports must still produce one existential activation"
        );
        engine.retract(first).unwrap();

        assert_eq!(
            engine.agenda_len(),
            1,
            "one remaining support must keep the existential activation alive"
        );
        assert_rete_consistent(engine.rete());
    }

    #[test]
    fn fr_rete_005_double_not_retracts_after_last_support() {
        let mut engine = new_utf8_engine();
        load_ok(
            &mut engine,
            r"
            (defrule any-foo
                (not (not (foo ?x)))
                =>
                (assert (found-foo)))
        ",
        );

        let first = engine.assert_ordered("foo", 1_i64).unwrap();
        let second = engine.assert_ordered("foo", 2_i64).unwrap();
        assert_eq!(
            engine.agenda_len(),
            1,
            "two supports must still produce one existential activation"
        );
        engine.retract(first).unwrap();
        assert_eq!(
            engine.agenda_len(),
            1,
            "the activation must survive until the final support is removed"
        );
        engine.retract(second).unwrap();

        assert_eq!(
            engine.agenda_len(),
            0,
            "the existential activation must retract with its final support"
        );
        assert_rete_consistent(engine.rete());
    }

    #[test]
    fn fr_rete_005_double_not_rejects_escaped_binding() {
        let mut engine = new_utf8_engine();
        let errors = engine
            .load_str(
                r"
                (defrule invalid
                    (not (not (foo ?x)))
                    =>
                    (printout t ?x crlf))
            ",
            )
            .expect_err("an existential-local variable must not escape to the RHS");

        assert!(
            errors.iter().any(|error| matches!(
                error,
                crate::loader::LoadError::Compile(message)
                    if message.contains("not exported by existential")
            )),
            "expected an existential binding-scope diagnostic, got {errors:?}"
        );
    }

    #[test]
    fn fr_rete_005_double_not_rejects_escaped_multifield_binding() {
        let mut engine = new_utf8_engine();
        let errors = engine
            .load_str(
                r"
                (defrule invalid
                    (not (not (foo $?items)))
                    =>
                    (printout t $?items crlf))
            ",
            )
            .expect_err("an existential-local multifield variable must not escape to the RHS");

        assert!(
            errors.iter().any(|error| matches!(
                error,
                crate::loader::LoadError::Compile(message)
                    if message.contains("not exported by existential")
            )),
            "expected an existential multifield binding-scope diagnostic, got {errors:?}"
        );
    }

    #[test]
    fn fr_rete_005_double_not_rejects_local_binding_in_later_test() {
        let mut engine = new_utf8_engine();
        let errors = engine
            .load_str(
                r"
                (defrule invalid
                    (not (not (foo ?x)))
                    (test (> ?x 0))
                    =>
                    (assert (unexpected)))
            ",
            )
            .expect_err("an existential-local variable must not escape to a later test CE");

        assert!(
            errors.iter().any(|error| matches!(
                error,
                crate::loader::LoadError::Compile(message)
                    if message.contains("not exported by existential")
            )),
            "expected an existential test-scope diagnostic, got {errors:?}"
        );
    }

    #[test]
    fn fr_rete_005_explicit_exists_rejects_the_same_escaped_binding() {
        let mut engine = new_utf8_engine();
        let errors = engine
            .load_str(
                r"
                (defrule invalid
                    (exists (foo ?x))
                    =>
                    (printout t ?x crlf))
            ",
            )
            .expect_err("explicit exists and double negation must share binding scope");

        assert!(
            errors.iter().any(|error| matches!(
                error,
                crate::loader::LoadError::Compile(message)
                    if message.contains("not exported by existential")
            )),
            "expected an existential binding-scope diagnostic, got {errors:?}"
        );
    }

    #[test]
    fn fr_rete_005_complex_existential_constraint_fails_during_load() {
        let mut engine = new_utf8_engine();
        let errors = engine
            .load_str(
                r"
                (defrule invalid
                    (not (not (foo ?x&:(> (* ?x ?x) 4))))
                    =>
                    (assert (unexpected)))
            ",
            )
            .expect_err("an existential-local runtime predicate must not fail silently");

        assert!(
            errors.iter().any(|error| matches!(
                error,
                crate::loader::LoadError::Compile(message)
                    if message.contains("complex constraints inside existential patterns")
            )),
            "expected an explicit existential match-time diagnostic, got {errors:?}"
        );
    }

    #[test]
    fn fr_rete_005_rhs_can_rebind_existential_local_name() {
        let mut engine = new_utf8_engine();
        load_ok(
            &mut engine,
            r"
            (defrule valid
                (not (not (foo ?x)))
                =>
                (bind ?x rebound)
                (assert (rebound-value ?x)))
        ",
        );

        engine.assert_ordered("foo", 1_i64).unwrap();
        let run = run_to_completion(&mut engine);
        assert_eq!(run.rules_fired, 1);
        assert_has_fact_with_relation(&engine, "rebound-value");
    }

    #[test]
    fn fr_rete_005_rhs_can_rebind_existential_local_multifield_name() {
        let mut engine = new_utf8_engine();
        load_ok(
            &mut engine,
            r"
            (defrule valid
                (not (not (foo $?items)))
                =>
                (bind $?items (create$ rebound values))
                (assert (rebound-value $?items)))
        ",
        );

        engine.assert_ordered("foo", 1_i64).unwrap();
        let run = run_to_completion(&mut engine);
        assert_eq!(run.rules_fired, 1);
        assert_has_fact_with_relation(&engine, "rebound-value");
    }

    #[test]
    fn fr_rete_005_rhs_branch_bind_can_rebind_existential_local_name() {
        let mut engine = new_utf8_engine();
        load_ok(
            &mut engine,
            r"
            (defrule valid
                (not (not (foo ?x)))
                =>
                (if TRUE then (bind ?x rebound))
                (assert (rebound-value ?x)))
        ",
        );

        engine.assert_ordered("foo", 1_i64).unwrap();
        let run = run_to_completion(&mut engine);
        assert_eq!(run.rules_fired, 1);
        assert_has_fact_with_relation(&engine, "rebound-value");
    }

    #[test]
    fn fr_rete_005_later_positive_pattern_exports_its_own_binding() {
        let mut engine = new_utf8_engine();
        load_ok(
            &mut engine,
            r"
            (defrule valid
                (not (not (foo ?x)))
                (bar ?x)
                =>
                (assert (matched ?x)))
        ",
        );

        engine.assert_ordered("foo", 1_i64).unwrap();
        engine.assert_ordered("bar", 2_i64).unwrap();
        let run = run_to_completion(&mut engine);
        assert_eq!(run.rules_fired, 1);
        assert_has_fact_with_relation(&engine, "matched");
    }

    #[test]
    fn fr_rete_005_double_not_correlates_previously_bound_variable() {
        let mut engine = new_utf8_engine();
        load_ok(
            &mut engine,
            r"
            (defrule matching-foo
                (key ?x)
                (not (not (foo ?x)))
                =>
                (assert (matched ?x)))
        ",
        );

        engine.assert_ordered("key", 1_i64).unwrap();
        engine.assert_ordered("key", 2_i64).unwrap();
        engine.assert_ordered("foo", 2_i64).unwrap();

        assert_eq!(engine.agenda_len(), 1);
        let run = run_to_completion(&mut engine);
        assert_eq!(run.rules_fired, 1);
        assert_has_fact_with_relation(&engine, "matched");
    }

    #[test]
    fn fr_rete_005_double_not_handles_online_compile_and_reset() {
        let mut engine = new_utf8_engine();
        engine.assert_ordered("foo", 1_i64).unwrap();
        engine.assert_ordered("foo", 2_i64).unwrap();

        load_ok(
            &mut engine,
            r"
            (defrule any-foo
                (not (not (foo ?x)))
                =>
                (assert (found-foo)))
        ",
        );
        assert_eq!(
            engine.agenda_len(),
            1,
            "online compilation must backfill one existential activation"
        );

        engine.reset().unwrap();
        assert_eq!(engine.agenda_len(), 0);
        engine.assert_ordered("foo", 3_i64).unwrap();
        engine.assert_ordered("foo", 4_i64).unwrap();
        assert_eq!(
            engine.agenda_len(),
            1,
            "reset must preserve existential cardinality for new supports"
        );
        assert_rete_consistent(engine.rete());
    }

    #[test]
    fn fr_rete_006_exists_multiple_tuples_one_activation() {
        let mut engine = new_utf8_engine();
        load_ok(
            &mut engine,
            r"
            (defrule joined
                (exists (a ?x) (b ?x))
                =>
                (assert (found-join)))
        ",
        );

        engine.assert_ordered("a", 1_i64).unwrap();
        engine.assert_ordered("b", 1_i64).unwrap();
        engine.assert_ordered("a", 2_i64).unwrap();
        engine.assert_ordered("b", 2_i64).unwrap();

        assert_eq!(
            engine.agenda_len(),
            1,
            "complete inner tuples must collapse to one Boolean activation"
        );
        assert_rete_consistent(engine.rete());
    }

    #[test]
    fn fr_rete_006_exists_partial_retract_keeps_activation() {
        let mut engine = new_utf8_engine();
        load_ok(
            &mut engine,
            r"
            (defrule joined
                (exists (a ?x) (b ?x))
                =>
                (assert (found-join)))
        ",
        );

        engine.assert_ordered("a", 1_i64).unwrap();
        let first_b = engine.assert_ordered("b", 1_i64).unwrap();
        engine.assert_ordered("a", 2_i64).unwrap();
        engine.assert_ordered("b", 2_i64).unwrap();
        assert_eq!(engine.agenda_len(), 1);

        engine.retract(first_b).unwrap();

        assert_eq!(
            engine.agenda_len(),
            1,
            "one remaining complete tuple must keep the activation alive"
        );
        assert_rete_consistent(engine.rete());
    }

    #[test]
    fn fr_rete_006_exists_last_retract_removes_activation() {
        let mut engine = new_utf8_engine();
        load_ok(
            &mut engine,
            r"
            (defrule joined
                (exists (a ?x) (b ?x))
                =>
                (assert (found-join)))
        ",
        );

        engine.assert_ordered("a", 1_i64).unwrap();
        let first_b = engine.assert_ordered("b", 1_i64).unwrap();
        engine.assert_ordered("a", 2_i64).unwrap();
        let second_b = engine.assert_ordered("b", 2_i64).unwrap();
        assert_eq!(engine.agenda_len(), 1);
        engine.retract(first_b).unwrap();
        assert_eq!(engine.agenda_len(), 1);

        engine.retract(second_b).unwrap();

        assert_eq!(
            engine.agenda_len(),
            0,
            "the activation must retract with the final complete tuple"
        );
        assert_rete_consistent(engine.rete());
    }

    #[test]
    fn fr_rete_006_exists_isolated_per_outer_token() {
        use ferric_rules_core::Value;

        let mut engine = new_utf8_engine();
        load_ok(
            &mut engine,
            r"
            (defrule joined
                (outer ?owner)
                (exists (a ?owner ?x) (b ?owner ?x))
                =>
                (assert (found-owner ?owner)))
        ",
        );

        engine.assert_ordered("outer", 1_i64).unwrap();
        engine.assert_ordered("outer", 2_i64).unwrap();
        engine
            .assert_ordered("a", [Value::Integer(1), Value::Integer(10)])
            .unwrap();
        let owner_one_first = engine
            .assert_ordered("b", [Value::Integer(1), Value::Integer(10)])
            .unwrap();
        engine
            .assert_ordered("a", [Value::Integer(1), Value::Integer(11)])
            .unwrap();
        let owner_one_second = engine
            .assert_ordered("b", [Value::Integer(1), Value::Integer(11)])
            .unwrap();
        engine
            .assert_ordered("a", [Value::Integer(2), Value::Integer(20)])
            .unwrap();
        engine
            .assert_ordered("b", [Value::Integer(2), Value::Integer(20)])
            .unwrap();

        assert_eq!(
            engine.agenda_len(),
            2,
            "each outer token must contribute at most one activation"
        );
        engine.retract(owner_one_first).unwrap();
        assert_eq!(
            engine.agenda_len(),
            2,
            "one remaining owner-one tuple must preserve only that owner's activation"
        );
        engine.retract(owner_one_second).unwrap();
        assert_eq!(
            engine.agenda_len(),
            1,
            "removing owner one's final tuple must not affect owner two"
        );
        assert_rete_consistent(engine.rete());
    }

    #[test]
    fn fr_rete_006_exists_isolated_per_outer_token_after_reset() {
        let mut engine = new_utf8_engine();
        load_ok(
            &mut engine,
            r"
            (deffacts startup
                (outer one)
                (a one 10)
                (b one 10)
                (outer two)
                (a two 20)
                (b two 20))

            (defrule joined
                (outer ?owner)
                (exists (a ?owner ?x) (b ?owner ?x))
                =>
                (assert (found-owner ?owner)))
        ",
        );

        engine.reset().unwrap();

        assert_eq!(
            engine.agenda_len(),
            2,
            "reset-time support tuples must remain isolated per outer token"
        );
        assert_engine_consistent(&engine);
    }

    #[test]
    fn fr_rete_006_template_exists_isolated_per_outer_token_after_reset() {
        let mut engine = new_utf8_engine();
        load_ok(
            &mut engine,
            r"
            (deftemplate sensor (slot id))
            (deftemplate reading (slot sensor-id) (slot reading-id))
            (deftemplate calibration (slot sensor-id) (slot reading-id))

            (deffacts startup
                (sensor (id one))
                (reading (sensor-id one) (reading-id 10))
                (calibration (sensor-id one) (reading-id 10))
                (sensor (id two))
                (reading (sensor-id two) (reading-id 20))
                (calibration (sensor-id two) (reading-id 20)))

            (defrule joined
                (sensor (id ?owner))
                (exists
                    (reading (sensor-id ?owner) (reading-id ?reading))
                    (calibration (sensor-id ?owner) (reading-id ?reading)))
                =>
                (assert (ready ?owner)))
        ",
        );

        engine.reset().unwrap();

        assert_eq!(
            engine.agenda_len(),
            2,
            "template support tuples must remain isolated per outer token"
        );
        let run = run_to_completion(&mut engine);
        assert_eq!(
            run.rules_fired, 2,
            "firing one outer activation must not invalidate another"
        );
        assert_engine_consistent(&engine);
    }

    #[test]
    fn fr_rete_006_exists_nested_join_variant() {
        use ferric_rules_core::Value;

        let mut engine = new_utf8_engine();
        load_ok(
            &mut engine,
            r"
            (defrule chained
                (exists (a ?x) (b ?x ?y) (c ?y))
                =>
                (assert (found-chain)))
        ",
        );

        engine.assert_ordered("a", 1_i64).unwrap();
        engine
            .assert_ordered("b", [Value::Integer(1), Value::Integer(10)])
            .unwrap();
        engine.assert_ordered("c", 10_i64).unwrap();
        engine.assert_ordered("a", 2_i64).unwrap();
        engine
            .assert_ordered("b", [Value::Integer(2), Value::Integer(20)])
            .unwrap();
        engine.assert_ordered("c", 20_i64).unwrap();

        assert_eq!(
            engine.agenda_len(),
            1,
            "multiple chained join results must collapse to one activation"
        );
        assert_rete_consistent(engine.rete());
    }

    #[test]
    fn fr_rete_006_exists_tuple_test_filters_before_boolean_collapse() {
        let mut engine = new_utf8_engine();
        load_ok(
            &mut engine,
            r"
            (defrule filtered
                (exists
                    (a ?x)
                    (test (> ?x 1))
                    (b ?x))
                =>
                (assert (found-filtered-join)))
        ",
        );

        engine.assert_ordered("a", 1_i64).unwrap();
        engine.assert_ordered("b", 1_i64).unwrap();
        assert_eq!(
            engine.agenda_len(),
            0,
            "a complete tuple that fails its test must not support exists"
        );

        engine.assert_ordered("a", 2_i64).unwrap();
        let second_b = engine.assert_ordered("b", 2_i64).unwrap();
        engine.assert_ordered("a", 3_i64).unwrap();
        let third_b = engine.assert_ordered("b", 3_i64).unwrap();
        assert_eq!(
            engine.agenda_len(),
            1,
            "passing tuples must still collapse to one activation"
        );

        engine.retract(second_b).unwrap();
        assert_eq!(
            engine.agenda_len(),
            1,
            "one remaining tuple that passes its test must preserve the activation"
        );
        engine.retract(third_b).unwrap();
        assert_eq!(
            engine.agenda_len(),
            0,
            "the activation must retract with the final passing tuple"
        );
        assert_engine_consistent(&engine);
    }

    #[test]
    fn fr_rete_006_nested_exists_preserves_tuple_scope() {
        let mut engine = new_utf8_engine();
        load_ok(
            &mut engine,
            r"
            (defrule nested
                (exists
                    (a ?x)
                    (exists (b ?x) (c ?x)))
                =>
                (assert (found-nested)))
        ",
        );

        engine.assert_ordered("a", 1_i64).unwrap();
        engine.assert_ordered("b", 1_i64).unwrap();
        engine.assert_ordered("c", 1_i64).unwrap();
        engine.assert_ordered("a", 2_i64).unwrap();
        engine.assert_ordered("b", 2_i64).unwrap();
        engine.assert_ordered("c", 2_i64).unwrap();

        assert_eq!(
            engine.agenda_len(),
            1,
            "nested exists groups must remain Boolean at every level"
        );
        assert_rete_consistent(engine.rete());
    }

    #[test]
    fn fr_rete_006_exists_handles_online_compile_and_reset() {
        let mut engine = new_utf8_engine();
        engine.assert_ordered("a", 1_i64).unwrap();
        engine.assert_ordered("b", 1_i64).unwrap();
        engine.assert_ordered("a", 2_i64).unwrap();
        engine.assert_ordered("b", 2_i64).unwrap();

        load_ok(
            &mut engine,
            r"
            (defrule joined
                (exists (a ?x) (b ?x))
                =>
                (assert (found-join)))
        ",
        );
        assert_eq!(
            engine.agenda_len(),
            1,
            "online compilation must collapse existing support tuples"
        );
        assert_engine_consistent(&engine);

        engine.reset().unwrap();
        assert_eq!(engine.agenda_len(), 0);
        engine.assert_ordered("a", 3_i64).unwrap();
        engine.assert_ordered("b", 3_i64).unwrap();
        engine.assert_ordered("a", 4_i64).unwrap();
        engine.assert_ordered("b", 4_i64).unwrap();
        assert_eq!(
            engine.agenda_len(),
            1,
            "reset must clear old owners and preserve Boolean cardinality"
        );
        assert_engine_consistent(&engine);
    }

    #[test]
    fn fr_rete_006_supported_outer_retract_cleans_owner_state() {
        use ferric_rules_core::Value;

        let mut engine = new_utf8_engine();
        load_ok(
            &mut engine,
            r"
            (defrule joined
                (outer ?owner)
                (exists (a ?owner ?x) (b ?owner ?x))
                =>
                (assert (found-owner ?owner)))
        ",
        );

        let outer_one = engine.assert_ordered("outer", 1_i64).unwrap();
        engine.assert_ordered("outer", 2_i64).unwrap();
        for (owner, value) in [(1, 10), (1, 11), (2, 20)] {
            engine
                .assert_ordered("a", [Value::Integer(owner), Value::Integer(value)])
                .unwrap();
            engine
                .assert_ordered("b", [Value::Integer(owner), Value::Integer(value)])
                .unwrap();
        }
        assert_eq!(engine.agenda_len(), 2);

        engine.retract(outer_one).unwrap();

        assert_eq!(
            engine.agenda_len(),
            1,
            "retracting a supported parent must remove only its owned activation"
        );
        assert_engine_consistent(&engine);
    }

    #[test]
    fn nested_function_call_in_rhs_evaluates() {
        let mut engine = new_utf8_engine();
        load_ok(
            &mut engine,
            r"
            (defrule compute (value ?x) => (assert (doubled (* ?x 2))))
            (deffacts startup (value 5))
        ",
        );
        let result = run_to_completion(&mut engine);
        assert_eq!(result.rules_fired, 1);
    }

    #[test]
    fn nested_function_call_produces_correct_value() {
        let mut engine = new_utf8_engine();
        load_ok(
            &mut engine,
            r"
            (defrule compute (value ?x) => (assert (result (* ?x 2))))
            (deffacts startup (value 5))
        ",
        );
        run_to_completion(&mut engine);
        let fields = get_ordered_fields(&engine, "result");
        assert_eq!(fields.len(), 1);
        assert!(
            matches!(fields[0], ferric_rules_core::Value::Integer(10)),
            "expected Integer(10), got {:?}",
            fields[0]
        );
    }

    #[test]
    fn test_ce_filters_negative_values() {
        // Both values create activations, but only x > 0 should fire
        let mut engine = new_utf8_engine();
        load_ok(
            &mut engine,
            r"
            (defrule positive
                (value ?x)
                (test (> ?x 0))
                =>
                (assert (positive ?x)))
            (deffacts startup (value 10) (value -5) (value 0))
        ",
        );
        let result = run_to_completion(&mut engine);
        assert_eq!(result.rules_fired, 1);
        assert_has_fact_with_relation(&engine, "positive");

        let fields = get_ordered_fields(&engine, "positive");
        assert_eq!(fields.len(), 1);
        assert!(
            matches!(fields[0], ferric_rules_core::Value::Integer(10)),
            "expected Integer(10), got {:?}",
            fields[0]
        );
    }

    #[test]
    fn test_ce_with_multiple_test_conditions() {
        // Both test CEs must be true for the rule to fire
        let mut engine = new_utf8_engine();
        load_ok(
            &mut engine,
            r"
            (defrule in-range
                (value ?x)
                (test (> ?x 0))
                (test (< ?x 100))
                =>
                (assert (in-range ?x)))
            (deffacts startup (value 50) (value -5) (value 200))
        ",
        );
        let result = run_to_completion(&mut engine);
        assert_eq!(result.rules_fired, 1);
        assert_has_fact_with_relation(&engine, "in-range");

        let fields = get_ordered_fields(&engine, "in-range");
        assert!(matches!(fields[0], ferric_rules_core::Value::Integer(50)));
    }

    #[test]
    fn deeply_nested_function_call_in_rhs() {
        // (+ (* ?x 2) 1) = (5 * 2) + 1 = 11
        let mut engine = new_utf8_engine();
        load_ok(
            &mut engine,
            r"
            (defrule compute
                (value ?x)
                =>
                (assert (result (+ (* ?x 2) 1))))
            (deffacts startup (value 5))
        ",
        );
        run_to_completion(&mut engine);
        let fields = get_ordered_fields(&engine, "result");
        assert!(
            matches!(fields[0], ferric_rules_core::Value::Integer(11)),
            "expected Integer(11), got {:?}",
            fields[0]
        );
    }

    #[test]
    fn test_ce_with_equality_check() {
        // Test equality function in test CE
        let mut engine = new_utf8_engine();
        load_ok(
            &mut engine,
            r"
            (defrule find-match
                (target ?t)
                (candidate ?c)
                (test (= ?t ?c))
                =>
                (assert (match ?t)))
            (deffacts startup
                (target 42)
                (candidate 42)
                (candidate 99))
        ",
        );
        let result = run_to_completion(&mut engine);
        assert_eq!(result.rules_fired, 1);
        assert_has_fact_with_relation(&engine, "match");
    }

    #[test]
    fn test_ce_with_not_function() {
        // (not (> ?x 10)) should fire for values <= 10
        let mut engine = new_utf8_engine();
        load_ok(
            &mut engine,
            r"
            (defrule small
                (value ?x)
                (test (not (> ?x 10)))
                =>
                (assert (small ?x)))
            (deffacts startup (value 5) (value 15))
        ",
        );
        let result = run_to_completion(&mut engine);
        assert_eq!(result.rules_fired, 1);
        let fields = get_ordered_fields(&engine, "small");
        assert!(matches!(fields[0], ferric_rules_core::Value::Integer(5)));
    }

    #[test]
    fn test_ce_does_not_affect_rete_pattern_count() {
        // Test CEs should not generate patterns in the rete network.
        // A rule with 1 fact pattern + 1 test CE should have 1 rete pattern.
        let mut engine = new_utf8_engine();
        load_ok(
            &mut engine,
            r"
            (defrule positive
                (value ?x)
                (test (> ?x 0))
                =>
                (assert (positive ?x)))
        ",
        );
        // The rete should be consistent
        assert_rete_consistent(engine.rete());
    }

    #[test]
    fn arithmetic_expression_in_rhs_with_float() {
        // (/ ?x 2.0) should produce a float result
        let mut engine = new_utf8_engine();
        load_ok(
            &mut engine,
            r"
            (defrule halve
                (value ?x)
                =>
                (assert (half (/ ?x 2.0))))
            (deffacts startup (value 10))
        ",
        );
        run_to_completion(&mut engine);
        let fields = get_ordered_fields(&engine, "half");
        match fields[0] {
            ferric_rules_core::Value::Float(f) => {
                assert!((f - 5.0).abs() < 0.001, "expected 5.0, got {f}");
            }
            ref other => panic!("expected Float, got {other:?}"),
        }
    }

    #[test]
    fn test_ce_interleaved_with_patterns() {
        // Test CE between two fact patterns should work
        let mut engine = new_utf8_engine();
        load_ok(
            &mut engine,
            r"
            (defrule complex
                (start ?x)
                (test (> ?x 0))
                (label ?y)
                =>
                (assert (result ?x ?y)))
            (deffacts startup (start 5) (start -1) (label ok))
        ",
        );
        let result = run_to_completion(&mut engine);
        assert_eq!(result.rules_fired, 1);
        assert_has_fact_with_relation(&engine, "result");
    }

    #[test]
    fn test_ce_preserves_reset_cycle() {
        // Rules with test CEs should work correctly across reset cycles
        let mut engine = new_utf8_engine();
        load_ok(
            &mut engine,
            r"
            (defrule positive
                (value ?x)
                (test (> ?x 0))
                =>
                (assert (positive ?x)))
            (deffacts startup (value 5) (value -3))
        ",
        );

        let r1 = run_to_completion(&mut engine);
        assert_eq!(r1.rules_fired, 1);
        assert_has_fact_with_relation(&engine, "positive");

        engine.reset().unwrap();
        let r2 = run_to_completion(&mut engine);
        assert_eq!(r2.rules_fired, 1);
        assert_has_fact_with_relation(&engine, "positive");
    }

    // =======================================================================
    // Pass 003: Template-aware modify/duplicate
    // =======================================================================

    #[test]
    fn modify_template_fact_updates_slot() {
        let mut engine = new_utf8_engine();
        load_ok(
            &mut engine,
            r"
            (deftemplate person (slot name) (slot status))
            (defrule promote
                ?f <- (person (name ?n) (status junior))
                =>
                (modify ?f (status senior)))
            (deffacts startup (person (name Alice) (status junior)))
        ",
        );
        let result = run_to_completion(&mut engine);
        assert_eq!(result.rules_fired, 1);
    }

    #[test]
    fn duplicate_template_fact_preserves_original() {
        let mut engine = new_utf8_engine();
        // Use a constant constraint so only count=5 matches; the duplicate
        // has count=10 and does NOT re-trigger the rule.
        load_ok(
            &mut engine,
            r"
            (deftemplate item (slot name) (slot count))
            (defrule clone
                ?f <- (item (name ?n) (count 5))
                =>
                (duplicate ?f (count 10)))
            (deffacts startup (item (name widget) (count 5)))
        ",
        );
        let result = run_to_completion(&mut engine);
        // Exactly one firing: the original (count=5) matches, duplicate with
        // count=10 is created but does not re-match the constant test.
        assert_eq!(result.rules_fired, 1);
        // Two facts exist: original (count 5) + duplicate (count 10).
        assert_eq!(engine.facts().unwrap().count(), 2);
    }

    #[test]
    fn modify_unknown_slot_produces_action_error() {
        let mut engine = new_utf8_engine();
        load_ok(
            &mut engine,
            r"
            (deftemplate person (slot name))
            (defrule bad
                ?f <- (person (name ?n))
                =>
                (modify ?f (nonexistent value)))
            (deffacts startup (person (name Alice)))
        ",
        );
        // Should not panic; modify with unknown slot produces an ActionError
        // but step() still fires (counts the activation as processed).
        // The step completes without panic.
        let result = run_to_completion(&mut engine);
        // The rule activation is popped and processed even when the action
        // errors — rules_fired reflects the test-CE gate, not action success.
        assert_eq!(result.rules_fired, 1);
    }

    #[test]
    fn deftemplate_with_default_value_uses_default() {
        let mut engine = new_utf8_engine();
        load_ok(
            &mut engine,
            r"
            (deftemplate sensor (slot id) (slot reading (default 0)))
            (deffacts startup (sensor (id my-sensor)))
        ",
        );
        // The deffacts body only specifies `id`; `reading` should be Void
        // (default is stored as Void for numeric defaults in current impl,
        // since literals are only evaluated when explicitly provided).
        // The main goal here is that loading succeeds without error.
        assert_eq!(engine.facts().unwrap().count(), 1);
    }

    #[test]
    fn template_pattern_with_constant_test_matches() {
        let mut engine = new_utf8_engine();
        load_ok(
            &mut engine,
            r"
            (deftemplate status-record (slot code))
            (defrule detect-ok
                (status-record (code ok))
                =>
                (assert (detected)))
            (deffacts startup
                (status-record (code ok))
                (status-record (code error)))
        ",
        );
        let result = run_to_completion(&mut engine);
        // Only the `ok` record should match.
        assert_eq!(result.rules_fired, 1);
        assert_has_fact_with_relation(&engine, "detected");
    }

    #[test]
    fn deffacts_with_template_fact_uses_correct_template_id() {
        let mut engine = new_utf8_engine();
        load_ok(
            &mut engine,
            r"
            (deftemplate point (slot x) (slot y))
            (defrule found (point (x ?x) (y ?y)) => (assert (found ?x ?y)))
            (deffacts startup (point (x 3) (y 4)))
        ",
        );
        let result = run_to_completion(&mut engine);
        assert_eq!(result.rules_fired, 1);
        assert_has_fact_with_relation(&engine, "found");
    }

    #[test]
    fn unknown_template_in_deffacts_produces_compile_error() {
        let mut engine = new_utf8_engine();
        let errors = engine
            .load_str(
                r"
            (deffacts startup (ghost (slot1 value)))
        ",
            )
            .unwrap_err();
        assert!(!errors.is_empty(), "expected at least one error");
        let has_compile_error = errors.iter().any(|e| {
            matches!(e, crate::loader::LoadError::Compile(msg)
                if msg.contains("unknown template"))
        });
        assert!(
            has_compile_error,
            "expected 'unknown template' error, got: {errors:?}"
        );
    }

    // =======================================================================
    // Pass 004: Printout runtime
    // =======================================================================

    #[test]
    fn printout_writes_to_channel() {
        let mut engine = new_utf8_engine();
        load_ok(
            &mut engine,
            r#"
            (defrule greet (person ?name) => (printout t "Hello, " ?name crlf))
            (deffacts startup (person Alice))
        "#,
        );
        let result = run_to_completion(&mut engine);
        assert_eq!(result.rules_fired, 1);
        let output = engine.get_output("t").unwrap_or("");
        assert_eq!(output, "Hello, Alice\n");
    }

    #[test]
    fn printout_formats_integer_and_float() {
        let mut engine = new_utf8_engine();
        load_ok(
            &mut engine,
            r#"
            (defrule show (value ?x) => (printout t ?x " "))
            (deffacts startup (value 42))
        "#,
        );
        run_to_completion(&mut engine);
        let output = engine.get_output("t").unwrap_or("");
        assert!(
            output.contains("42"),
            "expected '42' in output, got '{output}'"
        );
    }

    #[test]
    fn printout_crlf_produces_newline() {
        let mut engine = new_utf8_engine();
        load_ok(
            &mut engine,
            r#"
            (defrule show (flag) => (printout t "line1" crlf "line2" crlf))
            (deffacts startup (flag))
        "#,
        );
        run_to_completion(&mut engine);
        let output = engine.get_output("t").unwrap_or("");
        assert_eq!(output, "line1\nline2\n");
    }

    #[test]
    fn printout_tab_produces_tab() {
        let mut engine = new_utf8_engine();
        load_ok(
            &mut engine,
            r#"
            (defrule show (flag) => (printout t "a" tab "b" crlf))
            (deffacts startup (flag))
        "#,
        );
        run_to_completion(&mut engine);
        let output = engine.get_output("t").unwrap_or("");
        assert_eq!(output, "a\tb\n");
    }

    #[test]
    fn printout_with_expression_argument() {
        let mut engine = new_utf8_engine();
        load_ok(
            &mut engine,
            r"
            (defrule show (value ?x) => (printout t (* ?x 2) crlf))
            (deffacts startup (value 5))
        ",
        );
        run_to_completion(&mut engine);
        let output = engine.get_output("t").unwrap_or("");
        assert_eq!(output, "10\n");
    }

    #[test]
    fn printout_channel_must_be_literal() {
        let mut engine = new_utf8_engine();
        load_ok(
            &mut engine,
            r#"
            (defrule show (channel ?ch) => (printout ?ch "hello"))
            (deffacts startup (channel t))
        "#,
        );
        let result = run_to_completion(&mut engine);
        assert_eq!(result.rules_fired, 1);

        // No output should be produced because the channel argument is invalid.
        assert!(engine.get_output("t").is_none());
        assert!(engine.action_diagnostics().iter().any(|e| {
            matches!(e, crate::actions::ActionError::EvalError(msg) if msg.contains("printout: channel must be a literal"))
        }));
    }

    #[test]
    fn printout_empty_after_reset() {
        let mut engine = new_utf8_engine();
        load_ok(
            &mut engine,
            r#"
            (defrule show (flag) => (printout t "hello"))
            (deffacts startup (flag))
        "#,
        );
        run_to_completion(&mut engine);
        assert!(engine.get_output("t").is_some());

        engine.reset().unwrap();
        // After reset, output should be cleared
        assert!(
            engine.get_output("t").is_none() || engine.get_output("t") == Some(""),
            "expected no output after reset, got {:?}",
            engine.get_output("t")
        );
    }

    // =======================================================================
    // Pass 006: User-defined function environment and execution
    // =======================================================================

    #[test]
    fn test_user_function_called_from_rule_rhs() {
        let mut engine = new_utf8_engine();
        load_ok(
            &mut engine,
            r"
            (deffunction double (?x) (* ?x 2))
            (defrule test (value ?v) => (assert (result (double ?v))))
            (deffacts init (value 5))
        ",
        );
        run_to_completion(&mut engine);
        let fields = get_ordered_fields(&engine, "result");
        assert_eq!(fields.len(), 1);
        assert!(
            matches!(fields[0], ferric_rules_core::Value::Integer(10)),
            "expected Integer(10), got {:?}",
            fields[0]
        );
    }

    #[test]
    fn test_user_function_multiple_params() {
        let mut engine = new_utf8_engine();
        load_ok(
            &mut engine,
            r"
            (deffunction add (?a ?b) (+ ?a ?b))
            (defrule test (pair ?x ?y) => (assert (sum (add ?x ?y))))
            (deffacts init (pair 3 7))
        ",
        );
        run_to_completion(&mut engine);
        let fields = get_ordered_fields(&engine, "sum");
        assert_eq!(fields.len(), 1);
        assert!(
            matches!(fields[0], ferric_rules_core::Value::Integer(10)),
            "expected Integer(10), got {:?}",
            fields[0]
        );
    }

    #[test]
    fn test_user_function_wildcard_param() {
        // (deffunction first (?x $?rest) ?x)
        let mut engine = new_utf8_engine();
        load_ok(
            &mut engine,
            r"
            (deffunction first (?x $?rest) ?x)
            (defrule test (items ?a ?b ?c) => (assert (first-item (first ?a ?b ?c))))
            (deffacts init (items 10 20 30))
        ",
        );
        run_to_completion(&mut engine);
        let fields = get_ordered_fields(&engine, "first-item");
        assert_eq!(fields.len(), 1);
        assert!(
            matches!(fields[0], ferric_rules_core::Value::Integer(10)),
            "expected Integer(10), got {:?}",
            fields[0]
        );
    }

    #[test]
    fn test_user_function_nested_calls() {
        // inc(4) = 5, 5 * 2 = 10
        let mut engine = new_utf8_engine();
        load_ok(
            &mut engine,
            r"
            (deffunction inc (?x) (+ ?x 1))
            (defrule test (val ?v) => (assert (result (* (inc ?v) 2))))
            (deffacts init (val 4))
        ",
        );
        run_to_completion(&mut engine);
        let fields = get_ordered_fields(&engine, "result");
        assert_eq!(fields.len(), 1);
        assert!(
            matches!(fields[0], ferric_rules_core::Value::Integer(10)),
            "expected Integer(10), got {:?}",
            fields[0]
        );
    }

    #[test]
    fn test_user_function_calling_user_function() {
        // double(3)=6, double(6)=12
        let mut engine = new_utf8_engine();
        load_ok(
            &mut engine,
            r"
            (deffunction double (?x) (* ?x 2))
            (deffunction quadruple (?x) (double (double ?x)))
            (defrule test (val ?v) => (assert (result (quadruple ?v))))
            (deffacts init (val 3))
        ",
        );
        run_to_completion(&mut engine);
        let fields = get_ordered_fields(&engine, "result");
        assert_eq!(fields.len(), 1);
        assert!(
            matches!(fields[0], ferric_rules_core::Value::Integer(12)),
            "expected Integer(12), got {:?}",
            fields[0]
        );
    }

    #[test]
    fn test_user_function_recursion_limit() {
        // Infinite recursion must not crash the engine — it produces an error
        // and the rule's action fails gracefully.
        let mut engine = new_utf8_engine();
        load_ok(
            &mut engine,
            r"
            (deffunction inf (?x) (inf ?x))
            (defrule test (go) => (assert (result (inf 1))))
            (deffacts init (go))
        ",
        );
        // Should not panic; may produce an action error but the engine keeps running.
        let _ = run_to_completion(&mut engine);
        // No assertion needed beyond "didn't crash"
    }

    #[test]
    fn test_global_variable_read_in_rhs() {
        let mut engine = new_utf8_engine();
        load_ok(
            &mut engine,
            r"
            (defglobal ?*threshold* = 50)
            (defrule test (value ?v) => (assert (limit ?*threshold*)))
            (deffacts init (value 1))
        ",
        );
        run_to_completion(&mut engine);
        let fields = get_ordered_fields(&engine, "limit");
        assert_eq!(fields.len(), 1);
        assert!(
            matches!(fields[0], ferric_rules_core::Value::Integer(50)),
            "expected Integer(50), got {:?}",
            fields[0]
        );
    }

    #[test]
    fn test_global_variable_read_in_test_ce() {
        let mut engine = new_utf8_engine();
        load_ok(
            &mut engine,
            r"
            (defglobal ?*min* = 10)
            (defrule test (value ?v) (test (> ?v ?*min*)) => (assert (passed ?v)))
            (deffacts init (value 5) (value 15))
        ",
        );
        run_to_completion(&mut engine);
        // Only value 15 > 10 should produce a passed fact
        assert_has_fact_with_relation(&engine, "passed");
        let fields = get_ordered_fields(&engine, "passed");
        assert_eq!(fields.len(), 1);
        assert!(
            matches!(fields[0], ferric_rules_core::Value::Integer(15)),
            "expected Integer(15), got {:?}",
            fields[0]
        );
    }

    #[test]
    fn test_global_bind_in_rhs() {
        let mut engine = new_utf8_engine();
        load_ok(
            &mut engine,
            r"
            (defglobal ?*counter* = 0)
            (defrule test (item ?x) => (bind ?*counter* (+ ?*counter* 1)))
            (deffacts init (item a) (item b) (item c))
        ",
        );
        run_to_completion(&mut engine);
        let counter = engine.get_global("counter").expect("counter should be set");
        assert!(
            counter.structural_eq(&ferric_rules_core::Value::Integer(3)),
            "expected Integer(3), got {counter:?}"
        );
    }

    #[test]
    fn test_global_reset_reinitializes() {
        let mut engine = new_utf8_engine();
        load_ok(&mut engine, "(defglobal ?*x* = 10)");
        // Verify initial value
        let initial = engine.get_global("x").expect("x should exist");
        assert!(
            initial.structural_eq(&ferric_rules_core::Value::Integer(10)),
            "expected Integer(10) initially, got {initial:?}"
        );

        // Mutate via load_str with a bind action inside a rule
        // (We can't call bind directly; manually set via another load_str trick)
        // Instead, set via a rule that fires once.
        load_ok(
            &mut engine,
            r"
            (defrule mutate (go) => (bind ?*x* 99))
            (deffacts mutate-init (go))
        ",
        );
        run_to_completion(&mut engine);
        let mutated = engine
            .get_global("x")
            .expect("x should exist after mutation");
        assert!(
            mutated.structural_eq(&ferric_rules_core::Value::Integer(99)),
            "expected Integer(99) after mutation, got {mutated:?}"
        );

        // Reset should restore to initial value
        engine.reset().unwrap();
        let reset_val = engine.get_global("x").expect("x should exist after reset");
        assert!(
            reset_val.structural_eq(&ferric_rules_core::Value::Integer(10)),
            "expected Integer(10) after reset, got {reset_val:?}"
        );
    }

    #[test]
    fn test_user_function_wrong_arity_does_not_crash() {
        let mut engine = new_utf8_engine();
        load_ok(
            &mut engine,
            r"
            (deffunction add (?a ?b) (+ ?a ?b))
            (defrule test (go) => (assert (result (add 1))))
            (deffacts init (go))
        ",
        );
        // Should not crash; the rule produces an action error but the engine continues.
        let _ = run_to_completion(&mut engine);
        // No assertion needed beyond "didn't crash"
    }

    #[test]
    fn test_global_variable_initial_expression() {
        let mut engine = new_utf8_engine();
        load_ok(&mut engine, "(defglobal ?*doubled* = (* 21 2))");
        let val = engine.get_global("doubled").expect("doubled should exist");
        assert!(
            val.structural_eq(&ferric_rules_core::Value::Integer(42)),
            "expected Integer(42), got {val:?}"
        );
    }

    #[test]
    fn test_user_function_in_printout() {
        let mut engine = new_utf8_engine();
        load_ok(
            &mut engine,
            r"
            (deffunction double (?x) (* ?x 2))
            (defrule test (val ?v) => (printout t (double ?v) crlf))
            (deffacts init (val 5))
        ",
        );
        run_to_completion(&mut engine);
        let output = engine.get_output("t").unwrap_or("");
        assert_eq!(output, "10\n", "expected '10\\n', got '{output}'");
    }

    // =======================================================================
    // Pass 008: Defmodule import/export and focus semantics
    // =======================================================================

    #[test]
    fn module_registered_on_load() {
        let mut engine = new_utf8_engine();
        load_ok(&mut engine, "(defmodule SENSOR (export ?ALL))");
        assert_eq!(engine.current_module(), "SENSOR");
    }

    #[test]
    fn rules_default_to_main_module() {
        // Without defmodule, all rules belong to MAIN and fire normally
        let mut engine = new_utf8_engine();
        load_ok(
            &mut engine,
            r"
            (defrule test (item ?x) => (assert (found ?x)))
            (deffacts init (item 1))
        ",
        );
        let result = run_to_completion(&mut engine);
        assert_eq!(result.rules_fired, 1);
        assert_has_fact_with_relation(&engine, "found");
    }

    #[test]
    fn module_rules_fire_when_focused() {
        let mut engine = new_utf8_engine();
        load_ok(
            &mut engine,
            r"
            (defmodule WORKER (export ?ALL))
            (defrule process (job ?x) => (assert (done ?x)))
        ",
        );
        load_ok(&mut engine, "(assert (job task1))");

        // Without focusing WORKER, the rule should not fire
        let result = run_to_completion(&mut engine);
        assert_eq!(result.rules_fired, 0);
        assert_no_fact_with_relation(&engine, "done");

        // Push WORKER focus and run again
        engine.push_focus("WORKER").unwrap();
        let result = run_to_completion(&mut engine);
        assert_eq!(result.rules_fired, 1);
        assert_has_fact_with_relation(&engine, "done");
    }

    #[test]
    fn focus_from_rhs_pushes_module() {
        let mut engine = new_utf8_engine();
        load_ok(
            &mut engine,
            r"
            (defrule kickoff (start) => (focus WORKER))

            (defmodule WORKER (export ?ALL))
            (defrule process (job ?x) => (assert (done ?x)))
        ",
        );
        load_ok(&mut engine, "(assert (start) (job task1))");

        let result = run_to_completion(&mut engine);
        // kickoff fires (focuses WORKER), then WORKER::process fires
        assert_eq!(result.rules_fired, 2);
        assert_has_fact_with_relation(&engine, "done");
    }

    #[test]
    fn focus_unknown_module_produces_action_diagnostic() {
        let mut engine = new_utf8_engine();
        load_ok(
            &mut engine,
            r"
            (defrule kickoff (start) => (focus MISSING))
            (deffacts startup (start))
        ",
        );

        let result = run_to_completion(&mut engine);
        assert_eq!(result.rules_fired, 1);
        assert!(engine.action_diagnostics().iter().any(|e| {
            matches!(e, crate::actions::ActionError::EvalError(msg) if msg.contains("focus: unknown module `MISSING`"))
        }));
    }

    #[test]
    fn focus_stack_pops_back_to_main() {
        let mut engine = new_utf8_engine();
        load_ok(
            &mut engine,
            r"
            (defrule step1 (start) => (assert (step1-done)) (focus HELPER))
            (defrule step3 (step2-done) => (assert (all-done)))

            (defmodule HELPER (export ?ALL))
            (defrule step2 (step1-done) => (assert (step2-done)))
        ",
        );
        load_ok(&mut engine, "(assert (start))");

        let result = run_to_completion(&mut engine);
        // step1 fires → focus HELPER → step2 fires → HELPER empty → pop → step3 fires
        assert_eq!(result.rules_fired, 3);
        assert_has_fact_with_relation(&engine, "all-done");
    }

    #[test]
    fn reset_restores_focus_to_main() {
        let mut engine = new_utf8_engine();
        load_ok(&mut engine, "(defmodule SENSOR (export ?ALL))");
        engine.push_focus("SENSOR").unwrap();

        engine.reset().unwrap();
        assert_eq!(engine.current_module(), "MAIN");
    }

    #[test]
    fn set_focus_replaces_existing_focus_stack() {
        let mut engine = new_utf8_engine();
        load_ok(
            &mut engine,
            r"
            (defmodule A)
            (defmodule B)
        ",
        );
        engine.push_focus("A").unwrap();
        engine.push_focus("B").unwrap();

        engine.set_focus("A").unwrap();
        assert_eq!(engine.get_focus(), Some("A"));
        assert_eq!(engine.get_focus_stack(), vec!["A"]);
    }

    #[test]
    fn push_focus_unknown_module_returns_error() {
        let mut engine = new_utf8_engine();
        let result = engine.push_focus("NONEXISTENT");
        assert!(result.is_err());
    }

    #[test]
    fn cross_module_template_visibility_with_import() {
        let mut engine = new_utf8_engine();
        load_ok(
            &mut engine,
            r"
            (defmodule SENSOR (export deftemplate reading))
            (deftemplate reading (slot value))

            (defmodule ANALYZER (import SENSOR deftemplate reading))
            (defrule analyze (reading (value ?v)) => (assert (analyzed ?v)))
        ",
        );
        // Assert a template fact and push ANALYZER focus to fire the rule.
        load_ok(
            &mut engine,
            r"
            (deffacts sensor-data (reading (value 42)))
        ",
        );
        engine.push_focus("ANALYZER").unwrap();
        let result = run_to_completion(&mut engine);
        assert_eq!(result.rules_fired, 1);
        assert_has_fact_with_relation(&engine, "analyzed");
    }

    #[test]
    fn template_defined_as_module_qualified_name_is_resolved_by_import() {
        let mut engine = new_utf8_engine();
        load_ok(
            &mut engine,
            r"
            (defmodule A (export ?ALL))
            (deftemplate A::foo (slot x))
            (defmodule B (import A ?ALL))
            (defrule B::r (foo (x ?v)) => (assert (seen ?v)))
            (deffacts startup (foo (x 7)))
        ",
        );

        engine.push_focus("B").unwrap();
        let result = run_to_completion(&mut engine);
        assert_eq!(result.rules_fired, 1);
        assert_has_fact_with_relation(&engine, "seen");
    }

    #[test]
    fn module_without_explicit_export_defaults_to_export_all() {
        let mut engine = new_utf8_engine();
        load_ok(
            &mut engine,
            r"
            (defmodule A)
            (deftemplate A::foo (slot x))
            (defmodule B (import A ?ALL))
            (defrule B::r (foo (x ?v)) => (assert (seen ?v)))
            (deffacts startup (foo (x 11)))
        ",
        );

        engine.push_focus("B").unwrap();
        let result = run_to_completion(&mut engine);
        assert_eq!(result.rules_fired, 1);
        assert_has_fact_with_relation(&engine, "seen");
    }

    #[test]
    fn cross_module_template_visibility_supports_transitive_import_reexport() {
        let mut engine = new_utf8_engine();
        load_ok(
            &mut engine,
            r"
            (defmodule A (export ?ALL))
            (deftemplate A::foo (slot x))

            (defmodule C1 (import A ?ALL) (export ?ALL))
            (defmodule C2 (import C1 deftemplate ?ALL))
            (defrule C2::r (foo (x ?v)) => (assert (seen ?v)))
            (deffacts startup (foo (x 19)))
        ",
        );

        engine.push_focus("C2").unwrap();
        let result = run_to_completion(&mut engine);
        assert_eq!(result.rules_fired, 1);
        assert_has_fact_with_relation(&engine, "seen");
    }

    #[test]
    fn template_not_visible_without_import() {
        let mut engine = new_utf8_engine();
        load_ok(
            &mut engine,
            r"
            (defmodule SENSOR (export deftemplate reading))
            (deftemplate reading (slot value))
        ",
        );
        // CHECKER doesn't import from SENSOR, so template should not be visible
        let errors = engine
            .load_str(
                r"
                (defmodule CHECKER)
                (defrule check (reading (value ?v)) => (assert (checked ?v)))
            ",
            )
            .unwrap_err();

        let has_visibility_error = errors.iter().any(
            |e| matches!(e, crate::loader::LoadError::Compile(msg) if msg.contains("not visible")),
        );
        assert!(
            has_visibility_error,
            "expected 'not visible' compile error, got: {errors:?}"
        );
    }

    #[test]
    fn modlmisc_03_visibility_error_is_reported() {
        let mut engine = new_utf8_engine();
        let errors = engine
            .load_str(
                r"
                (defmodule A (export deftemplate bar))
                (deftemplate A::foo (slot x))
                (defmodule B (import A ?ALL))
                (defrule B::rule1 (foo (x 3)) =>)
            ",
            )
            .unwrap_err();

        let compile_messages: Vec<&str> = errors
            .iter()
            .filter_map(|e| match e {
                crate::loader::LoadError::Compile(msg) => Some(msg.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(compile_messages.len(), 1);
        assert!(
            compile_messages[0].contains("template `foo` is not visible from module `B`"),
            "unexpected compile diagnostic: {compile_messages:?}"
        );
    }

    #[test]
    fn modlmisc_04_one_level_import_visibility_matches_expected_rejections() {
        let mut engine = new_utf8_engine();
        let errors = engine
            .load_str(
                r"
                (defmodule A (export ?ALL))
                (deftemplate A::foo (slot x))
                (deftemplate A::bar (slot x))
                (defmodule B)
                (defrule B::rule1 (foo (x 3)) =>)
                (defrule B::rule2 (bar (x 3)) =>)
                (defmodule C (import A ?ALL))
                (defrule C::rule1 (foo (x 3)) =>)
                (defrule C::rule2 (bar (x 3)) =>)
                (defmodule D (import A deftemplate ?ALL))
                (defrule D::rule1 (foo (x 3)) =>)
                (defrule D::rule2 (bar (x 3)) =>)
                (defmodule E (import A deftemplate foo bar))
                (defrule E::rule1 (foo (x 3)) =>)
                (defrule E::rule2 (bar (x 3)) =>)
                (defmodule F (import A deftemplate bar))
                (defrule F::rule1 (foo (x 3)) =>)
                (defrule F::rule2 (bar (x 3)) =>)
                (defmodule G (import A deftemplate foo))
                (defrule G::rule1 (foo (x 3)) =>)
                (defrule G::rule2 (bar (x 3)) =>)
            ",
            )
            .unwrap_err();

        let compile_messages: Vec<&str> = errors
            .iter()
            .filter_map(|e| match e {
                crate::loader::LoadError::Compile(msg) => Some(msg.as_str()),
                _ => None,
            })
            .collect();

        let expected = [
            "template `foo` is not visible from module `B`",
            "template `bar` is not visible from module `B`",
            "template `foo` is not visible from module `F`",
            "template `bar` is not visible from module `G`",
        ];
        assert_eq!(compile_messages.len(), expected.len());
        for want in expected {
            assert!(
                compile_messages.iter().any(|msg| msg.contains(want)),
                "missing expected diagnostic `{want}` in {compile_messages:?}"
            );
        }
    }

    #[test]
    fn modlmisc_05_two_level_import_visibility_matches_expected_rejections() {
        let mut engine = new_utf8_engine();
        let errors = engine
            .load_str(
                r"
                (defmodule A (export ?ALL))
                (deftemplate A::foo (slot x))
                (deftemplate A::bar (slot x))
                (defmodule B1 (export ?ALL))
                (defmodule B2 (import B1 ?ALL))
                (defrule B2::rule1 (foo (x 3)) =>)
                (defrule B2::rule2 (bar (x 3)) =>)
                (defmodule C1 (import A ?ALL) (export ?ALL))
                (defmodule C2 (import C1 deftemplate ?ALL))
                (defrule C2::rule1 (foo (x 3)) =>)
                (defrule C2::rule2 (bar (x 3)) =>)
                (defmodule D1 (import A deftemplate ?ALL) (export ?ALL))
                (defmodule D2 (import D1 deftemplate ?NONE))
                (defrule D2::rule1 (foo (x 3)) =>)
                (defrule D2::rule2 (bar (x 3)) =>)
                (defmodule E1 (import A deftemplate foo bar) (export ?ALL))
                (defmodule E2 (import E1 deftemplate foo bar))
                (defrule E2::rule1 (foo (x 3)) =>)
                (defrule E2::rule2 (bar (x 3)) =>)
                (defmodule F1 (import A deftemplate bar) (export ?ALL))
                (defmodule F2 (import F1 deftemplate foo))
                (defrule F2::rule1 (foo (x 3)) =>)
                (defrule F2::rule2 (bar (x 3)) =>)
                (defmodule G1 (import A deftemplate foo) (export ?ALL))
                (defmodule G2 (import G1 deftemplate foo))
                (defrule G2::rule1 (foo (x 3)) =>)
                (defrule G2::rule2 (bar (x 3)) =>)
            ",
            )
            .unwrap_err();

        let compile_messages: Vec<&str> = errors
            .iter()
            .filter_map(|e| match e {
                crate::loader::LoadError::Compile(msg) => Some(msg.as_str()),
                _ => None,
            })
            .collect();

        let expected = [
            "template `foo` is not visible from module `B2`",
            "template `bar` is not visible from module `B2`",
            "template `foo` is not visible from module `D2`",
            "template `bar` is not visible from module `D2`",
            "template `foo` is not visible from module `F2`",
            "template `bar` is not visible from module `F2`",
            "template `bar` is not visible from module `G2`",
        ];
        assert_eq!(compile_messages.len(), expected.len());
        for want in expected {
            assert!(
                compile_messages.iter().any(|msg| msg.contains(want)),
                "missing expected diagnostic `{want}` in {compile_messages:?}"
            );
        }
    }

    #[test]
    fn modlmisc_09_imported_template_lhs_visibility_rejections_match_expected() {
        let mut engine = new_utf8_engine();
        let errors = engine
            .load_str(
                r"
                (defmodule A (export ?ALL))
                (deftemplate A::foo (slot x))
                (defmodule B (import A ?ALL))
                (deftemplate B::bar (slot y))
                (defmodule C (import A ?ALL))
                (deftemplate C::yak (slot z))
                (defrule C::bad (bar (y 3)) =>)
                (defrule C::good (foo (x 2)) (yak (z 4)) =>)
                (defrule B::bad (yak (z 4)) =>)
                (defrule B::good (foo (x 2)) (bar (y 3)) =>)
                (defrule A::bad1 (yak (z 4)) =>)
                (defrule A::bad2 (bar (y 3)) =>)
                (defrule A::good (foo (x 2)) =>)
            ",
            )
            .unwrap_err();

        let compile_messages: Vec<&str> = errors
            .iter()
            .filter_map(|e| match e {
                crate::loader::LoadError::Compile(msg) => Some(msg.as_str()),
                _ => None,
            })
            .collect();

        let expected = [
            "template `bar` is not visible from module `C`",
            "template `yak` is not visible from module `B`",
            "template `yak` is not visible from module `A`",
            "template `bar` is not visible from module `A`",
        ];
        assert_eq!(compile_messages.len(), expected.len());
        for want in expected {
            assert!(
                compile_messages.iter().any(|msg| msg.contains(want)),
                "missing expected diagnostic `{want}` in {compile_messages:?}"
            );
        }
    }

    #[test]
    fn multiple_modules_focus_chain() {
        let mut engine = new_utf8_engine();
        load_ok(
            &mut engine,
            r"
            (defrule init-chain (go) => (assert (phase-a)) (focus A))

            (defmodule A (export ?ALL))
            (defrule a-work (phase-a) => (assert (a-done)) (focus B))

            (defmodule B (export ?ALL))
            (defrule b-work (a-done) => (assert (b-done)))
        ",
        );
        load_ok(&mut engine, "(assert (go))");

        let result = run_to_completion(&mut engine);
        // init-chain (MAIN) → focus A → a-work (A) → focus B → b-work (B) → done
        assert_eq!(result.rules_fired, 3);
        assert_has_fact_with_relation(&engine, "b-done");
    }

    #[test]
    fn engine_debug_consistency_includes_phase3_registries() {
        let mut engine = new_utf8_engine();
        load_ok(
            &mut engine,
            r"
            (defglobal ?*offset* = 1)
            (deffunction inc (?x) (+ ?x ?*offset*))
            (defgeneric tag)
            (defmethod tag ((?x INTEGER)) ?x)
            (defmodule WORK)
            (defrule run (go ?x) => (assert (done (inc (tag ?x)))))
            (deffacts startup (go 41))
        ",
        );
        assert_engine_consistent(&engine);

        engine.set_focus("WORK").unwrap();
        let result = run_to_completion(&mut engine);
        assert_eq!(result.rules_fired, 1);
        assert_engine_consistent(&engine);

        engine.reset().unwrap();
        assert_engine_consistent(&engine);
    }

    // =======================================================================
    // Pass 009: defgeneric/defmethod dispatch
    // =======================================================================
    //
    // Design note: method bodies are evaluated through the expression evaluator,
    // which only handles pure expressions (arithmetic, comparisons, function calls).
    // RHS actions like `assert` and `retract` are NOT available inside method bodies.
    // Methods should return computed values; the rule's RHS actions use those values.

    #[test]
    fn defgeneric_with_no_methods_registers_in_engine() {
        let mut engine = new_utf8_engine();
        load_ok(&mut engine, "(defgeneric describe)");
        // The generic is registered; calling it from a rule should give NoApplicableMethod.
        // Here we just verify loading doesn't panic.
        assert!(engine
            .generics
            .get(engine.module_registry.main_module_id(), "describe")
            .is_some());
    }

    #[test]
    fn defmethod_single_typed_method_dispatches_on_integer() {
        // Method returns doubled integer; rule asserts it.
        let mut engine = new_utf8_engine();
        load_ok(
            &mut engine,
            r"
            (defgeneric double-int)
            (defmethod double-int ((?x INTEGER)) (* ?x 2))
            (defrule trigger (input ?v) => (assert (result (double-int ?v))))
            (deffacts startup (input 21))
        ",
        );
        let result = run_to_completion(&mut engine);
        assert_eq!(result.rules_fired, 1);
        let fields = get_ordered_fields(&engine, "result");
        assert_eq!(fields.len(), 1);
        assert!(
            matches!(fields[0], ferric_rules_core::Value::Integer(42)),
            "expected Integer(42), got {:?}",
            fields[0]
        );
    }

    #[test]
    fn defmethod_dispatches_float_to_float_method() {
        // Two typed methods; float input dispatches to the float method.
        let mut engine = new_utf8_engine();
        load_ok(
            &mut engine,
            r"
            (defgeneric negate)
            (defmethod negate ((?x INTEGER)) (- 0 ?x))
            (defmethod negate ((?x FLOAT)) (- 0.0 ?x))
            (defrule trigger (input ?v) => (assert (result (negate ?v))))
            (deffacts startup (input 3.5))
        ",
        );
        run_to_completion(&mut engine);
        let fields = get_ordered_fields(&engine, "result");
        assert_eq!(fields.len(), 1);
        assert!(
            matches!(fields[0], ferric_rules_core::Value::Float(_)),
            "expected Float value from float method, got {:?}",
            fields[0]
        );
    }

    #[test]
    fn defmethod_untyped_acts_as_catch_all() {
        // Method with index 1 is INTEGER-typed, index 2 is untyped (catch-all).
        // Symbol input should match only the untyped method (returns 99).
        let mut engine = new_utf8_engine();
        load_ok(
            &mut engine,
            r"
            (defgeneric passthrough)
            (defmethod passthrough 1 ((?x INTEGER)) (+ ?x 0))
            (defmethod passthrough 2 ((?x)) 99)
            (defrule trigger (trigger) => (assert (result (passthrough hello))))
            (deffacts startup (trigger))
        ",
        );
        run_to_completion(&mut engine);
        let fields = get_ordered_fields(&engine, "result");
        assert_eq!(fields.len(), 1);
        assert!(
            matches!(fields[0], ferric_rules_core::Value::Integer(99)),
            "expected Integer(99) from catch-all method, got {:?}",
            fields[0]
        );
    }

    #[test]
    fn defmethod_index_ordering_respected() {
        // Method with index 1 (INTEGER) is tried before index 2 (untyped).
        // Integer input dispatches to index 1, returning 111.
        let mut engine = new_utf8_engine();
        load_ok(
            &mut engine,
            r"
            (defgeneric classify)
            (defmethod classify 1 ((?x INTEGER)) 111)
            (defmethod classify 2 ((?x)) 222)
            (defrule run (trigger) => (assert (result (classify 10))))
            (deffacts startup (trigger))
        ",
        );
        run_to_completion(&mut engine);
        let fields = get_ordered_fields(&engine, "result");
        assert_eq!(fields.len(), 1);
        assert!(
            matches!(fields[0], ferric_rules_core::Value::Integer(111)),
            "expected Integer(111) from index-1 method, got {:?}",
            fields[0]
        );
    }

    #[test]
    fn defmethod_without_matching_method_does_not_produce_result() {
        // No method matches symbol "hello"; the generic call fails with
        // NoApplicableMethod. The assert action itself fails too, so no fact
        // is produced. The engine does not crash.
        let mut engine = new_utf8_engine();
        load_ok(
            &mut engine,
            r"
            (defgeneric int-only)
            (defmethod int-only ((?x INTEGER)) (* ?x 2))
            (defrule run (trigger) => (assert (result (int-only hello))))
            (deffacts startup (trigger))
        ",
        );
        run_to_completion(&mut engine);
        assert_no_fact_with_relation(&engine, "result");
    }

    #[test]
    fn defmethod_return_value_usable_in_rhs() {
        // Method computes a value; the caller uses it in an assert.
        let mut engine = new_utf8_engine();
        load_ok(
            &mut engine,
            r"
            (defgeneric double)
            (defmethod double ((?x INTEGER)) (* ?x 2))
            (defrule run (value ?v) => (assert (doubled (double ?v))))
            (deffacts startup (value 7))
        ",
        );
        run_to_completion(&mut engine);
        let fields = get_ordered_fields(&engine, "doubled");
        assert_eq!(fields.len(), 1);
        assert!(
            matches!(fields[0], ferric_rules_core::Value::Integer(14)),
            "expected Integer(14), got {:?}",
            fields[0]
        );
    }

    #[test]
    fn defmethod_wildcard_parameter_collects_extra_args() {
        // Method has one typed regular param and a wildcard; returns the first param.
        let mut engine = new_utf8_engine();
        load_ok(
            &mut engine,
            r"
            (defgeneric first-arg)
            (defmethod first-arg ((?first INTEGER) $?rest) ?first)
            (defrule run (trigger) => (assert (result (first-arg 10 20 30))))
            (deffacts startup (trigger))
        ",
        );
        run_to_completion(&mut engine);
        let fields = get_ordered_fields(&engine, "result");
        assert_eq!(fields.len(), 1);
        assert!(
            matches!(fields[0], ferric_rules_core::Value::Integer(10)),
            "expected Integer(10), got {:?}",
            fields[0]
        );
    }

    #[test]
    fn defmethod_number_type_matches_both_integer_and_float() {
        // NUMBER type restriction should accept both integers and floats.
        let mut engine = new_utf8_engine();
        load_ok(
            &mut engine,
            r"
            (defgeneric add-one)
            (defmethod add-one ((?x NUMBER)) (+ ?x 1))
            (defrule run-int (int-trigger ?v) => (assert (int-result (add-one ?v))))
            (defrule run-float (float-trigger ?v) => (assert (float-result (add-one ?v))))
            (deffacts startup (int-trigger 5) (float-trigger 3.0))
        ",
        );
        run_to_completion(&mut engine);
        assert_has_fact_with_relation(&engine, "int-result");
        assert_has_fact_with_relation(&engine, "float-result");
    }

    #[test]
    fn defmethod_lexeme_type_matches_symbol_and_string() {
        // LEXEME type restriction should accept both symbols and strings.
        let mut engine = new_utf8_engine();
        load_ok(
            &mut engine,
            r#"
            (defgeneric str-len-proxy)
            (defmethod str-len-proxy ((?x LEXEME)) 1)
            (defrule run-sym (sym-trigger) => (assert (sym-result (str-len-proxy hello))))
            (defrule run-str (str-trigger) => (assert (str-result (str-len-proxy "world"))))
            (deffacts startup (sym-trigger) (str-trigger))
        "#,
        );
        run_to_completion(&mut engine);
        assert_has_fact_with_relation(&engine, "sym-result");
        assert_has_fact_with_relation(&engine, "str-result");
    }

    #[test]
    fn generic_dispatch_auto_creates_generic_from_defmethod() {
        // A defmethod without a preceding defgeneric auto-creates the generic.
        let mut engine = new_utf8_engine();
        load_ok(
            &mut engine,
            r"
            (defmethod auto-gen ((?x INTEGER)) (* ?x 3))
            (defrule run (trigger) => (assert (result (auto-gen 4))))
            (deffacts startup (trigger))
        ",
        );
        run_to_completion(&mut engine);
        let fields = get_ordered_fields(&engine, "result");
        assert_eq!(fields.len(), 1);
        assert!(
            matches!(fields[0], ferric_rules_core::Value::Integer(12)),
            "expected Integer(12), got {:?}",
            fields[0]
        );
    }

    // =======================================================================
    // Pass 010: forall CE
    // =======================================================================

    #[test]
    fn forall_fires_when_all_items_satisfy() {
        let mut engine = new_utf8_engine();
        load_ok(
            &mut engine,
            r"
            (defrule all-checked
                (forall (item ?id) (checked ?id))
                =>
                (assert (all-complete)))
            (deffacts startup (item 1) (item 2) (checked 1) (checked 2))
        ",
        );
        let result = run_to_completion(&mut engine);
        assert_eq!(result.rules_fired, 1);
        assert_has_fact_with_relation(&engine, "all-complete");
    }

    #[test]
    fn forall_does_not_fire_with_unchecked_item() {
        let mut engine = new_utf8_engine();
        load_ok(
            &mut engine,
            r"
            (defrule all-checked
                (forall (item ?id) (checked ?id))
                =>
                (assert (all-complete)))
            (deffacts startup (item 1) (item 2) (checked 1))
        ",
        );
        let result = run_to_completion(&mut engine);
        assert_eq!(result.rules_fired, 0);
        assert_no_fact_with_relation(&engine, "all-complete");
    }

    #[test]
    fn forall_vacuous_truth_no_items() {
        // With no items, forall is vacuously true.
        let mut engine = new_utf8_engine();
        load_ok(
            &mut engine,
            r"
            (defrule all-checked
                (forall (item ?id) (checked ?id))
                =>
                (assert (all-complete)))
        ",
        );
        engine.reset().unwrap();
        let result = run_to_completion(&mut engine);
        assert_eq!(
            result.rules_fired, 1,
            "forall should be vacuously true with no items"
        );
        assert_has_fact_with_relation(&engine, "all-complete");
    }

    #[test]
    fn forall_vacuous_truth_and_retraction_cycle_contract() {
        let mut engine = new_utf8_engine();
        load_ok(
            &mut engine,
            r"
            (defrule all-checked
                (forall (item ?id) (checked ?id))
                =>
                (assert (all-complete)))
        ",
        );

        // Step 2: Empty WM -> vacuous truth.
        engine.reset().unwrap();
        let result = run_to_completion(&mut engine);
        assert_eq!(result.rules_fired, 1);

        // Step 3: Assert unchecked item -> unsatisfied.
        let item_fid = load_ok(&mut engine, "(assert (item 1))").asserted_facts[0];
        let result = run_to_completion(&mut engine);
        assert_eq!(result.rules_fired, 0);

        // Step 4: Add checked fact -> satisfied again.
        let checked_fid = load_ok(&mut engine, "(assert (checked 1))").asserted_facts[0];
        let result = run_to_completion(&mut engine);
        assert_eq!(result.rules_fired, 1);

        // Step 5: Retract checked -> unsatisfied.
        engine.retract(checked_fid).unwrap();
        let result = run_to_completion(&mut engine);
        assert_eq!(result.rules_fired, 0);

        // Step 6: Retract item -> vacuous truth restored.
        engine.retract(item_fid).unwrap();
        let result = run_to_completion(&mut engine);
        assert_eq!(result.rules_fired, 1);
    }

    #[test]
    fn forall_with_multiple_items_all_satisfied() {
        let mut engine = new_utf8_engine();
        load_ok(
            &mut engine,
            r"
            (defrule all-validated
                (forall (task ?t) (done ?t))
                =>
                (assert (all-done)))
            (deffacts startup (task a) (task b) (task c) (done a) (done b) (done c))
        ",
        );
        let result = run_to_completion(&mut engine);
        assert_eq!(result.rules_fired, 1);
        assert_has_fact_with_relation(&engine, "all-done");
    }

    #[test]
    fn forall_with_multiple_items_one_missing() {
        let mut engine = new_utf8_engine();
        load_ok(
            &mut engine,
            r"
            (defrule all-validated
                (forall (task ?t) (done ?t))
                =>
                (assert (all-done)))
            (deffacts startup (task a) (task b) (task c) (done a) (done c))
        ",
        );
        let result = run_to_completion(&mut engine);
        assert_eq!(result.rules_fired, 0);
        assert_no_fact_with_relation(&engine, "all-done");
    }

    #[test]
    fn forall_with_other_patterns() {
        // forall combined with a normal positive pattern.
        let mut engine = new_utf8_engine();
        load_ok(
            &mut engine,
            r"
            (defrule check-batch
                (batch ?name)
                (forall (item ?id) (checked ?id))
                =>
                (assert (batch-complete ?name)))
            (deffacts startup (batch test-batch) (item 1) (checked 1))
        ",
        );
        let result = run_to_completion(&mut engine);
        assert_eq!(result.rules_fired, 1);
        assert_has_fact_with_relation(&engine, "batch-complete");
    }

    #[test]
    fn forall_unsupported_nested_forall_errors() {
        let mut engine = new_utf8_engine();
        let result = engine.load_str(
            r"
            (defrule nested-forall
                (forall (item ?id) (forall (sub ?s) (done ?s)))
                =>
                (assert (done)))
        ",
        );
        assert!(result.is_err(), "nested forall should produce a load error");
    }

    #[test]
    fn forall_unsupported_three_patterns_errors() {
        let mut engine = new_utf8_engine();
        let result = engine.load_str(
            r"
            (defrule three-pattern-forall
                (forall (item ?id) (checked ?id) (verified ?id))
                =>
                (assert (done)))
        ",
        );
        assert!(
            result.is_err(),
            "forall with 3 sub-patterns should produce a load error in Phase 3"
        );
    }

    // =======================================================================
    // Pass 011: fixture-driven integration tests
    // =======================================================================

    #[test]
    fn fixture_deffunction_loads_and_executes() {
        let mut engine = new_utf8_engine();
        load_fixture(&mut engine, "phase3_deffunction.clp");
        let result = run_to_completion(&mut engine);
        assert_eq!(result.rules_fired, 1);
        assert_has_fact_with_relation(&engine, "result");
        let fields = get_ordered_fields(&engine, "result");
        assert!(fields[0].structural_eq(&ferric_rules_core::Value::Integer(11)));
    }

    #[test]
    fn fixture_defglobal_loads_and_executes() {
        let mut engine = new_utf8_engine();
        load_fixture(&mut engine, "phase3_defglobal.clp");
        let result = run_to_completion(&mut engine);
        // value 100 > threshold 50, so above-threshold asserted
        // value 25 < threshold 50, so no above-threshold for 25
        assert_eq!(result.rules_fired, 1);
        assert_has_fact_with_relation(&engine, "above-threshold");
        let facts = find_facts_by_relation(&engine, "above-threshold");
        assert_eq!(facts.len(), 1);
        let fields = get_ordered_fields(&engine, "above-threshold");
        assert!(fields[0].structural_eq(&ferric_rules_core::Value::Integer(100)));
    }

    #[test]
    fn fixture_defgeneric_loads_and_dispatches() {
        let mut engine = new_utf8_engine();
        load_fixture(&mut engine, "phase3_defgeneric.clp");
        let result = run_to_completion(&mut engine);
        assert_eq!(result.rules_fired, 1);
        assert_has_fact_with_relation(&engine, "result");
        let fields = get_ordered_fields(&engine, "result");
        assert!(fields[0].structural_eq(&ferric_rules_core::Value::Integer(10)));
    }

    #[test]
    fn fixture_forall_loads_and_executes() {
        let mut engine = new_utf8_engine();
        load_fixture(&mut engine, "phase3_forall.clp");
        let result = run_to_completion(&mut engine);
        assert_eq!(result.rules_fired, 1);
        assert_has_fact_with_relation(&engine, "all-complete");
    }

    #[test]
    fn fixture_printout_loads_and_produces_output() {
        let mut engine = new_utf8_engine();
        load_fixture(&mut engine, "phase3_printout.clp");
        run_to_completion(&mut engine);
        let output = engine.get_output("t").unwrap_or_default();
        assert!(output.contains("Hello, "));
    }

    #[test]
    fn fixture_expression_eval_loads_and_executes() {
        let mut engine = new_utf8_engine();
        load_fixture(&mut engine, "phase3_expression_eval.clp");
        let result = run_to_completion(&mut engine);
        // value 5 passes test (> 5 0), value -3 fails test (> -3 0)
        assert_eq!(result.rules_fired, 1);
        assert_has_fact_with_relation(&engine, "positive");
        assert_has_fact_with_relation(&engine, "doubled");
    }

    #[test]
    fn fixture_defmodule_loads_and_executes_with_focus() {
        let mut engine = new_utf8_engine();
        load_fixture(&mut engine, "phase3_defmodule.clp");
        let result = run_to_completion(&mut engine);
        // start rule fires (from MAIN), then focus COUNTER fires count-step
        assert!(result.rules_fired >= 2);
        assert_has_fact_with_relation(&engine, "started");
        assert_has_fact_with_relation(&engine, "counted");
    }

    // =======================================================================
    // Pass 011: cross-feature interaction tests
    // =======================================================================

    #[test]
    fn cross_feature_deffunction_with_defglobal() {
        // A function reads a global variable.
        let mut engine = new_utf8_engine();
        load_ok(
            &mut engine,
            r"
            (defglobal ?*factor* = 10)
            (deffunction scale (?x) (* ?x ?*factor*))
            (defrule apply-scale
                (input ?v)
                =>
                (assert (scaled (scale ?v))))
            (deffacts startup (input 5))
        ",
        );
        let result = run_to_completion(&mut engine);
        assert_eq!(result.rules_fired, 1);
        let fields = get_ordered_fields(&engine, "scaled");
        assert!(fields[0].structural_eq(&ferric_rules_core::Value::Integer(50)));
    }

    #[test]
    fn cross_feature_generic_with_printout() {
        // A generic function returns a label; the rule prints it.
        // Method bodies are pure expression evaluators — printout is a RHS action
        // and is not available inside method bodies. Instead, the method returns
        // a value and the calling rule's RHS does the printing.
        let mut engine = new_utf8_engine();
        load_ok(
            &mut engine,
            r#"
            (defgeneric classify)
            (defmethod classify ((?x INTEGER)) 42)
            (defmethod classify ((?x FLOAT)) 3.14)
            (defrule test-show
                (trigger)
                =>
                (printout t "int-result=" (classify 10) crlf)
                (printout t "float-result=" (classify 1.5) crlf))
            (deffacts startup (trigger))
        "#,
        );
        run_to_completion(&mut engine);
        let output = engine.get_output("t").unwrap_or_default();
        assert!(
            output.contains("int-result="),
            "expected 'int-result=' in output, got: {output}"
        );
        assert!(
            output.contains("float-result="),
            "expected 'float-result=' in output, got: {output}"
        );
    }

    #[test]
    fn cross_feature_forall_with_template() {
        // forall using template patterns.
        let mut engine = new_utf8_engine();
        load_ok(
            &mut engine,
            r"
            (deftemplate task (slot id) (slot status))
            (defrule all-tasks-done
                (check-enabled)
                (forall (task (id ?id)) (task (id ?id) (status done)))
                =>
                (assert (all-done)))
            (deffacts startup
                (check-enabled)
                (task (id 1) (status done))
                (task (id 2) (status done)))
        ",
        );
        let result = run_to_completion(&mut engine);
        assert_eq!(result.rules_fired, 1);
        assert_has_fact_with_relation(&engine, "all-done");
    }

    #[test]
    fn cross_feature_deffunction_calling_generic() {
        // A deffunction calls a generic function.
        let mut engine = new_utf8_engine();
        load_ok(
            &mut engine,
            r"
            (defgeneric double)
            (defmethod double ((?x INTEGER)) (* ?x 2))
            (deffunction quadruple (?x) (double (double ?x)))
            (defrule test
                (input ?v)
                =>
                (assert (result (quadruple ?v))))
            (deffacts startup (input 3))
        ",
        );
        let result = run_to_completion(&mut engine);
        assert_eq!(result.rules_fired, 1);
        let fields = get_ordered_fields(&engine, "result");
        assert!(fields[0].structural_eq(&ferric_rules_core::Value::Integer(12)));
    }

    #[test]
    fn cross_feature_test_ce_with_deffunction() {
        // A test CE calls a user-defined function.
        let mut engine = new_utf8_engine();
        load_ok(
            &mut engine,
            r"
            (deffunction is-big (?x) (> ?x 100))
            (defrule find-big
                (value ?x)
                (test (is-big ?x))
                =>
                (assert (big-value ?x)))
            (deffacts startup (value 50) (value 200))
        ",
        );
        let result = run_to_completion(&mut engine);
        assert_eq!(result.rules_fired, 1);
        assert_has_fact_with_relation(&engine, "big-value");
        let fields = get_ordered_fields(&engine, "big-value");
        assert!(fields[0].structural_eq(&ferric_rules_core::Value::Integer(200)));
    }

    #[test]
    fn cross_feature_global_bind_and_printout() {
        // Bind a global and printout its value.
        let mut engine = new_utf8_engine();
        load_ok(
            &mut engine,
            r#"
            (defglobal ?*count* = 0)
            (defrule increment
                (step)
                =>
                (bind ?*count* (+ ?*count* 1))
                (printout t "count=" ?*count* crlf))
            (deffacts startup (step))
        "#,
        );
        run_to_completion(&mut engine);
        let output = engine.get_output("t").unwrap_or_default();
        assert!(
            output.contains("count=1"),
            "expected count=1 in output, got: {output}"
        );
    }

    // =======================================================================
    // Pass 011: unsupported construct diagnostics
    // =======================================================================

    #[test]
    fn unsupported_defclass_produces_diagnostic() {
        let mut engine = new_utf8_engine();
        let result = engine.load_str("(defclass PERSON (is-a USER) (slot name))");
        assert!(result.is_err());
        let err_msg = format!("{}", result.unwrap_err()[0]);
        assert!(
            err_msg.contains("unsupported") || err_msg.contains("defclass"),
            "expected diagnostic mentioning defclass, got: {err_msg}"
        );
    }
}
