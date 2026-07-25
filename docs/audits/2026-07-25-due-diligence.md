# Production-readiness due-diligence review

- **Audit date:** 2026-07-25
- **Audited commit:** `dd366eb65a85e2138b8103e719e9fe0b8f52f921`
- **Repository:** `plx/ferric-rules`
- **Reviewer:** Codex, with independent core/runtime, language/compatibility, and
  FFI/bindings review workstreams

## Executive conclusion

`ferric-rules` is a credible and substantial prototype, but it is not ready for
production or work use and should not yet be described as an almost drop-in
CLIPS replacement.

The positive RETE path, Rust ownership model, generational identifiers,
pinned-worker design, test volume, and benchmark infrastructure are all
promising. The audit nevertheless found:

- memory-safety defects in the C and Go boundary;
- reproducible incorrect behavior in fundamental rule constructs;
- lifecycle and concurrency defects across the Node, Go, and Python bindings;
- compatibility testing that currently provides almost no affirmative evidence
  of CLIPS equivalence;
- robustness gaps for deeply nested or otherwise adversarial input;
- incomplete package/release paths; and
- a CI matrix too narrow to support the current portability, compatibility, and
  minimum-Rust-version claims.

The green test suites are meaningful evidence of implementation effort and
internal consistency. Most of them validate Ferric against Ferric's own
expectations, however, rather than comparing observable behavior to CLIPS. They
therefore cannot rule out the semantic defects found in this audit.

| Area | Assessment |
| --- | --- |
| Rust architecture | Strong prototype with significant maintainability hotspots |
| Core correctness | Release-blocking semantic defects |
| CLIPS compatibility | Material known divergences and broadly unproven |
| C ABI and Go | Unsafe to ship until undefined-behavior defects are fixed |
| Node | Direct API is functional; IDs and worker/pool behavior are unsafe for long-lived use |
| Python | Experimentally usable with lifecycle, GIL, version, and distribution limitations |
| Performance | Promising, but not yet reliable evidence of correct work |
| Testing | Broad happy-path coverage; inadequate differential, platform, and hardening coverage |
| Distribution | Rust, Go, npm, and Python release paths are incomplete |

## Scope and method

The review covered the Rust workspace, parser, compiler, RETE network,
evaluator, runtime APIs, CLI, serialization, C ABI, pinned worker, Go binding,
Python binding, Node/N-API binding, TypeScript wrappers, developer tooling,
benchmarks, packaging metadata, and continuous-integration workflows.

The static documentation site and landing page were excluded. API and
compatibility documents were read only where needed to compare public promises
with implementation behavior.

The work combined:

- source and architecture review;
- invariant and ownership tracing;
- targeted regression probes;
- differential execution against the repository's CLIPS reference container;
- ordinary, race-enabled, feature-specific, and scaling test runs;
- release-profile Criterion benchmarks;
- dependency, MSRV, and package-construction checks; and
- direct package-installability inspection.

This is not a formal verification or a substitute for production experience.
Findings described as reproduced were exercised against the audited commit.

## Release-blocking findings

### C and Go memory safety

#### Go methods can use a freed engine

`Engine.Close` frees the Rust allocation and marks the Go object closed, but
does not clear the native handle. Public methods such as `Load` then pass the
same dangling pointer back to Rust without first checking the closed state.

Relevant code:

- [`bindings/go/engine.go`](../../bindings/go/engine.go), `Close` and ordinary
  engine methods
- [`crates/ferric-ffi/src/engine.rs`](../../crates/ferric-ffi/src/engine.rs),
  native engine destruction

Any successful method call after `Close` can construct a Rust reference from
freed storage. This is a use-after-free and not merely an unfriendly API error.

#### Go multifields cross allocator ownership domains

The Go FFI layer allocates the element array of a multifield with `C.malloc`.
The Rust value destructor later reconstructs the pointer as a
`Box<[FerricValue]>` and releases it through Rust's allocator.

Relevant code:

- [`bindings/go/internal/ffi/ffi.go`](../../bindings/go/internal/ffi/ffi.go),
  multifield construction
- [`crates/ferric-ffi/src/types.rs`](../../crates/ferric-ffi/src/types.rs),
  `FerricValue` destruction

`Box::from_raw` requires an allocation produced with the corresponding Rust
allocator and layout. C allocator compatibility is not guaranteed, especially
across targets and link modes.

#### Caller-populated C values can create invalid Rust enum values

`FerricValue.value_type` is a public `#[repr(C)]` Rust enum. C callers can write
any integer to the field. Reading an integer outside the enum's valid
discriminants is undefined behavior in Rust before a `match` can reject it.

Relevant code:

- [`crates/ferric-ffi/src/types.rs`](../../crates/ferric-ffi/src/types.rs),
  `FerricValueType`, `FerricValue`, conversion, and destruction
- [`crates/ferric-ffi/src/engine.rs`](../../crates/ferric-ffi/src/engine.rs),
  structured assertions

The ABI should expose a fixed-width integer tag and perform checked conversion
before constructing or matching a Rust enum.

#### Cross-thread diagnostic access is unsynchronized

The C header deliberately exempts last-error accessors from the normal
thread-affinity requirement. The implementation stores diagnostic strings in a
plain mutable option and cached C strings in a `RefCell`, without a mutex.
Diagnostics can therefore race with engine mutation or with another diagnostic
read.

Relevant code:

- [`crates/ferric-ffi/src/engine.rs`](../../crates/ferric-ffi/src/engine.rs),
  `FerricEngine` state and last-error functions
- [`crates/ferric-ffi/src/error.rs`](../../crates/ferric-ffi/src/error.rs),
  error-state implementation
- [`crates/ferric-ffi/ferric.h`](../../crates/ferric-ffi/ferric.h),
  threading contract

This can cause data races, `RefCell` panics, and invalid Rust aliasing. The state
must be synchronized or the documented cross-thread exception must be removed.

#### FFI panics abort the embedding process

The FFI release profiles use `panic = "abort"`. A latent Rust panic therefore
terminates the host rather than being translated to an error. Fail-stop behavior
may be an intentional final containment policy, but it increases the severity
of all unchecked invariants at the embedding boundary and must be documented
and tested as such.

### Fundamental rule semantics

#### A leading simple `not` conditional element never activates

Initial-token injection handles empty rules and leading NCC paths, but does not
seed ordinary negative nodes. A rule beginning with a simple `not` therefore
has no left token and can never form its otherwise-valid activation.

Relevant code:

- [`crates/ferric-runtime/src/loader.rs`](../../crates/ferric-runtime/src/loader.rs),
  terminal initial-token propagation
- [`crates/ferric-core/src/rete.rs`](../../crates/ferric-core/src/rete.rs),
  negative-node activation

The audit probe produced zero Ferric activations where CLIPS produced one.

#### `exists` can produce duplicate activations

Existential CEs are lowered through double-negation or regular-join paths that
do not consistently implement existence as a single boolean contribution. A
representative rule produced two Ferric activations for two matching facts;
CLIPS produced one.

Relevant code:

- [`crates/ferric-runtime/src/loader.rs`](../../crates/ferric-runtime/src/loader.rs),
  existential lowering and beta compilation
- [`crates/ferric-core/src/rete.rs`](../../crates/ferric-core/src/rete.rs),
  existential memory propagation

#### `test` CEs are evaluated at firing time

The compiler records `test` CEs for later evaluation rather than filtering
matches as the activation is formed. An activation can consequently become
eligible because an unrelated higher-salience rule changes a global after that
activation was placed on the agenda.

Relevant code:

- [`crates/ferric-runtime/src/loader.rs`](../../crates/ferric-runtime/src/loader.rs),
  test-CE compilation
- [`crates/ferric-runtime/src/actions.rs`](../../crates/ferric-runtime/src/actions.rs),
  firing-time predicate evaluation
