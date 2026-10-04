//! Address identity participates in ordinary fact matching without becoming an index.

use ferric_rules_core::{Fact, Value};
use ferric_rules_runtime::{Engine, EngineConfig, HaltReason, RunLimit};

fn run_clean(engine: &mut Engine) {
    let result = engine.run(RunLimit::Count(30)).unwrap();
    assert_eq!(result.halt_reason, HaltReason::AgendaEmpty);
    assert!(engine.action_diagnostics().is_empty());
}

fn labels(engine: &Engine, relation: &str) -> Vec<String> {
    let mut labels: Vec<_> = engine
        .find_facts(relation)
        .unwrap()
        .into_iter()
        .map(|(_, fact)| {
            let Fact::Ordered(fact) = fact else {
                panic!("ordered result")
            };
            let Value::Symbol(symbol) = fact.fields[0] else {
                panic!("symbol result")
            };
            engine.resolve_core_symbol(symbol).unwrap().to_owned()
        })
        .collect();
    labels.sort();
    labels
}

#[test]
fn address_join_keys_and_duplicate_suppression_survive_target_replacement() {
    let mut engine = Engine::with_rules(
        r#"
        (deftemplate item (slot v))
        (defglobal ?*old* = FALSE ?*new* = FALSE)
        (deffacts seed (item (v 1)))
        (defrule capture (declare (salience 100)) =>
          (bind ?*old* (nth$ 1 (find-all-facts ((?f item)) TRUE)))
          (retract ?*old*)
          (assert (item (v 1)))
          (bind ?*new* (nth$ 1 (find-all-facts ((?f item)) TRUE)))
          (assert (identity ?*old*) (identity ?*old*)
                  (identity ?*new*) (identity ?*new*)
                  (identity (fact-index ?*new*)))
          (assert (left old ?*old*) (left new ?*new*)
                  (right old ?*old*) (right new ?*new*)
                  (blocked ?*old*) (current ?*new*))
          (printout t (eq ?*old* ?*new*) " " (fact-existp ?*old*) " " (fact-existp ?*new*) crlf))
        (defrule join (left ?label ?address) (right ?label ?address) => (assert (joined ?label)))
        (defrule negative (left ?label ?address) (not (blocked ?address)) => (assert (unblocked ?label)))
        (defrule existence (left ?label ?address) (exists (current ?address)) => (assert (current-label ?label)))
        (defrule type-safe-index (identity ?address) (current ?address) => (assert (same-identity yes)))
        "#,
    ).unwrap();
    run_clean(&mut engine);
    assert_eq!(engine.get_output("t"), Some("FALSE FALSE TRUE\n"));
    assert_eq!(engine.find_facts("identity").unwrap().len(), 3);
    assert_eq!(labels(&engine, "joined"), ["new", "old"]);
    assert_eq!(labels(&engine, "unblocked"), ["new"]);
    assert_eq!(labels(&engine, "current-label"), ["new"]);
    assert_eq!(engine.find_facts("same-identity").unwrap().len(), 1);

    // Removing the blocker changes negative matching even though its referent
    // has already gone; stored address identity remains an ordinary join key.
    let blocked = engine.find_facts("blocked").unwrap()[0].0;
    engine.retract(blocked).unwrap();
    run_clean(&mut engine);
    assert_eq!(labels(&engine, "unblocked"), ["new", "old"]);
}

#[test]
fn fact_address_defaults_are_dummy_values_and_multislots_start_empty() {
    let mut engine = Engine::with_rules(
        r#"
        (deftemplate holder (slot label) (slot ref (type FACT-ADDRESS))
                            (multislot refs (type FACT-ADDRESS)))
        (deffacts seed (holder (label a)) (holder (label b)))
        (defmethod kind ((?value FACT-ADDRESS)) address)
        (defrule inspect (holder (label a) (ref ?a) (refs $?empty))
                         (holder (label b) (ref ?b)) =>
          (printout t ?a " " (eq ?a ?b) " " (kind ?a) " " (length$ ?empty) crlf)
          (assert (holder (label a))))
        "#,
    )
    .unwrap();
    run_clean(&mut engine);
    assert_eq!(
        engine.get_output("t"),
        Some("<Dummy Fact> TRUE address 0\n")
    );
    assert_eq!(
        engine.fact_count(),
        2,
        "duplicate dummy-valued facts are suppressed"
    );
    for (id, _) in engine.facts().unwrap() {
        assert!(
            matches!(engine.get_fact_slot_by_name(id, "ref").unwrap(), Value::FactAddress(address) if address.is_dummy())
        );
        assert!(
            matches!(engine.get_fact_slot_by_name(id, "refs").unwrap(), Value::Multifield(values) if values.is_empty())
        );
    }
}

#[test]
fn typed_address_slots_accept_real_addresses_and_reject_integer_indices() {
    let mut engine = Engine::with_rules(
        r"
        (deftemplate item (slot v))
        (deftemplate holder (slot ref (type FACT-ADDRESS)) (multislot refs (type FACT-ADDRESS)))
        (deffacts seed (item (v 1)))
        (defrule store ?f <- (item) =>
          (assert (holder (ref ?f) (refs ?f ?f)))
          (printout t stored crlf)
          (assert (holder (ref (fact-index ?f))))
          (printout t unreachable crlf))
        ",
    )
    .unwrap();
    assert_eq!(
        engine.run(RunLimit::Count(10)).unwrap().halt_reason,
        HaltReason::ActionError
    );
    assert_eq!(engine.get_output("t"), Some("stored\n"));
    assert_eq!(
        engine.fact_count(),
        2,
        "failed typed assertion does not install a partial fact"
    );
    let holder = engine
        .facts()
        .unwrap()
        .find_map(|(id, fact)| {
            let Fact::Template(fact) = fact else {
                return None;
            };
            (engine.template_name_by_id(fact.template_id) == Some("holder")).then_some(id)
        })
        .unwrap();
    let address = engine.get_fact_slot_by_name(holder, "ref").unwrap();
    let Value::Multifield(fields) = engine.get_fact_slot_by_name(holder, "refs").unwrap() else {
        panic!("multislot")
    };
    assert_eq!(fields.len(), 2);
    assert!(fields.iter().all(|field| field.structural_eq(address)));
}

#[test]
fn missing_fact_indices_return_false_but_slot_and_argument_errors_still_stop() {
    let mut engine = Engine::with_rules(
        r#"
        (deftemplate item (slot v))
        (deffacts seed (item (v 1)))
        (defrule inspect =>
          (printout t (fact-slot-value 9 v) " " (fact-slot-value -1 v) crlf)
          (printout t continued crlf))
        "#,
    )
    .unwrap();
    run_clean(&mut engine);
    assert_eq!(engine.get_output("t"), Some("FALSE FALSE\ncontinued\n"));

    for expression in [
        "(fact-slot-value 1 absent)",
        "(funcall fact-slot-value 9 7)",
        "(fact-slot-value -1 (/ 1 0))",
        "(funcall fact-slot-value \"1\" v)",
    ] {
        let mut engine = Engine::new(EngineConfig::default());
        engine
            .load_str(&format!(
                "(deftemplate item (slot v))
             (deffacts seed (item (v 1)))
             (defrule inspect => (printout t before) {expression} (printout t after))"
            ))
            .unwrap();
        engine.reset().unwrap();
        assert_eq!(
            engine.run(RunLimit::Count(10)).unwrap().halt_reason,
            HaltReason::ActionError,
            "{expression}"
        );
        assert_eq!(engine.get_output("t"), Some("before"), "{expression}");
        assert_eq!(engine.fact_count(), 1);
    }
}
