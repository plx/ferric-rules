//! PR375: a failed match predicate must not poison a later candidate.
//! Expected activation membership and state follow pinned CLIPS 6.30 programs.
//! Reference image: `sha256:b4b99ba2f08102f9c18743c6adaecb5ab82789ddfa8628693cabfeced461a929`.
//! This does not reset sticky halt per match.

use ferric_rules_core::Value;
use ferric_rules_runtime::{Engine, EngineConfig, HaltReason, RunLimit};

const PREFIX: &str = r"
(defglobal ?*bad* = 0 ?*good* = 0 ?*plain* = 0 ?*calls* = 0 ?*pass-calls* = 0)
(deffunction broken (?a ?b) (bind ?*calls* (+ ?*calls* 1)) (/ 1 0))
(deffunction pass () (bind ?*pass-calls* (+ ?*pass-calls* 1)) TRUE)
";
const GOOD: &str = r#"
(defrule good (trigger) (test (eq TRUE TRUE))
  => (bind ?*good* (+ ?*good* 1)) (printout t "GOOD" crlf))
"#;
const PLAIN: &str = r#"
(defrule plain (trigger)
  => (bind ?*plain* (+ ?*plain* 1)) (printout t "PLAIN" crlf))
"#;
const START: &str = r#"
(defrule start (declare (salience 100))
  => (assert (trigger)) (printout t "AFTER-ASSERT" crlf))
"#;

