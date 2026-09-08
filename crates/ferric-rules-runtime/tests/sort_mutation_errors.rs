//! Fact construction keeps CLIPS error values separate from the deferred halt.
//! Exact reference programs are in the #343 assert-error/mutation-values probes.
//! Invalid multifield-to-scalar cases that fault the reference are excluded.
//! The sole seed is selected by its scalar slot, avoiding a multislot LHS dependency.

use ferric_rules_core::{Fact, Value};
use ferric_rules_runtime::{Engine, FactHandle, HaltReason, RunLimit};

fn mutation_engine(action: &str) -> Engine {
    Engine::with_rules(&format!(
        "(defglobal ?*trace* = 0)
         (deftemplate record (multislot values)
           (slot scalar (default 0)) (slot later (default 0)))
         (deffacts seed (record (values old)))
         (deffunction broken (?a ?b) (bind ?*trace* (+ ?*trace* 1)) (/ 1 0))
         (deffunction tap (?value) (bind ?*trace* (+ ?*trace* 10)) ?value)
         (defrule compute ?f <- (record (later 0)) => {action} (assert (after)))"
    ))
    .unwrap()
}

fn run_failure(engine: &mut Engine) {
    let run = engine.run(RunLimit::Count(10)).unwrap();
    assert_eq!(run.halt_reason, HaltReason::ActionError);
    assert_eq!(run.rules_fired, 1);
    assert!(!engine.action_diagnostics().is_empty());
    assert!(engine.find_facts("after").unwrap().is_empty());
    assert!(matches!(
        engine.get_global("trace"),
        Some(Value::Integer(1))
    ));
    engine.rete().validate_consistency().unwrap();
}

fn assert_integer_fields(values: &[Value], expected: &[i64]) {
    assert_eq!(values.len(), expected.len());
    assert!(values
        .iter()
        .zip(expected)
        .all(|(value, expected)| matches!(value, Value::Integer(actual) if actual == expected)));
}

fn record_ids(engine: &Engine) -> Vec<FactHandle> {
    engine
        .facts()
        .unwrap()
        .filter_map(|(id, fact)| match fact {
            Fact::Template(fact)
                if engine.template_name_by_id(fact.template_id) == Some("record") =>
            {
                Some(id)
            }
            _ => None,
        })
        .collect()
}

fn changed_record(engine: &Engine) -> FactHandle {
    let changed: Vec<_> = record_ids(engine)
        .into_iter()
        .filter(|id| {
            matches!(engine.get_fact_slot_by_name(*id, "later").unwrap(),
                Value::Symbol(symbol) if engine.resolve_core_symbol(*symbol) == Some("FALSE"))
        })
        .collect();
    assert_eq!(changed.len(), 1);
    changed[0]
}

fn assert_multislot(engine: &Engine, id: FactHandle, expected: &[i64]) {
    let Value::Multifield(values) = engine.get_fact_slot_by_name(id, "values").unwrap() else {
        panic!("values must be an actual multifield")
    };
    assert_integer_fields(values, expected);
}

#[test]
fn ordered_assertion_publishes_empty_or_retained_fields_according_to_evaluation_error() {
    for (data, expected) in [("3 1", &[][..]), ("3 1 2", &[3, 1, 2][..])] {
        let mut engine = mutation_engine(&format!("(assert (result (sort broken {data})))"));
        run_failure(&mut engine);
        let facts = engine.find_facts("result").unwrap();
        assert_eq!(facts.len(), 1);
        let Fact::Ordered(fact) = facts[0].1 else {
            panic!("expected ordered result")
        };
        assert_integer_fields(&fact.fields, expected);
    }
}

#[test]
fn template_assertion_keeps_the_fact_and_later_scalar_returned_by_a_skipped_call() {
    for (data, expected) in [("3 1", &[][..]), ("3 1 2", &[3, 1, 2][..])] {
        let mut engine = mutation_engine(&format!(
            "(assert (record (values (sort broken {data})) (later (tap 7))))"
        ));
        run_failure(&mut engine);
        assert_eq!(record_ids(&engine).len(), 2);
        assert_multislot(&engine, changed_record(&engine), expected);
    }
}

#[test]
fn modify_and_duplicate_commit_multislot_results_before_the_run_halts() {
    for operation in ["modify", "duplicate"] {
        for (data, expected) in [("3 1", &[][..]), ("3 1 2", &[3, 1, 2][..])] {
            let mut engine = mutation_engine(&format!(
                "({operation} ?f (values (sort broken {data})) (later (tap 7)))"
            ));
            let old = record_ids(&engine)[0];
            run_failure(&mut engine);
            assert_multislot(&engine, changed_record(&engine), expected);
            assert_eq!(
                record_ids(&engine).len(),
                if operation == "modify" { 1 } else { 2 }
            );
            assert_eq!(
                engine.get_fact(old).unwrap().is_some(),
                operation == "duplicate"
            );
        }
    }
}

#[test]
fn scalar_mutations_keep_length_values_instead_of_discarding_the_fact() {
    for operation in ["modify", "duplicate"] {
        for (data, expected) in [("3 1", 2), ("3 1 2", 3)] {
            let mut engine = mutation_engine(&format!(
                "({operation} ?f (scalar (length$ (sort broken {data}))) (later (tap 7)))"
            ));
            let old = record_ids(&engine)[0];
            run_failure(&mut engine);
            let changed = changed_record(&engine);
            assert!(
                matches!(engine.get_fact_slot_by_name(changed, "scalar").unwrap(), Value::Integer(actual) if *actual == expected)
            );
            assert_eq!(
                record_ids(&engine).len(),
                if operation == "modify" { 1 } else { 2 }
            );
            assert_eq!(
                engine.get_fact(old).unwrap().is_some(),
                operation == "duplicate"
            );
        }
    }
}

#[cfg(feature = "serde")]
#[test]
fn completed_error_mutations_restore_values_matching_and_fresh_host_operations() {
    for &format in ferric_rules_runtime::SerializationFormat::ALL {
        for operation in ["modify", "duplicate"] {
            let mut engine = mutation_engine(&format!(
                "({operation} ?f (values (sort broken 3 1)) (later (tap 7)))"
            ));
            engine
                .load_str("(defrule observe (record (later FALSE)) => (assert (observed)))")
                .unwrap();
            run_failure(&mut engine);
            assert!(engine.find_facts("observed").unwrap().is_empty());
            let mut restored =
                Engine::deserialize(&engine.serialize(format).unwrap(), format).unwrap();
            assert!(!restored.action_diagnostics().is_empty());
            assert_multislot(&restored, changed_record(&restored), &[]);
            let next = restored.run(RunLimit::Count(10)).unwrap();
            assert_eq!(next.halt_reason, HaltReason::AgendaEmpty);
            assert_eq!(next.rules_fired, 1);
            assert!(restored.action_diagnostics().is_empty());
            assert_eq!(restored.find_facts("observed").unwrap().len(), 1);
            let fresh = restored
                .assert_template_slots("record", [("values", 42_i64)])
                .unwrap();
            restored.retract(fresh).unwrap();
            restored.rete().validate_consistency().unwrap();
            restored.reset().unwrap();
            assert!(restored.action_diagnostics().is_empty());
            assert_eq!(record_ids(&restored).len(), 1);
        }
    }
}
