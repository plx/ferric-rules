//! Comparative benchmarks for engine serialization formats.
//!
//! Measures serialize/deserialize timing and output size across all supported
//! formats, using engines of varying complexity.
//!
//! Run with: `cargo bench -p ferric-runtime --features serde --bench serialization_bench`

use std::fmt::Write;

use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion};
use ferric_runtime::config::EngineConfig;
use ferric_runtime::serialization::SerializationFormat;
use ferric_runtime::Engine;

/// Create a minimal engine (no rules, no facts — just default state).
fn engine_empty() -> Engine {
    Engine::new(EngineConfig::default())
}

/// Create a small engine with a few rules and facts.
fn engine_small() -> Engine {
    let mut e = Engine::new(EngineConfig::default());
    e.load_str(
        r#"
        (deftemplate sensor (slot id (type INTEGER)) (slot value (type FLOAT)))
        (defrule alert
            (sensor (id ?id) (value ?v&:(> ?v 100.0)))
            =>
            (printout t "ALERT " ?id crlf))
        (deffacts initial
            (sensor (id 1) (value 50.0))
            (sensor (id 2) (value 150.0))
            (sensor (id 3) (value 75.0)))
    "#,
    )
    .unwrap();
    e.reset().unwrap();
    e
}

/// Create a medium engine with multiple templates, rules, globals, and functions.
fn engine_medium() -> Engine {
    let mut e = Engine::new(EngineConfig::default());
    e.load_str(
        r#"
        (deftemplate person (slot name) (slot age (type INTEGER)) (slot role))
        (deftemplate task (slot id (type INTEGER)) (slot assignee) (slot priority (type INTEGER)) (slot status))
        (deftemplate project (slot name) (slot budget (type FLOAT)) (slot active))

        (defglobal ?*task-counter* = 0)
        (defglobal ?*alert-threshold* = 100)

        (deffunction next-task-id ()
            (bind ?*task-counter* (+ ?*task-counter* 1))
            ?*task-counter*)

        (defrule assign-high-priority
            (person (name ?n) (role "engineer"))
            (task (id ?id) (priority ?p&:(> ?p 5)) (status "open") (assignee nil))
            =>
            (printout t "Assign task " ?id " to " ?n crlf))

        (defrule escalate
            (task (id ?id) (priority ?p&:(> ?p 8)) (status "open"))
            =>
            (printout t "Escalate task " ?id crlf))

        (defrule budget-check
            (project (name ?pn) (budget ?b&:(< ?b 1000.0)) (active TRUE))
            =>
            (printout t "Low budget: " ?pn crlf))

        (defrule complete-task
            (task (id ?id) (status "done"))
            =>
            (printout t "Task " ?id " completed" crlf))

        (defrule onboard
            (person (name ?n) (role "new"))
            =>
            (printout t "Onboarding " ?n crlf))

        (deffacts people
            (person (name "Alice") (age 30) (role "engineer"))
            (person (name "Bob") (age 25) (role "engineer"))
            (person (name "Charlie") (age 22) (role "new"))
            (person (name "Diana") (age 35) (role "manager")))

        (deffacts tasks
            (task (id 1) (assignee nil) (priority 9) (status "open"))
            (task (id 2) (assignee nil) (priority 3) (status "open"))
            (task (id 3) (assignee nil) (priority 7) (status "open"))
            (task (id 4) (assignee "Alice") (priority 5) (status "done"))
            (task (id 5) (assignee nil) (priority 10) (status "open")))

        (deffacts projects
            (project (name "Alpha") (budget 5000.0) (active TRUE))
            (project (name "Beta") (budget 500.0) (active TRUE))
            (project (name "Gamma") (budget 50000.0) (active FALSE)))
    "#,
    )
    .unwrap();
    e.reset().unwrap();
    e
}

