//! Definition-time salience and activation-driven module focus through public APIs.
use ferric_rules_runtime::{Engine, EngineConfig, HaltReason, RunLimit, Value};

fn run(engine: &mut Engine, count: usize) {
    let result = engine.run(RunLimit::Unlimited).unwrap();
    assert_eq!(result.halt_reason, HaltReason::AgendaEmpty);
    assert_eq!(result.rules_fired, count);
    assert!(engine.action_diagnostics().is_empty());
    #[cfg(debug_assertions)]
    engine.debug_assert_consistency();
}

fn integer_global(engine: &Engine, name: &str) -> i64 {
    let Some(Value::Integer(value)) = engine.get_global(name) else {
        panic!("missing integer global {name}");
    };
    *value
}

#[test]
fn salience_evaluates_once_before_or_expansion_and_never_again_on_reset() {
    let mut engine = Engine::new(EngineConfig::utf8());
    engine
        .load_str(
            r"
      (defglobal ?*calls* = 0)
      (deffunction priority () (printout t defined crlf) (bind ?*calls* (+ ?*calls* 1)) 42)
      (defrule high (declare (salience (priority))) (or (a ?x) (b ?x))
        => (printout t high ?x crlf))
      (deffacts seed (a 1) (b 2))
    ",
        )
        .unwrap();
    assert_eq!(integer_global(&engine, "calls"), 1);
    assert_eq!(engine.get_output("t"), Some("defined\n"));
    assert_eq!(engine.rules().len(), 2);
    assert!(engine.rules().iter().all(|(_, salience)| *salience == 42));
    for _ in 0..3 {
        engine.reset().unwrap();
        assert_eq!(integer_global(&engine, "calls"), 0);
        run(&mut engine, 2);
        assert_eq!(engine.get_output("t"), Some("high2\nhigh1\n"));
    }
}

#[test]
fn salience_cannot_use_globals_or_functions_declared_later_in_the_same_load() {
    for source in [
        "(defrule bad (declare (salience ?*later*)) =>) (defglobal ?*later* = 5)",
        "(defrule bad (declare (salience (later))) =>) (deffunction later () 5)",
    ] {
        let mut engine = Engine::new(EngineConfig::default());
        assert!(engine.load_str(source).is_err(), "accepted {source}");
        assert!(engine.rules().is_empty());
        assert_eq!(engine.agenda_len(), 0);
    }
}

#[test]
fn invalid_salience_replacement_keeps_the_previous_rule_and_queued_match() {
    for expression in [
        "1.5",
        "(+ 10000 1)",
        "(- -10000 1)",
        "(create$ 5)",
        "?missing",
        "(/ 1 0)",
    ] {
        let mut engine = Engine::with_rules(
            "(deffacts seed (item 7)) (defrule choose (item ?x) => (printout t old ?x crlf))",
        )
        .unwrap();
        let before = engine.rete().cardinality();
        let replacement = format!(
            "(defrule choose (declare (salience {expression})) (item ?x) => (printout t new crlf))"
        );
        assert!(
            engine.load_str(&replacement).is_err(),
            "accepted {expression}"
        );
        assert_eq!(engine.rete().cardinality(), before);
        assert_eq!(engine.rules().len(), 1);
        assert_eq!(engine.agenda_len(), 1);
        run(&mut engine, 1);
        assert_eq!(engine.get_output("t"), Some("old7\n"));
    }
}

#[test]
fn salience_effects_and_output_happen_at_definition_even_when_evaluation_fails() {
    let mut engine = Engine::new(EngineConfig::utf8());
    engine
        .load_str(
            r"
      (defrule first (declare (salience (progn
        (printout t defined) (assert (marker 7)) 3))) (never) =>)
    ",
        )
        .unwrap();
    assert_eq!(engine.get_output("t"), Some("defined"));
    assert_eq!(engine.find_facts("marker").unwrap().len(), 1);
    assert!(engine
        .rules()
        .iter()
        .any(|(name, salience)| *name == "first" && *salience == 3));
    assert!(engine
        .load_str(
            r"
      (defrule bad (declare (salience (progn
        (printout t failed) (assert (failure-marker 8)) (/ 1 0)))) =>)
    "
        )
        .is_err());
    assert_eq!(engine.get_output("t"), Some("definedfailed"));
    assert_eq!(engine.find_facts("failure-marker").unwrap().len(), 1);
    assert_eq!(engine.rules().len(), 1);
}

