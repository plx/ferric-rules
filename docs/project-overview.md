# ferric-rules — Project Overview

A CLIPS-compatible forward-chaining rules engine in Rust, with a CLI, a C ABI,
and TypeScript, Python, Swift, and Go bindings. This page is a map of the
repository: what exists and where. Usage and contracts live in the documents
linked from [§6](#6-documentation-docs).

## 1. Rust workspace (`crates/`)

| Crate | Purpose |
| --- | --- |
| `ferric-rules` | Facade crate. Re-exports `core`, `parser`, and `runtime`; hosts the Criterion benches, the compatibility-corpus test, the semantic regressions, and the scaling tests. |
| `ferric-rules-core` | Rete network internals: values/symbols/encoding, facts, alpha/beta networks, tokens, negative/NCC/exists nodes, agenda and conflict strategies, the rule compiler, and pattern validation. Not for direct use. |
| `ferric-rules-parser` | Lexer → S-expressions → Stage 2 typed constructs (`defrule`, `deftemplate`, `deffacts`, `deffunction`, `defglobal`, `defmodule`, `defgeneric`, `defmethod`). |
| `ferric-rules-runtime` | `Engine`, loader, run loop, expression evaluator, builtin functions, modules/focus, output router, host values (`host.rs`), and snapshots (`serialization.rs`, feature `serde`). |
| `ferric-rules-cli` | The `ferric` binary: `run`, `check`, `repl`, snapshot commands, `version`. Exit codes: 0 success, 1 runtime error, 2 usage error. |
| `ferric-rules-ffi` | C ABI over the runtime (`libferric_rules_ffi`, checked-in `ferric.h` verified by `just check-ffi-header`). Has its own `ffi-dev`/`ffi-release` profiles; C regression harnesses live in `crates/ferric-rules-ffi/tests/c/`. |
| `ferric-rules-ffi-macros` | Proc macro that wraps each exported C function in panic containment. |
| `ferric-rules-napi` | napi-rs native addon used by the TypeScript package. |
| `ferric-rules-python` | PyO3 extension module `ferric`, built with `maturin`; tests in `tests/*.py`. |
| `ferric-rules-bench-gen` | Generates benchmark inputs. |

`Engine` is `Send + Sync`: ownership can move between threads, shared
references can be read concurrently, and mutation requires exclusive access.
The C handle has its own serialized-call contract (section 16.13 of
[`compatibility.md`](compatibility.md)).

Other workspace members: `examples/users-guide/*` (one crate per guide
chapter), `tools/users-guide-sync` (checks that code in
[`users-guide.md`](users-guide.md) matches those crates), and
`tools/bindings-conformance-adapter` (Rust adapter for the cross-binding
corpus).

Feature flags: `serde` enables snapshots (propagated parser → core → runtime →
ffi → facade); `tracing` enables tracing spans (checked locally by `just
check-tracing`; CI's `Rust` job runs the tracing clippy and the
`tracing_smoke` test); `testing` (Python only) exposes instrumentation for
teardown tests.

## 2. Bindings

| Binding | Location | Notes |
| --- | --- | --- |
| TypeScript / Node | `crates/ferric-rules-napi` + `packages/ferric` (`@ferric-rules/node`) | Synchronous `Engine`, worker-backed `EngineHandle`, and `EnginePool`. See [`packages/ferric/README.md`](../packages/ferric/README.md) and the [normative contract](typescript-binding-normative-contract.md). |
| Python | `crates/ferric-rules-python` | Local source builds and host abi3 wheels; see [the binding README](../crates/ferric-rules-python/README.md#building-from-source). |
| Swift | `bindings/swift` | Local Swift 6 package over the C ABI (macOS 15 / iOS 18); built with `scripts/build-swift.sh`. |
| Go | `bindings/go` | cgo over the C ABI; kept building and tested, without broader distribution. |
| C | `crates/ferric-rules-ffi` | Static/dynamic library plus `ferric.h`. |

`tests/bindings-conformance/` holds a language-neutral corpus that `just
bindings-conformance` runs through the Rust, C, Go, Node, and Python adapters.

CI checks local packages through consumers outside the checkout:
[`scripts/python-consumer-smoke.sh`](../scripts/python-consumer-smoke.sh)
builds a host wheel and installs it into a fresh virtual environment;
[`scripts/test-node-package-artifact.mjs`](../scripts/test-node-package-artifact.mjs)
packs the Node package and host addon, then installs and exercises them offline.
The Swift binding keeps its native build and external consumer checks.

Registry publication is not currently planned. The full multi-target release
tooling can be restored from git at `3c3c8c7d` if publication work resumes.
Before any PyPI release, the Python distribution must be renamed, for example
to `ferric-rules`; its import name can remain `ferric`.

## 3. Tests and compatibility corpora

- Crate tests: unit tests in each crate's `src/`, integration tests in each
  crate's `tests/`. Runtime fixtures are in
  `crates/ferric-rules-runtime/tests/fixtures/`; facade fixtures in
  `crates/ferric-rules/tests/fixtures/`.
- `tests/fixtures/cli` and `tests/fixtures/ffi` — inputs for the CLI and FFI
  test suites.
- `tests/clips_compat/corpus/` — the granular compatibility corpus: small
  CLIPS programs with exact CLIPS 6.30 output goldens, run by
  `crates/ferric-rules/tests/compat_corpus/` (`just compat-corpus`).
  Known differences are active characterizations; `GAPS.md` links the issues.
  `just compat-corpus-reference` rechecks the goldens against the pinned CLIPS
  Docker image. `just compat-corpus-evidence OUTPUT` captures revision-bound
  Ferric/reference results; reports lead with declared counts and verification
  status rather than inferring success from corpus size.
- `tests/examples/` — third-party CLIPS projects (provenance in `SOURCES.md`)
  and the semantic differential lane: `ferric-semantic/`, `ferric-oracle/`,
  and the reviewed policies `compat-semantic-policy.json` and
  `compat-ci-policy.json` (`just compat-semantic-lane`,
  `just assess-compatibility`). The schema-v4 inventory retains 1,264 physical
  source paths as 642 canonical rows with 622 aliases and 58 separate oracle
  identities. Static findings and no-oracle files remain unassessed; imported
  bundle paths are preserved. Selected version-1 oracle library harnesses are
  generated and checked inside `.ferric-compat/`; version-2 scenarios use their
  own plans. Schema-v3 execution manifests remain supported.
- `tests/generated/` — tool-generated segments/expectations for `.bat` inputs;
  their presence does not establish executed compatibility.
- `crates/ferric-rules/tests/scaling_tests.rs` — fourteen `#[ignore]`
  complexity-class checks run by `just scaling-check`.

## 4. Benchmarks

Criterion suites live in `crates/ferric-rules/benches/` (engine, join,
negation, exists, forall, Waltz, Manners, churn, cascade, alpha fanout,
strategies, modules, queries, compile, evaluator, serialization) plus smaller
suites in `ferric-rules-runtime`, `ferric-rules-core`, and `ferric-rules-ffi`.
`benches/README.md` and `benches/PROTOCOL.md` describe the workloads and
measurement protocol; [`benchmark-policy.md`](benchmark-policy.md) covers
regression thresholds. Use `just bench-*` targets; numbers quoted in PRs must
come from release `cargo bench` runs. CI runs the benches weekly
(`benchmarks.yml`: bench smoke, absolute thresholds, CLIPS comparisons, and the
standalone perf report) and `just scaling-check` on pull requests and pushes
that touch Rust crates or Cargo manifests (`scaling.yml`).

## 5. Tooling

- `justfile` — the command surface. `just preflight-pr` must pass before a PR
  is opened or updated; `just check` is the non-fixing equivalent.
- `scripts/` — shell/Python helpers behind the `just` recipes (FFI header and
  sanitizer harnesses, host package consumer checks, dependency checks, CLIPS
  reference driver, Swift build, issue-triage helpers).
- `tools/ferric-tools/` — `uv`-managed Python package: `compat/`
  (scan/run/report/diff and CI gates against the CLIPS reference container),
  `bat/` (CLIPS `.bat` batch processing), `perf/` (Criterion result
  collection and diffs), and the bindings-conformance runner.
- `docker/clips-reference/` — CLIPS 6.30 reference image with observer and
  launcher programs; `docker/bench-runner/` — container for bench runs.
- `.github/workflows/` — CI:
  - every pull request to `main`: `ci.yml` (Rust format/clippy/tests plus the
    tracing checks and users-guide examples, Rust MSRV 1.75 for the workspace
    excluding the Python binding (MSRV 1.83), Python/Node/Go bindings, FFI
    sanitizer harnesses, cross-binding conformance, Python tools; skipped for site-only changes),
    `pr-assessment.yml` (base-vs-head CLIPS compatibility comparison through
    `compat-compare.yml`; its `PR Compatibility Gate` check is required), and
    `dependency-policy.yml` (advisory, license, and notices checks; also
    weekly). `ci.yml` and `dependency-policy.yml` also run on pushes to
    `main`, where `compat-standalone.yml` runs the compatibility gate;
  - path-filtered: `scaling.yml` (Rust crates or manifests), `swift.yml`
    (the Swift binding and the crates beneath it), and `site-check.yml` /
    `site-publish.yml` (`site/`);
  - weekly or manual: `benchmarks.yml`; `perf-compare.yml` is manual only.
- `site/` — the Astro/Starlight documentation site.
- `examples/embedding/` — shared example program used by consumer smokes.

## 6. Documentation (`docs/`)

Users:

- [`users-guide.md`](users-guide.md) — embedding walkthrough, backed by
  `examples/users-guide/`.
- [`compatibility.md`](compatibility.md) — supported CLIPS subset and known
  differences.
- [`migration.md`](migration.md) — moving from CLIPS, plus pre-1.0 breaking
  changes.
- [`host-api.md`](host-api.md) — host values and fact handles.
- [`snapshots.md`](snapshots.md) — versioned snapshot format (CBOR, plus JSON
  for inspection).

TypeScript binding:
[`typescript-binding-normative-contract.md`](typescript-binding-normative-contract.md),
[`typescript-binding-architecture.md`](typescript-binding-architecture.md),
[`typescript-binding-conformance-matrix.md`](typescript-binding-conformance-matrix.md),
[`typescript-binding-test-spec.md`](typescript-binding-test-spec.md), and the
[public API reference](typescript-binding-api.md).

Maintainers:

- [`compatibility-assessment.md`](compatibility-assessment.md) — differential
  oracle contract for the compatibility tooling.
- [`benchmark-policy.md`](benchmark-policy.md) — performance regression policy.
- [`dependency-security-policy.md`](dependency-security-policy.md) —
  dependency scanning.
- [`history.md`](history.md) — short project history.

Repository root: `README.md` (introduction), `AGENTS.md` (contributor and
agent guidelines; `CLAUDE.md` includes it), `THIRD_PARTY.md` and
`THIRD_PARTY_NOTICES.md` (third-party material and licenses).
