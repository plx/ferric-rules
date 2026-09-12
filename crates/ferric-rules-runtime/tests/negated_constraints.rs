//! #300: runtime expressions in negative field constraints.
//!
//! Semantic expectations are pinned to CLIPS 6.30 image
//! `sha256:b4b99ba2f08102f9c18743c6adaecb5ab82789ddfa8628693cabfeced461a929`.
//! Reference programs cover nonlinear constraints, lexical scope, callback
//! timing, template slots, module visibility, and conflict replacement order.
//! Staged reference commands become host assertions/retractions here. We assert
//! callback effects and activation membership, not CLIPS diagnostic spelling or
//! unrelated same-salience agenda ordering.

use ferric_rules_core::Value;
use ferric_rules_runtime::{Engine, EngineConfig, HaltReason, RunLimit};

fn setup(source: &str) -> Engine {
    let mut engine = Engine::new(EngineConfig::utf8());
    engine.load_str(source).unwrap();
    engine.reset().unwrap();
    engine
}

fn run(engine: &mut Engine, expected: usize) {
    let result = engine.run(RunLimit::Count(100)).unwrap();
    assert_eq!(result.halt_reason, HaltReason::AgendaEmpty);
    assert_eq!(result.rules_fired, expected);
    assert!(engine.action_diagnostics().is_empty());
    #[cfg(debug_assertions)]
    engine.debug_assert_consistency();
}

fn integer(engine: &Engine, name: &str) -> i64 {
    let Some(Value::Integer(value)) = engine.get_global(name) else {
        panic!("expected INTEGER global {name}")
    };
    *value
}

fn output(engine: &Engine) -> &str {
    engine.get_output("t").unwrap().unwrap_or("")
}

fn output_lines(engine: &Engine) -> Vec<&str> {
    let mut lines: Vec<_> = output(engine).lines().collect();
    lines.sort_unstable();
    lines
}

fn error_recorded(engine: &Engine) {
    assert!(!engine.action_diagnostics().is_empty());
}

const GATE_SETTER: &str = r"
(defrule change-gate (declare (salience 1000)) (set-gate ?value)
  => (bind ?*gate* ?value))
";

// There is no host global setter. This high-salience action changes only the
// global, then yields before any pending negative-rule action can execute.
fn set_gate(engine: &mut Engine, value: bool) {
    let fact = engine
        .assert_ordered_symbol("set-gate", if value { "TRUE" } else { "FALSE" })
        .unwrap();
    assert_eq!(engine.run(RunLimit::Count(1)).unwrap().rules_fired, 1);
    engine.retract(fact).unwrap();
}

const ORIGINAL_CASES: &[(&str, &str, &str)] = &[
    (
        "nonlinear-predicate",
        r#"(deffacts input (anchor 2) (anchor 5) (data 1) (data 3))
(defrule no-square-greater (anchor ?min) (not (data ?x&:(> (* ?x ?x) (* ?min ?min)))) => (printout t "safe:" ?min crlf))
"#,
        "safe:5\n",
    ),
    (
        "nonlinear-self-return",
        r#"(deffacts input (pair 2))
(defrule no-self-square (not (pair ?x&=(* ?x ?x))) => (printout t "safe-return" crlf))
"#,
        "safe-return\n",
    ),
    (
        "nonlinear-outer-return",
        r#"(deffacts input (target 2) (target 3) (pair 4))
(defrule no-square (target ?x) (not (pair =(* ?x ?x))) => (printout t "safe:" ?x crlf))
"#,
        "safe:3\n",
    ),
    (
        "explicit-ncc-equivalent",
        r#"(deffacts input (anchor 2) (anchor 5) (data 1) (data 3))
(defrule no-square-greater (anchor ?min) (not (and (data ?x) (test (> (* ?x ?x) (* ?min ?min))))) => (printout t "safe:" ?min crlf))
"#,
        "safe:5\n",
    ),
    (
        "simple-linear-control",
        r#"(deffacts input (anchor 2) (anchor 5) (data 1) (data 3))
(defrule no-greater (anchor ?min) (not (data ?x&:(> ?x ?min))) => (printout t "safe:" ?min crlf))
"#,
        "safe:5\n",
    ),
];

#[test]
fn original_nonlinear_predicates_and_returns_match_normally_and_when_installed_late() {
    for &(name, source, expected) in ORIGINAL_CASES {
        for late in [false, true] {
            let mut engine = if late {
                let split = source.find("(defrule").unwrap();
                let mut engine = setup(&source[..split]);
                engine.load_str(&source[split..]).unwrap();
                engine
            } else {
                setup(source)
            };
            assert!(
                engine.action_diagnostics().is_empty(),
                "{name}, late={late}"
            );
            run(&mut engine, 1);
            assert_eq!(output(&engine), expected, "{name}, late={late}");
            run(&mut engine, 0);
        }
    }
}

const ADMITTED_SCOPE_CASES: &[(&str, &str, i64)] = &[
    (
        "same-field-predicate",
        r"(defglobal ?*fires* = 0)
(deffacts input (data 2))
(defrule probe
 (not (data ?x&:(= (* ?x ?x) ?x)))
 => (bind ?*fires* (+ ?*fires* 1)))
",
        1,
    ),
    (
        "same-field-return",
        r"(defglobal ?*fires* = 0)
(deffacts input (data 2))
(defrule probe
 (not (data ?x&=(* ?x ?x)))
 => (bind ?*fires* (+ ?*fires* 1)))
",
        1,
    ),
    (
        "prior-field-predicate",
        r"(defglobal ?*fires* = 0)
(deffacts input (data 2 4))
(defrule probe
 (not (data ?x ?y&:(= ?y (* ?x ?x))))
 => (bind ?*fires* (+ ?*fires* 1)))
",
        0,
    ),
    (
        "prior-field-return",
        r"(defglobal ?*fires* = 0)
(deffacts input (data 2 4))
(defrule probe
 (not (data ?x =(* ?x ?x)))
 => (bind ?*fires* (+ ?*fires* 1)))
",
        0,
    ),
    (
        "outer-predicate",
        r#"(defglobal ?*fires* = 0)
(deffacts input (outer 2) (data 3))
(defrule probe
 (outer ?x) (not (data ?y&:(= ?y (* ?x ?x))))
 => (bind ?*fires* (+ ?*fires* 1)) (printout t "outer:" ?x crlf))
"#,
        1,
    ),
    (
        "outer-return",
        r#"(defglobal ?*fires* = 0)
(deffacts input (outer 2) (data 3))
(defrule probe
 (outer ?x) (not (data =(* ?x ?x)))
 => (bind ?*fires* (+ ?*fires* 1)) (printout t "outer:" ?x crlf))
"#,
        1,
    ),
    (
        "fresh-positive-reuse",
        r#"(defglobal ?*fires* = 0)
(deffacts input (fresh 7))
(defrule probe
 (not (data ?x&:(> ?x 0))) (fresh ?x) (test (= ?x 7))
 => (bind ?*fires* (+ ?*fires* 1)) (printout t "fresh:" ?x crlf))
"#,
        1,
    ),
    (
        "fresh-positive-same-field-reuse",
        r#"(defglobal ?*fires* = 0)
(deffacts input (b 7))
(defrule probe
 (not (a ?x)) (b ?x&:(> ?x 0))
 => (bind ?*fires* (+ ?*fires* 1)) (printout t "fresh:" ?x crlf))
"#,
        1,
    ),
    (
        "sibling-negative-fresh-reuse",
        r"(defglobal ?*fires* = 0)
(deffacts input (b -2))
(defrule probe
 (not (a ?x)) (not (b ?x&:(> ?x 0)))
 => (bind ?*fires* (+ ?*fires* 1)))
",
        1,
    ),
    (
        "ncc-internal-prior-pattern",
        r"(defglobal ?*fires* = 0)
(deffacts input (a 2) (b 3))
(defrule probe
 (not (and (a ?x) (b ?y&:(= ?y (* ?x ?x)))))
 => (bind ?*fires* (+ ?*fires* 1)))
",
        1,
    ),
    (
        "ncc-fresh-positive-reuse",
        r#"(defglobal ?*fires* = 0)
(deffacts input (fresh 7))
(defrule probe
 (not (and (a ?x) (b ?x))) (fresh ?x) (test (= ?x 7))
 => (bind ?*fires* (+ ?*fires* 1)) (printout t "fresh:" ?x crlf))
"#,
        1,
    ),
    (
        "exists-internal-prior-pattern",
        r"(defglobal ?*fires* = 0)
(deffacts input (a 2) (b 4))
(defrule probe
 (exists (and (a ?x) (b ?y&:(= ?y (* ?x ?x)))))
 => (bind ?*fires* (+ ?*fires* 1)))
",
        1,
    ),
    (
        "nested-not-outer-local-valid",
        r"(defglobal ?*fires* = 0)
(deffacts input (a 2) (b 4))
(defrule probe
 (not (and (a ?x) (not (b ?y&:(= ?y (* ?x ?x))))))
 => (bind ?*fires* (+ ?*fires* 1)))
",
        1,
    ),
    (
        "or-both-branches-bind-outer",
        r#"(defglobal ?*fires* = 0)
(deffacts input (outer 2))
(defrule probe
 (or (and (outer ?x) (not (data ?y&:(= ?y (* ?x ?x))))) (and (other ?x) (not (data ?z&:(= ?z (* ?x ?x)))))) (test (> ?x 0))
 => (bind ?*fires* (+ ?*fires* 1)) (printout t "outer:" ?x crlf))
"#,
        1,
    ),
    (
        "defined-global-callee-empty-wm",
        r"(defglobal ?*fires* = 0)
(defglobal ?*gate* = 2)
(deffunction allowed (?n) (> ?n ?*gate*))
(defrule probe
 (not (data ?y&:(allowed ?y)))
 => (bind ?*fires* (+ ?*fires* 1)))
",
        1,
    ),
];

