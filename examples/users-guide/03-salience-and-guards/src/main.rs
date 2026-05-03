//! Users-guide example 03: salience + guard facts.
//!
//! Source: docs/users-guide.md §4.

use ferric::core::Value;
use ferric::runtime::{Engine, RunLimit};

fn classify(engine: &mut Engine, smoke: &str, temperature: f64) -> anyhow::Result<Option<String>> {
    engine.reset()?;

    let smoke_kind = engine.symbol_value("smoke")?;
    let smoke_level = engine.symbol_value(smoke)?;
    engine.assert_ordered("sensor", vec![smoke_kind, smoke_level])?;

    let temp_kind = engine.symbol_value("temperature")?;
    engine.assert_ordered("sensor", vec![temp_kind, Value::Float(temperature)])?;

    engine.run(RunLimit::Unlimited)?;

    for (_, fact) in engine.find_facts("alert")? {
        if let ferric::core::Fact::Ordered(of) = fact {
            if let Some(Value::Symbol(sym)) = of.fields.first() {
                if let Some(name) = engine.resolve_symbol(*sym) {
                    return Ok(Some(name.to_string()));
                }
            }
        }
    }
    Ok(None)
}

fn main() -> anyhow::Result<()> {
    let rules = include_str!("../rules/alerts.clp");
    let mut engine = Engine::with_rules(rules)?;

    let fire = classify(&mut engine, "high", 80.0)?;
    let heat = classify(&mut engine, "low", 95.0)?;
    let calm = classify(&mut engine, "low", 70.0)?;

    assert_eq!(fire.as_deref(), Some("evacuate"));
    assert_eq!(heat.as_deref(), Some("high-temp"));
    assert_eq!(calm.as_deref(), Some("none"));

    println!("smoke=high  temp=80 → {fire:?}");
    println!("smoke=low   temp=95 → {heat:?}");
    println!("smoke=low   temp=70 → {calm:?}");
    Ok(())
}
