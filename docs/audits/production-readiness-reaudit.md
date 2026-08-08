# Production-Readiness Re-Audit Playbook

- Status: execution template
- Audience: an independent release auditor who did not implement the remediations
- Scope: `ferric-rules`, its CLI, C FFI, Rust facade, Python binding,
  Node.js/TypeScript binding, Go binding, snapshots, tooling, packages, and
  published compatibility claims

## 1. Purpose and audit rule

Use this playbook after the known correctness, compatibility, robustness, and
tooling findings have been remediated. It is a release gate, not a development
checklist. The auditor must reproduce evidence from a clean checkout at the exact
candidate commit. A green pull request, an earlier test report, or a maintainer's
statement is supporting context, not evidence of a pass.

The audit has four possible outcomes:

- **PASS**: every blocking gate passes and the retained evidence is complete.
- **PASS WITH ACCEPTED RISKS**: no critical or high-severity issue remains, and
  every lower-severity exception has an owner, expiry date, user-visible
  documentation, and written approval from the release decision-makers.
- **FAIL**: any blocking gate fails, evidence is missing, a claimed behavior is not
  tested non-vacuously, or an unresolved issue invalidates a production claim.
- **INCONCLUSIVE**: the candidate cannot be evaluated reproducibly because the
  environment, reference implementation, package, credentials, or workload is
  unavailable. Inconclusive is not permission to release.

Do not repair code while executing the audit. Record a failing reproduction,
open or link a remediation issue, and restart the affected audit sections from a
new candidate commit. This preserves independence and prevents a moving target.

## 2. Required roles and separation

Assign these roles before starting:

| Role | Responsibility |
|---|---|
| Candidate owner | Names the immutable commit and declared release surfaces. |
| Independent auditor | Executes this playbook and signs the evidence manifest. |
| Compatibility reviewer | Reviews CLIPS differential fixture design and oracles. |
| Safety reviewer | Reviews unsafe/FFI, deserialization, and sanitizer evidence. |
| Performance reviewer | Confirms benchmark protocol, baselines, and claims. |
| Release approver | Accepts or rejects the final decision; does not rewrite results. |

One person may fill multiple review roles when staffing requires it, but the
candidate owner must not be the sole auditor or sole release approver.

## 3. Candidate declaration and prerequisites

Create an audit ticket containing all of the following before running commands:

- Candidate commit SHA, branch, proposed version, and release date.
- Repository URL and whether submodules or Git LFS are required.
- The complete release surface:
  - Rust crates intended for publication.
  - `ferric` CLI binaries and supported targets.
  - C header/static/shared libraries and ABI promises.
  - Python package name, Python versions, interpreters, and wheel targets.
  - Node package names, Node versions, N-API level, and native targets.
  - Go module version, Go versions, and linked native-library targets.
  - Snapshot formats and compatibility/versioning promise.
- Supported operating systems, architectures, libc variants, and minimum Rust
  version. Do not infer these from build scripts.
- Exact CLIPS reference version and source/image digest. The current Docker
  reference reports CLIPS 6.30; if another version is normative, declare it.
- Claimed CLIPS-compatible subset and all intentional behavioral differences.
- Performance objectives, capacity assumptions, maximum input sizes, latency
  percentiles, memory ceilings, and throughput targets.
- Threat/trust boundaries:
  - Whether CLIPS source can be supplied by untrusted users.
  - Whether snapshots can be supplied by untrusted users.
  - Whether callbacks can invoke application code.
  - Whether file and I/O actions are enabled in production.
- Links to every remediation issue included in the candidate.
- Links to any requested risk acceptance.

### 3.1 Required tools

Record exact versions in `environment.txt`:

```sh
git --version
rustc --version --verbose
cargo --version --verbose
just --version
clang --version
cmake --version
python3 --version
uv --version
node --version
npm --version
go version
docker version
clips -version
hyperfine --version
jq --version
```

Install additional audit tools at pinned versions and record their versions:

- `cargo-audit`, `cargo-deny`, `cargo-about`, `cargo-fuzz`, `cargo-geiger`, and
  `cargo-nextest` if used.
- A nightly Rust toolchain containing Miri and sanitizer support.
- Valgrind on a supported Linux target.
- AddressSanitizer, UndefinedBehaviorSanitizer, LeakSanitizer, and a C/C++
  compiler suitable for ABI consumers.
- Platform package inspection tools such as `auditwheel`, `delocate`, or
  equivalent for the declared artifacts.

If a required tool cannot run on a target, identify the equivalent target/tool
that covers the same failure class. "Not installed" is not a pass.

## 4. Evidence layout and integrity

Create an evidence directory outside the repository worktree:

```text
reaudit-<version>-<short-sha>/
  manifest.tsv
  environment.txt
  candidate/
  build/
  tests/
  compatibility/
  rete/
  parser/
  ffi/
  bindings/
    python/
    node/
    go/
  fuzz/
  miri/
  sanitizers/
  snapshots/
  performance/
  dependencies/
  packages/
  soak/
  shadow/
  docs/
  decision/
```

For every command, retain:

- A UTF-8 log containing the command, start/end UTC timestamps, working
  directory, environment overrides, exit code, stdout, and stderr.
- Any JUnit, JSON, Criterion, coverage, sanitizer, fuzz corpus, or package
  artifact produced.
