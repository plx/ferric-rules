# Phase 6 Remediation Report

## Scope
Consistency review against:
- `documents/FerricImplementationPlan.md`
- `documents/plans/phases/006/Plan.md`
- `documents/plans/phases/006/passes/*.md`
- `documents/plans/phases/006/Notes.md`
- Implemented Phase 6 artifacts (compat tests, FFI/CLI contract suites, benchmarks, docs, CI)

## Current State
Phase 6 is mostly consistent with the planned deliverables:
- CLIPS compatibility suite exists and passes (`58` tests).
- FFI contract lock suite exists and passes (`17` tests).
- CLI JSON contract lock suite exists and passes (`10` tests).
- Benchmark suites/workloads exist and run in CI smoke mode (`19` benchmarks across 3 suites).
- Compatibility and migration documentation are present and substantially complete.

Material consistency gaps remain in CI policy enforcement and plan-to-doc structure alignment (detailed below).

## Verification Evidence
Executed during this review:
- `cargo test -p ferric --test clips_compat` (`58 passed`)
- `cargo test -p ferric-ffi contract_lock` (`17 passed`)
- `cargo test -p ferric-cli --test cli_integration contract_lock` (`10 passed`)
- `cargo bench -p ferric -- --test` (all benchmark targets execute successfully in smoke mode)
- `cargo bench -p ferric -- --noplot` (fails currently due argument forwarding to non-criterion lib bench target)

## Findings

| ID | Severity | Status | Finding | Evidence |
|---|---|---|---|---|
| R6-01 | High | Open | Benchmark regression thresholds are not CI-enforced deterministically, despite Pass 011 objective/DoD requiring automatic regression catching. Current policy is smoke-blocking plus advisory full runs. | `documents/plans/phases/006/passes/011-PerformanceRegressionPolicyAndCiBenchmarkGates.md`, `docs/benchmark-policy.md`, `.github/workflows/ci.yml` |
| R6-02 | Medium | Open | No dedicated CLIPS-compatibility CI job is present, while master-plan CI gates call for one. Compatibility currently runs only via the broad workspace test job. | `documents/FerricImplementationPlan.md` (§13.3), `.github/workflows/ci.yml` |
| R6-03 | Medium | Open | Compatibility documentation structure diverged from the master-plan Section 16 taxonomy (`16.1-16.8` conceptual buckets) to a construct-first `16.1-16.14` layout. Content is present, but plan references now mismatch actual doc organization. | `documents/FerricImplementationPlan.md` (§16), `docs/compatibility.md`, `documents/plans/phases/006/Notes.md` |
| R6-04 | Medium | Closed | Documented “full benchmark” verification command paths were corrected to executable per-benchmark forms (`engine_bench`, `waltz_bench`, `manners_bench`). | `benches/PROTOCOL.md`, `documents/plans/phases/006/passes/007-BenchmarkHarnessAndMeasurementProtocol.md`, `documents/plans/phases/006/passes/008-WaltzAndMannersBenchmarkWorkloads.md`, `documents/plans/phases/006/passes/009-PerformanceProfilingAndBudgetGapAnalysis.md`, `documents/plans/phases/006/passes/010-TargetedHotPathOptimizationImplementation.md`, `documents/plans/phases/006/passes/011-PerformanceRegressionPolicyAndCiBenchmarkGates.md`, `documents/plans/phases/006/passes/013-Phase6IntegrationAndReleaseReadinessValidation.md` |

## Resolved During Remediation

| ID | Status | Remediation Completed |
|---|---|---|
| R6-05 | Closed | Added a compatibility-harness run guard in `crates/ferric/tests/clips_compat.rs`: runs now use a bounded `RunLimit::Count` (default `10_000`) and fail fast on `HaltReason::LimitReached` with explicit non-quiescence diagnostics. Added local override (`FERRIC_COMPAT_RUN_LIMIT`) documentation in `tests/clips_compat/README.md`. This prevents runaway `clips_compat-*` binaries from spinning indefinitely on non-terminating fixtures/regressions. |
| R6-04 | Closed | Updated benchmark verification commands to valid target names and invocation forms. Replaced invalid/no-op forms such as `cargo bench -- --noplot`, `cargo bench --bench rete_bench -- --noplot`, `cargo bench --bench waltz -- --noplot`, and `cargo bench --bench manners -- --noplot` with executable `cargo bench -p ferric --bench <engine_bench\|waltz_bench\|manners_bench> -- --noplot` commands. |

## Documented Divergences Assessed As Acceptable

| ID | Status | Assessment |
|---|---|---|
| D6-A1 | Accepted | Compatibility harness consolidated into `crates/ferric/tests/clips_compat.rs` with fixture files under `tests/clips_compat/fixtures/`; this is explicitly documented in `Notes.md` and does not create downstream risk. |
| D6-A2 | Accepted with follow-up note | Waltz/Manners implementations are simplified/scaled forms due current parser/constraint limits; this is documented in `Notes.md` and `docs/performance-analysis.md`. Keep explicitly documented to avoid over-claiming strict CLIPS benchmark equivalence. |

## Required Remediation To Reach A Consistent State

1. Implement deterministic benchmark threshold enforcement in CI (R6-01).
   - Add a blocking benchmark-threshold job (not smoke-only) with explicit pass/fail thresholds for key workloads and selected microbenchmarks.
   - Emit machine-readable benchmark artifacts for delta review in CI.
   - Update `docs/benchmark-policy.md` and `benches/PROTOCOL.md` to match the enforced workflow.

2. Add a dedicated CLIPS-compatibility CI job (R6-02).
   - Add explicit job invoking `cargo test -p ferric --test clips_compat`.
   - Keep workspace test job unchanged; dedicated job is for contract visibility and gate clarity.

3. Align master-plan Section 16 references with implemented compatibility-doc structure (R6-03).
   - Either: re-map `docs/compatibility.md` headings back to the conceptual `16.1-16.8` scheme.
   - Or (preferred): update `documents/FerricImplementationPlan.md` to define the construct-first layout as canonical while preserving required content obligations.

## Consistency Exit Criteria For This Remediation
This phase should be considered fully consistent when:
- Benchmark regressions can fail CI against explicit thresholds (not advisory-only).
- CLIPS compatibility has a dedicated CI gate.
- Master-plan Section 16 references match the published compatibility document structure.
