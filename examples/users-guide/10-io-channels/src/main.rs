//! Users-guide example 10: input and output channels.
//!
//! Source: docs/users-guide.md §11.

use ferric::runtime::{Engine, RunLimit};

fn main() -> anyhow::Result<()> {
    let rules = include_str!("../rules/io.clp");
    let mut engine = Engine::with_rules(rules)?;

    engine.push_input("hello world");
    engine.assert_ordered("prompt-line", vec![])?;
    engine.run(RunLimit::Unlimited)?;

    let output = engine.get_output("t").unwrap_or("");
    print!("{output}");

    assert!(output.contains("n=42"));
    assert!(output.contains("got: hello world"));
    Ok(())
}
