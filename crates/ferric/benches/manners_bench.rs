use std::fmt::Write as FmtWrite;

use criterion::{criterion_group, criterion_main, Criterion};
use ferric::runtime::{Engine, EngineConfig, RunLimit};

/// Simplified Manners seating benchmark.
///
/// The Manners benchmark assigns N guests to seats at a table subject to
/// the constraint that no two adjacent guests share the same hobby.  This
/// simplified version drives the classic greedy seat-assignment pattern:
///
/// 1. Seat one guest at position 1 (the "seed" rule).
/// 2. Repeatedly extend the seating by choosing any remaining guest whose
///    hobby differs from the guest in the last filled seat.
///
/// It exercises:
///
/// - `deftemplate` with multiple slots
/// - Template fact pattern matching with variable bindings
/// - `retract` + `assert` as the core modification mechanism
/// - Cross-pattern variable sharing (join on `?prev`, `?ph`)
/// - `test` CE for cross-variable inequality (`neq`)
/// - `not` over a template pattern with a variable-bound slot
/// - Multi-rule salience ordering
///
/// Because Ferric does not yet support the compound constraint syntax
/// `?nh&~?ph`, the inequality is expressed as a `test` CE instead.
const HOBBIES: [&str; 4] = ["chess", "hiking", "cooking", "reading"];

fn generate_manners_source(n_guests: usize) -> String {
    let mut source = String::from(
        "\
(deftemplate guest (slot name) (slot hobby))
(deftemplate seating (slot seat) (slot guest))
(deftemplate count (slot value))

(deffacts guests\n",
    );

    for i in 0..n_guests {
        let hobby = HOBBIES[i % HOBBIES.len()];
        writeln!(source, "    (guest (name g{i}) (hobby {hobby}))").unwrap();
    }

    source.push_str(
        "    (count (value 0))
    (phase assign))

(defrule assign-first-seat
    (declare (salience 40))
    (phase assign)
    (guest (name ?n) (hobby ?h))
    (count (value 0))
    =>
    (assert (seating (seat 1) (guest ?n)))
    (assert (count (value 1))))

(defrule assign-next-seat
    (declare (salience 30))
    (phase assign)
    ?c <- (count (value ?v))
    (seating (seat ?v) (guest ?prev))
    (guest (name ?prev) (hobby ?ph))
    (guest (name ?next) (hobby ?nh))
    (test (neq ?nh ?ph))
    (not (seating (seat ?) (guest ?next)))
    =>
    (retract ?c)
    (assert (seating (seat (+ ?v 1)) (guest ?next)))
    (assert (count (value (+ ?v 1)))))
",
    );
    source
}

fn bench_manners_8(c: &mut Criterion) {
    let source = generate_manners_source(8);
    c.bench_function("manners_8_guests", |b| {
        b.iter(|| {
            let mut engine = Engine::new(EngineConfig::utf8());
            engine.load_str(&source).unwrap();
            engine.reset().unwrap();
            engine.run(RunLimit::Unlimited).unwrap()
        });
    });
}

fn bench_manners_16(c: &mut Criterion) {
    let source = generate_manners_source(16);
    c.bench_function("manners_16_guests", |b| {
        b.iter(|| {
            let mut engine = Engine::new(EngineConfig::utf8());
            engine.load_str(&source).unwrap();
            engine.reset().unwrap();
            engine.run(RunLimit::Unlimited).unwrap()
        });
    });
}

fn bench_manners_32(c: &mut Criterion) {
    let source = generate_manners_source(32);
    c.bench_function("manners_32_guests", |b| {
        b.iter(|| {
            let mut engine = Engine::new(EngineConfig::utf8());
            engine.load_str(&source).unwrap();
            engine.reset().unwrap();
            engine.run(RunLimit::Unlimited).unwrap()
        });
    });
}

fn bench_manners_48(c: &mut Criterion) {
    let source = generate_manners_source(48);
    c.bench_function("manners_48_guests", |b| {
        b.iter(|| {
            let mut engine = Engine::new(EngineConfig::utf8());
            engine.load_str(&source).unwrap();
            engine.reset().unwrap();
            engine.run(RunLimit::Unlimited).unwrap()
        });
    });
}

fn bench_manners_64(c: &mut Criterion) {
    let source = generate_manners_source(64);
    let mut group = c.benchmark_group("manners_64");
    group.sample_size(10);
    group.bench_function("manners_64_guests", |b| {
        b.iter(|| {
            let mut engine = Engine::new(EngineConfig::utf8());
            engine.load_str(&source).unwrap();
            engine.reset().unwrap();
            engine.run(RunLimit::Unlimited).unwrap()
        });
    });
    group.finish();
}

fn bench_manners_96(c: &mut Criterion) {
    let source = generate_manners_source(96);
    let mut group = c.benchmark_group("manners_96");
    group.sample_size(10);
    group.bench_function("manners_96_guests", |b| {
        b.iter(|| {
            let mut engine = Engine::new(EngineConfig::utf8());
            engine.load_str(&source).unwrap();
            engine.reset().unwrap();
            engine.run(RunLimit::Unlimited).unwrap()
        });
    });
    group.finish();
}

fn bench_manners_128(c: &mut Criterion) {
    let source = generate_manners_source(128);
    let mut group = c.benchmark_group("manners_128");
    group.sample_size(10);
    group.bench_function("manners_128_guests", |b| {
        b.iter(|| {
            let mut engine = Engine::new(EngineConfig::utf8());
            engine.load_str(&source).unwrap();
            engine.reset().unwrap();
            engine.run(RunLimit::Unlimited).unwrap()
        });
    });
    group.finish();
}

fn bench_manners_256(c: &mut Criterion) {
    let source = generate_manners_source(256);
    let mut group = c.benchmark_group("manners_256");
    group.sample_size(10);
    group.bench_function("manners_256_guests", |b| {
        b.iter(|| {
            let mut engine = Engine::new(EngineConfig::utf8());
            engine.load_str(&source).unwrap();
            engine.reset().unwrap();
            engine.run(RunLimit::Unlimited).unwrap()
        });
    });
    group.finish();
}

fn bench_manners_512(c: &mut Criterion) {
    let source = generate_manners_source(512);
    let mut group = c.benchmark_group("manners_512");
    group.sample_size(10);
    group.bench_function("manners_512_guests", |b| {
        b.iter(|| {
            let mut engine = Engine::new(EngineConfig::utf8());
            engine.load_str(&source).unwrap();
            engine.reset().unwrap();
            engine.run(RunLimit::Unlimited).unwrap()
        });
    });
    group.finish();
}

fn bench_manners_8_run_only(c: &mut Criterion) {
    let source = generate_manners_source(8);
    let mut engine = Engine::new(EngineConfig::utf8());
    engine.load_str(&source).unwrap();
    c.bench_function("manners_8_guests_run_only", |b| {
        b.iter(|| {
            engine.reset().unwrap();
            engine.run(RunLimit::Unlimited).unwrap()
        });
    });
}

criterion_group!(
    benches,
    bench_manners_8,
    bench_manners_16,
    bench_manners_32,
    bench_manners_48,
    bench_manners_64,
    bench_manners_96,
    bench_manners_128,
    bench_manners_256,
    bench_manners_512,
    bench_manners_8_run_only,
);
criterion_main!(benches);
