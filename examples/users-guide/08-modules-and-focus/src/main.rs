//! Users-guide example 08: modules and the focus stack.
//!
//! Source: docs/users-guide.md §9.
//!
//! Both modules match the same `(reading ...)` template, but only the
//! module on top of the focus stack is eligible to fire at any moment.
//! `push_focus` controls the order: ALERTS first (bottom), SENSORS second
//! (top), so SENSORS fires first, then ALERTS once SENSORS' agenda drains.

use ferric::core::Value;
use ferric::runtime::{Engine, RunLimit};

fn main() -> anyhow::Result<()> {
    let rules = include_str!("../rules/pipeline.clp");
    let mut engine = Engine::with_rules(rules)?;

    let kind = engine.symbol_value("temperature")?;
    engine.assert_template(
        "reading",
        &["kind", "value"],
        vec![kind, Value::Float(120.0)],
    )?;

    engine.push_focus("ALERTS")?;
    engine.push_focus("SENSORS")?;
    engine.run(RunLimit::Unlimited)?;

    let output = engine.get_output("t").unwrap_or("");
    print!("{output}");

    let sensors_first = output.find("SENSORS observed").unwrap_or(usize::MAX);
    let alerts_second = output.find("ALERTS: high").unwrap_or(0);
    assert!(
        sensors_first < alerts_second,
        "SENSORS should fire before ALERTS — focus pushed last is on top"
    );
    println!("finished in module {:?}", engine.get_focus());
    Ok(())
}
