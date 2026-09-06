mod support;

use std::fmt::Write as FmtWrite;

use criterion::{criterion_group, criterion_main, Criterion};
use ferric_rules::runtime::{Engine, EngineConfig, RunLimit};

fn validate_arithmetic(source: &str, limit: usize) {
    let mut value = 0_i64;
    let mut firings = 0;
    while value < i64::try_from(limit).unwrap() {
        value = value * 2 + (value - value / 3) + 1;
        firings += 1;
    }
    let engine = support::verify_source(source, firings);
    let ids = support::template_ids(&engine, "counter");
    assert_eq!(ids.len(), 1);
    assert_eq!(
        support::integer(engine.get_fact_slot_by_name(ids[0], "val").unwrap()),
        value
    );
}

fn validate_sum(source: &str, n: usize) {
    let engine = support::verify_source(source, 1);
    let ids = support::template_ids(&engine, "result");
    assert_eq!(ids.len(), 1);
    assert_eq!(
        support::integer(engine.get_fact_slot_by_name(ids[0], "val").unwrap()),
        i64::try_from(n * (n + 1) / 2).unwrap()
    );
}

fn validate_string(source: &str, n: usize) {
    let engine = support::verify_source(source, n);
    let ids = support::template_ids(&engine, "fragment");
    assert_eq!(ids.len(), n);
    let mut seen = std::collections::BTreeSet::new();
    for id in ids {
        let key = usize::try_from(support::integer(
            engine.get_fact_slot_by_name(id, "id").unwrap(),
        ))
        .unwrap();
        assert!(key < n);
        assert!(seen.insert(key));
        let word = WORDS[key % WORDS.len()];
        let ferric_rules::core::Value::String(text) =
            engine.get_fact_slot_by_name(id, "text").unwrap()
        else {
            panic!("fragment text must be a string")
        };
        assert_eq!(text.as_str(), format!("{word}-{}", &word[..3]));
    }
}

fn validate_test_ce(source: &str, n: usize) {
    let expected = (1..n)
        .filter(|i| i % 2 == 0)
        .map(|i| i64::try_from(i).unwrap())
        .collect::<Vec<_>>();
    let engine = support::verify_source(source, expected.len());
    assert_eq!(support::ordered_integers(&engine, "accepted"), expected);
}

const WORDS: [&str; 8] = [
    "hello", "world", "bench", "clips", "rules", "test", "data", "fast",
];

/// Expression evaluator throughput benchmark.
///
/// Exercises the evaluator under varying complexity: arithmetic, user-defined
/// function dispatch, loop constructs, and string operations. Every `test` CE
/// and every RHS argument passes through the evaluator; the dispatch chain
/// (builtins → `FunctionEnv` → `GenericRegistry` → error) adds per-call overhead.
/// Arithmetic RHS: a geometrically increasing counter fires until reaching N.
/// N is a stopping threshold, not a count of activations.
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

