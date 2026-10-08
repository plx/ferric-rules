use ferric_rules_runtime::{Engine, EngineConfig, RunLimit, Value};

#[test]
fn ordered_agenda_inspection_matches_firing_for_each_strategy() {
    use ferric_rules_core::ConflictResolutionStrategy;
    for strategy in [
        ConflictResolutionStrategy::Depth,
        ConflictResolutionStrategy::Breadth,
        ConflictResolutionStrategy::Lex,
        ConflictResolutionStrategy::Mea,
    ] {
        let mut engine = Engine::with_rules_config(
            "(deffacts seed (item 1) (item 2) (item 3))
             (defrule show (item ?x) => (printout t ?x crlf))",
            EngineConfig::default().with_strategy(strategy),
        )
        .unwrap();
        let entries = engine.agenda_entries();
        let expected: Vec<_> = if strategy == ConflictResolutionStrategy::Breadth {
            vec![1, 2, 3]
        } else {
            vec![3, 2, 1]
        };
        assert_eq!(
            entries
                .iter()
                .map(|entry| entry.basis[0].unwrap())
                .collect::<Vec<_>>(),
            expected
        );
        engine.run(RunLimit::Unlimited).unwrap();
        let output: Vec<u64> = engine
            .get_output("t")
            .unwrap()
            .lines()
            .map(|line| line.parse().unwrap())
            .collect();
        assert_eq!(output, expected, "{strategy:?}");
    }
}

#[test]
fn public_fact_indices_survive_slot_reuse_and_reject_stale_or_foreign_handles() {
    let mut engine = Engine::new(EngineConfig::default());
    let first = engine.assert_ordered("item", 1).unwrap();
    let second = engine.assert_ordered("item", 2).unwrap();
    assert_eq!(engine.public_fact_index(first), Some(1));
    assert_eq!(engine.public_fact_index(second), Some(2));
    engine.retract(first).unwrap();
    let replacement = engine.assert_ordered("item", 3).unwrap();
    assert_eq!(engine.public_fact_index(first), None);
    assert_eq!(engine.public_fact_index(replacement), Some(3));
    let mut other = Engine::new(EngineConfig::default());
    let foreign = other.assert_ordered("item", 1).unwrap();
    assert_eq!(engine.public_fact_index(foreign), None);
    engine.reset().unwrap();
    assert_eq!(engine.public_fact_index(replacement), None);
    let reset = engine.assert_ordered("item", 4).unwrap();
    assert_eq!(engine.public_fact_index(reset), Some(1));
}

