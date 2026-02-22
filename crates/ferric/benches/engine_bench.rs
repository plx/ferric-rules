use std::fmt::Write as FmtWrite;

use criterion::{criterion_group, criterion_main, Criterion};
use ferric::runtime::{Engine, EngineConfig, RunLimit};

// ---------------------------------------------------------------------------
// Lifecycle benchmarks
// ---------------------------------------------------------------------------

fn bench_engine_create(c: &mut Criterion) {
    c.bench_function("engine_create", |b| {
        b.iter(|| Engine::new(EngineConfig::utf8()));
    });
}

// ---------------------------------------------------------------------------
// Load + Run benchmarks (measures full pipeline)
// ---------------------------------------------------------------------------

fn bench_load_and_run_simple(c: &mut Criterion) {
    let source = r"
        (deffacts startup (item a) (item b) (item c))
        (defrule process (item ?x) => (assert (processed ?x)))
    ";
    c.bench_function("load_and_run_simple", |b| {
        b.iter(|| {
            let mut engine = Engine::new(EngineConfig::utf8());
            engine.load_str(source).unwrap();
            engine.reset().unwrap();
            engine.run(RunLimit::Unlimited).unwrap()
        });
    });
}

fn bench_load_and_run_chain(c: &mut Criterion) {
    let source = r"
        (deffacts startup (stage 1))
        (defrule s1 ?f <- (stage 1) => (retract ?f) (assert (stage 2)))
        (defrule s2 ?f <- (stage 2) => (retract ?f) (assert (stage 3)))
        (defrule s3 ?f <- (stage 3) => (retract ?f) (assert (stage 4)))
        (defrule s4 ?f <- (stage 4) => (retract ?f) (assert (done)))
    ";
    c.bench_function("load_and_run_chain_4", |b| {
        b.iter(|| {
            let mut engine = Engine::new(EngineConfig::utf8());
            engine.load_str(source).unwrap();
            engine.reset().unwrap();
            engine.run(RunLimit::Unlimited).unwrap()
        });
    });
}

// ---------------------------------------------------------------------------
// Reset + Run benchmarks (measures execution without compilation)
// ---------------------------------------------------------------------------

fn bench_reset_run_simple(c: &mut Criterion) {
    let source = r"
        (deffacts startup (item a) (item b) (item c))
        (defrule process (item ?x) => (assert (processed ?x)))
    ";
    let mut engine = Engine::new(EngineConfig::utf8());
    engine.load_str(source).unwrap();
    c.bench_function("reset_run_simple", |b| {
        b.iter(|| {
            engine.reset().unwrap();
            engine.run(RunLimit::Unlimited).unwrap()
        });
    });
}

fn bench_reset_run_many_facts(c: &mut Criterion) {
    // 20 facts matching a single rule — measures alpha network throughput
    let mut source = String::from("(deffacts startup");
    for i in 0..20 {
        write!(source, " (item f{i})").unwrap();
    }
    source.push_str(")\n(defrule process (item ?x) => (assert (done ?x)))");
    let mut engine = Engine::new(EngineConfig::utf8());
    engine.load_str(&source).unwrap();
    c.bench_function("reset_run_20_facts", |b| {
        b.iter(|| {
            engine.reset().unwrap();
            engine.run(RunLimit::Unlimited).unwrap()
        });
    });
}

fn bench_reset_run_negation(c: &mut Criterion) {
    let source = r"
        (deffacts startup (item a) (item b) (item c))
        (defrule safe (item ?x) (not (danger ?x)) => (assert (ok ?x)))
    ";
    let mut engine = Engine::new(EngineConfig::utf8());
    engine.load_str(source).unwrap();
    c.bench_function("reset_run_negation", |b| {
        b.iter(|| {
            engine.reset().unwrap();
            engine.run(RunLimit::Unlimited).unwrap()
        });
    });
}

fn bench_reset_run_join(c: &mut Criterion) {
    let source = r"
        (deffacts startup
            (person Alice) (age Alice 30)
            (person Bob) (age Bob 25)
            (person Carol) (age Carol 35))
        (defrule greet (person ?n) (age ?n ?a) => (assert (greeted ?n)))
    ";
    let mut engine = Engine::new(EngineConfig::utf8());
    engine.load_str(source).unwrap();
    c.bench_function("reset_run_join_3", |b| {
        b.iter(|| {
            engine.reset().unwrap();
            engine.run(RunLimit::Unlimited).unwrap()
        });
    });
}

fn bench_reset_run_retract_cycle(c: &mut Criterion) {
    let source = r"
        (deffacts startup (item a) (item b) (item c))
        (defrule consume ?f <- (item ?x) => (retract ?f) (assert (done ?x)))
    ";
    let mut engine = Engine::new(EngineConfig::utf8());
    engine.load_str(source).unwrap();
    c.bench_function("reset_run_retract_3", |b| {
        b.iter(|| {
            engine.reset().unwrap();
            engine.run(RunLimit::Unlimited).unwrap()
        });
    });
}

// ---------------------------------------------------------------------------
// Compilation benchmarks (measures parser + loader without execution)
// ---------------------------------------------------------------------------

fn bench_compile_only(c: &mut Criterion) {
    let source = r"
        (deftemplate sensor (slot name) (slot value (default 0)))
        (deffacts startup (sensor (name temp) (value 72)))
        (defrule check-temp
            (sensor (name temp) (value ?v))
            (test (> ?v 100))
            =>
            (assert (alarm temp)))
    ";
    c.bench_function("compile_template_rule", |b| {
        b.iter(|| {
            let mut engine = Engine::new(EngineConfig::utf8());
            engine.load_str(source).unwrap();
        });
    });
}

criterion_group!(
    benches,
    bench_engine_create,
    bench_load_and_run_simple,
    bench_load_and_run_chain,
    bench_reset_run_simple,
    bench_reset_run_many_facts,
    bench_reset_run_negation,
    bench_reset_run_join,
    bench_reset_run_retract_cycle,
    bench_compile_only,
);
criterion_main!(benches);
