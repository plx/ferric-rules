use std::fmt::Write as FmtWrite;

use criterion::{criterion_group, criterion_main, Criterion};
use ferric::runtime::{Engine, EngineConfig, RunLimit};

/// Compound constraint benchmark: disjunctive (|), predicate (:), and
/// negation (~) constraints.
///
/// Tests `EqualAny` linear scan cost for disjunctions and alpha-level
/// expression evaluation overhead for predicate constraints.
/// Disjunctive constraint: rule matches events whose type is one of D
/// alternatives. Half the fact types match, half don't.
fn generate_disjunction_source(n_alternatives: usize, n_facts: usize) -> String {
    let mut source = String::from(
        "\
(deftemplate event (slot type) (slot value))
(deftemplate matched (slot value))

(deffacts events\n",
    );

    // Facts cycle through types t0..t{2*D-1}; only even types match
    let total_types = n_alternatives * 2;
    for i in 0..n_facts {
        let type_idx = i % total_types;
        writeln!(source, "    (event (type t{type_idx}) (value {i}))").unwrap();
    }
    source.push_str(")\n\n");

    // Rule with disjunctive constraint matching only even-numbered types
    source.push_str("(defrule disjunctive-match\n    (event (type ");
    for i in 0..n_alternatives {
        if i > 0 {
            source.push('|');
        }
        write!(source, "t{}", i * 2).unwrap();
    }
    source.push_str(") (value ?v))\n    =>\n    (assert (matched (value ?v))))\n");
    source
}

/// Predicate constraint: rule matches sensors whose value is in range (25, 75).
fn generate_predicate_source(n_facts: usize) -> String {
    let mut source = String::from(
        "\
(deftemplate sensor (slot id) (slot value))
(deftemplate in-range (slot id))

(deffacts readings\n",
    );

    for i in 0..n_facts {
        let value = i % 100;
        writeln!(source, "    (sensor (id s{i}) (value {value}))").unwrap();
    }

    source.push_str(
        ")

(defrule range-check
    (sensor (id ?id) (value ?v&:(> ?v 25)&:(< ?v 75)))
    =>
    (assert (in-range (id ?id))))
",
    );
    source
}

/// Negation constraint: rule matches items whose status is NOT inactive.
fn generate_negation_constraint_source(n_facts: usize) -> String {
    let mut source = String::from(
        "\
(deftemplate item (slot category) (slot status))
(deftemplate active-item (slot category))

(deffacts items\n",
    );

    let statuses = ["active", "inactive", "pending"];
    for i in 0..n_facts {
        let status = statuses[i % statuses.len()];
        writeln!(
            source,
            "    (item (category cat{}) (status {status}))",
            i % 20
        )
        .unwrap();
    }

    source.push_str(
        ")

(defrule find-active
    (item (category ?c) (status ~inactive))
    =>
    (assert (active-item (category ?c))))
",
    );
    source
}

fn bench_constraint_disjunction(c: &mut Criterion) {
    let mut group = c.benchmark_group("constraint_disjunction");

    let source_4 = generate_disjunction_source(4, 200);
    group.bench_function("constraint_disj_4", |b| {
        b.iter(|| {
            let mut engine = Engine::new(EngineConfig::utf8());
            engine.load_str(&source_4).unwrap();
            engine.reset().unwrap();
            engine.run(RunLimit::Unlimited).unwrap()
        });
    });

    let source_8 = generate_disjunction_source(8, 200);
    group.bench_function("constraint_disj_8", |b| {
        b.iter(|| {
            let mut engine = Engine::new(EngineConfig::utf8());
            engine.load_str(&source_8).unwrap();
            engine.reset().unwrap();
            engine.run(RunLimit::Unlimited).unwrap()
        });
    });

    let source_16 = generate_disjunction_source(16, 200);
    group.sample_size(10);
    group.bench_function("constraint_disj_16", |b| {
        b.iter(|| {
            let mut engine = Engine::new(EngineConfig::utf8());
            engine.load_str(&source_16).unwrap();
            engine.reset().unwrap();
            engine.run(RunLimit::Unlimited).unwrap()
        });
    });

    let source_32 = generate_disjunction_source(32, 200);
    group.bench_function("constraint_disj_32", |b| {
        b.iter(|| {
            let mut engine = Engine::new(EngineConfig::utf8());
            engine.load_str(&source_32).unwrap();
            engine.reset().unwrap();
            engine.run(RunLimit::Unlimited).unwrap()
        });
    });

    group.finish();
}

fn bench_constraint_predicate(c: &mut Criterion) {
    let mut group = c.benchmark_group("constraint_predicate");

    let source_100 = generate_predicate_source(100);
    group.bench_function("constraint_pred_100", |b| {
        b.iter(|| {
            let mut engine = Engine::new(EngineConfig::utf8());
            engine.load_str(&source_100).unwrap();
            engine.reset().unwrap();
            engine.run(RunLimit::Unlimited).unwrap()
        });
    });

    let source_500 = generate_predicate_source(500);
    group.sample_size(10);
    group.bench_function("constraint_pred_500", |b| {
        b.iter(|| {
            let mut engine = Engine::new(EngineConfig::utf8());
            engine.load_str(&source_500).unwrap();
            engine.reset().unwrap();
            engine.run(RunLimit::Unlimited).unwrap()
        });
    });

    let source_1000 = generate_predicate_source(1000);
    group.bench_function("constraint_pred_1000", |b| {
        b.iter(|| {
            let mut engine = Engine::new(EngineConfig::utf8());
            engine.load_str(&source_1000).unwrap();
            engine.reset().unwrap();
            engine.run(RunLimit::Unlimited).unwrap()
        });
    });

    group.finish();
}

fn bench_constraint_negation(c: &mut Criterion) {
    let mut group = c.benchmark_group("constraint_negation");

    let source_100 = generate_negation_constraint_source(100);
    group.bench_function("constraint_neg_100", |b| {
        b.iter(|| {
            let mut engine = Engine::new(EngineConfig::utf8());
            engine.load_str(&source_100).unwrap();
            engine.reset().unwrap();
            engine.run(RunLimit::Unlimited).unwrap()
        });
    });

    let source_500 = generate_negation_constraint_source(500);
    group.sample_size(10);
    group.bench_function("constraint_neg_500", |b| {
        b.iter(|| {
            let mut engine = Engine::new(EngineConfig::utf8());
            engine.load_str(&source_500).unwrap();
            engine.reset().unwrap();
            engine.run(RunLimit::Unlimited).unwrap()
        });
    });

    let source_1000 = generate_negation_constraint_source(1000);
    group.bench_function("constraint_neg_1000", |b| {
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
    bench_constraint_disjunction,
    bench_constraint_predicate,
    bench_constraint_negation,
);
criterion_main!(benches);
