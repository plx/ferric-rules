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
