# Ferric rehabilitation

This is the execution record for the owner's September 6, 2026 instructions.
It replaces the old production-readiness scope, scheduling and exit contract.
Retired obligations are **not** a successful original audit. Required work below
remains open until its implementation PR is merged.

## Baseline and supported scope

Repository `plx/ferric-rules`; initial main and HEAD
`a38de6a852cce3f503467cd000ba7b182c4b5b30`. The starting branch
`plx/review-sol-readiness-work` is retained, with the supplied prompt and user
AGENTS.md changes preserved. Discovery found 141 program issues (60 closed,
81 open), no open PRs or active CI, and 208 native dependency edges. Exact
before-state, toolchain, experiment and issue logs are in `.context/rehabilitation/`.

The supported product is the Rust engine/CLI, TypeScript, Python and a local
Swift package over a healthy C ABI. Go and standalone C distribution receive
maintenance support. Common embedding operations include owned typed values and
errors, create/close, load/reset, assert/retract/query, limited runs, output and
versioned persistence. See [compatibility](../compatibility.md),
[host values](../host-api.md), [snapshots](../snapshots.md) and
[migration notes](../migration.md) for the precise contracts and pre-1.0 breaks.

Core scope retains ordered/template facts, ordinary joins and supported
`not`/`exists`/`test`, static salience, depth/breadth ordering, named deffacts/reset,
rule replacement and basic module export/focus. Unsupported semantics fail
explicitly. There is no task scheduler, durable-execution framework, public
publication requirement, expanded platform matrix or recurring owner renewal.

## Finite integration checklist

All 81 formerly open issues were triaged once. The prepared replacement has
13 work/gate members, consolidating overlapping defects rather than making
one PR per obsolete ticket. The live label/dependency batch is **not applied**.
It preserves issue history, retires 72 items as not planned, removes 180 obsolete
edges, adds 18 finite-cohort edges and preserves 41 unrelated edges. The existing
selector defaults to the new cohort and fails closed while that cohort is empty;
no selector is run during migration.

| Item | Concrete outcome | Integration state |
| --- | --- | --- |
| #297 | Standard dependency checks and retired scope | [#298](https://github.com/plx/ferric-rules/pull/298), merged `142c8d6b` |
| #299 | Template assertions and fact identity | [#301](https://github.com/plx/ferric-rules/pull/301), merged `21e007a1` |
| #100 | Retained benchmark correctness oracles | [#302](https://github.com/plx/ferric-rules/pull/302), merged `5f42ab13` |
| #303 | Measured Rust/C transfer contract | [#304](https://github.com/plx/ferric-rules/pull/304), merged `b6015037` |
| #157 | Selected core semantics and CLIPS evidence | [#306](https://github.com/plx/ferric-rules/pull/306), merged `56d0748b` |
| #194 | Versioned, bounded and validated snapshots | [#307](https://github.com/plx/ferric-rules/pull/307), merged `502a0360` |
| #171 | Node values, lifetimes and package imports | [#308](https://github.com/plx/ferric-rules/pull/308), prepared and validated |
| #190 | Python values, configuration and packages | [#309](https://github.com/plx/ferric-rules/pull/309), prepared and validated |
| #174 | Go module path and cheap lifecycle repairs | [#310](https://github.com/plx/ferric-rules/pull/310), prepared and validated |
| #203 | Host value/fact provenance across adapters | Prepared and validated; merge pending |
| #305 | Swift package and explicit header generation | Prepared and validated; merge pending |
| #153 | Exact Rust/CLI consumers and shared example | Prepared and validated; merge pending |
| #143 | Integrated evidence, CI and backlog migration | Final measurements complete; preceding merges/migration pending |

## Decisions and evidence

**Dependencies.** Standard cargo-deny, npm audit and uv/pip-audit replace about
17,300 lines of bespoke policy and tests. License notices and actionable scanner
results remain. Scoped advisory records explain the actual affected path and
an event that requires reconsideration; no expiry-date reset or whole-graph
waiver hash remains. Positive scans, representative applicable findings and
malformed configuration checks pass. PR #298's review and 99 CI checks passed.