#[test]
fn salience_local_bindings_are_fresh_and_do_not_enter_the_rhs_scope() {
    let mut engine = Engine::with_rules(
        "(defrule good (declare (salience (progn (bind ?x 9) ?x))) => (printout t good crlf))",
    )
    .unwrap();
    assert_eq!(engine.rules(), [("good", 9)]);
    for source in [
        "(defrule bad (declare (salience ?x)) =>)",
        "(defrule bad (declare (salience (bind ?x 5))) => (printout t ?x crlf))",
        "(defrule bad (declare (salience (return 5))) =>)",
        "(defrule bad (declare (salience (break))) =>)",
    ] {
        assert!(engine.load_str(source).is_err(), "accepted {source}");
    }
    run(&mut engine, 1);
    assert_eq!(engine.get_output("t"), Some("good\n"));
}

#[test]
fn salience_uses_the_rules_definition_module_for_globals_and_callables() {
    let mut engine = Engine::with_rules(
        "(defmodule A) (defglobal ?*rank* = 7) (deffunction priority () ?*rank*)
         (defrule fixed (declare (salience 5)) => (printout t fixed crlf))
         (defmodule B) (defglobal ?*rank* = -7) (deffunction priority () ?*rank*)
         (defrule A::dynamic (declare (salience (priority))) => (printout t dynamic crlf))",
    )
    .unwrap();
    engine.set_focus("A").unwrap();
    run(&mut engine, 2);
    assert_eq!(engine.get_output("t"), Some("dynamic\nfixed\n"));
}

const ONE_MODULE: &str = "
(defmodule MAIN (export ?ALL))
(deftemplate MAIN::go (slot value))
(defmodule WATCH (import MAIN ?ALL))
(defrule WATCH::alarm (declare (auto-focus TRUE)) (go (value ?v)) => (printout t watch ?v crlf))
";

#[test]
fn host_assertion_cancellation_keeps_focus_and_suppresses_only_duplicate_top_pushes() {
    let mut engine = Engine::with_rules(ONE_MODULE).unwrap();
    assert_eq!(engine.get_focus_stack(), ["MAIN"]);
    let first = engine.assert_template("MAIN::go", &["value"], [1]).unwrap();
    let second = engine.assert_template("MAIN::go", &["value"], [2]).unwrap();
    assert_eq!(engine.get_focus_stack(), ["MAIN", "WATCH"]);
    engine.retract(first).unwrap();
    engine.retract(second).unwrap();
    assert_eq!(engine.agenda_len(), 0);
    assert_eq!(engine.get_focus_stack(), ["MAIN", "WATCH"]);
    engine.push_focus("MAIN").unwrap();
    engine.assert_template("MAIN::go", &["value"], [3]).unwrap();
    assert_eq!(engine.get_focus_stack(), ["MAIN", "WATCH", "MAIN", "WATCH"]);
    run(&mut engine, 1);
    assert_eq!(engine.get_output("t"), Some("watch3\n"));
}

#[test]
fn negative_reset_and_last_blocker_retraction_push_focus_even_after_cancellation() {
    let mut engine = Engine::with_rules(
        "(defmodule MAIN (export ?ALL)) (deftemplate MAIN::block (slot id))
         (deffacts seed (block (id 1)) (block (id 2)))
         (defmodule WATCH (import MAIN ?ALL))
         (defrule absent (declare (auto-focus TRUE)) (not (block)) => (printout t absent crlf))",
    )
    .unwrap();
    // The reset root activation focused WATCH before seed facts canceled it.
    assert_eq!(engine.get_focus_stack(), ["MAIN", "WATCH"]);
    assert_eq!(engine.agenda_len(), 0);
    run(&mut engine, 0);
    let blockers: Vec<_> = engine.facts().unwrap().map(|(id, _)| id).collect();
    assert_eq!(blockers.len(), 2);
    engine.retract(blockers[0]).unwrap();
    assert_eq!(engine.get_focus(), None);
    engine.retract(blockers[1]).unwrap();
    assert_eq!(engine.get_focus(), Some("WATCH"));
    run(&mut engine, 1);
    assert_eq!(engine.get_output("t"), Some("absent\n"));
    engine.reset().unwrap();
    assert_eq!(engine.get_focus_stack(), ["MAIN", "WATCH"]);
    assert_eq!(engine.agenda_len(), 0);
}