- The host identifier, OS/kernel, architecture, CPU, memory, and container/image
  digest.
- A SHA-256 digest and byte size in `manifest.tsv`.

The evidence manifest must include the candidate source archive and all published
package candidates. Generate the final manifest only after the audit directory is
read-only. Retain evidence for at least the release support lifetime plus 90 days,
or the organization's longer compliance period.

Never include production secrets, customer source, or raw sensitive facts in the
evidence archive. Use stable case identifiers and approved redaction.

## 5. Clean-room setup

### 5.1 Create an isolated candidate checkout

Use a new disposable machine, VM, or container with no repository build cache.
Resolve the exact candidate before checking it out:

```sh
git clone --no-local <repository-url> ferric-reaudit
cd ferric-reaudit
git fetch --tags --force
git checkout --detach <candidate-sha>
test "$(git rev-parse HEAD)" = "<candidate-sha>"
git status --porcelain=v1
git show --no-patch --format=fuller HEAD
git tag --points-at HEAD
```

Expected:

- `git status --porcelain=v1` is empty.
- `HEAD` exactly matches the declared candidate.
- Lockfiles and generated FFI headers are present.

Archive the candidate with `git archive`, record its digest, and conduct at least
one package build from that archive rather than from a developer worktree.

### 5.2 Dependency reproducibility

Run locked online builds first, then repeat a representative build with network
disabled using only the populated dependency caches:

```sh
cargo fetch --locked
cargo build --workspace --locked --release
cargo build --workspace --locked --release --offline
```

Use `npm ci`, `uv sync --frozen`, and the committed Go module sums rather than
dependency-updating commands. Fail if a lockfile changes, an unpinned install is
required, or the offline rebuild produces materially different package contents.

### 5.3 Generated-file cleanliness

Run generation/check commands and require a clean worktree afterward:

```sh
just license-notices-check
just check-examples-sync
git diff --exit-code
git status --porcelain=v1
```

Build the Go FFI and verify committed headers as CI does:

```sh
just build-go-ffi
git diff --exit-code -- \
  crates/ferric-rules-ffi/ferric.h \
  bindings/go/internal/ffi/lib/ferric.h
```

Any uncommitted generated delta is a packaging/reproducibility failure.

## 6. Baseline quality gates

Run the repository's complete local gate before specialized testing:

```sh
just preflight
just check-tracing
just scaling-check
```

Also run documentation and release-mode tests explicitly:

```sh
cargo test --workspace --locked --release
cargo test --workspace --locked --all-features
cargo doc --workspace --locked --no-deps
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --locked --no-deps
```

Pass criteria:

- Zero test, formatting, lint, doc, example, or generated-file failures.
- Zero ignored failures hidden by a wrapper.
- No unexplained flaky retry. A test that passes only on retry fails the audit.
- The full command log proves all workspace members and declared feature
  combinations were covered.

## 7. Semantic differential validation against CLIPS

### 7.1 Reference integrity

Pin the CLIPS executable or Docker image by cryptographic digest. Record:

- CLIPS version banner.
- Image/source digest and build recipe.
- Host/container architecture.
- Startup commands and router/output configuration.

Run harness self-tests before any fixture:

1. A deliberately equal program must classify equivalent.
2. A one-character output difference must classify divergent.
3. A differing final fact must classify divergent even when stdout is equal.
4. A CLIPS parse error, construct error, runtime error, timeout, signal, and
   nonzero process exit must each receive distinct classifications.
5. A harnessed temporary file must be accessible to both engines.
6. A source ending in a non-`MAIN` module must still execute the verification
   oracle.
7. An empty output cannot be called equivalent unless the fixture explicitly
   declares empty output and a separate state oracle proves execution.

Fail this section if the runner mistakes a runtime diagnostic for a load error,
silently switches to Ferric-only mode, cannot prove which CLIPS version ran, or
normalizes away meaningful differences.

### 7.2 Non-vacuous fixture contract

Every differential fixture must declare:

```text
id:
feature:
setup:
expected phase reached:
expected rule firing count or named firing markers:
expected canonical final facts:
expected stdout/stderr:
expected diagnostic class and continuation behavior:
expected halt reason:
normalizations allowed:
```

Each executable fixture must emit a unique start marker and a unique completion
marker, or expose equivalent state through a structured adapter. Assert both.
Also assert at least one feature-specific effect. A fixture that merely loads,
resets, exits zero, or produces matching empty output is vacuous and fails.

Do not compare only text intended for humans. Canonicalize and compare:

- Ordered and template facts, including type tags, slot names, multifields, and
  duplicate behavior.
- Rule firing count and explicit rule markers.
- Halt reason and run limit.
- Captured router/channel outputs independently.
- Globals and focus stack where the fixture exercises them.
- Diagnostics by phase, category, and whether later RHS actions or later rules
  execute.

Normalize only nondeterministic fields listed by the fixture, such as fact IDs or
allowed float formatting. Preserve ordering whenever ordering is the behavior
under test.

### 7.3 Required semantic matrix

At minimum, differential fixtures must cover:

- Lexing: comments, escaped quotes, symbols, strings, integers, floats,
  variables, global variables, and multifield syntax.
- Construct lifecycle: load, reset, clear, multiple loads, legal and illegal
  redefinition of every supported construct.
