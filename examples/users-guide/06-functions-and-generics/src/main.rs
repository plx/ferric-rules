//! Users-guide example 06: user functions and generics.
//!
//! Source: docs/users-guide.md §7.

use ferric::core::Value;
use ferric::runtime::{Engine, RunLimit};

fn run_temp() -> anyhow::Result<()> {
    let rules = include_str!("../rules/temp.clp");
    let mut engine = Engine::with_rules(rules)?;

    let kind = engine.symbol_value("celsius")?;
    engine.assert_ordered("reading", vec![kind, Value::Float(20.0)])?;
    engine.run(RunLimit::Unlimited)?;

    let output = engine.get_output("t").unwrap_or("");
    assert!(output.contains("20"));
    assert!(output.contains("68"));
    print!("{output}");
    Ok(())
}

fn run_generics() -> anyhow::Result<()> {
    let rules = include_str!("../rules/describe.clp");
    let mut engine = Engine::with_rules(rules)?;

    engine.assert_ordered("value", Value::Integer(7))?;
    engine.assert_ordered("value", Value::Float(2.5))?;
    engine.run(RunLimit::Unlimited)?;

    let output = engine.get_output("t").unwrap_or("");
    assert!(output.contains("int/number(7)"));
    assert!(output.contains("number(2.5)"));
    print!("{output}");
    Ok(())
}

fn main() -> anyhow::Result<()> {
    println!("-- deffunction --");
    run_temp()?;
    println!("-- defgeneric / defmethod --");
    run_generics()?;
    Ok(())
}
