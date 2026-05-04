//! Users-guide example 02: ordered facts vs. template facts.
//!
//! Source: docs/users-guide.md §3.

use ferric::core::Value;
use ferric::runtime::{Engine, RunLimit};

fn assert_person(engine: &mut Engine, name: &str, age: i64) -> anyhow::Result<()> {
    let name_sym = engine.symbol_value(name)?;
    engine.assert_template(
        "person",
        &["name", "age"],
        vec![name_sym, Value::Integer(age)],
    )?;
    Ok(())
}

fn main() -> anyhow::Result<()> {
    let rules = include_str!("../rules/people.clp");
    let mut engine = Engine::with_rules(rules)?;

    assert_person(&mut engine, "Alice", 30)?;
    assert_person(&mut engine, "Bob", 12)?;

    engine.run(RunLimit::Unlimited)?;

    let output = engine.get_output("t").unwrap_or("");
    assert!(output.contains("Alice is an adult"));
    assert!(!output.contains("Bob is an adult"));

    print!("{output}");
    Ok(())
}
