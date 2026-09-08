//! Nonfatal evaluator diagnostics preserve returned values and execution.

use ferric_rules_core::Value;
use ferric_rules_runtime::{Engine, EngineConfig, HaltReason, RunLimit};

const WARNING_SOURCE: &str = r"
    (defglobal ?*trace* = 0 ?*result* = pending)
    (deffunction mark (?digit ?value)
      (bind ?*trace* (+ (* ?*trace* 10) ?digit)) ?value)
    (deffacts requests (request first))
    (defrule warn (request ?key) =>
      (bind ?*result* (sort (mark 1 missing-comparator) (mark 2 (create$ 3 1 2))))
      (assert (after-warning ?key)))
    (defrule next (after-warning ?key) => (assert (after-next ?key)))
";

fn assert_false(engine: &Engine, name: &str) {
    let Some(Value::Symbol(symbol)) = engine.get_global(name) else {
        panic!("{name} must contain actual SYMBOL FALSE")
    };
    assert_eq!(engine.resolve_core_symbol(*symbol), Some("FALSE"));
}

fn assert_sort_warning(engine: &Engine) {
    assert_eq!(engine.action_diagnostics().len(), 1);
    assert!(engine.action_diagnostics()[0].to_string().contains("sort"));
}

fn run_warning_and_follower(engine: &mut Engine) {
    let run = engine.run(RunLimit::Count(10)).unwrap();
    assert_eq!(run.halt_reason, HaltReason::AgendaEmpty);
    assert_eq!(run.rules_fired, 2);
    assert_false(engine, "result");
    assert_sort_warning(engine);
}

#[test]
fn warning_returns_false_and_allows_later_actions_and_activations() {
    let mut engine = Engine::with_rules(WARNING_SOURCE).unwrap();
    run_warning_and_follower(&mut engine);
    assert!(matches!(
        engine.get_global("trace"),
        Some(Value::Integer(1))
    ));
    assert_eq!(engine.find_facts("after-warning").unwrap().len(), 1);
    assert_eq!(engine.find_facts("after-next").unwrap().len(), 1);

    let run = engine.run(RunLimit::Count(10)).unwrap();
    assert_eq!(run.rules_fired, 0);
    assert_eq!(run.halt_reason, HaltReason::AgendaEmpty);
    assert!(engine.action_diagnostics().is_empty());
    assert_false(&engine, "result");
}

#[test]
fn continuation_retains_warnings_while_a_fresh_step_clears_them() {
    let mut engine = Engine::with_rules(WARNING_SOURCE).unwrap();
    let first = engine.run(RunLimit::Count(1)).unwrap();
    assert_eq!(first.halt_reason, HaltReason::LimitReached);
    assert_eq!(first.rules_fired, 1);
    assert_sort_warning(&engine);
    assert_false(&engine, "result");
    let continued = engine.continue_run(RunLimit::Count(10)).unwrap();
    assert_eq!(continued.halt_reason, HaltReason::AgendaEmpty);
    assert_eq!(continued.rules_fired, 1);
    assert_sort_warning(&engine);
    assert!(engine.step().unwrap().is_none());
    assert!(engine.action_diagnostics().is_empty());

    engine.reset().unwrap();
    assert!(engine.step().unwrap().is_some());
    assert_sort_warning(&engine);
    assert!(engine.step().unwrap().is_some());
    assert!(engine.action_diagnostics().is_empty());
    assert_eq!(engine.find_facts("after-next").unwrap().len(), 1);
}

#[test]
fn load_time_warnings_are_observable_without_leaking_into_another_load() {
    let mut engine = Engine::new(EngineConfig::utf8());
    let loaded = engine
        .load_str("(defglobal ?*result* = (sort missing-comparator))")
        .unwrap();
    assert_eq!(loaded.warnings.len(), 1);
    assert!(loaded.warnings[0].contains("sort"));
    assert_sort_warning(&engine);
    assert_false(&engine, "result");

    let next = engine
        .load_str("(defrule done => (assert (done)))")
        .unwrap();
    assert!(next.warnings.is_empty());
    assert_sort_warning(&engine);
    let run = engine.run(RunLimit::Count(10)).unwrap();
    assert_eq!(run.rules_fired, 1);
    assert_eq!(run.halt_reason, HaltReason::AgendaEmpty);
    assert!(engine.action_diagnostics().is_empty());
}

