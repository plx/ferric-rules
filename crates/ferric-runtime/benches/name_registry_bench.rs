use criterion::{black_box, criterion_group, criterion_main, Criterion};
use ferric_runtime::functions::{FunctionEnv, GenericRegistry, GlobalStore, UserFunction};
use ferric_runtime::ModuleId;

fn module_ids(count: usize) -> Vec<ModuleId> {
    (0..count).map(|idx| ModuleId(idx as u32)).collect()
}

fn local_names(count: usize) -> Vec<String> {
    (0..count).map(|idx| format!("name_{idx}")).collect()
}

fn build_function_env(modules: &[ModuleId], names: &[String]) -> FunctionEnv {
    let mut env = FunctionEnv::new();
    for &module in modules {
        for name in names {
            env.register(
                module,
                UserFunction {
                    name: name.clone(),
                    parameters: vec!["x".to_string()],
                    wildcard_parameter: None,
                    body: Vec::new(),
                },
            );
        }
    }
    env
}

fn build_global_store(modules: &[ModuleId], names: &[String]) -> GlobalStore {
    let mut store = GlobalStore::new();
    for &module in modules {
        for (idx, name) in names.iter().enumerate() {
            store.set(module, name, ferric_runtime::Value::Integer(idx as i64));
        }
    }
    store
}

fn build_generic_registry(modules: &[ModuleId], names: &[String]) -> GenericRegistry {
    let mut registry = GenericRegistry::new();
    for &module in modules {
        for name in names {
            registry.register_generic(module, name);
            registry.register_method(
                module,
                name,
                Some(1),
                vec!["x".to_string()],
                vec![Vec::new()],
                None,
                Vec::new(),
            );
        }
    }
    registry
}

fn bench_function_env_lookup(c: &mut Criterion) {
    let modules = module_ids(32);
    let names = local_names(64);
    let env = build_function_env(&modules, &names);

    c.bench_function("function_env_lookup_cycle", |b| {
        b.iter(|| {
            let mut hits = 0usize;
            for _ in 0..8 {
                for &module in &modules {
                    for name in &names {
                        if env.get(module, black_box(name)).is_some() {
                            hits += 1;
                        }
                    }
                }
            }
            black_box(hits);
        });
    });

    c.bench_function("function_env_modules_for_name", |b| {
        let target = &names[names.len() / 2];
        b.iter(|| black_box(env.modules_for_name(black_box(target))));
    });
}

fn bench_global_store_lookup(c: &mut Criterion) {
    let modules = module_ids(32);
    let names = local_names(64);
    let store = build_global_store(&modules, &names);

    c.bench_function("global_store_lookup_cycle", |b| {
        b.iter(|| {
            let mut sum = 0i64;
            for _ in 0..8 {
                for &module in &modules {
                    for name in &names {
                        if let Some(ferric_runtime::Value::Integer(value)) =
                            store.get(module, black_box(name))
                        {
                            sum += value;
                        }
                    }
                }
            }
            black_box(sum);
        });
    });
}

fn bench_generic_registry_lookup(c: &mut Criterion) {
    let modules = module_ids(32);
    let names = local_names(64);
    let registry = build_generic_registry(&modules, &names);

    c.bench_function("generic_registry_lookup_cycle", |b| {
        b.iter(|| {
            let mut hits = 0usize;
            for _ in 0..8 {
                for &module in &modules {
                    for name in &names {
                        if registry.get(module, black_box(name)).is_some() {
                            hits += 1;
                        }
                    }
                }
            }
            black_box(hits);
        });
    });

    c.bench_function("generic_registry_has_method_index", |b| {
        let target = &names[names.len() / 2];
        b.iter(|| {
            let mut hits = 0usize;
            for &module in &modules {
                if registry.has_method_index(module, black_box(target), 1) {
                    hits += 1;
                }
            }
            black_box(hits);
        });
    });
}

criterion_group!(
    benches,
    bench_function_env_lookup,
    bench_global_store_lookup,
    bench_generic_registry_lookup,
);
criterion_main!(benches);