#[test]
fn online_rule_installation_focuses_existing_matches_and_false_replacement_unregisters_it() {
    let mut engine = Engine::with_rules(
        "(defmodule MAIN (export ?ALL)) (deftemplate MAIN::go (slot value))
         (defmodule WATCH (import MAIN ?ALL))",
    )
    .unwrap();
    engine.assert_template("MAIN::go", &["value"], [1]).unwrap();
    engine.load_str(
        "(defrule WATCH::alarm (declare (auto-focus TRUE)) (go (value ?v)) => (printout t old ?v crlf))",
    ).unwrap();
    assert_eq!(engine.get_focus_stack(), ["MAIN", "WATCH"]);
    run(&mut engine, 1);
    engine.load_str(
        "(defrule WATCH::alarm (declare (auto-focus FALSE)) (go (value ?v)) => (printout t new ?v crlf))",
    ).unwrap();
    assert_eq!(engine.get_focus(), None);
    engine.set_focus("MAIN").unwrap();
    engine.assert_template("MAIN::go", &["value"], [2]).unwrap();
    assert_eq!(engine.get_focus_stack(), ["MAIN"]);
    assert_eq!(engine.agenda_len(), 2);
    run(&mut engine, 0);
    engine.set_focus("WATCH").unwrap();
    run(&mut engine, 2);
    assert_eq!(engine.get_output("t"), Some("old1\nnew2\nnew1\n"));
}

const TWO_MODULES: &str = "
(defmodule MAIN (export ?ALL))
(deftemplate MAIN::go (slot value))
(defglobal ?*calls* = 0)
(deffunction priority () (bind ?*calls* (+ ?*calls* 1)) 20)
(defmodule A (import MAIN ?ALL))
(defrule a (declare (auto-focus TRUE) (salience (priority))) (go (value ?v)) => (printout t A ?v crlf))
(defmodule B (import MAIN ?ALL))
(defrule b (declare (auto-focus TRUE) (salience (priority))) (go (value ?v)) => (printout t B ?v crlf))
";

#[test]
fn shared_terminal_events_preserve_module_order_when_modules_are_deeper_in_the_stack() {
    let mut engine = Engine::with_rules(TWO_MODULES).unwrap();
    engine.assert_template("MAIN::go", &["value"], [1]).unwrap();
    assert_eq!(engine.get_focus_stack(), ["MAIN", "B", "A"]);
    engine.assert_template("MAIN::go", &["value"], [2]).unwrap();
    assert_eq!(engine.get_focus_stack(), ["MAIN", "B", "A", "B", "A"]);
    run(&mut engine, 4);
    assert_eq!(engine.get_output("t"), Some("A2\nA1\nB2\nB1\n"));
}

