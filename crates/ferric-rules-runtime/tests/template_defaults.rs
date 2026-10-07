//! Defaults and constraints share semantics across source and embedding entry points.

use ferric_rules_runtime::{Engine, EngineConfig, EngineError, HaltReason, RunLimit, Value};

fn integer(engine: &Engine, fact: ferric_rules_runtime::FactHandle, slot: &str) -> i64 {
    match engine.get_fact_slot_by_name(fact, slot).unwrap() {
        Value::Integer(value) => *value,
        value => panic!("expected integer, got {value:?}"),
    }
}

#[test]
fn host_assertions_compute_static_once_and_dynamic_only_when_omitted() {
    let mut engine = Engine::new(EngineConfig::default());
    engine
        .load_str(
            "(defglobal ?*calls* = 0)
      (deffunction next () (bind ?*calls* (+ ?*calls* 1)) ?*calls*)
      (deftemplate item (slot static (default (next)))
        (slot dynamic (default-dynamic (next))))",
        )
        .unwrap();
    assert!(matches!(
        engine.get_global("calls"),
        Some(Value::Integer(1))
    ));
    let first = engine.assert_template("item", &[], ()).unwrap();
    let supplied = engine
        .assert_template("item", &["dynamic"], [Value::Integer(20)])
        .unwrap();
    let third = engine.assert_template("item", &[], ()).unwrap();
    assert_eq!(
        (
            integer(&engine, first, "static"),
            integer(&engine, first, "dynamic")
        ),
        (1, 2)
    );
    assert_eq!(
        (
            integer(&engine, supplied, "static"),
            integer(&engine, supplied, "dynamic")
        ),
        (1, 20)
    );
    assert_eq!(
        (
            integer(&engine, third, "static"),
            integer(&engine, third, "dynamic")
        ),
        (1, 3)
    );
}

#[test]
fn host_rejects_invalid_supplied_slots_before_running_any_defaults() {
    let mut engine = Engine::with_rules(
        "(defglobal ?*calls* = 0)
      (deftemplate item
        (slot generated (default-dynamic (bind ?*calls* (+ ?*calls* 1))))
        (slot checked (type INTEGER) (range 1 3)))",
    )
    .unwrap();
    for (names, values) in [
        (vec!["checked"], vec![Value::Integer(4)]),
        (
            vec!["checked", "checked"],
            vec![Value::Integer(1), Value::Integer(2)],
        ),
        (vec!["unknown"], vec![Value::Integer(1)]),
    ] {
        assert!(engine.assert_template("item", &names, values).is_err());
        assert!(matches!(
            engine.get_global("calls"),
            Some(Value::Integer(0))
        ));
        assert_eq!(engine.fact_count(), 0);
    }
    let fact = engine.assert_template("item", &[], ()).unwrap();
    assert_eq!(integer(&engine, fact, "generated"), 1);
    assert_eq!(integer(&engine, fact, "checked"), 1);
}

#[test]
fn computed_invalid_defaults_never_publish_partial_facts() {
    for default in ["(+ 2 2)", "(create$ 1 2)"] {
        let mut engine = Engine::with_rules(&format!(
            "(deftemplate item
          (slot checked (type INTEGER) (range 1 3) (default-dynamic {default})))"
        ));
        if default.starts_with("(create$") {
            assert!(engine.is_err());
            continue;
        }
        let engine = engine.as_mut().unwrap();
        assert!(matches!(
            engine.assert_template("item", &[], ()),
            Err(EngineError::InvalidSlotValue { .. })
        ));
        assert_eq!(engine.fact_count(), 0);
        let fact = engine
            .assert_template("item", &["checked"], [Value::Integer(2)])
            .unwrap();
        assert_eq!(integer(engine, fact, "checked"), 2);
    }
    let mut engine = Engine::with_rules(
        "(deftemplate item
        (slot checked (range 1 3) (default-dynamic (+ 2 2))))
      (defrule run => (assert (item)) (assert (after)))",
    )
    .unwrap();
    assert_eq!(
        engine.run(RunLimit::Unlimited).unwrap().halt_reason,
        HaltReason::ActionError
    );
    assert_eq!(engine.fact_count(), 0);
    assert!(!engine.action_diagnostics().is_empty());
}