/// Create a large engine with many rules, templates, and repetitive facts.
fn engine_large() -> Engine {
    let mut e = Engine::new(EngineConfig::default());

    // Base schema
    e.load_str(
        r"
        (deftemplate item (slot id (type INTEGER)) (slot category) (slot value (type FLOAT)) (slot status))
        (deftemplate order (slot id (type INTEGER)) (slot item-id (type INTEGER)) (slot qty (type INTEGER)))
        (deftemplate customer (slot id (type INTEGER)) (slot name) (slot tier))
    ",
    )
    .unwrap();

    // Generate 20 rules
    for i in 0..20 {
        let threshold = (i + 1) * 50;
        e.load_str(&format!(
            r#"(defrule rule-{i}
                (item (id ?id) (value ?v&:(> ?v {threshold}.0)) (status "active"))
                =>
                (printout t "Rule {i} matched item " ?id crlf))"#,
        ))
        .unwrap();
    }

    // Generate 100 facts
    let mut facts = String::new();
    facts.push_str("(deffacts bulk\n");
    for i in 0..100 {
        let category = match i % 4 {
            0 => "electronics",
            1 => "clothing",
            2 => "food",
            _ => "tools",
        };
        let value = f64::from(i) * 12.5;
        let status = if i % 3 == 0 { "active" } else { "archived" };
        writeln!(
            facts,
            "    (item (id {i}) (category \"{category}\") (value {value}) (status \"{status}\"))"
        )
        .unwrap();
    }
    for i in 0..30 {
        writeln!(
            facts,
            "    (order (id {i}) (item-id {}) (qty {}))",
            i * 3,
            (i % 10) + 1
        )
        .unwrap();
    }
    for i in 0..20 {
        let tier = match i % 3 {
            0 => "gold",
            1 => "silver",
            _ => "bronze",
        };
        writeln!(
            facts,
            "    (customer (id {i}) (name \"Customer{i}\") (tier \"{tier}\"))"
        )
        .unwrap();
    }
    facts.push(')');
    e.load_str(&facts).unwrap();

    e.reset().unwrap();
    e
}

fn bench_serialize(c: &mut Criterion) {
    let engines: Vec<(&str, Engine)> = vec![
        ("empty", engine_empty()),
        ("small", engine_small()),
        ("medium", engine_medium()),
        ("large", engine_large()),
    ];

    let mut group = c.benchmark_group("serialize");

    for (engine_name, engine) in &engines {
        for &format in SerializationFormat::ALL {
            let id = BenchmarkId::new(format.name(), engine_name);
            group.bench_with_input(id, &(engine, format), |b, (engine, format)| {
                b.iter(|| engine.serialize(*format).unwrap());
            });
        }
    }

    group.finish();
}

fn bench_deserialize(c: &mut Criterion) {
    let engines: Vec<(&str, Engine)> = vec![
        ("empty", engine_empty()),
        ("small", engine_small()),
        ("medium", engine_medium()),
        ("large", engine_large()),
    ];

    let mut group = c.benchmark_group("deserialize");

    for (engine_name, engine) in &engines {
        for &format in SerializationFormat::ALL {
            let bytes = engine.serialize(format).unwrap();
            let id = BenchmarkId::new(format.name(), engine_name);
            group.bench_with_input(id, &(bytes, format), |b, (bytes, format)| {
                b.iter(|| Engine::deserialize(bytes, *format).unwrap());
            });
        }
    }

    group.finish();
}

/// Print a summary table of serialized sizes for each engine × format combination.
fn bench_size_report(c: &mut Criterion) {
    let engines: Vec<(&str, Engine)> = vec![
        ("empty", engine_empty()),
        ("small", engine_small()),
        ("medium", engine_medium()),
        ("large", engine_large()),
    ];

    // Print size report as a side-effect of a trivial benchmark.
    let mut group = c.benchmark_group("size_report");

    println!("\n  Serialized Size Report (bytes)");
    println!("  {:-<65}", "");
    print!("  {:>9} ", "Engine");
    for &format in SerializationFormat::ALL {
        print!("| {:>10} ", format.name());
    }
    println!();
    println!("  {:-<65}", "");

    for (engine_name, engine) in &engines {
        print!("  {engine_name:>9} ");
        for &format in SerializationFormat::ALL {
            let bytes = engine.serialize(format).unwrap();
            print!("| {:>10} ", bytes.len());
        }
        println!();
    }
    println!("  {:-<65}", "");
    println!();

    // Trivial benchmark so criterion doesn't complain about empty groups.
    let engine = engine_small();
    group.bench_function("small_bincode", |b| {
        b.iter(|| engine.serialize(SerializationFormat::Bincode).unwrap());
    });

    group.finish();
}

criterion_group!(
    benches,
    bench_serialize,
    bench_deserialize,
    bench_size_report
);
criterion_main!(benches);
