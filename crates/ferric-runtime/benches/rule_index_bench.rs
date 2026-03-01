use std::fmt::Write as _;

use criterion::{black_box, criterion_group, criterion_main, Criterion};
use ferric_core::RuleId;
use ferric_runtime::{Engine, EngineConfig};

fn rule_source(rule_count: usize) -> String {
    let mut source = String::from("(deffacts startup)\n");
    for idx in 1..=rule_count {
        let _ = writeln!(
            source,
            "(defrule rule-{idx} (initial-fact) => (assert (seen {idx})))"
        );
    }
    source
}

fn loaded_engine(rule_count: usize) -> Engine {
    let source = rule_source(rule_count);
    let mut engine = Engine::new(EngineConfig::utf8());
    engine.load_str(&source).expect("load rules");
    engine
}

fn bench_rule_index_lookup(c: &mut Criterion) {
    let rule_count = 2048;
    let engine = loaded_engine(rule_count);
    let rule_ids: Vec<RuleId> = (1..=rule_count).map(|idx| RuleId(idx as u32)).collect();

    c.bench_function("engine_rule_name_lookup_cycle", |b| {
        b.iter(|| {
            let mut hits = 0usize;
            for _ in 0..8 {
                for &rule_id in &rule_ids {
                    if engine.rule_name(black_box(rule_id)).is_some() {
                        hits += 1;
                    }
                }
            }
            black_box(hits);
        });
    });

    c.bench_function("engine_rules_list", |b| {
        b.iter(|| black_box(engine.rules()));
    });
}

criterion_group!(benches, bench_rule_index_lookup);
criterion_main!(benches);