- `deffacts`: no load-time assertion, reset-time assertion, `initial-fact`
  ordering, repeated resets, module scoping, globals in initial facts.
- Rules: empty LHS, salience, refraction, duplicate facts, retraction and
  re-assertion, depth/breadth/LEX/MEA behavior within documented guarantees.
- Templates: defaults, multislots, missing/unknown slots, module visibility,
  facts surviving or preventing redefinition as CLIPS specifies.
- Conditional elements: positive joins, `test`, `not`, `exists`, `forall`,
  NCC, `and`, `or`, logical support if claimed, and every documented nesting
  boundary.
- Constraints: equality, numeric coercion, `&`, `|`, `~`, predicate and
  return-value constraints, repeated variables, multifield head/tail capture.
- RHS actions: assert, retract, modify, duplicate, bind, printout, focus, halt,
  reset, clear, run, loops, switch, queries, load/save operations if claimed.
- Deffunctions: zero/regular/wildcard parameters, lexical/global scope,
  redefinition, recursion limit, explicit early `return`, last-expression
  return, nested calls, and error propagation.
- Generics/methods: dispatch ranking, wildcard methods, `call-next-method`,
  method redefinition, ambiguity, visibility, and deffunction conflicts.
- Modules: import/export, qualified names, ambiguous names, focus stack, reset
  focus behavior, and module ownership of generated/harness rules.
- Standard library: every function marked supported in compatibility docs,
  including wrong arity/type/domain and boundary values.
- Errors: parse, interpretation, compile, runtime evaluation, I/O, recursion,
  and wrong-thread behavior. Assert whether the current RHS stops, whether the
  engine continues to another activation, and the public error channel.
- Encoding modes: ASCII, UTF-8, and mixed mode, including invalid data at FFI
  boundaries.

Explicitly retain regression fixtures for:

- Deffacts load/reset timing and `initial-fact` ordering.
- Same-name `defrule` and `deftemplate` redefinition.
- Early `return` from a deffunction.
- RHS evaluation error continuation.
- Maximum accepted/rejected pattern and S-expression nesting.

### 7.4 Corpus assessment

Run the maintained focused suite and external-example assessment:

```sh
cargo test -p ferric-rules --test ferric_semantic_regressions -- --nocapture
just compat-semantic-lane
just assess-compatibility
```

`just assess-compatibility` is the complete blocking sequence: it builds the
candidate and pinned reference, scans, generates and independently verifies
harnesses, runs every declared fixture through both engines, enforces the exact
policy, and reports. Do not substitute the lower-level scan/run/report commands
without the generation, verification, and policy-gate steps.

Require the manifest to identify the exact candidate and CLIPS digest. Manually
sample at least:

- 20 equivalent fixtures across distinct feature families.
- Every divergent fixture.
- Every harness-generated library case.
- Every error-classification case.
- Every result based on output normalization.

No unexplained divergence is allowed inside the claimed compatible subset.
Unsupported external examples may remain incompatible only when the detected
unsupported feature is accurate and documented.

## 8. RETE correctness and invariants

Run existing unit, integration, property, scaling, and churn tests. Add temporary
audit instrumentation in a separate audit branch only if public/debug APIs cannot
observe the invariants; do not use that branch as the release candidate.

Verify after every generated operation sequence:

- Each live fact appears in exactly the applicable alpha memories and no
  inapplicable alpha memory.
- Every beta token has a valid parent chain, valid fact references, and bindings
  consistent with all joins on its path.
- Removing a fact removes all dependent tokens, activations, negative blockers,
  NCC results, and logical supports; no dangling identifier is reachable.
- A negative node is enabled exactly when its blocker set is empty.
- `exists` contributes at most one logical match for an outer token regardless
  of the number of witnesses.
- NCC owner/result relationships are bidirectionally consistent.
- Agenda entries correspond to live terminal tokens, enabled rules, current
  refraction state, and valid module ownership.
- Agenda ordering obeys the selected conflict strategy and salience contract.
- Duplicate suppression and fact identity/refraction behave as documented.
- `modify` is equivalent to the documented retract/assert behavior and does not
  leak stale matches.
- `reset` clears runtime state while preserving constructs; `clear` removes both.
- Repeated reset/run cycles reach the same canonical state for deterministic
  rule sets.
- Serialization round trips preserve all invariant-bearing relationships.

### 8.1 Reference-model property test

Generate bounded sequences of:

```text
load construct
assert fact
retract live fact
modify live template fact
duplicate fact
reset
run N
step
clear
serialize/deserialize
```

Compare Ferric after each operation with a deliberately simple reference model
for fact membership, matching, and agenda eligibility. Use deterministic seeds,
retain every failing/minimized seed, and run at least 100,000 operations across
multiple seeds in release mode. Include high-churn scenarios and IDs reused after
reset/deserialize.

Run `just scaling-check` separately. Scaling success is not proof of correctness;
the model/invariant oracle must also pass.

## 9. Parser and language robustness

Exercise both `ferric check` and the library parser/loader. Inputs include:

- Empty, whitespace-only, comment-only, and truncated source.
- Every token at end-of-file and every missing delimiter.
- Deeply nested lists and expressions immediately below and above the declared
  limit.
