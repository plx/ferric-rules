# Ferric rehabilitation

Owner scope: September 6, 2026 rehabilitation execution instructions. This
record supersedes the old production-readiness goal, epic approval checkpoints,
and re-audit exit contract. Retired requirements are not completed audit gates.

## Baseline and execution

- Repository: `plx/ferric-rules`; target `origin/main`.
- Starting HEAD and fetched main: `a38de6a852cce3f503467cd000ba7b182c4b5b30`.
- Starting branch retained: `plx/review-sol-readiness-work`. Only the supplied
  `astra-remediation-prompt.md` was untracked before work; preserve it.
- Live September 6 cohort: 141 issues, 60 closed, 81 open; no open PRs or
  active CI runs. Existing other worktrees are not part of this execution.
- Effective main ruleset requires `PR Compatibility Gate`; no required review
  rule is configured. Consequential changes still receive fresh review.
- Local baseline: Apple M4 Max, 64 GiB, macOS 27, Rust/Cargo 1.93.0,
  Node 26.8.1, Python 3.14.7, Swift 6.4, macOS/iOS/simulator SDK 27.
  Declared consumer minima remain subject to their existing supported contracts.
- Raw state, issue bodies, rulesets, environment and experiment logs live in
  `.context/rehabilitation/`. This document retains concise durable conclusions.
- Refreshed pinned CLIPS 6.30 lane passes its existing exact policy: 22
  scenarios, 11 equivalent and 11 known divergences (not 22 equivalences).
  Reference Debian package `6.30-4.1`, ARM64 binary SHA-256
  `a9fca5ca7d0f9a71626553245bb7fe9d4cde9ea79c4afcb7c6ec3b6607247450`.
  Existing selector offline tests: 57 passed.
- One coordinator; no selector runs until the replacement cohort is fully
  migrated and verified. Preserve native dependency history before editing it.

## Supported product

Rust engine and CLI; TypeScript then Python bindings; a local Swift package over
the C ABI. Required common operations: create/close, load/reset, typed facts,
assert/retract/query, limited runs, output/errors, snapshot/restore. Serialized
work must be usable across host threads, using transfer or a tested native
worker boundary. Concurrent mutation of one engine is not a goal.

Core scope includes ordered/template facts, ordinary joins, supported
`not`/`exists`/`test`, static salience, depth/breadth, named deffacts/reset,
rule replacement, and basic module exports/focus. Optional semantics must fail
explicitly when unsupported. Existing meaningful pinned CLIPS 6.30 evidence
remains; add at least 25 distinct useful scenarios and one shared launch/modal
selection example, including persistence/resume, across all four host languages.

## Finite work checklist

