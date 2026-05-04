//! Users-guide example 14: salience-phased reading pipeline.
//!
//! Source: docs/users-guide.md §15.

use ferric::core::{Fact, Value};
use ferric::runtime::{Engine, RunLimit};

fn run(engine: &mut Engine, inputs: &[(i64, &str, f64)]) -> anyhow::Result<()> {
    engine.reset()?;

    for (id, kind, value) in inputs {
        let kind_sym = engine.symbol_value(kind)?;
        engine.assert_template(
            "reading",
            &["id", "kind", "value"],
            vec![Value::Integer(*id), kind_sym, Value::Float(*value)],
        )?;
    }

    let result = engine.run(RunLimit::Count(10_000))?;
    println!("pipeline complete: {} rules fired", result.rules_fired);

    for (_, fact) in engine.find_facts("diagnosis")? {
        if let Fact::Ordered(of) = fact {
            print!("  diagnosis:");
            for field in &of.fields {
                match field {
                    Value::Integer(n) => print!(" {n}"),
                    Value::Symbol(sym) => {
                        if let Some(name) = engine.resolve_symbol(*sym) {
                            print!(" {name}");
                        }
                    }
                    Value::String(s) => print!(" {:?}", s.as_str()),
                    other => print!(" {other:?}"),
                }
            }
            println!();
        }
    }
    Ok(())
}

fn main() -> anyhow::Result<()> {
    let rules = include_str!("../rules/pipeline.clp");
    let mut engine = Engine::with_rules(rules)?;

    run(
        &mut engine,
        &[(1, "fahrenheit", 120.0), (2, "celsius", 20.0)],
    )?;

    let diagnoses = engine.find_facts("diagnosis")?.len();
    assert_eq!(diagnoses, 2, "expected one diagnosis per input reading");
    Ok(())
}
