//! PR378 + PR351 composition: physical ordered widths precede runtime callbacks.
//! These are regression expectations, not newly executed reference probes.

use ferric_rules_core::Value;
use ferric_rules_runtime::{Engine, EngineConfig, HaltReason, RunLimit};

const PREFIX: &str = r"
(defglobal ?*calls* = 0)
(deffunction local-positive (?value)
  (bind ?*calls* (+ ?*calls* 1))
  (> (/ 1 ?value) 0))
(deffunction join-positive (?value ?limit)
  (bind ?*calls* (+ ?*calls* 1))
  (> (/ ?value ?limit) 0))
";

fn setup(rule: &str) -> Engine {
    let mut engine = Engine::new(EngineConfig::utf8());
    engine.load_str(&format!("{PREFIX}\n{rule}")).unwrap();
    engine.reset().unwrap();
    engine
}

fn calls(engine: &Engine) -> i64 {
    let Some(Value::Integer(value)) = engine.get_global("calls") else {
        panic!("INTEGER calls")
    };
    *value
}

fn run(engine: &mut Engine, expected: usize) {
    let result = engine.run(RunLimit::Count(20)).unwrap();
    assert_eq!(result.rules_fired, expected);
    assert_eq!(result.halt_reason, HaltReason::AgendaEmpty);
    assert!(engine.action_diagnostics().is_empty());
}

#[test]
fn positive_and_negative_local_callbacks_skip_short_and_excess_widths() {
    for negative in [false, true] {
        let pattern = "(data ?value&:(local-positive ?value))";
        let lhs = if negative {
            format!("(not {pattern})")
        } else {
            pattern.into()
        };
        let mut engine = setup(&format!("(defrule width {lhs} => (printout t fired crlf))"));
        engine.assert_ordered("data", Vec::<i64>::new()).unwrap();
        // The zero would raise an error if the excess-width fact reached the callback.
        engine.assert_ordered("data", [0, 99]).unwrap();
        assert_eq!(calls(&engine), 0);
        assert!(engine.action_diagnostics().is_empty());
        run(&mut engine, usize::from(negative));
        let valid = engine.assert_ordered("data", [2]).unwrap();
        assert_eq!(calls(&engine), 1);
        run(&mut engine, usize::from(!negative));
        engine.retract(valid).unwrap();
        run(&mut engine, usize::from(negative));
        assert_eq!(calls(&engine), 1);
    }
}

#[test]
fn lazy_negative_and_exists_join_callbacks_require_exact_width_before_selection() {
    for wrapper in ["not", "exists"] {
        let mut engine = setup(&format!(
            "(defrule width (anchor ?limit) ({wrapper} (data ?limit ?value&:(join-positive ?value ?limit))) => (printout t fired crlf))"
        ));
        engine.assert_ordered("data", [2]).unwrap();
        engine.assert_ordered("data", [2, 9, 99]).unwrap();
        engine.assert_ordered("anchor", [2]).unwrap();
        assert_eq!(calls(&engine), 0);
        run(&mut engine, usize::from(wrapper == "not"));
        let selected = engine.assert_ordered("data", [2, 9]).unwrap();
        assert_eq!(calls(&engine), 1);
        // Additional wrong-width candidate must not become replacement support.
        engine.assert_ordered("data", [2, 8, 88]).unwrap();
        run(&mut engine, usize::from(wrapper == "exists"));
        engine.retract(selected).unwrap();
        assert_eq!(calls(&engine), 1);
        run(&mut engine, usize::from(wrapper == "not"));
    }
}

#[test]
fn late_runtime_rule_backfill_filters_excess_width_before_evaluation() {
    let mut engine = Engine::new(EngineConfig::utf8());
    engine.load_str(PREFIX).unwrap();
    engine.reset().unwrap();
    engine.assert_ordered("data", Vec::<i64>::new()).unwrap();
    engine.assert_ordered("data", [0, 99]).unwrap();
    engine.assert_ordered("data", [2]).unwrap();
    engine
        .load_str("(defrule late (not (data ?v&:(local-positive ?v))) => (printout t fired crlf))")
        .unwrap();
    assert_eq!(calls(&engine), 1);
    assert!(engine.action_diagnostics().is_empty());
    run(&mut engine, 0);
}

#[cfg(feature = "serde")]
#[test]
fn every_codec_preserves_shape_filtered_memories_and_future_candidate_admission() {
    use ferric_rules_runtime::SerializationFormat;
    let mut pending = setup("(defrule width (anchor ?limit) (not (data ?limit ?v&:(join-positive ?v ?limit))) => (printout t fired crlf))");
    pending.assert_ordered("anchor", [2]).unwrap();
    pending.assert_ordered("data", [2, 9, 99]).unwrap();
    assert_eq!(calls(&pending), 0);
    for &format in SerializationFormat::ALL {
        let mut restored =
            Engine::deserialize(&pending.serialize(format).unwrap(), format).unwrap();
        run(&mut restored, 1);
        restored.assert_ordered("data", [2]).unwrap();
        restored.assert_ordered("data", [2, 8, 88]).unwrap();
        assert_eq!(calls(&restored), 0);
        let good = restored.assert_ordered("data", [2, 9]).unwrap();
        assert_eq!(calls(&restored), 1);
        let completed = restored.serialize(format).unwrap();
        let mut completed = Engine::deserialize(&completed, format).unwrap();
        // Resolve a current-engine handle after restore, never reuse `good`.
        let current = completed
            .find_facts("data")
            .unwrap()
            .into_iter()
            .find_map(|(id, fact)| {
                let ferric_rules_core::Fact::Ordered(ordered) = fact else {
                    return None;
                };
                (ordered.fields.len() == 2).then_some(id)
            })
            .unwrap();
        completed.retract(current).unwrap();
        assert_eq!(calls(&completed), 1);
        run(&mut completed, 1);
        // The live source-engine handle remains valid only for its own engine.
        restored.retract(good).unwrap();
    }
}
