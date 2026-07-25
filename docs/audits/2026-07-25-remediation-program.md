# Production-readiness remediation program

Snapshot date: 2026-07-25

Repository: [plx/ferric-rules](https://github.com/plx/ferric-rules)

Milestone: [Production readiness remediation](https://github.com/plx/ferric-rules/milestone/1)

Bootstrap inventory: **141 open program issues**, **141 unique stable IDs**, and **205 GitHub-native blocked-by relationships**

## Program verdict and scope

The 2026 due-diligence verdict is unchanged: ferric-rules is a credible, substantial prototype, but it is **not ready for production or work use** and must not yet be presented as an almost drop-in CLIPS replacement. The program covers core/RETE semantics, language control flow, compatibility evidence, hostile-input robustness, C ABI and pinned execution, Go/Node/Python bindings, cross-binding conformance, packaging and distribution, release/CI, performance evidence, and the final independent audit.

The durable source documents are the [2026-07-25 due-diligence report](2026-07-25-due-diligence.md) and the [post-remediation production-readiness re-audit playbook](production-readiness-reaudit.md). The audited baseline revision is `dd366eb65a85e2138b8103e719e9fe0b8f52f921`.

GitHub is the **canonical mutable dependency and status state**. This document is the **2026-07-25 bootstrap snapshot**: use it to understand the initial program, taxonomy, and graph, but use live issue state and native dependencies for scheduling and closure decisions after this date.

The operational selector and PR lifecycle are defined in
[Production-readiness automatic work selection](production-readiness-work-selection.md).

## Milestone and program control

All 141 issues are open in milestone [#1, Production readiness remediation](https://github.com/plx/ferric-rules/milestone/1). At snapshot time the milestone has 141 open and 0 closed issues.

- [#224, FR-PRR-000](https://github.com/plx/ferric-rules/issues/224) is the top-level program epic. It is blocked by #223 and closes only after the final decision and evidence meet the program completion criteria.
- [#223, FR-AUDIT-001](https://github.com/plx/ferric-rules/issues/223) is the final independent clean-room audit. It is natively blocked by all 12 component epics.
- [#225, FR-RETE-API-EPIC](https://github.com/plx/ferric-rules/issues/225) is the core topic sub-epic for public API integrity and explicit compatibility-scope behavior. It is blocked by FR-RETE-029 through FR-RETE-035 and itself blocks #211.

## Label taxonomy

Every program issue carries `program:production-readiness`, exactly one `priority:*`, exactly one `type:*`, and one or more `area:*`, `component:*`, and `risk:*` labels. `size/*` is optional for implementation estimates and normally omitted from pure tracking epics.

### Program labels

| Label | Meaning |
|---|---|
| `program:production-readiness` | 2026 production-readiness remediation program |

### Priority labels

| Label | Meaning |
|---|---|
| `priority:p0` | Release blocker: memory safety or fundamental silent wrong results |
| `priority:p1` | Required before a production-readiness claim |
| `priority:p2` | Important hardening, quality, or delivery work |
| `priority:p3` | Follow-up optimization or polish |

### Type labels

| Label | Meaning |
|---|---|
| `type:architecture` | Structural change or maintainability work |
| `type:audit` | Verification or audit activity |
| `type:defect` | Confirmed incorrect behavior |
| `type:delivery` | Packaging, release, or CI delivery work |
| `type:epic` | Tracking issue that organizes a remediation workstream |
| `type:hardening` | Robustness, safety, or defensive engineering |

### Area labels

| Label | Meaning |
|---|---|
| `area:architecture` | Cross-cutting internal architecture |
| `area:bindings` | Language binding APIs and wrappers |
| `area:compatibility` | CLIPS differential compatibility |
| `area:ffi` | Native C ABI and ownership boundary |
| `area:performance` | Runtime performance and scaling |
| `area:release` | Packaging, versioning, and artifact delivery |
| `area:rete` | RETE network construction, matching, or propagation |
| `area:robustness` | Limits, hostile input, and failure containment |
| `area:semantics` | CLIPS language and execution semantics |
| `area:tooling` | Developer and validation tooling |

### Component labels

| Label | Meaning |
|---|---|
| `component:c-abi` | ferric-ffi C ABI |
| `component:ci` | Continuous integration workflows |
| `component:compat-tool` | Compatibility scanner, harness, and runner |
| `component:core` | ferric-core and shared model types |
| `component:cross-cutting` | Program-wide work spanning multiple implementation components |
| `component:go` | Go binding |
| `component:node` | N-API addon and TypeScript binding |
| `component:packaging` | Published package and native artifact assembly |
| `component:parser` | ferric-parser |
| `component:pinned` | ferric-pinned worker layer |
| `component:python` | Python/PyO3 binding |
| `component:runtime` | ferric-runtime and facade behavior |

### Risk labels

| Label | Meaning |
|---|---|
| `risk:availability` | Hang, abort, resource exhaustion, or leaked capacity |
| `risk:compatibility` | Behavior differs from CLIPS or public contracts |
| `risk:concurrency` | Race, interleaving, cancellation, or lifecycle risk |
| `risk:correctness` | Silent or observable incorrect behavior |
| `risk:distribution` | Package cannot be built, installed, or supported reliably |
| `risk:maintainability` | Architecture makes safe changes difficult |
| `risk:memory-safety` | Undefined behavior, use-after-free, or allocator safety |
| `risk:performance` | Complexity or throughput risk |
| `risk:security` | Untrusted-input or security-boundary risk |

### Size labels

| Label | Meaning |
|---|---|
| `size/l` | Large task (3-5 days) |
| `size/m` | Medium task (1-2 days) |
| `size/s` | Small task (<= 0.5 day) |
| `size/xl` | Extra-large task that should be decomposed across several PRs |

### Workflow labels

These three operational labels were added after the 46-label audit taxonomy was
inventoried. They drive automatic selection without changing the historical
taxonomy counts above.

| Label | Meaning |
|---|---|
| `workflow:production-readiness` | Member of the automatic remediation workflow |
| `workflow:production-readiness-leaf` | One of 126 independently actionable remediation tickets |
| `workflow:production-readiness-gate` | One of 15 topic/component, audit, or program gates |

## Priority and dependency burn rules

1. **Native dependencies outrank numeric priority.** For automatic leaf
   scheduling, a blocker is covered when closed or when an open default-branch
   PR will close it; this permits intentional sequential or stacked work.
   Organizing/audit gates remain blocked until every blocker is actually
   closed. Coverage is not merge or acceptance evidence, and a dependent PR
   must not merge until its blocker issues are closed.
2. Among ready leaves, burn `p0` first, then `p1`, `p2`, and `p3`. `p0` represents a release blocker; `p1` is required before a production-readiness claim; `p2` is important hardening/quality/delivery; `p3` is follow-up optimization or polish unless it blocks a higher gate.
3. Keep dependencies native. Body checklists explain hierarchy, but scheduling and readiness come from GitHub blocked-by state.
4. Component epics close only after all their native blockers/tracked leaves are closed and their epic-level validation and acceptance criteria pass.
5. #223 starts only after its 12 component-epic blockers close. #224 closes only after #223 records PASS, or an explicitly approved PASS WITH ACCEPTED RISKS, with the immutable evidence bundle retained.

## Issue body, implementation PR, and closure contract

Each leaf body is the executable contract. Preserve its Context, Problem, evidence/reproduction and exact locations, Required change, explicit pre-fix regression tests, Validation commands, Acceptance criteria, and Dependencies/source ID. If implementation discoveries change scope or dependencies, update the issue and native graph before treating the work as ready.

Every implementation PR must:

- name the stable remediation ID and GitHub issue number;
- include the issue's pre-fix regression tests and demonstrate that they fail for the intended reason before the fix;
- implement the required change without silently narrowing acceptance criteria;
- run and report the issue's focused validation plus `just preflight-pr` before pushing or updating the PR;
- use release-profile `cargo bench` before/after evidence for performance claims; and
- update user-facing compatibility, limits, ownership, platform, or packaging documentation when behavior changes.

A leaf closes only when its native blockers are closed, regression tests and implementation are merged, validation passes, and every acceptance criterion is satisfied or explicitly moved to a separately linked issue without weakening a blocking gate. An epic closes only when every tracked child and epic-level criterion is complete. A merged PR alone is not sufficient closure evidence.

When acceptance work moves to a follow-up issue, that issue must also receive
the milestone, program/workflow classification, priority and semantic taxonomy,
and native prerequisite/gate relationships described in
[Production-readiness automatic work selection](production-readiness-work-selection.md).
A body link by itself does not keep deferred work inside the burn-down graph.

## Epic hierarchy

- **Program:** [#224 FR-PRR-000](https://github.com/plx/ferric-rules/issues/224)
  - **Final gate:** [#223 FR-AUDIT-001](https://github.com/plx/ferric-rules/issues/223)
    - [#211 FR-RETE-000](https://github.com/plx/ferric-rules/issues/211) — Epic: production-hardening and CLIPS conformance for the core runtime
      - [#225 FR-RETE-API-EPIC](https://github.com/plx/ferric-rules/issues/225) — topic roll-up for FR-RETE-029 through FR-RETE-035
    - [#212 FR-LANG-EPIC](https://github.com/plx/ferric-rules/issues/212) — Epic: make callable and RHS control flow CLIPS-conformant
    - [#213 FR-COMPAT-EPIC](https://github.com/plx/ferric-rules/issues/213) — Epic: turn compatibility tooling into non-vacuous differential evidence
    - [#214 FR-ROBUST-EPIC](https://github.com/plx/ferric-rules/issues/214) — Epic: bound hostile language input and continuously exercise hardening
    - [#217 FR-CABI-EPIC](https://github.com/plx/ferric-rules/issues/217) — Epic: harden the C ABI and pinned host boundary for production embedding
    - [#218 FR-GO-EPIC](https://github.com/plx/ferric-rules/issues/218) — Epic: make the Go bindings safe, deterministic, and idiomatic under lifecycle and concurrency stress
    - [#219 FR-NODE-EPIC](https://github.com/plx/ferric-rules/issues/219) — Epic: productionize the Node native, worker, and pool APIs
    - [#220 FR-PY-EPIC](https://github.com/plx/ferric-rules/issues/220) — Epic: harden the Python extension for threading, cleanup, and error fidelity
    - [#222 FR-BIND-EPIC](https://github.com/plx/ferric-rules/issues/222) — Epic: establish cross-binding semantic conformance
    - [#221 FR-DIST-EPIC](https://github.com/plx/ferric-rules/issues/221) — Epic: make C, Go, Node, and Python artifacts installable from clean consumer environments
    - [#215 FR-RELEASE-EPIC](https://github.com/plx/ferric-rules/issues/215) — Epic: make the Rust release, CI matrix, and dependency claims reproducible
    - [#216 FR-PERF-EPIC](https://github.com/plx/ferric-rules/issues/216) — Epic: make performance evidence prove correct and representative work

The native direction is leaf prerequisite → topic/component epic → final audit → program completion. #225 replaces seven direct leaf-to-#211 edges: FR-RETE-029 through FR-RETE-035 block #225, and #225 blocks #211.

## Complete bootstrap issue inventory

The **Native blockers** column is the live GitHub REST dependency state read for this snapshot with `per_page=100`; it is not inferred from body text.

### Program controls (2)

| Stable ID | GitHub issue | Priority | Type | Primary components | Native blockers |
|---|---|---|---|---|---|
| `FR-AUDIT-001` | [#223](https://github.com/plx/ferric-rules/issues/223) Execute the independent post-remediation production-readiness audit | `p0` | `audit` | `ci`, `packaging` | [FR-RETE-000 (#211)](https://github.com/plx/ferric-rules/issues/211)<br>[FR-LANG-EPIC (#212)](https://github.com/plx/ferric-rules/issues/212)<br>[FR-COMPAT-EPIC (#213)](https://github.com/plx/ferric-rules/issues/213)<br>[FR-ROBUST-EPIC (#214)](https://github.com/plx/ferric-rules/issues/214)<br>[FR-RELEASE-EPIC (#215)](https://github.com/plx/ferric-rules/issues/215)<br>[FR-PERF-EPIC (#216)](https://github.com/plx/ferric-rules/issues/216)<br>[FR-CABI-EPIC (#217)](https://github.com/plx/ferric-rules/issues/217)<br>[FR-GO-EPIC (#218)](https://github.com/plx/ferric-rules/issues/218)<br>[FR-NODE-EPIC (#219)](https://github.com/plx/ferric-rules/issues/219)<br>[FR-PY-EPIC (#220)](https://github.com/plx/ferric-rules/issues/220)<br>[FR-DIST-EPIC (#221)](https://github.com/plx/ferric-rules/issues/221)<br>[FR-BIND-EPIC (#222)](https://github.com/plx/ferric-rules/issues/222) |
| `FR-PRR-000` | [#224](https://github.com/plx/ferric-rules/issues/224) Production-readiness remediation program | `p0` | `epic` | `cross-cutting` | [FR-AUDIT-001 (#223)](https://github.com/plx/ferric-rules/issues/223) |

### Core runtime and RETE (37)

| Stable ID | GitHub issue | Priority | Type | Primary components | Native blockers |
|---|---|---|---|---|---|
| `FR-RETE-000` | [#211](https://github.com/plx/ferric-rules/issues/211) Epic: production-hardening and CLIPS conformance for the core runtime | `p0` | `epic` | `core`, `runtime` | [FR-RETE-001 (#103)](https://github.com/plx/ferric-rules/issues/103)<br>[FR-RETE-002 (#104)](https://github.com/plx/ferric-rules/issues/104)<br>[FR-RETE-003 (#105)](https://github.com/plx/ferric-rules/issues/105)<br>[FR-RETE-004 (#106)](https://github.com/plx/ferric-rules/issues/106)<br>[FR-RETE-005 (#107)](https://github.com/plx/ferric-rules/issues/107)<br>[FR-RETE-006 (#108)](https://github.com/plx/ferric-rules/issues/108)<br>[FR-RETE-007 (#109)](https://github.com/plx/ferric-rules/issues/109)<br>[FR-RETE-008 (#154)](https://github.com/plx/ferric-rules/issues/154)<br>[FR-RETE-009 (#155)](https://github.com/plx/ferric-rules/issues/155)<br>[FR-RETE-010 (#156)](https://github.com/plx/ferric-rules/issues/156)<br>[FR-RETE-011 (#157)](https://github.com/plx/ferric-rules/issues/157)<br>[FR-RETE-012 (#158)](https://github.com/plx/ferric-rules/issues/158)<br>[FR-RETE-013 (#159)](https://github.com/plx/ferric-rules/issues/159)<br>[FR-RETE-015 (#160)](https://github.com/plx/ferric-rules/issues/160)<br>[FR-RETE-018 (#161)](https://github.com/plx/ferric-rules/issues/161)<br>[FR-RETE-020 (#162)](https://github.com/plx/ferric-rules/issues/162)<br>[FR-RETE-021 (#163)](https://github.com/plx/ferric-rules/issues/163)<br>[FR-RETE-014 (#191)](https://github.com/plx/ferric-rules/issues/191)<br>[FR-RETE-016 (#192)](https://github.com/plx/ferric-rules/issues/192)<br>[FR-RETE-017 (#193)](https://github.com/plx/ferric-rules/issues/193)<br>[FR-RETE-019 (#194)](https://github.com/plx/ferric-rules/issues/194)<br>[FR-RETE-022 (#195)](https://github.com/plx/ferric-rules/issues/195)<br>[FR-RETE-023 (#196)](https://github.com/plx/ferric-rules/issues/196)<br>[FR-RETE-024 (#197)](https://github.com/plx/ferric-rules/issues/197)<br>[FR-RETE-025 (#198)](https://github.com/plx/ferric-rules/issues/198)<br>[FR-RETE-026 (#199)](https://github.com/plx/ferric-rules/issues/199)<br>[FR-RETE-027 (#200)](https://github.com/plx/ferric-rules/issues/200)<br>[FR-RETE-028 (#201)](https://github.com/plx/ferric-rules/issues/201)<br>[FR-RETE-API-EPIC (#225)](https://github.com/plx/ferric-rules/issues/225) |
| `FR-RETE-API-EPIC` | [#225](https://github.com/plx/ferric-rules/issues/225) Epic: harden public API and explicit compatibility-scope behavior | `p1` | `epic` | `core`, `runtime` | [FR-RETE-033 (#164)](https://github.com/plx/ferric-rules/issues/164)<br>[FR-RETE-029 (#202)](https://github.com/plx/ferric-rules/issues/202)<br>[FR-RETE-030 (#203)](https://github.com/plx/ferric-rules/issues/203)<br>[FR-RETE-031 (#204)](https://github.com/plx/ferric-rules/issues/204)<br>[FR-RETE-034 (#205)](https://github.com/plx/ferric-rules/issues/205)<br>[FR-RETE-032 (#209)](https://github.com/plx/ferric-rules/issues/209)<br>[FR-RETE-035 (#210)](https://github.com/plx/ferric-rules/issues/210) |
| `FR-RETE-001` | [#103](https://github.com/plx/ferric-rules/issues/103) Make fact duplication behavior CLIPS-compatible and observable | `p0` | `defect` | `core`, `runtime` | — |
| `FR-RETE-002` | [#104](https://github.com/plx/ferric-rules/issues/104) Seed leading simple negative conditional elements | `p0` | `defect` | `core`, `runtime` | — |
| `FR-RETE-003` | [#105](https://github.com/plx/ferric-rules/issues/105) Backfill existing working memory when compiling rules online | `p0` | `defect` | `core`, `runtime` | — |
| `FR-RETE-004` | [#106](https://github.com/plx/ferric-rules/issues/106) Evaluate test conditional elements during matching | `p0` | `defect` | `core`, `runtime` | — |
| `FR-RETE-005` | [#107](https://github.com/plx/ferric-rules/issues/107) Compile double negation with existential semantics | `p0` | `defect` | `core`, `runtime` | — |
| `FR-RETE-006` | [#108](https://github.com/plx/ferric-rules/issues/108) Give multi-pattern exists tuple-level existential semantics | `p0` | `defect` | `core`, `runtime` | — |
| `FR-RETE-007` | [#109](https://github.com/plx/ferric-rules/issues/109) Make individual rule compilation and installation failure-atomic | `p0` | `defect` | `core`, `runtime` | — |
| `FR-RETE-008` | [#154](https://github.com/plx/ferric-rules/issues/154) Implement CLIPS depth and breadth activation ordering | `p1` | `defect` | `core`, `runtime` | — |
| `FR-RETE-009` | [#155](https://github.com/plx/ferric-rules/issues/155) Implement CLIPS LEX and MEA conflict ordering | `p1` | `defect` | `core`, `runtime` | [FR-RETE-008 (#154)](https://github.com/plx/ferric-rules/issues/154) |
| `FR-RETE-010` | [#156](https://github.com/plx/ferric-rules/issues/156) Assert initial-fact before deffacts during reset | `p1` | `defect` | `runtime` | — |
| `FR-RETE-011` | [#157](https://github.com/plx/ferric-rules/issues/157) Replace an existing rule on same-name redefinition | `p1` | `defect` | `core`, `runtime` | [FR-RETE-007 (#109)](https://github.com/plx/ferric-rules/issues/109) |
| `FR-RETE-012` | [#158](https://github.com/plx/ferric-rules/issues/158) Reject or safely migrate in-use template redefinitions | `p1` | `defect` | `core`, `runtime` | — |
| `FR-RETE-013` | [#159](https://github.com/plx/ferric-rules/issues/159) Define and implement transactional source-load semantics | `p1` | `hardening` | `runtime` | [FR-RETE-007 (#109)](https://github.com/plx/ferric-rules/issues/109) |
| `FR-RETE-014` | [#191](https://github.com/plx/ferric-rules/issues/191) Reclaim RETE network state when rules are undefined | `p2` | `architecture` | `core`, `runtime` | [FR-RETE-007 (#109)](https://github.com/plx/ferric-rules/issues/109) |
| `FR-RETE-015` | [#160](https://github.com/plx/ferric-rules/issues/160) Treat omitted defmodule exports as exporting nothing | `p1` | `defect` | `runtime` | — |
| `FR-RETE-016` | [#192](https://github.com/plx/ferric-rules/issues/192) Apply focus changes immediately during RHS execution | `p2` | `defect` | `runtime` | — |
| `FR-RETE-017` | [#193](https://github.com/plx/ferric-rules/issues/193) Drain the focus stack consistently when run completes | `p2` | `defect` | `runtime` | [FR-RETE-016 (#192)](https://github.com/plx/ferric-rules/issues/192) |
| `FR-RETE-018` | [#161](https://github.com/plx/ferric-rules/issues/161) Model named deffacts definitions and assert them only on reset | `p1` | `defect` | `runtime` | [FR-RETE-010 (#156)](https://github.com/plx/ferric-rules/issues/156) |
| `FR-RETE-019` | [#194](https://github.com/plx/ferric-rules/issues/194) Version, validate, and bound engine snapshots | `p2` | `hardening` | `core`, `runtime` | [FR-RETE-020 (#162)](https://github.com/plx/ferric-rules/issues/162) |
| `FR-RETE-020` | [#162](https://github.com/plx/ferric-rules/issues/162) Complete cross-structure RETE invariant validation | `p1` | `hardening` | `core`, `runtime` | — |
| `FR-RETE-021` | [#163](https://github.com/plx/ferric-rules/issues/163) Replace global auxiliary-memory cleanup scans with owner indexes | `p1` | `architecture` | `core` | [FR-RETE-020 (#162)](https://github.com/plx/ferric-rules/issues/162) |
| `FR-RETE-022` | [#195](https://github.com/plx/ferric-rules/issues/195) Use equality indexes for negative and exists right activation | `p2` | `architecture` | `core` | — |
| `FR-RETE-023` | [#196](https://github.com/plx/ferric-rules/issues/196) Index agenda selection by focused module | `p2` | `architecture` | `core`, `runtime` | — |
| `FR-RETE-024` | [#197](https://github.com/plx/ferric-rules/issues/197) Share alpha-network prefixes instead of only exact full paths | `p2` | `architecture` | `core` | — |
| `FR-RETE-025` | [#198](https://github.com/plx/ferric-rules/issues/198) Eliminate quadratic beta-child attachment during compilation | `p2` | `architecture` | `core` | — |
| `FR-RETE-026` | [#199](https://github.com/plx/ferric-rules/issues/199) Reduce full binding-set cloning across beta tokens | `p2` | `architecture` | `core`, `runtime` | — |
| `FR-RETE-027` | [#200](https://github.com/plx/ferric-rules/issues/200) Bound and diagnose OR-conditional-element expansion | `p2` | `hardening` | `parser`, `runtime` | — |
| `FR-RETE-028` | [#201](https://github.com/plx/ferric-rules/issues/201) Make alpha and beta propagation stack-safe with work limits | `p2` | `hardening` | `core`, `runtime` | — |
| `FR-RETE-029` | [#202](https://github.com/plx/ferric-rules/issues/202) Reject template slot-name/value cardinality mismatches | `p2` | `defect` | `core`, `runtime` | — |
| `FR-RETE-030` | [#203](https://github.com/plx/ferric-rules/issues/203) Validate provenance of host-supplied facts, symbols, and template IDs | `p2` | `hardening` | `core`, `runtime` | — |
| `FR-RETE-031` | [#204](https://github.com/plx/ferric-rules/issues/204) Make synthetic initial-fact tracking lifecycle-safe and query-consistent | `p2` | `defect` | `core`, `runtime` | — |
| `FR-RETE-032` | [#209](https://github.com/plx/ferric-rules/issues/209) Implement CLIPS-compatible module ownership and fact visibility | `p3` | `architecture` | `core`, `runtime` | [FR-RETE-015 (#160)](https://github.com/plx/ferric-rules/issues/160) |
| `FR-RETE-033` | [#164](https://github.com/plx/ferric-rules/issues/164) Reject logical CEs until truth maintenance is implemented | `p1` | `defect` | `runtime` | — |
| `FR-RETE-034` | [#205](https://github.com/plx/ferric-rules/issues/205) Implement dynamic salience modes and functional refresh-agenda | `p2` | `defect` | `core`, `runtime` | [FR-RETE-008 (#154)](https://github.com/plx/ferric-rules/issues/154)<br>[FR-RETE-009 (#155)](https://github.com/plx/ferric-rules/issues/155) |
| `FR-RETE-035` | [#210](https://github.com/plx/ferric-rules/issues/210) Complete the CLIPS conflict-strategy surface or document a strict subset | `p3` | `delivery` | `core`, `runtime` | [FR-RETE-008 (#154)](https://github.com/plx/ferric-rules/issues/154)<br>[FR-RETE-009 (#155)](https://github.com/plx/ferric-rules/issues/155) |

### Language and control flow (4)

| Stable ID | GitHub issue | Priority | Type | Primary components | Native blockers |
|---|---|---|---|---|---|
| `FR-LANG-EPIC` | [#212](https://github.com/plx/ferric-rules/issues/212) Epic: make callable and RHS control flow CLIPS-conformant | `p0` | `epic` | `runtime` | [FR-LANG-001 (#98)](https://github.com/plx/ferric-rules/issues/98)<br>[FR-LANG-002 (#99)](https://github.com/plx/ferric-rules/issues/99)<br>[FR-LANG-003 (#180)](https://github.com/plx/ferric-rules/issues/180) |
| `FR-LANG-001` | [#98](https://github.com/plx/ferric-rules/issues/98) Implement non-local return from deffunction callables | `p0` | `defect` | `runtime` | — |
| `FR-LANG-002` | [#99](https://github.com/plx/ferric-rules/issues/99) Stop the current RHS action sequence after an evaluation error | `p0` | `defect` | `runtime` | — |
| `FR-LANG-003` | [#180](https://github.com/plx/ferric-rules/issues/180) Consolidate duplicated evaluator and action control-flow semantics | `p2` | `architecture` | `runtime` | [FR-LANG-001 (#98)](https://github.com/plx/ferric-rules/issues/98)<br>[FR-LANG-002 (#99)](https://github.com/plx/ferric-rules/issues/99)<br>[FR-ROBUST-002 (#111)](https://github.com/plx/ferric-rules/issues/111) |

### CLIPS compatibility tooling (9)

| Stable ID | GitHub issue | Priority | Type | Primary components | Native blockers |
|---|---|---|---|---|---|
| `FR-COMPAT-EPIC` | [#213](https://github.com/plx/ferric-rules/issues/213) Epic: turn compatibility tooling into non-vacuous differential evidence | `p0` | `epic` | `compat-tool`, `ci` | [FR-COMPAT-001 (#88)](https://github.com/plx/ferric-rules/issues/88)<br>[FR-COMPAT-002 (#89)](https://github.com/plx/ferric-rules/issues/89)<br>[FR-COMPAT-003 (#90)](https://github.com/plx/ferric-rules/issues/90)<br>[FR-COMPAT-004 (#91)](https://github.com/plx/ferric-rules/issues/91)<br>[FR-COMPAT-007 (#92)](https://github.com/plx/ferric-rules/issues/92)<br>[FR-COMPAT-008 (#93)](https://github.com/plx/ferric-rules/issues/93)<br>[FR-COMPAT-005 (#119)](https://github.com/plx/ferric-rules/issues/119)<br>[FR-COMPAT-006 (#120)](https://github.com/plx/ferric-rules/issues/120) |
| `FR-COMPAT-001` | [#88](https://github.com/plx/ferric-rules/issues/88) Attach generated library harnesses to compatibility manifest entries | `p0` | `defect` | `compat-tool` | — |
| `FR-COMPAT-002` | [#89](https://github.com/plx/ferric-rules/issues/89) Run composed compatibility harnesses from a CLIPS-visible path | `p0` | `defect` | `compat-tool` | [FR-COMPAT-001 (#88)](https://github.com/plx/ferric-rules/issues/88) |
| `FR-COMPAT-003` | [#90](https://github.com/plx/ferric-rules/issues/90) Guarantee generated verifier execution regardless of ending module | `p0` | `defect` | `compat-tool` | [FR-COMPAT-001 (#88)](https://github.com/plx/ferric-rules/issues/88) |
| `FR-COMPAT-004` | [#91](https://github.com/plx/ferric-rules/issues/91) Reject vacuous empty-output compatibility results with structured oracles | `p0` | `architecture` | `compat-tool` | [FR-COMPAT-003 (#90)](https://github.com/plx/ferric-rules/issues/90) |
| `FR-COMPAT-005` | [#119](https://github.com/plx/ferric-rules/issues/119) Classify CLIPS diagnostics by execution phase instead of bracket syntax | `p1` | `defect` | `compat-tool` | — |
| `FR-COMPAT-006` | [#120](https://github.com/plx/ferric-rules/issues/120) Make compatibility feature scanning string- and comment-aware | `p1` | `defect` | `parser`, `compat-tool` | — |
| `FR-COMPAT-007` | [#92](https://github.com/plx/ferric-rules/issues/92) Run harness generation and blocking differential assessment in CI | `p0` | `delivery` | `compat-tool`, `ci` | [FR-COMPAT-001 (#88)](https://github.com/plx/ferric-rules/issues/88)<br>[FR-COMPAT-002 (#89)](https://github.com/plx/ferric-rules/issues/89)<br>[FR-COMPAT-003 (#90)](https://github.com/plx/ferric-rules/issues/90)<br>[FR-COMPAT-004 (#91)](https://github.com/plx/ferric-rules/issues/91)<br>[FR-COMPAT-005 (#119)](https://github.com/plx/ferric-rules/issues/119)<br>[FR-COMPAT-006 (#120)](https://github.com/plx/ferric-rules/issues/120) |
| `FR-COMPAT-008` | [#93](https://github.com/plx/ferric-rules/issues/93) Add a real pinned-CLIPS differential semantic test lane | `p0` | `audit` | `runtime`, `compat-tool`, `ci` | [FR-COMPAT-004 (#91)](https://github.com/plx/ferric-rules/issues/91)<br>[FR-COMPAT-005 (#119)](https://github.com/plx/ferric-rules/issues/119) |

### Robustness (4)

| Stable ID | GitHub issue | Priority | Type | Primary components | Native blockers |
|---|---|---|---|---|---|
| `FR-ROBUST-EPIC` | [#214](https://github.com/plx/ferric-rules/issues/214) Epic: bound hostile language input and continuously exercise hardening | `p0` | `epic` | `parser`, `runtime`, `ci` | [FR-ROBUST-001 (#110)](https://github.com/plx/ferric-rules/issues/110)<br>[FR-ROBUST-002 (#111)](https://github.com/plx/ferric-rules/issues/111)<br>[FR-ROBUST-003 (#165)](https://github.com/plx/ferric-rules/issues/165) |
| `FR-ROBUST-001` | [#110](https://github.com/plx/ferric-rules/issues/110) Bound S-expression nesting without process stack overflow | `p0` | `hardening` | `parser` | — |
| `FR-ROBUST-002` | [#111](https://github.com/plx/ferric-rules/issues/111) Apply one action-level iteration budget to loop-for-count and while | `p0` | `hardening` | `runtime` | — |
| `FR-ROBUST-003` | [#165](https://github.com/plx/ferric-rules/issues/165) Add persistent parser/runtime/snapshot fuzz targets and CI smoke runs | `p1` | `hardening` | `parser`, `runtime`, `ci` | [FR-ROBUST-001 (#110)](https://github.com/plx/ferric-rules/issues/110)<br>[FR-ROBUST-002 (#111)](https://github.com/plx/ferric-rules/issues/111)<br>[FR-RETE-019 (#194)](https://github.com/plx/ferric-rules/issues/194)<br>[FR-RETE-027 (#200)](https://github.com/plx/ferric-rules/issues/200)<br>[FR-RETE-028 (#201)](https://github.com/plx/ferric-rules/issues/201) |

### C ABI and pinned boundary (13)

| Stable ID | GitHub issue | Priority | Type | Primary components | Native blockers |
|---|---|---|---|---|---|
| `FR-CABI-EPIC` | [#217](https://github.com/plx/ferric-rules/issues/217) Epic: harden the C ABI and pinned host boundary for production embedding | `p0` | `epic` | `c-abi`, `pinned` | [FR-CABI-001 (#85)](https://github.com/plx/ferric-rules/issues/85)<br>[FR-CABI-002 (#86)](https://github.com/plx/ferric-rules/issues/86)<br>[FR-CABI-008 (#87)](https://github.com/plx/ferric-rules/issues/87)<br>[FR-CABI-003 (#113)](https://github.com/plx/ferric-rules/issues/113)<br>[FR-CABI-004 (#114)](https://github.com/plx/ferric-rules/issues/114)<br>[FR-CABI-005 (#115)](https://github.com/plx/ferric-rules/issues/115)<br>[FR-CABI-006 (#116)](https://github.com/plx/ferric-rules/issues/116)<br>[FR-CABI-007 (#117)](https://github.com/plx/ferric-rules/issues/117)<br>[FR-CABI-009 (#118)](https://github.com/plx/ferric-rules/issues/118)<br>[FR-CABI-010 (#168)](https://github.com/plx/ferric-rules/issues/168)<br>[FR-CABI-011 (#169)](https://github.com/plx/ferric-rules/issues/169)<br>[FR-CABI-012 (#170)](https://github.com/plx/ferric-rules/issues/170) |
| `FR-CABI-001` | [#85](https://github.com/plx/ferric-rules/issues/85) Replace caller-populated Rust enum fields with fixed-width validated ABI integers | `p0` | `defect` | `c-abi` | — |
| `FR-CABI-002` | [#86](https://github.com/plx/ferric-rules/issues/86) Synchronize per-engine diagnostics or restore an explicit thread-affinity contract | `p0` | `defect` | `c-abi` | — |
| `FR-CABI-003` | [#113](https://github.com/plx/ferric-rules/issues/113) Standardize global and per-engine error-channel updates | `p1` | `defect` | `c-abi`, `go` | — |
| `FR-CABI-004` | [#114](https://github.com/plx/ferric-rules/issues/114) Give output-buffer pointers engine-scoped lifetime and bounded storage | `p1` | `defect` | `c-abi` | — |
| `FR-CABI-005` | [#115](https://github.com/plx/ferric-rules/issues/115) Define and enforce a lossless embedded-NUL string policy | `p1` | `defect` | `c-abi`, `go`, `node`, `python` | — |
| `FR-CABI-006` | [#116](https://github.com/plx/ferric-rules/issues/116) Contain Rust panics at every C host boundary | `p1` | `hardening` | `c-abi` | — |
| `FR-CABI-007` | [#117](https://github.com/plx/ferric-rules/issues/117) Guarantee exactly-once pinned async completion and registry cleanup on request panic | `p1` | `defect` | `c-abi`, `pinned` | [FR-CABI-006 (#116)](https://github.com/plx/ferric-rules/issues/116) |
| `FR-CABI-008` | [#87](https://github.com/plx/ferric-rules/issues/87) Provide allocator-owned multifield construction and explicit provenance | `p0` | `defect` | `c-abi`, `go` | — |
| `FR-CABI-009` | [#118](https://github.com/plx/ferric-rules/issues/118) Expose logical-run continuation for batched and cancelable hosts | `p1` | `architecture` | `c-abi`, `go`, `node` | — |
| `FR-CABI-010` | [#168](https://github.com/plx/ferric-rules/issues/168) Add an extended step API that returns fired-rule metadata | `p2` | `architecture` | `c-abi`, `go` | — |
| `FR-CABI-011` | [#169](https://github.com/plx/ferric-rules/issues/169) Export an ABI version and capability-negotiation contract | `p2` | `hardening` | `c-abi`, `packaging` | — |
| `FR-CABI-012` | [#170](https://github.com/plx/ferric-rules/issues/170) Stop Cargo builds from rewriting the tracked C header | `p2` | `delivery` | `c-abi`, `ci`, `packaging` | — |

### Go binding (17)

| Stable ID | GitHub issue | Priority | Type | Primary components | Native blockers |
|---|---|---|---|---|---|
| `FR-GO-EPIC` | [#218](https://github.com/plx/ferric-rules/issues/218) Epic: make the Go bindings safe, deterministic, and idiomatic under lifecycle and concurrency stress | `p0` | `epic` | `c-abi`, `pinned`, `go` | [FR-GO-001 (#96)](https://github.com/plx/ferric-rules/issues/96)<br>[FR-GO-002 (#97)](https://github.com/plx/ferric-rules/issues/97)<br>[FR-GO-003 (#125)](https://github.com/plx/ferric-rules/issues/125)<br>[FR-GO-004 (#126)](https://github.com/plx/ferric-rules/issues/126)<br>[FR-GO-005 (#127)](https://github.com/plx/ferric-rules/issues/127)<br>[FR-GO-006 (#128)](https://github.com/plx/ferric-rules/issues/128)<br>[FR-GO-007 (#129)](https://github.com/plx/ferric-rules/issues/129)<br>[FR-GO-008 (#130)](https://github.com/plx/ferric-rules/issues/130)<br>[FR-GO-012 (#131)](https://github.com/plx/ferric-rules/issues/131)<br>[FR-GO-016 (#132)](https://github.com/plx/ferric-rules/issues/132)<br>[FR-GO-009 (#174)](https://github.com/plx/ferric-rules/issues/174)<br>[FR-GO-010 (#175)](https://github.com/plx/ferric-rules/issues/175)<br>[FR-GO-011 (#176)](https://github.com/plx/ferric-rules/issues/176)<br>[FR-GO-013 (#177)](https://github.com/plx/ferric-rules/issues/177)<br>[FR-GO-014 (#178)](https://github.com/plx/ferric-rules/issues/178)<br>[FR-GO-015 (#179)](https://github.com/plx/ferric-rules/issues/179) |
| `FR-GO-001` | [#96](https://github.com/plx/ferric-rules/issues/96) Make every Go Engine method safe and deterministic after Close | `p0` | `defect` | `c-abi`, `go` | — |
| `FR-GO-002` | [#97](https://github.com/plx/ferric-rules/issues/97) Eliminate the Go/C/Rust multifield allocator mismatch | `p0` | `defect` | `c-abi`, `go` | [FR-CABI-008 (#87)](https://github.com/plx/ferric-rules/issues/87) |
| `FR-GO-003` | [#125](https://github.com/plx/ferric-rules/issues/125) Track Go engine-option presence independently from zero/default values | `p1` | `defect` | `go` | — |
| `FR-GO-004` | [#126](https://github.com/plx/ferric-rules/issues/126) Return the current Go operation's error text instead of stale parse diagnostics | `p1` | `defect` | `c-abi`, `go` | [FR-CABI-003 (#113)](https://github.com/plx/ferric-rules/issues/113) |
| `FR-GO-005` | [#127](https://github.com/plx/ferric-rules/issues/127) Preserve halt and diagnostics across Go cancelable run batches | `p1` | `defect` | `c-abi`, `go` | [FR-CABI-009 (#118)](https://github.com/plx/ferric-rules/issues/118) |
| `FR-GO-006` | [#128](https://github.com/plx/ferric-rules/issues/128) Allow PinnedEngine Halt and Close to interrupt an active unlimited run | `p1` | `architecture` | `pinned`, `go` | [FR-GO-005 (#127)](https://github.com/plx/ferric-rules/issues/127) |
| `FR-GO-007` | [#129](https://github.com/plx/ferric-rules/issues/129) Recover callback panics inside PinnedEngine and Coordinator workers | `p1` | `defect` | `pinned`, `go` | — |
| `FR-GO-008` | [#130](https://github.com/plx/ferric-rules/issues/130) Remove post-dispatch cancellation races and shared captured result slots | `p1` | `defect` | `pinned`, `go` | — |
| `FR-GO-009` | [#174](https://github.com/plx/ferric-rules/issues/174) Make all concurrent Coordinator.Close callers wait for full shutdown | `p2` | `defect` | `pinned`, `go` | — |
| `FR-GO-010` | [#175](https://github.com/plx/ferric-rules/issues/175) Stop swallowing dispatch and closed-state errors in Go convenience methods | `p2` | `hardening` | `pinned`, `go` | [FR-GO-001 (#96)](https://github.com/plx/ferric-rules/issues/96) |
| `FR-GO-011` | [#176](https://github.com/plx/ferric-rules/issues/176) Reject serialization payload lengths that overflow C.int | `p2` | `defect` | `c-abi`, `go` | — |
| `FR-GO-012` | [#131](https://github.com/plx/ferric-rules/issues/131) Reject embedded NUL in every Go string passed through C.CString | `p1` | `defect` | `c-abi`, `go` | [FR-CABI-005 (#115)](https://github.com/plx/ferric-rules/issues/115) |
| `FR-GO-013` | [#177](https://github.com/plx/ferric-rules/issues/177) Enforce documented source/snapshot option combinations and restored configuration | `p2` | `defect` | `go` | [FR-GO-003 (#125)](https://github.com/plx/ferric-rules/issues/125) |
| `FR-GO-014` | [#178](https://github.com/plx/ferric-rules/issues/178) Populate Go FiredRule.RuleName from the step operation | `p2` | `defect` | `c-abi`, `go` | [FR-CABI-010 (#168)](https://github.com/plx/ferric-rules/issues/168) |
| `FR-GO-015` | [#179](https://github.com/plx/ferric-rules/issues/179) Reject negative Go run limits instead of treating them as unlimited | `p2` | `defect` | `go` | — |
| `FR-GO-016` | [#132](https://github.com/plx/ferric-rules/issues/132) Validate internal Go template slice lengths before indexing or calling C | `p1` | `defect` | `c-abi`, `go` | — |

### Node binding (18)

| Stable ID | GitHub issue | Priority | Type | Primary components | Native blockers |
|---|---|---|---|---|---|
| `FR-NODE-EPIC` | [#219](https://github.com/plx/ferric-rules/issues/219) Epic: productionize the Node native, worker, and pool APIs | `p1` | `epic` | `pinned`, `node` | [FR-NODE-001 (#133)](https://github.com/plx/ferric-rules/issues/133)<br>[FR-NODE-002 (#134)](https://github.com/plx/ferric-rules/issues/134)<br>[FR-NODE-003 (#135)](https://github.com/plx/ferric-rules/issues/135)<br>[FR-NODE-004 (#136)](https://github.com/plx/ferric-rules/issues/136)<br>[FR-NODE-005 (#137)](https://github.com/plx/ferric-rules/issues/137)<br>[FR-NODE-007 (#138)](https://github.com/plx/ferric-rules/issues/138)<br>[FR-NODE-008 (#139)](https://github.com/plx/ferric-rules/issues/139)<br>[FR-NODE-009 (#140)](https://github.com/plx/ferric-rules/issues/140)<br>[FR-NODE-011 (#141)](https://github.com/plx/ferric-rules/issues/141)<br>[FR-NODE-006 (#181)](https://github.com/plx/ferric-rules/issues/181)<br>[FR-NODE-010 (#182)](https://github.com/plx/ferric-rules/issues/182)<br>[FR-NODE-012 (#183)](https://github.com/plx/ferric-rules/issues/183)<br>[FR-NODE-013 (#184)](https://github.com/plx/ferric-rules/issues/184)<br>[FR-NODE-016 (#185)](https://github.com/plx/ferric-rules/issues/185)<br>[FR-NODE-017 (#186)](https://github.com/plx/ferric-rules/issues/186)<br>[FR-NODE-014 (#206)](https://github.com/plx/ferric-rules/issues/206)<br>[FR-NODE-015 (#207)](https://github.com/plx/ferric-rules/issues/207) |
| `FR-NODE-001` | [#133](https://github.com/plx/ferric-rules/issues/133) Make Node fact identifiers lossless beyond JavaScript's safe-integer range | `p1` | `defect` | `node` | — |
| `FR-NODE-002` | [#134](https://github.com/plx/ferric-rules/issues/134) Preserve logical-run semantics in Node worker batching | `p1` | `defect` | `pinned`, `node` | — |
| `FR-NODE-003` | [#135](https://github.com/plx/ferric-rules/issues/135) Give EnginePool.do an exclusive worker lease for the entire callback | `p1` | `defect` | `node` | — |
| `FR-NODE-004` | [#136](https://github.com/plx/ferric-rules/issues/136) Terminate EngineHandle workers when initialization rejects | `p1` | `defect` | `node` | — |
| `FR-NODE-005` | [#137](https://github.com/plx/ferric-rules/issues/137) Make worker error/exit a terminal state for all pool work | `p1` | `defect` | `node` | — |
| `FR-NODE-006` | [#181](https://github.com/plx/ferric-rules/issues/181) Remove queued AbortSignal listeners when pool work is dispatched | `p2` | `defect` | `node` | — |
| `FR-NODE-007` | [#138](https://github.com/plx/ferric-rules/issues/138) Validate EnginePool thread count as a finite bounded integer | `p1` | `defect` | `node` | — |
| `FR-NODE-008` | [#139](https://github.com/plx/ferric-rules/issues/139) Roll back Node request bookkeeping when postMessage throws synchronously | `p1` | `defect` | `node` | — |
| `FR-NODE-009` | [#140](https://github.com/plx/ferric-rules/issues/140) Invalidate pool proxies on cancellation and define post-dispatch mutation semantics | `p1` | `defect` | `node` | [FR-NODE-003 (#135)](https://github.com/plx/ferric-rules/issues/135) |
| `FR-NODE-010` | [#182](https://github.com/plx/ferric-rules/issues/182) Make concurrent Node close calls share one completion barrier | `p2` | `defect` | `node` | — |
| `FR-NODE-011` | [#141](https://github.com/plx/ferric-rules/issues/141) Add bounded backpressure to the Node engine pool queue | `p1` | `architecture` | `node` | — |
| `FR-NODE-012` | [#183](https://github.com/plx/ferric-rules/issues/183) Make Node run limits and fired counts checked and lossless | `p2` | `defect` | `node` | — |
| `FR-NODE-013` | [#184](https://github.com/plx/ferric-rules/issues/184) Map every runtime LoadError variant to a stable Node error class | `p2` | `defect` | `node` | — |
| `FR-NODE-014` | [#206](https://github.com/plx/ferric-rules/issues/206) Validate the value of the Node tagged-symbol marker | `p3` | `hardening` | `node` | — |
| `FR-NODE-015` | [#207](https://github.com/plx/ferric-rules/issues/207) Convert native-addon property getter failures through the Node error mapper | `p3` | `defect` | `node` | — |
| `FR-NODE-016` | [#185](https://github.com/plx/ferric-rules/issues/185) Provide non-blocking alternatives for expensive synchronous Node engine operations | `p2` | `architecture` | `node` | — |
| `FR-NODE-017` | [#186](https://github.com/plx/ferric-rules/issues/186) Align the declared Node version range with Symbol.asyncDispose usage | `p2` | `delivery` | `node`, `ci`, `packaging` | — |

### Python binding (8)

| Stable ID | GitHub issue | Priority | Type | Primary components | Native blockers |
|---|---|---|---|---|---|
| `FR-PY-EPIC` | [#220](https://github.com/plx/ferric-rules/issues/220) Epic: harden the Python extension for threading, cleanup, and error fidelity | `p1` | `epic` | `pinned`, `python` | [FR-PY-001 (#146)](https://github.com/plx/ferric-rules/issues/146)<br>[FR-PY-002 (#147)](https://github.com/plx/ferric-rules/issues/147)<br>[FR-PY-003 (#187)](https://github.com/plx/ferric-rules/issues/187)<br>[FR-PY-004 (#188)](https://github.com/plx/ferric-rules/issues/188)<br>[FR-PY-005 (#189)](https://github.com/plx/ferric-rules/issues/189)<br>[FR-PY-007 (#190)](https://github.com/plx/ferric-rules/issues/190)<br>[FR-PY-006 (#208)](https://github.com/plx/ferric-rules/issues/208) |
| `FR-PY-001` | [#146](https://github.com/plx/ferric-rules/issues/146) Destroy Python engines when the final reference drops on a foreign thread | `p1` | `defect` | `python` | — |
| `FR-PY-002` | [#147](https://github.com/plx/ferric-rules/issues/147) Release the Python GIL during long engine, serialization, and file operations | `p1` | `architecture` | `pinned`, `python` | — |
| `FR-PY-003` | [#187](https://github.com/plx/ferric-rules/issues/187) Expose max_call_depth in Python engine construction | `p2` | `defect` | `python` | — |
| `FR-PY-004` | [#188](https://github.com/plx/ferric-rules/issues/188) Add a dedicated Python serialization exception | `p2` | `defect` | `python` | — |
| `FR-PY-005` | [#189](https://github.com/plx/ferric-rules/issues/189) Map every runtime LoadError variant to a stable Python exception | `p2` | `defect` | `python` | — |
| `FR-PY-006` | [#208](https://github.com/plx/ferric-rules/issues/208) Fix non-transitive equality for Python Symbol and String wrappers | `p3` | `defect` | `python` | — |
| `FR-PY-007` | [#190](https://github.com/plx/ferric-rules/issues/190) Provide a thread-safe pinned Python engine facade | `p2` | `architecture` | `pinned`, `python` | — |

### Cross-binding conformance (4)

| Stable ID | GitHub issue | Priority | Type | Primary components | Native blockers |
|---|---|---|---|---|---|
| `FR-BIND-EPIC` | [#222](https://github.com/plx/ferric-rules/issues/222) Epic: establish cross-binding semantic conformance | `p1` | `epic` | `c-abi`, `go`, `node`, `python`, `ci` | [FR-BIND-003 (#112)](https://github.com/plx/ferric-rules/issues/112)<br>[FR-BIND-001 (#166)](https://github.com/plx/ferric-rules/issues/166)<br>[FR-BIND-002 (#167)](https://github.com/plx/ferric-rules/issues/167) |
| `FR-BIND-001` | [#166](https://github.com/plx/ferric-rules/issues/166) Choose and unify plain host-string semantics across bindings | `p2` | `architecture` | `c-abi`, `go`, `node`, `python` | — |
| `FR-BIND-002` | [#167](https://github.com/plx/ferric-rules/issues/167) Stop silently converting ExternalAddress values to null in Node and Python | `p2` | `defect` | `c-abi`, `go`, `node`, `python` | — |
| `FR-BIND-003` | [#112](https://github.com/plx/ferric-rules/issues/112) Create a shared cross-binding semantic conformance matrix | `p1` | `audit` | `c-abi`, `go`, `node`, `python`, `ci` | — |

### Distribution (10)

| Stable ID | GitHub issue | Priority | Type | Primary components | Native blockers |
|---|---|---|---|---|---|
| `FR-DIST-EPIC` | [#221](https://github.com/plx/ferric-rules/issues/221) Epic: make C, Go, Node, and Python artifacts installable from clean consumer environments | `p0` | `epic` | `c-abi`, `go`, `node`, `python`, `ci`, `packaging` | [FR-DIST-001 (#94)](https://github.com/plx/ferric-rules/issues/94)<br>[FR-DIST-004 (#95)](https://github.com/plx/ferric-rules/issues/95)<br>[FR-DIST-002 (#121)](https://github.com/plx/ferric-rules/issues/121)<br>[FR-DIST-006 (#122)](https://github.com/plx/ferric-rules/issues/122)<br>[FR-DIST-007 (#123)](https://github.com/plx/ferric-rules/issues/123)<br>[FR-DIST-008 (#124)](https://github.com/plx/ferric-rules/issues/124)<br>[FR-DIST-003 (#171)](https://github.com/plx/ferric-rules/issues/171)<br>[FR-DIST-005 (#172)](https://github.com/plx/ferric-rules/issues/172)<br>[FR-DIST-009 (#173)](https://github.com/plx/ferric-rules/issues/173) |
| `FR-DIST-001` | [#94](https://github.com/plx/ferric-rules/issues/94) Publish an npm package that actually contains or resolves a native addon | `p0` | `delivery` | `node`, `ci`, `packaging` | — |
| `FR-DIST-002` | [#121](https://github.com/plx/ferric-rules/issues/121) Expand and correctly detect the Node native platform/libc matrix | `p1` | `delivery` | `node`, `ci`, `packaging` | [FR-DIST-001 (#94)](https://github.com/plx/ferric-rules/issues/94) |
| `FR-DIST-003` | [#171](https://github.com/plx/ferric-rules/issues/171) Define and test the Node CommonJS and ESM export contract | `p2` | `delivery` | `node`, `ci`, `packaging` | [FR-DIST-001 (#94)](https://github.com/plx/ferric-rules/issues/94) |
| `FR-DIST-004` | [#95](https://github.com/plx/ferric-rules/issues/95) Make the Go module self-linking and cross-platform for clean consumers | `p0` | `delivery` | `c-abi`, `go`, `ci`, `packaging` | — |
| `FR-DIST-005` | [#172](https://github.com/plx/ferric-rules/issues/172) Split optional Go integrations out of the base module dependency graph | `p2` | `architecture` | `go`, `packaging` | — |
| `FR-DIST-006` | [#122](https://github.com/plx/ferric-rules/issues/122) Make Python 3.14 support metadata truthful | `p1` | `delivery` | `python`, `ci`, `packaging` | — |
| `FR-DIST-007` | [#123](https://github.com/plx/ferric-rules/issues/123) Publish and test a Python wheel matrix, with an explicit abi3 decision | `p1` | `delivery` | `python`, `ci`, `packaging` | [FR-DIST-006 (#122)](https://github.com/plx/ferric-rules/issues/122) |
| `FR-DIST-008` | [#124](https://github.com/plx/ferric-rules/issues/124) Add cross-platform, minimum-version, and clean-consumer binding CI | `p1` | `delivery` | `c-abi`, `go`, `node`, `python`, `ci`, `packaging` | — |
| `FR-DIST-009` | [#173](https://github.com/plx/ferric-rules/issues/173) Ship a versioned, installable C SDK with build-system metadata | `p2` | `delivery` | `c-abi`, `ci`, `packaging` | [FR-CABI-011 (#169)](https://github.com/plx/ferric-rules/issues/169)<br>[FR-CABI-012 (#170)](https://github.com/plx/ferric-rules/issues/170) |

### Release and CI (9)

| Stable ID | GitHub issue | Priority | Type | Primary components | Native blockers |
|---|---|---|---|---|---|
| `FR-RELEASE-EPIC` | [#215](https://github.com/plx/ferric-rules/issues/215) Epic: make the Rust release, CI matrix, and dependency claims reproducible | `p0` | `epic` | `ci`, `packaging` | [FR-RELEASE-002 (#101)](https://github.com/plx/ferric-rules/issues/101)<br>[FR-RELEASE-007 (#102)](https://github.com/plx/ferric-rules/issues/102)<br>[FR-RELEASE-001 (#148)](https://github.com/plx/ferric-rules/issues/148)<br>[FR-RELEASE-003 (#149)](https://github.com/plx/ferric-rules/issues/149)<br>[FR-RELEASE-004 (#150)](https://github.com/plx/ferric-rules/issues/150)<br>[FR-RELEASE-005 (#151)](https://github.com/plx/ferric-rules/issues/151)<br>[FR-RELEASE-006 (#152)](https://github.com/plx/ferric-rules/issues/152)<br>[FR-RELEASE-008 (#153)](https://github.com/plx/ferric-rules/issues/153) |
| `FR-RELEASE-001` | [#148](https://github.com/plx/ferric-rules/issues/148) Fix and gate the tracing feature configuration | `p1` | `defect` | `runtime`, `ci` | — |
| `FR-RELEASE-002` | [#101](https://github.com/plx/ferric-rules/issues/101) Restore the declared Rust 1.75 MSRV or explicitly raise it | `p0` | `delivery` | `ci`, `packaging` | — |
| `FR-RELEASE-003` | [#149](https://github.com/plx/ferric-rules/issues/149) Add native OS, architecture, and libc CI coverage for declared Rust artifacts | `p1` | `delivery` | `ci`, `packaging` | — |
| `FR-RELEASE-004` | [#150](https://github.com/plx/ferric-rules/issues/150) Make all-features, hardening, dependency, tracing, and scaling checks required CI | `p1` | `delivery` | `core`, `runtime`, `ci` | [FR-PERF-003 (#143)](https://github.com/plx/ferric-rules/issues/143)<br>[FR-RELEASE-001 (#148)](https://github.com/plx/ferric-rules/issues/148)<br>[FR-ROBUST-003 (#165)](https://github.com/plx/ferric-rules/issues/165) |
| `FR-RELEASE-005` | [#151](https://github.com/plx/ferric-rules/issues/151) Replace or explicitly contain unmaintained bincode 1.3 snapshot support | `p1` | `hardening` | `runtime`, `packaging` | [FR-RETE-019 (#194)](https://github.com/plx/ferric-rules/issues/194) |
| `FR-RELEASE-006` | [#152](https://github.com/plx/ferric-rules/issues/152) Establish blocking multi-ecosystem dependency advisory policy | `p1` | `delivery` | `ci`, `packaging` | — |
| `FR-RELEASE-007` | [#102](https://github.com/plx/ferric-rules/issues/102) Make the public ferric crate packageable from crates.io metadata | `p0` | `defect` | `packaging` | — |
| `FR-RELEASE-008` | [#153](https://github.com/plx/ferric-rules/issues/153) Add clean-room Rust crate and CLI install smoke tests | `p1` | `delivery` | `ci`, `packaging` | [FR-RELEASE-007 (#102)](https://github.com/plx/ferric-rules/issues/102) |

### Performance evidence (6)

| Stable ID | GitHub issue | Priority | Type | Primary components | Native blockers |
|---|---|---|---|---|---|
| `FR-PERF-EPIC` | [#216](https://github.com/plx/ferric-rules/issues/216) Epic: make performance evidence prove correct and representative work | `p1` | `epic` | `core`, `runtime`, `ci` | [FR-PERF-001 (#100)](https://github.com/plx/ferric-rules/issues/100)<br>[FR-PERF-002 (#142)](https://github.com/plx/ferric-rules/issues/142)<br>[FR-PERF-003 (#143)](https://github.com/plx/ferric-rules/issues/143)<br>[FR-PERF-004 (#144)](https://github.com/plx/ferric-rules/issues/144)<br>[FR-PERF-005 (#145)](https://github.com/plx/ferric-rules/issues/145) |
| `FR-PERF-001` | [#100](https://github.com/plx/ferric-rules/issues/100) Add correctness oracles to every timed benchmark workload | `p0` | `defect` | `core`, `runtime` | [FR-RETE-001 (#103)](https://github.com/plx/ferric-rules/issues/103)<br>[FR-RETE-002 (#104)](https://github.com/plx/ferric-rules/issues/104)<br>[FR-RETE-004 (#106)](https://github.com/plx/ferric-rules/issues/106)<br>[FR-RETE-005 (#107)](https://github.com/plx/ferric-rules/issues/107)<br>[FR-RETE-006 (#108)](https://github.com/plx/ferric-rules/issues/108)<br>[FR-RETE-018 (#161)](https://github.com/plx/ferric-rules/issues/161) |
| `FR-PERF-002` | [#142](https://github.com/plx/ferric-rules/issues/142) Replace ineffective absolute benchmark thresholds with measured regression budgets | `p1` | `hardening` | `core`, `ci` | [FR-PERF-001 (#100)](https://github.com/plx/ferric-rules/issues/100) |
| `FR-PERF-003` | [#143](https://github.com/plx/ferric-rules/issues/143) Make asymptotic scaling checks blocking CI | `p1` | `delivery` | `core`, `runtime`, `ci` | [FR-PERF-001 (#100)](https://github.com/plx/ferric-rules/issues/100) |
| `FR-PERF-004` | [#144](https://github.com/plx/ferric-rules/issues/144) Add production-shaped benchmark workloads and capacity/resource budgets | `p1` | `audit` | `core`, `runtime` | [FR-PERF-001 (#100)](https://github.com/plx/ferric-rules/issues/100) |
| `FR-PERF-005` | [#145](https://github.com/plx/ferric-rules/issues/145) Gate cross-engine performance claims on semantically equivalent work | `p1` | `audit` | `runtime`, `compat-tool`, `ci` | [FR-COMPAT-004 (#91)](https://github.com/plx/ferric-rules/issues/91)<br>[FR-COMPAT-007 (#92)](https://github.com/plx/ferric-rules/issues/92)<br>[FR-PERF-001 (#100)](https://github.com/plx/ferric-rules/issues/100)<br>[FR-PERF-004 (#144)](https://github.com/plx/ferric-rules/issues/144) |

## Snapshot validation

The bootstrap was validated against live GitHub state on 2026-07-25:

- 141 open issues carry `program:production-readiness`.
- All 141 stable IDs parsed from titles are unique.
- All 141 issues belong to milestone #1 and carry recognized program taxonomy
  and workflow labels; required priority, type, area, component, and risk
  dimensions are present.
- The native blocked-by endpoints contain 205 edges, including #224 → #223, #223 → 12 component epics, FR-RETE-029..035 → #225, and #225 → #211.
- Every inventory row contains the stable ID, GitHub link/number, priority, type, primary component labels, and complete native blocker list.

Future edits should refresh this document only as an intentional historical snapshot revision. Day-to-day graph changes belong in GitHub.