#[test]
fn prompt_and_fact_formatting_preserve_template_shape_and_clips_value_spelling() {
    let mut engine = Engine::with_rules(
        r#"(deftemplate person (slot name) (slot number) (multislot tags))
           (deffacts seed (person (name "alice") (number 1e20) (tags crlf "quoted")))"#,
    )
    .unwrap();
    let (_, fact) = engine.facts().unwrap().next().unwrap();
    assert_eq!(
        engine.format_fact(fact).unwrap(),
        "(person (name \"alice\") (number 1e+20) (tags crlf \"quoted\"))"
    );
    let control = engine.eval_str("crlf").unwrap();
    assert_eq!(engine.format_value(&control), "crlf");
    let quoted = engine.eval_str(r#""text""#).unwrap();
    assert_eq!(engine.format_value(&quoted), "\"text\"");
    let values = engine.eval_str(r#"(create$ crlf "text" 1e20)"#).unwrap();
    assert_eq!(engine.format_value(&values), "(crlf \"text\" 1e+20)");
    assert_eq!(engine.format_value(&Value::Void), "");
    engine
        .eval_str("(assert (person (name empty) (number 1)))")
        .unwrap();
    assert!(engine.facts().unwrap().any(|(_, fact)| {
        engine.format_fact(fact).unwrap() == "(person (name empty) (number 1) (tags))"
    }));
}

#[test]
fn agenda_entries_follow_strategy_and_preserve_outer_conditional_element_basis() {
    let mut engine = Engine::with_rules(
        "(deffacts seed (q) (p 1) (p 2))
         (defrule plain (p ?x) =>)
         (defrule complex (declare (salience 10)) (p ?x) (not (missing)) (exists (q)) (test (> ?x 0)) =>)
         (defrule empty =>)",
    ).unwrap();
    let rows = engine.agenda_entries();
    assert_eq!(rows.len(), 5);
    assert_eq!(rows[0].rule_name, "complex");
    assert_eq!(rows[0].basis, vec![Some(3), None, None]);
    assert_eq!(rows[0].format_basis(), "f-3,*,*");
    assert_eq!(rows[0].format_line(), "10     complex: f-3,*,*");
    assert_eq!(rows[1].basis, vec![Some(2), None, None]);
    assert_eq!(rows[4].rule_name, "empty");
    assert_eq!(rows[4].basis, vec![None]);
    assert_eq!(engine.agenda_entries(), rows);
    assert_eq!(engine.agenda_len(), 5);
    engine.run(RunLimit::Count(1)).unwrap();
    assert_eq!(engine.agenda_entries()[0], rows[1]);
}

#[test]
fn agenda_module_selection_does_not_follow_or_change_focus() {
    let mut engine =
        Engine::with_rules("(defrule main =>) (defmodule OTHER) (defrule other =>)").unwrap();
    engine.load_str("(defrule MAIN::another =>)").unwrap();
    engine.eval_str("(focus MAIN)").unwrap();
    let current = engine.agenda_entries();
    assert!(current
        .iter()
        .all(|entry| entry.module_name == engine.current_module()));
    assert_eq!(
        engine.agenda_entries_in_module("OTHER").unwrap()[0].rule_name,
        "other"
    );
    assert_eq!(engine.agenda_entries_in_module("*").unwrap().len(), 3);
    assert!(engine.agenda_entries_in_module("missing").is_err());
    engine.step().unwrap();
    assert_eq!(engine.agenda_entries_in_module("OTHER").unwrap().len(), 1);
    assert_eq!(engine.agenda_entries_in_module("MAIN").unwrap().len(), 1);
}

#[test]
fn watch_records_each_mutation_and_captures_rule_basis_before_retraction() {
    let mut engine = Engine::with_rules(
        "(deftemplate person (slot age))
         (deffacts seed (person (age 30)))
         (defrule birthday ?p <- (person (age 30)) =>
             (modify ?p (age 31))
             (bind ?tmp (assert (tmp))) (retract ?tmp))",
    )
    .unwrap();
    engine.set_watch_facts(true);
    engine.set_watch_rules(true);
    engine.run(RunLimit::Unlimited).unwrap();
    let trace = engine.get_output("wtrace").unwrap();
    let lines: Vec<_> = trace.lines().collect();
    assert_eq!(lines.len(), 5);
    assert!(lines[0].contains("birthday: f-1"), "{trace}");
    assert!(lines[1].starts_with("<== f-1 ") && lines[1].ends_with("(person (age 30))"));
    assert!(lines[2].starts_with("==> f-2 ") && lines[2].ends_with("(person (age 31))"));
    assert!(lines[3].starts_with("==> f-3 ") && lines[3].ends_with("(tmp)"));
    assert!(lines[4].starts_with("<== f-3 ") && lines[4].ends_with("(tmp)"));
    engine.clear_output_channel("wtrace");
    engine.eval_str("(assert (person (age 31)))").unwrap();
    assert!(engine.get_output("wtrace").is_none());
}

#[test]
fn watch_events_keep_cross_channel_order_and_reset_and_clear_mutations() {
    let mut engine = Engine::with_rules("(deffacts seed (item 1))").unwrap();
    engine.enable_output_events();
    engine.eval_str("(watch facts)").unwrap();
    engine.eval_str("(progn (printout t before) (bind ?f (assert (tmp))) (printout stdout between) (retract ?f) (printout t after))").unwrap();
    let events = engine.drain_output_events();
    assert_eq!(
        events
            .iter()
            .map(|(channel, _)| channel.as_str())
            .collect::<Vec<_>>(),
        ["t", "wtrace", "stdout", "wtrace", "t"]
    );
    assert_eq!(events[0].1, "before");
    assert!(events[1].1.contains("(tmp)"));
    assert_eq!(events[2].1, "between");
    assert!(events[3].1.starts_with("<=="));
    engine.reset().unwrap();
    let reset = engine
        .drain_output_events()
        .into_iter()
        .map(|(_, text)| text)
        .collect::<String>();
    assert!(reset.contains("<== f-1"), "{reset}");
    assert!(reset.contains("==> f-1"), "{reset}");
    engine.clear();
    let cleared = engine
        .drain_output_events()
        .into_iter()
        .map(|(_, text)| text)
        .collect::<String>();
    assert!(cleared.contains("<== f-1"), "{cleared}");
    assert!(engine.watch_facts());
}

#[cfg(feature = "serde")]
#[test]
fn snapshots_keep_agenda_order_but_do_not_restore_watch_settings() {
    use ferric_rules_runtime::SerializationFormat;
    let mut engine =
        Engine::with_rules("(deffacts seed (p 1) (p 2)) (defrule r (p ?x) =>)").unwrap();
    engine.set_watch_rules(true);
    engine.set_watch_facts(true);
    for &format in SerializationFormat::ALL {
        let bytes = engine.serialize(format).unwrap();
        let restored = Engine::deserialize(&bytes, format).unwrap();
        assert_eq!(restored.agenda_entries(), engine.agenda_entries());
        assert!(!restored.watch_facts());
        assert!(!restored.watch_rules());
    }
}

#[test]
fn source_clear_preserves_watches_and_only_traces_its_new_initial_fact() {
    let mut engine = Engine::with_rules("(deffacts seed (item 1))").unwrap();
    engine.enable_output_events();
    engine.eval_str("(watch all)").unwrap();
    assert!(matches!(engine.eval_str("(clear)").unwrap(), Value::Void));
    assert_eq!(
        engine.drain_output_events(),
        vec![("wtrace".into(), "==> f-0     (initial-fact)\n".into())]
    );
    assert!(engine.watch_facts());
    assert!(engine.watch_rules());
    let fact = engine.assert_ordered("after", 7).unwrap();
    assert_eq!(engine.public_fact_index(fact), Some(1));
    assert_eq!(
        engine.drain_output_events(),
        vec![("wtrace".into(), "==> f-1     (after 7)\n".into())]
    );
}

#[test]
fn rule_agenda_action_uses_ordered_rows_and_fact_basis() {
    let mut engine = Engine::with_rules(
        "(deffacts seed (item 1) (item 2))
         (defrule report (declare (salience 10)) => (agenda))
         (defrule pending (item ?x) =>)",
    )
    .unwrap();
    engine.run(RunLimit::Count(1)).unwrap();
    assert_eq!(
        engine.get_output("t"),
        Some("0      pending: f-2\n0      pending: f-1\nFor a total of 2 activations.\n")
    );
}

#[test]
fn firing_watch_ordinals_continue_a_run_and_restart_for_a_new_run() {
    let mut engine = Engine::with_rules(
        "(deffacts seed (item 1) (item 2) (item 3))
         (defrule show (item ?x) =>)",
    )
    .unwrap();
    engine.set_watch_rules(true);
    engine.run(RunLimit::Count(1)).unwrap();
    engine.continue_run(RunLimit::Count(1)).unwrap();
    engine.run(RunLimit::Count(1)).unwrap();
    assert_eq!(
        engine.get_output("wtrace"),
        Some("FIRE    1 show: f-3\nFIRE    2 show: f-2\nFIRE    1 show: f-1\n")
    );
}

#[test]
fn fact_listing_includes_initial_fact_and_rule_names_follow_the_current_module() {
    let mut engine = Engine::new(EngineConfig::default());
    engine
        .load_str(
            "(defrule b =>) (defrule a =>) (defmodule EXTRA) (defrule c =>)
             (deffacts seed (item 1))",
        )
        .unwrap();
    assert_eq!(engine.current_module_rule_names(), ["c"]);
    engine.reset().unwrap();
    assert_eq!(engine.current_module_rule_names(), ["b", "a"]);
    let listing: Vec<_> = engine
        .fact_listing()
        .into_iter()
        .map(|(index, fact)| (index, engine.format_fact(fact).unwrap()))
        .collect();
    assert_eq!(
        listing,
        [(0, "(initial-fact)".to_owned()), (1, "(item 1)".to_owned())]
    );
    // Host fact queries keep hiding the protected fact.
    assert_eq!(engine.facts().unwrap().count(), 1);
    engine.eval_str("(clear)").unwrap();
    assert!(engine.current_module_rule_names().is_empty());
    assert_eq!(engine.fact_listing().len(), 1);
}

fn watch_engine() -> Engine {
    Engine::with_rules(
        "(deftemplate item (slot x))
         (deffunction f () 1)
         (defglobal ?*g* = 1)
         (defgeneric gg)
         (defmethod gg ((?x INTEGER)) ?x)
         (defrule r =>)",
    )
    .unwrap()
}

#[test]
fn watch_accepts_every_clips_item_and_construct_name_forms() {
    let mut engine = watch_engine();
    for item in [
        "facts",
        "instances",
        "slots",
        "rules",
        "activations",
        "messages",
        "message-handlers",
        "generic-functions",
        "methods",
        "deffunctions",
        "compilations",
        "statistics",
        "globals",
        "focus",
        "all",
    ] {
        for function in ["watch", "unwatch"] {
            assert!(
                matches!(
                    engine.eval_str(&format!("({function} {item})")).unwrap(),
                    Value::Void
                ),
                "({function} {item})"
            );
        }
    }
    // Names are checked against the construct type CLIPS uses for the item.
    for (item, names) in [
        ("facts", "item initial-fact"),
        ("rules", "r"),
        ("activations", "MAIN::r"),
        ("deffunctions", "f"),
        ("globals", "g"),
        ("generic-functions", "gg"),
        ("methods", "gg"),
        ("instances", "USER"),
        ("slots", "USER"),
        ("message-handlers", "USER"),
    ] {
        for function in ["watch", "unwatch"] {
            let source = format!("({function} {item} {names})");
            assert!(
                matches!(engine.eval_str(&source).unwrap(), Value::Void),
                "{source}"
            );
        }
    }
    // Tracing stays global: naming one template traces every fact.
    engine.eval_str("(watch facts item)").unwrap();
    assert!(engine.watch_facts());
    engine.eval_str("(assert (other 1))").unwrap();
    assert_eq!(engine.get_output("wtrace"), Some("==> f-1     (other 1)\n"));
    engine.eval_str("(unwatch facts item)").unwrap();
    assert!(!engine.watch_facts());
}

#[test]
fn watch_rejects_unknown_items_and_names_without_changing_state() {
    let mut engine = watch_engine();
    for (source, expected) in [
        (
            "(watch bogus)",
            "argument #1 to be of type watchable symbol",
        ),
        (
            "(unwatch bogus)",
            "argument #1 to be of type watchable symbol",
        ),
        (
            "(watch (str-cat facts))",
            "argument #1 to be of type symbol",
        ),
        (
            "(watch facts nope)",
            "argument #2 to be of type deftemplate",
        ),
        (
            "(watch facts item nope)",
            "argument #3 to be of type deftemplate",
        ),
        (
            "(watch facts \"item\")",
            "argument #2 to be of type deftemplate",
        ),
        ("(watch rules nope)", "argument #2 to be of type defrule"),
        (
            "(watch activations nope)",
            "argument #2 to be of type defrule",
        ),
        (
            "(watch deffunctions nope)",
            "argument #2 to be of type deffunction",
        ),
        (
            "(unwatch deffunctions nope)",
            "argument #2 to be of type deffunction",
        ),
        (
            "(watch globals nope)",
            "argument #2 to be of type defglobal",
        ),
        (
            "(watch generic-functions nope)",
            "argument #2 to be of type defgeneric",
        ),
        (
            "(watch methods nope)",
            "argument #2 to be of type generic function name",
        ),
        ("(watch focus MAIN)", "expected 1, got 2"),
        ("(watch all r)", "expected 1, got 2"),
    ] {
        let error = engine.eval_str(source).unwrap_err().to_string();
        assert!(error.contains(expected), "{source}: {error}");
    }
    // A rejected name leaves the watch state unchanged.
    assert!(!engine.watch_facts());
}

#[test]
fn source_clear_refused_during_execution_removes_facts_without_retraction_traces() {
    let mut engine = Engine::with_rules(
        "(deffacts seed (item 1) (item 2))
         (defrule clrr (item 1) => (clear) (assert (z)))",
    )
    .unwrap();
    engine.reset().unwrap();
    engine.enable_output_events();
    engine.set_watch_facts(true);
    engine.run(RunLimit::Unlimited).unwrap();
    let events = engine.drain_output_events();
    assert_eq!(
        events,
        vec![
            (
                "werror".into(),
                "[CONSTRCT1] Some constructs are still in use. Clear cannot continue.\n".into()
            ),
            ("wtrace".into(), "==> f-0     (z)\n".into()),
        ]
    );
}
