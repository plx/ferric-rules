# Compatibility assessment oracles

The `just compat-*` pipeline reports two separate evidence sets: the granular
CLIPS compatibility corpus and the legacy example oracles. Reports lead with
the corpus's declared counts and the actual Ferric/reference verification
status. Static example inventory is not execution evidence. Matching process
output, including matching empty output, is not sufficient evidence for the
legacy semantic oracles.

The scanner writes inventory schema v4; the runner and blocking gate retain
strict schema-v3 support. Reports also read older manifests without promoting
their output-based claims to verified compatibility.

## Granular corpus evidence

`tests/clips_compat/corpus/manifest.json` declares conformance cases, active
known-gap characterizations, and expected load/run errors. Expected errors are
a subset of conformance, while an accepted known gap remains a gap. Neither
case counts nor the manifest's historical reference stamp prove a current run.

After building the pinned reference image, capture and verify the full corpus:

```console
just compat-corpus-evidence "$PWD/.ferric-compat/corpus-evidence"
just compat-report --corpus-summary "$PWD/.ferric-compat/corpus-evidence/summary.json"
```

The evidence recipe records the actual checkout revision, dirty state, fresh
run ID, manifest digest, and each case's source/input/execution-contract and
golden digests. It runs the Ferric corpus tests and all registered cases against
the pinned CLIPS container. Reference calls use a positive 120-second timeout,
including image/version probes, with four case workers. Reference provenance
includes the immutable image ID and observed CLIPS version.

`summary.json` retains declared counts separately from selected, reported, and
accepted results, command exit codes, failures, and infrastructure errors.
Complete matching Ferric and reference results are required for verification;
a filtered pass, old raw observations without a verdict sidecar, or missing
reference evidence stays partial or unavailable. A dirty checkout is explicitly
reported as working-tree evidence. Replay and host-test counts are separate
from the corpus case count.

Compare captured revisions without rereading an unrelated checkout:

```console
just compat-diff BASE_MANIFEST HEAD_MANIFEST \
  --base-corpus-summary BASE_EVIDENCE/summary.json \
  --head-corpus-summary HEAD_EVIDENCE/summary.json
```

A gap-to-conformance change is verified only with matching scenario/golden
identity and successful evidence on both sides. Reports distinguish declaration
changes, added coverage, removed cases, changed scenarios, and changed goldens.
Removing a gap case or changing its oracle is not reported as a fixed gap.
Omitting a summary leaves corpus evidence unavailable; no counts or successful
run are inferred from the live checkout.

## Fixture declarations

Executable evidence starts in
`tests/examples/compat-oracles.json`. The registry is versioned, rejects
duplicate JSON fields and duplicate fixture identities, and maps normalized
paths relative to `tests/examples/` to version-1 or version-2 declarations.

Each declaration binds:

- a protocol-safe fixture ID and a human-readable feature;
- the exact source and composed-input SHA-256 digests;
- the `load`, `reset`, `run` setup sequence;
- expected phase, firing count or ordered firing names, semantic effects,
  canonical final facts, stdout and stderr, diagnostic state, run termination,
  and any selected focus-stack or global state; and
- the fixture-specific normalizers that are allowed.

Oracle v1 supports an unlimited run and `agenda-empty`, `halt-requested`, or
`action-error` completion, plus exactly the `stdout` and `stderr` semantic
channels. The projection normalizes the pinned CLIPS adapter's native `error`
spelling to `action-error`. Firing count must agree with the number of firing
names when both are declared. Unsupported declarations are invalid
configuration, not engine divergence.

The only normalizers are:

- `fact-ids` — ignore engine-assigned fact identity;
- `fact-order` — compare final facts and their fact-derived effects without
  enumeration order; and
- `float-format` — compare finite decimal float spellings by numeric value.

No normalizer is applied globally. Duplicate facts and values remain
significant.

When a fixture or generated harness changes, update both digests in its
declaration. `just compat-scan` rejects a stale declaration before an engine is
started.

