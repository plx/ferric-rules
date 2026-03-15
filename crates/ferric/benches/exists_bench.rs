use std::fmt::Write as FmtWrite;

use criterion::{criterion_group, criterion_main, Criterion};
use ferric::runtime::{Engine, EngineConfig, RunLimit};

/// Exists CE benchmark: scales the number of supporting facts per parent token.
///
/// For N sensors each with M readings, the exists node sees N parent tokens
/// each backed by M supporters. This exercises the `ExistsMemory` bidirectional
/// hash maps (`support`, `satisfied`, `fact_to_parents`) and the
/// `parents_supported_by()` Vec allocation.
fn generate_exists_source(n_sensors: usize, m_readings: usize) -> String {
    let mut source = String::from(
        "\
(deftemplate sensor (slot id))
(deftemplate reading (slot sensor-id) (slot value))
(deftemplate has-data (slot sensor-id))

(deffacts sensors\n",
    );

    for i in 0..n_sensors {
        writeln!(source, "    (sensor (id s{i}))").unwrap();
    }
    source.push_str(")\n\n(deffacts readings\n");

    for i in 0..n_sensors {
        for j in 0..m_readings {
            writeln!(source, "    (reading (sensor-id s{i}) (value {j}))").unwrap();
        }
    }

    source.push_str(
        ")

(defrule sensor-has-data
    (sensor (id ?sid))
    (exists (reading (sensor-id ?sid)))
    =>
    (assert (has-data (sensor-id ?sid))))
",
    );
    source
}

fn bench_exists_10s_5r(c: &mut Criterion) {
    let source = generate_exists_source(10, 5);
    c.bench_function("exists_10s_5r", |b| {
        b.iter(|| {
            let mut engine = Engine::new(EngineConfig::utf8());
            engine.load_str(&source).unwrap();
            engine.reset().unwrap();
            engine.run(RunLimit::Unlimited).unwrap()
        });
    });
}

fn bench_exists_50s_10r(c: &mut Criterion) {
    let source = generate_exists_source(50, 10);
    c.bench_function("exists_50s_10r", |b| {
        b.iter(|| {
            let mut engine = Engine::new(EngineConfig::utf8());
            engine.load_str(&source).unwrap();
            engine.reset().unwrap();
            engine.run(RunLimit::Unlimited).unwrap()
        });
    });
}

fn bench_exists_100s_20r(c: &mut Criterion) {
    let source = generate_exists_source(100, 20);
    c.bench_function("exists_100s_20r", |b| {
        b.iter(|| {
            let mut engine = Engine::new(EngineConfig::utf8());
            engine.load_str(&source).unwrap();
            engine.reset().unwrap();
            engine.run(RunLimit::Unlimited).unwrap()
        });
    });
}

fn bench_exists_200s_50r(c: &mut Criterion) {
    let source = generate_exists_source(200, 50);
    let mut group = c.benchmark_group("exists_200s_50r");
    group.sample_size(10);
    group.bench_function("exists_200s_50r", |b| {
        b.iter(|| {
            let mut engine = Engine::new(EngineConfig::utf8());
            engine.load_str(&source).unwrap();
            engine.reset().unwrap();
            engine.run(RunLimit::Unlimited).unwrap()
        });
    });
    group.finish();
}

fn bench_exists_500s_100r(c: &mut Criterion) {
    let source = generate_exists_source(500, 100);
    let mut group = c.benchmark_group("exists_500s_100r");
    group.sample_size(10);
    group.bench_function("exists_500s_100r", |b| {
        b.iter(|| {
            let mut engine = Engine::new(EngineConfig::utf8());
            engine.load_str(&source).unwrap();
            engine.reset().unwrap();
            engine.run(RunLimit::Unlimited).unwrap()
        });
    });
    group.finish();
}

fn bench_exists_1000s_50r(c: &mut Criterion) {
    let source = generate_exists_source(1000, 50);
    let mut group = c.benchmark_group("exists_1000s_50r");
    group.sample_size(10);
    group.bench_function("exists_1000s_50r", |b| {
        b.iter(|| {
            let mut engine = Engine::new(EngineConfig::utf8());
            engine.load_str(&source).unwrap();
            engine.reset().unwrap();
            engine.run(RunLimit::Unlimited).unwrap()
        });
    });
    group.finish();
}

fn bench_exists_10s_5r_run_only(c: &mut Criterion) {
    let source = generate_exists_source(10, 5);
    let mut engine = Engine::new(EngineConfig::utf8());
    engine.load_str(&source).unwrap();
    c.bench_function("exists_10s_5r_run_only", |b| {
        b.iter(|| {
            engine.reset().unwrap();
            engine.run(RunLimit::Unlimited).unwrap()
        });
    });
}

criterion_group!(
    benches,
    bench_exists_10s_5r,
    bench_exists_50s_10r,
    bench_exists_100s_20r,
    bench_exists_200s_50r,
    bench_exists_500s_100r,
    bench_exists_1000s_50r,
    bench_exists_10s_5r_run_only,
);
criterion_main!(benches);
