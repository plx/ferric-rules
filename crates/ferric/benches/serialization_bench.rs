#![cfg(feature = "serde")]

use std::fmt::Write as FmtWrite;

use criterion::{criterion_group, criterion_main, Criterion};
use ferric::runtime::serialization::SerializationFormat;
use ferric::runtime::{Engine, EngineConfig, RunLimit};

/// Engine serialization/deserialization benchmark.
///
/// Measures `serialize()`/`deserialize()` round-trip latency at varying engine
/// sizes. Also benchmarks compilation as a baseline to validate that
/// deserialization is faster than full compilation.
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
