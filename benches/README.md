# Benchmarks

Benchmarks follow Rust conventions and live in individual crate `benches/` directories.

## Main benchmark suite

The `ferric` facade crate contains the primary benchmark suite covering the full
engine lifecycle (create, load, run, reset).

**Run the benchmarks:**

```sh
cargo bench -p ferric
```

**Criterion HTML reports** are written to `target/criterion/` and can be opened in
any browser:

```sh
open target/criterion/report/index.html
```

**Test mode** (verifies benchmarks execute without running full measurement):

```sh
cargo bench -p ferric --bench engine_bench -- --test
```

## Benchmark inventory

### engine_bench

| Benchmark | What it measures |
|-----------|-----------------|
| `engine_create` | Bare engine construction cost |
| `load_and_run_simple` | Full pipeline: create + load + reset + run (3 facts, 1 rule) |
| `load_and_run_chain_4` | Full pipeline with 4-step rule chain |
| `reset_run_simple` | Reset + run cycle (3 facts, 1 rule) — no compilation |
| `reset_run_20_facts` | Reset + run with 20 facts, 1 rule — alpha throughput |
| `reset_run_negation` | Reset + run with negation patterns |
| `reset_run_join_3` | Reset + run with 2-pattern join (3 entity pairs) |
| `reset_run_retract_3` | Reset + run with retract actions (3 facts consumed) |
| `compile_template_rule` | Parser + loader only (template + rule, no execution) |

### waltz_bench

Simplified Waltz line-labeling benchmark. Labels edges in a scene graph based
on junction types (L, T, fork). Exercises template matching, `modify`, and
negation over template slots.

| Benchmark | What it measures |
|-----------|-----------------|
| `waltz_5_junctions` | Full pipeline with 5-junction scene |
| `waltz_20_junctions` | Full pipeline with 20-junction scene |
| `waltz_50_junctions` | Full pipeline with 50-junction scene |
| `waltz_100_junctions` | Full pipeline with 100-junction scene |
| `waltz_5_junctions_run_only` | Reset + run only (no compilation) |

### manners_bench

Simplified Manners seating benchmark. Seats N guests at a table subject to
the constraint that adjacent guests must have different hobbies. Exercises
template matching, multi-pattern joins, `test` CE with `neq`, negation, and
retraction cycles.

| Benchmark | What it measures |
|-----------|-----------------|
| `manners_8_guests` | Full pipeline with 8 guests |
| `manners_16_guests` | Full pipeline with 16 guests |
| `manners_32_guests` | Full pipeline with 32 guests |
| `manners_64_guests` | Full pipeline with 64 guests (reduced sample size) |
| `manners_8_guests_run_only` | Reset + run only (no compilation) |

## Adding new benchmarks

1. Add your benchmark functions to `crates/ferric/benches/engine_bench.rs`, or
   create a new `[[bench]]` entry in `crates/ferric/Cargo.toml` for a separate file.
2. Register new benchmark functions in the `criterion_group!` macro at the bottom
   of the file.
3. Keep benchmarks focused on stable, representative workloads so regressions are
   easy to detect.

## Measurement Protocol

See `benches/PROTOCOL.md` for the full measurement protocol, including environment
guidance and anti-flake recommendations.
