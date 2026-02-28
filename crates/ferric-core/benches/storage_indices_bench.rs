use criterion::{black_box, criterion_group, criterion_main, BatchSize, Criterion};
use ferric_core::{
    BindingSet, FactBase, FactId, NodeId, StringEncoding, Symbol, SymbolTable, Token, TokenStore,
};
use slotmap::SlotMap;
use smallvec::SmallVec;

fn ascii_symbols(table: &mut SymbolTable, count: usize) -> Vec<Symbol> {
    (0..count)
        .map(|idx| {
            table
                .intern_symbol(&format!("sym_{idx}"), StringEncoding::Ascii)
                .expect("ASCII symbol")
        })
        .collect()
}

fn fact_ids(count: usize) -> Vec<FactId> {
    let mut ids = Vec::with_capacity(count);
    let mut temp: SlotMap<FactId, ()> = SlotMap::with_key();
    for _ in 0..count {
        ids.push(temp.insert(()));
    }
    ids
}

fn bench_symbol_table_ascii_intern(c: &mut Criterion) {
    let names: Vec<String> = (0..128).map(|idx| format!("bench_symbol_{idx}")).collect();

    c.bench_function("symbol_table_ascii_intern_cycle", |b| {
        b.iter_batched(
            SymbolTable::new,
            |mut table| {
                for _ in 0..8 {
                    for name in &names {
                        black_box(
                            table
                                .intern_symbol(name, StringEncoding::Ascii)
                                .expect("ASCII symbol"),
                        );
                    }
                }
                black_box(table.len());
            },
            BatchSize::SmallInput,
        );
    });
}

fn bench_fact_base_relation_index_cycle(c: &mut Criterion) {
    let mut symbols = SymbolTable::new();
    let relations = ascii_symbols(&mut symbols, 32);

    c.bench_function("fact_base_relation_index_cycle", |b| {
        b.iter_batched(
            FactBase::new,
            |mut fact_base| {
                let mut ids = Vec::with_capacity(1024);
                for idx in 0..1024 {
                    let relation = relations[idx % relations.len()];
                    ids.push(fact_base.assert_ordered(relation, SmallVec::new()));
                }

                let total_indexed: usize = relations
                    .iter()
                    .map(|&relation| fact_base.facts_by_relation(relation).count())
                    .sum();
                black_box(total_indexed);

                for id in ids {
                    black_box(fact_base.retract(id));
                }
            },
            BatchSize::SmallInput,
        );
    });
}

fn bench_token_store_reverse_index_cycle(c: &mut Criterion) {
    let fact_pool = fact_ids(256);

    c.bench_function("token_store_reverse_index_cycle", |b| {
        b.iter_batched(
            TokenStore::new,
            |mut token_store| {
                let mut roots = Vec::with_capacity(64);
                let mut chain_tail = None;

                for idx in 0..512 {
                    let parent = if idx % 8 == 0 { None } else { chain_tail };
                    let token = Token {
                        facts: SmallVec::from_buf([
                            fact_pool[idx % fact_pool.len()],
                            fact_pool[(idx * 7) % fact_pool.len()],
                            fact_pool[(idx * 13) % fact_pool.len()],
                            fact_pool[(idx * 17) % fact_pool.len()],
                        ]),
                        bindings: BindingSet::new(),
                        parent,
                        owner_node: NodeId((idx % 16) as u32),
                    };
                    let token_id = token_store.insert(token);
                    if parent.is_none() {
                        roots.push(token_id);
                    }
                    chain_tail = Some(token_id);
                }

                let touched: usize = fact_pool
                    .iter()
                    .take(16)
                    .map(|&fact_id| token_store.tokens_containing(fact_id).count())
                    .sum();
                black_box(touched);

                for root in roots {
                    black_box(token_store.remove_cascade(root).len());
                }
            },
            BatchSize::SmallInput,
        );
    });
}

criterion_group!(
    benches,
    bench_symbol_table_ascii_intern,
    bench_fact_base_relation_index_cycle,
    bench_token_store_reverse_index_cycle,
);
criterion_main!(benches);