- [`crates/ferric-runtime/src/engine.rs`](../../crates/ferric-runtime/src/engine.rs),
  step/run reporting

A differential probe with a false global guard fired only the higher-salience
opener in CLIPS, but fired both rules in Ferric.

#### Rules added after facts are not backfilled

Newly compiled alpha and beta paths start empty. Existing facts and tokens are
not replayed through the new network, so a rule loaded after a matching fact
does not activate. CLIPS activates the equivalent late-loaded rule.

Relevant code:

- [`crates/ferric-core/src/compiler.rs`](../../crates/ferric-core/src/compiler.rs),
  network construction
- [`crates/ferric-core/src/beta.rs`](../../crates/ferric-core/src/beta.rs),
  beta request/index assumptions

The API must either support dynamic loading correctly or reject it explicitly
once working memory is populated.

#### Failed RHS compilation leaves a zombie terminal

The loader mutates the LHS network before compiling the RHS and registering
complete rule metadata. If RHS compilation fails, the terminal can remain in
the network without corresponding rule metadata. Later facts create an agenda
entry that cannot fire or be removed normally.

Relevant code:

- [`crates/ferric-runtime/src/loader.rs`](../../crates/ferric-runtime/src/loader.rs),
  rule compilation sequence
- [`crates/ferric-runtime/src/engine.rs`](../../crates/ferric-runtime/src/engine.rs),
  agenda/focus processing

This is one instance of the broader need for staged, transactional loading.

#### Fact-duplication defaults and return values differ from CLIPS

Ferric permits duplicate facts by default. CLIPS defaults fact duplication to
false. Ferric's `set-fact-duplication` also returns the newly assigned value,
where CLIPS returns the previous value.

Relevant code:

- [`crates/ferric-core/src/fact.rs`](../../crates/ferric-core/src/fact.rs),
  fact insertion policy
- [`crates/ferric-runtime/src/engine.rs`](../../crates/ferric-runtime/src/engine.rs),
  assertion
- [`crates/ferric-runtime/src/evaluator.rs`](../../crates/ferric-runtime/src/evaluator.rs),
  builtin behavior

#### Conflict strategies do not match CLIPS

Activation timestamps and recency vectors do not encode the ordering required
by CLIPS depth, breadth, and LEX strategies. Representative probes produced the
reverse order from CLIPS.

Relevant code:

- [`crates/ferric-core/src/rete.rs`](../../crates/ferric-core/src/rete.rs),
  activation construction and recency
- [`crates/ferric-core/src/agenda.rs`](../../crates/ferric-core/src/agenda.rs),
  conflict ordering

Each public conflict strategy requires a differential conformance suite,
including ties, salience, refraction, assertion order, and multi-pattern
recency.

## Loading, control flow, and module behavior

### Loading is not atomic

Symbols, templates, rules, network nodes, globals, and facts can be committed
incrementally before a later form fails. Correcting only the zombie-terminal
case would leave other partial-state failures. A loader transaction should
stage semantic state and RETE changes, validate the complete unit, and commit
once.

### `deffacts` are asserted during load

Ferric asserts `deffacts` immediately while loading. CLIPS stores the construct
and asserts it on reset. This creates facts earlier than expected and changes
the ordering between the initial fact and user deffacts.

Relevant code:

- [`crates/ferric-runtime/src/loader.rs`](../../crates/ferric-runtime/src/loader.rs),
  deffacts loading
- [`crates/ferric-runtime/src/engine.rs`](../../crates/ferric-runtime/src/engine.rs),
  reset order

### Redefinition and undefinition retain stale state

Redefining a rule adds another network instead of replacing the original.
Redefining a template can create distinct template identifiers that resolve
ambiguously. `undefrule` disables a rule but does not reclaim its network,
tokens, and memories. Repeated dynamic mutation therefore changes behavior and
causes retained-state growth.

### `(return ...)` does not return from a callable

The return builtin evaluates to its argument but does not unwind the deffunction
body. A function containing `(return 1)` followed by `2` returns `2` in Ferric
and `1` in CLIPS.

