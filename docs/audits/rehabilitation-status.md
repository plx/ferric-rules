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

- [ ] Replace dependency-policy machinery ([#297](https://github.com/plx/ferric-rules/issues/297)) with standard scanners, actionable
  scoped exceptions, and retained license notices; validate positive/negative cases.
- [ ] Establish benchmark correctness oracles (#100), measure the threading
  choice, implement all affected ownership/lifetime contracts and regression tests.
- [ ] Correct rule replacement/removal and template load safety (#157, #158,
  #191); depth/breadth (#154); reset/named deffacts/initial-fact (#156, #161, #204).
- [ ] Correct basic module export/focus (#160, #192, #193); document incremental
  load (#159); reject unsupported logical/optional strategy/module/salience cases
  (#164, #155, #205, #209, #210), retaining PR #254's complex-negation disclosure.
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
- Threading remains an implementation decision pending measurement. Inspection
  confirms escaped `Rc` values, configuration `Cell`s, separate C caches/guards,
  and a Python GIL/mutex lock-order hazard; removing affinity checks alone is
  insufficient. No blanket unsafe trait implementations are authorized.

## Current next action

Dependency replacement for #297 is implemented and independently reviewed with
no blocking findings. Native scans of seven graphs and malformed/vulnerable
input rejection checks pass; Python 322 passed/16 existing testing-feature
skips, tooling 722 passed, Node build/types and affected docs builds pass.
`just preflight-pr` passes. Local Go lint now uses the existing CI linter and
Go versions, fixing failures caused by newer host tools. The first PR still
needs CI and merge.

Benchmark oracles exposed a supported template RHS assertion defect: CLIPS
produces `5:ready` and `(result (key 5) (status ready))`; baseline Ferric stops
with unknown function `key`. Repair on the common base before timing. A separate
structural Send candidate is under test; it is not the selected/landed contract.
After the early PR, finish common-base oracles, run uncontended release
comparisons, select/land threading, apply the prepared backlog migration, and
continue the checklist. No phase is complete yet.