Version 2 adds a strict, digest-bound scenario for regressions that cannot be
represented by one `load`, `reset`, `run` sequence. Its ordered `sources` array
names examples-relative regular files; its setup steps may load those sources,
reset, select `depth`, `breadth`, `lex`, or `mea`, and finish with exactly one
unlimited run. Canonical plan bytes are UTF-8 with LF endings and a final
newline. Both adapters independently enforce path containment, SHA-256
identity, at most 64 sources and 256 steps, a 1 MiB plan, 16 MiB per source,
and 64 MiB across the source bundle. A step may continue after a semantic
load/reset error, but malformed plans, harness errors, and the final run always
stop. The manifest's top-level `oracle_protocol_version: 1` continues to name
the shared observation/evaluation protocol; each declaration and evidence
record carries its own version.

## Static inventory and deduplication

The current legacy scan covers 1,264 physical `.clp` and `.bat` paths in 642
canonical rows, retaining 622 aliases and 58 independent oracle identities.
These are inventory/declaration counts, not compatibility results. Only
unassessed files with identical bytes and the same suffix are combined; each
canonical row records its content digest and sorted aliases with original
paths and upstream source names. Physical files remain in their imported
bundles so relative loads, companion resources, and attribution remain intact.
Oracle-backed paths are never combined or allowed to inherit another path's
result, even when their bytes match.

`compat-scan` classifies recognized form heads. Strings, comments, and symbol
substrings cannot create feature detections; real form heads remain
case-insensitive, including nested forms. Every successfully decoded entry
carries `feature_scan` version 1 with a `valid` or `invalid` status, ordered
detections, and lexical issues. Detections include feature/category/reason and
form-head/enclosing-form spans. Issues include their kind, reason, and span.
Spans use half-open UTF-8 byte offsets and 1-based line/column coordinates.

All unexecuted entries are `unassessed`. Malformed source retains
`reason: malformed-source` and `runability: unknown`; static unsupported-feature,
interactive, loading, read-error, and library findings remain reasons for
assessment planning. They are never counted as incompatible execution results.
A registered oracle's contract takes precedence over scanner dispositions,
including unknown runability. Explicit execution of a file without an oracle
is refused; an invalid declared oracle fails rather than becoming unassessed.

Harness planning, generation, and checking select only validated version-1
oracle libraries. Version-2 scenarios own their setup plans. Unassessed
libraries receive no harness metadata or generated files. Before writing,
generation revalidates the current registry, bound source/composed digests,
paths, and deterministic plans; checking also verifies materialized bytes.
Obsolete generated no-oracle harnesses have been removed.

## Observation boundary

Every run receives a fresh 128-bit nonce. Successful observations must produce
exactly one nonce- and digest-bound `START`/`COMPLETE` lifecycle and a complete
post-run observation. A semantic load, reset, or run failure may instead end at
its authenticated terminal phase record; incomplete or out-of-order protocol
evidence remains invalid.

Ferric exposes this through the hidden `ferric compat-observe` command, leaving
the public `ferric run --json` contract unchanged. Reference CLIPS is loaded by
a dedicated native embedding that owns the single reset/run boundary, counts
the agenda across every module, and enables typed post-run probes only after
`EnvRun` returns. It emits length-prefixed, nonce-bound records on the process
stderr boundary, separate from fixture router output. Each record also carries
a keyed authentication tag whose per-run key is withheld from the live fixture.
The nonce, identity, and authentication binding are consumed before the CLIPS
environment is created and never enter the fixture-visible environment or
command stream. After the child exits, the runner retains the invocation key
with the raw transcript so the blocking gate can independently replay and
verify every authenticated record. Parsing uses byte offsets, so UTF-8 values
are framed by their encoded byte length.

The native adapter installs a dedicated CLIPS error router and emits explicit
authenticated `load`, `reset`, and `run` phase boundaries. Router bytes are
teed immediately to raw stderr, while CLIPS load/evaluation/halt state decides
whether they become a diagnostic; fixture text alone cannot create one.
Diagnostic payloads are length-framed so native messages containing delimiters
or newlines remain exact.

The native observer is built into `ferric-rules/clips-reference:latest` from
`docker/clips-reference/`. Rebuild that local image after changing the
observer:

```console
docker build -t ferric-rules/clips-reference:latest docker/clips-reference
```

Run `just compat-observer-test` after the image is available when changing the
observer. The live regression checks imported facts are emitted only once and
same-named private templates retain their actual modules. It also checks that
capture adds no fixture output or diagnostics. CI runs this test before the
pinned compatibility assessment.

Do not use `just clips-build` for a local-only rebuild: that recipe publishes
unless invoked with its explicitly local options.

The image pins Debian by manifest digest and CLIPS by package version. Before
execution, the runner obtains one strict provenance record containing the
engine/package versions, platform, measured CLIPS executable and library
SHA-256 digests, base-image digest, and local image ID. That record is stored as
top-level manifest `reference` evidence.

The runner also hashes the exact release-mode Ferric executable before and
after assessment and records the explicitly supplied 40-character revision
SHA. Hosted gates establish that mapping by building from a clean checkout of
the recorded revision; the executable digest remains the authoritative byte
identity for local assessments of a dirty tree. The resulting top-level
`candidate` record contains both values. A stale manifest identity, unreadable
or symlinked executable, or executable that changes during the run fails before
the result can be accepted.

Generated verifier records and its single firing are instrumentation, not
fixture effects. Generation v2 asserts no facts, so every observed fact remains
fixture-owned even when its relation resembles the reserved verifier name. If
either adapter cannot separate instrumentation from feature behavior, the
observation is invalid rather than equivalent.

## Classification and exit behavior

An entry is `equivalent` only when:

- its declaration is current and valid;
- both engines independently reach and complete the observation;
- each engine demonstrates the declared feature effect and all expectations;
  and
- the normalized observations agree with each other.

A valid executed semantic mismatch is `divergent`. Missing declarations and
valid but unexecuted declarations are `unassessed`. Invalid declarations and
missing, stale, malformed, incomplete, spoofed, or unsupported evidence for an
attempted oracle are `evidence-failure`; the exact composed input is retained
under `.ferric-compat/failures/`, when one exists, and `compat-run` exits nonzero.
Reports share this evidence-based view across Markdown, JSON, CSV, and TSV,
while retaining raw scanner/legacy labels. Only completed valid oracle evidence
from both engines contributes to equivalent/divergent counts. The blocking
gate independently revalidates physical inputs and raw observations.

Diagnostics use taxonomy version 1 and retain their engine-native message
alongside the canonical fields `phase`, `category`, and `continued`. The
semantic mappings are `parse/syntax-error`, `load/construct-error`, and
`reset|run/evaluation-error`. Multiple native diagnostics may collapse only
when all canonical fields agree. Unknown categories, versions, or mixed
diagnostic states fail closed. A known phase, category, or continuation
mismatch is `divergent`; matching terminal diagnostics without a complete
semantic oracle cannot establish equivalence.

Process termination is recorded independently as `exit`, `timeout`, `signal`,
or `spawn-error`, with exit status and signal number where available. It does
not overwrite authenticated engine diagnostics. Timeout and signal evidence
also retains the last authenticated active phase when one was observed. Raw
stdout and stderr bytes remain losslessly encoded under each result's
`raw_output` field, while readable channel text and observation envelopes
remain in the manifest for audit.

Legacy output-based labels are not promoted into executed oracle results.
Undeclared fixtures remain unassessed. Static `incompatible` or `pending` labels
becoming unassessed during migration are neutral inventory changes, not engine
improvements.

## Maintainer workflow

Run the assessment from the repository root:

```console
just assess-compatibility
```

That recipe builds the pinned reference image and release Ferric candidate,
verifies the complete granular corpus, checks the native observer, scans the
legacy inventory, generates and verifies selected oracle library harnesses
inside `.ferric-compat/`, runs every structured oracle through both engines,
enforces the reviewed policy, and reports both evidence sets. The lower-level
equivalent is:

```console
cargo build --release -p ferric-rules-cli
docker build -t ferric-rules/clips-reference:latest docker/clips-reference/
just compat-corpus-evidence "$PWD/.ferric-compat/corpus-evidence"
just compat-observer-test
just compat-scan
just harness-gen --output-dir "$PWD/.ferric-compat/harnesses"
just harness-gen --output-dir "$PWD/.ferric-compat/harnesses" --check
just compat-run --all --require-selected --candidate-sha "$(git rev-parse HEAD)"
just compat-ci-gate --expected-commit-sha "$(git rev-parse HEAD)"
just compat-report --corpus-summary "$PWD/.ferric-compat/corpus-evidence/summary.json"
```

`--require-selected` makes a zero-fixture or declaration-free selection an
error. The generated-harness control
`ferric-oracle/empty-output-state.clp` deliberately has no committed harness:
scan validates the deterministic plan bytes, generation materializes them,
verification re-resolves their digests, and only then may the runner compose
and execute the control. Its empty channels are non-vacuous because both
engines prove the declared final state/effect while generated verifier firings
remain instrumentation.

`compat-ci-gate` is the outer compatibility policy. It preserves the reviewed
57-scenario semantic matrix and its issue-linked known divergences, requires every oracle
registry fixture to belong to the reviewed policy, recomputes the generated
harness control as equivalent from raw engine observations, verifies complete
candidate/reference provenance and manifest totals, and rejects every missing,
partial, vacuous, or unexplained result.

The semantic matrix also has an exact policy gate:

```console
just compat-semantic-lane
```

`compat-semantic-lane` rescans, runs every `ferric-semantic` scenario against
both engines, and then enforces
`tests/examples/compat-semantic-policy.json`. Every required scenario ID must be
present exactly once. The current matrix contains 57 scenarios; the separate
empty-output control brings the legacy lane to 58 oracle identities.
Equivalence is accepted only with valid, mismatch-free evidence. A temporary known divergence
must match its issue-linked reason,
exact mismatch fields, and normalized Ferric semantic fingerprint; changed
behavior fails, and newly equivalent behavior fails until the stale deviation
is removed. The policy also checks the measured reference binary/library
digests for the active platform.

`compat-report` exposes declaration, lifecycle, effect, oracle version,
normalization, diagnostic phase/category/continuation, and process termination
evidence. Legacy improvements also require a stable source/composed identity and
oracle contract; changed expectations are identified as oracle changes. Compare
legacy manifests alone with:

```console
just compat-diff BASE_MANIFEST HEAD_MANIFEST
```

The PR compatibility workflow also retains a scanner-only comparison. It
captures each revision's manifest immediately after `compat-scan`, before
`compat-run` can replace the scanner's classification and reason with runtime
results. The Markdown and TSV summaries compare `features`,
`unsupported_features`, classification, reason, runability, and structured scan
status and issues. The JSON artifact additionally retains the complete
`feature_scan` detections, reasons, issues, and exact spans for every reported
file. A base manifest without `feature_scan` is identified as legacy evidence
instead of reporting every file as changed. Scanner changes are review evidence
and do not fail CI; failure to generate the retained artifacts does fail the
workflow.

The standalone (push to `main`) and pull-request comparison jobs run the full
pinned-reference corpus check with 120-second per-call limits, followed by the
legacy scan → selected harness generation → harness verification → dual-engine
run → policy-gate lane. These checks are blocking. The comparison workflow uses
head Python assessment tooling on the base checkout, but never overlays head
Rust sources into the base binary. The base corpus summary explicitly records
the resulting dirty checkout; an older base without a verdict sidecar remains
unverified rather than borrowing head evidence. Report finalization and artifact
upload use GitHub Actions `always()` handling, so a missing reference image,
harness failure, state/output divergence, or policy violation still produces a
manifest or explicit fallback status plus candidate/reference provenance and
retained failure inputs; those postmortem steps do not change the failed job
conclusion. Pull requests expose the stable `PR Compatibility Gate` aggregation
context for repository rules.

The blocking gates reject unverified claimed oracle outcomes and required
evidence-coverage loss for both supported manifest schemas. Scanner-only label
changes cannot supply a semantic improvement. The initial empty-output control is
`tests/examples/ferric-oracle/empty-output-state.clp`; it is equivalent only
because both engines prove the declared final fact, effect, and firing count.
