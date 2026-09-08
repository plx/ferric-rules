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
| Existential joins | The compiler builds equality indexes, but right-side `exists` activation scans every parent. | Reuse the indexed candidate lookup; verify order, backfill, non-indexable values, and a new scaling gate. |
| Template resolution | Every lookup scans all templates and allocates parsed names for each definition. | Index local-name candidates and defer discarded diagnostics; retain live visibility checks. |
| Action loops | Counted loops copy token bindings and rule metadata for every iteration; runtime local bindings are rebuilt for expression evaluation. | Investigate frame reuse and removal of intermediate copies. |
| Retraction | Every removed token scans all negative, NCC, and exists memories for parent cleanup. | Follow the token owner's child nodes; borrow facts during read-only cleanup. |
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

## Indexed existential support

The compiler already requests equality indexes for the parent memory of an
`exists` node. Right activation now uses the same candidate lookup as positive
joins instead of collecting every parent token. Matching candidates retain
newest-first traversal, all join predicates are still evaluated, and small
memories, absent indexes, and non-indexable values retain the scan fallback.
No persisted fields or public APIs change.

The integration regression test covers both sides of the index threshold,
online installation onto a populated shared prefix, integer/string/multifield
keys, multiple witnesses, last-witness removal, and refiring order. It passed on
the parent implementation before the optimization and on the candidate.

`just scaling-check` now includes existential support assertion. It fixes the
number of readings per sensor and increases the number of sensors, exposing the
quadratic scan of unrelated parents. The new gate rejects the parent revision
in an isolated release run; all six gates pass on the candidate. These gate
results are correctness evidence for scaling, not benchmark timing claims.

Validation also includes `just preflight-pr` and 1,485 optimized core/runtime
tests with all features, including snapshot validation and resume behavior.

Measured implementation: `a04b810b`, against the shared-template parent
`8e53e793`. All eight `exists` workloads were measured twice, with the repeat
alternating an isolated parent checkout and the candidate. The complete
[measurement record](2026-09-07-exists-indexing.json) includes both passes and
13 engine controls measured twice.

| Workload | Before median | After median | Change |
| --- | ---: | ---: | ---: |
| 50 sensors × 10 readings | 974.67 µs | 627.25 µs | -35.64% |
| 100 sensors × 20 readings | 4.94 ms | 2.37 ms | -52.03% |
| 200 sensors × 50 readings | 37.50 ms | 12.08 ms | -67.79% |
| 500 sensors × 100 readings | 387.39 ms | 64.49 ms | -83.35% |
| 1,000 sensors × 50 readings | 856.35 ms | 66.45 ms | -92.24% |

The first pass also improved every indexed workload (30.02–91.72%). Small
scan-fallback and tuple-based workloads moved in both directions between passes;
their repeat medians improved 0.62–5.97%. Engine controls had smaller mixed
movements, including a +5.70% repeated result for `reset_run_retract_3` (first
pass +1.12%). They remain in the cumulative audit rather than being omitted
from the record. The substantial, repeatable indexed-workload gains justify
retaining the change.

## Indexed template resolution

Template references now inspect a derived local-name candidate index instead of
scanning and reparsing every definition. The index contains IDs only; module
visibility is checked on each lookup, so changed imports/exports take effect
immediately. Local definitions retain preference over imported definitions and
ambiguity diagnostics retain sorted module names. Ordered-relation probes use
a typed lookup result and avoid formatting errors they discard.

The index is maintained on installation, retained across same-ID replacement and
reset, cleared by `clear`, and rebuilt from template definitions on snapshot
restore. It adds no persisted fields. The cold definition-identity lookup retains
SlotMap traversal order and now borrows name parts instead of allocating them.

Validation includes full `just preflight-pr`, runtime tests with all features,
property comparisons against the existing qualified-name parser (arbitrary
Unicode strings and colon-heavy names), exact visibility/ambiguity diagnostics,
live import updates, local shadowing, redefinition, clear/reset, and candidate
reconstruction in all five snapshot formats.