#[test]
fn a_later_load_error_preserves_an_earlier_warning_and_initialized_false() {
    let mut engine = Engine::new(EngineConfig::utf8());
    let failed =
        engine.load_str("(defglobal ?*result* = (sort missing-comparator) ?*broken* = (/ 1 0))");
    assert!(failed.is_err());
    assert_false(&engine, "result");
    assert!(engine.get_global("broken").is_none());
    assert_sort_warning(&engine);

    let loaded = engine
        .load_str("(defrule done => (assert (done)))")
        .unwrap();
    assert!(loaded.warnings.is_empty());
    let run = engine.run(RunLimit::Count(10)).unwrap();
    assert_eq!(run.halt_reason, HaltReason::AgendaEmpty);
    assert_eq!(run.rules_fired, 1);
    assert!(engine.action_diagnostics().is_empty());
}

#[test]
fn match_time_warning_is_drained_before_host_assertion_returns() {
    let mut engine = Engine::with_rules(
        "(defrule blocked (item ?value) (test (sort missing-comparator))
           => (assert (unexpected)))",
    )
    .unwrap();
    engine.assert_ordered("item", vec![1_i64]).unwrap();
    assert_sort_warning(&engine);
    assert_eq!(engine.agenda_len(), 0);
    assert!(engine.find_facts("unexpected").unwrap().is_empty());

    #[cfg(feature = "serde")]
    for &format in ferric_rules_runtime::SerializationFormat::ALL {
        let restored = Engine::deserialize(&engine.serialize(format).unwrap(), format).unwrap();
        assert_sort_warning(&restored);
    }
    let run = engine.run(RunLimit::Count(10)).unwrap();
    assert_eq!(run.rules_fired, 0);
    assert!(engine.action_diagnostics().is_empty());
}

#[test]
fn reset_and_clear_remove_previous_warning_state() {
    let mut engine = Engine::with_rules(WARNING_SOURCE).unwrap();
    run_warning_and_follower(&mut engine);
    engine.reset().unwrap();
    assert!(engine.action_diagnostics().is_empty());
    run_warning_and_follower(&mut engine);
    engine.clear();
    assert!(engine.action_diagnostics().is_empty());
    assert!(engine.get_global("result").is_none());
    let loaded = engine
        .load_str("(defrule done => (assert (done)))")
        .unwrap();
    assert!(loaded.warnings.is_empty());
    let run = engine.run(RunLimit::Count(10)).unwrap();
    assert_eq!(run.rules_fired, 1);
    assert!(engine.action_diagnostics().is_empty());
}

#[cfg(feature = "serde")]
#[test]
fn warnings_and_false_survive_all_codecs_without_refiring_or_stale_diagnostics() {
    use ferric_rules_runtime::SerializationFormat;

    for &format in SerializationFormat::ALL {
        let pending = Engine::with_rules(WARNING_SOURCE).unwrap();
        let mut engine = Engine::deserialize(&pending.serialize(format).unwrap(), format).unwrap();
        run_warning_and_follower(&mut engine);
        let diagnostic = engine.action_diagnostics()[0].to_string();
        let mut completed =
            Engine::deserialize(&engine.serialize(format).unwrap(), format).unwrap();
        assert_false(&completed, "result");
        assert_sort_warning(&completed);
        assert_eq!(completed.action_diagnostics()[0].to_string(), diagnostic);
        let run = completed.run(RunLimit::Count(10)).unwrap();
        assert_eq!(run.rules_fired, 0);
        assert_eq!(run.halt_reason, HaltReason::AgendaEmpty);
        assert!(completed.action_diagnostics().is_empty());
        let second = completed.intern_symbol("second").unwrap();
        completed.assert_ordered("request", vec![second]).unwrap();
        run_warning_and_follower(&mut completed);
        assert_eq!(completed.find_facts("after-next").unwrap().len(), 2);
    }
}

