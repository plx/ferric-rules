//! Field alternatives remain one match and preserve quantified support lifetimes.

use ferric_rules_runtime::{Engine, EngineConfig, FactHandle, HaltReason, RunLimit, Value};

fn assert_item(engine: &mut Engine, key: i64, tag: i64) -> FactHandle {
    engine
        .assert_ordered("item", [Value::Integer(key), Value::Integer(tag)])
        .unwrap()
}

fn fire(engine: &mut Engine, expected: usize) {
    assert_eq!(engine.agenda_len(), expected);
    let result = engine.run(RunLimit::Unlimited).unwrap();
    assert_eq!(result.rules_fired, expected);
    assert_eq!(result.halt_reason, HaltReason::AgendaEmpty);
    assert!(engine.action_diagnostics().is_empty());
    engine.rete().validate_consistency().unwrap();
}

#[test]
fn nine_literal_disjunctions_compile_to_one_rule_and_one_activation() {
    let mut engine =
        Engine::with_rules("(defrule select (item 0|1 0|1 0|1 0|1 0|1 0|1 0|1 0|1 0|1) =>)")
            .unwrap();
    assert_eq!(engine.rules(), [("select", 0)]);
    engine
        .assert_ordered("item", vec![Value::Integer(1); 9])
        .unwrap();
    fire(&mut engine, 1);
    let mut excluded = vec![Value::Integer(1); 9];
    excluded[8] = Value::Integer(2);
    engine.assert_ordered("item", excluded).unwrap();
    fire(&mut engine, 0);
}

#[test]
fn quantified_variable_alternatives_follow_every_support_transition() {
    for (condition, negative) in [
        ("(not (item ?k|99 ?tag))", true),
        ("(exists (item ?k|99 ?tag))", false),
        ("(not (not (item ?k|99 ?tag)))", false),
        ("(not (and (guard ?v) (item ?v|?k ?tag)))", true),
        ("(exists (guard ?v) (item ?v|?k ?tag))", false),
    ] {
        for load_after_facts in [false, true] {
            let source = format!("(defrule select (key ?k) {condition} => (assert (selected ?k)))");
            let mut engine = Engine::new(EngineConfig::default());
            if !load_after_facts {
                engine.load_str(&source).unwrap();
            }
            let parent = engine.assert_ordered("key", [Value::Integer(7)]).unwrap();
            engine
                .assert_ordered("guard", [Value::Integer(99)])
                .unwrap();
            let unrelated = assert_item(&mut engine, 8, 0);
            if load_after_facts {
                engine.load_str(&source).unwrap();
            }
            assert_eq!(engine.rules().len(), 1, "{condition}");
            assert_eq!(engine.agenda_len(), usize::from(negative), "{condition}");

            // Both alternatives are supports. Removing either one must leave
            // the other in place, including when a rule attaches after facts.
            let first = assert_item(&mut engine, 7, 1);
            let second = assert_item(&mut engine, 99, 2);
            assert_eq!(engine.agenda_len(), usize::from(!negative), "{condition}");
            engine.retract(first).unwrap();
            fire(&mut engine, usize::from(!negative));
            engine.retract(unrelated).unwrap();
            fire(&mut engine, 0);
            engine.retract(second).unwrap();
            fire(&mut engine, usize::from(negative));

            let replacement = assert_item(&mut engine, 7, 3);
            fire(&mut engine, usize::from(!negative));
            engine.retract(replacement).unwrap();
            fire(&mut engine, usize::from(negative));
            assert_eq!(engine.find_facts("selected").unwrap().len(), 1);

            // Destroying the owner while supported must clean all links; later
            // support retraction cannot recreate an activation for that owner.
            let final_support = assert_item(&mut engine, 99, 4);
            engine.retract(parent).unwrap();
            engine.retract(final_support).unwrap();
            fire(&mut engine, 0);
        }
    }
}

#[test]
fn forall_alternatives_track_both_condition_and_consequence_changes() {
    let mut engine = Engine::with_rules(
        "(defrule select (key ?k)
           (forall (item ?v&?k|99 ?tag) (ok ?v|?k))
           => (assert (selected ?k)))",
    )
    .unwrap();
    engine.assert_ordered("key", [Value::Integer(7)]).unwrap();
    fire(&mut engine, 1);
    assert_item(&mut engine, 8, 0);
    fire(&mut engine, 0);
    let ninety_nine = assert_item(&mut engine, 99, 1);
    fire(&mut engine, 0);
    engine.assert_ordered("ok", [Value::Integer(8)]).unwrap();
    fire(&mut engine, 0);
    let first_support = engine.assert_ordered("ok", [Value::Integer(99)]).unwrap();
    fire(&mut engine, 1);
    let seven = assert_item(&mut engine, 7, 2);
    fire(&mut engine, 0);
    let shared_support = engine.assert_ordered("ok", [Value::Integer(7)]).unwrap();
    fire(&mut engine, 1);
    engine.retract(first_support).unwrap();
    fire(&mut engine, 0);
    engine.retract(shared_support).unwrap();
    fire(&mut engine, 0);
    engine.retract(seven).unwrap();
    fire(&mut engine, 0);
    engine.retract(ninety_nine).unwrap();
    fire(&mut engine, 1);
}

