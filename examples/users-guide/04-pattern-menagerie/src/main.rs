//! Users-guide example 04: the pattern menagerie.
//!
//! Source: docs/users-guide.md §5.
//!
//! Each scenario asserts a different mix of facts so that the printouts
//! show which rule fired. Reset between scenarios keeps them independent.

use ferric::core::Value;
use ferric::runtime::{Engine, RunLimit};

fn run_scenario(
    engine: &mut Engine,
    label: &str,
    setup: impl FnOnce(&mut Engine) -> anyhow::Result<()>,
) -> anyhow::Result<()> {
    engine.reset()?;
    engine.clear_output_channel("t");
    setup(engine)?;
    engine.run(RunLimit::Unlimited)?;
    let output = engine.get_output("t").unwrap_or("").trim_end();
    println!("[{label}]\n{output}\n");
    Ok(())
}

fn main() -> anyhow::Result<()> {
    let rules = include_str!("../rules/menagerie.clp");
    let mut engine = Engine::with_rules(rules)?;

    run_scenario(&mut engine, "no tasks", |e| {
        e.assert_ordered("start", vec![])?;
        Ok(())
    })?;

    run_scenario(&mut engine, "work pending", |e| {
        let f = e.symbol_value("FALSE")?;
        e.assert_template("task", &["id", "done"], vec![Value::Integer(1), f.clone()])?;
        e.assert_template("task", &["id", "done"], vec![Value::Integer(2), f])?;
        Ok(())
    })?;

    run_scenario(&mut engine, "everything done", |e| {
        e.assert_ordered("ready", vec![])?;
        let t = e.symbol_value("TRUE")?;
        e.assert_template("task", &["id", "done"], vec![Value::Integer(1), t.clone()])?;
        e.assert_template("task", &["id", "done"], vec![Value::Integer(2), t])?;
        Ok(())
    })?;

    run_scenario(&mut engine, "ticket needs attention", |e| {
        let high = e.symbol_value("high")?;
        let open = e.symbol_value("open")?;
        e.assert_template("ticket", &["severity", "status"], vec![high, open])?;
        Ok(())
    })?;

    Ok(())
}
