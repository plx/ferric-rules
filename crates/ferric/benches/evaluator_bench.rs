use std::fmt::Write as FmtWrite;

use criterion::{criterion_group, criterion_main, Criterion};
use ferric::runtime::{Engine, EngineConfig, RunLimit};

/// Expression evaluator throughput benchmark.
///
/// Exercises the evaluator under varying complexity: arithmetic, user-defined
/// function dispatch, loop constructs, and string operations. Every `test` CE
/// and every RHS argument passes through the evaluator; the dispatch chain
/// (builtins → `FunctionEnv` → `GenericRegistry` → error) adds per-call overhead.
/// Arithmetic-heavy RHS: counter fires N times, each time doing arithmetic.
fn generate_arithmetic_source(n: usize) -> String {
    let mut source = String::new();
    writeln!(
        source,
        "\
(deftemplate counter (slot val))
(deffacts init (counter (val 0)))

(defrule compute
    ?f <- (counter (val ?v))
    (test (< ?v {n}))
    =>
    (modify ?f (val (+ (* ?v 2) (- ?v (div ?v 3)) 1))))"
    )
    .unwrap();
    source
}

/// Deffunction dispatch: iterative sum via user-defined function.
fn generate_deffunction_source(n: usize) -> String {
    let mut source = String::new();
    writeln!(
        source,
        "\
(deffunction compute-sum (?n)
    (bind ?sum 0)
    (loop-for-count (?i 1 ?n)
        (bind ?sum (+ ?sum ?i)))
    ?sum)

(deftemplate input (slot n))
(deftemplate result (slot val))
(deffacts inputs (input (n {n})))

(defrule call-sum
    (input (n ?n))
    =>
    (assert (result (val (compute-sum ?n)))))"
    )
    .unwrap();
    source
}

/// Loop-for-count in RHS: single rule fires once with tight loop.
fn generate_loop_source(n: usize) -> String {
    let mut source = String::new();
    writeln!(
        source,
        "\
(deftemplate result (slot val))
(deffacts init (trigger))

(defrule loop-test
    (trigger)
    =>
    (bind ?sum 0)
    (loop-for-count (?i 1 {n})
        (bind ?sum (+ ?sum ?i)))
    (assert (result (val ?sum))))"
    )
    .unwrap();
    source
}

