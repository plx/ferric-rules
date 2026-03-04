use std::fmt::Write as FmtWrite;

use criterion::{criterion_group, criterion_main, Criterion};
use ferric::runtime::{Engine, EngineConfig, RunLimit};

/// Simplified Waltz line-labeling benchmark.
///
/// The Waltz algorithm assigns labels (convex, concave, boundary) to edges
/// in a line drawing based on the junction types at each endpoint.  This
/// simplified version encodes a scene with junctions and fires labeling
/// rules for each unknown edge until all edges are labeled.  It exercises:
///
/// - `deftemplate` with a slot that has a `default` value
/// - Template fact pattern matching (slot variable bindings)
/// - `modify` to update a slot value in place
/// - `not (edge (label unknown))` — negation over a template pattern
/// - Multi-rule salience ordering
const JUNCTION_TYPES: [&str; 3] = ["L", "T", "fork"];

/// Generate a Waltz scene with `n_junctions` junctions connected in a mesh.
/// Each junction connects to the next, and additional cross-edges are added
/// to create a denser graph.
fn generate_waltz_source(n_junctions: usize) -> String {
    let mut source = String::from(
        "\
(deftemplate edge (slot p1) (slot p2) (slot label (default unknown)))
(deftemplate junction (slot name) (slot type))

(deffacts scene\n",
    );

    // Generate junctions with cycling types
    for i in 0..n_junctions {
        let jtype = JUNCTION_TYPES[i % JUNCTION_TYPES.len()];
        writeln!(source, "    (junction (name j{i}) (type {jtype}))").unwrap();
    }

    // Generate edges: sequential chain + cross-links
    // Chain: j0-j1, j1-j2, ..., j(n-2)-j(n-1)
    for i in 0..n_junctions.saturating_sub(1) {
        writeln!(source, "    (edge (p1 j{i}) (p2 j{}))", i + 1).unwrap();
    }
    // Cross-links: every 3rd junction connects to junction 2 ahead (if it exists)
    for i in (0..n_junctions.saturating_sub(2)).step_by(3) {
        writeln!(source, "    (edge (p1 j{i}) (p2 j{}))", i + 2).unwrap();
    }
    // Close the loop for larger scenes
    if n_junctions > 3 {
        writeln!(source, "    (edge (p1 j{}) (p2 j0))", n_junctions - 1).unwrap();
    }

    source.push_str(
        "    (phase label))

(defrule label-L-junction
    (declare (salience 10))
    (phase label)
    (junction (name ?j) (type L))
    ?e <- (edge (p1 ?j) (p2 ?) (label unknown))
    =>
    (modify ?e (label convex)))

(defrule label-T-junction
    (declare (salience 10))
    (phase label)
    (junction (name ?j) (type T))
    ?e <- (edge (p1 ?j) (p2 ?) (label unknown))
    =>
    (modify ?e (label boundary)))

(defrule label-fork-junction
    (declare (salience 10))
    (phase label)
    (junction (name ?j) (type fork))
    ?e <- (edge (p1 ?j) (p2 ?) (label unknown))
    =>
    (modify ?e (label concave)))

(defrule done-labeling
    (declare (salience -10))
    (phase label)
    (not (edge (label unknown)))
    =>
    (printout t \"Labeling complete\" crlf))
",
    );
    source
}

fn bench_waltz_5(c: &mut Criterion) {
    let source = generate_waltz_source(5);
    c.bench_function("waltz_5_junctions", |b| {
        b.iter(|| {
            let mut engine = Engine::new(EngineConfig::utf8());
            engine.load_str(&source).unwrap();
            engine.reset().unwrap();
            engine.run(RunLimit::Unlimited).unwrap()
        });
    });
}

fn bench_waltz_10(c: &mut Criterion) {
    let source = generate_waltz_source(10);
    c.bench_function("waltz_10_junctions", |b| {
        b.iter(|| {
            let mut engine = Engine::new(EngineConfig::utf8());
            engine.load_str(&source).unwrap();
            engine.reset().unwrap();
            engine.run(RunLimit::Unlimited).unwrap()
        });
    });
}

fn bench_waltz_20(c: &mut Criterion) {
    let source = generate_waltz_source(20);
    c.bench_function("waltz_20_junctions", |b| {
        b.iter(|| {
            let mut engine = Engine::new(EngineConfig::utf8());
            engine.load_str(&source).unwrap();
            engine.reset().unwrap();
            engine.run(RunLimit::Unlimited).unwrap()
        });
    });
}

fn bench_waltz_50(c: &mut Criterion) {
    let source = generate_waltz_source(50);
    c.bench_function("waltz_50_junctions", |b| {
        b.iter(|| {
            let mut engine = Engine::new(EngineConfig::utf8());
            engine.load_str(&source).unwrap();
            engine.reset().unwrap();
            engine.run(RunLimit::Unlimited).unwrap()
        });
    });
}

fn bench_waltz_100(c: &mut Criterion) {
    let source = generate_waltz_source(100);
    c.bench_function("waltz_100_junctions", |b| {
        b.iter(|| {
            let mut engine = Engine::new(EngineConfig::utf8());
            engine.load_str(&source).unwrap();
            engine.reset().unwrap();
            engine.run(RunLimit::Unlimited).unwrap()
        });
    });
}

fn bench_waltz_150(c: &mut Criterion) {
    let source = generate_waltz_source(150);
    c.bench_function("waltz_150_junctions", |b| {
        b.iter(|| {
            let mut engine = Engine::new(EngineConfig::utf8());
            engine.load_str(&source).unwrap();
            engine.reset().unwrap();
            engine.run(RunLimit::Unlimited).unwrap()
        });
    });
}

fn bench_waltz_200(c: &mut Criterion) {
    let source = generate_waltz_source(200);
    let mut group = c.benchmark_group("waltz_200");
    group.sample_size(10);
    group.bench_function("waltz_200_junctions", |b| {
        b.iter(|| {
            let mut engine = Engine::new(EngineConfig::utf8());
            engine.load_str(&source).unwrap();
            engine.reset().unwrap();
            engine.run(RunLimit::Unlimited).unwrap()
        });
    });
    group.finish();
}

fn bench_waltz_300(c: &mut Criterion) {
    let source = generate_waltz_source(300);
    let mut group = c.benchmark_group("waltz_300");
    group.sample_size(10);
    group.bench_function("waltz_300_junctions", |b| {
        b.iter(|| {
            let mut engine = Engine::new(EngineConfig::utf8());
            engine.load_str(&source).unwrap();
            engine.reset().unwrap();
            engine.run(RunLimit::Unlimited).unwrap()
        });
    });
    group.finish();
}

fn bench_waltz_500(c: &mut Criterion) {
    let source = generate_waltz_source(500);
    let mut group = c.benchmark_group("waltz_500");
    group.sample_size(10);
    group.bench_function("waltz_500_junctions", |b| {
        b.iter(|| {
            let mut engine = Engine::new(EngineConfig::utf8());
            engine.load_str(&source).unwrap();
            engine.reset().unwrap();
            engine.run(RunLimit::Unlimited).unwrap()
        });
    });
    group.finish();
}

fn bench_waltz_750(c: &mut Criterion) {
    let source = generate_waltz_source(750);
    let mut group = c.benchmark_group("waltz_750");
    group.sample_size(10);
    group.bench_function("waltz_750_junctions", |b| {
        b.iter(|| {
            let mut engine = Engine::new(EngineConfig::utf8());
            engine.load_str(&source).unwrap();
            engine.reset().unwrap();
            engine.run(RunLimit::Unlimited).unwrap()
        });
    });
    group.finish();
}

fn bench_waltz_1000(c: &mut Criterion) {
    let source = generate_waltz_source(1000);
    let mut group = c.benchmark_group("waltz_1000");
    group.sample_size(10);
    group.bench_function("waltz_1000_junctions", |b| {
        b.iter(|| {
            let mut engine = Engine::new(EngineConfig::utf8());
            engine.load_str(&source).unwrap();
            engine.reset().unwrap();
            engine.run(RunLimit::Unlimited).unwrap()
        });
    });
    group.finish();
}

fn bench_waltz_5_run_only(c: &mut Criterion) {
    let source = generate_waltz_source(5);
    let mut engine = Engine::new(EngineConfig::utf8());
    engine.load_str(&source).unwrap();
    c.bench_function("waltz_5_junctions_run_only", |b| {
        b.iter(|| {
            engine.reset().unwrap();
            engine.run(RunLimit::Unlimited).unwrap()
        });
    });
}

criterion_group!(
    benches,
    bench_waltz_5,
    bench_waltz_10,
    bench_waltz_20,
    bench_waltz_50,
    bench_waltz_100,
    bench_waltz_150,
    bench_waltz_200,
    bench_waltz_300,
    bench_waltz_500,
    bench_waltz_750,
    bench_waltz_1000,
    bench_waltz_5_run_only,
);
criterion_main!(benches);
