//! Ferric's run boundaries inside a query body. `reset` and `clear` stop the
//! query and the rest of the RHS; CLIPS instead continues (and `reset` in a
//! `do-for-all-facts` body can loop forever there), so there is no CLIPS
//! golden for these.

use ferric_rules_core::{Fact, Value};
use ferric_rules_runtime::{Engine, EngineConfig, HaltReason, RunLimit};

fn item_values(engine: &Engine) -> Vec<i64> {
    let mut values: Vec<_> = engine
        .facts()
        .unwrap()
        .filter_map(|(_, fact)| match fact {
            Fact::Template(item) => match item.slots.first() {
                Some(Value::Integer(value)) => Some(*value),
                _ => None,
            },
            Fact::Ordered(_) => None,
        })
        .collect();
    values.sort_unstable();
    values
}

#[test]
fn reset_and_clear_inside_a_query_body_end_the_rhs() {
    for (action, values, rules) in [("reset", &[10, 20][..], 1), ("clear", &[][..], 0)] {
        let mut engine = Engine::new(EngineConfig::utf8());
        engine
            .load_str(&format!(
                "(deftemplate item (slot value))
                 (deffacts seed (item (value 10)) (item (value 20)))
                 (defrule probe =>
                   (do-for-all-facts ((?f item)) TRUE
                     (retract ?f)
                     ({action})
                     (printout t \"inside-after\" crlf))
                   (printout t \"outside-after\" crlf))"
            ))
            .unwrap();
        engine.reset().unwrap();
        let result = engine.run(RunLimit::Count(10)).unwrap();
        assert_eq!(result.halt_reason, HaltReason::HaltRequested, "{action}");
        assert_eq!(result.rules_fired, 1, "{action}");
        assert!(engine.action_diagnostics().is_empty(), "{action}");
        assert_eq!(engine.get_output("t").unwrap_or(""), "", "{action}");
        assert_eq!(item_values(&engine), values, "{action}");
        assert_eq!(engine.rules().len(), rules, "{action}");
    }
}
