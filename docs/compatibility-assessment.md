# Compatibility assessment oracles

The `just compat-*` pipeline produces differential evidence between Ferric and
the pinned reference CLIPS container. Manifest schema v3 is intentionally
fail-closed: matching process output, including matching empty output, is not
compatibility evidence by itself.

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

## Static source classification

`compat-scan` makes one lexical pass over each raw source and classifies only
recognized form heads. Strings, comments, and symbol substrings cannot create
feature detections; real form heads remain case-insensitive, including nested
forms. Every successfully decoded entry carries `feature_scan` version 1 with a
`valid` or `invalid` status, ordered detections, and lexical issues. Each
detection records its feature, category, reason, and exact form-head and
enclosing-form spans; each issue records its kind, reason, and exact span. Spans
use half-open UTF-8 byte offsets plus 1-based line and column coordinates.

Malformed source is fail-closed as
`incompatible/malformed-source` with runability `unknown`, and explicit
compatibility-run selection refuses it. This preserves any trustworthy
detections found before the lexical error without silently treating the file as
runnable or compatible.

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

A valid semantic mismatch is `divergent`. A missing declaration is
`pending/oracle-missing`. Missing, stale, malformed, incomplete, spoofed, or
unsupported evidence is `pending/oracle-invalid:*`; the exact composed input
is retained under `.ferric-compat/failures/`, when one exists, and
`compat-run` exits nonzero.

Diagnostics use taxonomy version 1 and retain their engine-native message
alongside the canonical fields `phase`, `category`, and `continued`. The
semantic mappings are `parse/syntax-error`, `load/construct-error`, and
`reset|run/evaluation-error`. Multiple native diagnostics may collapse only
when all canonical fields agree. Unknown categories, versions, or mixed
diagnostic states fail closed. A known phase, category, or continuation
mismatch is `divergent`; matching terminal diagnostics without a complete
semantic oracle are `incompatible`, never `equivalent`.

Process termination is recorded independently as `exit`, `timeout`, `signal`,
or `spawn-error`, with exit status and signal number where available. It does
not overwrite authenticated engine diagnostics. Timeout and signal evidence
also retains the last authenticated active phase when one was observed. Raw
stdout and stderr bytes remain losslessly encoded under each result's
`raw_output` field, while readable channel text and observation envelopes
remain in the manifest for audit.

Manifest-v2 output-based results are deliberately reset during schema-v3
migration. Undeclared fixtures remain pending instead of preserving legacy
`equivalent` or `divergent` claims.

## Maintainer workflow

Run the assessment from the repository root:

```console
just assess-compatibility
```

That recipe performs the complete ordered lane: build the pinned reference
image and release Ferric candidate, scan, generate deterministic library
harnesses into `.ferric-compat/`, verify their source/output/contracts without
rewriting them, run every structured-oracle fixture through both engines,
enforce the reviewed compatibility policy, and finally report. The lower-level
equivalent is:

```console
cargo build --release -p ferric-rules-cli
docker build -t ferric-rules/clips-reference:latest docker/clips-reference/
just compat-scan
just harness-gen --output-dir "$PWD/.ferric-compat/harnesses"
just harness-gen --output-dir "$PWD/.ferric-compat/harnesses" --check
just compat-run --all --require-selected --candidate-sha "$(git rev-parse HEAD)"
just compat-ci-gate --expected-commit-sha "$(git rev-parse HEAD)"
just compat-report
```

`--require-selected` makes a zero-fixture or declaration-free selection an
error. The generated-harness control
`ferric-oracle/empty-output-state.clp` deliberately has no committed harness:
scan validates the deterministic plan bytes, generation materializes them,
verification re-resolves their digests, and only then may the runner compose
and execute the control. Its empty channels are non-vacuous because both
engines prove the declared final state/effect while generated verifier firings
remain instrumentation.

`compat-ci-gate` is the outer release policy. It preserves the exact 22-case
semantic matrix and its issue-linked known divergences, requires every oracle
registry fixture to belong to the reviewed policy, recomputes the generated
harness control as equivalent from raw engine observations, verifies complete
candidate/reference provenance and manifest totals, and rejects every missing,
partial, vacuous, or unexplained result.

The release-blocking matrix of 22 scenarios covering 20 audit IDs has an
additional exact policy gate:

```console
just compat-semantic-lane
```

`compat-semantic-lane` rescans, runs every `ferric-semantic` scenario against
both engines, and then enforces
`tests/examples/compat-semantic-policy.json`. Every required scenario ID must be
present exactly once; the 22 scenarios cover 20 audit IDs. Equivalence is
accepted only with valid, mismatch-free evidence. A temporary known divergence
must match its issue-linked reason,
exact mismatch fields, and normalized Ferric semantic fingerprint; changed
behavior fails, and newly equivalent behavior fails until the stale deviation
is removed. The policy also checks the measured reference binary/library
digests for the active platform.

`compat-report` exposes declaration, lifecycle, effect, oracle version,
normalization, diagnostic phase/category/continuation, and process termination
evidence. Compare a baseline and candidate with:

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

Standalone, pull-request comparison, and direct CI compatibility jobs all use
the same scan → harness generation → harness verification → dual-engine run →
policy-gate order. Core steps are blocking. Report finalization and artifact
upload use GitHub Actions `always()` handling, so a missing reference image,
harness failure, state/output divergence, or policy violation still produces a
manifest or explicit fallback status plus candidate/reference provenance and
retained failure inputs; those postmortem steps do not change the failed job
conclusion. Pull requests expose the stable `Compatibility Gate` aggregation
context for repository rules.

For schema-v3 heads, the diff gate rejects every unverified equivalence claim,
including a legacy claim copied forward unchanged, as well as evidence-coverage
loss. The initial empty-output control is
`tests/examples/ferric-oracle/empty-output-state.clp`; it is equivalent only
because both engines prove the declared final fact, effect, and firing count.