Measured implementation: `ffa4ff46`, against `2711ff5e`. Both passes cover all
39 compilation, engine, module, and Waltz workloads. Complete medians are in
[the measurement record](2026-09-07-template-resolution.json).

| Workload | Before median | After median | Change |
| --- | ---: | ---: | ---: |
| Compile 100 rules / 20 templates | 909.27 µs | 713.47 µs | -21.53% |
| Compile 500 rules / 50 templates | 8,146.51 µs | 5,805.18 µs | -28.74% |
| Load/reset/run 1,000 facts | 1,274.20 µs | 1,123.33 µs | -11.84% |
| Waltz, 100 junctions | 437.79 µs | 404.37 µs | -7.63% |
| Engine creation | 360.17 ns | 371.08 ns | +3.03% |

The first pass improves the five compilation sizes by 6.37–29.40%; the repeat
improves them by 8.53–28.74%. Other controls are mixed, with engine creation
+0.63% initially and +3.03% in the repeat. The repeat's largest regression is
that engine-creation case. The large, repeatable compilation and runtime gains
justify retaining the index.

## Retraction cleanup

Parent bookkeeping now visits only the removed token owner's immediate negative,
NCC, and exists children. Alpha retraction uses its complete fact-to-memory
reverse index instead of recursively visiting every branch under the relation.
NCC result lookup visits the owner's partner children in ascending memory order,
preserving the previous first-match behavior and subsequent unblocking path.
Host and RHS retraction borrow facts while Rete reads them. Fact-slot expressions likewise
borrow the containing fact and clone only the requested value; modify/duplicate
retain an owned original across expression evaluation.

The new regression also found an existing snapshot failure: removal of the last
indexed parent left an empty outer beta-variable map. Snapshot validation
rebuilds a sparse map from surviving tokens and correctly rejected that state.
Beta cleanup now removes empty outer maps, matching alpha cleanup. The validator
is unchanged. The minimal snapshot regression fails on the parent revision;
restoration and refiring in all five formats pass with the fix.

The independent-memory benchmark varies negative/NCC/exists rules from 1 to 512,
excludes compilation, and times reset plus parent retraction. Its untimed oracle
checks initial firings/results, complete cleanup, and refiring after reinsertion.
A mixed shared-prefix test covers online installation and blocked/unblocked
parents; its functional assertions pass on both revisions. A new scaling gate
isolates cleanup of independent negative parents with compilation/assertion setup
excluded from its measured operation.

The independent-negative scaling gate initially still rejected the candidate
after parent cleanup was narrowed. That exposed the separate alpha branch walk;
using the existing membership index removes that second quadratic operation.
The unchanged gate rejects the parent and passes the optimized cleanup. No
scaling-test timings are used as performance measurements.

Measured implementation: `86850c82`, against `23e2650f`, using identical benchmark
sources on both revisions. Both passes cover 69 workloads: the new 12-case
cleanup matrix plus all cascade, churn, negation, forall, query, and engine
controls. Complete medians are in [the measurement record](2026-09-07-retraction-cleanup.json).

| Workload | Before median | After median | Change |
| --- | ---: | ---: | ---: |
| 512 negative memories | 4.64 ms | 1.96 ms | -57.79% |
| 512 NCC memories | 5.48 ms | 2.02 ms | -63.10% |
| 512 exists memories | 6.50 ms | 3.66 ms | -43.79% |
| Forall, 2,000 tasks | 29.71 ms | 25.68 ms | -13.55% |
| Churn, 100,000 facts | 460.00 ms | 441.72 ms | -3.97% |
| Small retraction, reset/run | 2.97 µs | 2.91 µs | -2.12% |

The first pass's largest small-case regression is +4.48% for one negative memory;
its repeat is +0.59%. The repeat's largest regression is +2.66% for the small
three-layer cascade (first pass -2.77%). Both full passes retain these controls.
The large independent-memory and forall gains reproduce, with broader smaller
churn gains, so the change is a net win on the measured suite.

