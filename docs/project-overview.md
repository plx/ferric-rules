# ferric-rules — Project Overview

A CLIPS-compatible forward-chaining rules engine in Rust. Workspace-oriented:
core engine crates, a facade, FFI/CLI, language bindings, Python/Rust tooling,
benches, and a large compatibility test corpus.

This document is a **detailed table of contents**: it points at what exists and
what it contains, not how to use it. Per-area reference docs belong under
`docs/`.

---

## 1. Rust workspace (`crates/`)

Four-crate core plus facade, FFI, CLI, Python binding crate, and a bench-data
generator.

### `ferric-rules-core`

Rete network internals and low-level engine data structures. Not for end-user
use; re-exported via `ferric_rules::core`.

- `value.rs`, `symbol.rs`, `string.rs`, `encoding.rs` — value primitives,
  interned symbols, CLIPS string/encoding rules.
- `fact.rs` — ordered/template fact types, `FactBase`, `FactId`, `TemplateId`,
  timestamps.
- `alpha.rs`, `beta.rs`, `rete.rs`, `token.rs` — alpha/beta networks, token
  store, join network.
- `negative.rs`, `ncc.rs`, `exists.rs` — negative, NCC, and existential node
  types.
- `agenda.rs`, `strategy.rs` — activation agenda and conflict-resolution
  strategies (Depth, Breadth, LEX, MEA).
- `binding.rs` — `BindingSet`, `VarId`, `VarMap`.
- `compiler.rs` — `ReteCompiler`, compilable pattern/rule/condition IR.
- `validation.rs` — pattern nesting and structural validation.
- `serde_helpers.rs` (feature `serde`) — `FxHashMap`/`FxHashSet` serde adapters.
- `tracing_support.rs` — optional tracing spans.

### `ferric-rules-parser`

Three-stage parser: Lexer → S-expression → Stage 2 AST.

- `lexer.rs` — tokenization, including module-qualified `MODULE::name`
  (single-token).
- `sexpr.rs` — S-expression parser, `parse_sexprs`, atoms/connectives.
- `stage2.rs` — construct interpretation: `defrule`, `deftemplate`,
  `deffacts`, `deffunction`, `defgeneric`/`defmethod`, `defglobal`,
  `defmodule`, including patterns, actions, constraints, slot definitions.
- `qualified_name.rs` — `MODULE::name` parsing helpers.
- `span.rs`, `error.rs` — source spans, lex/parse error types.

### `ferric-rules-runtime`

Engine, loader, execution loop, evaluator, modules, I/O.

- `engine.rs` — `Engine` type, public engine API (asserts, run, reset, focus,
  etc.).
- `loader.rs` — Stage 2 AST → rete network wiring and construct registration.
- `execution.rs` — run/step loop, halt/reset/clear deferred flags,
  action-result tuple.
- `actions.rs` — RHS actions (`assert`, `retract`, `modify`, `duplicate`,
  `halt`, `focus`, `bind`, `printout`, …).
- `evaluator.rs` — shared expression evaluator used by RHS, `test` CEs, user
  functions, generic methods.
- `functions.rs` — builtin registry, dispatch chain (builtins → user → generic
  → `UnknownFunction`), Section 10.2 stdlib surface.
- `templates.rs` — template registration, slot typing/constraints.
- `modules.rs` — module registry, focus stack, cross-module visibility.
- `router.rs` — `OutputRouter`, per-channel output capture, `read`/`readline`
  input buffers.
- `config.rs` — `EngineConfig` and encoding/strategy/execution-limit settings.
- `qualified_name.rs` — runtime-side module-qualified resolution.
- `serialization.rs` (feature `serde`) — `EngineSnapshotRef`/`Owned`,
  bincode/JSON/CBOR/MessagePack/Postcard payloads, ExternalAddress
  pre-flight rejection.
- `integration_tests.rs`, `phase{2,3,4}_integration_tests.rs` — in-crate
  integration test modules.
- `test_helpers.rs`, `tracing_support.rs`.

### `ferric` (facade)

Thin re-export crate: `ferric_rules::core`, `ferric_rules::parser`, `ferric_rules::runtime`.

