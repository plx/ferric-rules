//! Users-guide example 12: error handling.
//!
//! Source: docs/users-guide.md §13.
//!
//! Demonstrates the two categories the guide describes:
//!
//! 1. Fatal errors — `Engine::with_rules` returning `InitError` on parse
//!    failure.
//! 2. Non-fatal action diagnostics — surface as warnings after a `run`,
//!    then clear on the next `run`, `step`, `reset`, or explicit clear.

use ferric::runtime::{Engine, RunLimit};

fn demo_init_error() {
    let bogus = "(defrule oops "; // unbalanced — parser should reject
    match Engine::with_rules(bogus) {
        Ok(_) => panic!("expected an InitError"),
        Err(e) => println!("init error (as expected): {e}"),
    }
}

fn demo_action_diagnostics() -> anyhow::Result<()> {
    let rules = include_str!("../rules/diagnostics.clp");
    let mut engine = Engine::with_rules(rules)?;
    engine.assert_ordered("begin", vec![])?;

    let _ = engine.run(RunLimit::Unlimited)?;

    let diags = engine.action_diagnostics();
    println!("action diagnostics after run: {}", diags.len());
    for diag in diags {
        println!("  - {diag:?}");
    }
    assert!(
        !diags.is_empty(),
        "expected at least one diagnostic from the unresolved focus action"
    );

    engine.clear_action_diagnostics();
    assert!(engine.action_diagnostics().is_empty());
    Ok(())
}

fn main() -> anyhow::Result<()> {
    demo_init_error();
    demo_action_diagnostics()?;
    Ok(())
}