Final validation includes `just preflight-pr`, 1,491 core/runtime tests with all
features, and all seven scaling gates on the measured revision.

## Temporary action frames

Counted loops now create one binding frame lazily after the first iteration-budget
check and update its counter for subsequent iterations. Unnamed counted loops
borrow the existing frame. `progn$`/`foreach` retain the evaluated list and reuse
one element/index frame; changing the source global cannot change traversal.
Dispatch borrows existing function-call syntax trees, and temporary rule metadata
omits the source text that introspection obtains from the registered rule.

When runtime locals are present, evaluation builds its binding set directly from
shared outer values and a single copy of each local. It avoids the intermediate
owned name/value map. Canonical multifield aliases retain last-outer-binding
precedence, locals override outer bindings, and symbols are normalized to the
current string encoding. Evaluation entry points, budgets, tracing and action
error handling remain in place.

Five regressions pass on both parent and candidate: nested counter shadowing,
RHS locals overriding later counter updates, retained progn traversal after a
global changes, local multifield aliases, and rule-source introspection inside a
loop. A randomized binding test covers alias order, local overlays, scalar and
multifield values, unbound slots, and mixed encodings. Validation includes full
`just preflight-pr` and runtime tests with all features.

Four new oracle-equipped reset/run benchmarks cover unnamed counters, nested
loops, conditional bodies, and progn element/index bindings. Existing evaluator,
engine, Waltz, and Manners cases remain controls.

Measured implementation: `129b0fd7`, against `4733dc05` (repeat `1c082432`,
which adds only the package manifest correction). Both paired passes cover the
same 54 evaluator, engine, Waltz, and Manners workloads. The candidate repeat
also runs the remaining facade suite as a cumulative checkpoint. Complete paired
medians are in [the measurement record](2026-09-07-action-frames.json).

| Workload | Before median | After median | Change |
| --- | ---: | ---: | ---: |
| Named loop, 100,000 iterations | 52,515.91 µs | 26,144.06 µs | -50.22% |
| Unnamed loop, 10,000 iterations | 3,938.19 µs | 2,217.06 µs | -43.70% |
| Nested loops, 100 × 100 | 5,545.84 µs | 2,682.19 µs | -51.64% |
| Conditional loop, 10,000 iterations | 4,586.67 µs | 2,591.44 µs | -43.50% |
| Progn, 1,000 elements | 952.12 µs | 385.39 µs | -59.52% |
| Simple ordered facts, reset/run | 2.05 µs | 2.17 µs | +5.88% |

The seven targeted loop cases improve 43.50–59.52% in the repeat,
after improving 42.87–59.52% in the first pass. Controls remain in the record:
the first pass's largest regression is +2.25% for `load_and_run_simple`;
the repeat's largest is +5.88% for `reset_run_simple`
(first pass -2.35%). These small controls are retained in the
cumulative audit. The substantial, repeated loop gains justify the change.

## Host boundary bookkeeping

Host identity removal, clearing, and amortized pruning now use `Mutex::get_mut`
under exclusive engine access. Shared exports and lookups retain their mutex,
global identities, and provenance checks. The existing bounded pruning policy
remains in place; synchronous cleanup on every RHS removal was measured and
rejected after repeatable small-churn regressions.

Host-value validation retains its depth/item limits, ownership checks, and
traversal/error order, but stores its first eight pending values inline instead
of allocating a vector for each scalar input.

A dense `SecondaryMap` reverse index was evaluated and rejected. It made the
first read of a high-index fact allocate for many unexported slots. The new
`host_first_sparse_export` control excludes engine setup/destruction and retains
an exact target-value and stable-handle oracle. The retained implementation uses
sparse hash maps, so storage scales with exported identities rather than the
fact arena's highest occupied slot. Rejected prototype measurements remain in
[the experiment record](2026-09-07-host-bookkeeping-experiments.json).