- `tests/ferric_semantic_regressions.rs` — Ferric-only semantic regression
  suite (engagement scenarios, etc.).
- `tests/scaling_tests.rs` — `#[ignore]` asymptotic scaling regression tests
  (join propagation, engine run, retraction cascade, churn, alpha fanout),
  run via `just scaling-check`.
- `benches/` — Criterion suite (see §5).

### `ferric-rules-pinned`

Rust-owned worker-thread execution for embedding contexts that cannot preserve
the runtime engine's thread affinity. Requests use a bounded FIFO queue with
cooperative cancellation and optional Apple autorelease-pool policies.
Completion-aware request envelopes keep the operation separate from one
terminal callback: an ordinary operation panic becomes `PinnedError::Internal`,
the callback still runs exactly once, and the worker continues serving later
requests.

### `ferric-rules-ffi`

C-ABI wrapper over the runtime. Produces `libferric_rules_ffi.{a,dylib,so}` plus
auto-generated `ferric.h` via `cbindgen`. Dedicated `ffi-dev`/`ffi-release`
profiles retain unwind support for generated panic-containing export wrappers.

- `boundary.rs` — shared panic containment, error recording, and return sentinels.
- `engine.rs` — `ferric_engine_*` surface (new/free, asserts, run, serialize,
  query).
- `types.rs` — `FerricValue`, allocator-owned multifield copying, arrays,
  ownership, and recursive cleanup.
- `error.rs` — global + per-engine error channels, thread-local storage.
- `header.rs` — C header metadata generation.
- `tests.rs` + `src/tests/` — lifecycle, contract-lock, diagnostic-parity,
  error-model, execution, template-assertion, copy-error, build-matrix,
  ffi-expansion, values, multifield-copy, and header tests.
- `tests/c/` — real-C ABI regressions run under the sanitizer harness.
- Thread-affinity invariant: runtime state is owner-thread-only. Per-engine
  last-error copies are synchronized across threads; borrowed last-error
  pointer use must not overlap other borrowed reads or destruction. The
  destruction-only unchecked free skips affinity but must not overlap access.

### `ferric-rules-ffi-macros`

Internal proc-macro dependency that separates each authored C signature into a
generated `extern "C"` containment wrapper and a private non-extern
implementation. It is published only so registry builds of
`ferric-rules-ffi` retain the same boundary architecture.

### `ferric-rules-cli`

`ferric` binary: batch + interactive driver.

- `commands/run.rs`, `check.rs`, `snapshot.rs`, `version.rs`, `common.rs`.
- `commands/repl/` — `commands.rs`, `display.rs`, `history.rs`, `input.rs`,
  `session.rs` (rustyline-based REPL).
- Exit codes: 0 success, 1 runtime error, 2 usage error.

### `ferric-rules-python`

PyO3 extension module (`ferric`) built via `maturin`. Located in
`crates/ferric-rules-python/`; tests in `tests/*.py`.

- `engine.rs` — `PyEngine`.
- `fact.rs` — `Fact`, `FactType`.
- `value.rs` — `Symbol`, `ClipsString` (preserves symbol/string distinction).
- `config.rs` — `Strategy`, `Encoding`, `Format` (serde feature).
- `result.rs` — `RunResult`, `HaltReason`, `FiredRule`.
- `error.rs` — exception hierarchy registered on module init.
- `testing` feature — `engine_instance_count` for test harness.

### `ferric-rules-bench-gen`

Standalone binary generating benchmark inputs (for the `benches/` + scaling
tests). Single `main.rs`.

---

## 2. Language bindings (`bindings/`)

### `bindings/go` — Go binding on top of `ferric-rules-ffi`

- `engine.go`, `engine_options.go`, `pinned_engine.go` — engine façade;
  pinned-goroutine variant for Go's movable goroutines vs. FFI thread affinity.
- `coordinator.go`, `coordinator_options.go`, `manager.go` — multi-engine-type
  orchestration (`Coordinator` + per-type `Manager`).
- `fact.go`, `values.go`, `result.go`, `iterators.go` — Go-side value/fact
  model and iteration.