Relevant code:

- [`crates/ferric-runtime/src/evaluator.rs`](../../crates/ferric-runtime/src/evaluator.rs),
  callable-body execution and the return builtin

### RHS errors do not stop the current action sequence

After an RHS evaluation error Ferric emits a diagnostic and executes later
actions. CLIPS stops the current RHS. This is especially dangerous when later
actions have external side effects.

Relevant code:

- [`crates/ferric-runtime/src/actions.rs`](../../crates/ferric-runtime/src/actions.rs),
  action-sequence evaluation

### Focus/module semantics diverge

Observed differences include:

- no explicit export being treated like `?ALL`;
- focus changes being deferred until the full RHS completes;
- the final MAIN focus being retained rather than drained; and
- rule visibility expectations encoded by tests that differ from CLIPS.

These require a single module/focus conformance workstream rather than isolated
ordering patches.

## Binding correctness and lifecycle

### Node fact IDs lose precision

Versioned `u64` fact identifiers are converted to JavaScript `number`, while
input rejects values above `2^53 - 1`. After 1,048,577 assert/retract reuses, the
addon returned `9007203549708288` and then rejected that same ID.

Relevant code:

- [`crates/ferric-napi/src/fact.rs`](../../crates/ferric-napi/src/fact.rs)
- [`crates/ferric-napi/src/engine.rs`](../../crates/ferric-napi/src/engine.rs)

Fact IDs must be lossless JavaScript `bigint`, strings, or opaque objects.

### Batched Go and Node runs continue past `(halt)`

The TypeScript worker, pool worker, and cancelable Go loop call a fresh native
`run` for every chunk. `Engine::run` clears the halt flag and run diagnostics.
The next chunk therefore resumes as though no halt occurred.

Relevant code:

- [`packages/ferric/src/worker.ts`](../../packages/ferric/src/worker.ts)
- [`packages/ferric/src/pool-worker.ts`](../../packages/ferric/src/pool-worker.ts)
- [`bindings/go/engine.go`](../../bindings/go/engine.go)
- [`crates/ferric-runtime/src/engine.rs`](../../crates/ferric-runtime/src/engine.rs)

A 101-rule probe with a halt at rule 100 stopped correctly in the direct native
API and fired rule 101 through the batched wrappers. The Rust pinned worker
already demonstrates the intended `continue_run` and `is_halted` behavior:
[`crates/ferric-pinned/src/worker.rs`](../../crates/ferric-pinned/src/worker.rs).

### `EnginePool.do` is not a transaction

The pool chooses a worker and returns a proxy, but each proxy method is
independently queued. Another callback can use the same engine between the first
callback's reset/assert/run steps.

Relevant code:

- [`packages/ferric/src/engine-pool.ts`](../../packages/ferric/src/engine-pool.ts)

A one-worker reproduction interleaved two callbacks and exposed the second
callback's fact to the first. The implementation must lease a slot for the
callback or encode the whole operation as one worker request.

### Go option presence is inferred incorrectly

Go's functional options lack explicit “was set” fields and use zero-valued enums
as sentinels. This makes valid choices such as ASCII encoding indistinguishable
from “unset,” ignores an explicit depth of 256, and can apply unrelated
zero-valued settings when any other option requests native configuration.

Relevant code:

- [`bindings/go/engine_options.go`](../../bindings/go/engine_options.go)
- [`bindings/go/engine.go`](../../bindings/go/engine.go)

### Go reports stale error text

The Go wrapper always prefers the per-engine error, while several FFI read APIs
set only the global channel. After a parse error, a missing-fact query returned
the earlier parse diagnostic rather than fact-not-found.

Relevant code:

- [`bindings/go/errors.go`](../../bindings/go/errors.go)
- [`crates/ferric-ffi/src/engine.rs`](../../crates/ferric-ffi/src/engine.rs)

### Python can retain unreachable foreign-thread engines