const HALT_SOURCE: &str = r"
    (defglobal ?*result* = pending)
    (deffunction broken (?a ?b) (/ 1 0))
    (deffacts requests (request))
    (defrule fail (declare (salience 10)) (request) =>
      (bind ?*result* (sort broken 3 1))
      (assert (unexpected)))
    (defrule follower (request) => (assert (followed)))
";

fn assert_retained_sort_value(engine: &Engine) {
    let Some(Value::Multifield(values)) = engine.get_global("result") else {
        panic!("failed sort must return the retained multifield")
    };
    assert!(matches!(
        values.as_slice(),
        [Value::Integer(3), Value::Integer(1)]
    ));
}

fn run_deferred_failure(engine: &mut Engine) {
    let result = engine.run(RunLimit::Count(10)).unwrap();
    assert_eq!(result.halt_reason, HaltReason::ActionError);
    assert_eq!(result.rules_fired, 1);
    assert_retained_sort_value(engine);
    assert!(!engine.action_diagnostics().is_empty());
    assert!(engine.find_facts("unexpected").unwrap().is_empty());
    assert!(engine.find_facts("followed").unwrap().is_empty());
}

#[test]
fn deferred_failure_preserves_current_bind_and_stops_later_rhs_and_activations() {
    let mut engine = Engine::with_rules(HALT_SOURCE).unwrap();
    run_deferred_failure(&mut engine);
    let resumed = engine.run(RunLimit::Count(10)).unwrap();
    assert_eq!(resumed.halt_reason, HaltReason::AgendaEmpty);
    assert_eq!(resumed.rules_fired, 1);
    assert!(engine.action_diagnostics().is_empty());
    assert_retained_sort_value(&engine);
    assert_eq!(engine.find_facts("followed").unwrap().len(), 1);

    engine.reset().unwrap();
    assert!(engine.action_diagnostics().is_empty());
    run_deferred_failure(&mut engine);
    engine.clear();
    assert!(engine.action_diagnostics().is_empty());
    engine
        .load_str("(defrule done => (assert (done)))")
        .unwrap();
    assert_eq!(engine.run(RunLimit::Count(10)).unwrap().rules_fired, 1);
}

#[test]
fn deferred_failure_stops_nested_action_bodies_after_the_completed_expression() {
    for body in [
        "(if TRUE then (bind ?*result* (sort broken 3 1)) (assert (inside)))",
        "(while TRUE do (bind ?*result* (sort broken 3 1)) (assert (inside)))",
        "(loop-for-count (?i 1 3) do (bind ?*result* (sort broken 3 1)) (assert (inside)))",
        "(progn$ (?i (create$ 1 2)) (bind ?*result* (sort broken 3 1)) (assert (inside)))",
    ] {
        let source = format!(
            "(defglobal ?*result* = pending)
             (deffunction broken (?a ?b) (/ 1 0))
             (defrule run => {body} (assert (outside)))"
        );
        let mut engine = Engine::with_rules(&source).unwrap();
        let result = engine.run(RunLimit::Count(10)).unwrap();
        assert_eq!(result.halt_reason, HaltReason::ActionError, "{body}");
        assert_eq!(result.rules_fired, 1, "{body}");
        assert_retained_sort_value(&engine);
        assert!(engine.find_facts("inside").unwrap().is_empty(), "{body}");
        assert!(engine.find_facts("outside").unwrap().is_empty(), "{body}");
    }
}