- `wire_conv.go`, `wire_helpers.go`, `wire_types.go` — FFI marshaling layer.
- `observability.go`, `errors.go`, `example_test.go`.
- `internal/ffi/` — cgo wrapper:
  - `ffi.go`, `accessors.go`, `types.go`, `serialization.go`
  - `lib/` — vendored `libferric_rules_ffi.a` + `ferric.h` (copied by
    `just build-go-ffi`).
- `temporal/` — Temporal.io activity wrappers (`activity.go`,
  `activity_options.go`).
- Test suite: `*_test.go` alongside sources; property tests, serialization
  tests, stress/race targets (`test-go-stress`).
- `CI_POLICY.md` — binding CI rules.

Planned bindings (not present): C++, Swift.

---

## 3. Tests (`tests/` at workspace root)

- `tests/fixtures/` — hand-written `.clp` fixtures grouped by phase and area
  (`phase2_*.clp`, `phase3_*.clp`, `phase4_stdlib_*.clp`, `forall_vacuous_truth.clp`).
  Also `fixtures/cli/` and `fixtures/ffi/` for CLI and FFI harnesses.
- `tests/clips_compat/` — real-world CLIPS compatibility corpus and
  `fixtures/`.
- `tests/examples/` — third-party CLIPS projects used for compatibility
  validation (clips-official, clips-executive, fawkes-robotics, galletas,
  rcll-refbox, telefonica-clips, diagnostico-covid, labcegor,
  decision-tree-family, missionaries-cannibals, language-deficit-screener,
  learn-clips, small-clips-examples, troubleshooting). `SOURCES.md` tracks
  provenance; `bat-analysis.json` is the parsed `.bat` manifest.
- `tests/harnesses/` — standalone run harnesses per project.
- `tests/generated/` — tooling-produced artefacts (benchmarks,
  `test-suite-segments/`, `segment-check-expectations.json`).

Crate-local tests live under each crate's `tests/` (`ferric/tests/`,
`ferric-rules-ffi/src/tests/`, `ferric-rules-python/tests/`, `ferric-rules-cli/tests/`).

---

## 4. Tools (`tools/ferric-tools/`)

Python package managed with `uv`; commands wrapped by `just` recipes. Shared
helpers: `_clips_parser.py`, `_manifest.py`, `_subprocess.py`, `_formatting.py`,
`_paths.py`.

- `bat/` — `analyze`, `convert`, `extract`, `harness`, `segment`: processes
  CLIPS `.bat` batch scripts into runnable `.clp` segments and harnesses.
- `compat/` — `scan`, `run`, `report`, `diff`, `semantic_gate`, `ci_gate`:
  compatibility assessment and blocking reviewed-policy enforcement
  pipeline (ferric vs. CLIPS reference container). Its structured,
  non-vacuous oracle contract is documented in
  [`compatibility-assessment.md`](compatibility-assessment.md).
- `perf/` — `collect`, `report`, `diff`: Criterion → performance-manifest
  pipeline.

---

## 5. Benchmarks (`crates/ferric-rules/benches/` + `benches/`)

Top-level `benches/` is documentation-only (`README.md`, `PROTOCOL.md`); the
actual Criterion benches live in the facade crate.

- `engine_bench.rs`, `compile_bench.rs`, `evaluator_bench.rs`.
- `join_bench.rs`, `negation_bench.rs`, `exists_bench.rs`, `forall_bench.rs`,
  `ncc`-coverage via `negation_bench`.
- `waltz_bench.rs`, `manners_bench.rs` — classic AI rule-engine workloads.
- `cascade_bench.rs`, `churn_bench.rs`, `alpha_fanout_bench.rs` — scaling /
  throughput microbenches.
- `constraint_bench.rs`, `strategy_bench.rs`, `module_bench.rs`,
  `query_bench.rs`.
- `serialization_bench.rs` — in `ferric-rules-runtime` (requires `serde` feature).

CI gates: `bench-smoke` (compile-only), `bench-thresholds` (absolute ns
thresholds). Scaling regression: `just scaling-check` runs facade-crate
`scaling_tests.rs` with two sizes, asserts asymptotic ratio bounds.

---

## 6. Docker (`docker/`)

- `clips-reference/` — reference CLIPS container used by `compat-run` and
  `perf-collect --clips-reference`. Wrapped by `scripts/clips-reference.sh`.
