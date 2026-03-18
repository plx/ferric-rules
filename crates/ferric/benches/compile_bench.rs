use std::fmt::Write as FmtWrite;

use criterion::{criterion_group, criterion_main, Criterion};
use ferric::runtime::{Engine, EngineConfig};

/// Compilation scaling benchmark: cold-start parse + Rete network construction
/// for large rule bases.
///
/// Measures `load_str` only (no reset/run). Rete sharing decisions involve
/// comparing existing alpha/beta nodes, which is potentially `O(existing_nodes)`
/// per new pattern if not indexed.
fn generate_compile_source(n_rules: usize, n_templates: usize) -> String {
    let mut source = String::new();

    // Generate T templates, each with 4 slots
    for t in 0..n_templates {
        writeln!(
            source,
            "(deftemplate t{t} (slot s0) (slot s1) (slot s2) (slot s3))"
        )
        .unwrap();
    }
    source.push('\n');

    // Generate R rules, each joining two consecutive templates on s0
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
    source
}

fn bench_compile_10r_5t(c: &mut Criterion) {
    let source = generate_compile_source(10, 5);
    c.bench_function("compile_10r_5t", |b| {
        b.iter(|| {
            let mut engine = Engine::new(EngineConfig::utf8());
            engine.load_str(&source).unwrap();
        });
    });
}

fn bench_compile_50r_10t(c: &mut Criterion) {
    let source = generate_compile_source(50, 10);
    c.bench_function("compile_50r_10t", |b| {
        b.iter(|| {
            let mut engine = Engine::new(EngineConfig::utf8());
            engine.load_str(&source).unwrap();
        });
    });
}

fn bench_compile_100r_20t(c: &mut Criterion) {
    let source = generate_compile_source(100, 20);
    let mut group = c.benchmark_group("compile_100r_20t");
    group.sample_size(10);
    group.bench_function("compile_100r_20t", |b| {
        b.iter(|| {
            let mut engine = Engine::new(EngineConfig::utf8());
            engine.load_str(&source).unwrap();
        });
    });
    group.finish();
}

fn bench_compile_200r_30t(c: &mut Criterion) {
    let source = generate_compile_source(200, 30);
    let mut group = c.benchmark_group("compile_200r_30t");
    group.sample_size(10);
    group.bench_function("compile_200r_30t", |b| {
        b.iter(|| {
            let mut engine = Engine::new(EngineConfig::utf8());
            engine.load_str(&source).unwrap();
        });
    });
    group.finish();
}

fn bench_compile_500r_50t(c: &mut Criterion) {
    let source = generate_compile_source(500, 50);
    let mut group = c.benchmark_group("compile_500r_50t");
    group.sample_size(10);
    group.bench_function("compile_500r_50t", |b| {
        b.iter(|| {
            let mut engine = Engine::new(EngineConfig::utf8());
            engine.load_str(&source).unwrap();
        });
    });
    group.finish();
}

criterion_group!(
    benches,
    bench_compile_10r_5t,
    bench_compile_50r_10t,
    bench_compile_100r_20t,
    bench_compile_200r_30t,
    bench_compile_500r_50t,
);
criterion_main!(benches);