#[test]
fn an_alternative_cannot_rebind_the_value_seen_by_later_predicates() {
    for source in [
        "(defrule select (key ?x) (item ?x|99)
           (test (= ?x 7)) => (printout t ?x crlf))",
        "(defrule select (pair ?x ?x|99)
           (test (= ?x 7)) => (printout t ?x crlf))",
    ] {
        let mut engine = Engine::with_rules(source).unwrap();
        engine.assert_ordered("key", [Value::Integer(7)]).unwrap();
        engine.assert_ordered("item", [Value::Integer(99)]).unwrap();
        engine
            .assert_ordered("pair", [Value::Integer(7), Value::Integer(99)])
            .unwrap();
        fire(&mut engine, 1);
        assert_eq!(engine.get_output("t"), Some("7\n"));
    }
}

#[test]
fn alternatives_and_quantified_locals_do_not_introduce_outer_bindings() {
    for source in [
        "(defrule invalid (item ?x|99) (test (= ?x 7)) =>)",
        "(defrule invalid (key ?k) (exists (item ?x&?k|99))
           (test (= ?x 7)) =>)",
        "(defrule invalid (key ?k) (not (not (item ?x&?k|99)))
           (test (= ?x 7)) =>)",
    ] {
        let mut engine = Engine::new(EngineConfig::default());
        assert!(engine.load_str(source).is_err(), "{source}");
        assert!(engine.rules().is_empty());
    }
}

#[test]
fn disjunctions_keep_the_explicit_complex_negative_constraint_boundary() {
    let mut engine = Engine::new(EngineConfig::default());
    let errors = engine
        .load_str(
            "(defrule unsupported (key ?k)
               (not (item ?x&:(> (* ?x ?x) ?k)|99)) =>)",
        )
        .expect_err("nonlinear negative predicates remain explicitly unsupported (#300)");
    assert!(errors.iter().any(|error| error
        .to_string()
        .contains("complex constraints inside negated patterns")));
    assert!(engine.rules().is_empty());
}

#[test]
fn negated_exists_over_a_disjunction_compiles_to_one_rule() {
    let mut engine = Engine::with_rules(
        "(defrule select (key ?k) (not (exists (item ?k|99)))
           => (printout t ?k crlf))",
    )
    .unwrap();
    assert_eq!(engine.rules(), [("select", 0)]);
    engine.assert_ordered("key", [Value::Integer(1)]).unwrap();
    engine.assert_ordered("key", [Value::Integer(2)]).unwrap();
    let support = engine.assert_ordered("item", [Value::Integer(2)]).unwrap();
    fire(&mut engine, 1);
    assert_eq!(engine.get_output("t"), Some("1\n"));

    // The literal alternative supports every key; removing the variable
    // witness alone must not unblock its key.
    let shared = engine.assert_ordered("item", [Value::Integer(99)]).unwrap();
    engine.retract(support).unwrap();
    fire(&mut engine, 0);
    engine.retract(shared).unwrap();
    fire(&mut engine, 2);
    let mut keys: Vec<_> = engine.get_output("t").unwrap().lines().collect();
    keys.sort_unstable();
    assert_eq!(keys, ["1", "1", "2"]);
}

fn assert_load_error(source: &str, message: &str) {
    let mut engine = Engine::new(EngineConfig::default());
    let errors = engine.load_str(source).expect_err(source);
    assert!(
        errors.iter().any(|error| error.to_string().contains(message)),
        "{source}: {errors:?}"
    );
    assert!(engine.rules().is_empty(), "{source}");
}

#[test]
fn every_alternative_keeps_the_restrictions_of_its_position() {
    // A correlated alternative must not hide a later one from the negated
    // pattern restrictions (#300): the boundary is independent of order.
    for field in ["?x&?k|:(> (* ?x ?x) ?k)", "?x&:(> (* ?x ?x) ?k)|?k"] {
        for condition in [
            format!("(not (cell (v {field})))"),
            format!("(forall (go) (cell (v {field})))"),
        ] {
            assert_load_error(
                &format!(
                    "(deftemplate cell (slot v))
                     (defrule unsupported (key ?k) {condition} =>)"
                ),
                "predicate constraints inside negated patterns",
            );
        }
        assert_load_error(
            &format!("(defrule unsupported (key ?k) (not (item {field})) =>)"),
            "complex constraints inside negated patterns",
        );
    }
    // A negated predicate is rejected in an alternative as it is on its own.
    for field in ["?x&?k|~:(> ?x 1)", "?x&~:(> ?x 1)|?k"] {
        assert_load_error(
            &format!("(defrule unsupported (key ?k) (item {field}) =>)"),
            "only negated literals",
        );
    }
}
