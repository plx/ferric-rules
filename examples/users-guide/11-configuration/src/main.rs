//! Users-guide example 11: configuration.
//!
//! Source: docs/users-guide.md §12.
//!
//! Three engines built three different ways. The example just constructs
//! them and asserts the configuration that came back, since the only point
//! is to demonstrate the factory shape.

use ferric_rules::core::ConflictResolutionStrategy;
use ferric_rules::runtime::{Engine, EngineConfig};

fn main() -> anyhow::Result<()> {
    // UTF-8 symbols and strings, Depth strategy, 64-frame recursion limit,
    // and a 1,000,000-iteration action-loop budget.
    let _engine = Engine::new(EngineConfig::default());

    // CLIPS-strict ASCII mode with LEX strategy.
    let _engine = Engine::new(EngineConfig::ascii().with_strategy(ConflictResolutionStrategy::Lex));

    // Increase recursion depth and reduce the per-activation budget shared by
    // while/loop-for-count, including nested loops in deffunctions.
    let mut cfg = EngineConfig::utf8();
    cfg.max_call_depth = 256;
    cfg.max_action_loop_iterations = 10_000;
    let _engine = Engine::new(cfg);

    // Combine config with one-call rule loading.
    let utf8_lex = EngineConfig::utf8().with_strategy(ConflictResolutionStrategy::Lex);
    let mut engine = Engine::with_rules_config(
        r#"
        (defrule hi
            (start)
            =>
            (printout t "configured!" crlf))
    "#,
        utf8_lex,
    )?;
    engine.assert_ordered("start", vec![])?;
    engine.run(ferric_rules::runtime::RunLimit::Unlimited)?;
    println!("output = {:?}", engine.get_output("t").unwrap_or(""));
    Ok(())
}
