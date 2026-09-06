mod support;

use std::fmt::Write as FmtWrite;

use criterion::{criterion_group, criterion_main, Criterion};
use ferric_rules::runtime::{Engine, EngineConfig, RunLimit};

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

fn validate_workload(source: &str, n_guests: usize) {
    let engine = support::verify_source(source, n_guests);
    assert_eq!(support::template_ids(&engine, "guest").len(), n_guests);
    let counts = support::template_ids(&engine, "count");
    assert_eq!(counts.len(), 1);
    assert_eq!(
        support::integer(engine.get_fact_slot_by_name(counts[0], "value").unwrap()),
        i64::try_from(n_guests).unwrap()
    );
    let mut seating = std::collections::BTreeMap::new();
    let mut guests = std::collections::BTreeSet::new();
    for id in support::template_ids(&engine, "seating") {
        let seat = support::integer(engine.get_fact_slot_by_name(id, "seat").unwrap());
        let guest = support::symbol(&engine, engine.get_fact_slot_by_name(id, "guest").unwrap());
        let guest_index: usize = guest.strip_prefix('g').unwrap().parse().unwrap();
        assert!(guest_index < n_guests);
        assert!(guests.insert(guest_index), "guest was seated twice");
        assert!(
            seating.insert(seat, guest_index).is_none(),
            "seat was assigned twice"
        );
    }
    assert_eq!(
        seating.keys().copied().collect::<Vec<_>>(),
        (1..=i64::try_from(n_guests).unwrap()).collect::<Vec<_>>()
    );
    let guest_order = seating.values().copied().collect::<Vec<_>>();
    for pair in guest_order.windows(2) {
        assert_ne!(
            HOBBIES[pair[0] % HOBBIES.len()],
            HOBBIES[pair[1] % HOBBIES.len()]
        );
    }
}

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
    ?c <- (count (value 0))
    =>
    (retract ?c)
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
        validate_workload(&source, 8);
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
        validate_workload(&source, 16);
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
        validate_workload(&source, 32);
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
        validate_workload(&source, 48);
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
        validate_workload(&source, 64);
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
        validate_workload(&source, 96);
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
        validate_workload(&source, 128);
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
        validate_workload(&source, 256);
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
        validate_workload(&source, 512);
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
        validate_workload(&source, 8);
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