Python engines are stored in creator-thread TLS. `Drop` intentionally does
nothing on another thread, so clearing the last Python reference from a worker
thread leaves the native engine alive until the creator thread exits.

Relevant code:

- [`crates/ferric-python/src/engine.rs`](../../crates/ferric-python/src/engine.rs)
- [`crates/ferric-python/tests/test_threading.py`](../../crates/ferric-python/tests/test_threading.py)

The existing test checks only that no panic or stderr is produced and does not
check reclamation.

### Other binding risks

Additional independently actionable findings include:

- Go `PinnedEngine.Halt` queues behind the active run and cannot interrupt it;
- Go user callback panics occur on internal goroutines and can terminate the
  process;
- Node `EngineHandle.create` leaks a worker if initialization fails;
- a Node pool worker failure can leave its slot permanently wedged;
- pool thread counts accept `NaN`, fractions, and infinity;
- queued abort listeners survive successful completion;
- synchronous `postMessage` failure leaves bookkeeping stale;
- concurrent `close()` calls return before the first cleanup completes;
- embedded NULs are silently truncated or converted to empty strings;
- C output-pointer caching is per thread/channel rather than per engine;
- Python holds the GIL during long engine work and file I/O;
- Python lacks maximum call-depth configuration;
- error classes and plain-string value semantics vary by binding;
- N-API run counts are limited to `u32`;
- Go snapshot options are described as applied but ignored;
- Go snapshots above 2 GiB truncate their length; and
- the C ABI has no exported ABI version or capability query.

Each item is tracked separately in the remediation issue hierarchy created from
this audit.

## Robustness and untrusted input

### Recursive parsing can abort the process

The S-expression parser recursively descends nested lists. A 50,000-level input
aborted the release CLI with stack overflow. Nested RHS arithmetic failed at a
much lower depth.

Relevant code:

- [`crates/ferric-parser/src/sexpr.rs`](../../crates/ferric-parser/src/sexpr.rs)

The parser needs an explicit nesting budget or an iterative representation, and
property/fuzz tests must exercise depths well beyond the current generator.

### `loop-for-count` has no action-level iteration budget

The action implementation of `while` has a maximum-iteration guard, but
`loop-for-count` does not. A range through `i64::MAX` ran until externally
killed. Duplicate evaluator/action implementations already disagree about the
available limits.

Relevant code:

- [`crates/ferric-runtime/src/actions.rs`](../../crates/ferric-runtime/src/actions.rs)
- [`crates/ferric-runtime/src/evaluator.rs`](../../crates/ferric-runtime/src/evaluator.rs)

### Snapshot decoding is effectively trusted-only

Snapshot deserialization has no explicit byte, collection, or nesting budgets
and little post-decode invariant validation.

Relevant code:

- [`crates/ferric-runtime/src/serialization.rs`](../../crates/ferric-runtime/src/serialization.rs)

Until hardened, snapshots must be described and treated as trusted cache data,
not as a general interchange or remotely supplied format.

### API provenance and arity are not consistently enforced

The Rust `assert_template` API silently zips mismatched slot/value slices.
`assert(Fact)` can accept symbol/template identifiers originating in another
engine. Both cases should fail explicitly before mutating working memory.

## CLIPS compatibility evidence

The existing compatibility workflow is not yet suitable for a compatibility
claim.

The corpus contained 1,206 fixtures. The scanner preclassified 548 and selected
658 for execution. After manually connecting generated harnesses and correcting
the temporary-directory mismatch with the Docker reference runner, the result
was:

| Outcome | Count |
| --- | ---: |
| Classified equivalent | 76 |
| Divergent | 123 |
| Runtime-incompatible | 459 |

All 76 “equivalent” cases had empty output. There were no matching non-empty
results.

These counts are not a compatibility percentage because the corpus includes
features Ferric intentionally does not support. They do establish that the
current workflow produced no affirmative non-vacuous evidence of equivalent
observable behavior.

The tooling problems are:

- the scanner detects library-style fixtures but does not attach generated
  harnesses to manifest entries;
