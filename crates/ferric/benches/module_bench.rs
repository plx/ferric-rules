use std::fmt::Write as FmtWrite;

use criterion::{criterion_group, criterion_main, Criterion};
use ferric::runtime::{Engine, EngineConfig, RunLimit};

/// Multi-module focus stack benchmark.
///
/// M modules in a processing chain, N items flowing through each module
/// via `(focus ...)`. Measures focus-stack transition overhead and
/// module-scoped rule firing.
fn generate_module_source(n_modules: usize, n_items: usize) -> String {
    // Define MAIN module with template and deffacts first (before phase modules)
    let mut source = String::from(
        "\
(defmodule MAIN (export ?ALL))
(deftemplate MAIN::item (slot id) (slot stage (default 0)))

(deffacts MAIN::items\n",
    );

    for i in 0..n_items {
        writeln!(source, "    (item (id i{i}))").unwrap();
    }
    source.push_str(")\n\n");

    // Define phase modules (import MAIN's exports including the template)
    for m in 1..=n_modules {
        writeln!(source, "(defmodule PHASE-{m} (import MAIN ?ALL))").unwrap();
    }
    source.push('\n');

    // Each phase module has a rule that advances items from stage K-1 to K
    for m in 1..=n_modules {
        let from_stage = m - 1;
        writeln!(
            source,
            "\
(defrule PHASE-{m}::advance
    ?f <- (MAIN::item (id ?id) (stage {from_stage}))
    =>
    (modify ?f (stage {m})))\n"
        )
        .unwrap();
    }

    // MAIN rule to push focus stack with all phases when work remains
    source.push_str("(defrule MAIN::run-phases\n    (declare (salience 10))\n");
    writeln!(source, "    (exists (MAIN::item (stage ~{n_modules})))").unwrap();
    source.push_str("    =>\n    (focus");
    for m in 1..=n_modules {
        write!(source, " PHASE-{m}").unwrap();
    }
    source.push_str("))\n\n");

    // MAIN completion rule
    source.push_str("(defrule MAIN::done\n    (declare (salience -10))\n");
    writeln!(source, "    (not (MAIN::item (stage ~{n_modules})))").unwrap();
    source.push_str("    =>\n    (assert (finished)))\n");

    source
}

fn bench_module_3m_100i(c: &mut Criterion) {
    let source = generate_module_source(3, 100);
    c.bench_function("module_3m_100i", |b| {
        b.iter(|| {
            let mut engine = Engine::new(EngineConfig::utf8());
            engine.load_str(&source).unwrap();
            engine.reset().unwrap();
            engine.run(RunLimit::Unlimited).unwrap()
        });
    });
}

fn bench_module_5m_100i(c: &mut Criterion) {
    let source = generate_module_source(5, 100);
    let mut group = c.benchmark_group("module_5m_100i");
    group.sample_size(10);
    group.bench_function("module_5m_100i", |b| {
        b.iter(|| {
            let mut engine = Engine::new(EngineConfig::utf8());
            engine.load_str(&source).unwrap();
            engine.reset().unwrap();
            engine.run(RunLimit::Unlimited).unwrap()
        });
    });
    group.finish();
}

fn bench_module_10m_50i(c: &mut Criterion) {
    let source = generate_module_source(10, 50);
    let mut group = c.benchmark_group("module_10m_50i");
    group.sample_size(10);
    group.bench_function("module_10m_50i", |b| {
        b.iter(|| {
            let mut engine = Engine::new(EngineConfig::utf8());
            engine.load_str(&source).unwrap();
            engine.reset().unwrap();
            engine.run(RunLimit::Unlimited).unwrap()
        });
    });
    group.finish();
}

fn bench_module_20m_20i(c: &mut Criterion) {
    let source = generate_module_source(20, 20);
    let mut group = c.benchmark_group("module_20m_20i");
    group.sample_size(10);
    group.bench_function("module_20m_20i", |b| {
        b.iter(|| {
            let mut engine = Engine::new(EngineConfig::utf8());
            engine.load_str(&source).unwrap();
            engine.reset().unwrap();
            engine.run(RunLimit::Unlimited).unwrap()
        });
    });
    group.finish();
}

fn bench_module_3m_100i_run_only(c: &mut Criterion) {
    let source = generate_module_source(3, 100);
    let mut engine = Engine::new(EngineConfig::utf8());
    engine.load_str(&source).unwrap();
    c.bench_function("module_3m_100i_run_only", |b| {
        b.iter(|| {
            engine.reset().unwrap();
            engine.run(RunLimit::Unlimited).unwrap()
        });
    });
}

criterion_group!(
    benches,
    bench_module_3m_100i,
    bench_module_5m_100i,
    bench_module_10m_50i,
    bench_module_20m_20i,
    bench_module_3m_100i_run_only,
);
criterion_main!(benches);