fn setup(good_first: bool, rhs_assert: bool, fields: &str) -> Engine {
    let bad = format!(
        "(defrule bad (trigger) (test (sort broken {fields}))
           => (bind ?*bad* (+ ?*bad* 1)) (printout t \"BAD\" crlf))"
    );
    let rules = if good_first {
        format!("{GOOD}{bad}")
    } else {
        format!("{bad}{GOOD}")
    };
    let source = format!(
        "{PREFIX}{rules}{PLAIN}{}",
        if rhs_assert { START } else { "" }
    );
    let mut engine = Engine::new(EngineConfig::utf8());
    engine.load_str(&source).unwrap();
    engine.reset().unwrap();
    engine
}

fn integer(engine: &Engine, name: &str) -> i64 {
    let Some(Value::Integer(value)) = engine.get_global(name) else {
        panic!("expected INTEGER global {name}")
    };
    *value
}

fn assert_error(engine: &Engine) {
    assert_eq!(engine.action_diagnostics().len(), 1);
    assert!(engine.action_diagnostics()[0]
        .to_string()
        .contains("division by zero"));
}

fn assert_valid_matches_run(engine: &mut Engine, count: i64) {
    let result = engine.run(RunLimit::Count(10)).unwrap();
    assert_eq!(result.halt_reason, HaltReason::AgendaEmpty);
    assert_eq!(result.rules_fired, 2);
    assert_eq!(integer(engine, "good"), count);
    assert_eq!(integer(engine, "plain"), count);
    assert_eq!(integer(engine, "bad"), 0);
    assert_eq!(integer(engine, "calls"), count);
    assert!(engine.action_diagnostics().is_empty());
    assert_eq!(engine.agenda_len(), 0);
}

fn assert_rhs_stops_with_valid_matches_pending(engine: &mut Engine) {
    let first = engine.run(RunLimit::Count(10)).unwrap();
    assert_eq!(first.halt_reason, HaltReason::ActionError);
    assert_eq!(first.rules_fired, 1);
    assert_eq!(integer(engine, "good"), 0);
    assert_eq!(integer(engine, "plain"), 0);
    assert_eq!(integer(engine, "bad"), 0);
    assert_eq!(integer(engine, "calls"), 1);
    assert_eq!(engine.get_output("t").unwrap_or(""), "");
    assert_error(engine);
    assert_eq!(engine.agenda_len(), 2);
}

#[test]
fn host_assertion_isolates_error_for_both_candidate_orders() {
    for good_first in [false, true] {
        let mut engine = setup(good_first, false, "3 1");
        engine.assert_ordered("trigger", Vec::<i64>::new()).unwrap();
        assert_error(&engine);
        assert_eq!(engine.agenda_len(), 2, "good_first={good_first}");
        assert_valid_matches_run(&mut engine, 1);
        assert_eq!(engine.get_output("t"), Some("GOOD\nPLAIN\n"));
        assert_eq!(engine.run(RunLimit::Count(10)).unwrap().rules_fired, 0);
    }
}

#[test]
fn rhs_halt_preserves_later_matches_for_the_next_run_in_both_orders() {
    for good_first in [false, true] {
        let mut engine = setup(good_first, true, "3 1");
        assert_rhs_stops_with_valid_matches_pending(&mut engine);
        assert_valid_matches_run(&mut engine, 1);
        assert_eq!(engine.get_output("t"), Some("GOOD\nPLAIN\n"));
        assert_eq!(engine.run(RunLimit::Count(10)).unwrap().rules_fired, 0);
    }
}

#[test]
fn three_field_recovery_still_admits_its_own_predicate_result() {
    // The skipped later user-comparator call clears EvaluationError while
    // leaving Halt set. The returned MF is therefore a passing test result;
    // using the sticky Halt flag as the predicate verdict would reject it.
    let mut engine = setup(false, false, "3 1 2");
    engine.assert_ordered("trigger", Vec::<i64>::new()).unwrap();
    assert_error(&engine);
    assert_eq!(engine.agenda_len(), 3);
    let result = engine.run(RunLimit::Count(10)).unwrap();
    assert_eq!(result.halt_reason, HaltReason::AgendaEmpty);
    assert_eq!(result.rules_fired, 3);
    for name in ["bad", "good", "plain", "calls"] {
        assert_eq!(integer(&engine, name), 1);
    }
    assert!(engine.action_diagnostics().is_empty());
    // This control pins membership, not unrelated same-salience tie order.
    assert_eq!(engine.get_output("t").unwrap().lines().count(), 3);
    assert_eq!(engine.run(RunLimit::Count(10)).unwrap().rules_fired, 0);
}

#[cfg(feature = "serde")]
#[test]
fn host_error_matches_survive_all_codecs_and_fresh_assertion_after_restore() {
    use ferric_rules_runtime::SerializationFormat;
    for good_first in [false, true] {
        let mut pending = setup(good_first, false, "3 1");
        pending
            .assert_ordered("trigger", Vec::<i64>::new())
            .unwrap();
        assert_eq!(pending.agenda_len(), 2);
        assert_error(&pending);
        for &format in SerializationFormat::ALL {
            let mut restored =
                Engine::deserialize(&pending.serialize(format).unwrap(), format).unwrap();
            assert_eq!(restored.agenda_len(), 2);
            assert_error(&restored);
            assert_valid_matches_run(&mut restored, 1);
            let mut completed =
                Engine::deserialize(&restored.serialize(format).unwrap(), format).unwrap();
            assert_eq!(completed.run(RunLimit::Count(10)).unwrap().rules_fired, 0);
            let trigger = completed.find_facts("trigger").unwrap()[0].0;
            completed.retract(trigger).unwrap();
            completed
                .assert_ordered("trigger", Vec::<i64>::new())
                .unwrap();
            assert_eq!(completed.agenda_len(), 2);
            assert_error(&completed);
            assert_valid_matches_run(&mut completed, 2);
        }
    }
}

#[cfg(feature = "serde")]
#[test]
fn rhs_error_snapshots_preserve_second_run_activations_in_all_codecs() {
    use ferric_rules_runtime::SerializationFormat;
    for good_first in [false, true] {
        let mut pending = setup(good_first, true, "3 1");
        assert_rhs_stops_with_valid_matches_pending(&mut pending);
        for &format in SerializationFormat::ALL {
            let mut restored =
                Engine::deserialize(&pending.serialize(format).unwrap(), format).unwrap();
            assert_eq!(restored.agenda_len(), 2);
            assert_error(&restored);
            assert_valid_matches_run(&mut restored, 1);
            assert_eq!(restored.get_output("t"), Some("GOOD\nPLAIN\n"));
            assert_eq!(restored.run(RunLimit::Count(10)).unwrap().rules_fired, 0);
        }
    }
}