- temporary harness files normally live outside the repository mounted into
  the CLIPS container and are rejected;
- a generated verifier rule can be placed in a non-MAIN module and never run;
- empty-equals-empty output is accepted without proof that the verifier ran;
- bracketed CLIPS diagnostics are broadly classified as load errors, masking
  runtime differences;
- scanner regexes recognize commands inside strings; and
- CI scans and runs without generating the available harnesses.

Relevant code:

- [`tools/ferric-tools/src/ferric_tools/compat/scan.py`](../../tools/ferric-tools/src/ferric_tools/compat/scan.py)
- [`tools/ferric-tools/src/ferric_tools/bat/harness.py`](../../tools/ferric-tools/src/ferric_tools/bat/harness.py)
- [`tools/ferric-tools/src/ferric_tools/compat/run.py`](../../tools/ferric-tools/src/ferric_tools/compat/run.py)
- [`.github/workflows/compat-standalone.yml`](../../.github/workflows/compat-standalone.yml)

The `clips_compat.rs` suite runs only Ferric and compares it with hand-written
expectations. It is useful regression coverage, but it is not differential
evidence.

## Architecture and maintainability

### Strengths

- Clear crate separation among core types, parser, runtime, facade, CLI, FFI,
  pinned execution, and language bindings.
- Unsafe Rust is largely confined to explicit interoperability boundaries.
- Generational IDs reduce accidental stale-identifier reuse.
- Positive equality joins are indexed.
- Reverse token indices support iterative retraction cascades.
- Negative, NCC, and existential state have extensive property and invariant
  tests.
- Native engine thread affinity is explicit.
- The Rust pinned layer has bounded FIFO admission, capacity waits,
  cancellation tokens, out-of-band halt, correct continuation behavior,
  reentrant-call protection, shutdown draining, and synchronized diagnostics.
- Test and benchmark coverage is unusually broad for a prototype.

### Maintainability risks

- `evaluator.rs` exceeds 10,000 lines; loader, compiler stages, RETE, and action
  execution are also large.
- Expression and action semantics are duplicated. Inconsistent loop limits and
  binding run behavior demonstrate that the copies have already drifted.
- Parsing, semantic validation, network mutation, and registration are too
  intertwined for reliable rollback.
- Several negative/NCC/exists cross-memory debug invariants remain TODOs.
- Dynamic undefinition/redefinition retains network state indefinitely.

### Scaling hotspots

Source inspection found paths that can dominate real workloads:

- retraction scans negative, NCC, or exists memories for each token;
- existential right activation scans all left parents;
- negative activation scans parent tokens;
- module-filtered agenda popping scans the global agenda and can become
  quadratic;
- binding sets are cloned at successive beta levels;
- adding a beta child clones child lists;
- OR expansion has no Cartesian-product budget;
- propagation remains recursive; and
- alpha sharing is based on complete paths rather than shared prefixes.

These are performance risks, not demonstrated regressions. They should be
profiled only after correctness oracles ensure the engine is doing equivalent
work.

## Performance evidence

All performance numbers below came from release-profile `cargo bench` runs with
Criterion on Darwin arm64, as required by repository policy. They are a
current-machine smoke sample, not before/after claims.

| Benchmark | Approximate median |
| --- | ---: |
| Engine creation | 289 ns |
| Simple load and run | 6.97 µs |
| Representative join | 2.72 µs |
| Simplified Waltz, 100 items | 0.479 ms |
| Simplified Waltz, 1,000 items | 4.73 ms |
| Simplified Manners, 512 items | 1.08 ms |

All five release-mode `just scaling-check` tests passed.

The numbers are encouraging but not production evidence:

- Waltz and Manners are explicitly simplified workloads;
- benchmark functions do not assert the expected activation count or final
  result, so skipped or incorrect work can appear faster;
- CI thresholds are four to five orders of magnitude above current local
  medians;
- scaling checks are not CI-gating; and
- no production-shaped workload or cross-engine equivalent-work comparison
  exists.

