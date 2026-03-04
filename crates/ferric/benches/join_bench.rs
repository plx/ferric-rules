use std::fmt::Write as FmtWrite;

use criterion::{criterion_group, criterion_main, Criterion};
use ferric::runtime::{Engine, EngineConfig, RunLimit};

/// Join-width stress benchmark.
///
/// Scales the depth of the beta join network to detect per-level overhead
/// that grows with network depth.  Each benchmark creates W template types
/// (`layer-0` through `layer-{W-1}`) with K facts per template, all sharing
/// the same key space.  A single rule joins all W templates on the shared
/// `?key` variable, firing K times (once per key).
///
/// With 1:1 key matching, each join level produces exactly K tokens, so
/// total work should be O(K * W).  If per-token propagation has hidden
/// O(depth) overhead, total work becomes O(K * W^2).
///
/// It exercises:
///
/// - `deftemplate` at scale (W + 1 templates)
/// - Deep beta join networks (W-1 join nodes)
/// - Variable binding across many patterns
/// - Token propagation through deep networks
const N_KEYS: usize = 100;

fn generate_join_source(width: usize, n_keys: usize) -> String {
    let mut source = String::new();

    // Declare layer templates
    for w in 0..width {
        writeln!(source, "(deftemplate layer-{w} (slot key) (slot val))").unwrap();
    }
    writeln!(source, "(deftemplate result (slot key) (slot matched))").unwrap();
    source.push('\n');

    // Generate facts
    source.push_str("(deffacts data\n");
    for w in 0..width {
        for k in 0..n_keys {
            writeln!(source, "    (layer-{w} (key k{k}) (val v{w}-{k}))").unwrap();
        }
    }
    source.push_str(")\n\n");

    // Generate the wide-join rule
    source.push_str("(defrule wide-join\n");
    for w in 0..width {
        writeln!(source, "    (layer-{w} (key ?k) (val ?v{w}))").unwrap();
    }
    source.push_str("    =>\n");
    source.push_str("    (assert (result (key ?k) (matched yes))))\n");

    source
}

fn bench_join_3(c: &mut Criterion) {
    let source = generate_join_source(3, N_KEYS);
    c.bench_function("join_3_wide", |b| {
        b.iter(|| {
            let mut engine = Engine::new(EngineConfig::utf8());
            engine.load_str(&source).unwrap();
            engine.reset().unwrap();
            engine.run(RunLimit::Unlimited).unwrap()
        });
    });
}

fn bench_join_5(c: &mut Criterion) {
    let source = generate_join_source(5, N_KEYS);
    c.bench_function("join_5_wide", |b| {
        b.iter(|| {
            let mut engine = Engine::new(EngineConfig::utf8());
            engine.load_str(&source).unwrap();
            engine.reset().unwrap();
            engine.run(RunLimit::Unlimited).unwrap()
        });
    });
}

fn bench_join_7(c: &mut Criterion) {
    let source = generate_join_source(7, N_KEYS);
    c.bench_function("join_7_wide", |b| {
        b.iter(|| {
            let mut engine = Engine::new(EngineConfig::utf8());
            engine.load_str(&source).unwrap();
            engine.reset().unwrap();
            engine.run(RunLimit::Unlimited).unwrap()
        });
    });
}

fn bench_join_9(c: &mut Criterion) {
    let source = generate_join_source(9, N_KEYS);
    c.bench_function("join_9_wide", |b| {
        b.iter(|| {
            let mut engine = Engine::new(EngineConfig::utf8());
            engine.load_str(&source).unwrap();
            engine.reset().unwrap();
            engine.run(RunLimit::Unlimited).unwrap()
        });
    });
}

fn bench_join_11(c: &mut Criterion) {
    let source = generate_join_source(11, N_KEYS);
    c.bench_function("join_11_wide", |b| {
        b.iter(|| {
            let mut engine = Engine::new(EngineConfig::utf8());
            engine.load_str(&source).unwrap();
            engine.reset().unwrap();
            engine.run(RunLimit::Unlimited).unwrap()
        });
    });
}

fn bench_join_13(c: &mut Criterion) {
    let source = generate_join_source(13, N_KEYS);
    c.bench_function("join_13_wide", |b| {
        b.iter(|| {
            let mut engine = Engine::new(EngineConfig::utf8());
            engine.load_str(&source).unwrap();
            engine.reset().unwrap();
            engine.run(RunLimit::Unlimited).unwrap()
        });
    });
}

fn bench_join_15(c: &mut Criterion) {
    let source = generate_join_source(15, N_KEYS);
    let mut group = c.benchmark_group("join_15");
    group.sample_size(10);
    group.bench_function("join_15_wide", |b| {
        b.iter(|| {
            let mut engine = Engine::new(EngineConfig::utf8());
            engine.load_str(&source).unwrap();
            engine.reset().unwrap();
            engine.run(RunLimit::Unlimited).unwrap()
        });
    });
    group.finish();
}

fn bench_join_17(c: &mut Criterion) {
    let source = generate_join_source(17, N_KEYS);
    let mut group = c.benchmark_group("join_17");
    group.sample_size(10);
    group.bench_function("join_17_wide", |b| {
        b.iter(|| {
            let mut engine = Engine::new(EngineConfig::utf8());
            engine.load_str(&source).unwrap();
            engine.reset().unwrap();
            engine.run(RunLimit::Unlimited).unwrap()
        });
    });
    group.finish();
}

fn bench_join_19(c: &mut Criterion) {
    let source = generate_join_source(19, N_KEYS);
    let mut group = c.benchmark_group("join_19");
    group.sample_size(10);
    group.bench_function("join_19_wide", |b| {
        b.iter(|| {
            let mut engine = Engine::new(EngineConfig::utf8());
            engine.load_str(&source).unwrap();
            engine.reset().unwrap();
            engine.run(RunLimit::Unlimited).unwrap()
        });
    });
    group.finish();
}

fn bench_join_21(c: &mut Criterion) {
    let source = generate_join_source(21, N_KEYS);
    let mut group = c.benchmark_group("join_21");
    group.sample_size(10);
    group.bench_function("join_21_wide", |b| {
        b.iter(|| {
            let mut engine = Engine::new(EngineConfig::utf8());
            engine.load_str(&source).unwrap();
            engine.reset().unwrap();
            engine.run(RunLimit::Unlimited).unwrap()
        });
    });
    group.finish();
}

fn bench_join_3_run_only(c: &mut Criterion) {
    let source = generate_join_source(3, N_KEYS);
    let mut engine = Engine::new(EngineConfig::utf8());
    engine.load_str(&source).unwrap();
    c.bench_function("join_3_wide_run_only", |b| {
        b.iter(|| {
            engine.reset().unwrap();
            engine.run(RunLimit::Unlimited).unwrap()
        });
    });
}

criterion_group!(
    benches,
    bench_join_3,
    bench_join_5,
    bench_join_7,
    bench_join_9,
    bench_join_11,
    bench_join_13,
    bench_join_15,
    bench_join_17,
    bench_join_19,
    bench_join_21,
    bench_join_3_run_only,
);
criterion_main!(benches);
