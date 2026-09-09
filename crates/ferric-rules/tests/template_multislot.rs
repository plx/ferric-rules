//! Issue #322: template multislots match complete field sequences.
//!
//! Every fixture was checked with CLIPS 6.30 (3/17/15), image
//! sha256:b4b99ba2f08102f9c18743c6adaecb5ab82789ddfa8628693cabfeced461a929,
//! both normally and with rules loaded after reset. Ordering fixtures were
//! additionally checked under breadth; the remaining fixtures use depth.

use ferric_rules::core::ConflictResolutionStrategy;
use ferric_rules::runtime::{Engine, EngineConfig, HaltReason, RunLimit};

fn check(source: &str, expected: &str, late_rules: bool, strategy: ConflictResolutionStrategy) {
    let mut engine = Engine::new(EngineConfig::utf8().with_strategy(strategy));
    if late_rules {
        // Backfill must recover every multislot split and preserve written slot order.
        let rules_start = source.find("(defrule").expect("fixture has rules");
        engine.load_str(&source[..rules_start]).unwrap();
        engine.reset().unwrap();
        engine.load_str(&source[rules_start..]).unwrap();
    } else {
        engine.load_str(source).unwrap();
        engine.reset().unwrap();
    }

    #[cfg(feature = "serde")]
    for &format in ferric_rules::runtime::SerializationFormat::ALL {
        let bytes = engine.serialize(format).unwrap();
        let mut restored = Engine::deserialize(&bytes, format).unwrap();
        assert_output(&mut restored, expected);
    }
    assert_output(&mut engine, expected);
}

fn assert_output(engine: &mut Engine, expected: &str) {
    let result = engine.run(RunLimit::Count(1_000)).unwrap();
    assert_eq!(result.halt_reason, HaltReason::AgendaEmpty);
    assert_eq!(engine.get_output("t").unwrap().unwrap_or(""), expected);
    assert!(
        engine.action_diagnostics().is_empty(),
        "{:?}",
        engine.action_diagnostics()
    );
}

macro_rules! fixture {
    ($name:ident) => {
        #[test]
        fn $name() {
            let source = include_str!(concat!("fixtures/core/", stringify!($name), ".clp"));
            let expected = include_str!(concat!("fixtures/core/", stringify!($name), ".out"));
            for late_rules in [false, true] {
                check(
                    source,
                    expected,
                    late_rules,
                    ConflictResolutionStrategy::Depth,
                );
            }
        }
    };
}

fixture!(template_multislot_prefix_capture);
fixture!(template_multislot_whole_capture);
fixture!(template_multislot_single_field);
fixture!(template_multislot_empty_exact);
fixture!(template_multislot_single_anonymous);
fixture!(template_multislot_two_exact);
fixture!(template_multislot_whole_anonymous);
fixture!(template_multislot_omitted_constraint);
fixture!(template_multislot_mixed_types);
fixture!(template_multislot_middle_empty);
fixture!(template_multislot_middle_nonempty);
fixture!(template_multislot_split_order);
fixture!(template_multislot_cross_slot_equality);
fixture!(template_multislot_cross_slot_split_equality);
fixture!(template_multislot_anonymous_product);
fixture!(template_multislot_declaration_order);
fixture!(template_multislot_reverse_order);
fixture!(template_multislot_single_slot_wildcard);
fixture!(template_multislot_capture_joins);
fixture!(template_multislot_connected_constraints);
fixture!(template_multislot_repeated_capture);
fixture!(template_multislot_retract_splits);
fixture!(template_multislot_negative_transitions);
fixture!(template_multislot_owner_lifecycle);
fixture!(template_multislot_indexed_joins);

fn projected_template_join_control(
    template_slots: &str,
    fact_slots: impl Fn(usize) -> String,
    pattern_slots: &str,
) -> String {
    use std::fmt::Write as _;

    let mut source = format!(
        "(deftemplate item {template_slots} (slot side))\n\
         (defglobal ?*left* = 0 ?*right* = 0)\n\
         (deffacts input\n"
    );
    // Cross both candidate-index thresholds, with each arrival direction
    // isolated by a physical scalar slot constant.
    for key in 0..24 {
        writeln!(&mut source, "  (key-before k{key})").unwrap();
    }
    for side in ["right", "left"] {
        for key in 0..24 {
            writeln!(&mut source, "  (item {} (side {side}))", fact_slots(key)).unwrap();
        }
    }
    for key in 0..24 {
        writeln!(&mut source, "  (key-after k{key})").unwrap();
    }
    source.push_str(")\n");
    writeln!(
        &mut source,
        "(defrule right-arrival\n\
           (key-before ?key) (item {pattern_slots} (side right))\n\
           => (bind ?*right* (+ ?*right* 1)))"
    )
    .unwrap();
    writeln!(
        &mut source,
        "(defrule left-arrival\n\
           (key-after ?key) (item {pattern_slots} (side left))\n\
           => (bind ?*left* (+ ?*left* 1)))"
    )
    .unwrap();
    source.push_str(
        r#"(defrule summary (declare (salience -10))
             => (printout t ?*left* ":" ?*right* crlf))
"#,
    );
    source
}

