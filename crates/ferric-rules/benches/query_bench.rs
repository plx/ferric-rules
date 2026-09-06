mod support;

use std::fmt::Write as FmtWrite;

use criterion::{criterion_group, criterion_main, Criterion};
use ferric_rules::core::{Fact, Value};
use ferric_rules::runtime::{Engine, EngineConfig};

/// Host-side template queries over the public fact inspection API. Each category
/// requires a full scan; the workload reports both a count and a value sum.
/// This replaces the invalid historical do-for-all-facts action benchmark.
fn generate_query_source(n_items: usize, n_categories: usize) -> String {
    let mut source =
        String::from("(deftemplate item (slot category) (slot value))\n(deffacts items\n");
    for i in 0..n_items {
        writeln!(
            source,
            "(item (category cat{}) (value {i}))",
            i % n_categories
        )
        .unwrap();
    }
    source.push_str(")\n");
    source
}

fn prepare(source: &str) -> Engine {
    let mut engine = Engine::new(EngineConfig::utf8());
    engine.load_str(source).unwrap();
    engine.reset().unwrap();
    engine
}

fn query_summaries(engine: &Engine, n_categories: usize) -> Vec<(usize, i64)> {
    (0..n_categories)
        .map(|category| {
            let name = format!("cat{category}");
            engine
                .facts()
                .unwrap()
                .filter_map(|(id, fact)| {
                    let Fact::Template(fact) = fact else {
                        return None;
                    };
                    if engine.template_name_by_id(fact.template_id) != Some("item") {
                        return None;
                    }
                    let Value::Symbol(actual_category) =
                        engine.get_fact_slot_by_name(id, "category").unwrap()
                    else {
                        return None;
                    };
                    if engine.resolve_symbol(*actual_category) != Some(name.as_str()) {
                        return None;
                    }
                    let Value::Integer(value) = engine.get_fact_slot_by_name(id, "value").unwrap()
                    else {
                        return None;
                    };
                    Some(*value)
                })
                .fold((0, 0), |(count, sum), value| (count + 1, sum + value))
        })
        .collect()
}

fn validate_workload(engine: &mut Engine, n_items: usize, n_categories: usize) {
    support::verify_run(engine, 0);
    assert_eq!(support::template_ids(engine, "item").len(), n_items);
    let expected = (0..n_categories)
        .map(|category| {
            let values = (category..n_items).step_by(n_categories);
            (
                values.clone().count(),
                values.map(|i| i64::try_from(i).unwrap()).sum(),
            )
        })
        .collect::<Vec<_>>();
    assert_eq!(query_summaries(engine, n_categories), expected);
}

fn bench_api_queries(c: &mut Criterion) {
    for (n_items, n_categories) in [(100, 10), (500, 20), (1_000, 50), (5_000, 100)] {
        let source = generate_query_source(n_items, n_categories);
        c.bench_function(&format!("api_query_load_{n_items}i_{n_categories}c"), |b| {
            validate_workload(&mut prepare(&source), n_items, n_categories);
            b.iter(|| query_summaries(&prepare(&source), n_categories));
        });
        let mut engine = prepare(&source);
        c.bench_function(&format!("api_query_scan_{n_items}i_{n_categories}c"), |b| {
            validate_workload(&mut engine, n_items, n_categories);
            b.iter(|| query_summaries(&engine, n_categories));
        });
    }
}

criterion_group!(benches, bench_api_queries);
criterion_main!(benches);