#[test]
fn local_outer_nested_and_branch_scopes_match_reference_admission_and_membership() {
    for &(name, source, expected) in ADMITTED_SCOPE_CASES {
        for late in [false, true] {
            let split = source.find("(defrule").unwrap();
            let mut engine = if late {
                let mut engine = setup(&source[..split]);
                engine
                    .load_str(&source[split..])
                    .unwrap_or_else(|error| panic!("late scope case {name}: {error:?}"));
                engine
            } else {
                let mut engine = Engine::new(EngineConfig::utf8());
                engine
                    .load_str(source)
                    .unwrap_or_else(|error| panic!("scope case {name}: {error:?}"));
                engine.reset().unwrap();
                engine
            };
            assert_eq!(
                engine.agenda_len(),
                usize::try_from(expected).unwrap(),
                "{name}, late={late}"
            );
            assert!(
                engine.action_diagnostics().is_empty(),
                "{name}, late={late}"
            );
            run(&mut engine, usize::try_from(expected).unwrap());
            assert_eq!(integer(&engine, "fires"), expected, "{name}, late={late}");
            run(&mut engine, 0);
        }
    }
}

const REJECTED_SCOPE_CASES: &[(&str, &str)] = &[
    (
        "later-field-reference",
        r"(defrule probe
 (not (data ?x&:(= ?x ?y) ?y))
 => (bind ?*fires* (+ ?*fires* 1)))
",
    ),
    (
        "same-field-binding-after-predicate",
        r"(defrule probe
 (not (data :(> ?x 0)&?x))
 => (bind ?*fires* (+ ?*fires* 1)))
",
    ),
    (
        "negative-local-to-later-test",
        r"(defrule probe
 (not (data ?x&:(> ?x 0))) (test (> ?x 0))
 => (bind ?*fires* (+ ?*fires* 1)))
",
    ),
    (
        "negative-local-to-rhs",
        r"(defrule probe
 (not (data ?x&:(> ?x 0)))
 => (bind ?*fires* (+ ?*fires* 1)) (printout t ?x crlf))
",
    ),
    (
        "negative-local-to-later-constraint",
        r"(defrule probe
 (not (a ?x)) (b ?y&:(= ?y ?x))
 => (bind ?*fires* (+ ?*fires* 1)))
",
    ),
    (
        "sibling-negative-leak",
        r"(defrule probe
 (not (a ?x)) (not (b ?y&:(= ?x ?y)))
 => (bind ?*fires* (+ ?*fires* 1)))
",
    ),
    (
        "ncc-local-to-later-test",
        r"(defrule probe
 (not (and (a ?x) (b ?x))) (test (> ?x 0))
 => (bind ?*fires* (+ ?*fires* 1)))
",
    ),
    (
        "exists-local-to-later-test",
        r"(defrule probe
 (exists (a ?x)) (test (> ?x 0))
 => (bind ?*fires* (+ ?*fires* 1)))
",
    ),
    (
        "double-not-local-to-later-test",
        r"(defrule probe
 (not (not (a ?x))) (test (> ?x 0))
 => (bind ?*fires* (+ ?*fires* 1)))
",
    ),
    (
        "nested-not-inner-local-escape",
        r"(defrule probe
 (not (and (a ?outer) (not (b ?local)) (c ?z&:(= ?z ?local))))
 => (bind ?*fires* (+ ?*fires* 1)))
",
    ),
    (
        "or-negative-local-missing-in-branch",
        r"(defrule probe
 (or (not (a ?x)) (fresh ?x)) (test (> ?x 0))
 => (bind ?*fires* (+ ?*fires* 1)))
",
    ),
    (
        "undefined-local-empty-wm",
        r"(defrule probe
 (not (data ?y&:(= ?y (* ?missing ?missing))))
 => (bind ?*fires* (+ ?*fires* 1)))
",
    ),
    (
        "undefined-global-empty-wm",
        r"(defrule probe
 (not (data ?y&:(= ?y ?*missing*)))
 => (bind ?*fires* (+ ?*fires* 1)))
",
    ),
    (
        "undefined-callee-empty-wm",
        r"(defrule probe
 (not (data ?y&:(missing ?y)))
 => (bind ?*fires* (+ ?*fires* 1)))
",
    ),
    (
        "undefined-global-return-empty-wm",
        r"(defrule probe
 (not (data =(* ?*missing* 2)))
 => (bind ?*fires* (+ ?*fires* 1)))
",
    ),
    (
        "undefined-callee-return-empty-wm",
        r"(defrule probe
 (not (data =(missing 2)))
 => (bind ?*fires* (+ ?*fires* 1)))
",
    ),
];

#[test]
fn invalid_negative_scope_is_rejected_with_empty_memory_and_preserves_existing_rule() {
    for &(name, source) in REJECTED_SCOPE_CASES {
        let mut engine = setup(
            r"
            (defglobal ?*fires* = 0)
            (defrule probe (ready) => (bind ?*fires* (+ ?*fires* 1)))
        ",
        );
        assert_eq!(engine.agenda_len(), 0);
        assert!(
            engine.load_str(source).is_err(),
            "unexpected admission: {name}"
        );
        assert_eq!(engine.rules().len(), 1, "replacement changed rules: {name}");
        assert_eq!(integer(&engine, "fires"), 0);
        engine.assert_ordered("ready", Vec::<i64>::new()).unwrap();
        run(&mut engine, 1);
        assert_eq!(integer(&engine, "fires"), 1, "old rule lost: {name}");
    }
}

const COUNTED_JOIN: &str = r#"
(defglobal ?*hits* = 0 ?*calls* = 0)
(deffunction qualifies (?x ?min)
  (bind ?*calls* (+ ?*calls* 1)) (> (* ?x ?x) (* ?min ?min)))
(defrule safe (anchor ?min) (not (data ?x&:(qualifies ?x ?min)))
  => (bind ?*hits* (+ ?*hits* 1)) (printout t "safe:" ?min crlf))
"#;

