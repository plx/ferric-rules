#![allow(clippy::format_push_string, clippy::needless_raw_string_hashes)]

use std::fmt::Write as _;

use criterion::{black_box, criterion_group, criterion_main, Criterion};
use ferric_rules_runtime::{Engine, EngineConfig, RunLimit};

fn many_templates_source(template_count: usize, slot_count: usize) -> String {
    let mut source = String::new();
    for template_idx in 0..template_count {
        let _ = write!(source, "(deftemplate t{template_idx}");
        for slot_idx in 0..slot_count {
            let _ = write!(source, " (slot s{slot_idx} (default 0))");
        }
        source.push_str(")\n");
    }
    source
}

fn load_modify_engine() -> Engine {
    let source = r#"
        (deftemplate sensor
            (slot id)
            (slot value))
        (deffacts startup
            (sensor (id 1) (value 10)))
        (defrule bump
            ?f <- (sensor (id 1) (value ?v))
            (test (< ?v 11))
            =>
            (modify ?f (value (+ ?v 1))))
    "#;
    let mut engine = Engine::new(EngineConfig::utf8());
    engine
        .load_str(source)
        .expect("load template modify program");
    engine
}

fn load_wide_modify_engine(slot_count: usize) -> Engine {
    let mut source = String::from("(deftemplate sensor");
    for slot_idx in 0..slot_count {
        source.push_str(&format!(" (slot s{slot_idx})"));
    }
    source.push_str(")\n(deffacts startup (sensor");
    for slot_idx in 0..slot_count {
        if slot_idx == 0 {
            source.push_str(&format!(" (s{slot_idx} FALSE)"));
        } else {
            source.push_str(&format!(" (s{slot_idx} 0)"));
        }
    }
    source.push_str("))\n(defrule bump ?f <- (sensor (s0 FALSE)) => (modify ?f");
    for slot_idx in 0..slot_count {
        if slot_idx == 0 {
            source.push_str(" (s0 TRUE)");
        } else {
            source.push_str(&format!(" (s{slot_idx} {slot_idx})"));
        }
    }
    source.push_str("))");

    let mut engine = Engine::new(EngineConfig::utf8());
    engine
        .load_str(&source)
        .expect("load wide template modify program");
    engine
}

fn load_many_templates_engine(template_count: usize, slot_count: usize) -> Engine {
    let source = many_templates_source(template_count, slot_count);
    let mut engine = Engine::new(EngineConfig::utf8());
    engine
        .load_str(&source)
        .expect("load many templates for registry bench");
    engine
}

fn bench_template_registry(c: &mut Criterion) {
    let load_source = many_templates_source(128, 8);

    c.bench_function("load_many_templates", |b| {
        b.iter(|| {
            let mut engine = Engine::new(EngineConfig::utf8());
            black_box(
                engine
                    .load_str(black_box(&load_source))
                    .expect("load templates"),
            );
        });
    });

    let mut engine = load_modify_engine();
    c.bench_function("template_modify_reset_run", |b| {
        b.iter(|| {
            engine.reset().expect("reset");
            black_box(engine.run(RunLimit::Unlimited).expect("run"));
        });
    });

    let engine = load_many_templates_engine(256, 8);
    c.bench_function("template_registry_list_cycle", |b| {
        b.iter(|| {
            let names = engine.templates();
            black_box(names.last().copied());
            black_box(names.len());
        });
    });

    let mut engine = load_wide_modify_engine(8);
    c.bench_function("template_wide_modify_reset_run", |b| {
        b.iter(|| {
            engine.reset().expect("reset");
            black_box(engine.run(RunLimit::Unlimited).expect("run"));
        });
    });
}

