# Phase 6 Plan Adjustments

## Purpose
Record the master-plan updates required so `documents/FerricImplementationPlan.md`
matches the implemented Phase 6 outcomes and their downstream implications.

## Applied Adjustments To Master Plan

## 1) Section 16 Is Now Topic-Based (Not Heading-Number Locked)
Updated Section 16 to explicitly state:
- `docs/compatibility.md` may use a construct-first/finer-grained taxonomy
  (current implementation: `16.1-16.14`).
- The Section 16 numbered items in the master plan are normative coverage
  topics, not a required one-to-one heading structure.

Why:
- Prevents false inconsistency when content is complete but organized under a
  different heading scheme.

## 2) CI Gate Model Clarified For Phase 6
Updated Section 13.3 (`CI Gates`) to include:
- Dedicated CLIPS compatibility job.
- Benchmark smoke gate.
- Benchmark-threshold gate in Phase 6 for key workloads/microbenchmarks with
  published Criterion estimate artifacts.

Why:
- Matches the implemented CI posture: compatibility and performance are both
  first-class, blocking contract gates.

## 3) Benchmark Fidelity Note Added
Updated Section 14.1 performance notes to state that Phase 6 workload benches
may be scaled/simplified fixtures that still exercise the same hot paths and
must be labeled as such.

Why:
- Aligns documented performance claims with actual benchmark workload design.

## Additional Phase-Plan Alignment Applied
To keep phase-level planning consistent with the master-plan adjustments:
- `documents/plans/phases/006/Plan.md` now references Section 16 as required
  coverage topics (taxonomy-independent), not fixed `16.1-16.8` headings.
- `documents/plans/phases/006/passes/012-CompatibilityDocumentationMigrationAndExamples.md`
  now uses the same topic-based wording.

## Implications For Subsequent Phases
1. References to compatibility docs should target required topics/content, not
   rigid subsection numbering.
2. Performance remediations should preserve the two-layer gate model:
   `bench-smoke` for runnability, `bench-thresholds` for deterministic failure
   conditions.
3. Any future benchmark redesign must keep fidelity disclaimers explicit in docs
   and reports when workloads are scaled/simplified.