#[test]
fn blockers_retract_reassert_without_refiring_for_nonblocker_churn() {
    let mut engine = setup(COUNTED_JOIN);
    engine.assert_ordered("anchor", [2]).unwrap();
    run(&mut engine, 1);
    let first = engine.assert_ordered("data", [3]).unwrap();
    let second = engine.assert_ordered("data", [4]).unwrap();
    assert_eq!(integer(&engine, "calls"), 1);
    run(&mut engine, 0);
    engine.retract(first).unwrap();
    assert_eq!(integer(&engine, "calls"), 2);
    run(&mut engine, 0);
    engine.retract(second).unwrap();
    run(&mut engine, 1);
    let small = engine.assert_ordered("data", [1]).unwrap();
    run(&mut engine, 0);
    engine.retract(small).unwrap();
    run(&mut engine, 0);
    let blocker = engine.assert_ordered("data", [3]).unwrap();
    engine.retract(blocker).unwrap();
    run(&mut engine, 1);
    assert_eq!(integer(&engine, "hits"), 3);
    assert_eq!(output(&engine), "safe:2\nsafe:2\nsafe:2\n");
}

#[test]
fn each_outer_owner_keeps_an_independent_selected_blocker() {
    let mut engine = setup(COUNTED_JOIN);
    engine.assert_ordered("anchor", [2]).unwrap();
    engine.assert_ordered("anchor", [5]).unwrap();
    let first = engine.assert_ordered("data", [3]).unwrap();
    let second = engine.assert_ordered("data", [6]).unwrap();
    assert_eq!(integer(&engine, "calls"), 3);
    run(&mut engine, 0);
    engine.retract(first).unwrap();
    assert_eq!(integer(&engine, "calls"), 4);
    run(&mut engine, 0);
    engine.retract(second).unwrap();
    run(&mut engine, 2);
    assert_eq!(output_lines(&engine), ["safe:2", "safe:5"]);
}

fn gate_engine(local: bool, late: bool) -> Engine {
    let function = if local {
        "(deffunction qualifies (?x) (bind ?*calls* (+ ?*calls* 1)) ?*gate*)"
    } else {
        "(deffunction qualifies (?x ?a) (bind ?*calls* (+ ?*calls* 1)) ?*gate*)"
    };
    let mut engine = setup(&format!(
        "(defglobal ?*gate* = TRUE ?*calls* = 0 ?*hits* = 0) {function} {GATE_SETTER}"
    ));
    if !late {
        install_gate_rule(&mut engine, local);
    }
    engine
}