Correct result assertions must become part of each benchmark setup before
performance numbers support product claims.

## CI, dependencies, MSRV, and distribution

### Successful checks

- `just check`
- `just go-full`, including `go test -race`
- `just napi-full`
- Python binding tests: 221 passed, 3 skipped on CPython 3.12
- all five `just scaling-check` tests
- release Criterion smoke benchmarks for engine, Waltz, and Manners

The Rust core and runtime package suites also passed independently, including
hundreds of unit and integration tests.

### Negative checks and gaps

- `just check-tracing` failed clippy in the tracing configuration.
- `cargo audit` reported two PyO3 advisories and a dev-only
  `crossbeam-epoch` advisory. Direct use of the affected PyO3 APIs was not
  found, but the dependency remains unresolved.
- `bincode` 1.3.3 is unmaintained.
- npm reports one low-severity, development-only esbuild advisory.
- The workspace declares Rust 1.75, but the current dependency graph requires
  newer toolchains for the CLI and serialization feature sets.
- CI exercises only a current Rust toolchain on Ubuntu, Python 3.12, Node 22,
  and the Go version in `go.mod`.
- There is no macOS, Windows, musl, ARM, MSRV, Miri, sanitizer, fuzz,
  all-features, dependency-audit, scaling, or tracing matrix.
- Compatibility and performance comparison jobs are report-only.

### Packages are not release-ready

- `cargo package -p ferric` fails because public path dependencies do not
  specify versions.
- The Go module expects a host-specific static archive under
  `internal/ffi/lib`, but no archive is distributed.
- The npm package advertises a `native/` directory, but `npm pack` contains no
  native binaries and no release workflow populates it.
- The native Node loader covers only a limited target set and has no musl or
  Linux arm64 distribution wiring.
- Python does not use abi3 or test a supported-interpreter/platform wheel
  matrix. PyO3 0.23.5 failed to build on CPython 3.14 while package metadata
  declares no upper bound.
- The C ABI has no version/capability function, and header generation writes
  into the crate source tree during Cargo builds.

## Current adoption guidance

For personal experimentation, use only the Rust facade, pin the audited/fixed
commit, supply trusted rule input, and deliberately constrain the language
subset:

- load all rules before asserting facts;
- prefer simple positive patterns and equality joins;
- avoid leading negation, `exists`, `test` CEs, modules/focus, redefinition,
  dynamic loading, and complex deffunction control flow;
- do not accept externally supplied snapshots;
- run every application rule set against CLIPS and require matching non-empty
  facts, output, and firing order; and
- use process isolation where a hang, stack overflow, or abort would be costly.

Do not deploy the C ABI or Go binding before the memory-safety findings are
resolved. Do not rely on the Node worker/pool for stateful transactions or
halt-sensitive execution. Python should be limited to controlled experiments on
the creator thread with a pinned supported interpreter.

## Minimum production-readiness gates

1. Repair all C/Go undefined behavior and use-after-free paths, then add
   sanitizer-backed boundary tests.
2. Fix the core semantic blockers and make loading transactional.
3. Replace vacuous compatibility checks with a non-empty differential oracle.
4. Consolidate batched execution on the pinned worker's continuation model.
5. Add parser, loop, network-expansion, and snapshot resource budgets.
6. Add fuzzing, Miri/sanitizers, platform matrices, MSRV checks, dependency
   auditing, tracing, and scaling checks to CI.
7. Produce and independently install-test self-contained Rust, wheel, npm, Go,
   and C artifacts.
8. Validate a production-shaped workload in dual-run/shadow mode against CLIPS
   before trusting decisions or side effects.
9. Execute the independent post-remediation audit in
   [`production-readiness-reaudit.md`](production-readiness-reaudit.md) and
   retain its complete evidence bundle.

The GitHub remediation hierarchy created alongside this report divides these
gates into independently implementable issues with explicit priorities,
components, risks, validation requirements, and native dependency links.
