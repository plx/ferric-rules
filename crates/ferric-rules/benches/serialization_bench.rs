#![cfg(feature = "serde")]

mod support;

use std::fmt::Write as FmtWrite;

use criterion::{criterion_group, criterion_main, Criterion};
use ferric_rules::runtime::serialization::SerializationFormat;
use ferric_rules::runtime::{Engine, EngineConfig, RunLimit};

/// Engine serialization/deserialization benchmark.
///
/// Measures `serialize()`/`deserialize()` round-trip latency at varying engine
/// sizes. Compilation is a separate comparison workload; no relative speed is
/// assumed. The generated distinct keys intentionally leave joins unmatched.
/// The untimed oracle checks stored facts and completes a join after restore.
fn validate_snapshot(
    engine: &Engine,
    bytes: &[u8],
    format: SerializationFormat,
    n_templates: usize,
    n_rules: usize,
    n_facts: usize,
) {
    let mut restored = Engine::deserialize(bytes, format).unwrap();
    for state in [engine, &restored] {
        let mut seen = std::collections::BTreeSet::new();
        for template in 0..n_templates {
            for id in support::template_ids(state, &format!("t{template}")) {
                let slot =
                    |name| support::symbol(state, state.get_fact_slot_by_name(id, name).unwrap());
                let i: usize = slot("s0").strip_prefix('v').unwrap().parse().unwrap();
                assert!(i < n_facts);
                assert_eq!(i % n_templates, template);
                assert!(seen.insert(i));
                assert_eq!(slot("s1"), format!("a{}", i % 10));
                assert_eq!(slot("s2"), format!("b{}", i % 7));
                assert_eq!(slot("s3"), format!("c{}", i % 5));
            }
        }
        assert_eq!(seen.len(), n_facts);
    }
    support::verify_run(&mut restored, 0);
    restored
        .load_str("(assert (t1 (s0 v0) (s2 probe)))")
        .unwrap();
    support::verify_run(
        &mut restored,
        (0..n_rules).filter(|r| r % n_templates == 0).count(),
    );
    for rule in 0..n_rules {
        let facts = restored.find_facts(&format!("result-{rule}")).unwrap();
        if rule % n_templates == 0 {
            assert_eq!(facts.len(), 1);
            let ferric_rules::core::Fact::Ordered(fact) = facts[0].1 else {
                unreachable!()
            };
            assert_eq!(
                fact.fields
                    .iter()
                    .map(|value| support::symbol(&restored, value))
                    .collect::<Vec<_>>(),
                ["v0", "a0", "probe"]
            );
        } else {
            assert!(facts.is_empty());
        }
    }
}
fn generate_serde_source(n_templates: usize, n_rules: usize, n_facts: usize) -> String {
    let mut source = String::new();

    // Generate templates with 4 slots each
    for t in 0..n_templates {
        writeln!(
            source,
            "(deftemplate t{t} (slot s0) (slot s1) (slot s2) (slot s3))"
        )
        .unwrap();
    }
    source.push('\n');

    // Generate rules joining consecutive templates on s0
    for r in 0..n_rules {
        let t1 = r % n_templates;
        let t2 = (r + 1) % n_templates;
        writeln!(
            source,
            "\
(defrule rule-{r}
    (t{t1} (s0 ?x) (s1 ?y))
    (t{t2} (s0 ?x) (s2 ?z))
    =>
    (assert (result-{r} ?x ?y ?z)))\n"
        )
        .unwrap();
    }

    // Generate facts cycling across templates with unique s0 values
    source.push_str("(deffacts data\n");
    for i in 0..n_facts {
        let t = i % n_templates;
        writeln!(
            source,
            "    (t{t} (s0 v{i}) (s1 a{}) (s2 b{}) (s3 c{}))",
            i % 10,
            i % 7,
            i % 5
        )
        .unwrap();
    }
    source.push_str(")\n");
    source
}

fn bench_serde_small(c: &mut Criterion) {
    let source = generate_serde_source(5, 10, 50);
    let format = SerializationFormat::Bincode;

    // Prepare engine state
    let mut engine = Engine::new(EngineConfig::utf8());
    engine.load_str(&source).unwrap();
    engine.reset().unwrap();
    engine.run(RunLimit::Unlimited).unwrap();
    let bytes = engine.serialize(format).unwrap();
    validate_snapshot(&engine, &bytes, format, 5, 10, 50);

    let mut group = c.benchmark_group("serde_small");

    group.bench_function("serialize", |b| {
        b.iter(|| engine.serialize(format).unwrap());
    });

    group.bench_function("deserialize", |b| {
        b.iter(|| Engine::deserialize(&bytes, format).unwrap());
    });

    let source_clone = source.clone();
    group.bench_function("compile_baseline", |b| {
        b.iter(|| {
            let mut e = Engine::new(EngineConfig::utf8());
            e.load_str(&source_clone).unwrap();
            e.reset().unwrap();
        });
    });

    group.finish();
}

fn bench_serde_medium(c: &mut Criterion) {
    let source = generate_serde_source(20, 100, 500);
    let format = SerializationFormat::Bincode;

    let mut engine = Engine::new(EngineConfig::utf8());
    engine.load_str(&source).unwrap();
    engine.reset().unwrap();
    engine.run(RunLimit::Unlimited).unwrap();
    let bytes = engine.serialize(format).unwrap();
    validate_snapshot(&engine, &bytes, format, 20, 100, 500);

    let mut group = c.benchmark_group("serde_medium");
    group.sample_size(10);

    group.bench_function("serialize", |b| {
        b.iter(|| engine.serialize(format).unwrap());
    });

    group.bench_function("deserialize", |b| {
        b.iter(|| Engine::deserialize(&bytes, format).unwrap());
    });

    let source_clone = source.clone();
    group.bench_function("compile_baseline", |b| {
        b.iter(|| {
            let mut e = Engine::new(EngineConfig::utf8());
            e.load_str(&source_clone).unwrap();
            e.reset().unwrap();
        });
    });

    group.finish();
}

fn bench_serde_large(c: &mut Criterion) {
    let source = generate_serde_source(50, 500, 2000);
    let format = SerializationFormat::Bincode;

    let mut engine = Engine::new(EngineConfig::utf8());
    engine.load_str(&source).unwrap();
    engine.reset().unwrap();
    engine.run(RunLimit::Unlimited).unwrap();
    let bytes = engine.serialize(format).unwrap();
    validate_snapshot(&engine, &bytes, format, 50, 500, 2000);

    let mut group = c.benchmark_group("serde_large");
    group.sample_size(10);

    group.bench_function("serialize", |b| {
        b.iter(|| engine.serialize(format).unwrap());
    });

    group.bench_function("deserialize", |b| {
        b.iter(|| Engine::deserialize(&bytes, format).unwrap());
    });

    let source_clone = source.clone();
    group.bench_function("compile_baseline", |b| {
        b.iter(|| {
            let mut e = Engine::new(EngineConfig::utf8());
            e.load_str(&source_clone).unwrap();
            e.reset().unwrap();
        });
    });

    group.finish();
}

criterion_group!(
    benches,
    bench_serde_small,
    bench_serde_medium,
    bench_serde_large,
);
criterion_main!(benches);
