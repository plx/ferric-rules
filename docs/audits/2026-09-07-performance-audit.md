# Performance audit after rehabilitation

## Measurement contract

The starting revision is `201665e7`, after the rehabilitation PRs. Historical
benchmarks that halted early or performed less work are not comparison baselines.
The initial survey ran all 164 facade workloads, including their correctness
oracles and CBOR snapshot workloads. Two additional owned-fact capture workloads
use identical benchmark source on the baseline and candidate.

Local environment: Apple M4 Max, aarch64 macOS (Darwin 27.0.0), Rust 1.93.0
(`254b59607`). All performance measurements use `cargo bench`, release optimization,
LTO, one codegen unit, and the `serde` feature. Tables use Criterion's
`median.point_estimate` from `estimates.json`, not its printed slope or mean.
Tests and benchmark smoke runs provide correctness evidence only.

The survey command is:

```sh
cargo bench -p ferric-rules --features serde --bench '*' -- \
  --noplot --sample-size 20 --warm-up-time 1 --measurement-time 1 \
  --save-baseline audit-base
```

Existing per-group sample-size overrides remain in effect. Criterion extends
measurement time for workloads that cannot complete their samples in one second.
Compilers and tests are stopped during measurement. Focused comparisons are
repeated with an isolated baseline checkout, alternating baseline and candidate.
These are local measurements; CI machines can produce different absolute times.

## Findings and disposition

| Area | Finding | Disposition |
| --- | --- | --- |
| Template actions and host reads | Assertions, modifications, and owned reads deep-copy immutable template metadata. | Share immutable definitions; measure engine workloads and owned capture separately. |
| Existential joins | The compiler builds equality indexes, but right-side `exists` activation scans every parent. | Exercise the existing indexed candidate lookup, including order, backfill, and non-indexable values. |
| Template resolution | Every lookup scans all templates and allocates parsed names for each definition. | Investigate a derived local-name index without caching visibility decisions. |
| Action loops | Counted loops copy token bindings and rule metadata for every iteration; runtime local bindings are rebuilt for expression evaluation. | Investigate frame reuse and removal of intermediate copies. |
| Retraction | Every removed token scans all negative, NCC, and exists memories for parent cleanup. | Investigate cleanup through the token owner's child nodes. |
| Ordered membership | Linked hash membership preserves the repaired CLIPS traversal order but hashes each iterator step. | Evaluate only alternatives that preserve insertion order and bounded churn storage. |
| Snapshots | Validation checks complete persisted state, graph bounds, identities, and resumed behavior. | Retain validation; use the snapshot suites to detect incidental regressions. |
| Host boundaries | Provenance, transient handles, and synchronization are correctness requirements. | Retain the ownership contract and measure copies at the public API. |

## Shared template definitions

`RegisteredTemplate` remains immutable after installation. The registry, action
helpers, and `HostFact` retain `Arc` handles instead of cloning slot names, type
unions, defaults, and the slot lookup table. Redefinition installs a fresh
allocation, so a captured fact retains the original shape for checked
reassertion. Serde serializes the same underlying definition; the snapshot schema
does not change.

The new `owned_template_fact_{8,64}_slots` benchmarks verify each captured value,
retract the source fact, reassert the capture, and verify the recaptured values
before timing repeated owned reads. They run through
`cargo bench -p ferric-rules-runtime --features serde --bench template_registry_bench`.

Validation includes full `just preflight-pr`, runtime tests with all features,
the committed schema-one snapshot resume/reset fixture, and the existing
host-provenance and template-redefinition regression tests.

Measured implementation: `4c51f5f6`. The first pass covered all engine, Waltz,
Manners, and module workloads (40 cases), plus the two new owned-read cases.
An alternating repeat covered the following nine cases. The full per-case
medians from both passes are retained in
[the measurement record](2026-09-07-template-sharing.json).

| Workload | Before median | After median | Change |
| --- | ---: | ---: | ---: |
| Owned template fact, 8 slots | 287.08 ns | 50.38 ns | -82.45% |
| Owned template fact, 64 slots | 1,589.83 ns | 152.50 ns | -90.41% |
| Waltz, 100 junctions | 477.99 µs | 436.66 µs | -8.65% |
| Waltz, 5 junctions, reset/run | 11.71 µs | 10.82 µs | -7.64% |
| Modules, 3 modules × 100 items, reset/run | 406.89 µs | 371.29 µs | -8.75% |
| Engine creation | 359.97 ns | 361.29 ns | +0.37% |
| Negation, reset/run | 2.61 µs | 2.62 µs | +0.41% |
| Simple ordered facts, reset/run | 2.21 µs | 2.33 µs | +5.00% |
| Three-pair ordered join, reset/run | 3.57 µs | 3.78 µs | +5.81% |

The first-pass engine-only geometric mean is 4.64% lower, weighting each of
the 40 workloads equally. This is a description of this suite, not a prediction
for an arbitrary application. The two small ordered reset cases also regressed
in the first pass (+2.82% and +2.02% respectively), so they remain explicit
cumulative-audit checks. The template and owned-read gains justify retaining
this change; the small regressions are not classified as noise.