- Deeply nested `not`, `exists`, `forall`, NCC, expression, and action forms.
- Very wide lists, rules, multifields, strings, identifiers, and construct sets.
- Escaped quotes/backslashes, semicolons inside/outside strings, CRLF, lone CR,
  NUL bytes, invalid UTF-8 at byte-oriented APIs, and non-ASCII in every encoding
  mode.
- Extreme integer/float values, overflow, NaN/infinity where representable,
  division by zero, and malformed exponents.
- Random token insertion/deletion, duplicated delimiters, and mixed valid/invalid
  top-level constructs.
- Valid constructs following an invalid construct to verify documented recovery
  and absence of unintended partial mutation.

Pass criteria:

- No panic, abort, stack overflow, hang, or allocator runaway.
- Exceeding a limit returns a stable diagnostic with source location.
- The documented accepted maximum succeeds; maximum plus one fails cleanly.
- Time and peak memory remain within declared parser budgets for maximum-size
  supported input and adversarial invalid input.
- A failed load leaves the engine in the documented atomic or partial state;
  the behavior is tested and documented.

Run parser tests under a deliberately small thread stack to expose accidental
recursion. Independently test destruction/drop of deeply nested ASTs, because
parsing may succeed while recursive drop overflows.

## 10. Native memory safety and C FFI

### 10.1 Rust unsafe inventory

Record all unsafe code and generated bindings:

```sh
rg -n "unsafe|extern \"C\"|no_mangle|export_name" crates bindings
cargo geiger --all-features
```

Review each unsafe block for:

- Pointer provenance, alignment, validity duration, and aliasing.
- Ownership transfer and exact allocator/free pairing.
- Integer conversion, length/capacity, null-plus-zero-length handling.
- Panic containment across `extern "C"`.
- Thread-affinity enforcement and intentionally exempt diagnostic/free calls.
- Callback lifetime, reentrancy, cancellation, and worker-thread execution.
- Destruction ordering and behavior during partial construction or panic.

No unwinding may cross a C ABI boundary. Production FFI artifacts must use the
documented `panic=abort` profile or prove an equivalent catch boundary.

### 10.2 ABI contract tests

Compile minimal C and C++ consumers against the installed, not source-tree,
header/library on every declared platform. Cover:

- Create/configure/free and repeated create/free.
- Load/reset/run/step/clear/halt.
- Ordered and template fact creation, lookup, mutation, and returned fact IDs.
- Every `FerricValue` variant, nested multifields, empty data, and external
  address ownership rules.
- Error enums, per-engine errors, global thread-local errors, copy-to-buffer
  sizing, truncation, and lifetime after subsequent calls.
- Output and action-diagnostic count/copy/clear.
- Every snapshot format and allocated-byte free function.
- Pinned-engine submit, cancellation, callback completion, close/drain, and
  forced free contracts.

For every pointer/length API test:

- Null handle.
- Null data with zero length and with nonzero length.
- Zero-sized destination buffer.
- Exact-size and one-byte-short destination.
- Invalid enum discriminant.
- Stale fact ID.
- Wrong-thread call.
- Repeated free only where the API explicitly promises idempotence; otherwise
  validate ownership with sanitizers without deliberately invoking undefined
  behavior in an uncontrolled process.

Compare generated headers byte-for-byte with committed headers. Use ABI tooling
to compare exported symbol names/signatures with the previous supported release.
Any removal or incompatible layout change requires a declared breaking release.

### 10.3 Dynamic memory tools

Run the C contract suite and representative engine workloads under:

- AddressSanitizer and LeakSanitizer.
- UndefinedBehaviorSanitizer.
- Valgrind Memcheck on Linux.
- The platform's native leak checker where applicable.

Pass criteria are zero invalid accesses, use-after-free, double-free, leaks,
uninitialized reads, and sanitizer runtime errors. Suppressions must be
dependency-specific, minimal, retained, and reviewed; suppressing project frames
fails the gate.

## 11. Binding-specific conformance

Every binding must run against the exact native artifact that will ship. Test the
same semantic fixture set through the binding as through Rust, then test its
binding-specific lifecycle.

### 11.1 Common binding matrix

For Python, Node, and Go, cover:

- Default/configured construction and construction from each snapshot format.
- Empty engine, load, reset, repeated run/step, run limit zero/one/unlimited,
  halt, clear, and reuse after errors.
- Deterministic destruction, explicit close/free, repeated close as documented,
  finalizer/GC fallback, and process exit with live engines.
- Ordered/template facts and every scalar/multifield value at minimum, maximum,
  empty, Unicode, and wrong-type boundaries.
- Fact snapshots remaining valid after engine mutation or close, if promised.
- Error type/code/message/cause preservation for parse, load, runtime,
  thread-affinity, closed handle, invalid argument, and snapshot errors.
- Output channels and action diagnostics, including copy/clear behavior.
- Cancellation before dispatch, while queued, during run, and racing completion.
- Concurrent submissions, fairness/FIFO promises, close while queued/in flight,
  callback panic/exception, worker panic/exit, and post-close calls.
- Ten thousand create/use/close cycles with memory plateau measurement.
- No native handle use from an unauthorized thread.

### 11.2 Python

Build wheels, install them into fresh environments, and run:

```sh
cd crates/ferric-rules-python
uv sync --frozen
uv run maturin build --release
uv run pytest tests/ -v
```

Test declared CPython versions, currently `>=3.9`, rather than only CI's Python
3.12. Test all declared OS/architecture wheel tags. Include:

- Context-manager and explicit-close behavior if exposed.
- Reference cycles, GC on a different scheduling context, interpreter shutdown,
  and exceptions during construction/method calls.
- Python `int` overflow, float edge cases, embedded NUL, Unicode, bytes/buffer
  ownership, and list/tuple-to-multifield conversions.
- Calls from multiple Python threads and behavior while the GIL is released.
- Repeated import/unload in subprocesses and subinterpreters if claimed.

Inspect wheels for unintended shared-library dependencies and install each with
`pip --no-index` into a clean environment before running a smoke program.

### 11.3 Node.js and TypeScript

Build the native addon and package using clean installs:

```sh
cd crates/ferric-rules-napi
npm ci
npm run build

cd ../../packages/ferric
npm ci
npm run build
npm run lint
npm test
npm run test:coverage
```

Run on every declared Node version (`package.json` currently claims Node
`>=18`) and target. Cover separately:

- Synchronous `Engine` thread affinity and native proxy/factory behavior.
- `EngineHandle` worker initialization from source/snapshot, request correlation,
  transferable buffers, abort, worker error/exit, pending promise rejection, and
  close.
- `EnginePool` thread-count validation, queueing, FIFO/fairness claims, parallel
  evaluations, cancellation while queued/in flight, close/drain, and worker
  replacement or failure policy.
- Structured-clone conversion for every value/error type.
- Event-loop responsiveness during long runs.
- Unhandled rejection/exception and process-exit cleanliness.

Pack with `npm pack`, inspect the tarball, install it into a fresh consumer
project with no repository-relative paths, and run CommonJS/ESM and type-check
smokes as supported.

### 11.4 Go

Build the exact native library, then run:

```sh
just build-go-ffi
just test-go
just test-go-race
just test-go-stress 100
just go-lint
```

The module currently declares Go 1.25; test that version and every additional
declared version. Cover separately:

- Raw thread-affine `Engine` with locked/unlocked OS-thread misuse.
- `PinnedEngine` FIFO serialization, close/drain, finalizer fallback, cancellation,
  panic propagation, and concurrent callers.
- `Coordinator` lazy engine creation, multiple workers, routing/fairness,
  cancellation, concurrent close, and worker failure.
- Temporal/Nexus adapters: deterministic serialization, retry/cancellation,
  activity teardown, and error translation without starting external services
  unless integration tests explicitly provision them.
- cgo pointer rules, Go memory passed to C, callback lifetimes, and finalizers on
  arbitrary goroutines.

Run `go test -race -count=100` for the concurrency-heavy packages and a
production-duration stress test with randomized close/cancel/submit races.
Install the Go module into a fresh consumer module and prove native header/library
discovery without repository-relative assumptions.

## 12. Fuzzing, Miri, and sanitizers

### 12.1 Fuzz targets

Required persistent fuzz targets:

- Lexer/parser on arbitrary bytes.
- Stage-2 construct interpretation on parsed/structured inputs.
- Engine `load_str`, including multiple sequential loads.
- Assert/retract/modify/reset/run operation sequences.
- Expression evaluator and standard-library calls.
- All snapshot decoders for every format.
- FFI value conversion and copy-to-buffer helpers.
- Compatibility output parser/normalizer and harness manifest reader.

Seed corpora with:

- Focused compatibility fixtures.
- Every previously crashing or divergent input.
- External example subsets.
- Boundary-depth and maximum-size inputs.
- Valid snapshots from every supported version/format.

For each target:

1. Run a deterministic smoke in CI.
2. Run at least 24 CPU-hours for the release re-audit.
3. Run with AddressSanitizer where supported.
4. Retain corpus, crash artifacts, exact command, seed, and toolchain.
5. Minimize and convert every finding into a regression test.

Absence of a required fuzz target is a failed gate, not "no findings."

### 12.2 Miri

Run Miri on crates and tests that do not require unsupported FFI operations:

```sh
rustup toolchain install nightly --component miri
cargo +nightly miri setup
cargo +nightly miri test -p ferric-rules-core
cargo +nightly miri test -p ferric-rules-parser
cargo +nightly miri test -p ferric-rules-runtime
cargo +nightly miri test -p ferric-rules-pinned
```

Use deterministic proptest seeds and reduced case counts only when necessary for
runtime. Record every excluded test and why equivalent coverage exists elsewhere.
Zero Miri errors are required.

### 12.3 Rust sanitizers

On supported nightly Linux targets, build tests with:

```sh
RUSTFLAGS="-Zsanitizer=address" \
  cargo +nightly test -Zbuild-std --target x86_64-unknown-linux-gnu \
  --workspace --all-features

RUSTFLAGS="-Zsanitizer=leak" \
  cargo +nightly test -Zbuild-std --target x86_64-unknown-linux-gnu \
  --workspace --all-features
```

Run ThreadSanitizer on pinned-worker and binding concurrency suites where the
toolchain supports all native dependencies. Treat sanitizer-incompatible
dependencies as a coverage gap requiring an alternate tool, not an automatic
waiver.

## 13. Snapshots and untrusted input

Test Bincode, JSON, CBOR, MessagePack, and Postcard independently.

### 13.1 Round-trip equivalence

For each format and representative engine state:

1. Capture canonical facts, rules, templates, globals, focus stack, agenda,
   output, diagnostics, configuration, and next observable behavior.