#[test]
fn template_single_element_multislot_is_not_a_raw_scalar_index() {
    // A Single projection of a multislot still reads a raw Multifield value.
    let source = projected_template_join_control(
        "(multislot value)",
        |key| format!("(value k{key})"),
        "(value ?key)",
    );
    for late_rules in [false, true] {
        check(
            &source,
            "24:24\n",
            late_rules,
            ConflictResolutionStrategy::Depth,
        );
    }
}

#[test]
fn template_written_slot_order_is_not_physical_index_order() {
    // The key is logical slot 0, but raw slot 0 contains the decoy instead.
    let source = projected_template_join_control(
        "(slot decoy) (slot key) (multislot tags)",
        |key| format!("(decoy sentinel) (key k{key}) (tags x y)"),
        "(key ?key) (tags $?tail) (decoy sentinel)",
    );
    for late_rules in [false, true] {
        check(
            &source,
            "24:24\n",
            late_rules,
            ConflictResolutionStrategy::Depth,
        );
    }
}

macro_rules! rejected_fixture {
    ($name:ident) => {
        #[test]
        fn $name() {
            let source = include_str!(concat!("fixtures/core/", stringify!($name), ".clp"));
            // The companion .out preserves the pinned CLIPS rejection evidence.
            // Engine diagnostics can differ in wording, but accepting the rule
            // would silently change this single-slot syntax into a valid pattern.
            let mut engine = Engine::new(EngineConfig::utf8());
            assert!(
                engine.load_str(source).is_err(),
                "invalid single-slot pattern loaded"
            );
        }
    };
}

rejected_fixture!(template_multislot_single_slot_empty_rejected);
rejected_fixture!(template_multislot_single_slot_many_rejected);
rejected_fixture!(template_multislot_single_slot_multivariable_rejected);

#[test]
fn template_multislot_declaration_order_breadth() {
    let source = include_str!("fixtures/core/template_multislot_declaration_order.clp");
    let expected = include_str!("fixtures/core/template_multislot_declaration_order.breadth.out");
    for late_rules in [false, true] {
        check(
            source,
            expected,
            late_rules,
            ConflictResolutionStrategy::Breadth,
        );
    }
}

#[test]
fn template_multislot_reverse_order_breadth() {
    let source = include_str!("fixtures/core/template_multislot_reverse_order.clp");
    let expected = include_str!("fixtures/core/template_multislot_reverse_order.breadth.out");
    for late_rules in [false, true] {
        check(
            source,
            expected,
            late_rules,
            ConflictResolutionStrategy::Breadth,
        );
    }
}

#[test]
fn template_multislot_refraction_survives_incremental_runs() {
    let source = include_str!("fixtures/core/template_multislot_declaration_order.clp");
    let expected = include_str!("fixtures/core/template_multislot_declaration_order.out");
    let mut engine = Engine::new(EngineConfig::utf8());
    engine.load_str(source).unwrap();
    engine.reset().unwrap();

    // Pause between all four combinations of the two constrained multislots.
    // Restoring a partially fired agenda must retain the remaining combinations.
    let mut firings = 0;
    loop {
        let result = engine.run(RunLimit::Count(1)).unwrap();
        if result.halt_reason == HaltReason::AgendaEmpty {
            break;
        }
        firings += 1;
        assert!(
            firings <= 4,
            "a previously fired split became eligible again"
        );
        #[cfg(feature = "serde")]
        for &format in ferric_rules::runtime::SerializationFormat::ALL {
            let bytes = engine.serialize(format).unwrap();
            let mut restored = Engine::deserialize(&bytes, format).unwrap();
            assert_output(&mut restored, expected);
        }
    }
    assert_output(&mut engine, expected);
}

#[test]
fn template_multislot_top_level_assert_after_reset_and_restore() {
    let constructs = include_str!("fixtures/core/template_multislot_top_level_assert.clp");
    let assertions = include_str!("fixtures/core/template_multislot_top_level_assertions.clp");
    let expected = include_str!("fixtures/core/template_multislot_top_level_assert.out");
    let mut engine = Engine::new(EngineConfig::utf8());
    engine.load_str(constructs).unwrap();
    engine.reset().unwrap();

    // The pinned CLIPS invocation loads constructs, resets, executes every
    // companion assertion, then runs. Resetting after assertions would erase
    // the facts and fail to exercise top-level assertion storage.
    #[cfg(feature = "serde")]
    for &format in ferric_rules::runtime::SerializationFormat::ALL {
        let bytes = engine.serialize(format).unwrap();
        let mut restored_plan = Engine::deserialize(&bytes, format).unwrap();
        restored_plan.load_str(assertions).unwrap();
        let populated = restored_plan.serialize(format).unwrap();
        let mut restored_matches = Engine::deserialize(&populated, format).unwrap();
        assert_output(&mut restored_matches, expected);
        assert_output(&mut restored_plan, expected);
    }

    engine.load_str(assertions).unwrap();
    #[cfg(feature = "serde")]
    for &format in ferric_rules::runtime::SerializationFormat::ALL {
        let bytes = engine.serialize(format).unwrap();
        let mut restored = Engine::deserialize(&bytes, format).unwrap();
        assert_output(&mut restored, expected);
    }
    assert_output(&mut engine, expected);
}