fn install_gate_rule(engine: &mut Engine, local: bool) {
    let call = if local {
        "(qualifies ?x)"
    } else {
        "(qualifies ?x ?a)"
    };
    engine
        .load_str(&format!(
            "(defrule safe (anchor ?a) (not (data ?x&:{call}))
            => (bind ?*hits* (+ ?*hits* 1)) (printout t \"safe:\" ?a crlf))"
        ))
        .unwrap();
}

#[test]
fn local_membership_is_captured_before_outer_and_shared_by_later_owners() {
    let mut engine = gate_engine(true, false);
    let data = engine.assert_ordered("data", [3]).unwrap();
    assert_eq!(integer(&engine, "calls"), 1);
    set_gate(&mut engine, false);
    engine.assert_ordered("anchor", [2]).unwrap();
    engine.assert_ordered("anchor", [4]).unwrap();
    assert_eq!(integer(&engine, "calls"), 1);
    run(&mut engine, 0);
    engine.retract(data).unwrap();
    engine.assert_ordered("data", [3]).unwrap();
    assert_eq!(integer(&engine, "calls"), 2);
    run(&mut engine, 2);
    assert_eq!(output_lines(&engine), ["safe:2", "safe:4"]);
}

#[test]
fn late_local_installation_evaluates_facts_even_without_outer_tokens() {
    let mut engine = gate_engine(true, true);
    engine.assert_ordered("data", [3]).unwrap();
    set_gate(&mut engine, false);
    assert_eq!(integer(&engine, "calls"), 0);
    install_gate_rule(&mut engine, true);
    assert_eq!(integer(&engine, "calls"), 1);
    set_gate(&mut engine, true);
    engine.assert_ordered("anchor", [2]).unwrap();
    engine.assert_ordered("anchor", [4]).unwrap();
    run(&mut engine, 2);
    assert_eq!(integer(&engine, "calls"), 1);
    assert_eq!(output_lines(&engine), ["safe:2", "safe:4"]);
}

#[test]
fn join_membership_waits_for_outer_and_does_not_react_to_global_changes_alone() {
    let mut engine = gate_engine(false, false);
    let data = engine.assert_ordered("data", [3]).unwrap();
    assert_eq!(integer(&engine, "calls"), 0);
    set_gate(&mut engine, false);
    engine.assert_ordered("anchor", [2]).unwrap();
    assert_eq!(integer(&engine, "calls"), 1);
    // Original match-time-global control: changing the global after matching
    // does not retract the already-admitted activation.
    set_gate(&mut engine, true);
    run(&mut engine, 1);
    engine.retract(data).unwrap();
    let data = engine.assert_ordered("data", [3]).unwrap();
    assert_eq!(integer(&engine, "calls"), 2);
    set_gate(&mut engine, false);
    run(&mut engine, 0);
    engine.retract(data).unwrap();
    run(&mut engine, 1);
    assert_eq!(integer(&engine, "hits"), 2);
}

fn rejected_predecessor_engine() -> Engine {
    let mut engine = setup(&format!(
        r#"
        (defglobal ?*gate* = FALSE ?*calls* = 0)
        (deffunction qualifies (?x ?a)
          (bind ?*calls* (+ ?*calls* 1)) (or (= ?x 9) ?*gate*))
        (defrule safe (anchor ?a) (not (data ?x&:(qualifies ?x ?a)))
          => (printout t "safe" crlf))
        {GATE_SETTER}
    "#
    ));
    engine.assert_ordered("anchor", [2]).unwrap();
    engine.assert_ordered("data", [1]).unwrap();
    engine.assert_ordered("data", [9]).unwrap();
    assert_eq!(integer(&engine, "calls"), 2);
    set_gate(&mut engine, true);
    engine
}

fn integer_fact(engine: &Engine, relation: &str, value: i64) -> ferric_rules_runtime::FactHandle {
    engine
        .find_facts(relation)
        .unwrap()
        .into_iter()
        .find(|(_, fact)| {
            matches!(fact, ferric_rules_core::Fact::Ordered(fact)
            if matches!(fact.fields.first(), Some(Value::Integer(field)) if *field == value))
        })
        .unwrap()
        .0
}

fn remove_blocker_without_rechecking_predecessor(engine: &mut Engine) {
    let rejected = integer_fact(engine, "data", 1);
    let selected = integer_fact(engine, "data", 9);
    engine.retract(selected).unwrap();
    assert_eq!(integer(engine, "calls"), 2);
    run(engine, 1);
    engine.retract(rejected).unwrap();
    run(engine, 0);
    assert_eq!(output(engine), "safe\n");
}

#[test]
fn conflict_replacement_continues_after_blocker_without_rescanning_rejected_predecessor() {
    remove_blocker_without_rechecking_predecessor(&mut rejected_predecessor_engine());
}

fn failing_engine(local: bool, late: bool) -> Engine {
    let parameters = if local { "?x" } else { "?x ?a" };
    let mut engine = setup(&format!(
        r"
        (defglobal ?*calls* = 0)
        (deffunction qualifies ({parameters})
          (bind ?*calls* (+ ?*calls* 1)) (> (/ 1 ?x) 0))
    "
    ));
    if !late {
        install_failing_rule(&mut engine, local);
    }
    engine
}

fn install_failing_rule(engine: &mut Engine, local: bool) {
    let call = if local {
        "(qualifies ?x)"
    } else {
        "(qualifies ?x ?a)"
    };
    engine
        .load_str(&format!(
            "(defrule safe (anchor ?a) (not (data ?x&:{call})) => (printout t \"safe\" crlf))"
        ))
        .unwrap();
}

#[test]
fn failed_local_candidates_are_rejected_but_failed_negative_joins_block_in_both_arrival_orders() {
    for local in [false, true] {
        for candidate_first in [false, true] {
            let mut engine = failing_engine(local, false);
            if !candidate_first {
                engine.assert_ordered("anchor", [2]).unwrap();
            }
            let data = engine.assert_ordered("data", [0]).unwrap();
            if candidate_first {
                assert_eq!(integer(&engine, "calls"), i64::from(local));
                engine.assert_ordered("anchor", [2]).unwrap();
            }
            assert_eq!(integer(&engine, "calls"), 1);
            error_recorded(&engine);
            assert_eq!(engine.agenda_len(), usize::from(local));
            run(&mut engine, usize::from(local));
            engine.retract(data).unwrap();
            run(&mut engine, usize::from(!local));
            assert_eq!(output(&engine), "safe\n");
        }
    }
}

#[test]
fn late_join_error_is_a_blocker_until_retraction() {
    let mut engine = failing_engine(false, true);
    engine.assert_ordered("anchor", [2]).unwrap();
    let data = engine.assert_ordered("data", [0]).unwrap();
    install_failing_rule(&mut engine, false);
    assert_eq!(integer(&engine, "calls"), 1);
    error_recorded(&engine);
    run(&mut engine, 0);
    engine.retract(data).unwrap();
    run(&mut engine, 1);
}

#[test]
fn later_join_candidates_are_not_evaluated_until_selected_blocker_is_removed() {
    for bad_first in [false, true] {
        let mut engine = failing_engine(false, false);
        engine.assert_ordered("anchor", [2]).unwrap();
        let first = engine
            .assert_ordered("data", [if bad_first { 0 } else { 2 }])
            .unwrap();
        assert_eq!(!engine.action_diagnostics().is_empty(), bad_first);
        engine.clear_action_diagnostics();
        let second = engine
            .assert_ordered("data", [if bad_first { 2 } else { 0 }])
            .unwrap();
        assert_eq!(integer(&engine, "calls"), 1);
        assert!(engine.action_diagnostics().is_empty());
        run(&mut engine, 0);
        engine.retract(first).unwrap();
        assert_eq!(integer(&engine, "calls"), 2);
        assert_eq!(!engine.action_diagnostics().is_empty(), !bad_first);
        run(&mut engine, 0);
        engine.retract(second).unwrap();
        run(&mut engine, 1);
    }
}

#[test]
fn failed_return_constraints_follow_the_same_local_vs_join_error_protocol() {
    for local in [false, true] {
        let expression = if local { "(/ 1 ?x)" } else { "(/ ?a 0)" };
        let field = if local {
            format!("?x&={expression}")
        } else {
            format!("={expression}")
        };
        let mut engine = setup(&format!(
            "(defrule safe (anchor ?a) (not (data {field})) => (printout t \"safe\" crlf))"
        ));
        engine.assert_ordered("anchor", [2]).unwrap();
        let data = engine.assert_ordered("data", [0]).unwrap();
        error_recorded(&engine);
        run(&mut engine, usize::from(local));
        engine.retract(data).unwrap();
        run(&mut engine, usize::from(!local));
    }
}

#[test]
fn rhs_assertion_error_stops_current_run_but_retains_correct_next_run_membership() {
    for local in [false, true] {
        let mut engine = failing_engine(local, false);
        engine
            .load_str(
                r#"
            (defrule driver (declare (salience 10)) (go)
              => (printout t "before" crlf) (assert (data 0)) (printout t "after" crlf))
        "#,
            )
            .unwrap();
        engine.assert_ordered("anchor", [2]).unwrap();
        engine.assert_ordered("go", Vec::<i64>::new()).unwrap();
        let result = engine.run(RunLimit::Count(100)).unwrap();
        assert_eq!(result.halt_reason, HaltReason::ActionError);
        assert_eq!(result.rules_fired, 1);
        assert_eq!(output(&engine), "before\n");
        error_recorded(&engine);
        assert_eq!(engine.agenda_len(), usize::from(local));
        run(&mut engine, usize::from(local));
        assert!(engine.action_diagnostics().is_empty());
        assert_eq!(
            output(&engine),
            if local { "before\nsafe\n" } else { "before\n" }
        );
    }
}

#[test]
fn failed_single_pattern_ncc_blocks_while_genuine_two_pattern_ncc_does_not() {
    for two_patterns in [false, true] {
        let inner = if two_patterns {
            "(data ?x) (aux ?x)"
        } else {
            "(data ?x)"
        };
        let mut engine = setup(&format!(
            r#"
            (deffunction qualifies (?x ?a) (> (/ ?a ?x) 0))
            (defrule safe (anchor ?a)
              (not (and {inner} (test (qualifies ?x ?a))))
              => (printout t "safe" crlf))
        "#
        ));
        engine.assert_ordered("anchor", [2]).unwrap();
        engine.assert_ordered("aux", [0]).unwrap();
        let data = engine.assert_ordered("data", [0]).unwrap();
        error_recorded(&engine);
        run(&mut engine, usize::from(two_patterns));
        engine.retract(data).unwrap();
        run(&mut engine, usize::from(!two_patterns));
        assert_eq!(output(&engine), "safe\n");
    }
}

#[test]
fn conjunction_short_circuits_in_reached_source_order() {
    let mut engine = setup(
        r#"
        (deffunction fails (?x ?a) (printout t "fails" crlf) (> (/ ?a ?x) 0))
        (deffunction rejects (?x ?a) (printout t "rejects" crlf) FALSE)
        (defrule error-first (anchor ?a)
          (not (first ?x&:(fails ?x ?a)&:(rejects ?x ?a)))
          => (printout t "wrong" crlf))
        (defrule false-first (anchor ?a)
          (not (second ?x&:(rejects ?x ?a)&:(fails ?x ?a)))
          => (printout t "safe" crlf))
    "#,
    );
    engine.assert_ordered("anchor", [2]).unwrap();
    engine.assert_ordered("first", [0]).unwrap();
    error_recorded(&engine);
    engine.assert_ordered("second", [0]).unwrap();
    assert_eq!(output(&engine), "fails\nrejects\n");
    run(&mut engine, 1);
    assert_eq!(output(&engine), "fails\nrejects\nsafe\n");
}
#[test]
fn direct_join_primitives_keep_their_boolean_contract_without_changing_ordinary_values() {
    for field_value in [false, true] {
        let binding = if field_value { "?flag ?n" } else { "?n" };
        let flag = if field_value { "?flag" } else { "?*gate*" };
        let mut engine = setup(&format!(
            r#"
            (defglobal ?*gate* = FALSE)
            (deffunction wrapped (?flag ?n ?a) (and ?flag (> ?n ?a)))
            (defrule negative (anchor ?a) (not (data {binding}&:(and {flag} (> ?n ?a))))
              => (printout t "wrong-negative" crlf))
            (defrule positive (anchor ?a) (data {binding}&:(and {flag} (> ?n ?a)))
              => (printout t "positive" crlf))
            (defrule explicit-test (anchor ?a) (data {binding}) (test (and {flag} (> ?n ?a)))
              => (printout t "test" crlf))
            (defrule local (data {binding}&:(and {flag} (> ?n 0)))
              => (printout t "wrong-local" crlf))
            (defrule helper (anchor ?a) (not (data {binding}&:(wrapped {flag} ?n ?a)))
              => (printout t "helper-negative" crlf))
            (defrule ordinary (declare (salience -100)) (anchor ?a)
              => (printout t "ordinary:" (and FALSE TRUE) crlf))
        "#
        ));
        engine.assert_ordered("anchor", [2]).unwrap();
        if field_value {
            engine.load_str("(assert (data FALSE 3))").unwrap();
        } else {
            engine.assert_ordered("data", [3]).unwrap();
        }
        run(&mut engine, 4);
        assert_eq!(
            output_lines(&engine),
            ["helper-negative", "ordinary:FALSE", "positive", "test"]
        );
    }
}

#[test]
fn void_predicate_values_are_true_and_reached_join_type_errors_block() {
    let mut engine = setup(
        r#"
        (deffunction local-void (?x) (printout t "local-void" crlf))
        (deffunction join-void (?x ?a) (printout t "join-void" crlf))
        (deffunction numeric (?x ?a) (> (* ?x ?x) ?a))
        (defrule lv (anchor ?a) (not (lv ?x&:(local-void ?x))) => (printout t "wrong-lv" crlf))
        (defrule jv (anchor ?a) (not (jv ?x&:(join-void ?x ?a))) => (printout t "wrong-jv" crlf))
        (defrule jt (anchor ?a) (not (jt ?x&:(numeric ?x ?a))) => (printout t "wrong-type" crlf))
    "#,
    );
    engine.assert_ordered("anchor", [2]).unwrap();
    engine.assert_ordered("lv", [7]).unwrap();
    engine.assert_ordered("jv", [7]).unwrap();
    engine.assert_ordered_symbol("jt", "red").unwrap();
    error_recorded(&engine);
    run(&mut engine, 0);
    assert_eq!(output(&engine), "local-void\njoin-void\n");
}

#[test]
fn repeated_outer_name_uses_current_field_for_local_callback_and_separate_join_equality() {
    let mut engine = setup(
        r#"
        (defglobal ?*calls* = 0)
        (deffunction callback (?x) (bind ?*calls* (+ ?*calls* 1)) TRUE)
        (defrule safe (anchor ?x) (not (data ?x&:(callback ?x))) => (printout t "safe:" ?x crlf))
    "#,
    );
    engine.assert_ordered("data", [3]).unwrap();
    assert_eq!(
        integer(&engine, "calls"),
        1,
        "local call happens before any outer exists"
    );
    engine.assert_ordered("anchor", [3]).unwrap();
    engine.assert_ordered("anchor", [4]).unwrap();
    assert_eq!(integer(&engine, "calls"), 1);
    run(&mut engine, 1);
    assert_eq!(output(&engine), "safe:4\n");
}

#[test]
fn a_later_local_constant_does_not_suppress_an_earlier_reached_callback() {
    let mut engine = setup(
        r#"
        (defglobal ?*calls* = 0)
        (deffunction callback (?x) (bind ?*calls* (+ ?*calls* 1)) TRUE)
        (defrule safe (not (data ?x&:(callback ?x) 7)) => (printout t "safe" crlf))
    "#,
    );
    engine.assert_ordered("data", [2, 8]).unwrap();
    assert_eq!(integer(&engine, "calls"), 1);
    run(&mut engine, 1);
}

#[test]
fn hashable_outer_equality_filters_candidates_but_other_constraints_keep_reached_order() {
    for (pattern, fields, calls, firings) in [
        ("?x&:(callback ?x ?outer) ?wanted", [2, 8], 0, 1),
        ("?wanted ?x&:(callback ?x ?outer)", [8, 2], 0, 1),
        ("?x&:(callback ?x ?outer) ?wanted", [2, 7], 1, 0),
        ("?x&:(callback ?x ?outer) ~?wanted", [2, 7], 1, 1),
    ] {
        let mut engine = setup(&format!(
            r#"
            (defglobal ?*calls* = 0)
            (deffunction callback (?x ?outer) (bind ?*calls* (+ ?*calls* 1)) TRUE)
            (defrule safe (anchor ?outer ?wanted) (not (data {pattern}))
              => (printout t "safe" crlf))
        "#
        ));
        engine.assert_ordered("data", fields).unwrap();
        assert_eq!(integer(&engine, "calls"), 0);
        engine.assert_ordered("anchor", [3, 7]).unwrap();
        assert_eq!(integer(&engine, "calls"), calls, "{pattern}");
        run(&mut engine, firings);
    }
}

const OR_BINDING_FUNCTIONS: &str = r#"
(deffunction accepts (?x) (printout t "PRED:" ?x crlf) (eq ?x 8))
(deffunction later (?x ?y) (printout t "LATER:" ?x ":" ?y crlf) (= ?y (* ?x ?x)))
"#;

#[test]
fn field_disjunction_keeps_leading_binding_on_both_alternatives() {
    for later_field in [false, true] {
        let tail = if later_field { "?y&:(later ?x ?y)" } else { "" };
        let rhs = if later_field {
            "(printout t \"BOUND:\" ?x \":\" ?y crlf)"
        } else {
            "(printout t \"BOUND:\" ?x crlf)"
        };
        let mut engine = setup(&format!(
            "{OR_BINDING_FUNCTIONS} (defrule probe (data ?x&:(accepts ?x)|7 {tail}) => {rhs})"
        ));
        for value in [7, 8] {
            let fields = if later_field {
                vec![value, value * value]
            } else {
                vec![value]
            };
            engine.assert_ordered("data", fields).unwrap();
            run(&mut engine, 1);
        }
        let expected = if later_field {
            "PRED:7\nLATER:7:49\nBOUND:7:49\nPRED:8\nLATER:8:64\nBOUND:8:64\n"
        } else {
            "PRED:7\nBOUND:7\nPRED:8\nBOUND:8\n"
        };
        assert_eq!(output(&engine), expected);
    }
}

#[test]
fn negative_field_disjunction_exposes_binding_to_later_field_and_evaluates_each_candidate_once() {
    let mut engine = setup(&format!(
        r#"
        {OR_BINDING_FUNCTIONS}
        (defrule safe (not (data ?x&:(accepts ?x)|7 ?y&:(later ?x ?y)))
          => (printout t "SAFE" crlf))
    "#
    ));
    let blocker = engine.assert_ordered("data", [7, 49]).unwrap();
    run(&mut engine, 0);
    engine.assert_ordered("data", [7, 48]).unwrap();
    run(&mut engine, 0);
    engine.retract(blocker).unwrap();
    run(&mut engine, 1);
    assert_eq!(
        output(&engine),
        "PRED:7\nLATER:7:49\nPRED:7\nLATER:7:48\nSAFE\n"
    );
}

#[test]
fn false_predicate_or_literal_is_one_local_pattern_not_duplicate_rule_activations() {
    let mut engine = setup(
        r#"
        (defglobal ?*calls* = 0)
        (deffunction callback (?x) (bind ?*calls* (+ ?*calls* 1)) FALSE)
        (defrule safe (not (data ?x&:(callback ?x)|7)) => (printout t "SAFE" crlf))
    "#,
    );
    let blocker = engine.assert_ordered("data", [7]).unwrap();
    engine.assert_ordered("data", [8]).unwrap();
    assert_eq!(integer(&engine, "calls"), 2);
    run(&mut engine, 0);
    engine.retract(blocker).unwrap();
    run(&mut engine, 1);
    assert_eq!(output(&engine), "SAFE\n");
    assert_eq!(engine.rules().len(), 1);
}

#[test]
fn disjunction_error_verdict_depends_on_placement_and_reached_primitive_kind() {
    // A later user-call primitive clears EvaluationError while sticky halt
    // skips its body. A direct variable primitive leaves the error set.
    for (pattern, expected) in [
        (
            "(anchor ?a) (not (data ?x&:(or (fails ?x ?a) (later ?x ?a))))",
            1,
        ),
        ("(anchor ?a) (data ?x&:(or (fails ?x ?a) (later ?x ?a)))", 0),
        ("(not (data ?x&:(or (fails ?x 0) (later ?x 0))))", 1),
        ("(anchor ?a) (not (data ?x&:(or (fails ?x ?a) ?x)))", 0),
    ] {
        let mut engine = setup(&format!(
            r#"
            (deffunction fails (?x ?a) (printout t "FAIL" crlf) (/ 1 0))
            (deffunction later (?x ?a) (printout t "LATER" crlf) TRUE)
            (defrule check {pattern} => (printout t "MATCH" crlf))
        "#
        ));
        engine.assert_ordered("anchor", [1]).unwrap();
        engine.assert_ordered_symbol("data", "FALSE").unwrap();
        error_recorded(&engine);
        assert_eq!(output(&engine), "FAIL\n");
        assert_eq!(engine.agenda_len(), expected, "{pattern}");
        run(&mut engine, expected);
        assert_eq!(
            output(&engine),
            if expected == 1 {
                "FAIL\nMATCH\n"
            } else {
                "FAIL\n"
            }
        );
    }
}

#[test]
fn reset_undefine_and_reload_clear_owned_filter_and_conflict_state() {
    let mut engine = setup(COUNTED_JOIN);
    for _ in 0..2 {
        engine.assert_ordered("anchor", [2]).unwrap();
        let data = engine.assert_ordered("data", [3]).unwrap();
        run(&mut engine, 0);
        engine
            .load_str("(defrule erase (declare (salience 100)) => (undefrule safe erase))")
            .unwrap();
        run(&mut engine, 1);
        assert!(engine.rules().is_empty());
        engine.retract(data).unwrap();
        assert_eq!(engine.agenda_len(), 0);
        let rule = &COUNTED_JOIN[COUNTED_JOIN.find("(defrule safe").unwrap()..];
        engine.load_str(rule).unwrap();
        run(&mut engine, 1);
        assert_eq!(output(&engine), "safe:2\n");
        engine.reset().unwrap();
        assert_eq!(integer(&engine, "calls"), 0);
        assert_eq!(integer(&engine, "hits"), 0);
        assert_eq!(output(&engine), "");
        run(&mut engine, 0);
    }
    engine.clear();
    engine.load_str(COUNTED_JOIN).unwrap();
    engine.reset().unwrap();
    engine.assert_ordered("anchor", [2]).unwrap();
    run(&mut engine, 1);
}

#[cfg(feature = "serde")]
#[test]
fn snapshots_preserve_local_membership_and_late_owners_without_reevaluating_callbacks() {
    use ferric_rules_runtime::SerializationFormat;
    let mut pending = gate_engine(true, true);
    pending.assert_ordered("data", [3]).unwrap();
    set_gate(&mut pending, false);
    install_gate_rule(&mut pending, true);
    set_gate(&mut pending, true);
    assert_eq!(integer(&pending, "calls"), 1);
    for &format in SerializationFormat::ALL {
        let mut restored =
            Engine::deserialize(&pending.serialize(format).unwrap(), format).unwrap();
        restored.assert_ordered("anchor", [2]).unwrap();
        restored.assert_ordered("anchor", [4]).unwrap();
        run(&mut restored, 2);
        assert_eq!(integer(&restored, "calls"), 1);
        let mut completed =
            Engine::deserialize(&restored.serialize(format).unwrap(), format).unwrap();
        run(&mut completed, 0);
        let old = completed.find_facts("data").unwrap()[0].0;
        completed.retract(old).unwrap();
        let new = completed.assert_ordered("data", [3]).unwrap();
        assert_eq!(integer(&completed, "calls"), 2);
        run(&mut completed, 0);
        completed.retract(new).unwrap();
        run(&mut completed, 2);
    }
}

#[cfg(feature = "serde")]
#[test]
fn snapshots_preserve_selected_conflict_successor_and_all_owner_links() {
    use ferric_rules_runtime::SerializationFormat;
    let mut pending = setup(COUNTED_JOIN);
    pending.assert_ordered("anchor", [2]).unwrap();
    pending.assert_ordered("anchor", [5]).unwrap();
    pending.assert_ordered("data", [3]).unwrap();
    pending.assert_ordered("data", [6]).unwrap();
    for &format in SerializationFormat::ALL {
        let mut restored =
            Engine::deserialize(&pending.serialize(format).unwrap(), format).unwrap();
        assert_eq!(integer(&restored, "calls"), 3);
        let first = integer_fact(&restored, "data", 3);
        let second = integer_fact(&restored, "data", 6);
        restored.retract(first).unwrap();
        assert_eq!(integer(&restored, "calls"), 4);
        run(&mut restored, 0);
        restored.retract(second).unwrap();
        run(&mut restored, 2);
        assert_eq!(output_lines(&restored), ["safe:2", "safe:5"]);
        let fresh = restored.assert_ordered("data", [6]).unwrap();
        run(&mut restored, 0);
        restored.retract(fresh).unwrap();
        run(&mut restored, 2);
    }
    let pending = rejected_predecessor_engine();
    for &format in SerializationFormat::ALL {
        let mut restored =
            Engine::deserialize(&pending.serialize(format).unwrap(), format).unwrap();
        remove_blocker_without_rechecking_predecessor(&mut restored);
    }
}

#[cfg(feature = "serde")]
#[test]
fn snapshots_preserve_error_blocker_and_pending_rhs_halt_membership() {
    use ferric_rules_runtime::SerializationFormat;
    for local in [false, true] {
        let mut pending = failing_engine(local, false);
        pending
            .load_str(
                r#"
            (defrule driver (declare (salience 10)) (go)
              => (assert (data 0)) (printout t "wrong-after" crlf))
        "#,
            )
            .unwrap();
        pending.assert_ordered("anchor", [2]).unwrap();
        pending.assert_ordered("go", Vec::<i64>::new()).unwrap();
        assert_eq!(
            pending.run(RunLimit::Count(100)).unwrap().halt_reason,
            HaltReason::ActionError
        );
        for &format in SerializationFormat::ALL {
            let mut restored =
                Engine::deserialize(&pending.serialize(format).unwrap(), format).unwrap();
            error_recorded(&restored);
            assert_eq!(restored.agenda_len(), usize::from(local));
            run(&mut restored, usize::from(local));
            let data = restored.find_facts("data").unwrap()[0].0;
            restored.retract(data).unwrap();
            run(&mut restored, usize::from(!local));
            assert_eq!(output(&restored), "safe\n");
        }
    }
}

// Exact initial and replacement orders are pinned separately: new right facts
// visit newest outer owners first, while each shared block list replaces its
// selected owners in reverse selection order, including across rule joins.
// Both callback traces are measured against the pinned reference image.
fn shared_blocker_engine(two_rules: bool) -> Engine {
    let source = if two_rules {
        r#"
        (deffunction qualifies (?label ?x ?a)
          (printout t "CHECK:" ?label ":" ?x ":" ?a crlf) TRUE)
        (defrule first (anchor ?a) (not (data ?x&:(qualifies first ?x ?a)))
          => (printout t "FIRST:" ?a crlf))
        (defrule second (anchor ?a) (not (data ?x&:(qualifies second ?x ?a)))
          => (printout t "SECOND:" ?a crlf))
        "#
    } else {
        r#"
        (deffunction qualifies (?x ?a) (printout t "CHECK:" ?x ":" ?a crlf) TRUE)
        (defrule safe (anchor ?a) (not (data ?x&:(qualifies ?x ?a)))
          => (printout t "SAFE:" ?a crlf))
        "#
    };
    let mut engine = setup(source);
    engine.assert_ordered("anchor", [2]).unwrap();
    engine.assert_ordered("anchor", [4]).unwrap();
    assert_eq!(output(&engine), "");
    engine.assert_ordered("data", [9]).unwrap();
    let expected = if two_rules {
        "CHECK:second:9:4\nCHECK:second:9:2\nCHECK:first:9:4\nCHECK:first:9:2\n"
    } else {
        "CHECK:9:4\nCHECK:9:2\n"
    };
    assert_eq!(output(&engine), expected);
    engine.clear_output_channel("t");
    engine.assert_ordered("data", [8]).unwrap();
    assert_eq!(
        output(&engine),
        "",
        "blocked owners skip appended candidate"
    );
    assert_eq!(engine.agenda_len(), 0);
    engine
}

fn check_shared_blocker_replacement(engine: &mut Engine, two_rules: bool) {
    let blocker = integer_fact(engine, "data", 9);
    engine.retract(blocker).unwrap();
    let expected = if two_rules {
        "CHECK:first:8:2\nCHECK:first:8:4\nCHECK:second:8:2\nCHECK:second:8:4\n"
    } else {
        "CHECK:8:2\nCHECK:8:4\n"
    };
    assert_eq!(
        output(engine),
        expected,
        "replacement occurs during retraction"
    );
    assert_eq!(engine.agenda_len(), 0);
    run(engine, 0);
    assert_eq!(
        output(engine),
        expected,
        "all owners acquired the replacement"
    );
}

#[test]
fn shared_blocker_replaces_owners_in_reverse_selection_order() {
    check_shared_blocker_replacement(&mut shared_blocker_engine(false), false);
}

#[test]
fn shared_alpha_blocker_replacement_order_crosses_rule_boundaries() {
    check_shared_blocker_replacement(&mut shared_blocker_engine(true), true);
}

#[test]
fn new_outer_searches_existing_candidates_chronologically_and_resumes_at_successor() {
    // The pinned left-arrival trace differs from right arrival: unlike its
    // arrival's newest-owner traversal, candidate memory is chronological.
    let mut engine = setup(
        r#"
        (deffunction qualifies (?x ?a) (printout t "CHECK:" ?x ":" ?a crlf) (= ?x 9))
        (defrule safe (anchor ?a) (not (data ?x&:(qualifies ?x ?a)))
          => (printout t "SAFE:" ?a crlf))
    "#,
    );
    let predecessor = engine.assert_ordered("data", [1]).unwrap();
    let selected = engine.assert_ordered("data", [9]).unwrap();
    let successor = engine.assert_ordered("data", [8]).unwrap();
    assert_eq!(output(&engine), "");
    engine.assert_ordered("anchor", [2]).unwrap();
    assert_eq!(output(&engine), "CHECK:1:2\nCHECK:9:2\n");
    run(&mut engine, 0);
    engine.clear_output_channel("t");
    engine.retract(selected).unwrap();
    assert_eq!(output(&engine), "CHECK:8:2\n");
    run(&mut engine, 1);
    assert_eq!(output(&engine), "CHECK:8:2\nSAFE:2\n");
    engine.retract(predecessor).unwrap();
    run(&mut engine, 0);
    engine.retract(successor).unwrap();
    run(&mut engine, 0);
    assert_eq!(output(&engine), "CHECK:8:2\nSAFE:2\n");
}

#[cfg(feature = "serde")]
#[test]
fn all_codecs_preserve_shared_blocker_owner_order_across_rule_boundaries() {
    use ferric_rules_runtime::SerializationFormat;
    for two_rules in [false, true] {
        let pending = shared_blocker_engine(two_rules);
        for &format in SerializationFormat::ALL {
            let mut restored =
                Engine::deserialize(&pending.serialize(format).unwrap(), format).unwrap();
            assert_eq!(output(&restored), "");
            check_shared_blocker_replacement(&mut restored, two_rules);
        }
    }
}

const TEMPLATE_LOCAL_SOURCE: &str = r#"(defglobal ?*blocker* = FALSE)
(deftemplate record (slot squared) (slot base))
(deffunction square (?x ?y) (printout t "CHECK:" ?x ":" ?y crlf) (= ?y (* ?x ?x)))
(defrule safe (not (record (base ?x) (squared ?y&:(square ?x ?y)))) => (printout t "SAFE" crlf))
"#;

const TEMPLATE_RETURN_SOURCE: &str = r#"(defglobal ?*blocker* = FALSE)
(deftemplate anchor (slot value))
(deftemplate record (slot unused (default 0)) (slot squared))
(deffunction square (?x) (printout t "SQUARE:" ?x crlf) (* ?x ?x))
(defrule safe (anchor (value ?x)) (not (record (squared =(square ?x)))) => (printout t "SAFE:" ?x crlf))
"#;

const MULTISLOT_LOCAL_SOURCE: &str = r#"(defglobal ?*blocker* = FALSE)
(deftemplate bag (slot tag (default ok)) (multislot items))
(deffunction long (?xs) (printout t "LENGTH:" (length$ ?xs) crlf) (> (length$ ?xs) 1))
(defrule safe (not (bag (items $?xs&:(long ?xs)))) => (printout t "SAFE" crlf))
"#;

const MODULE_LOCAL_SOURCE: &str = r#"(defglobal ?*gate* = 100 ?*blocker* = FALSE)
(deffunction qualifies (?x) (printout t "WRONG-MAIN" crlf) FALSE)
(defmodule DATA (export deftemplate ?ALL))
(deftemplate anchor (slot value))
(deftemplate item (slot value))
(defmodule APP (import DATA deftemplate ?ALL))
(defglobal ?*gate* = 1 ?*blocker* = FALSE)
(deffunction qualifies (?x) (printout t "LOCAL:" ?x ":" ?*gate* crlf) (> ?x ?*gate*))
(defrule safe (anchor (value ?a)) (not (item (value ?x&:(qualifies ?x)))) => (printout t "SAFE:" ?a crlf))
"#;

const MODULE_IMPORTED_SOURCE: &str = r#"(defglobal ?*threshold* = 100)
(deffunction qualifies (?x ?a) (printout t "WRONG-MAIN" crlf) FALSE)
(defmodule DATA (export ?ALL))
(defglobal ?*offset* = 1)
(deftemplate anchor (slot value))
(deftemplate item (slot value))
(deffunction qualifies (?x ?a) (printout t "IMPORTED:" ?x ":" ?a ":" ?*offset* crlf) (> (* ?x ?x) (+ ?a ?*offset*)))
(defmodule APP (import DATA ?ALL))
(defglobal ?*threshold* = 1 ?*blocker* = FALSE)
(defrule safe (anchor (value ?a)) (not (item (value ?x&:(qualifies ?x (+ ?a ?*threshold*))))) => (printout t "SAFE:" ?a crlf))
"#;

#[test]
fn template_local_predicate_uses_written_binding_order_and_named_physical_slots() {
    let mut engine = setup(TEMPLATE_LOCAL_SOURCE);
    let blocker = engine
        .assert_template("record", &["base", "squared"], [2, 4])
        .unwrap();
    engine
        .assert_template("record", &["base", "squared"], [3, 8])
        .unwrap();
    assert_eq!(output(&engine), "CHECK:2:4\nCHECK:3:8\n");
    run(&mut engine, 0);
    engine.retract(blocker).unwrap();
    run(&mut engine, 1);
    assert_eq!(output(&engine), "CHECK:2:4\nCHECK:3:8\nSAFE\n");
}

#[test]
fn template_return_constraint_resolves_outer_values_in_nonfirst_slot() {
    let mut engine = setup(TEMPLATE_RETURN_SOURCE);
    let blocker = engine.assert_template("record", &["squared"], [4]).unwrap();
    assert_eq!(output(&engine), "");
    engine.assert_template("anchor", &["value"], [2]).unwrap();
    engine.assert_template("anchor", &["value"], [3]).unwrap();
    assert_eq!(output(&engine), "SQUARE:2\nSQUARE:3\n");
    run(&mut engine, 1);
    engine.retract(blocker).unwrap();
    run(&mut engine, 1);
    assert_eq!(output(&engine), "SQUARE:2\nSQUARE:3\nSAFE:3\nSAFE:2\n");
}

fn whole_multislot_engine() -> Engine {
    use ferric_rules_runtime::HostValue;
    let mut engine = setup(MULTISLOT_LOCAL_SOURCE);
    for fields in [vec![], vec![7_i64], vec![7_i64, 8]] {
        let value =
            HostValue::multifield(fields.into_iter().map(HostValue::from).collect()).unwrap();
        engine.assert_template("bag", &["items"], [value]).unwrap();
    }
    assert_eq!(output(&engine), "LENGTH:0\nLENGTH:1\nLENGTH:2\n");
    assert!(engine.action_diagnostics().is_empty());
    assert_eq!(engine.agenda_len(), 0);
    engine
}

fn remove_multislot_blocker(engine: &mut Engine) {
    let blocker = engine.facts().unwrap().find_map(|(id, fact)| {
        match fact {
            ferric_rules_core::Fact::Template(fact)
                if matches!(&fact.slots[1], Value::Multifield(values) if values.len() == 2) => Some(id),
            _ => None,
        }
    }).unwrap();
    engine.retract(blocker).unwrap();
    run(engine, 1);
    assert_eq!(output(engine), "LENGTH:0\nLENGTH:1\nLENGTH:2\nSAFE\n");
}

#[test]
fn whole_multislot_predicates_receive_empty_singleton_and_multiple_fields() {
    let mut engine = whole_multislot_engine();
    run(&mut engine, 0);
    remove_multislot_blocker(&mut engine);
}

#[test]
fn multislot_runtime_scope_rejects_mixed_cardinality_and_sequence_dependent_forms() {
    for constraint in [
        "$?xs&=(repeated ?x)",
        "?xs&:(> (* ?xs ?xs) 0)",
        "=(repeated ?x)",
    ] {
        let mut engine = setup(
            r"
            (deftemplate anchor (slot value)) (deftemplate bag (multislot items))
            (deffunction repeated (?x) (create$ ?x ?x))
        ",
        );
        // The first form is invalid CLIPS single/multifield mixing. The other
        // two are valid CLIPS single-field constraints that require the separate
        // multislot sequence matcher; they are explicitly outside this repair.
        assert!(engine.load_str(&format!(
            "(defrule unsupported (anchor (value ?x)) (not (bag (items {constraint}))) => (assert (wrong)))"
        )).is_err(), "unexpected unsupported admission: {constraint}");
        assert!(engine.rules().is_empty());
    }
}

#[test]
fn local_template_filter_uses_its_rule_module_and_global_before_any_outer() {
    let mut engine = setup(MODULE_LOCAL_SOURCE);
    let blocker = engine.assert_template("item", &["value"], [3]).unwrap();
    assert_eq!(output(&engine), "LOCAL:3:1\n");
    // Use one rule to apply the reference's top-level APP global bind through
    // the public API, before introducing the outer match.
    engine
        .load_str("(defmodule APP (import DATA deftemplate ?ALL)) (defrule raise-threshold (raise-threshold) => (bind ?*gate* 10))")
        .unwrap();
    engine
        .assert_ordered("raise-threshold", Vec::<i64>::new())
        .unwrap();
    engine.set_focus("APP").unwrap();
    run(&mut engine, 1);
    assert_eq!(integer(&engine, "gate"), 10);
    engine.assert_template("anchor", &["value"], [2]).unwrap();
    engine.set_focus("APP").unwrap();
    run(&mut engine, 0);
    assert_eq!(output(&engine), "LOCAL:3:1\n");
    engine.retract(blocker).unwrap();
    engine.set_focus("APP").unwrap();
    run(&mut engine, 1);
    assert_eq!(output(&engine), "LOCAL:3:1\nSAFE:2\n");
}

#[test]
fn imported_template_join_resolves_rule_globals_and_callable_defining_module() {
    let mut engine = setup(MODULE_IMPORTED_SOURCE);
    let blocker = engine.assert_template("item", &["value"], [3]).unwrap();
    assert_eq!(output(&engine), "");
    engine.assert_template("anchor", &["value"], [2]).unwrap();
    assert_eq!(output(&engine), "IMPORTED:3:3:1\n");
    engine.set_focus("APP").unwrap();
    run(&mut engine, 0);
    engine.retract(blocker).unwrap();
    engine.set_focus("APP").unwrap();
    run(&mut engine, 1);
    assert_eq!(output(&engine), "IMPORTED:3:3:1\nSAFE:2\n");
}

#[test]
fn hidden_module_globals_and_callables_reject_empty_memory_rule_replacement() {
    for expression in ["(hidden ?x)", "(> (* ?x ?x) ?*gate*)"] {
        let mut engine = Engine::new(EngineConfig::utf8());
        engine
            .load_str(
                r"
            (defmodule DATA (export deftemplate ?ALL))
            (deftemplate item (slot value))
            (deffunction hidden (?x) (> ?x 0)) (defglobal ?*gate* = 2)
            (defmodule APP (import DATA deftemplate ?ALL))
            (defrule invalid => (assert (preserved)))
        ",
            )
            .unwrap();
        assert!(engine
            .load_str(&format!(
                "(defrule invalid (not (item (value ?x&:{expression}))) => (assert (wrong)))"
            ))
            .is_err());
        assert_eq!(engine.rules().len(), 1);
        engine.reset().unwrap();
        engine.set_focus("APP").unwrap();
        run(&mut engine, 1);
    }
}

#[test]
fn nonleading_field_variables_require_prior_bindings() {
    for operator in ["|", "&"] {
        let mut engine = setup(
            r"
            (deffunction callback (?y) TRUE)
            (defrule preserved => (assert (preserved)))
        ",
        );
        assert!(
            engine
                .load_str(&format!(
                    "(defrule invalid (data 1{operator}?x ?y&:(callback ?y)) => (printout t ?x))"
                ))
                .is_err(),
            "nonleading binding admitted: {operator}"
        );
        assert_eq!(engine.rules().len(), 1);
    }
    for (operator, outer, expected) in [
        ("|", 2, "CHECK:8\nCHECK:9\nBOUND:2:9\nBOUND:2:8\n"),
        ("&", 1, "CHECK:8\nBOUND:1:8\n"),
    ] {
        let mut engine = setup(&format!(
            r#"
            (deffunction callback (?y) (printout t "CHECK:" ?y crlf) TRUE)
            (defrule r (anchor ?x) (data 1{operator}?x ?y&:(callback ?y))
              => (printout t "BOUND:" ?x ":" ?y crlf))
        "#
        ));
        engine.assert_ordered("anchor", [outer]).unwrap();
        engine.assert_ordered("data", [1, 8]).unwrap();
        engine.assert_ordered("data", [2, 9]).unwrap();
        run(&mut engine, if operator == "|" { 2 } else { 1 });
        assert_eq!(output(&engine), expected);
    }
}

#[test]
fn bare_disjunction_variables_require_prior_bindings() {
    for source in [
        "(defrule invalid (data ?x|1) =>)",
        "(deftemplate pair (slot x) (slot y)) (defrule invalid (pair (x ?x|?y) (y ?x|?y)) =>)",
        "(deftemplate pair (slot x) (slot y)) (defrule invalid (seed ?y) (pair (x ?x|?y) (y ?x|?y)) =>)",
    ] {
        let mut engine = Engine::new(EngineConfig::utf8());
        assert!(engine.load_str(source).is_err(), "bare OR introduced a binding: {source}");
        assert!(engine.rules().is_empty());
    }
    let mut engine = setup(
        "(deftemplate pair (slot x) (slot y)) (defrule valid (seed ?x ?y) (pair (x ?x|?y) (y ?x|?y)) =>)",
    );
    engine.assert_ordered("seed", [1, 2]).unwrap();
    engine.assert_template("pair", &["x", "y"], [1, 2]).unwrap();
    run(&mut engine, 1);
}

#[test]
fn leading_primitive_disjunction_binding_survives_constraint_expansion() {
    let mut engine = setup(
        r#"
        (defrule r (data ?x&1|2) (test (> ?x 0)) => (printout t "BOUND:" ?x crlf))
    "#,
    );
    engine.assert_ordered("data", [1]).unwrap();
    engine.assert_ordered("data", [2]).unwrap();
    run(&mut engine, 2);
    assert_eq!(output(&engine), "BOUND:2\nBOUND:1\n");
}

#[cfg(feature = "serde")]
#[test]
fn whole_multislot_filter_membership_survives_all_codecs_without_reevaluation() {
    use ferric_rules_runtime::SerializationFormat;
    let pending = whole_multislot_engine();
    for &format in SerializationFormat::ALL {
        let mut restored =
            Engine::deserialize(&pending.serialize(format).unwrap(), format).unwrap();
        run(&mut restored, 0);
        remove_multislot_blocker(&mut restored);
    }
}