#[test]
fn cardinality_checks_the_entire_flattened_multislot() {
    let mut engine = Engine::with_rules(
        "(deftemplate item
      (multislot values (type INTEGER) (cardinality 3 4)
        (default (create$ 1) (create$ 2 3))))",
    )
    .unwrap();
    let default = engine.assert_template("item", &[], ()).unwrap();
    let Value::Multifield(values) = engine.get_fact_slot_by_name(default, "values").unwrap() else {
        panic!()
    };
    assert_eq!(values.len(), 3);
    assert!(engine
        .assert_template("item", &["values"], [Value::Integer(1)])
        .is_err());
    engine
        .load_str(
            "(defrule add => (bind ?more (create$ 3 4))
      (assert (item (values (create$ 1 2) ?more))))",
        )
        .unwrap();
    assert_eq!(
        engine.run(RunLimit::Unlimited).unwrap().halt_reason,
        HaltReason::AgendaEmpty
    );
    assert_eq!(engine.fact_count(), 2);
    assert!(engine
        .load_str("(defrule too-many => (assert (item (values (create$ 1 2) 3 4 5))))")
        .is_err());
}

#[test]
fn static_default_failure_preserves_previous_template_and_flushes_output() {
    let mut engine = Engine::with_rules("(deftemplate item (slot n (default 7)))").unwrap();
    assert!(engine
        .load_str(
            "(deffunction fail () (printout t attempted crlf) (/ 1 0))
      (deftemplate item (slot n (default (fail))))"
        )
        .is_err());
    assert_eq!(engine.get_output("t"), Some("attempted\n"));
    let fact = engine.assert_template("item", &[], ()).unwrap();
    assert_eq!(integer(&engine, fact, "n"), 7);
}

#[test]
fn static_default_cannot_assert_the_template_it_redefines() {
    // A Ferric-only rejection: for this ordered-form assert, CLIPS 6.30 creates
    // a second, implied `item` template instead (see docs/compatibility.md).
    let mut engine = Engine::with_rules("(deftemplate item (slot n (default 7)))").unwrap();
    let error = engine
        .load_str(
            "(deftemplate item (slot other
      (default (fact-index (assert (item))))))",
        )
        .unwrap_err();
    assert!(
        error
            .iter()
            .any(|error| error.to_string().contains("being redefined")),
        "{error:?}"
    );
    // The reference is rejected before the default runs, so nothing was asserted.
    assert_eq!(engine.fact_count(), 0);
    let fact = engine.assert_template("item", &[], ()).unwrap();
    assert_eq!(integer(&engine, fact, "n"), 7);
}

#[test]
fn dynamic_default_cannot_reference_the_template_it_redefines() {
    for default in [
        "(assert (item (original 7)))",
        "(find-all-facts ((?f item)) TRUE)",
        "(if FALSE then (any-factp ((?f item)) TRUE) else 0)",
    ] {
        let mut engine = Engine::with_rules("(deftemplate item (slot original))").unwrap();
        let error = engine
            .load_str(&format!(
                "(deftemplate item (slot replacement (default-dynamic {default})))"
            ))
            .unwrap_err();
        assert!(
            error
                .iter()
                .any(|error| error.to_string().contains("being redefined")),
            "{default}: {error:?}"
        );

        // The previous layout stays installed and asserts as before.
        let fact = engine
            .assert_template("item", &["original"], [Value::Integer(8)])
            .unwrap();
        assert_eq!(integer(&engine, fact, "original"), 8);
        engine.retract(fact).unwrap();

        // Because the rejected default was never installed, it does not keep
        // the template in use, and a valid redefinition still succeeds.
        engine
            .load_str("(deftemplate item (slot fixed (default-dynamic (+ 1 2))))")
            .unwrap();
        let fact = engine.assert_template("item", &[], ()).unwrap();
        assert_eq!(integer(&engine, fact, "fixed"), 3, "{default}");
    }
}