#[cfg(feature = "serde")]
#[test]
fn snapshots_preserve_pending_focus_without_replaying_salience_or_push_notifications() {
    use ferric_rules_runtime::SerializationFormat;
    let mut original = Engine::with_rules(TWO_MODULES).unwrap();
    original
        .assert_template("MAIN::go", &["value"], [1])
        .unwrap();
    original
        .assert_template("MAIN::go", &["value"], [2])
        .unwrap();
    for &format in SerializationFormat::ALL {
        let mut engine = Engine::deserialize(&original.serialize(format).unwrap(), format).unwrap();
        for _ in 0..3 {
            assert_eq!(engine.get_focus_stack(), ["MAIN", "B", "A", "B", "A"]);
            assert_eq!(integer_global(&engine, "calls"), 0);
            assert_eq!(engine.agenda_len(), 4);
            engine = Engine::deserialize(&engine.serialize(format).unwrap(), format).unwrap();
        }
        let partial = engine.run(RunLimit::Count(1)).unwrap();
        assert_eq!(partial.rules_fired, 1);
        assert_eq!(engine.get_output("t"), Some("A2\n"));
        let stack: Vec<_> = engine
            .get_focus_stack()
            .into_iter()
            .map(str::to_owned)
            .collect();
        engine = Engine::deserialize(&engine.serialize(format).unwrap(), format).unwrap();
        assert_eq!(engine.get_focus_stack(), stack);
        run(&mut engine, 3);
        assert_eq!(engine.get_output("t"), Some("A2\nA1\nB2\nB1\n"));
        engine.assert_template("MAIN::go", &["value"], [3]).unwrap();
        assert_eq!(engine.get_focus_stack(), ["B", "A"]);
        run(&mut engine, 2);
        assert_eq!(engine.get_output("t"), Some("A2\nA1\nB2\nB1\nA3\nB3\n"));
        assert_eq!(integer_global(&engine, "calls"), 0);
    }
}

#[cfg(feature = "serde")]
#[test]
fn snapshots_keep_historical_focus_after_every_activation_has_been_canceled() {
    use ferric_rules_runtime::SerializationFormat;
    let mut original = Engine::with_rules(ONE_MODULE).unwrap();
    let fact = original
        .assert_template("MAIN::go", &["value"], [1])
        .unwrap();
    original.retract(fact).unwrap();
    for &format in SerializationFormat::ALL {
        let mut engine = Engine::deserialize(&original.serialize(format).unwrap(), format).unwrap();
        assert_eq!(engine.get_focus_stack(), ["MAIN", "WATCH"]);
        assert_eq!(engine.agenda_len(), 0);
        run(&mut engine, 0);
        engine.assert_template("MAIN::go", &["value"], [2]).unwrap();
        assert_eq!(engine.get_focus(), Some("WATCH"));
        run(&mut engine, 1);
        assert_eq!(engine.get_output("t"), Some("watch2\n"));
    }
}

const NCC_TEMPLATES: &str = "
(defmodule MAIN (export ?ALL))
(deftemplate MAIN::item (slot value))
(deftemplate MAIN::blocker (slot marker))
(deftemplate MAIN::other)
";

fn ncc_patterns(deferred: bool, predicate_passes: bool) -> String {
    let support = if deferred {
        format!("(test (eq 1 {}))", if predicate_passes { 1 } else { 2 })
    } else {
        "(other)".to_owned()
    };
    format!("(item (value ?x)) (not (and (blocker) {support}))")
}

fn late_ncc_engine(shared: bool, deferred: bool, parent_first: bool) -> Engine {
    let patterns = ncc_patterns(deferred, true);
    let old = if shared {
        format!("(defrule MAIN::old {patterns} =>)")
    } else {
        String::new()
    };
    let mut engine = Engine::with_rules(&format!(
        "{NCC_TEMPLATES}{old}(defmodule WATCH (import MAIN ?ALL))"
    ))
    .unwrap();
    if parent_first {
        engine
            .assert_template("MAIN::item", &["value"], [1])
            .unwrap();
    }
    engine
        .assert_template("MAIN::blocker", &["marker"], [77])
        .unwrap();
    engine.assert_template("MAIN::other", &[], ()).unwrap();
    if !parent_first {
        engine
            .assert_template("MAIN::item", &["value"], [1])
            .unwrap();
    }
    assert_eq!(engine.agenda_len(), 0);
    assert_eq!(engine.get_focus_stack(), ["MAIN"]);
    engine
        .load_str(&format!(
            "(defrule WATCH::late (declare (auto-focus TRUE)) {patterns} => (printout t late ?x crlf))"
        ))
        .unwrap();
    engine
}

fn check_blocked_late_ncc(engine: &Engine) {
    assert_eq!(engine.agenda_len(), 0);
    assert_eq!(engine.get_focus_stack(), ["MAIN"]);
}

