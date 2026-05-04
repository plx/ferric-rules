//! Users-guide example 13: snapshots and warm starts.
//!
//! Source: docs/users-guide.md §14.
//!
//! Compile a small ruleset once, freeze the engine to bytes, then thaw
//! a fresh engine from the bytes and run it without re-parsing.

use ferric::runtime::{Engine, RunLimit, SerializationFormat};

fn main() -> anyhow::Result<()> {
    let rules = include_str!("../rules/rules.clp");

    // Offline: compile once, save a baseline snapshot.
    let engine = Engine::with_rules(rules)?;
    let bytes = engine.serialize(SerializationFormat::Bincode)?;
    println!("snapshot size: {} bytes", bytes.len());

    // Online: fast path — no parsing, no compilation.
    let mut engine = Engine::deserialize(&bytes, SerializationFormat::Bincode)?;
    engine.assert_ordered("reading", 7_i64)?;
    engine.run(RunLimit::Unlimited)?;

    let output = engine.get_output("t").unwrap_or("");
    print!("{output}");
    assert!(output.contains("saw 7"));
    assert!(output.contains("threshold=42"));
    Ok(())
}