#[test]
fn dynamic_defaults_share_one_budget_across_slots_and_reset_it_after_errors() {
    let mut config = EngineConfig::default();
    config.max_action_loop_iterations = 3;
    let mut engine = Engine::new(config);
    engine
        .load_str(
            "(deftemplate item
      (slot a (default-dynamic (if (loop-for-count (?i 1 2) (+ ?i 1)) then 3 else 3)))
      (slot b (default-dynamic (if (loop-for-count (?i 1 2) (+ ?i 1)) then 3 else 3))))",
        )
        .unwrap();
    let error = engine.assert_template("item", &[], ()).unwrap_err();
    assert!(error.to_string().contains("iteration"), "{error}");
    assert_eq!(engine.fact_count(), 0);
    let fact = engine
        .assert_template("item", &["b"], [Value::Integer(9)])
        .unwrap();
    assert_eq!(integer(&engine, fact, "a"), 3);
    assert_eq!(integer(&engine, fact, "b"), 9);
}

#[test]
fn dynamic_default_recursion_obeys_existing_call_depth_limit() {
    let mut config = EngineConfig::default();
    config.max_call_depth = 3;
    let mut engine = Engine::new(config);
    engine
        .load_str(
            "(deffunction make () 1)
      (deftemplate item (slot n (default-dynamic (make))))
      (deffunction make () (assert (item)))",
        )
        .unwrap();
    let error = engine.assert_template("item", &[], ()).unwrap_err();
    assert!(error.to_string().contains("depth"), "{error}");
    assert_eq!(engine.fact_count(), 0);
}

