use std::fmt::Write as FmtWrite;

use criterion::{criterion_group, criterion_main, Criterion};
use ferric::runtime::{Engine, EngineConfig, RunLimit};

/// Fact query action benchmark: `do-for-all-facts` performance scanning
/// large working memories.
///
/// These actions iterate over the fact base linearly. A rule that fires C
/// times with N total facts costs O(C*N) — potentially quadratic in the
/// overall workload.
fn generate_query_source(n_items: usize, n_categories: usize) -> String {
    let mut source = String::from(
        "\
(deftemplate item (slot category) (slot value))
(deftemplate summary (slot category) (slot total))
(deftemplate category-marker (slot name))

(deffacts categories\n",
    );

    for i in 0..n_categories {
        writeln!(source, "    (category-marker (name cat{i}))").unwrap();
    }

    source.push_str(")\n\n(deffacts items\n");
    for i in 0..n_items {
        let cat = i % n_categories;
        writeln!(source, "    (item (category cat{cat}) (value {i}))").unwrap();
    }

    source.push_str(
        ")

(defrule summarize
    (category-marker (name ?cat))
    (not (summary (category ?cat)))
    =>
    (bind ?count 0)
    (do-for-all-facts ((?i item)) (eq ?i:category ?cat)
        (bind ?count (+ ?count 1)))
    (assert (summary (category ?cat) (total ?count))))
",
    );
    source
}

fn bench_query_100i_10c(c: &mut Criterion) {
    let source = generate_query_source(100, 10);
    c.bench_function("query_100i_10c", |b| {
        b.iter(|| {
            let mut engine = Engine::new(EngineConfig::utf8());
            engine.load_str(&source).unwrap();
            engine.reset().unwrap();
            engine.run(RunLimit::Unlimited).unwrap()
        });
    });
}

fn bench_query_500i_20c(c: &mut Criterion) {
    let source = generate_query_source(500, 20);
    let mut group = c.benchmark_group("query_500i_20c");
    group.sample_size(10);
    group.bench_function("query_500i_20c", |b| {
        b.iter(|| {
            let mut engine = Engine::new(EngineConfig::utf8());
            engine.load_str(&source).unwrap();
            engine.reset().unwrap();
            engine.run(RunLimit::Unlimited).unwrap()
        });
    });
    group.finish();
}

fn bench_query_1000i_50c(c: &mut Criterion) {
    let source = generate_query_source(1000, 50);
    let mut group = c.benchmark_group("query_1000i_50c");
    group.sample_size(10);
    group.bench_function("query_1000i_50c", |b| {
        b.iter(|| {
            let mut engine = Engine::new(EngineConfig::utf8());
            engine.load_str(&source).unwrap();
            engine.reset().unwrap();
            engine.run(RunLimit::Unlimited).unwrap()
        });
    });
    group.finish();
}

fn bench_query_5000i_100c(c: &mut Criterion) {
    let source = generate_query_source(5000, 100);
    let mut group = c.benchmark_group("query_5000i_100c");
    group.sample_size(10);
    group.bench_function("query_5000i_100c", |b| {
        b.iter(|| {
            let mut engine = Engine::new(EngineConfig::utf8());
            engine.load_str(&source).unwrap();
            engine.reset().unwrap();
            engine.run(RunLimit::Unlimited).unwrap()
        });
    });
    group.finish();
}

fn bench_query_100i_10c_run_only(c: &mut Criterion) {
    let source = generate_query_source(100, 10);
    let mut engine = Engine::new(EngineConfig::utf8());
    engine.load_str(&source).unwrap();
    c.bench_function("query_100i_10c_run_only", |b| {
        b.iter(|| {
            engine.reset().unwrap();
            engine.run(RunLimit::Unlimited).unwrap()
        });
    });
}

criterion_group!(
    benches,
    bench_query_100i_10c,
    bench_query_500i_20c,
    bench_query_1000i_50c,
    bench_query_5000i_100c,
    bench_query_100i_10c_run_only,
);
criterion_main!(benches);
