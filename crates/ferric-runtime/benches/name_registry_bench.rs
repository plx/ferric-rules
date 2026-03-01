#![allow(
    clippy::cast_possible_truncation,
    clippy::cast_possible_wrap,
    clippy::format_push_string
)]

use criterion::{black_box, criterion_group, criterion_main, Criterion};
use ferric_runtime::functions::{FunctionEnv, GenericRegistry, GlobalStore, UserFunction};
use ferric_runtime::{Engine, EngineConfig, ModuleId, ModuleRegistry};

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

fn build_module_registry(names: &[String]) -> ModuleRegistry {
    let mut registry = ModuleRegistry::new();
    for name in names {
        registry.register(name, Vec::new(), Vec::new());
    }
    registry
}

fn many_deffunctions_source(module_count: usize, names_per_module: usize) -> String {
    let mut source = String::new();
    for module_idx in 0..module_count {
        if module_idx > 0 {
            source.push_str(&format!("(defmodule M{module_idx} (export ?ALL))\n"));
        }
        for name_idx in 0..names_per_module {
            source.push_str(&format!("(deffunction f{name_idx} (?x) ?x)\n"));
        }
    }
    source
}

fn many_defglobals_source(module_count: usize, names_per_module: usize) -> String {
    let mut source = String::new();
    for module_idx in 0..module_count {
        if module_idx > 0 {
            source.push_str(&format!("(defmodule G{module_idx} (export ?ALL))\n"));
        }
        for name_idx in 0..names_per_module {
            source.push_str(&format!("(defglobal ?*g{name_idx}* = {name_idx})\n"));
        }
    }
    source
}

fn many_defgenerics_source(module_count: usize, names_per_module: usize) -> String {
    let mut source = String::new();
    for module_idx in 0..module_count {
        if module_idx > 0 {
            source.push_str(&format!("(defmodule D{module_idx} (export ?ALL))\n"));
        }
        for name_idx in 0..names_per_module {
            source.push_str(&format!("(defgeneric h{name_idx})\n"));
        }
    }
    source
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

fn bench_module_registry_lookup(c: &mut Criterion) {
    let names = local_names(256);
    let registry = build_module_registry(&names);

    c.bench_function("module_registry_lookup_cycle", |b| {
        b.iter(|| {
            let mut hits = 0usize;
            for _ in 0..16 {
                for name in &names {
                    if registry.get_by_name(black_box(name)).is_some() {
                        hits += 1;
                    }
                }
            }
            black_box(hits);
        });
    });
}

fn bench_function_owner_map_load(c: &mut Criterion) {
    let source = many_deffunctions_source(16, 32);

    c.bench_function("function_owner_map_load_cycle", |b| {
        b.iter(|| {
            let mut engine = Engine::new(EngineConfig::utf8());
            black_box(
                engine
                    .load_str(black_box(&source))
                    .expect("load deffunctions"),
            );
        });
    });
}

fn bench_global_owner_map_load(c: &mut Criterion) {
    let source = many_defglobals_source(16, 32);

    c.bench_function("global_owner_map_load_cycle", |b| {
        b.iter(|| {
            let mut engine = Engine::new(EngineConfig::utf8());
            black_box(
                engine
                    .load_str(black_box(&source))
                    .expect("load defglobals"),
            );
        });
    });
}

fn bench_generic_owner_map_load(c: &mut Criterion) {
    let source = many_defgenerics_source(16, 32);

    c.bench_function("generic_owner_map_load_cycle", |b| {
        b.iter(|| {
            let mut engine = Engine::new(EngineConfig::utf8());
            black_box(
                engine
                    .load_str(black_box(&source))
                    .expect("load defgenerics"),
            );
        });
    });
}

criterion_group!(
    benches,
    bench_function_env_lookup,
    bench_global_store_lookup,
    bench_generic_registry_lookup,
    bench_module_registry_lookup,
    bench_function_owner_map_load,
    bench_global_owner_map_load,
    bench_generic_owner_map_load,
);
criterion_main!(benches);