**Threading.** Rust `Engine` is structurally `Send + Sync`, with immutable shared
values using `Arc`; mutation remains exclusive. Escaped values and destruction
are covered, with no blanket unsafe trait implementation. External addresses are
explicit opaque host registry tokens, never pointer casts or silent nulls.
C handles require external serialization and retain call/reentrancy protection;
borrowed output and thread-local errors are copied under their lifetime boundary.
Python releases the GIL before waiting for the engine mutex or doing long work.
Node workers retain event-loop offload/pools. Swift uses a protected native owner
and dispatch queue, with owned results and no unchecked Sendable assertion.

The initial core threading comparison used common base
`36a6a53e81d61868d9c09acd40628c65184ec96c` and candidate
`63ec35315f9270b69128de9eb842aad6427d1d2e`. Both use identical oracle-checked
benchmarks, Rust 1.93, serde, release LTO and one codegen unit. Quiet AMD EPYC
7763 runners built both revisions before alternating A/B/A/B runs, with 30
samples (existing Waltz500/medium-snapshot overrides use ten), 1s warmup and 3s
measurement. [Core artifacts](https://github.com/plx/ferric-rules/actions/runs/34056683658)
and [C ABI artifacts](https://github.com/plx/ferric-rules/actions/runs/34057468971)
retain all actual Criterion median estimates and samples. The C ABI comparison
used baseline `3b8faf6662f5e2c5cb3abd8737687f82cdc9df41` and candidate
`a3a372f9ff120740d8b6fe195791fb3a40b2f3f6`. End-to-end core deltas
were -2.68% to +3.49%; raw snapshot deltas reached +5.41%. No 10% investigation
trigger was reached. C lifecycle deltas were -1.08% to +1.20%.

Representative actual medians in microseconds, in alternating base/candidate pairs:

| Initial threading workload | Base A | Candidate A | Base B | Candidate B |
| --- | ---: | ---: | ---: | ---: |
| Nested multifield joins, 1000 keys | 7327.801 | 7421.159 | 7175.505 | 7426.017 |
| Load/reset/run, 1000 facts | 3371.073 | 3286.173 | 3307.434 | 3287.311 |
| Raw Bincode small snapshot write | 27.089 | 26.726 | 26.296 | 27.719 |
| C lifecycle, 1000 facts | 4227.374 | 4181.535 | 4159.138 | 4201.571 |

**Core behavior.** The pinned CLIPS 6.30 lane now passes 57 scenarios: 55 match,
two retain exact documented LEX/MEA differences. All 35 additions beyond the
refreshed 22-case baseline match real CLIPS; the original 22 now have 20 matches.
The Debian 6.30-4.1 ARM64 reference binary SHA-256 is
`a9fca5ca7d0f9a71626553245bb7fe9d4cde9ea79c4afcb7c6ec3b6607247450`.
Reference firing behavior, facts, output and relevant errors remain meaningful;
Ferric output was never used as its own oracle. Repairs cover replacement and
reload, reset/named seeds, activation/join chronology, module exports/focus,
primitive slot validation and clear unsupported-form rejection. PR #306 passed
102 successful or intentionally skipped checks after its review findings were fixed.

**Persistence.** The recommended format is CBOR in a bounded version-one envelope
with a corruption checksum, input/decoder limits and restored-state validation.
The stored schema fixture resumes actual pending work and later blocker changes.
Raw legacy snapshots are explicitly rejected; application data must be exported
using its producing version before upgrading. Alternative codecs remain
experimental. External values fail explicitly, including nested values. Counter
exhaustion returns errors while reads/persistence remain available; failed modify
and replacement preserve prior state. Review regressions also cover root/negative
metadata, strategy agreement and registered global identities. All 59 snapshot
tests pass; this is an application persistence contract, not permanent migration
of arbitrary RETE internals or hostile-input certification.

**Validation scope.** Reuse existing deterministic regressions, meaningful
consumer smokes and relevant sanitizer checks. They establish owned-value/error
lifetimes, serialized close/use and rule behavior after restore. No new fuzzing
campaign or wider pointer-probing program is required. Mandatory preflight still
runs before PR creation/updates; repeat other suites for actual changed paths or
review findings.

## Final performance and consumer checkpoint

The accepted final comparison uses baseline
`dd6adee5245dce7a9f1c94ce0078a236df6bf8ee` and candidate
`f7e8ca35a9aa8bb6637ebb24a0a4122fefaee7c9` (product `99d23897` plus the common
CBOR harness/collector). This baseline **already contains transferable threading**;
these deltas measure subsequent semantic/lifecycle repairs, checked host ownership
and validated persistence. Both revisions use the same expected firing/final-state
oracles and identical benchmark/helper objects. All 20 release correctness checks
pass on both sources. No skipped work or test/profile duration is a speedup claim.

The final [core](https://github.com/plx/ferric-rules/actions/runs/34077615293) and
[C ABI](https://github.com/plx/ferric-rules/actions/runs/34077623211) A/B/A/B runs
pass, with all 80 median estimates/sample counts and clean source identities
verified. Rust 1.93, serde, release LTO/one codegen unit, sample/warmup/measurement
settings match the earlier procedure. Core uses EPYC 7763 and C ABI uses EPYC 9V74,
each a quiet four-vCPU runner; compare within pairs, never across runners.
Reproduce with `scripts/bench-threading.sh <base> <candidate> <output> threading`
(or `threading-capi`). Raw logs, samples, estimates/confidence intervals and source
IDs are in the linked artifacts and `.context/rehabilitation/measurements/`.

Actual Criterion median point estimates in microseconds:

| Workload | Base A µs | Final A µs | Δ A | Base B µs | Final B µs | Δ B |
|---|---:|---:|---:|---:|---:|---:|
| churn_2000_facts | 14577.371 | 13901.732 | -4.63% | 14452.016 | 14008.392 | -3.07% |
| churn_500_facts | 3599.614 | 3434.560 | -4.59% | 3553.456 | 3451.861 | -2.86% |
| join_nested_multifields_100 | 797.430 | 901.759 | +13.08% | 790.081 | 900.779 | +14.01% |
| join_nested_multifields_1000 | 7932.421 | 8801.138 | +10.95% | 7837.415 | 8860.593 | +13.06% |
| join_strings_100 | 482.274 | 547.732 | +13.57% | 475.870 | 551.964 | +15.99% |
| join_strings_1000 | 4899.573 | 6127.940 | +25.07% | 4524.786 | 5214.922 | +15.25% |
| lifecycle_load_reset_run_100 | 363.094 | 301.710 | -16.91% | 362.543 | 298.817 | -17.58% |
| lifecycle_load_reset_run_1000 | 3527.724 | 2905.255 | -17.65% | 3491.687 | 2872.968 | -17.72% |
| lifecycle_reset_run_100 | 179.184 | 190.357 | +6.23% | 179.726 | 186.869 | +3.97% |
| lifecycle_reset_run_1000 | 1866.091 | 2014.902 | +7.97% | 1880.740 | 1987.838 | +5.69% |
| snapshot_cbor_medium/deserialize | 5290.351 | 6478.903 | +22.47% | 5414.523 | 6492.787 | +19.91% |
| snapshot_cbor_medium/serialize | 1839.528 | 9563.153 | +419.87% | 1833.045 | 9589.211 | +423.13% |
| snapshot_cbor_small/deserialize | 607.232 | 788.377 | +29.83% | 621.010 | 792.788 | +27.66% |
| snapshot_cbor_small/serialize | 203.575 | 1159.837 | +469.73% | 207.854 | 1162.624 | +459.35% |
| waltz_100_junctions | 1362.535 | 1179.840 | -13.41% | 1369.632 | 1176.534 | -14.10% |
| waltz_500/waltz_500_junctions | 6213.700 | 5275.022 | -15.11% | 6235.595 | 5266.352 | -15.54% |
| capi/lifecycle/100 | 464.982 | 377.986 | -18.71% | 465.672 | 382.503 | -17.86% |
| capi/lifecycle/1000 | 4583.311 | 3566.064 | -22.19% | 4559.923 | 3618.437 | -20.65% |
| capi/read_output/100 | 15.521 | 17.435 | +12.33% | 15.367 | 17.507 | +13.93% |
| capi/read_output/1000 | 152.436 | 179.142 | +17.52% | 152.943 | 175.026 | +14.44% |

Accept these explicit costs. Checked ownership prevents foreign/stale handles
from silently addressing another engine's facts. String/nested joins add about
0.07–1.23 ms across the selected sizes. The 1000-key string result varies materially
between pairs (6.128 ms versus 5.215 ms); retain both +25.07% and +15.25%, without
averaging away the regression. The five retained scaling checks pass separately;
their timings are not latency evidence.

For 20 templates/100 rules/500 input facts, CBOR write rises from about 1.83 ms to
9.56–9.59 ms and restore from 5.29–5.41 ms to 6.48–6.49 ms. Small write/restore cost
about 1.16 ms/0.79 ms. Keep the checks so a successful persisted snapshot satisfies
the same documented restore limits. Profiles identify decode-budget verification,
encoding and checksum work, not graph validation alone, as the main write costs.
C checked read/output adds 22–27 µs per 1000 facts (+14.44–17.52%); accept this
separately from the faster complete C lifecycle. These costs and the passing
scaling evidence justify retaining the implementation without a broader redesign.

Earlier integrated results triggered one bounded host-validation optimization
experiment. It was rejected: the 1000-key string join changed from 5182.542 to
6008.728 microseconds (+15.94%) and 5305.634 to 5976.186 (+12.64%) in the two
[release pairs](https://github.com/plx/ferric-rules/actions/runs/34075912463).
Small gains elsewhere do not erase this regression. Keep the simpler original
host implementation; no second optimization experiment or RETE redesign is
warranted. Short CPU profiles identified decoder verification, encoding and
checksum work as the main snapshot-write costs. Full snapshot checks remain.

Frozen consumer candidates pass the common launch-selection source and pending
snapshot resume. Its several candidates select at most one action per session.
Exact Rust/CLI archives install and run outside the checkout, including invalid
input diagnostics. Node real packages pass CJS/ESM/type resolution and Node22/26
consumer smokes; Python exact wheels and normalized source packages pass their
external consumers. The host migration passes 27 shared cases across five
adapters (12 documented deviations), 12 provenance tests and all five existing
release scaling checks. Swift's three native slices, 14 Swift6 strict tests,
14 Swift-ASan tests, iOS device/simulator wrapper builds and copied external
macOS package consumer pass. Swift-ASan instruments Swift, not all native Rust.
Current local Apple Silicon SDK27 builds target Swift6/macOS15/iOS18; existing CI
covers declared environments unavailable locally. No iOS device execution is claimed.

Essential merge evidence includes core quality/MSRV/features, optimized behavior
and scaling, pinned CLIPS, affected binding and package consumers, dependency
scans/license notices and relevant existing C/lifecycle sanitizer checks. The
protected `PR Compatibility Gate` is required but is not the entire quality bar.
Consequential changes receive fresh review; red checks are fixed, never bypassed.

## Deliberate deferrals and next action

Deferred: broad Go/C distribution or parity, public tags/registries/releases,
new platform certification, production SLO/soak/shadow programs, a task scheduler,
recurring owner renewals, and speculative RETE/index/refcount redesigns without
reproduced need. Logical CEs and unsupported optional module/query/strategy forms
fail explicitly. LEX/MEA remain experimental. CLIPS-valid complex negated
constraints rejected by PR #254 remain disclosed and tracked in
[#300](https://github.com/plx/ferric-rules/issues/300); this was not silently closed.
Qualified distinct template declarations work; unsupported unqualified cross-module
name collisions fail before state changes. Migration notes describe Python string,
Node version/precision, Go cancellation/path, host handles and legacy snapshot breaks.

Next: merge Node, Python, Go, host, Swift and external-consumer changes in
dependency order after their checks. Apply and verify the prepared issue/label/native-dependency batch, complete final
validation and merge #143's closing record. Reconcile the starting checkout while
preserving user changes. Completion requires all accepted implementation PRs merged.
