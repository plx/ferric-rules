mod support;

use std::fmt::Write as FmtWrite;

use criterion::{criterion_group, criterion_main, Criterion};
use ferric_rules::runtime::{Engine, EngineConfig, RunLimit};

fn validate_disjunction(source: &str, _n_alternatives: usize, n_facts: usize) {
    let expected = (0..n_facts)
        .step_by(2)
        .map(|i| i64::try_from(i).unwrap())
        .collect::<Vec<_>>();
    let engine = support::verify_source(source, expected.len());
    assert_eq!(support::template_ids(&engine, "event").len(), n_facts);
    let mut actual = support::template_ids(&engine, "matched")
        .into_iter()
        .map(|id| support::integer(engine.get_fact_slot_by_name(id, "value").unwrap()))
        .collect::<Vec<_>>();
    actual.sort_unstable();
    assert_eq!(actual, expected);
}

fn validate_predicate(source: &str, n_facts: usize) {
    let mut expected = (0..n_facts)
        .filter(|i| (26..75).contains(&(i % 100)))
        .map(|i| format!("s{i}"))
        .collect::<Vec<_>>();
    expected.sort_unstable();
    let engine = support::verify_source(source, expected.len());
    assert_eq!(
        support::template_symbols(&engine, "in-range", "id"),
        expected
    );
}

fn validate_negation(source: &str, n_facts: usize) {
    let matching = (0..n_facts).filter(|i| i % 3 != 1).collect::<Vec<_>>();
    let expected = matching
        .iter()
        .map(|i| format!("cat{}", i % 20))
        .collect::<std::collections::BTreeSet<_>>();
    let engine = support::verify_source(source, matching.len());
    assert_eq!(support::template_ids(&engine, "item").len(), n_facts);
    assert_eq!(
        support::template_symbols(&engine, "active-item", "category"),
        expected.into_iter().collect::<Vec<_>>()
    );
}

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
(deftemplate item (slot id) (slot category) (slot status))
(deftemplate active-item (slot category))

(deffacts items\n",
    );

    let statuses = ["active", "inactive", "pending"];
    for i in 0..n_facts {
        let status = statuses[i % statuses.len()];
        writeln!(
            source,
            "    (item (id {i}) (category cat{}) (status {status}))",
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
        validate_disjunction(&source_4, 4, 200);
        b.iter(|| {
            let mut engine = Engine::new(EngineConfig::utf8());
            engine.load_str(&source_4).unwrap();
            engine.reset().unwrap();
            engine.run(RunLimit::Unlimited).unwrap()
        });
    });

    let source_8 = generate_disjunction_source(8, 200);
    group.bench_function("constraint_disj_8", |b| {
        validate_disjunction(&source_8, 8, 200);
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
        validate_disjunction(&source_16, 16, 200);
        b.iter(|| {
            let mut engine = Engine::new(EngineConfig::utf8());
            engine.load_str(&source_16).unwrap();
            engine.reset().unwrap();
            engine.run(RunLimit::Unlimited).unwrap()
        });
    });

    let source_32 = generate_disjunction_source(32, 200);
    group.bench_function("constraint_disj_32", |b| {
        validate_disjunction(&source_32, 32, 200);
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
        validate_predicate(&source_100, 100);
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
        validate_predicate(&source_500, 500);
        b.iter(|| {
            let mut engine = Engine::new(EngineConfig::utf8());
            engine.load_str(&source_500).unwrap();
            engine.reset().unwrap();
            engine.run(RunLimit::Unlimited).unwrap()
        });
    });

    let source_1000 = generate_predicate_source(1000);
    group.bench_function("constraint_pred_1000", |b| {
        validate_predicate(&source_1000, 1000);
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
        validate_negation(&source_100, 100);
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
        validate_negation(&source_500, 500);
        b.iter(|| {
            let mut engine = Engine::new(EngineConfig::utf8());
            engine.load_str(&source_500).unwrap();
            engine.reset().unwrap();
            engine.run(RunLimit::Unlimited).unwrap()
        });
    });

    let source_1000 = generate_negation_constraint_source(1000);
    group.bench_function("constraint_neg_1000", |b| {
        validate_negation(&source_1000, 1000);
        b.iter(|| {
            let mut engine = Engine::new(EngineConfig::utf8());
            engine.load_str(&source_1000).unwrap();
            engine.reset().unwrap();
            engine.run(RunLimit::Unlimited).unwrap()
        });
    });

    group.finish();
}

