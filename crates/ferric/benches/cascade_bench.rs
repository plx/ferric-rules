use std::fmt::Write as FmtWrite;

use criterion::{criterion_group, criterion_main, Criterion};
use ferric::runtime::{Engine, EngineConfig, RunLimit};

/// Deep token tree retraction benchmark.
///
/// Creates a D-deep join rule with N keys. After initial run populates
/// the token tree, a retract rule removes base-layer facts, triggering
/// cascade retraction through D-1 child levels. This exercises
/// `TokenStore::remove_cascade` and the `fact_to_tokens` /
/// `parent_to_children` index maintenance.
fn generate_cascade_source(depth: usize, n_keys: usize) -> String {
    let mut source = String::new();

    // Generate D template definitions
    for d in 0..depth {
        writeln!(source, "(deftemplate layer-{d} (slot key))").unwrap();
    }
    writeln!(source, "(deftemplate matched (slot key))").unwrap();
    writeln!(source, "(deftemplate trigger-retract)").unwrap();
    source.push('\n');

    // Generate the deep-join rule joining all D layers on ?k
    source.push_str("(defrule deep-join\n    (declare (salience 10))\n");
    for d in 0..depth {
        writeln!(source, "    (layer-{d} (key ?k))").unwrap();
    }
    source.push_str("    =>\n    (assert (matched (key ?k))))\n\n");

    // Generate the retract rule that removes layer-0 facts
    source.push_str(
        "(defrule retract-base\n    \
         (declare (salience -10))\n    \
         (trigger-retract)\n    \
         ?f <- (layer-0 (key ?k))\n    \
         =>\n    \
         (retract ?f))\n\n",
    );

    // Generate deffacts with n_keys keys across all layers
    source.push_str("(deffacts layers\n");
    for i in 0..n_keys {
        for d in 0..depth {
            writeln!(source, "    (layer-{d} (key k{i}))").unwrap();
        }
    }
    source.push_str("    (trigger-retract))\n");
    source
}

fn bench_cascade_d3_100k(c: &mut Criterion) {
    let source = generate_cascade_source(3, 100);
    c.bench_function("cascade_d3_100k", |b| {
        b.iter(|| {
            let mut engine = Engine::new(EngineConfig::utf8());
            engine.load_str(&source).unwrap();
            engine.reset().unwrap();
            engine.run(RunLimit::Unlimited).unwrap()
        });
    });
}

fn bench_cascade_d5_100k(c: &mut Criterion) {
    let source = generate_cascade_source(5, 100);
    let mut group = c.benchmark_group("cascade_d5_100k");
    group.sample_size(10);
    group.bench_function("cascade_d5_100k", |b| {
        b.iter(|| {
            let mut engine = Engine::new(EngineConfig::utf8());
            engine.load_str(&source).unwrap();
            engine.reset().unwrap();
            engine.run(RunLimit::Unlimited).unwrap()
        });
    });
    group.finish();
}

fn bench_cascade_d7_50k(c: &mut Criterion) {
    let source = generate_cascade_source(7, 50);
    let mut group = c.benchmark_group("cascade_d7_50k");
    group.sample_size(10);
    group.bench_function("cascade_d7_50k", |b| {
        b.iter(|| {
            let mut engine = Engine::new(EngineConfig::utf8());
            engine.load_str(&source).unwrap();
            engine.reset().unwrap();
            engine.run(RunLimit::Unlimited).unwrap()
        });
    });
    group.finish();
}

fn bench_cascade_d10_30k(c: &mut Criterion) {
    let source = generate_cascade_source(10, 30);
    let mut group = c.benchmark_group("cascade_d10_30k");
    group.sample_size(10);
    group.bench_function("cascade_d10_30k", |b| {
        b.iter(|| {
            let mut engine = Engine::new(EngineConfig::utf8());
            engine.load_str(&source).unwrap();
            engine.reset().unwrap();
            engine.run(RunLimit::Unlimited).unwrap()
        });
    });
    group.finish();
}

fn bench_cascade_d15_20k(c: &mut Criterion) {
    let source = generate_cascade_source(15, 20);
    let mut group = c.benchmark_group("cascade_d15_20k");
    group.sample_size(10);
    group.bench_function("cascade_d15_20k", |b| {
        b.iter(|| {
            let mut engine = Engine::new(EngineConfig::utf8());
            engine.load_str(&source).unwrap();
            engine.reset().unwrap();
            engine.run(RunLimit::Unlimited).unwrap()
        });
    });
    group.finish();
}

fn bench_cascade_d3_100k_run_only(c: &mut Criterion) {
    let source = generate_cascade_source(3, 100);
    let mut engine = Engine::new(EngineConfig::utf8());
    engine.load_str(&source).unwrap();
    c.bench_function("cascade_d3_100k_run_only", |b| {
        b.iter(|| {
            engine.reset().unwrap();
            engine.run(RunLimit::Unlimited).unwrap()
        });
    });
}

criterion_group!(
    benches,
    bench_cascade_d3_100k,
    bench_cascade_d5_100k,
    bench_cascade_d7_50k,
    bench_cascade_d10_30k,
    bench_cascade_d15_20k,
    bench_cascade_d3_100k_run_only,
);
criterion_main!(benches);