/// Deffunction dispatch: iterative sum with a supported global accumulator.
/// Local bind inside deffunctions is not currently supported; old timings that
/// stopped at that evaluation error are not comparable.
fn generate_deffunction_source(n: usize) -> String {
    let mut source = String::new();
    writeln!(
        source,
        "\
(defglobal ?*sum* = 0)
(deffunction compute-sum (?n)
    (bind ?*sum* 0)
    (loop-for-count (?i 1 ?n)
        (bind ?*sum* (+ ?*sum* ?i)))
    ?*sum*)

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

    for i in 0..n {
        let word = WORDS[i % WORDS.len()];
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

/// Match-time `test` CE throughput with an even split of accepted and rejected
/// partial matches.
fn generate_test_ce_source(n: usize) -> String {
    let mut source = String::from(
        "\
(deffacts candidates\n",
    );
    for value in 0..n {
        writeln!(source, "    (candidate {value})").unwrap();
    }
    source.push_str(
        ")

(defrule accept-even-positive
    (candidate ?value)
    (test (and (> ?value 0) (= (mod ?value 2) 0)))
    =>
    (assert (accepted ?value)))
",
    );
    source
}

fn bench_evaluator_arithmetic(c: &mut Criterion) {
    let mut group = c.benchmark_group("eval_arithmetic");

    let source_100 = generate_arithmetic_source(100);
    group.bench_function("eval_arith_100", |b| {
        validate_arithmetic(&source_100, 100);
        b.iter(|| {
            let mut engine = Engine::new(EngineConfig::utf8());
            engine.load_str(&source_100).unwrap();
            engine.reset().unwrap();
            engine.run(RunLimit::Unlimited).unwrap()
        });
    });

    let source_500 = generate_arithmetic_source(500);
    group.bench_function("eval_arith_500", |b| {
        validate_arithmetic(&source_500, 500);
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
        validate_arithmetic(&source_1000, 1000);
        b.iter(|| {
            let mut engine = Engine::new(EngineConfig::utf8());
            engine.load_str(&source_1000).unwrap();
            engine.reset().unwrap();
            engine.run(RunLimit::Unlimited).unwrap()
        });
    });

    let source_5000 = generate_arithmetic_source(5000);
    group.bench_function("eval_arith_5000", |b| {
        validate_arithmetic(&source_5000, 5000);
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
    group.bench_function("eval_defun_global_sum_100", |b| {
        validate_sum(&source_100, 100);
        b.iter(|| {
            let mut engine = Engine::new(EngineConfig::utf8());
            engine.load_str(&source_100).unwrap();
            engine.reset().unwrap();
            engine.run(RunLimit::Unlimited).unwrap()
        });
    });

    let source_1000 = generate_deffunction_source(1000);
    group.bench_function("eval_defun_global_sum_1000", |b| {
        validate_sum(&source_1000, 1000);
        b.iter(|| {
            let mut engine = Engine::new(EngineConfig::utf8());
            engine.load_str(&source_1000).unwrap();
            engine.reset().unwrap();
            engine.run(RunLimit::Unlimited).unwrap()
        });
    });

    let source_10000 = generate_deffunction_source(10000);
    group.sample_size(10);
    group.bench_function("eval_defun_global_sum_10000", |b| {
        validate_sum(&source_10000, 10000);
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
        validate_sum(&source_1000, 1000);
        b.iter(|| {
            let mut engine = Engine::new(EngineConfig::utf8());
            engine.load_str(&source_1000).unwrap();
            engine.reset().unwrap();
            engine.run(RunLimit::Unlimited).unwrap()
        });
    });

    let source_10000 = generate_loop_source(10000);
    group.bench_function("eval_loop_10000", |b| {
        validate_sum(&source_10000, 10000);
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
        validate_sum(&source_100000, 100_000);
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
        validate_string(&source_100, 100);
        b.iter(|| {
            let mut engine = Engine::new(EngineConfig::utf8());
            engine.load_str(&source_100).unwrap();
            engine.reset().unwrap();
            engine.run(RunLimit::Unlimited).unwrap()
        });
    });

    let source_500 = generate_string_source(500);
    group.bench_function("eval_string_500", |b| {
        validate_string(&source_500, 500);
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
        validate_string(&source_1000, 1000);
        b.iter(|| {
            let mut engine = Engine::new(EngineConfig::utf8());
            engine.load_str(&source_1000).unwrap();
            engine.reset().unwrap();
            engine.run(RunLimit::Unlimited).unwrap()
        });
    });

    group.finish();
}

fn bench_test_ce_matching(c: &mut Criterion) {
    let mut group = c.benchmark_group("test_ce_matching");

    let source_100 = generate_test_ce_source(100);
    group.bench_function("test_ce_100", |b| {
        validate_test_ce(&source_100, 100);
        b.iter(|| {
            let mut engine = Engine::new(EngineConfig::utf8());
            engine.load_str(&source_100).unwrap();
            engine.reset().unwrap();
            engine.run(RunLimit::Unlimited).unwrap()
        });
    });

    let source_1000 = generate_test_ce_source(1000);
    group.sample_size(10);
    group.bench_function("test_ce_1000", |b| {
        validate_test_ce(&source_1000, 1000);
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
    bench_test_ce_matching,
);
criterion_main!(benches);