// (shared NCC, deferred support predicate, parent asserted before supports).
// Fresh parent-first installation and shared deferred support have separate,
// exact compatibility characterizations in the 398 late-NCC corpus fixtures.
const CONFORMING_LATE_NCC_CASES: [(bool, bool, bool); 4] = [
    (false, false, false),
    (false, true, false),
    (true, false, false),
    (true, false, true),
];

fn release_late_ncc(engine: &mut Engine, shared: bool) {
    let blocker = engine
        .facts()
        .unwrap()
        .map(|(id, _)| id)
        .find(|&id| {
            matches!(
                engine.get_fact_slot_by_name(id, "marker"),
                Ok(Value::Integer(77))
            )
        })
        .unwrap();
    engine.retract(blocker).unwrap();
    assert_eq!(engine.get_focus_stack(), ["MAIN", "WATCH"]);
    run(engine, 1 + usize::from(shared));
    assert_eq!(engine.get_output("t"), Some("late1\n"));
}

#[test]
fn late_blocked_ncc_installation_and_unblocking_match_reference_focus() {
    // Pinned CLIPS 6.30 oracle, with both plain and (test (eq 1 1)) NCC support:
    // install WATCH::late only after all item/blocker/other facts exist. A fresh
    // supports-first network leaves MAIN alone. An already populated identical
    // plain NCC does not replay canceled historical activations in either order.
    // Retraction of the blocker then focuses WATCH and prints late1 in all cases.
    for (shared, deferred, parent_first) in CONFORMING_LATE_NCC_CASES {
        let mut engine = late_ncc_engine(shared, deferred, parent_first);
        check_blocked_late_ncc(&engine);
        release_late_ncc(&mut engine, shared);
    }
}

#[test]
fn deferred_ncc_assertions_focus_only_genuine_matches_and_keep_prior_focus_after_cancellation() {
    // Actual CLIPS 6.30: blocker-first with a passing test never focuses WATCH;
    // item-first focuses WATCH before a later blocker cancels the activation.
    // A failing test leaves the negative condition satisfied and fires once.
    for predicate_passes in [false, true] {
        for parent_first in [false, true] {
            let patterns = ncc_patterns(true, predicate_passes);
            let mut engine = Engine::with_rules(&format!(
                "{NCC_TEMPLATES}(defmodule WATCH (import MAIN ?ALL))
                 (defrule WATCH::late (declare (auto-focus TRUE)) {patterns}
                   => (printout t fired ?x crlf))"
            ))
            .unwrap();
            if parent_first {
                engine
                    .assert_template("MAIN::item", &["value"], [1])
                    .unwrap();
                assert_eq!(engine.get_focus_stack(), ["MAIN", "WATCH"]);
            }
            engine
                .assert_template("MAIN::blocker", &["marker"], [77])
                .unwrap();
            if !parent_first {
                assert_eq!(engine.get_focus_stack(), ["MAIN"]);
                engine
                    .assert_template("MAIN::item", &["value"], [1])
                    .unwrap();
            }
            let expected = if parent_first || !predicate_passes {
                vec!["MAIN", "WATCH"]
            } else {
                vec!["MAIN"]
            };
            assert_eq!(engine.get_focus_stack(), expected);
            run(&mut engine, usize::from(!predicate_passes));
            assert_eq!(
                engine.get_output("t").unwrap_or(""),
                if predicate_passes { "" } else { "fired1\n" }
            );
        }
    }
}

#[cfg(feature = "serde")]
#[test]
fn late_ncc_focus_history_and_blocked_state_survive_every_snapshot_format() {
    use ferric_rules_runtime::SerializationFormat;
    for (shared, deferred, parent_first) in CONFORMING_LATE_NCC_CASES {
        let original = late_ncc_engine(shared, deferred, parent_first);
        for &format in SerializationFormat::ALL {
            let bytes = original.serialize(format).unwrap();
            let mut engine = Engine::deserialize(&bytes, format).unwrap();
            check_blocked_late_ncc(&engine);
            release_late_ncc(&mut engine, shared);
        }
    }
}
