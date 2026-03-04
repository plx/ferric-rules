use std::fmt::Write as FmtWrite;

use criterion::{criterion_group, criterion_main, Criterion};
use ferric::runtime::{Engine, EngineConfig, RunLimit};

/// Fact assert/retract churn benchmark.
///
/// Scales the number of facts asserted, modified, and retracted to detect
/// per-operation overhead that grows with working memory size.  Each
/// benchmark creates N items with `status=pending`, then:
///
/// 1. A process rule (salience 10) modifies each item to `done`.
/// 2. A cleanup rule (salience 5) retracts each `done` item.
/// 3. A finish rule (salience -10) fires when no items remain.
///
/// Total: 2N+1 rule firings.  Each `modify` is internally retract+assert,
/// so the Rete network processes ~3N operations.  If per-operation cost
/// grows with working memory size (O(N) instead of O(1)), total time
/// becomes O(N^2).
///
/// It exercises:
///
/// - `deftemplate` with `default` values
/// - `modify` at scale (N modifications)
/// - `retract` at scale (N retractions)
/// - `not` over a template pattern (finish condition)
/// - Working memory lifecycle: assert -> modify -> retract
fn generate_churn_source(n_items: usize) -> String {
    let mut source = String::from(
        "\
(deftemplate item (slot id) (slot status (default pending)))
(deftemplate phase (slot name))

(deffacts initial
    (phase (name run))\n",
    );

    for i in 0..n_items {
        writeln!(source, "    (item (id {i}) (status pending))").unwrap();
    }

    source.push_str(
        ")

(defrule process-item
    (declare (salience 10))
    (phase (name run))
    ?item <- (item (id ?id) (status pending))
    =>
    (modify ?item (status done)))

(defrule cleanup-item
    (declare (salience 5))
    (phase (name run))
    ?item <- (item (id ?id) (status done))
    =>
    (retract ?item))

(defrule all-done
    (declare (salience -10))
    (phase (name run))
    (not (item))
    =>
    (printout t \"All items processed\" crlf))
",
    );
    source
}

fn bench_churn_100(c: &mut Criterion) {
    let source = generate_churn_source(100);
    c.bench_function("churn_100_facts", |b| {
        b.iter(|| {
            let mut engine = Engine::new(EngineConfig::utf8());
            engine.load_str(&source).unwrap();
            engine.reset().unwrap();
            engine.run(RunLimit::Unlimited).unwrap()
        });
    });
}

fn bench_churn_250(c: &mut Criterion) {
    let source = generate_churn_source(250);
    c.bench_function("churn_250_facts", |b| {
        b.iter(|| {
            let mut engine = Engine::new(EngineConfig::utf8());
            engine.load_str(&source).unwrap();
            engine.reset().unwrap();
            engine.run(RunLimit::Unlimited).unwrap()
        });
    });
}

fn bench_churn_500(c: &mut Criterion) {
    let source = generate_churn_source(500);
    c.bench_function("churn_500_facts", |b| {
        b.iter(|| {
            let mut engine = Engine::new(EngineConfig::utf8());
            engine.load_str(&source).unwrap();
            engine.reset().unwrap();
            engine.run(RunLimit::Unlimited).unwrap()
        });
    });
}

fn bench_churn_1000(c: &mut Criterion) {
    let source = generate_churn_source(1000);
    c.bench_function("churn_1000_facts", |b| {
        b.iter(|| {
            let mut engine = Engine::new(EngineConfig::utf8());
            engine.load_str(&source).unwrap();
            engine.reset().unwrap();
            engine.run(RunLimit::Unlimited).unwrap()
        });
    });
}

fn bench_churn_2000(c: &mut Criterion) {
    let source = generate_churn_source(2000);
    c.bench_function("churn_2000_facts", |b| {
        b.iter(|| {
            let mut engine = Engine::new(EngineConfig::utf8());
            engine.load_str(&source).unwrap();
            engine.reset().unwrap();
            engine.run(RunLimit::Unlimited).unwrap()
        });
    });
}

fn bench_churn_5000(c: &mut Criterion) {
    let source = generate_churn_source(5000);
    c.bench_function("churn_5000_facts", |b| {
        b.iter(|| {
            let mut engine = Engine::new(EngineConfig::utf8());
            engine.load_str(&source).unwrap();
            engine.reset().unwrap();
            engine.run(RunLimit::Unlimited).unwrap()
        });
    });
}

fn bench_churn_10000(c: &mut Criterion) {
    let source = generate_churn_source(10000);
    let mut group = c.benchmark_group("churn_10000");
    group.sample_size(10);
    group.bench_function("churn_10000_facts", |b| {
        b.iter(|| {
            let mut engine = Engine::new(EngineConfig::utf8());
            engine.load_str(&source).unwrap();
            engine.reset().unwrap();
            engine.run(RunLimit::Unlimited).unwrap()
        });
    });
    group.finish();
}

fn bench_churn_25000(c: &mut Criterion) {
    let source = generate_churn_source(25000);
    let mut group = c.benchmark_group("churn_25000");
    group.sample_size(10);
    group.bench_function("churn_25000_facts", |b| {
        b.iter(|| {
            let mut engine = Engine::new(EngineConfig::utf8());
            engine.load_str(&source).unwrap();
            engine.reset().unwrap();
            engine.run(RunLimit::Unlimited).unwrap()
        });
    });
    group.finish();
}

fn bench_churn_50000(c: &mut Criterion) {
    let source = generate_churn_source(50000);
    let mut group = c.benchmark_group("churn_50000");
    group.sample_size(10);
    group.bench_function("churn_50000_facts", |b| {
        b.iter(|| {
            let mut engine = Engine::new(EngineConfig::utf8());
            engine.load_str(&source).unwrap();
            engine.reset().unwrap();
            engine.run(RunLimit::Unlimited).unwrap()
        });
    });
    group.finish();
}

fn bench_churn_100000(c: &mut Criterion) {
    let source = generate_churn_source(100000);
    let mut group = c.benchmark_group("churn_100000");
    group.sample_size(10);
    group.bench_function("churn_100000_facts", |b| {
        b.iter(|| {
            let mut engine = Engine::new(EngineConfig::utf8());
            engine.load_str(&source).unwrap();
            engine.reset().unwrap();
            engine.run(RunLimit::Unlimited).unwrap()
        });
    });
    group.finish();
}

fn bench_churn_100_run_only(c: &mut Criterion) {
    let source = generate_churn_source(100);
    let mut engine = Engine::new(EngineConfig::utf8());
    engine.load_str(&source).unwrap();
    c.bench_function("churn_100_facts_run_only", |b| {
        b.iter(|| {
            engine.reset().unwrap();
            engine.run(RunLimit::Unlimited).unwrap()
        });
    });
}

criterion_group!(
    benches,
    bench_churn_100,
    bench_churn_250,
    bench_churn_500,
    bench_churn_1000,
    bench_churn_2000,
    bench_churn_5000,
    bench_churn_10000,
    bench_churn_25000,
    bench_churn_50000,
    bench_churn_100000,
    bench_churn_100_run_only,
);
criterion_main!(benches);
