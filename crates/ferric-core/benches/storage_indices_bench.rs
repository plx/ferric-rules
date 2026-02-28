use criterion::{black_box, criterion_group, criterion_main, BatchSize, Criterion};
use ferric_core::{
    Activation, ActivationId, ActivationSeq, Agenda, AlphaEntryType, AlphaMemory, AlphaMemoryId,
    AlphaNetwork, BetaMemoryId, BetaNetwork, BindingSet, ExistsMemory, ExistsMemoryId, Fact,
    FactBase, FactId, NccMemory, NccMemoryId, NegativeMemory, NegativeMemoryId, NodeId, RuleId,
    Salience, SlotIndex, StringEncoding, Symbol, SymbolTable, Timestamp, Token, TokenId,
    TokenStore, Value,
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

fn token_ids(count: usize) -> Vec<TokenId> {
    let mut ids = Vec::with_capacity(count);
    let mut temp: SlotMap<TokenId, ()> = SlotMap::with_key();
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

fn bench_alpha_network_reverse_index_cycle(c: &mut Criterion) {
    let mut symbols = SymbolTable::new();
    let relation = symbols
        .intern_symbol("alpha-item", StringEncoding::Ascii)
        .expect("ASCII symbol");

    c.bench_function("alpha_network_reverse_index_cycle", |b| {
        b.iter_batched(
            || {
                let mut network = AlphaNetwork::new();
                let entry = network.create_entry_node(AlphaEntryType::OrderedRelation(relation));
                let _ = network.create_memory(entry);
                network
            },
            |mut network| {
                let mut fact_base = FactBase::new();
                let mut asserted: Vec<(FactId, Fact)> = Vec::with_capacity(512);
                let mut accepted = 0usize;

                for _ in 0..512 {
                    let fact_id = fact_base.assert_ordered(relation, SmallVec::new());
                    let fact = fact_base
                        .get(fact_id)
                        .expect("fact must exist")
                        .fact
                        .clone();
                    accepted += network.assert_fact(fact_id, &fact).len();
                    asserted.push((fact_id, fact));
                }

                black_box(accepted);

                for (fact_id, fact) in asserted {
                    network.retract_fact(fact_id, &fact);
                }
            },
            BatchSize::SmallInput,
        );
    });
}

fn bench_alpha_memory_indexed_slots_cycle(c: &mut Criterion) {
    let mut symbols = SymbolTable::new();
    let relation = symbols
        .intern_symbol("alpha-memory-item", StringEncoding::Ascii)
        .expect("ASCII symbol");

    c.bench_function("alpha_memory_indexed_slots_cycle", |b| {
        b.iter_batched(
            || {
                let mut fact_base = FactBase::new();
                let mut facts = Vec::with_capacity(256);

                for idx in 0..256 {
                    let fact_id = fact_base.assert_ordered(
                        relation,
                        smallvec::smallvec![
                            Value::Integer((idx % 8) as i64),
                            Value::Integer((idx % 16) as i64),
                            Value::Integer((idx % 32) as i64),
                        ],
                    );
                    let fact = fact_base
                        .get(fact_id)
                        .expect("fact must exist")
                        .fact
                        .clone();
                    facts.push((fact_id, fact));
                }

                (fact_base, facts)
            },
            |(fact_base, facts)| {
                let mut memory = AlphaMemory::new(AlphaMemoryId(0));
                memory.request_index(SlotIndex::Ordered(0), &fact_base);
                memory.request_index(SlotIndex::Ordered(1), &fact_base);
                memory.request_index(SlotIndex::Ordered(2), &fact_base);

                for (fact_id, fact) in &facts {
                    memory.insert(*fact_id, fact);
                }

                let lookups = [
                    memory.lookup_by_slot(SlotIndex::Ordered(0), &ferric_core::AtomKey::Integer(3)),
                    memory.lookup_by_slot(SlotIndex::Ordered(1), &ferric_core::AtomKey::Integer(7)),
                    memory
                        .lookup_by_slot(SlotIndex::Ordered(2), &ferric_core::AtomKey::Integer(15)),
                ];
                black_box(lookups);

                for (fact_id, fact) in &facts {
                    memory.remove(*fact_id, fact);
                }

                black_box(memory.len());
            },
            BatchSize::SmallInput,
        );
    });
}

fn bench_beta_fanout_index_cycle(c: &mut Criterion) {
    c.bench_function("beta_fanout_index_cycle", |b| {
        b.iter_batched(
            || {
                let alpha_mems = [
                    AlphaMemoryId(0),
                    AlphaMemoryId(1),
                    AlphaMemoryId(2),
                    AlphaMemoryId(3),
                ];
                let mut beta = BetaNetwork::new(NodeId(100_000));
                let root_id = beta.root_id();

                for &alpha_mem in &alpha_mems {
                    for _ in 0..3 {
                        let _ = beta.create_join_node(root_id, alpha_mem, Vec::new(), Vec::new());
                    }
                    for _ in 0..2 {
                        let _ = beta.create_negative_node(root_id, alpha_mem, Vec::new());
                    }
                    for _ in 0..2 {
                        let _ = beta.create_exists_node(root_id, alpha_mem, Vec::new());
                    }
                }

                (beta, alpha_mems)
            },
            |(beta, alpha_mems)| {
                let mut total = 0usize;

                for _ in 0..4096 {
                    for &alpha_mem in &alpha_mems {
                        let join_nodes: SmallVec<[NodeId; 4]> =
                            SmallVec::from_slice(beta.join_nodes_for_alpha(alpha_mem));
                        let negative_nodes: SmallVec<[NodeId; 4]> =
                            SmallVec::from_slice(beta.negative_nodes_for_alpha(alpha_mem));
                        let exists_nodes: SmallVec<[NodeId; 4]> =
                            SmallVec::from_slice(beta.exists_nodes_for_alpha(alpha_mem));

                        total += join_nodes.len() + negative_nodes.len() + exists_nodes.len();

                        black_box(join_nodes.last().copied());
                        black_box(negative_nodes.last().copied());
                        black_box(exists_nodes.last().copied());
                    }
                }

                black_box(total);
            },
            BatchSize::SmallInput,
        );
    });
}

fn bench_beta_memory_store_cycle(c: &mut Criterion) {
    let token_pool = token_ids(256);

    c.bench_function("beta_memory_store_cycle", |b| {
        b.iter_batched(
            || {
                let mut beta = BetaNetwork::new(NodeId(100_000));
                let root_id = beta.root_id();

                for idx in 0..128 {
                    let alpha_mem = AlphaMemoryId((idx % 8) as u32);
                    let _ = beta.create_join_node(root_id, alpha_mem, Vec::new(), Vec::new());
                }

                let memory_ids: Vec<BetaMemoryId> = beta.memory_ids().collect();
                (beta, memory_ids)
            },
            |(mut beta, memory_ids)| {
                let mut touched = 0usize;

                for round in 0..256 {
                    let token_id = token_pool[round % token_pool.len()];
                    for &memory_id in &memory_ids {
                        if let Some(memory) = beta.get_memory_mut(memory_id) {
                            memory.insert(token_id);
                            memory.remove(token_id);
                        }

                        touched += beta.get_memory(memory_id).map_or(0, |memory| memory.len());
                    }
                }

                black_box(touched);
            },
            BatchSize::SmallInput,
        );
    });
}

fn bench_beta_negative_memory_store_cycle(c: &mut Criterion) {
    let token_pool = token_ids(256);
    let fact_pool = fact_ids(128);

    c.bench_function("beta_negative_memory_store_cycle", |b| {
        b.iter_batched(
            || {
                let mut beta = BetaNetwork::new(NodeId(100_000));
                let root_id = beta.root_id();

                for idx in 0..128 {
                    let alpha_mem = AlphaMemoryId((idx % 8) as u32);
                    let _ = beta.create_negative_node(root_id, alpha_mem, Vec::new());
                }

                let neg_memory_ids: Vec<NegativeMemoryId> = beta.neg_memory_ids().collect();
                (beta, neg_memory_ids)
            },
            |(mut beta, neg_memory_ids)| {
                let mut touched = 0usize;

                for round in 0..256 {
                    let token_id = token_pool[round % token_pool.len()];
                    let fact_id = fact_pool[round % fact_pool.len()];

                    for &neg_memory_id in &neg_memory_ids {
                        if let Some(memory) = beta.get_neg_memory_mut(neg_memory_id) {
                            memory.add_blocker(token_id, fact_id);
                            black_box(memory.remove_blocker(token_id, fact_id));
                        }

                        touched += beta
                            .get_neg_memory(neg_memory_id)
                            .map_or(0, |memory| memory.blocked_count());
                    }
                }

                black_box(touched);
            },
            BatchSize::SmallInput,
        );
    });
}

fn bench_beta_ncc_memory_store_cycle(c: &mut Criterion) {
    let token_pool = token_ids(512);

    c.bench_function("beta_ncc_memory_store_cycle", |b| {
        b.iter_batched(
            || {
                let mut beta = BetaNetwork::new(NodeId(100_000));

                for _ in 0..128 {
                    let _ = beta.allocate_ncc_memory();
                }

                let ncc_memory_ids: Vec<NccMemoryId> = beta.ncc_memory_ids().collect();
                (beta, ncc_memory_ids)
            },
            |(mut beta, ncc_memory_ids)| {
                let mut touched = 0usize;

                for round in 0..256 {
                    let parent_token_id = token_pool[round % token_pool.len()];
                    let result_token_id = token_pool[(round + 257) % token_pool.len()];
                    let passthrough_token_id = token_pool[(round + 129) % token_pool.len()];

                    for &ncc_memory_id in &ncc_memory_ids {
                        if let Some(memory) = beta.get_ncc_memory_mut(ncc_memory_id) {
                            black_box(memory.add_result(parent_token_id, result_token_id));
                            black_box(memory.remove_result(result_token_id));
                            memory.set_unblocked(parent_token_id, passthrough_token_id);
                            black_box(memory.remove_unblocked(parent_token_id));
                        }

                        touched += beta.get_ncc_memory(ncc_memory_id).map_or(0, |memory| {
                            memory.result_count(parent_token_id)
                                + usize::from(memory.get_passthrough(parent_token_id).is_some())
                        });
                    }
                }

                black_box(touched);
            },
            BatchSize::SmallInput,
        );
    });
}

fn bench_negative_memory_outer_index_cycle(c: &mut Criterion) {
    let token_pool = token_ids(256);
    let fact_pool = fact_ids(128);

    c.bench_function("negative_memory_outer_index_cycle", |b| {
        b.iter_batched(
            || NegativeMemory::new(NegativeMemoryId(0)),
            |mut memory| {
                for idx in 0..1024 {
                    memory.add_blocker(
                        token_pool[idx % token_pool.len()],
                        fact_pool[idx % fact_pool.len()],
                    );
                }

                for idx in 0..64 {
                    memory.set_unblocked(
                        token_pool[(idx * 3) % token_pool.len()],
                        token_pool[(idx * 5 + 1) % token_pool.len()],
                    );
                }

                let mut blocked = 0usize;
                for &fact_id in fact_pool.iter().take(16) {
                    blocked += memory.tokens_blocked_by(fact_id).len();
                }
                black_box(blocked);

                for idx in 0..1024 {
                    black_box(memory.remove_blocker(
                        token_pool[idx % token_pool.len()],
                        fact_pool[idx % fact_pool.len()],
                    ));
                }

                for &token_id in token_pool.iter().take(64) {
                    memory.remove_parent_token(token_id);
                }

                black_box(memory.unblocked_count());
            },
            BatchSize::SmallInput,
        );
    });
}

fn bench_exists_memory_outer_index_cycle(c: &mut Criterion) {
    let token_pool = token_ids(256);
    let fact_pool = fact_ids(128);

    c.bench_function("exists_memory_outer_index_cycle", |b| {
        b.iter_batched(
            || ExistsMemory::new(ExistsMemoryId(0)),
            |mut memory| {
                for idx in 0..1024 {
                    black_box(memory.add_support(
                        token_pool[idx % token_pool.len()],
                        fact_pool[idx % fact_pool.len()],
                    ));
                }

                for idx in 0..64 {
                    memory.set_satisfied(
                        token_pool[(idx * 3) % token_pool.len()],
                        token_pool[(idx * 5 + 1) % token_pool.len()],
                    );
                }

                let mut supported = 0usize;
                for &fact_id in fact_pool.iter().take(16) {
                    supported += memory.parents_supported_by(fact_id).len();
                }
                black_box(supported);

                for idx in 0..1024 {
                    black_box(memory.remove_support(
                        token_pool[idx % token_pool.len()],
                        fact_pool[idx % fact_pool.len()],
                    ));
                }

                for &token_id in token_pool.iter().take(64) {
                    memory.remove_parent_token(token_id);
                }

                black_box(memory.is_empty());
            },
            BatchSize::SmallInput,
        );
    });
}

fn bench_ncc_memory_outer_index_cycle(c: &mut Criterion) {
    let token_pool = token_ids(512);

    c.bench_function("ncc_memory_outer_index_cycle", |b| {
        b.iter_batched(
            || NccMemory::new(NccMemoryId(0)),
            |mut memory| {
                for idx in 0..256 {
                    black_box(memory.add_result(token_pool[idx], token_pool[idx + 256]));
                }

                for idx in 0..64 {
                    memory.set_unblocked(token_pool[idx], token_pool[idx + 128]);
                }

                let mut total = 0usize;
                for &token_id in token_pool.iter().take(64) {
                    total += memory.result_count(token_id);
                    total += usize::from(memory.get_passthrough(token_id).is_some());
                }
                black_box(total);

                for &result_token_id in token_pool.iter().skip(256).take(256) {
                    black_box(memory.remove_result(result_token_id));
                }

                for &token_id in token_pool.iter().take(64) {
                    memory.remove_parent_token(token_id);
                }

                black_box(memory.is_empty());
            },
            BatchSize::SmallInput,
        );
    });
}

fn bench_agenda_token_index_cycle(c: &mut Criterion) {
    let token_pool = token_ids(128);

    c.bench_function("agenda_token_index_cycle", |b| {
        b.iter_batched(
            || {
                let mut temp: SlotMap<ActivationId, ()> = SlotMap::with_key();
                (Agenda::new(), temp.insert(()))
            },
            |(mut agenda, placeholder_id)| {
                for idx in 0..1024 {
                    let activation = Activation {
                        id: placeholder_id,
                        rule: RuleId((idx + 1) as u32),
                        token: token_pool[idx % token_pool.len()],
                        salience: Salience::new((idx % 8) as i32),
                        timestamp: Timestamp::new(idx as u64),
                        activation_seq: ActivationSeq::ZERO,
                        recency: SmallVec::new(),
                    };
                    black_box(agenda.add(activation));
                }

                let mut removed = 0usize;
                for &token_id in token_pool.iter().take(32) {
                    removed += agenda.remove_activations_for_token(token_id).len();
                }
                black_box(removed);

                while let Some(activation) = agenda.pop() {
                    black_box(activation.id);
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
    bench_alpha_network_reverse_index_cycle,
    bench_alpha_memory_indexed_slots_cycle,
    bench_beta_fanout_index_cycle,
    bench_beta_memory_store_cycle,
    bench_beta_negative_memory_store_cycle,
    bench_beta_ncc_memory_store_cycle,
    bench_negative_memory_outer_index_cycle,
    bench_exists_memory_outer_index_cycle,
    bench_ncc_memory_outer_index_cycle,
    bench_agenda_token_index_cycle,
);
criterion_main!(benches);
