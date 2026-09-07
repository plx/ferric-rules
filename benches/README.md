# Benchmarks

Benchmarks follow Rust conventions and live in individual crate `benches/` directories.

## Main benchmark suite

The `ferric-rules` facade crate contains the primary benchmark suite covering the full
engine lifecycle (create, load, run, reset).

**Run the benchmarks:**

```sh
cargo bench -p ferric-rules
```

**Criterion HTML reports** are written to `target/criterion/` and can be opened in
any browser:

```sh
open target/criterion/report/index.html
```

**Test mode** (verifies benchmarks execute without running full measurement):

```sh
cargo bench -p ferric-rules --bench engine_bench -- --test
```

Each retained engine workload checks its expected rule firings, completion, and
useful final facts or output before Criterion starts timing. Compilation and
snapshot workloads also check subsequent rule behavior. The oracle uses the same
generated source or preparation function as the measured iteration; expected
results come from a simple model of the workload. Filtering a benchmark skips
its oracle as well as its measurement. Snapshot checks require `--features serde`.

Use `--test` results only as correctness evidence. Measurements and comparisons
come from release-profile `cargo bench` and Criterion's actual median estimates.

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
| `lifecycle_load_reset_run_{100,1000}` | Load, reset, consume N facts and assert N results |
| `lifecycle_reset_run_{100,1000}` | The same verified workload with compilation excluded |

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

### Shared-value and query workloads

`join_strings_{100,1000}` and `join_nested_multifields_{100,1000}` load three
relations with one fact per integer key. They join on that key while carrying
UTF-8 strings or nested multifields through bindings into result facts. The
oracle checks every key and payload, including ordered-assert multifield splicing.
They do not claim support for structural multifield equality as a join key.

`query_bench` uses the public fact inspection API to count and sum each category
of template facts. `api_query_load_*` includes load/reset; `api_query_scan_*`
measures repeated host-side scans. The expected categories, counts, and sums are
checked outside timing.

### Comparability after correctness repairs

The September 2026 oracle preparation exposed invalid historical workloads:

- Manners retained its initial zero counter and could assign every guest seat 1.
  The initial rule now retracts that counter; every seat and adjacent hobby is checked.
- Constraint-negation repeated category/status pairs, allowing fact deduplication
  to cap working memory at 60 facts. A unique id now preserves the stated input size.
- Churn and negation now use explicit template slots for their empty-slot patterns;
  forall uses a matching task pattern as its consequent, replacing the failing
  `forall`/`test` combination.
- The historical `do-for-all-facts` query action halted with an evaluation error.
  It is retired from timing; the host API replacement has distinct benchmark names.
- The old deffunction sum stopped at unsupported local `bind`. It now uses
  a supported global accumulator with distinct `eval_defun_global_sum_*` names.
- Several template-result workloads halted at RHS assertions. Successful
  `run().unwrap()` alone did not establish successful execution; the oracles now
  reject `ActionError` and missing or incomplete results.

Do not compare old timings that skipped this work with repaired benchmarks.
Apply workload and necessary semantic fixes to a common base before measuring
implementation changes. A failing oracle invalidates that workload's timing.

## Adding new benchmarks

1. Add your benchmark functions to `crates/ferric-rules/benches/engine_bench.rs`, or
   create a new `[[bench]]` entry in `crates/ferric-rules/Cargo.toml` for a separate file.
2. Register new benchmark functions in the `criterion_group!` macro at the bottom
   of the file.
3. Keep benchmarks focused on stable, representative workloads so regressions are
   easy to detect.
4. Before timing, verify expected firings and useful final state outside the timed
   region, using the same source and preparation helpers. Check execution's halt
   reason as well as its `Result`.

## Measurement Protocol

See `benches/PROTOCOL.md` for the full measurement protocol, including environment
guidance and anti-flake recommendations.

## C ABI consumer overhead

`cargo bench -p ferric-rules-ffi --bench capi_bench` uses the same release/LTO
profile as the engine suites. `-- --test` runs untimed correctness oracles.
At 100 and 1,000 input items, `capi/lifecycle` includes create/load/reset/run,
owned typed fact reads, output copies, and native destruction; `capi/read_output`
measures those reads and copies against a prepared engine. ABI functions are
called through black-boxed function pointers so LTO cannot inline away the
boundary. This measures the C substrate used by hosts, not Python's GIL or
Swift scheduling overhead.

Before timing, the shared operations must produce N firings, 2N facts, exactly
N selected integer/string pairs and output labels, and an owned missing-fact
error. The lifecycle oracle inspects returned copies after RAII has freed the
engine. Every handle is uniquely owned and freed once on the invoking thread,
so the benchmark source works unchanged on the confined baseline and the
transferable candidate. Use identical source, sizes, features, sampling, and
profile for comparisons; no benchmark numbers come from smoke runs.