Regression tests check bounded storage reclamation through repeated RHS retract
and template modify cycles, stable live handles, rejection of retired handles,
reset/clear, and eight concurrent readers exporting the same initially
unexported fact. Direct assertion/retraction benchmarks verify one and eight
integer fields, alongside sparse reads, owned captures, registry operations,
and facade lifecycle/retraction/churn/query controls.

## Runtime snapshot benchmark repair

The broader runtime audit exposed a pre-existing invalid workload: the large
snapshot generator printed integral floating-point values as integer literals,
which violate its `FLOAT` slot constraint. Formatting those generated values
with one decimal place preserves the intended data and lets the existing fact,
firing, output, and quiescence oracles run. Comparisons use the repaired source
on both revisions; failed runs contribute no performance claims.

## Focused agenda selection

A focused module could previously rescan every higher-priority dormant
activation before each firing. Agenda selection now tries the first activation
without allocating and builds a derived per-rule ordering only after a focus
miss. Selection compares eligible rule heads using the existing priority keys.
The index tracks additions and removals, invalidates on clear/sequence rebasing,
and is rebuilt after snapshot restoration; serialized fields are unchanged.
Focus eligibility is evaluated anew on every call. The public activation-based
predicate API retains its priority order and one call per visited activation.

A randomized differential test compares indexed selection with the original scan
through arbitrary mutations under Depth, Breadth, Lex, and MEA. Integration tests
exercise partial execution, retained dormant matches, later focus changes, exact
output order, and resume through all five snapshot formats. The new
`module_dormant_focus` release benchmarks cover 128, 512, and 2,048 facts under
all four strategies, with result oracles. A separate scaling gate excludes setup
and detects repeated dormant-activation scanning. The gate rejects the parent
and passes the candidate; all eight scaling gates pass on the candidate.

## Small ordered memberships

Alpha and beta memory membership sets now keep their first two keys inline.
The third distinct insertion promotes to the existing hash-linked storage in
insertion order. Duplicate insertion does not promote or reorder a member;
removal followed by reinsertion still appends. Promoted sets retain their
allocation when cleared or reduced, avoiding churn between representations.
Large-set insertion/removal remains hash based. Iterators preserve forward,
reverse, and mixed-direction traversal and exact remaining lengths.

Small sets reuse the existing first/last-key fields while the hash table has zero
insertion capacity. This removes the representation and iterator enums. Linked
iteration retains its direct table lookup, while the small case reads the endpoint
fields. A fully occupied collision table can return to zero insertion capacity
after removal; endpoint reuse preserves its allocation and ordering. A focused
collision/reuse regression covers this boundary.

Snapshots still encode only the ordered key sequence, reconstruct the storage
on decode, and reject duplicate members. Existing randomized membership tests
cover arbitrary mutation; focused tests cover the promotion threshold and
allocation reuse. New `beta_membership_sizes` controls measure cold construction,
traversal, and removal at 1, 2, 3, 32, and 1,024 members, with duplicate/order and
empty-result oracles. Core storage, join, cascade, churn, alpha fanout, engine,
and Manners workloads remain paired controls.

## Rejected cascade experiment

Keeping eight pending cascade tokens inline and borrowing the owner child array
passed optimized correctness tests, but did not establish a broad performance
win. Two paired 99-case comparisons are retained in the
[initial record](2026-09-07-cascade-initial.json) and
[repeat record](2026-09-07-cascade-repeat.json). Both used AMD EPYC 7763 Linux
runners with release/LTO builds and identical benchmark source on both revisions.

The four-branch microbenchmark improves 14.84% / 13.55% in the first comparison
and 17.20% / 17.41% in the second (second repeat: 286.17 to 236.34 ns).
The equally weighted complete suite changes +0.07% / -0.01%, then +0.85% / +0.39%.
The second comparison also regresses two core storage controls by more than 5%
in both rounds. The microbenchmark gain does not justify retaining this change
without a broader win, so the production optimization is removed.

The 32-token-chain and four/32-branch benchmark controls remain, including exact
traversal and empty-result oracles. The audit runner retains its complete
facade/runtime/core/C ABI suites so future proposals can repeat this assessment.
