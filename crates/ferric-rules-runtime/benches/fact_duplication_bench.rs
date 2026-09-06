//! High-volume assertion benchmarks for both fact-duplication policies.

use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion};
use ferric_rules_runtime::{Engine, EngineConfig};

const ASSERTION_COUNT: usize = 10_000;

fn benchmark_fact_duplication_policy(c: &mut Criterion) {
    let mut group = c.benchmark_group("fact_assertion/duplication_policy");

    for enabled in [false, true] {
        let label = if enabled { "enabled" } else { "disabled" };
        group.bench_with_input(
            BenchmarkId::new(label, ASSERTION_COUNT),
            &enabled,
            |b, &enabled| {
                let mut oracle =
                    Engine::new(EngineConfig::default().with_fact_duplication(enabled));
                for value in 0..ASSERTION_COUNT {
                    oracle
                        .assert_ordered("item", i64::try_from(value).unwrap())
                        .unwrap();
                }
                let mut values = oracle
                    .find_facts("item")
                    .unwrap()
                    .iter()
                    .map(|(_, fact)| {
                        let ferric_rules_core::Fact::Ordered(fact) = fact else {
                            unreachable!()
                        };
                        let ferric_rules_core::Value::Integer(value) = fact.fields[0] else {
                            panic!("expected integer item")
                        };
                        value
                    })
                    .collect::<Vec<_>>();
                values.sort_unstable();
                assert_eq!(
                    values,
                    (0..i64::try_from(ASSERTION_COUNT).unwrap()).collect::<Vec<_>>()
                );
                oracle.assert_ordered("item", 0_i64).unwrap();
                assert_eq!(
                    oracle.find_facts("item").unwrap().len(),
                    ASSERTION_COUNT + usize::from(enabled)
                );
                b.iter(|| {
                    let config = EngineConfig::default().with_fact_duplication(enabled);
                    let mut engine = Engine::new(config);
                    for value in 0..ASSERTION_COUNT {
                        #[allow(clippy::cast_possible_wrap)]
                        engine
                            .assert_ordered("item", black_box(value as i64))
                            .unwrap();
                    }
                    black_box(engine);
                });
            },
        );
    }

    group.finish();
}

criterion_group!(benches, benchmark_fact_duplication_policy);
criterion_main!(benches);
