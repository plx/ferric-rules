//! Untimed correctness checks shared by the engine benchmarks.
//!
//! Call these inside `bench_function`, before `b.iter`, so filtered-out large
//! workloads are not run. Expected results come from each workload's model,
//! never from recording an earlier engine result.
#![allow(dead_code)]

use ferric_rules::core::{Fact, Value};
use ferric_rules::runtime::{Engine, EngineConfig, FactHandle as FactId, HaltReason, RunLimit};

pub fn verify_source(source: &str, expected_firings: usize) -> Engine {
    let mut engine = Engine::new(EngineConfig::utf8());
    engine.load_str(source).unwrap();
    verify_reset_run(&mut engine, expected_firings);
    engine
}

pub fn verify_reset_run(engine: &mut Engine, expected_firings: usize) {
    engine.reset().unwrap();
    verify_run(engine, expected_firings);
}

pub fn verify_run(engine: &mut Engine, expected_firings: usize) {
    let result = engine.run(RunLimit::Unlimited).unwrap();
    assert_eq!(
        result.halt_reason,
        HaltReason::AgendaEmpty,
        "action diagnostics: {:?}",
        engine.action_diagnostics()
    );
    assert_eq!(result.rules_fired, expected_firings);
    let again = engine.run(RunLimit::Unlimited).unwrap();
    assert_eq!(again.halt_reason, HaltReason::AgendaEmpty);
    assert_eq!(
        again.rules_fired, 0,
        "completed workload must stay quiescent"
    );
}

pub fn template_ids(engine: &Engine, name: &str) -> Vec<FactId> {
    engine
        .facts()
        .unwrap()
        .filter_map(|(id, fact)| match fact {
            Fact::Template(fact) if engine.template_name_by_id(fact.template_id) == Some(name) => {
                Some(id)
            }
            _ => None,
        })
        .collect()
}

pub fn symbol<'a>(engine: &'a Engine, value: &Value) -> &'a str {
    let Value::Symbol(value) = value else {
        panic!("expected symbol, got {value:?}");
    };
    engine.resolve_core_symbol(*value).unwrap()
}

pub fn integer(value: &Value) -> i64 {
    let Value::Integer(value) = value else {
        panic!("expected integer, got {value:?}");
    };
    *value
}

pub fn ordered_integers(engine: &Engine, relation: &str) -> Vec<i64> {
    let mut result = engine
        .find_facts(relation)
        .unwrap()
        .into_iter()
        .map(|(_, fact)| {
            let Fact::Ordered(fact) = fact else {
                unreachable!()
            };
            assert_eq!(fact.fields.len(), 1);
            integer(&fact.fields[0])
        })
        .collect::<Vec<_>>();
    result.sort_unstable();
    result
}

pub fn ordered_symbols(engine: &Engine, relation: &str) -> Vec<String> {
    let mut result = engine
        .find_facts(relation)
        .unwrap()
        .into_iter()
        .map(|(_, fact)| {
            let Fact::Ordered(fact) = fact else {
                unreachable!()
            };
            assert_eq!(fact.fields.len(), 1);
            symbol(engine, &fact.fields[0]).to_owned()
        })
        .collect::<Vec<_>>();
    result.sort_unstable();
    result
}

pub fn expected_symbols(prefix: &str, count: usize) -> Vec<String> {
    let mut result = (0..count)
        .map(|i| format!("{prefix}{i}"))
        .collect::<Vec<_>>();
    result.sort_unstable();
    result
}

pub fn template_symbols(engine: &Engine, name: &str, slot: &str) -> Vec<String> {
    let mut result = template_ids(engine, name)
        .into_iter()
        .map(|id| symbol(engine, engine.get_fact_slot_by_name(id, slot).unwrap()).to_owned())
        .collect::<Vec<_>>();
    result.sort_unstable();
    result
}