2. Serialize.
3. Deserialize on the same thread and on a fresh allowed worker/thread.
4. Compare canonical state.
5. Run/step both original and restored engines and compare subsequent behavior.
6. Repeat after reset, partial run, halt, errors, and high-churn operations.

Verify `ExternalAddress` and every other nonserializable value fails before
emitting a misleadingly successful snapshot.

### 13.2 Malformed and hostile data

For every decoder, test:

- Empty, one-byte, truncated-at-every-offset, appended-junk, bit-flipped, and
  wrong-format data.
- Valid envelope with invalid enum, identifier, index, length, or graph
  relationship.
- Extremely large declared lengths, deeply nested values, duplicate map keys,
  NaN payloads, invalid UTF-8, and integer overflow.
- Snapshots from newer/older versions according to the declared compatibility
  policy.
- Repeated failure followed by valid deserialize to detect poisoned global state.

Require explicit limits for input bytes, nesting, collection lengths, facts,
rules, tokens, and allocation. Measure rejection time and peak memory. If
snapshots are not safe for untrusted input, public APIs and documentation must
say so prominently and production entry points must enforce the trust boundary.

No panic, abort, stack overflow, uncontrolled allocation, hang, or partially
returned engine is allowed.

## 14. Performance and correctness benchmarks

Correctness gates run before performance. Never treat faster incorrect output as
a performance result.

Follow `benches/PROTOCOL.md` and `docs/benchmark-policy.md`. Performance numbers
used in decisions or release notes must come from release-profile `cargo bench`,
not `cargo test`, debug mode, or `cargo test --bench`.

### 14.1 Baseline protocol

- Use the same dedicated machine, power mode, OS, compiler, features, and
  background load for before/after.
- Record CPU model, core topology, memory, kernel, governor, thermal state, and
  container settings.
- Run candidate and baseline in alternating order at least three times.
- Retain Criterion medians and confidence intervals.
- Quote actual medians, not estimates inferred from source.

Run:

```sh
cargo bench -p ferric-rules
just bench-thresholds
just scaling-check
just bench-compare
```

Also measure:

- Parse/load latency by source size and nesting.
- First reset/run and steady-state reset/run.
- Assert/retract/modify throughput and tail latency under churn.
- Join width/depth, negative blockers, exists/forall/NCC fanout.
- Agenda sizes and all conflict strategies.
- Serialization/deserialization latency, throughput, and snapshot size.
- Engine create/free and binding create/close.
- Pinned/worker/pool queue latency at 1, 2, 4, and saturation concurrency.
- Peak RSS, retained heap after reset/clear/close, and memory per fact/token/rule.

Every benchmark workload must assert a result checksum, canonical fact count, or
equivalent correctness oracle outside the timed region. Comparative CLIPS
workloads must prove both engines reached the same final state.

### 14.2 Gates

- All current absolute thresholds pass.
- All scaling checks remain in the intended complexity class.
- No claimed workload regresses beyond the declared budget versus the previous
  supported release on like-for-like hardware.
- No monotonic memory growth appears across repeated lifecycle/churn cycles.
- Comparative claims include CLIPS version, environment, and matching correctness
  oracle.

Changing a threshold to make the candidate pass requires a separate reviewed
change with before/after release-profile evidence.

## 15. CI, platform, MSRV, and dependencies

### 15.1 CI parity and platform matrix

Re-run all CI workflows at the immutable candidate. Inspect job logs rather than
only status summaries. Require:

- No skipped job caused by path/event conditions.
- No allowed failure for a declared release surface.
- Artifact uploads succeeded and contain expected files.
- Tests ran against packaged/native artifacts where intended.
- Cache-disabled rerun succeeds.

The current primary CI is Ubuntu-based. Add or execute independent jobs for every
declared OS/architecture. At minimum, a portable production claim should
explicitly decide coverage for:

- Linux x86_64 glibc.
- Linux aarch64 glibc.
- Linux musl targets if distributed.
- macOS arm64 and x86_64 if distributed.
- Windows x86_64 if distributed.

Do not publish an artifact for a platform tested only by cross-compilation unless
the release policy explicitly permits it and a native install smoke exists.

### 15.2 MSRV

Workspace metadata currently declares `rust-version = "1.75"`, while CI pins
Rust 1.93.0. Test both:

```sh
cargo +1.75 check --workspace --locked
cargo +1.75 test --workspace --locked
cargo +1.93.0 test --workspace --locked --all-features
```

If dependencies no longer resolve/build on 1.75 with the committed lockfile,
either remediate or update the declared MSRV as an explicit compatibility
decision before release. Metadata and tested reality must agree.

### 15.3 Dependency and supply-chain review

Run and retain:

```sh
cargo tree --workspace --all-features
cargo audit
cargo deny check
just license-notices-check
```

Also review:

- Rust, Python, npm, and Go lockfiles for unexpected source changes, git/path
  dependencies, yanked packages, and duplicate high-risk versions.
- Known vulnerabilities and maintenance advisories across all ecosystems.
- Licenses against project policy and generated notices.
- Build scripts, proc macros, native binaries, install scripts, and downloaded
  artifacts.
- Checksums/signatures and provenance for reference images and release packages.
- Secret scanning and absence of credentials in package archives.

