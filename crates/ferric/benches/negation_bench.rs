use std::fmt::Write as FmtWrite;

use criterion::{criterion_group, criterion_main, Criterion};
use ferric::runtime::{Engine, EngineConfig, RunLimit};

/// Negative node pressure benchmark.
///
/// Scales the number of facts matching a negated pattern to detect
/// per-retraction overhead in the blocker map management.  Each benchmark
/// creates 1 signal fact and N blocker facts all matching the signal's
/// name.  A remove rule (salience 10) retracts one blocker per firing.
/// A signal-clear rule (salience -10) fires after all blockers are gone
/// via `(not (blocker (name ?n)))`.
///
/// Total: N+1 rule firings.  Each blocker retraction updates the negative
/// node's blocker map.  If removal cost is O(`remaining_blockers`), total
/// is O(N^2/2).  If O(1) per removal, total is O(N).
///
/// It exercises:
///
/// - `not` with a join test (blocker name matches signal name)
/// - Negative node blocker map at scale
/// - Retraction through negative nodes
/// - Agenda management with N initial activations
fn generate_negation_source(n_blockers: usize) -> String {
    let mut source = String::from(
        "\
(deftemplate signal (slot name))
(deftemplate blocker (slot name) (slot seq))
(deftemplate phase (slot name))

(deffacts setup
    (phase (name clear))
    (signal (name S))\n",
    );

    for i in 0..n_blockers {
        writeln!(source, "    (blocker (name S) (seq {i}))").unwrap();
    }

    source.push_str(
        ")

(defrule remove-blocker
    (declare (salience 10))
    (phase (name clear))
    ?b <- (blocker)
    =>
    (retract ?b))

(defrule signal-clear
    (declare (salience -10))
    (phase (name clear))
    (signal (name ?n))
    (not (blocker (name ?n)))
    =>
    (printout t \"Signal clear\" crlf))
",
    );
    source
}

fn bench_negation_50(c: &mut Criterion) {
    let source = generate_negation_source(50);
    c.bench_function("negation_50_blockers", |b| {
        b.iter(|| {
            let mut engine = Engine::new(EngineConfig::utf8());
            engine.load_str(&source).unwrap();
            engine.reset().unwrap();
            engine.run(RunLimit::Unlimited).unwrap()
        });
    });
}

fn bench_negation_100(c: &mut Criterion) {
    let source = generate_negation_source(100);
    c.bench_function("negation_100_blockers", |b| {
        b.iter(|| {
            let mut engine = Engine::new(EngineConfig::utf8());
            engine.load_str(&source).unwrap();
            engine.reset().unwrap();
            engine.run(RunLimit::Unlimited).unwrap()
        });
    });
}

fn bench_negation_200(c: &mut Criterion) {
    let source = generate_negation_source(200);
    c.bench_function("negation_200_blockers", |b| {
        b.iter(|| {
            let mut engine = Engine::new(EngineConfig::utf8());
            engine.load_str(&source).unwrap();
            engine.reset().unwrap();
            engine.run(RunLimit::Unlimited).unwrap()
        });
    });
}

fn bench_negation_500(c: &mut Criterion) {
    let source = generate_negation_source(500);
    c.bench_function("negation_500_blockers", |b| {
        b.iter(|| {
            let mut engine = Engine::new(EngineConfig::utf8());
            engine.load_str(&source).unwrap();
            engine.reset().unwrap();
            engine.run(RunLimit::Unlimited).unwrap()
        });
    });
}

fn bench_negation_1000(c: &mut Criterion) {
    let source = generate_negation_source(1000);
    c.bench_function("negation_1000_blockers", |b| {
        b.iter(|| {
            let mut engine = Engine::new(EngineConfig::utf8());
            engine.load_str(&source).unwrap();
            engine.reset().unwrap();
            engine.run(RunLimit::Unlimited).unwrap()
        });
    });
}

fn bench_negation_2500(c: &mut Criterion) {
    let source = generate_negation_source(2500);
    c.bench_function("negation_2500_blockers", |b| {
        b.iter(|| {
            let mut engine = Engine::new(EngineConfig::utf8());
            engine.load_str(&source).unwrap();
            engine.reset().unwrap();
            engine.run(RunLimit::Unlimited).unwrap()
        });
    });
}

fn bench_negation_5000(c: &mut Criterion) {
    let source = generate_negation_source(5000);
    let mut group = c.benchmark_group("negation_5000");
    group.sample_size(10);
    group.bench_function("negation_5000_blockers", |b| {
        b.iter(|| {
            let mut engine = Engine::new(EngineConfig::utf8());
            engine.load_str(&source).unwrap();
            engine.reset().unwrap();
            engine.run(RunLimit::Unlimited).unwrap()
        });
    });
    group.finish();
}

fn bench_negation_10000(c: &mut Criterion) {
    let source = generate_negation_source(10_000);
    let mut group = c.benchmark_group("negation_10000");
    group.sample_size(10);
    group.bench_function("negation_10000_blockers", |b| {
        b.iter(|| {
            let mut engine = Engine::new(EngineConfig::utf8());
            engine.load_str(&source).unwrap();
            engine.reset().unwrap();
            engine.run(RunLimit::Unlimited).unwrap()
        });
    });
    group.finish();
}

fn bench_negation_25000(c: &mut Criterion) {
    let source = generate_negation_source(25_000);
    let mut group = c.benchmark_group("negation_25000");
    group.sample_size(10);
    group.bench_function("negation_25000_blockers", |b| {
        b.iter(|| {
            let mut engine = Engine::new(EngineConfig::utf8());
            engine.load_str(&source).unwrap();
            engine.reset().unwrap();
            engine.run(RunLimit::Unlimited).unwrap()
        });
    });
    group.finish();
}

fn bench_negation_50000(c: &mut Criterion) {
    let source = generate_negation_source(50_000);
    let mut group = c.benchmark_group("negation_50000");
    group.sample_size(10);
    group.bench_function("negation_50000_blockers", |b| {
        b.iter(|| {
            let mut engine = Engine::new(EngineConfig::utf8());
            engine.load_str(&source).unwrap();
            engine.reset().unwrap();
            engine.run(RunLimit::Unlimited).unwrap()
        });
    });
    group.finish();
}

fn bench_negation_50_run_only(c: &mut Criterion) {
    let source = generate_negation_source(50);
    let mut engine = Engine::new(EngineConfig::utf8());
    engine.load_str(&source).unwrap();
    c.bench_function("negation_50_blockers_run_only", |b| {
        b.iter(|| {
            engine.reset().unwrap();
            engine.run(RunLimit::Unlimited).unwrap()
        });
    });
}

criterion_group!(
    benches,
    bench_negation_50,
    bench_negation_100,
    bench_negation_200,
    bench_negation_500,
    bench_negation_1000,
    bench_negation_2500,
    bench_negation_5000,
    bench_negation_10000,
    bench_negation_25000,
    bench_negation_50000,
    bench_negation_50_run_only,
);
criterion_main!(benches);
