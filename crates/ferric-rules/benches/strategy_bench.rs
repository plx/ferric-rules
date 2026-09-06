mod support;

use std::fmt::Write as FmtWrite;

use criterion::{criterion_group, criterion_main, Criterion};
use ferric_rules::core::ConflictResolutionStrategy;
use ferric_rules::runtime::{Engine, EngineConfig, RunLimit};

/// Conflict resolution strategy benchmark.
///
/// All 4 strategies (Depth, Breadth, LEX, MEA) under identical workloads.
/// LEX wraps `Reverse<SmallVec<[Timestamp; 4]>>` in the `AgendaKey`, making
/// every `BTreeMap` operation compare up to 4 timestamps element-by-element.
/// MEA has separate first-recency and rest-recency fields.
/// Many-activations benchmark: N items compete for agenda ordering.
/// Items trigger different subsets of three rules according to priority and
/// category, producing up to 3*N activations for the strategy to sort.
fn generate_many_activations_source(n_items: usize) -> String {
    let mut source = String::from(
        "\
(deftemplate item (slot id) (slot priority) (slot category))
(deftemplate processed-a (slot id))
(deftemplate processed-b (slot id))
(deftemplate processed-c (slot id))

(deffacts items\n",
    );

    let categories = ["alpha", "beta", "gamma", "delta"];
    for i in 0..n_items {
        let cat = categories[i % categories.len()];
        let priority = i % 10;
        writeln!(
            source,
            "    (item (id i{i}) (priority {priority}) (category {cat}))"
        )
        .unwrap();
    }

    source.push_str(
        ")

(defrule process-a
    (declare (salience 10))
    (item (id ?id) (priority ?p))
    (test (> ?p 3))
    (not (processed-a (id ?id)))
    =>
    (assert (processed-a (id ?id))))

(defrule process-b
    (declare (salience 5))
    (item (id ?id) (category ?c))
    (test (neq ?c delta))
    (not (processed-b (id ?id)))
    =>
    (assert (processed-b (id ?id))))

(defrule process-c
    (declare (salience 1))
    (item (id ?id))
    (not (processed-c (id ?id)))
    =>
    (assert (processed-c (id ?id))))
",
    );
    source
}

/// Churn benchmark: assert→modify→retract cycle for N items.
fn generate_churn_source(n_items: usize) -> String {
    let mut source = String::from(
        "\
(deftemplate item (slot id) (slot status (default pending)))

(deffacts items\n",
    );

    for i in 0..n_items {
        writeln!(source, "    (item (id i{i}))").unwrap();
    }

    source.push_str(
        ")

(defrule process
    (declare (salience 10))
    ?f <- (item (id ?id) (status pending))
    =>
    (modify ?f (status done)))

(defrule cleanup
    (declare (salience 5))
    ?f <- (item (id ?id) (status done))
    =>
    (retract ?f))

(defrule finish
    (declare (salience -10))
    (not (item (id ?)))
    =>
    (assert (finished)))
",
    );
    source
}

fn bench_with_strategy(
    group: &mut criterion::BenchmarkGroup<criterion::measurement::WallTime>,
    name: &str,
    source: &str,
    strategy: ConflictResolutionStrategy,
    n_items: usize,
    churn: bool,
) {
    group.bench_function(name, |b| {
        let mut oracle = Engine::new(EngineConfig::utf8().with_strategy(strategy));
        oracle.load_str(source).unwrap();
        if churn {
            support::verify_reset_run(&mut oracle, 2 * n_items + 1);
            assert!(support::template_ids(&oracle, "item").is_empty());
            assert_eq!(oracle.find_facts("finished").unwrap().len(), 1);
        } else {
            let a = (0..n_items).filter(|i| i % 10 > 3).count();
            let b = (0..n_items).filter(|i| i % 4 != 3).count();
            support::verify_reset_run(&mut oracle, a + b + n_items);
            for (relation, indices) in [
                (
                    "processed-a",
                    (0..n_items).filter(|i| i % 10 > 3).collect::<Vec<_>>(),
                ),
                (
                    "processed-b",
                    (0..n_items).filter(|i| i % 4 != 3).collect::<Vec<_>>(),
                ),
                ("processed-c", (0..n_items).collect::<Vec<_>>()),
            ] {
                let mut expected = indices
                    .into_iter()
                    .map(|i| format!("i{i}"))
                    .collect::<Vec<_>>();
                expected.sort_unstable();
                assert_eq!(support::template_symbols(&oracle, relation, "id"), expected);
            }
        }
        b.iter(|| {
            let mut engine = Engine::new(EngineConfig::utf8().with_strategy(strategy));
            engine.load_str(source).unwrap();
            engine.reset().unwrap();
            engine.run(RunLimit::Unlimited).unwrap()
        });
    });
}

fn bench_strategy_activations(c: &mut Criterion) {
    let source = generate_many_activations_source(200);
    let mut group = c.benchmark_group("strategy_activations200");
    group.sample_size(10);
    bench_with_strategy(
        &mut group,
        "depth",
        &source,
        ConflictResolutionStrategy::Depth,
        200,
        false,
    );
    bench_with_strategy(
        &mut group,
        "breadth",
        &source,
        ConflictResolutionStrategy::Breadth,
        200,
        false,
    );
    bench_with_strategy(
        &mut group,
        "lex",
        &source,
        ConflictResolutionStrategy::Lex,
        200,
        false,
    );
    bench_with_strategy(
        &mut group,
        "mea",
        &source,
        ConflictResolutionStrategy::Mea,
        200,
        false,
    );
    group.finish();
}

fn bench_strategy_churn(c: &mut Criterion) {
    let source = generate_churn_source(1000);
    let mut group = c.benchmark_group("strategy_churn1000");
    group.sample_size(10);
    bench_with_strategy(
        &mut group,
        "depth",
        &source,
        ConflictResolutionStrategy::Depth,
        1000,
        true,
    );
    bench_with_strategy(
        &mut group,
        "breadth",
        &source,
        ConflictResolutionStrategy::Breadth,
        1000,
        true,
    );
    bench_with_strategy(
        &mut group,
        "lex",
        &source,
        ConflictResolutionStrategy::Lex,
        1000,
        true,
    );
    bench_with_strategy(
        &mut group,
        "mea",
        &source,
        ConflictResolutionStrategy::Mea,
        1000,
        true,
    );
    group.finish();
}

criterion_group!(benches, bench_strategy_activations, bench_strategy_churn,);
criterion_main!(benches);
