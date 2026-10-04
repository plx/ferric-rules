//! Immediate lifecycle effects retain the active RHS and its lexical frames.

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
fn reset_and_refused_clear_continue_the_query_body_and_rhs() {
    for (action, values) in [("reset", &[10, 20][..]), ("clear", &[][..])] {
        let mut engine = Engine::new(EngineConfig::utf8());
        engine
            .load_str(&format!(
                "(deftemplate item (slot value))
             (deffacts seed (item (value 10)) (item (value 20)))
             (defrule probe =>
               (printout t before crlf)
               (do-for-all-facts ((?f item)) TRUE
                 (retract ?f)
                 ({action})
                 (printout t \"inside-after:\" ?f:value crlf))
               (printout t \"outside-after\" crlf)
               (halt))"
            ))
            .unwrap();
        engine.reset().unwrap();
        let result = engine.run(RunLimit::Count(10)).unwrap();
        assert_eq!(result.halt_reason, HaltReason::HaltRequested, "{action}");
        assert_eq!(result.rules_fired, 1, "{action}");
        assert!(engine.action_diagnostics().is_empty(), "{action}");
        assert_eq!(
            engine.get_output("t"),
            Some("before\ninside-after:10\noutside-after\n"),
            "{action}"
        );
        assert_eq!(item_values(&engine), values, "{action}");
        assert_eq!(engine.rules().len(), 1, "{action}");
    }
}