#[derive(Clone, Copy, Debug)]
enum RuntimeIndexCase {
    Missing,
    Late,
    Replacement,
}

fn runtime_index_setup(
    size: usize,
    case: RuntimeIndexCase,
) -> (Engine, Option<ferric_rules::runtime::FactHandle>) {
    let mut engine = Engine::new(EngineConfig::utf8());
    engine
        .load_str(
            r"
        (defglobal ?*calls* = 0)
        (deffunction supports (?value ?limit)
          (bind ?*calls* (+ ?*calls* 1))
          (> ?value ?limit))
        (defrule negative
          (anchor ?key ?limit)
          (not (data ?key ?value&:(supports ?value ?limit)))
          =>)
    ",
        )
        .unwrap();
    engine.reset().unwrap();
    let count = i64::try_from(size).unwrap();
    let selected = if matches!(case, RuntimeIndexCase::Replacement) {
        Some(engine.assert_ordered("data", [-1, count * 2]).unwrap())
    } else {
        None
    };
    for key in 0..count {
        engine.assert_ordered("data", [key, count * 2]).unwrap();
    }
    if matches!(case, RuntimeIndexCase::Replacement) {
        engine.assert_ordered("data", [-1, count * 3]).unwrap();
        for index in 0..count {
            engine.assert_ordered("anchor", [-1, index + 1]).unwrap();
        }
    }
    (engine, selected)
}

fn exercise_runtime_index(
    engine: &mut Engine,
    selected: Option<ferric_rules::runtime::FactHandle>,
    size: usize,
    case: RuntimeIndexCase,
) {
    let count = i64::try_from(size).unwrap();
    if let Some(selected) = selected {
        engine.retract(selected).unwrap();
    } else {
        for index in 0..count {
            let key = if matches!(case, RuntimeIndexCase::Missing) {
                count + index
            } else {
                count - 1
            };
            engine.assert_ordered("anchor", [key, index + 1]).unwrap();
        }
    }
    let result = engine.run(RunLimit::Unlimited).unwrap();
    assert_eq!(
        result.rules_fired,
        if matches!(case, RuntimeIndexCase::Missing) {
            size
        } else {
            0
        }
    );
    assert!(engine.action_diagnostics().is_empty());
}

fn bench_runtime_negative_index(c: &mut Criterion) {
    let mut group = c.benchmark_group("runtime_negative_index");
    group.sample_size(10);
    for size in [256, 1024] {
        for case in [
            RuntimeIndexCase::Missing,
            RuntimeIndexCase::Late,
            RuntimeIndexCase::Replacement,
        ] {
            // Validate membership and callback count outside measurement first.
            let (mut checked, selected) = runtime_index_setup(size, case);
            exercise_runtime_index(&mut checked, selected, size, case);
            let expected_calls = match case {
                RuntimeIndexCase::Missing => 0,
                RuntimeIndexCase::Late => size,
                RuntimeIndexCase::Replacement => size * 2,
            };
            assert!(matches!(
                checked.get_global("calls"),
                Some(ferric_rules::core::Value::Integer(actual))
                    if *actual == i64::try_from(expected_calls).unwrap()
            ));
            // Release the validation fixture before Criterion starts sampling.
            drop(checked);
            group.bench_function(format!("{case:?}_{size}"), |b| {
                // Borrow each input so Engine teardown is outside measurement.
                b.iter_batched_ref(
                    || runtime_index_setup(size, case),
                    |(engine, selected)| {
                        exercise_runtime_index(engine, selected.take(), size, case);
                    },
                    criterion::BatchSize::LargeInput,
                );
            });
        }
    }
    group.finish();
}

criterion_group!(
    benches,
    bench_constraint_disjunction,
    bench_constraint_predicate,
    bench_constraint_negation,
    bench_runtime_negative_index,
);
criterion_main!(benches);