/// Capture an owned fact while retaining its original template contract.
fn bench_owned_template_fact(c: &mut Criterion) {
    for slots in [8, 64] {
        let source = many_templates_source(1, slots);
        c.bench_function(&format!("owned_template_fact_{slots}_slots"), |b| {
            let mut engine = Engine::with_rules(&source).unwrap();
            let id = engine
                .assert_template_slots("t0", [("s0", 42_i64)])
                .unwrap();
            let captured = engine.get_fact_owned(id).unwrap().unwrap();
            for index in 0..slots {
                let value = captured.value(index).unwrap();
                let ferric_rules_runtime::Value::Integer(value) = value.as_value() else {
                    panic!("captured slot must be an integer");
                };
                assert_eq!(*value, if index == 0 { 42 } else { 0 });
            }
            engine.retract(id).unwrap();
            let id = engine.assert(captured.clone()).unwrap();
            // A retained fact survives source removal and reasserts against
            // the same definition; its values remain owned by the capture.
            let recaptured = engine.get_fact_owned(id).unwrap().unwrap();
            for index in 0..slots {
                let value = recaptured.value(index).unwrap();
                let ferric_rules_runtime::Value::Integer(value) = value.as_value() else {
                    panic!("reasserted slot must be an integer");
                };
                assert_eq!(*value, if index == 0 { 42 } else { 0 });
            }
            assert_eq!(engine.fact_count(), 1);
            b.iter(|| black_box(engine.get_fact_owned(black_box(id)).unwrap().unwrap()));
        });
    }
}

/// A first public read near the end of a mostly unexported fact arena must
/// retain cheap sparse access; setup and engine destruction are excluded.
fn bench_first_sparse_host_export(c: &mut Criterion) {
    let mut group = c.benchmark_group("host_first_sparse_export");
    // Cold reads need more independent setups than the inexpensive warm controls.
    group.sample_size(60);
    for count in [1_000, 100_000] {
        let mut source = String::from("(deffacts seed\n");
        for id in 0..count {
            writeln!(source, "(background {id})").unwrap();
        }
        source.push_str("(target 42))");
        let prepare = || {
            let mut engine = Engine::new(EngineConfig::utf8());
            engine.load_str(&source).unwrap();
            engine.reset().unwrap();
            engine
        };
        group.bench_function(count.to_string(), |b| {
            let engine = prepare();
            assert_eq!(engine.fact_count(), count + 1);
            let found = engine.find_facts("target").unwrap();
            assert_eq!(found.len(), 1);
            let ferric_rules_core::Fact::Ordered(fact) = found[0].1 else {
                panic!("target must be ordered");
            };
            assert!(matches!(
                fact.fields.as_slice(),
                [ferric_rules_runtime::Value::Integer(42)]
            ));
            assert_eq!(engine.find_facts("target").unwrap()[0].0, found[0].0);
            drop(found);
            drop(engine);
            b.iter_batched_ref(
                prepare,
                |engine| black_box(engine.find_facts("target").unwrap()[0].0),
                criterion::BatchSize::PerIteration,
            );
        });
    }
    group.finish();
}

fn bench_host_assert_retract(c: &mut Criterion) {
    fn case<const N: usize>(c: &mut Criterion) {
        let values: [i64; N] = std::array::from_fn(|index| i64::try_from(index).unwrap());
        let mut engine = Engine::new(EngineConfig::utf8());
        c.bench_function(&format!("host_assert_retract_{N}_fields"), |b| {
            let id = engine.assert_ordered("item", values).unwrap();
            let ferric_rules_core::Fact::Ordered(fact) = engine.get_fact(id).unwrap().unwrap() else {
                panic!("round-trip input must remain ordered");
            };
            assert_eq!(fact.fields.len(), N);
            for (value, expected) in fact.fields.iter().zip(values) {
                assert!(matches!(value, ferric_rules_runtime::Value::Integer(actual) if *actual == expected));
            }
            engine.retract(id).unwrap();
            assert_eq!(engine.fact_count(), 0);
            b.iter(|| {
                let id = engine.assert_ordered("item", black_box(values)).unwrap();
                engine.retract(id).unwrap();
                black_box(id)
            });
        });
    }
    case::<1>(c);
    case::<8>(c);
}

criterion_group!(
    benches,
    bench_template_registry,
    bench_owned_template_fact,
    bench_first_sparse_host_export,
    bench_host_assert_retract
);
criterion_main!(benches);
