//! Users-guide example 01: a minimal embedding of ferric-rules.
//!
//! Source: docs/users-guide.md §2.

use ferric::runtime::{Engine, RunLimit};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut engine = Engine::with_rules(
        r#"
        (defrule greet
            (user ?name)
            =>
            (printout t "Hello, " ?name "!" crlf))
    "#,
    )?;

    engine.assert_ordered_symbol("user", "Alice")?;
    engine.run(RunLimit::Unlimited)?;

    assert_eq!(engine.get_output("t"), Some("Hello, Alice!\n"));
    Ok(())
}
