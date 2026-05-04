//! Users-guide example 05: RHS actions — modify, retract.
//!
//! Source: docs/users-guide.md §6.

use ferric::core::{Fact, Value};
use ferric::runtime::{Engine, RunLimit};

fn main() -> anyhow::Result<()> {
    let rules = include_str!("../rules/counter.clp");
    let mut engine = Engine::with_rules(rules)?;

    engine.assert_template("counter", &[], vec![])?;
    for _ in 0..5 {
        engine.assert_ordered("tick", vec![])?;
    }

    engine.run(RunLimit::Unlimited)?;

    // Template facts aren't returned by `find_facts` (which is ordered-only).
    // Iterate `engine.facts()` and identify templates by name.
    let mut value: Option<i64> = None;
    for (id, fact) in engine.facts()? {
        if let Fact::Template(t) = fact {
            if engine.template_name_by_id(t.template_id) == Some("counter") {
                if let Ok(Value::Integer(n)) = engine.get_fact_slot_by_name(id, "value") {
                    value = Some(*n);
                }
            }
        }
    }

    let count = value.expect("counter fact should still be present");
    assert_eq!(count, 5, "five ticks should drive the counter to 5");
    println!("counter advanced to {count}");

    let remaining_ticks = engine.find_facts("tick")?.len();
    assert_eq!(remaining_ticks, 0, "all tick facts should be retracted");
    println!("tick facts remaining: {remaining_ticks}");
    Ok(())
}