/// String operations at scale: rules concatenate string fragments.
fn generate_string_source(n: usize) -> String {
    let mut source = String::from(
        "\
(deftemplate word (slot id) (slot text))
(deftemplate fragment (slot id) (slot text))

(deffacts words\n",
    );

    let words = [
        "hello", "world", "bench", "clips", "rules", "test", "data", "fast",
    ];
    for i in 0..n {
        let word = words[i % words.len()];
        writeln!(source, "    (word (id {i}) (text \"{word}\"))").unwrap();
    }

    source.push_str(
        ")

(defrule concat-words
    (word (id ?id) (text ?t))
    (not (fragment (id ?id)))
    =>
    (assert (fragment (id ?id) (text (str-cat ?t \"-\" (sub-string 1 3 ?t))))))
",
    );
    source
}

fn bench_evaluator_arithmetic(c: &mut Criterion) {
    let mut group = c.benchmark_group("eval_arithmetic");

    let source_100 = generate_arithmetic_source(100);
    group.bench_function("eval_arith_100", |b| {
        b.iter(|| {
            let mut engine = Engine::new(EngineConfig::utf8());
            engine.load_str(&source_100).unwrap();
            engine.reset().unwrap();
            engine.run(RunLimit::Unlimited).unwrap()
        });
    });

    let source_500 = generate_arithmetic_source(500);
    group.bench_function("eval_arith_500", |b| {
        b.iter(|| {
            let mut engine = Engine::new(EngineConfig::utf8());
            engine.load_str(&source_500).unwrap();
            engine.reset().unwrap();
            engine.run(RunLimit::Unlimited).unwrap()
        });
    });

    let source_1000 = generate_arithmetic_source(1000);
    group.sample_size(10);
    group.bench_function("eval_arith_1000", |b| {
        b.iter(|| {
            let mut engine = Engine::new(EngineConfig::utf8());
            engine.load_str(&source_1000).unwrap();
            engine.reset().unwrap();
            engine.run(RunLimit::Unlimited).unwrap()
        });
    });

    let source_5000 = generate_arithmetic_source(5000);
    group.bench_function("eval_arith_5000", |b| {
        b.iter(|| {
            let mut engine = Engine::new(EngineConfig::utf8());
            engine.load_str(&source_5000).unwrap();
            engine.reset().unwrap();
            engine.run(RunLimit::Unlimited).unwrap()
        });
    });

    group.finish();
}

fn bench_evaluator_deffunction(c: &mut Criterion) {
    let mut group = c.benchmark_group("eval_deffunction");

    let source_100 = generate_deffunction_source(100);
    group.bench_function("eval_defun_100", |b| {
        b.iter(|| {
            let mut engine = Engine::new(EngineConfig::utf8());
            engine.load_str(&source_100).unwrap();
            engine.reset().unwrap();
            engine.run(RunLimit::Unlimited).unwrap()
        });
    });

    let source_1000 = generate_deffunction_source(1000);
    group.bench_function("eval_defun_1000", |b| {
        b.iter(|| {
            let mut engine = Engine::new(EngineConfig::utf8());
            engine.load_str(&source_1000).unwrap();
            engine.reset().unwrap();
            engine.run(RunLimit::Unlimited).unwrap()
        });
    });

    let source_10000 = generate_deffunction_source(10000);
    group.sample_size(10);
    group.bench_function("eval_defun_10000", |b| {
        b.iter(|| {
            let mut engine = Engine::new(EngineConfig::utf8());
            engine.load_str(&source_10000).unwrap();
            engine.reset().unwrap();
            engine.run(RunLimit::Unlimited).unwrap()
        });
    });

    group.finish();
}

fn bench_evaluator_loop(c: &mut Criterion) {
    let mut group = c.benchmark_group("eval_loop");

    let source_1000 = generate_loop_source(1000);
    group.bench_function("eval_loop_1000", |b| {
        b.iter(|| {
            let mut engine = Engine::new(EngineConfig::utf8());
            engine.load_str(&source_1000).unwrap();
            engine.reset().unwrap();
            engine.run(RunLimit::Unlimited).unwrap()
        });
    });

    let source_10000 = generate_loop_source(10000);
    group.bench_function("eval_loop_10000", |b| {
        b.iter(|| {
            let mut engine = Engine::new(EngineConfig::utf8());
            engine.load_str(&source_10000).unwrap();
            engine.reset().unwrap();
            engine.run(RunLimit::Unlimited).unwrap()
        });
    });

    let source_100000 = generate_loop_source(100_000);
    group.sample_size(10);
    group.bench_function("eval_loop_100000", |b| {
        b.iter(|| {
            let mut engine = Engine::new(EngineConfig::utf8());
            engine.load_str(&source_100000).unwrap();
            engine.reset().unwrap();
            engine.run(RunLimit::Unlimited).unwrap()
        });
    });

    group.finish();
}

fn bench_evaluator_string(c: &mut Criterion) {
    let mut group = c.benchmark_group("eval_string");

    let source_100 = generate_string_source(100);
    group.bench_function("eval_string_100", |b| {
        b.iter(|| {
            let mut engine = Engine::new(EngineConfig::utf8());
            engine.load_str(&source_100).unwrap();
            engine.reset().unwrap();
            engine.run(RunLimit::Unlimited).unwrap()
        });
    });

    let source_500 = generate_string_source(500);
    group.bench_function("eval_string_500", |b| {
        b.iter(|| {
            let mut engine = Engine::new(EngineConfig::utf8());
            engine.load_str(&source_500).unwrap();
            engine.reset().unwrap();
            engine.run(RunLimit::Unlimited).unwrap()
        });
    });

    let source_1000 = generate_string_source(1000);
    group.sample_size(10);
    group.bench_function("eval_string_1000", |b| {
        b.iter(|| {
            let mut engine = Engine::new(EngineConfig::utf8());
            engine.load_str(&source_1000).unwrap();
            engine.reset().unwrap();
            engine.run(RunLimit::Unlimited).unwrap()
        });
    });

    group.finish();
}

criterion_group!(
    benches,
    bench_evaluator_arithmetic,
    bench_evaluator_deffunction,
    bench_evaluator_loop,
    bench_evaluator_string,
);
criterion_main!(benches);
