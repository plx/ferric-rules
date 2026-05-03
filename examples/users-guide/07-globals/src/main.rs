//! Users-guide example 07: defglobal.
//!
//! Source: docs/users-guide.md §8.
//!
//! Note: `engine.get_global` takes the bare global name (no surrounding
//! stars). The CLIPS-side syntax `?*session-count*` declares a global
//! whose host-side name is `session-count`.

use ferric::core::Value;
use ferric::runtime::{Engine, RunLimit};

fn main() -> anyhow::Result<()> {
    let rules = include_str!("../rules/sessions.clp");
    let mut engine = Engine::with_rules(rules)?;

    for _ in 0..3 {
        engine.assert_ordered("session-start", vec![])?;
        engine.run(RunLimit::Unlimited)?;
    }

    if let Some(Value::Integer(n)) = engine.get_global("session-count") {
        println!("engine has counted {n} sessions");
        assert_eq!(*n, 3);
    } else {
        panic!("expected session-count to be an Integer");
    }

    print!("{}", engine.get_output("t").unwrap_or(""));
    Ok(())
}