- [x] Replace dependency-policy machinery ([#297](https://github.com/plx/ferric-rules/issues/297)) with standard scanners, actionable
  scoped exceptions, and retained license notices; validate positive/negative cases.
- [ ] Establish benchmark correctness oracles (#100), measure the threading
  choice, implement all affected ownership/lifetime contracts and regression tests.
- [ ] Correct template RHS assertions/cardinality, correlated last-blocker `not`,
  and multifield equality joins discovered by the required evidence workloads.
- [ ] Correct rule replacement/removal and template load safety (#157, #158,
  #191); depth/breadth (#154); reset/named deffacts/initial-fact (#156, #161, #204).
- [ ] Correct basic module export/focus (#160, #192, #193); document incremental
  load (#159); reject unsupported logical/optional strategy/module/salience cases
  (#164, #155, #205, #209, #210), retaining PR #254's complex-negation disclosure ([#300](https://github.com/plx/ferric-rules/issues/300)).
- [ ] Validate host fact shape/provenance (#202, #203) and bound reachable
  dangerous construct expansion/depth (#200, #201), without a RETE redesign.
- [ ] Version, bound, and validate snapshots with a concrete compatibility
  policy and behavioral round trips (#194, #151).
- [ ] Complete TS values/errors/imports/lifecycle/runtime contract (#166, #167,
  #171, #181–#186, #206, #207); Python values/errors/config/threading (#187–#190,
  #208); use existing workers where useful.
- [ ] Deliver Swift strict-concurrency wrapper, owned values/errors, local
  macOS/iOS build path, external consumer and task/lifecycle/persistence tests.
- [ ] Verify packaged Rust/CLI, TS and Python external consumers (#153, focused
  #124); make C header generation safe for packaging (#170).
- [ ] Repair cheap serious Go defects (#174, #176, #177, #179), preserve working
  source-build support and document remaining maintenance-only limitations.
- [ ] Integrate example and differential evidence; retain meaningful release,
  scaling and safety CI (#143, focused #142/#145/#150); finish reviewed merges.
- [ ] Complete one coherent label/dependency migration, retire obsolete issues
  as not planned, and point existing scheduling/docs to this finite scope.

These are behavior groups, not a promise of one PR per historical issue.
All 81 open historical issues have a locally prepared disposition: 18
retained/consolidated behavior groups, plus dependency/threading/Swift additions.
The saved native baseline has 208 edges. Migration is not applied yet.
Consolidate overlaps before selecting; each closing PR covers only one active
cohort item. Required behavior may not be retired to complete the checklist.

## Decisions and deferrals

- No registry/tag/release publication, standalone C SDK productization, broad
  Go distribution/parity, platform proliferation, production SLO/soak/shadow
  program, task scheduler, or recurring owner waiver renewal.
- Defer speculative RETE architecture/index/refcount projects (#163,
  #195–#199) unless reproduced scaling or selected measurements establish need;
  broad evaluator consolidation (#180), invariant expansion (#162), and fuzz
  campaigns (#165) are not prerequisites for the bounded product.
- Benchmark families selected before timing: Waltz 100/500, churn 500/2000,
  load/reset/run 100/1000, string and nested-multifield joins 100/1000. Oracles
  must validate the same work outside timing. Manners currently leaves its
  initial counter live; repair on the common benchmark base and do not compare
  to old timings that measured incorrect work.
- Select one structurally `Send + Sync` Rust engine. Paired measurements below
  show no 10% end-to-end regression, so no optimization experiments or parallel
  Rc/Arc implementations are needed. C calls remain serialized; shared Rust reads
  do not authorize concurrent C access. External addresses become explicit host
  registry tokens; unsupported binding and snapshot values are rejected.

## Current next action

Dependency replacement [PR #298](https://github.com/plx/ferric-rules/pull/298)
merged at `142c8d6b03a785b829d65e04efd9273dcf2609e1`; all 99 CI checks and
independent review passed. Standard scanners, scoped applicability records,
negative/malformed-input tests and license notices replace the retired policy.
Portable scanner tests and exact-set npm validation fix initial CI findings.

All 17 retained facade benchmark suites now pass correctness oracles, as do
runtime snapshot/fact-duplication suites. Repairs include invalid template RHS
execution and historical Manners, duplicate-input, query and deffunction-sum
workloads. New query/deffunction names avoid false historical comparisons.
No performance claim uses old skipped work or correctness-only test timings.

The measured common base is `36a6a53e81d61868d9c09acd40628c65184ec96c`; candidate
`63ec35315f9270b69128de9eb842aad6427d1d2e` adds transferable shared values and
structural `Send + Sync`. Exclusive evaluation remains required; private
configuration atomics permit shared reads. Trait, handoff/destruction, escaped
value, concurrent-read, serde/tracing and independent reachability review pass.
C calls remain serialized. Python/Go lifetime, reentrancy and TLS changes pass
339 Python tests, C tests and sanitizer harnesses, and Go race tests; independent
review findings are addressed. These changes and Swift are not yet landed.

The [paired experiment](https://github.com/plx/ferric-rules/actions/runs/34056683658)
uses the existing performance workflow with an optional bounded workload set.
Both revisions build first and run baseline/candidate/baseline/candidate on one
runner. Local unrelated CPU activity made that preferable to timing this host.
Criterion median/sample artifacts retain every run. The core runner was AMD
EPYC 7763 (4 vCPUs), Ubuntu, Rust 1.93, serde, release LTO/one codegen unit;
30 samples, 1s warmup/3s measurement, with existing Waltz-500 and medium snapshot
10-sample overrides identical on both revisions. Timings below come from
`cargo bench` median point estimates, never correctness-only tests.

Actual medians in microseconds; A and B are alternating baseline/candidate pairs:

| Workload | Base A | Candidate A | Delta A | Base B | Candidate B | Delta B |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| churn_2000_facts | 14373.123 | 14744.452 | +2.58% | 14142.558 | 14100.016 | -0.30% |
| churn_500_facts | 3537.776 | 3638.118 | +2.84% | 3490.906 | 3490.227 | -0.02% |
| join_nested_multifields_100 | 770.020 | 781.393 | +1.48% | 774.438 | 774.427 | -0.00% |
| join_nested_multifields_1000 | 7327.801 | 7421.159 | +1.27% | 7175.505 | 7426.017 | +3.49% |
| join_strings_100 | 459.244 | 463.093 | +0.84% | 460.687 | 456.840 | -0.84% |
| join_strings_1000 | 4375.099 | 4401.259 | +0.60% | 4375.358 | 4351.902 | -0.54% |
| lifecycle_load_reset_run_100 | 348.883 | 339.539 | -2.68% | 345.226 | 343.497 | -0.50% |
| lifecycle_load_reset_run_1000 | 3371.073 | 3286.173 | -2.52% | 3307.434 | 3287.311 | -0.61% |
| lifecycle_reset_run_100 | 179.479 | 178.031 | -0.81% | 175.657 | 178.818 | +1.80% |
| lifecycle_reset_run_1000 | 1901.301 | 1852.289 | -2.58% | 1833.459 | 1855.230 | +1.19% |
| serde_medium/deserialize | 674.826 | 672.591 | -0.33% | 677.940 | 691.590 | +2.01% |
| serde_medium/serialize | 278.475 | 272.755 | -2.05% | 271.496 | 274.780 | +1.21% |
| serde_small/deserialize | 75.935 | 75.862 | -0.10% | 75.766 | 77.319 | +2.05% |
| serde_small/serialize | 27.089 | 26.726 | -1.34% | 26.296 | 27.719 | +5.41% |
| waltz_100_junctions | 1356.475 | 1342.975 | -1.00% | 1342.676 | 1368.999 | +1.96% |
| waltz_500/waltz_500_junctions | 6145.343 | 6088.864 | -0.92% | 6125.077 | 6233.813 | +1.78% |
| capi/lifecycle/100 | 437.163 | 434.313 | -0.65% | 438.303 | 443.571 | +1.20% |
| capi/lifecycle/1000 | 4227.374 | 4181.535 | -1.08% | 4159.138 | 4201.571 | +1.02% |
| capi/read_output/100 | 15.372 | 14.395 | -6.36% | 15.439 | 14.459 | -6.35% |
| capi/read_output/1000 | 159.280 | 152.703 | -4.13% | 160.238 | 152.100 | -5.08% |

End-to-end core changes range from -2.68% to +3.49% across both pairs; the
largest snapshot change is +5.41%. No workload reaches the 10% investigation
trigger. Small differences should not be treated as reliable speedups.
The [C ABI comparison](https://github.com/plx/ferric-rules/actions/runs/34057468971)
uses base `3b8faf6662f5e2c5cb3abd8737687f82cdc9df41` and candidate
`a3a372f9ff120740d8b6fe195791fb3a40b2f3f6`, with 30 samples for all four
workloads. C lifecycle changes range from -1.08% to +1.20%; read/output copying
improves 4.13–6.37%. Snapshot timings here measure the existing raw format;
versioned persistence will receive its own validation and comparison.

Six new template scenarios (RH-CORE-016/017/026–029) now pass the expanded
28-case lane: 17 equivalent, 11 unchanged exact divergences. Template repair
[#299](https://github.com/plx/ferric-rules/issues/299) has 14 focused regressions,
full runtime and scaling checks, independent review, and passing `just preflight-pr`.

35 new authenticated CLIPS scenarios are prepared: baseline 18 equivalent and
17 semantic differences, with no invalid observations. Existing 22 reference
expectations remain unchanged. Template assertion/cardinality fixes, last-blocker
negation, multifield equality, and activation chronology have local regressions
and pinned reference evidence. Independent review found and fixed a multislot
void-result edge case. These fixes need integrated checks and reviewed merges.
Reload safety, module exports/focus, and bounded versioned persistence are in
progress. After landing threading across consumers, apply the prepared
backlog migration and continue the finite checklist. Dependency simplification is merged; required product outcomes remain in progress.

PR #301 review follow-up: a later explicit template could reinterpret an installed ordered RHS assertion. The candidate now rejects identity replacement while facts, seeds, or constructs use the ordered relation, including earlier same-load and qualified references. Seven baseline-failing regressions pass with the guard; real CLIPS rejects the ordinary RHS/LHS examples before the original rule runs. Qualified ordered syntax is a Ferric safety regression, not a new parity claim. The guard changes no snapshot layout.