- `bench-runner/` — container for reproducible bench runs.

---

## 7. Top-level documentation

### `docs/` (user/reader facing)

- `users-guide.md` — embedding-focused walkthrough from "hello world" to
  multi-module pipelines.
- `compatibility.md` — CLIPS compatibility matrix, by Basic Programming Guide
  section.
- `compatibility-assessment.md` — maintainer contract for structured
  differential oracles and evidence.
- `migration.md` — CLIPS → ferric migration guide.
- `benchmark-policy.md` — regression policy and thresholds.
- `performance-analysis.md` — Phase 6 baseline numbers.
- `phase6-baseline.md` — historic baseline snapshot.
- `project-overview.md` — **this file**.

### Repo-root docs

- `README.md` — user intro + engagement-rule walkthrough.
- `AGENTS.md` / `CLAUDE.md` — agent guidelines (`CLAUDE.md` aliases
  `AGENTS.md`).

---

## 8. Build system (`justfile`, `scripts/`)

### `just` recipes (key groups)

- Build: `build`, `build-release`, `build-crate`, `build-ffi(-release)`,
  `build-cli(-release)`, `build-go-ffi`.
- Test: `test`, `test-<crate>`, `test-filter`, `test-go(-race|-stress)`,
  `py-test`, `py-bindings-test`.
- Lint/format: `fmt(-check)`, `clippy`, `cargo-check`, `py-fmt(-check)`,
  `py-lint(-fix)`, `go-lint` (auto-installs golangci-lint).
- Composite: `check`, `preflight`, `preflight-pr` (fmt + clippy + tests +
  cargo check + Python + Go lint). **Required before any PR push.**
- Tracing: `check-tracing` (feature gate build+clippy+test).
- Bench: `bench`, `bench-engine`, `bench-waltz`, `bench-serde`,
  `bench-manners`, `bench-join`, `bench-churn`, `bench-negation`,
  `bench-thresholds`, `bench-compare`, `scaling-check`.
- Compat: `compat-scan`, `compat-run`, `compat-report`, `compat-diff`,
  `compat-semantic-gate`, `compat-ci-gate`, `compat-semantic-lane`,
  `assess-compatibility`.
- Bat processing: `bat-analyze`, `bat-extract`, `bat-convert`,
  `harness-gen`, `segment-check`.
- Perf: `perf-collect`, `perf-report`, `perf-diff`, `assess-performance`.
- CLIPS reference: `clips-build`, `clips-run`.
- Docs: `doc`, `doc-open`.
- Issue triage: `find-next-matching-issue`, `list-open-issues`.

### `scripts/`

- `preflight.sh` — wraps preflight flow.
- `bench-compare.sh`, `bench-thresholds.sh` — comparative + threshold gates.
- `clips-reference.sh` — Docker CLIPS reference driver.
- `compose-pr-comment.sh` — CI PR comment formatting.
- `find-next-matching-issue.sh`, `list-open-issues.sh` — GitHub triage helpers.

---

## 9. Feature flags

- `serde` (per crate, propagated `parser → core → runtime → ffi → facade`) —
  enables engine serialization; required for Go bindings' serialization and
  CLI snapshot commands. `slotmap`/`smallvec` serde features always on.
- `tracing` — optional tracing spans (runtime + core); validated by
  `just check-tracing`.
- `testing` (ferric-rules-python only) — exposes `engine_instance_count` for
  teardown-leak tests.

Workspace lints: `unsafe_code = deny`; clippy `all = deny`, `pedantic = warn`
with a few pedantic allows (`module_name_repetitions`, `must_use_candidate`,
`missing_errors_doc`, `missing_panics_doc`).

---

## 10. Status snapshot

- Core engine: Phase 2–4 complete (rules, templates, patterns, negation,
  NCC, exists, forall subset, modules, focus, user functions, globals,
  generics, Section 10.2 stdlib, agenda strategies).
- Bindings: C (FFI) and Go shipping; Python shipping with audited surface;
  Swift/C++ planned.
- Known gaps: logical/truth-maintenance support, COOL object system
  (not planned), `if/then/else` expression form, full pattern-nesting
  triples, some exotic connectives.
