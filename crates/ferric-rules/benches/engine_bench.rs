mod support;

use std::fmt::Write as FmtWrite;

use criterion::{criterion_group, criterion_main, Criterion};
use ferric_rules::runtime::{Engine, EngineConfig, RunLimit};

// ---------------------------------------------------------------------------
// Lifecycle benchmarks
// ---------------------------------------------------------------------------

fn bench_engine_create(c: &mut Criterion) {
    c.bench_function("engine_create", |b| {
        let mut engine = Engine::new(EngineConfig::utf8());
        assert_eq!(engine.facts().unwrap().count(), 0);
        support::verify_reset_run(&mut engine, 0);
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
        let engine = support::verify_source(source, 3);
        assert_eq!(
            support::ordered_symbols(&engine, "processed"),
            ["a", "b", "c"]
        );
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
        let engine = support::verify_source(source, 4);
        assert!(engine.find_facts("stage").unwrap().is_empty());
        assert_eq!(engine.find_facts("done").unwrap().len(), 1);
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
        for _ in 0..2 {
            support::verify_reset_run(&mut engine, 3);
            assert_eq!(
                support::ordered_symbols(&engine, "processed"),
                ["a", "b", "c"]
            );
        }
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
        support::verify_reset_run(&mut engine, 20);
        assert_eq!(
            support::ordered_symbols(&engine, "done"),
            support::expected_symbols("f", 20)
        );
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
        support::verify_reset_run(&mut engine, 3);
        assert_eq!(support::ordered_symbols(&engine, "ok"), ["a", "b", "c"]);
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
        support::verify_reset_run(&mut engine, 3);
        assert_eq!(
            support::ordered_symbols(&engine, "greeted"),
            ["Alice", "Bob", "Carol"]
        );
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
        support::verify_reset_run(&mut engine, 3);
        assert!(engine.find_facts("item").unwrap().is_empty());
        assert_eq!(support::ordered_symbols(&engine, "done"), ["a", "b", "c"]);
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
        let mut engine = support::verify_source(source, 0);
        assert_eq!(support::template_ids(&engine, "sensor").len(), 1);
        assert!(engine.find_facts("alarm").unwrap().is_empty());
        engine
            .load_str("(assert (sensor (name temp) (value 101)))")
            .unwrap();
        support::verify_run(&mut engine, 1);
        assert_eq!(support::ordered_symbols(&engine, "alarm"), ["temp"]);
        b.iter(|| {
            let mut engine = Engine::new(EngineConfig::utf8());
            engine.load_str(source).unwrap();
        });
    });
}

/// Larger lifecycle workloads selected before the threading comparison.
/// The same source and execution path supply the oracle and measured work.
fn bench_lifecycle_sizes(c: &mut Criterion) {
    for n_items in [100, 1_000] {
        let mut source = String::from("(deffacts startup\n");
        for i in 0..n_items {
            writeln!(source, "(item {i})").unwrap();
        }
        source.push_str(")\n(defrule consume ?f <- (item ?i) => (retract ?f) (assert (done ?i)))");
        let validate = |engine: &Engine| {
            assert!(engine.find_facts("item").unwrap().is_empty());
            assert_eq!(
                support::ordered_integers(engine, "done"),
                (0..i64::try_from(n_items).unwrap()).collect::<Vec<_>>()
            );
        };
        c.bench_function(&format!("lifecycle_load_reset_run_{n_items}"), |b| {
            validate(&support::verify_source(&source, n_items));
            b.iter(|| {
                let mut engine = Engine::new(EngineConfig::utf8());
                engine.load_str(&source).unwrap();
                engine.reset().unwrap();
                engine.run(RunLimit::Unlimited).unwrap()
            });
        });
        let mut engine = Engine::new(EngineConfig::utf8());
        engine.load_str(&source).unwrap();
        c.bench_function(&format!("lifecycle_reset_run_{n_items}"), |b| {
            for _ in 0..2 {
                support::verify_reset_run(&mut engine, n_items);
                validate(&engine);
            }
            b.iter(|| {
                engine.reset().unwrap();
                engine.run(RunLimit::Unlimited).unwrap()
            });
        });
    }
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
    bench_lifecycle_sizes,
);
criterion_main!(benches);