#[test]
fn load_facts_evaluates_omitted_defaults_but_still_requires_literal_fields() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("defaults.fct");
    std::fs::write(&path, "(item)\n(item (n 20))\n(item (n (+ 1 2)))\n").unwrap();
    let escaped = path
        .to_str()
        .unwrap()
        .replace('\\', "\\\\")
        .replace('"', "\\\"");
    let mut engine = Engine::with_rules(&format!(
        "(defglobal ?*calls* = 0)
      (deftemplate item (slot n (default-dynamic (bind ?*calls* (+ ?*calls* 1)))))
      (defrule read => (load-facts \"{escaped}\"))"
    ))
    .unwrap();
    assert_eq!(
        engine.run(RunLimit::Unlimited).unwrap().halt_reason,
        HaltReason::ActionError
    );
    assert_eq!(engine.fact_count(), 2);
    assert!(matches!(
        engine.get_global("calls"),
        Some(Value::Integer(1))
    ));
    let values: Vec<_> = engine
        .facts()
        .unwrap()
        .map(|(fact, _)| integer(&engine, fact, "n"))
        .collect();
    assert_eq!(values, [1, 20]);
}

#[test]
fn enormous_derived_cardinality_fails_before_allocation_but_explicit_none_is_allowed() {
    let mut engine = Engine::new(EngineConfig::default());
    assert!(engine
        .load_str("(deftemplate item (multislot x (cardinality 9223372036854775807 ?VARIABLE)))")
        .is_err());
    engine.load_str("(deftemplate item (multislot x (cardinality 9223372036854775807 ?VARIABLE) (default ?NONE)))").unwrap();
    assert!(engine.assert_template("item", &[], ()).is_err());
    assert_eq!(engine.fact_count(), 0);
}

#[test]
fn defaults_reject_nonlocal_return_but_allow_callable_returns() {
    for kind in ["default", "default-dynamic"] {
        for body in [
            "(return 7)",
            "(if FALSE then (return 7) else 1)",
            "(fact-index (assert (value (return 7))))",
        ] {
            let mut engine = Engine::new(EngineConfig::default());
            assert!(engine
                .load_str(&format!("(deftemplate item (slot n ({kind} {body})))"))
                .is_err());
        }
    }
    let mut engine = Engine::with_rules(
        "(deffunction value () (return 7))
      (deftemplate item (slot n (default-dynamic (value))))",
    )
    .unwrap();
    let fact = engine.assert_template("item", &[], ()).unwrap();
    assert_eq!(integer(&engine, fact, "n"), 7);
}

#[test]
fn dynamic_scalar_void_defaults_become_nil_but_static_void_defaults_are_invalid() {
    let mut engine = Engine::new(EngineConfig::default());
    assert!(engine
        .load_str("(deftemplate invalid (slot a (default (printout t static crlf))))")
        .is_err());
    assert_eq!(engine.get_output("t"), Some("static\n"));
    engine
        .load_str("(deftemplate item (slot b (default-dynamic (printout t dynamic crlf))))")
        .unwrap();
    let fact = engine.assert_template("item", &[], ()).unwrap();
    assert_eq!(engine.get_output("t"), Some("static\ndynamic\n"));
    let Value::Symbol(value) = engine.get_fact_slot_by_name(fact, "b").unwrap() else {
        panic!()
    };
    assert_eq!(engine.resolve_core_symbol(*value), Some("nil"));
}

#[test]
fn static_multislot_void_elements_are_invalid_but_dynamic_ones_are_omitted() {
    let mut engine = Engine::with_rules("(deftemplate item (multislot m (default 9)))").unwrap();
    // CLIPS evaluates every element, then rejects the default (CSTRNCHK1).
    assert!(engine
        .load_str(
            "(deffunction nothing () (printout t nothing crlf))
      (deftemplate item (multislot m (default 1 (nothing) (printout t after crlf) 2)))"
        )
        .is_err());
    assert_eq!(engine.get_output("t"), Some("nothing\nafter\n"));
    let multifield = |engine: &Engine, fact| match engine.get_fact_slot_by_name(fact, "m").unwrap()
    {
        Value::Multifield(values) => values
            .iter()
            .map(|value| match value {
                Value::Integer(value) => *value,
                value => panic!("expected integer, got {value:?}"),
            })
            .collect::<Vec<_>>(),
        value => panic!("expected multifield, got {value:?}"),
    };
    // The previous definition stays installed.
    let fact = engine.assert_template("item", &[], ()).unwrap();
    assert_eq!(multifield(&engine, fact), [9]);
    engine.retract(fact).unwrap();

    engine
        .load_str("(deftemplate item (multislot m (default-dynamic 1 (nothing) 2)))")
        .unwrap();
    let fact = engine.assert_template("item", &[], ()).unwrap();
    assert_eq!(multifield(&engine, fact), [1, 2]);
}

#[test]
fn computed_constraint_failures_preserve_original_facts_across_all_mutations() {
    for (slot, computed, initial) in [
        ("(slot n (type INTEGER) (range 1 5))", "(+ 5 1)", "3"),
        ("(slot n (allowed-integers 1 3))", "(+ 1 1)", "3"),
        ("(multislot n (cardinality 1 2))", "(create$ 1 2 3)", "1 2"),
    ] {
        for action in [
            "(assert (item (n ?bad)))",
            "(modify ?f (n ?bad))",
            "(duplicate ?f (n ?bad))",
        ] {
            let mut engine = Engine::with_rules(&format!(
                "(deftemplate item {slot})
              (deffacts seed (item (n {initial})))
              (defrule invalid ?f <- (item) => (bind ?bad {computed})
                {action} (assert (after)))"
            ))
            .unwrap();
            let original = engine.facts().unwrap().next().unwrap().0;
            let value = engine.get_fact_slot_by_name(original, "n").unwrap().clone();
            assert_eq!(
                engine.run(RunLimit::Unlimited).unwrap().halt_reason,
                HaltReason::ActionError,
                "{slot}: {action}"
            );
            assert_eq!(engine.fact_count(), 1, "{slot}: {action}");
            assert!(engine
                .get_fact_slot_by_name(original, "n")
                .unwrap()
                .structural_eq(&value));
            assert!(engine.find_facts("after").unwrap().is_empty());
        }
    }
}

#[test]
fn invalid_earlier_default_prevents_later_definition_effects() {
    for invalid in [
        "(type INTEGER) (default (str-cat x))",
        "(range 1 3) (default (+ 2 2))",
        "(allowed-symbols red) (default-dynamic blue)",
    ] {
        let mut engine = Engine::new(EngineConfig::default());
        engine.load_str("(defglobal ?*ticks* = 0)").unwrap();
        assert!(engine
            .load_str(&format!(
                "(deftemplate item (slot a {invalid})
          (slot b (default (bind ?*ticks* (+ ?*ticks* 1)))))"
            ))
            .is_err());
        assert!(
            matches!(engine.get_global("ticks"), Some(Value::Integer(0))),
            "{invalid}"
        );
        assert!(matches!(
            engine.assert_template("item", &[], ()),
            Err(EngineError::TemplateNotFound(_))
        ));
    }
}