#[test]
fn failed_global_initializer_is_unpublished_and_later_constructs_still_load() {
    let mut engine = Engine::new(EngineConfig::utf8());
    let loaded = engine.load_str(
        "(deffunction broken (?a ?b) (/ 1 0))
         (defglobal ?*first* = 7 ?*result* = (sort broken 3 1) ?*skipped* = 8)
         (defglobal ?*later* = 99)
         (defrule done => (assert (done)))",
    );
    assert!(loaded.is_err());
    assert!(matches!(
        engine.get_global("first"),
        Some(Value::Integer(7))
    ));
    assert!(engine.get_global("result").is_none());
    assert!(engine.get_global("skipped").is_none());
    assert!(matches!(
        engine.get_global("later"),
        Some(Value::Integer(99))
    ));
    assert!(!engine.action_diagnostics().is_empty());
    let result = engine.run(RunLimit::Count(10)).unwrap();
    assert_eq!(result.halt_reason, HaltReason::AgendaEmpty);
    assert_eq!(result.rules_fired, 1);
    assert!(engine.action_diagnostics().is_empty());
}

#[test]
fn failed_match_predicate_rejects_match_and_halts_only_the_current_action_run() {
    let declarations = "
        (deffunction broken (?a ?b) (/ 1 0))
        (defrule blocked (item) (test (sort broken 3 1)) => (assert (unexpected)))
        (defrule follower (item) => (assert (followed)))";
    let mut host = Engine::with_rules(declarations).unwrap();
    host.assert_ordered("item", ()).unwrap();
    assert!(!host.action_diagnostics().is_empty());
    assert_eq!(host.agenda_len(), 1);
    #[cfg(feature = "serde")]
    for &format in ferric_rules_runtime::SerializationFormat::ALL {
        let restored = Engine::deserialize(&host.serialize(format).unwrap(), format).unwrap();
        assert!(!restored.action_diagnostics().is_empty());
    }
    let result = host.run(RunLimit::Count(10)).unwrap();
    assert_eq!(result.halt_reason, HaltReason::AgendaEmpty);
    assert_eq!(result.rules_fired, 1);
    assert!(host.find_facts("unexpected").unwrap().is_empty());

    let mut rhs = Engine::with_rules(&format!(
        "{declarations} (defrule trigger => (assert (item)) (assert (after-assert)))"
    ))
    .unwrap();
    let first = rhs.run(RunLimit::Count(10)).unwrap();
    assert_eq!(first.halt_reason, HaltReason::ActionError);
    assert_eq!(first.rules_fired, 1);
    assert!(!rhs.action_diagnostics().is_empty());
    assert!(rhs.find_facts("after-assert").unwrap().is_empty());
    assert!(rhs.find_facts("followed").unwrap().is_empty());
    let second = rhs.run(RunLimit::Count(10)).unwrap();
    assert_eq!(second.halt_reason, HaltReason::AgendaEmpty);
    assert_eq!(second.rules_fired, 1);
    assert!(rhs.action_diagnostics().is_empty());
    assert!(rhs.find_facts("unexpected").unwrap().is_empty());
}

#[cfg(feature = "serde")]
#[test]
fn deferred_failure_snapshots_keep_value_and_diagnostics_without_sticky_control() {
    for &format in ferric_rules_runtime::SerializationFormat::ALL {
        let pending = Engine::with_rules(HALT_SOURCE).unwrap();
        let mut engine = Engine::deserialize(&pending.serialize(format).unwrap(), format).unwrap();
        run_deferred_failure(&mut engine);
        let diagnostics: Vec<_> = engine
            .action_diagnostics()
            .iter()
            .map(ToString::to_string)
            .collect();
        let mut restored = Engine::deserialize(&engine.serialize(format).unwrap(), format).unwrap();
        assert_retained_sort_value(&restored);
        assert_eq!(
            restored
                .action_diagnostics()
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>(),
            diagnostics
        );
        let resumed = restored.run(RunLimit::Count(10)).unwrap();
        assert_eq!(resumed.halt_reason, HaltReason::AgendaEmpty);
        assert_eq!(resumed.rules_fired, 1);
        assert!(restored.action_diagnostics().is_empty());
        assert_retained_sort_value(&restored);
        restored.reset().unwrap();
        run_deferred_failure(&mut restored);
    }
}