Generate an SBOM for each shipped artifact or release bundle. Every advisory
exception needs scope, exploitability analysis, owner, expiry, and approval.

## 16. Packaging and installation smoke tests

Build release packages from the source archive in a clean environment.

### 16.1 Rust and CLI

- Run `cargo package --list` and inspect every intended crate.
- Run `cargo package`/`cargo publish --dry-run` in dependency order.
- Extract each `.crate`, build it without workspace-relative files, and run its
  available tests/examples.
- Install the CLI with `cargo install --path crates/ferric-rules-cli --locked` and
  smoke `version`, `check`, `run`, REPL EOF, invalid file, and JSON diagnostics.
- Inspect stripped release binaries, dynamic dependencies, licenses, and version
  metadata.

### 16.2 C artifacts

- Install headers and libraries into a staging prefix.
- Compile C11 and C++ consumers using only that prefix.
- Test static and shared variants if both ship.
- Verify loader paths/rpaths, SONAME/install-name, exported symbols, and debug
  symbol policy.

### 16.3 Python

- Build all declared wheels and any source distribution.
- Inspect tags and bundled libraries.
- Install with `pip --no-index --find-links <wheel-dir>` into fresh environments.
- Import from a directory outside the repository and execute lifecycle,
  template, Unicode, error, and snapshot smokes.

### 16.4 Node

- Run `npm pack`, inspect the file list, and reject source-tree/native paths not
  present in the tarball.
- Install the tarball into a fresh project with network disabled.
- Run sync, worker, pool, type declaration, ESM/CommonJS, close, and snapshot
  smokes on every declared Node/target combination.

### 16.5 Go

- Tag/replace the candidate in a fresh consumer module.
- Download/build with a clean module and build cache.
- Prove the native library/header installation instructions are sufficient.
- Run raw, pinned, coordinator, error, and snapshot smokes.

Pass criteria:

- A consumer needs no undeclared repository checkout or build-time network.
- Package metadata versions/licenses/repository links agree.
- Installation and uninstall/cleanup leave no leaked worker processes or files.
- Artifact digests match those referenced by release metadata.

## 17. Production-shaped validation

These tests use the candidate packages, not `cargo run` from the worktree.

### 17.1 Soak

Run at least one 24-hour soak per production runtime shape:

- Long-lived single engine with repeated assert/run/retract/reset.
- Compile-once/snapshot-many worker pattern.
- Go pinned engine/coordinator at expected concurrency.
- Node worker/pool at expected concurrency.
- Python service lifecycle if Python is a supported server surface.

Use representative rule counts, facts, joins, templates, modules, snapshots, and
request distributions at normal load, peak load, and 2x forecast peak. Monitor:

- Throughput and p50/p95/p99/max latency.
- RSS, heap, native allocation, file descriptors, threads, handles, and queues.
- CPU, context switches, worker restarts, cancellation latency, and timeouts.
- Error/diagnostic counts, output size, agenda depth, fact/token counts.
- Correctness checksum per request and periodic full canonical-state audit.

Pass only if correctness remains exact, resources reach a stable plateau, no
deadlock/starvation occurs, and all declared SLOs hold after warm-up.

### 17.2 Fault injection

Inject, at controlled boundaries:

- Invalid source and snapshots.
- Maximum-size and over-limit inputs.
- Callback exception/panic.
- Worker thread/process crash.
- Cancellation before, during, and immediately after completion.
- Close concurrent with submit/run/serialize.
- File I/O denial, disk full, read-only directory, broken pipe, and missing file.
- Reference/application shutdown while work is queued.
- CPU throttling and memory pressure near declared operating limits.

Verify bounded recovery, stable error types, no use-after-close, no stuck queue,
no orphan worker, and a documented retry/rollback path. Do not simulate native
undefined behavior in a shared production-like host.

### 17.3 Shadow comparison

When migrating from CLIPS or an earlier Ferric release:

- Replay a representative, approved, redacted input sample to both systems.
- Suppress or sandbox external side effects.
- Canonicalize final facts, decisions, diagnostics, and halting behavior.
- Record divergence by stable case ID and feature.
- Review every divergence; sampling averages cannot hide a wrong decision.
- Run long enough to cover rare rules and peak-shaped input.

Define the required sample size and zero/near-zero divergence threshold before
observing results. Any divergence in a safety- or business-critical decision is a
blocking failure unless it is a predeclared intentional difference.

### 17.4 Rollback rehearsal

Prove:

- Previous package/binary can be restored within the rollback objective.
- Snapshot compatibility across rollback direction matches the documented
  promise; otherwise snapshots are regenerated from the source of truth.
- In-flight work drains or is safely retried.
- Metrics and logs identify the deployed candidate digest.

## 18. Documentation and compatibility-claim audit

Review README, compatibility matrix, migration guide, users guide, API docs,
examples, headers, package metadata, and generated declarations against observed
behavior.

Require documentation to state accurately:

- Exactly which CLIPS version/subset is targeted.
- Deffacts load/reset behavior and `initial-fact` order.
- Construct redefinition rules.
- Deffunction `return` and RHS error continuation.
- Pattern and general parser nesting/size limits.
- Supported standard-library functions and intentional differences.
- Conflict-order guarantees and nondeterminism.
- Thread affinity, worker/pool semantics, cancellation, and close behavior.
- FFI pointer ownership, allocator/free pairs, callback threads, and error
  lifetime.