fn diagnostics(engine: &Engine) -> String {
    engine
        .action_diagnostics()
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn funcall_return_in_a_dynamic_default_does_not_return_from_the_asserting_function() {
    let mut engine = Engine::with_rules(
        "(deftemplate item (slot n (default-dynamic (funcall return 7))))
      (deffunction make () (assert (item)) (printout t after crlf) 1)
      (defrule run => (printout t result \" \" (make) crlf))",
    )
    .unwrap();
    assert_eq!(
        engine.run(RunLimit::Unlimited).unwrap().halt_reason,
        HaltReason::ActionError
    );
    let output = engine.get_output("t").unwrap_or_default();
    assert!(!output.contains("result 7"), "{output}");
    assert!(!output.contains("after"), "{output}");
    let diagnostics = diagnostics(&engine);
    assert!(
        diagnostics.contains("not valid outside a callable"),
        "{diagnostics}"
    );
    assert!(!diagnostics.contains("internal"), "{diagnostics}");
    assert!(engine.find_facts("item").unwrap().is_empty());
}

#[test]
fn funcall_break_in_a_dynamic_default_does_not_end_the_enclosing_loop() {
    let mut engine = Engine::with_rules(
        "(deftemplate item (slot n (default-dynamic (funcall break))))
      (defrule run =>
        (loop-for-count (?i 1 3) (assert (item)) (printout t ?i crlf))
        (printout t done crlf))",
    )
    .unwrap();
    assert_eq!(
        engine.run(RunLimit::Unlimited).unwrap().halt_reason,
        HaltReason::ActionError
    );
    assert!(!engine.get_output("t").unwrap_or_default().contains("done"));
    let diagnostics = diagnostics(&engine);
    assert!(
        diagnostics.contains("not valid outside a loop"),
        "{diagnostics}"
    );
    assert!(engine.find_facts("item").unwrap().is_empty());
}

#[test]
fn funcall_return_in_root_defaults_and_deffacts_reports_the_user_error() {
    let mut engine = Engine::new(EngineConfig::default());
    let error = engine
        .load_str("(deftemplate item (slot n (default (funcall return 7))))")
        .unwrap_err();
    let message = format!("{error:?}");
    assert!(
        message.contains("not valid outside a callable"),
        "{message}"
    );
    assert!(!message.contains("internal"), "{message}");

    let mut engine = Engine::new(EngineConfig::default());
    engine
        .load_str(
            "(deftemplate item (slot n))
      (deffacts seed (item (n (funcall return 7))))",
        )
        .unwrap();
    let message = engine
        .reset()
        .map_err(|error| error.to_string())
        .unwrap_err();
    assert!(
        message.contains("not valid outside a callable"),
        "{message}"
    );
    assert!(!message.contains("internal"), "{message}");
}

#[test]
fn slot_violations_report_their_entry_point_without_a_template_slot_wrapper() {
    let mut engine = Engine::with_rules(
        "(deftemplate item (slot n (range 1 3)))
      (defrule bad => (assert (item (n (+ 2 2)))))",
    )
    .unwrap();
    assert_eq!(
        engine.run(RunLimit::Unlimited).unwrap().halt_reason,
        HaltReason::ActionError
    );
    let diagnostics = diagnostics(&engine);
    assert!(diagnostics.contains("`assert`"), "{diagnostics}");
    assert!(diagnostics.contains("line"), "{diagnostics}");
    assert!(!diagnostics.contains("template slot"), "{diagnostics}");

    let mut engine = Engine::new(EngineConfig::default());
    engine
        .load_str(
            "(deftemplate item (slot n (range 1 3)))
      (deffacts seed (item (n (+ 2 2))))",
        )
        .unwrap();
    let message = engine
        .reset()
        .map_err(|error| error.to_string())
        .unwrap_err();
    assert!(!message.contains("unsupported operation"), "{message}");
    assert!(message.contains("range"), "{message}");

    let mut engine =
        Engine::with_rules("(deftemplate item (slot n (range 1 3) (default-dynamic (+ 2 2))))")
            .unwrap();
    let Err(EngineError::InvalidSlotValue { slot, reason, .. }) =
        engine.assert_template("item", &[], ())
    else {
        panic!("a computed violation must be an invalid slot value");
    };
    assert_eq!(slot, "n");
    assert!(!reason.contains("unsupported operation"), "{reason}");
    assert!(reason.contains("range"), "{reason}");
}
