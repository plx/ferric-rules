use std::fmt::Write as FmtWrite;

use criterion::{criterion_group, criterion_main, Criterion};
use ferric::runtime::{Engine, EngineConfig, RunLimit};

/// Forall CE benchmark: scales the number of tasks that must all be processed.
///
/// Forall desugars to NCC `(not (and P (not Q)))`. The NCC memory tracks
/// result counts and result owners; `remove_parent_token` does a `retain`
/// scan over `result_owner` which is `O(total_results)` per parent removal.
fn generate_forall_source(n_tasks: usize) -> String {
    let mut source = String::from(
        "\
(deftemplate task (slot id) (slot status (default pending)))

(deffacts tasks\n",
    );

    for i in 0..n_tasks {
        writeln!(source, "    (task (id t{i}))").unwrap();
    }

    source.push_str(
        ")

(defrule process
    (declare (salience 10))
    ?t <- (task (id ?id) (status pending))
    =>
    (modify ?t (status done)))

(defrule all-done
    (declare (salience -10))
    (forall (task (id ?id) (status ?s))
            (test (eq ?s done)))
    =>
    (assert (complete)))
",
    );
    source
}

fn bench_forall_20(c: &mut Criterion) {
    let source = generate_forall_source(20);
    c.bench_function("forall_20_tasks", |b| {
        b.iter(|| {
            let mut engine = Engine::new(EngineConfig::utf8());
            engine.load_str(&source).unwrap();
            engine.reset().unwrap();
            engine.run(RunLimit::Unlimited).unwrap()
        });
    });
}

fn bench_forall_50(c: &mut Criterion) {
    let source = generate_forall_source(50);
    c.bench_function("forall_50_tasks", |b| {
        b.iter(|| {
            let mut engine = Engine::new(EngineConfig::utf8());
            engine.load_str(&source).unwrap();
            engine.reset().unwrap();
            engine.run(RunLimit::Unlimited).unwrap()
        });
    });
}

fn bench_forall_100(c: &mut Criterion) {
    let source = generate_forall_source(100);
    c.bench_function("forall_100_tasks", |b| {
        b.iter(|| {
            let mut engine = Engine::new(EngineConfig::utf8());
            engine.load_str(&source).unwrap();
            engine.reset().unwrap();
            engine.run(RunLimit::Unlimited).unwrap()
        });
    });
}

fn bench_forall_200(c: &mut Criterion) {
    let source = generate_forall_source(200);
    let mut group = c.benchmark_group("forall_200");
    group.sample_size(10);
    group.bench_function("forall_200_tasks", |b| {
        b.iter(|| {
            let mut engine = Engine::new(EngineConfig::utf8());
            engine.load_str(&source).unwrap();
            engine.reset().unwrap();
            engine.run(RunLimit::Unlimited).unwrap()
        });
    });
    group.finish();
}

fn bench_forall_500(c: &mut Criterion) {
    let source = generate_forall_source(500);
    let mut group = c.benchmark_group("forall_500");
    group.sample_size(10);
    group.bench_function("forall_500_tasks", |b| {
        b.iter(|| {
            let mut engine = Engine::new(EngineConfig::utf8());
            engine.load_str(&source).unwrap();
            engine.reset().unwrap();
            engine.run(RunLimit::Unlimited).unwrap()
        });
    });
    group.finish();
}

fn bench_forall_1000(c: &mut Criterion) {
    let source = generate_forall_source(1000);
    let mut group = c.benchmark_group("forall_1000");
    group.sample_size(10);
    group.bench_function("forall_1000_tasks", |b| {
        b.iter(|| {
            let mut engine = Engine::new(EngineConfig::utf8());
            engine.load_str(&source).unwrap();
            engine.reset().unwrap();
            engine.run(RunLimit::Unlimited).unwrap()
        });
    });
    group.finish();
}

fn bench_forall_2000(c: &mut Criterion) {
    let source = generate_forall_source(2000);
    let mut group = c.benchmark_group("forall_2000");
    group.sample_size(10);
    group.bench_function("forall_2000_tasks", |b| {
        b.iter(|| {
            let mut engine = Engine::new(EngineConfig::utf8());
            engine.load_str(&source).unwrap();
            engine.reset().unwrap();
            engine.run(RunLimit::Unlimited).unwrap()
        });
    });
    group.finish();
}

fn bench_forall_20_run_only(c: &mut Criterion) {
    let source = generate_forall_source(20);
    let mut engine = Engine::new(EngineConfig::utf8());
    engine.load_str(&source).unwrap();
    c.bench_function("forall_20_tasks_run_only", |b| {
        b.iter(|| {
            engine.reset().unwrap();
            engine.run(RunLimit::Unlimited).unwrap()
        });
    });
}

criterion_group!(
    benches,
    bench_forall_20,
    bench_forall_50,
    bench_forall_100,
    bench_forall_200,
    bench_forall_500,
    bench_forall_1000,
    bench_forall_2000,
    bench_forall_20_run_only,
);
criterion_main!(benches);
