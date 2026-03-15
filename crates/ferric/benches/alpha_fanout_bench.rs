use std::fmt::Write as FmtWrite;

use criterion::{criterion_group, criterion_main, Criterion};
use ferric::runtime::{Engine, EngineConfig, RunLimit};

/// Alpha network fan-out benchmark: many rules sharing the same template type.
///
/// When a fact enters the alpha network, it must be tested against all
/// constant-test chains for its type. With R rules on the same template,
/// each fact assertion traverses R branches. This detects whether alpha
/// entry-node routing is O(R) (linear scan) or O(1) (indexed).
fn generate_alpha_fanout_source(n_rules: usize, n_facts: usize) -> String {
    let mut source =
        String::from("(deftemplate event (slot type) (slot priority) (slot source))\n\n");

    // Generate R rules, each matching a different constant on the `type` slot
    for i in 0..n_rules {
        writeln!(
            source,
            "(defrule handle-type-{i}\n    (event (type t{i}) (priority ?p) (source ?s))\n    =>\n    (assert (handled-{i} ?p)))\n"
        )
        .unwrap();
    }

    // Generate N event facts cycling through rule types
    source.push_str("(deffacts events\n");
    for i in 0..n_facts {
        let type_idx = i % n_rules;
        let src_idx = i % 10;
        writeln!(
            source,
            "    (event (type t{type_idx}) (priority {i}) (source src{src_idx}))"
        )
        .unwrap();
    }
    source.push_str(")\n");
    source
}

fn bench_alpha_fanout_10r_100f(c: &mut Criterion) {
    let source = generate_alpha_fanout_source(10, 100);
    c.bench_function("alpha_fanout_10r_100f", |b| {
        b.iter(|| {
            let mut engine = Engine::new(EngineConfig::utf8());
            engine.load_str(&source).unwrap();
            engine.reset().unwrap();
            engine.run(RunLimit::Unlimited).unwrap()
        });
    });
}

fn bench_alpha_fanout_50r_500f(c: &mut Criterion) {
    let source = generate_alpha_fanout_source(50, 500);
    c.bench_function("alpha_fanout_50r_500f", |b| {
        b.iter(|| {
            let mut engine = Engine::new(EngineConfig::utf8());
            engine.load_str(&source).unwrap();
            engine.reset().unwrap();
            engine.run(RunLimit::Unlimited).unwrap()
        });
    });
}

fn bench_alpha_fanout_100r_1000f(c: &mut Criterion) {
    let source = generate_alpha_fanout_source(100, 1000);
    let mut group = c.benchmark_group("alpha_fanout_100r_1000f");
    group.sample_size(10);
    group.bench_function("alpha_fanout_100r_1000f", |b| {
        b.iter(|| {
            let mut engine = Engine::new(EngineConfig::utf8());
            engine.load_str(&source).unwrap();
            engine.reset().unwrap();
            engine.run(RunLimit::Unlimited).unwrap()
        });
    });
    group.finish();
}

fn bench_alpha_fanout_200r_2000f(c: &mut Criterion) {
    let source = generate_alpha_fanout_source(200, 2000);
    let mut group = c.benchmark_group("alpha_fanout_200r_2000f");
    group.sample_size(10);
    group.bench_function("alpha_fanout_200r_2000f", |b| {
        b.iter(|| {
            let mut engine = Engine::new(EngineConfig::utf8());
            engine.load_str(&source).unwrap();
            engine.reset().unwrap();
            engine.run(RunLimit::Unlimited).unwrap()
        });
    });
    group.finish();
}

fn bench_alpha_fanout_500r_5000f(c: &mut Criterion) {
    let source = generate_alpha_fanout_source(500, 5000);
    let mut group = c.benchmark_group("alpha_fanout_500r_5000f");
    group.sample_size(10);
    group.bench_function("alpha_fanout_500r_5000f", |b| {
        b.iter(|| {
            let mut engine = Engine::new(EngineConfig::utf8());
            engine.load_str(&source).unwrap();
            engine.reset().unwrap();
            engine.run(RunLimit::Unlimited).unwrap()
        });
    });
    group.finish();
}

fn bench_alpha_fanout_10r_100f_run_only(c: &mut Criterion) {
    let source = generate_alpha_fanout_source(10, 100);
    let mut engine = Engine::new(EngineConfig::utf8());
    engine.load_str(&source).unwrap();
    c.bench_function("alpha_fanout_10r_100f_run_only", |b| {
        b.iter(|| {
            engine.reset().unwrap();
            engine.run(RunLimit::Unlimited).unwrap()
        });
    });
}

criterion_group!(
    benches,
    bench_alpha_fanout_10r_100f,
    bench_alpha_fanout_50r_500f,
    bench_alpha_fanout_100r_1000f,
    bench_alpha_fanout_200r_2000f,
    bench_alpha_fanout_500r_5000f,
    bench_alpha_fanout_10r_100f_run_only,
);
criterion_main!(benches);
