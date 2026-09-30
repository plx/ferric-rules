mod support;

use std::fmt::Write as FmtWrite;

use criterion::{criterion_group, criterion_main, BatchSize, BenchmarkId, Criterion};
use ferric_rules::core::{Fact, FerricString, Multifield, StringEncoding, Value};
use ferric_rules::runtime::{Engine, EngineConfig, RunLimit};

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

fn validate_workload(source: &str, width: usize, n_keys: usize) {
    let engine = support::verify_source(source, n_keys);
    assert_eq!(
        support::template_symbols(&engine, "result", "key"),
        support::expected_symbols("k", n_keys)
    );
    for id in support::template_ids(&engine, "result") {
        assert_eq!(
            support::symbol(
                &engine,
                engine.get_fact_slot_by_name(id, "matched").unwrap()
            ),
            "yes"
        );
    }
    for layer in 0..width {
        assert_eq!(
            support::template_ids(&engine, &format!("layer-{layer}")).len(),
            n_keys
        );
    }
}

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
        validate_workload(&source, 3, N_KEYS);
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
        validate_workload(&source, 5, N_KEYS);
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
        validate_workload(&source, 7, N_KEYS);
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
        validate_workload(&source, 9, N_KEYS);
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
        validate_workload(&source, 11, N_KEYS);
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
        validate_workload(&source, 13, N_KEYS);
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
        validate_workload(&source, 15, N_KEYS);
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
        validate_workload(&source, 17, N_KEYS);
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
        validate_workload(&source, 19, N_KEYS);
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
        validate_workload(&source, 21, N_KEYS);
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
        validate_workload(&source, 3, N_KEYS);
        b.iter(|| {
            engine.reset().unwrap();
            engine.run(RunLimit::Unlimited).unwrap()
        });
    });
}

const PAYLOAD_RULE: &str = "
    (defrule join-payload
        (left ?key ?payload)
        (middle ?key ?middle)
        (right ?key ?right)
        => (assert (result ?key ?payload)))";

fn payload(key: usize, nested: bool) -> Value {
    let text = Value::String(
        FerricString::new(
            &format!(
                "key-{key}: UTF-8 payload café — {}",
                "shared-value-content/".repeat(6)
            ),
            StringEncoding::Utf8,
        )
        .unwrap(),
    );
    if nested {
        let inner = Value::Multifield(Box::new(
            [text.clone(), Value::Integer(i64::try_from(key).unwrap())]
                .into_iter()
                .collect::<Multifield>(),
        ));
        Value::Multifield(Box::new(
            [text, inner.clone(), inner]
                .into_iter()
                .collect::<Multifield>(),
        ))
    } else {
        text
    }
}

fn prepare_payload_join(n_keys: usize, nested: bool) -> Engine {
    let mut engine = Engine::new(EngineConfig::utf8());
    engine.load_str(PAYLOAD_RULE).unwrap();
    engine.reset().unwrap();
    for key in 0..n_keys {
        let value = payload(key, nested);
        for relation in ["left", "middle", "right"] {
            engine
                .assert_ordered(
                    relation,
                    vec![Value::Integer(i64::try_from(key).unwrap()), value.clone()],
                )
                .unwrap();
        }
    }
    engine
}

fn validate_payload_join(n_keys: usize, nested: bool) {
    let mut engine = prepare_payload_join(n_keys, nested);
    support::verify_run(&mut engine, n_keys);
    let results = engine.find_facts("result").unwrap();
    assert_eq!(results.len(), n_keys);
    let mut keys = std::collections::BTreeSet::new();
    for (_, fact) in results {
        let Fact::Ordered(fact) = fact else {
            unreachable!()
        };
        let key = usize::try_from(support::integer(&fact.fields[0])).unwrap();
        assert!(key < n_keys);
        assert!(keys.insert(key));
        // Ordered RHS assertions splice the outer multifield; its nested
        // members remain values and must survive the join without flattening.
        let expected = match payload(key, nested) {
            Value::Multifield(values) => values.into_iter().collect::<Vec<_>>(),
            value => vec![value],
        };
        assert_eq!(fact.fields.len(), expected.len() + 1);
        for (actual, expected) in fact.fields[1..].iter().zip(&expected) {
            assert!(actual.structural_eq(expected));
        }
    }
}

fn bench_payload_joins(c: &mut Criterion) {
    for (kind, nested) in [("strings", false), ("nested_multifields", true)] {
        for n_keys in [100, 1_000] {
            c.bench_function(&format!("join_{kind}_{n_keys}"), |b| {
                validate_payload_join(n_keys, nested);
                b.iter(|| {
                    let mut engine = prepare_payload_join(n_keys, nested);
                    engine.run(RunLimit::Unlimited).unwrap()
                });
            });
        }
    }
}

const PREFIX_RULE: &str = "
    (defrule join-prefix
        (key ?key)
        (row ?key $?tail)
        => (assert (hit ?key)))";

fn assert_prefix_side(engine: &mut Engine, n_keys: usize, rows: bool) {
    for key in 0..n_keys {
        let key = Value::Integer(i64::try_from(key).unwrap());
        if rows {
            engine
                .assert_ordered("row", vec![key, Value::Integer(7), Value::Integer(9)])
                .unwrap();
        } else {
            engine.assert_ordered("key", vec![key]).unwrap();
        }
    }
}

fn prepare_prefix_join(n_keys: usize, rows_first: bool) -> Engine {
    let mut engine = Engine::new(EngineConfig::utf8());
    engine.load_str(PREFIX_RULE).unwrap();
    engine.reset().unwrap();
    assert_prefix_side(&mut engine, n_keys, rows_first);
    engine
}

/// Include arrival-side assertion: joins propagate before `run` is called.
/// The two arrival orders exercise alpha-fact and parent-token indexes.
fn bench_ordered_prefix_index(c: &mut Criterion) {
    let mut group = c.benchmark_group("ordered_prefix_index");
    for (arrival, rows_first) in [("rows_arrive", false), ("keys_arrive", true)] {
        for n_keys in [128, 512, 2_048] {
            group.bench_with_input(BenchmarkId::new(arrival, n_keys), &n_keys, |b, &n| {
                let mut engine = prepare_prefix_join(n, rows_first);
                assert_prefix_side(&mut engine, n, !rows_first);
                support::verify_run(&mut engine, n);
                assert!(engine.action_diagnostics().is_empty());
                assert_eq!(
                    support::ordered_integers(&engine, "hit"),
                    (0..i64::try_from(n).unwrap()).collect::<Vec<_>>()
                );
                b.iter_batched(
                    || prepare_prefix_join(n, rows_first),
                    |mut engine| {
                        assert_prefix_side(&mut engine, n, !rows_first);
                        engine.run(RunLimit::Unlimited).unwrap();
                        engine
                    },
                    BatchSize::SmallInput,
                );
            });
        }
    }
    group.finish();
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
    bench_payload_joins,
    bench_ordered_prefix_index,
);
criterion_main!(benches);
