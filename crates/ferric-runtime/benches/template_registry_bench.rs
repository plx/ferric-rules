#![allow(clippy::format_push_string, clippy::needless_raw_string_hashes)]

use std::fmt::Write as _;

use criterion::{black_box, criterion_group, criterion_main, Criterion};
use ferric_runtime::{Engine, EngineConfig, RunLimit};

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

criterion_group!(benches, bench_template_registry);
criterion_main!(benches);