- Snapshot formats, version compatibility, nonserializable values, limits, and
  trust assumptions.
- Supported Rust/Python/Node/Go versions and OS/architecture targets.
- Performance numbers with release-profile command, CLIPS version, hardware,
  date, and correctness oracle.

Run every documented command and example from a fresh install. Verify inlined
examples with:

```sh
just check-examples
just check-examples-sync
```

No "drop-in," "same semantics," "safe for concurrent use," "all platforms," or
similar broad claim may exceed the tested and documented contract.

## 19. Blocking pass/fail gates

| Gate | Pass condition |
|---|---|
| Candidate integrity | Exact SHA, clean checkout, reproducible locked build, complete evidence. |
| Baseline quality | All preflight, feature, docs, examples, release tests pass without retry. |
| CLIPS semantics | Non-vacuous harness self-tests pass; zero unexplained divergence in claimed subset. |
| RETE correctness | All invariants/model tests pass across required operation count and seeds. |
| Parser robustness | No panic/abort/hang/runaway; documented limits produce diagnostics. |
| FFI safety | Independent unsafe review; ABI tests and all memory tools are clean. |
| Python | All declared versions/targets install and pass lifecycle/value/error tests. |
| Node | All declared versions/targets; sync/worker/pool/package tests pass. |
| Go | Declared versions/targets; race/stress/raw/pinned/coordinator tests pass. |
| Fuzz/Miri/sanitizers | Required targets/hours complete; zero unresolved finding. |
| Snapshots | Round trips and hostile-input tests pass within limits for every format. |
| Performance | Correctness oracles pass; thresholds, scaling, resource, and regression budgets pass. |
| CI/platform/MSRV | Candidate CI green; native target matrix and declared MSRV pass. |
| Dependencies | No unaccepted advisory/license/provenance issue; SBOM complete. |
| Packaging | Every shipped artifact installs and smokes in a clean consumer. |
| Production shape | Soak, fault, shadow, and rollback acceptance criteria pass. |
| Documentation | Claims, limits, examples, versions, and ownership contracts match evidence. |

Automatic fail conditions:

- Any unresolved critical/high-severity correctness, memory-safety, data-loss,
  deadlock, or compatibility defect.
- Any panic/abort/stack overflow on input allowed at a public boundary.
- Any unexplained CLIPS divergence inside the claimed compatible subset.
- A vacuous compatibility result, unavailable reference, or silently skipped
  release surface.
- Any sanitizer, Miri, race-detector, or fuzz crash not proven external and
  harmless.
- Any package that was not installed and tested outside the source tree.
- Missing evidence or inability to tie an artifact to the candidate SHA.

Lower-severity accepted risks must include:

```text
issue:
severity:
affected surface:
user impact:
why release is still acceptable:
mitigation/documentation:
owner:
expiry/remediation release:
approvers:
```

## 20. Final evidence review

Before signing:

1. Recompute every artifact digest.
2. Confirm the candidate worktree is still clean.
3. Confirm no audit ran against a later local rebuild or different native addon.
4. Confirm all failures and reruns appear in the logs.
5. Confirm all exceptions are linked and approved.
6. Have the compatibility, safety, and performance reviewers sign their sections.
7. Store the immutable evidence URI and manifest digest in the release record.

## 21. Final decision template

```markdown
# ferric-rules <version> Production-Readiness Decision

Candidate commit:
Candidate source SHA-256:
Audit start/end UTC:
Auditor:
Compatibility reviewer:
Safety reviewer:
Performance reviewer:
Release approver:
Evidence URI:
Evidence manifest SHA-256:

## Declared release surface

- Rust:
- CLI targets:
- C ABI targets/version:
- Python versions/targets:
- Node versions/targets:
- Go versions/targets:
- Snapshot formats/version policy:
- CLIPS reference/version and compatible subset:

## Gate results

| Gate | PASS / FAIL / INCONCLUSIVE | Evidence path | Notes |
|---|---|---|---|
| Candidate integrity | | | |
| Baseline quality | | | |
| CLIPS semantics | | | |
| RETE correctness | | | |
| Parser robustness | | | |
| FFI safety | | | |
| Python | | | |
| Node | | | |
| Go | | | |
| Fuzz/Miri/sanitizers | | | |
| Snapshots | | | |
| Performance | | | |
| CI/platform/MSRV | | | |
| Dependencies | | | |
| Packaging | | | |
| Production-shaped validation | | | |
| Documentation | | | |

## Differential summary

- Fixtures executed:
- Non-vacuous oracle checks:
- Equivalent:
- Intentional documented differences:
- Unexplained divergences:

## Safety summary

- Unsafe blocks reviewed:
- FFI ABI baseline:
- Sanitizer/Miri/fuzz duration and findings:
- Snapshot hostile-input result:

## Performance summary

- Baseline candidate:
- Hardware/environment:
- Criterion regressions/improvements:
- Scaling result:
- Peak resource result:

## Open risks

<One subsection per approved risk using the required risk template.>

## Decision

Decision: PASS / PASS WITH ACCEPTED RISKS / FAIL / INCONCLUSIVE

Rationale:

Rollback trigger and owner:

Auditor signature/date:
Release approver signature/date:
```

The release may proceed only when the signed decision and immutable evidence
manifest agree and every blocking gate is `PASS`.
