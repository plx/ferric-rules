# Project history

A short factual record of how ferric-rules reached its current state. Current
behavior and contracts are documented elsewhere (see
[project overview](project-overview.md)); this page only explains where things
came from and where the older records went.

| Period (2026) | Phase | PRs |
| --- | --- | --- |
| Feb – Apr | Agent-driven build-out of the engine, CLI, C ABI, Python and Go bindings | #1 – #73 |
| Apr – Jul | Node/TypeScript binding, user's guide, docs site, binding test suites | #74 – #84 |
| Jul 25 – Aug 10 | Due-diligence review and the production-readiness program | #79, ~#226 – #296 |
| Sep 6 – 7 | Rehabilitation: program retired, finite repair scope | #298 – #315 |
| Sep 7 – 8 | Performance work on the rehabilitated baseline | #316 – #364 |
| Sep 7 – 12 | Granular CLIPS compatibility corpus | #348 |

## Build-out (February – April)

The repository started on 2026-02-08. Coding agents implemented the engine in
numbered phases from in-repo plan documents: the Rete core, the three-stage
parser, the runtime and standard library, the C ABI, and the CLI. Late February
and March added join indexing and other performance work (#26, #31, #44), the
PyO3 Python binding (#35), the C ABI expansion (#36), Go bindings (#41), and
opt-in `serde` snapshots (#39, #42). April added the scaling regression tests
(#73) and audits of the Python and Go bindings (#71, #72). A Dockerized CLIPS
reference harness (#7) and a corpus of third-party CLIPS programs (#8)
supported compatibility testing. The plan and spec documents were removed in
#76.

## Bindings and site (April – July)

The Node/TypeScript binding began with #74, followed by its specification
documents and later property-style coverage (#82). The user's guide (#76),
license notices (#77), and the Astro/Starlight documentation site (#78, #80,
#84) were added, along with coverage suites for the Python and Go bindings
(#81, #83).

## Due diligence and the production-readiness program (July – August)

On 2026-07-25 an independent review of commit `dd366eb6` (which had just added
the Rust-owned pinned engine, #79) concluded that the project was a credible
prototype but not production ready: it found memory-safety defects at the C and
Go boundary, incorrect behavior in core rule constructs, binding lifecycle and
concurrency defects, little affirmative evidence of CLIPS equivalence, and gaps
in packaging and CI. That review seeded a production-readiness program: 141
issues in a milestone with about 205 native blocked-by edges, a label
taxonomy, an automated next-issue selector, and a final re-audit playbook.
Roughly 60 PRs (about #226 – #296) landed through 2026-08-10, including C ABI
hardening, Node pool and worker fixes, Python GIL release, compatibility
evidence tooling, native release artifacts, and a dependency advisory policy.
The Rust crates were renamed to the `ferric-rules-*` namespace in #250.

## Rehabilitation (September 6 – 7)

On 2026-09-06 the owner retired the program as an execution contract. Retiring
it did not mean the original audit passed. The 81 open program issues were
triaged once: overlapping defects were consolidated into a finite set of 13
work items, delivered as PRs #298 – #315, and 72 issues were closed as not
planned. Decisions:

- **Supported scope.** The Rust engine and CLI, TypeScript, Python, and a local
  Swift package (#312) over a healthy C ABI. Go (#310) and standalone C
  distribution receive maintenance only.
- **Deliberate deferrals.** Broad Go/C distribution or parity, public
  registries and releases, new platform certification, production SLO/soak
  programs, a task scheduler, and speculative Rete redesigns. Unsupported
  constructs (for example `logical` CEs) fail explicitly; LEX/MEA ordering
  remains experimental.
- **Threading.** Rust `Engine` became structurally `Send + Sync` (#304), with
  immutable shared values behind `Arc` and exclusive mutation. The C handle
  keeps a serialized-call contract; bindings build on that.
- **Persistence.** Snapshots use a bounded, versioned CBOR envelope with a
  checksum and restore-time validation (#307). Unversioned legacy snapshots are
  rejected. See [snapshots](snapshots.md).
- **Dependencies.** Standard scanners (`cargo deny`, `npm audit`,
  `pip-audit`) replaced about 17,300 lines of bespoke policy code and tests
  (#298). See [dependency checks](dependency-security-policy.md).
- **Semantics.** Template RHS assertions, rule replacement and reload,
  reset/deffacts ordering, activation chronology, module exports/focus, and
  host value/fact-handle provenance were repaired (#301, #306, #311), with
  regressions checked against real CLIPS 6.30.

The accepted costs were recorded at the time: validated CBOR snapshot writes
became about five times slower than the previous unvalidated snapshots, and
some string and multifield join workloads regressed by roughly 10–25% in
exchange for checked host ownership.

## Performance work (September 7 – 8)

A follow-up audit measured every Criterion suite against the post-rehabilitation
baseline `201665e7` and kept only changes that showed a net benefit, each in its
own PR:

- shared immutable template definitions for actions and owned reads (#316);
- equality-index reuse for `exists` support propagation (#317);
- indexed template name resolution (#318);
- retraction cleanup limited to the affected memories (#319);
- reused temporary action binding frames (#347);
- lighter sparse host-handle bookkeeping (#349);
- a lazy rule-ordering index after focused agenda misses (#350);
- small ordered memberships stored inline (#354).

An experiment that kept pending cascade tokens inline was rejected for lack of
a broad win (#355); its benchmark controls were kept. Across the final stack,
the equally weighted geometric mean of median times fell by about 19% for the
192 facade workloads, 6–7% for runtime, 14% for core storage, and 1–2% for the
C ABI. Large individual gains included indexed `exists` (about −86%), long
action loops (about −49%), and dormant Depth-strategy focus (about −79%).
Engine construction, registry listing, sparse host exports, and serialization
regressed by 5–38% and were left as follow-up targets. These PRs also added
three scaling checks (`exists` support assertion, independent negative cleanup,
and dormant focus selection), bringing `just scaling-check` to eight.

## Granular compatibility corpus (September 7 – 12)

PR #348 added `tests/clips_compat/corpus/`: 219 small CLIPS programs with exact
CLIPS 6.30 output goldens, run by `crates/ferric-rules/tests/compat_corpus.rs`.
At merge, 158 cases conformed and 61 were active characterizations of known
differences. The gaps it found were filed as issues #320 – #346 and are listed
in `tests/clips_compat/corpus/GAPS.md`; the repairs followed as separate
compatibility work.

## Where the older records went

The due-diligence report, the remediation-program snapshot, the re-audit
playbook, the work-selection contract, the rehabilitation execution record, the
September performance audit with its JSON measurement records, the
pinned-engine plan, the Phase 6 baseline and performance-analysis notes, the
TypeScript binding implementation plan and spec post-mortem, and the Maquette
design mockups were removed in the cleanup PR that added this page. See git
history before this commit; the last commit that contains them is `aa3586e1`
(paths: `docs/audits/`, `plan/pinned-engine.md`, `docs/phase6-baseline.md`,
`docs/performance-analysis.md`, `docs/typescript-binding-implementation-plan.md`,
`docs/typescript-binding-spec-postmortem.md`, `.maquette/`, `documentation/`).

The rehabilitation's measurement branches were archived as tags named
`archive/rehab-*` (for example `archive/rehab-threading-measurement` and
`archive/rehab-final-measurement-base`).
