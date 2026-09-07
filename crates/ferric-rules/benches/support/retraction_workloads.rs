//! Retraction cost as the number of independent negative memories grows.

use std::fmt::Write as _;

use criterion::{BenchmarkId, Criterion};
use ferric_rules::runtime::{Engine, EngineConfig, RunLimit};

use super::support;

fn source(kind: &str, rules: usize) -> String {
    let mut source = String::new();
    for group in 0..rules {
        let condition = match kind {
            "negative" => format!("(not (support {group} ?key))"),
            "ncc" => format!("(not (and (support {group} ?key) (extra {group} ?key)))"),
            "exists" => format!("(exists (support {group} ?key))"),
            _ => unreachable!(),
        };
        writeln!(
            source,
            "(defrule r-{group} (item {group} ?key) {condition} => (assert (seen {group})))"
        )
        .unwrap();
    }
    source.push_str("(deffacts seed\n");
    for group in 0..rules {
        writeln!(source, "(item {group} 0)").unwrap();
        if kind == "exists" {
            writeln!(source, "(support {group} 0)").unwrap();
        }
    }
    source.push_str(")\n");
    source
}

fn retract_items(engine: &mut Engine) {
    // A single indexed relation lookup, rather than a scan for each rule.
    let handles: Vec<_> = engine
        .find_facts("item")
        .unwrap()
        .into_iter()
        .map(|(id, _)| id)
        .collect();
    for handle in handles {
        engine.retract(handle).unwrap();
    }
}

pub(super) fn bench_unrelated_memories(c: &mut Criterion) {
    let mut group = c.benchmark_group("retract_independent_memories");
    for kind in ["negative", "ncc", "exists"] {
        for rules in [1, 32, 128, 512] {
            let source = source(kind, rules);
            group.bench_with_input(BenchmarkId::new(kind, rules), &source, |b, source| {
                let mut engine = Engine::new(EngineConfig::utf8());
                engine.load_str(source).unwrap();
                engine.reset().unwrap();
                support::verify_run(&mut engine, rules);
                assert_eq!(
                    support::ordered_integers(&engine, "seen"),
                    (0..rules)
                        .map(|i| i64::try_from(i).unwrap())
                        .collect::<Vec<_>>()
                );
                retract_items(&mut engine);
                support::verify_run(&mut engine, 0);
                assert!(engine.find_facts("item").unwrap().is_empty());
                engine.rete().validate_consistency().unwrap();
                engine.assert_ordered("item", [0_i64, 0_i64]).unwrap();
                support::verify_run(&mut engine, 1);
                engine.rete().validate_consistency().unwrap();
                b.iter(|| {
                    engine.reset().unwrap();
                    retract_items(&mut engine);
                    engine.run(RunLimit::Unlimited).unwrap()
                });
            });
        }
    }
    group.finish();
}
