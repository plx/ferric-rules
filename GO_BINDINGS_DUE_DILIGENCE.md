# Go Bindings Due Diligence Review

Date: 2026-03-23
Repo: ferric-rules
Scope: `bindings/go` (+ `crates/ferric-ffi` ownership contracts where relevant)

## Executive Summary

The Go bindings are thoughtfully structured and already provide a strong baseline:
- Good package split (`ferric`, `internal/ffi`, `temporal`)
- Functional options are clean and readable
- Coordinator/worker architecture correctly uses `runtime.LockOSThread()` on worker goroutines
- Core tests, race tests, and lint currently pass in normal runs

That said, there are several important issues to address before calling this production-hardened for server usage:

1. `Coordinator.Close()` semantics do not match docs and can return `coordinator is closed` for requests that actually ran to completion.
2. Structured assertion paths likely leak C-allocated `FerricValue` resources (symbol/string) in Go.
3. Direct `Engine` usage is easy to misuse with Go goroutine migration; stress runs expose thread-violation flakes in current tests.
4. Some APIs silently drop errors and return partial/empty data instead of surfacing errors.
5. Coverage is decent at a package level (~69% total), but critical branches and semantics are still under-tested.
6. Observability hooks are minimal (no built-in tracing/metrics/logger integration).
7. A simpler stateful single-engine API is feasible and would materially improve ergonomics/safety.

## Methodology

Static review:
- All files in `bindings/go`
- Relevant ownership and FFI behavior in `crates/ferric-ffi`

Validation commands run:
- `just build-go-ffi` (pass)
- `just test-go` (pass)
- `just test-go-race` (pass; linker warnings on macOS, no race reports)
- `just go-lint` (pass, `0 issues`)
- `cd bindings/go && go test ./... -coverprofile=coverage.out`

Coverage snapshot:
- `github.com/prb/ferric-rules/bindings/go`: 68.0%
- `github.com/prb/ferric-rules/bindings/go/internal/ffi`: 73.6%
- `github.com/prb/ferric-rules/bindings/go/temporal`: 47.6%
- Total: 69.0%

Stress checks run:
- `cd bindings/go && go test ./... -count=30` (intermittent failures observed)
  - Examples: thread-violation close errors, empty relation due swallowed errors

## Findings (Prioritized)

### F1 (High): Close semantics can report failure for successful work

Evidence:
- [coordinator.go](/Users/prb/conductor/workspaces/ferric-rules/cancun/bindings/go/coordinator.go:85)
- [manager.go](/Users/prb/conductor/workspaces/ferric-rules/cancun/bindings/go/manager.go:69)

`Close()` closes `coord.done` before workers drain. In `Manager.Do`, waiting callers select on `coord.done` and can return `errCoordinatorClosed` even if the closure already executed successfully.

Observed repro (local):
- In-flight closure ran (`ran=true`), but `Do` returned `ferric: coordinator is closed`.

Impact:
- Violates the documented "in-flight requests complete" expectation.
- Can cause false negatives and duplicate retries at higher layers.

Mitigation:
- Introduce explicit draining lifecycle:
  - Stop accepting new requests first.
  - Drain queued accepted requests.
  - Then close workers.
- Distinguish "not accepted" vs "accepted but closing" in request state.
- Add deterministic tests for accepted request outcomes during shutdown.

### F2 (High): Potential C resource leak in structured assertion paths

Evidence:
- [engine.go](/Users/prb/conductor/workspaces/ferric-rules/cancun/bindings/go/engine.go:222)
- [engine.go](/Users/prb/conductor/workspaces/ferric-rules/cancun/bindings/go/engine.go:240)
- [ffi.go](/Users/prb/conductor/workspaces/ferric-rules/cancun/bindings/go/internal/ffi/ffi.go:559)
- [ferric.h ownership docs](/Users/prb/conductor/workspaces/ferric-rules/cancun/crates/ferric-ffi/ferric.h:1)
- [ferric_value_symbol docs](/Users/prb/conductor/workspaces/ferric-rules/cancun/crates/ferric-ffi/ferric.h:1144)

`goToFFIValue` creates symbol/string `FerricValue`s with owned heap pointers. After `EngineAssertOrdered` / `EngineAssertTemplate`, these values are not freed in Go.

Rust FFI assertion code converts input values but does not free caller-owned resources after conversion.

Impact:
- Long-running server processes may leak memory proportional to assertion volume with string/symbol-heavy inputs.

Mitigation:
- In `Engine.AssertFact` and `Engine.AssertTemplate`, `defer`/post-call loop over constructed `ffi.Value`s and call `ffi.ValueFree(&vals[i])`.
- Ensure cleanup runs on both success and error paths.
- Add stress tests with many symbol/string assertions and memory-growth guardrails.

### F3 (High): Thread-affinity ergonomics are fragile for direct Engine usage

Evidence:
- [engine.go docs](/Users/prb/conductor/workspaces/ferric-rules/cancun/bindings/go/engine.go:27)
- [engine_test init lock](/Users/prb/conductor/workspaces/ferric-rules/cancun/bindings/go/engine_test.go:10)
- [iterators_test init lock](/Users/prb/conductor/workspaces/ferric-rules/cancun/bindings/go/iterators_test.go:8)

`Engine` requires same physical thread for all calls. Current tests try to lock thread in `init()`, but test functions run in separate goroutines and can migrate.

Observed repro:
- `go test ./... -count=30` intermittently fails with thread-violation close errors.

Impact:
- Direct `Engine` API is easy to misuse in real Go programs.
- Current test reliability for direct engine path is weaker than it appears.

Mitigation:
- For direct-engine tests, lock thread inside each test/operation goroutine that owns engine lifetime.
- Consider a safer wrapper API (see Section "Simpler Single-Engine API") so users avoid manual thread pinning.
- Add explicit tests that verify expected behavior on wrong-thread calls.

### F4 (Medium): Error taxonomy incomplete vs exposed sentinels

Evidence:
- [errors.go sentinels](/Users/prb/conductor/workspaces/ferric-rules/cancun/bindings/go/errors.go:31)
- [errors.go translation](/Users/prb/conductor/workspaces/ferric-rules/cancun/bindings/go/errors.go:129)

`ErrThreadViolation` and `ErrInvalidArgument` are exported, but `errorFromFFI` does not map `ffi.ErrThreadViolation` / `ffi.ErrInvalidArgument` to dedicated types/sentinel-compatible behavior.

Impact:
- `errors.Is(err, ErrThreadViolation)` is not reliable for these cases.
- Callers cannot robustly branch on important operational errors.

Mitigation:
- Add concrete types (or `Is` behavior) for thread violation and invalid argument.
- Add direct `errors.Is` tests for these mappings.

### F5 (Medium): Silent error swallowing can return partial/corrupt data

Evidence:
- [engine.go ordered relation handling](/Users/prb/conductor/workspaces/ferric-rules/cancun/bindings/go/engine.go:661)
- [engine.go template slot-name loop](/Users/prb/conductor/workspaces/ferric-rules/cancun/bindings/go/engine.go:650)
- [iterators.go](/Users/prb/conductor/workspaces/ferric-rules/cancun/bindings/go/iterators.go:11)

Examples:
- `buildFact` silently ignores relation lookup errors and may return empty `Relation`.
- Template slot-name lookup errors break loop and return partial `Slots` without error.
- Iterator APIs silently stop on error with no error channel.

Impact:
- Difficult debugging and potential bad downstream decisions on partial facts.

Mitigation:
- Prefer error-returning variants (`FactIterE`, `RuleIterE`) or expose last-error callback.
- In `buildFact`, return error on relation/slot metadata fetch failures (or provide strict mode).

### F6 (Medium): Wire schema accepts multifield inputs that assertion path rejects

Evidence:
- [wire_conv.go multifield support](/Users/prb/conductor/workspaces/ferric-rules/cancun/bindings/go/wire_conv.go:68)
- [values.go multifield rejection](/Users/prb/conductor/workspaces/ferric-rules/cancun/bindings/go/values.go:44)
- [manager.go assertion path](/Users/prb/conductor/workspaces/ferric-rules/cancun/bindings/go/manager.go:115)

`WireValueMultifield` is supported in conversion, but assertion eventually routes through `goToFFIValue`, which rejects `[]any` for assertion.

Impact:
- Surprising runtime failures for syntactically valid wire payloads.

Mitigation:
- Implement multifield assertion conversion end-to-end, or reject multifield earlier in request validation with clear error text.
- Add explicit test coverage for both accepted and rejected multifield paths.

### F7 (Medium): Dispatch policy contract can panic coordinator

Evidence:
- [coordinator.go pickWorker](/Users/prb/conductor/workspaces/ferric-rules/cancun/bindings/go/coordinator.go:77)

Custom `DispatchPolicy` return index is used without bounds checks. Invalid index can panic the process.

Impact:
- One bad policy implementation can crash production.

Mitigation:
- Clamp or validate index and fallback to round-robin on invalid values.
- Add tests for negative/out-of-range policy outputs.

### F8 (Low): `NewManager` ignores an error path

Evidence:
- [manager.go](/Users/prb/conductor/workspaces/ferric-rules/cancun/bindings/go/manager.go:288)

`mgr, _ := coord.Manager("_default")` ignores error. Practically impossible in current code, but still not ideal.

Mitigation:
- Handle error explicitly; on failure close coordinator and return wrapped error.

## 1. Idiomaticity Assessment (Go)

### What is idiomatic today

- Functional options for engine/coordinator config are idiomatic.
- `context.Context` is used consistently for operations that can block.
- `internal/ffi` boundary is good encapsulation.
- `Manager` as a concurrency-safe handle is a good Go pattern.

### Where idiomaticity can improve

- Public methods that can fail but currently return silent fallback values (`Rules`, `Templates`, `FocusStack`, iterator methods) should have error-aware variants.
- Direct thread-affine `Engine` is inherently non-idiomatic in Go unless wrapped in a pinned worker abstraction.
- `DispatchPolicy` safety contract should be defensive; panics from user policy are avoidable.

## 2. Correctness / Robustness / Safety / Quality

Overall: medium-to-strong baseline with several high-impact edge-case issues.

Strong points:
- Worker thread affinity is correctly enforced in coordinator path.
- Context cancellation behavior is deliberate and well-commented.
- Serialization format handling is clean and tested.

Priority fixes:
- F1 shutdown semantics
- F2 assertion resource ownership
- F3 direct Engine thread-affinity ergonomics
- F4 error mapping completeness

## 3. Test Suite Robustness

### Current strengths

- Good happy-path API coverage across `engine`, `manager`, `coordinator`, `serialization`, `temporal`.
- `-race` test suite passes.

### Gaps in API branch coverage

Notable low/zero-coverage surfaces from `go tool cover -func`:
- `WithDispatchPolicy`: 0%
- `Engine.Focus`, `Engine.FocusStack`: 0%
- `Engine.DiagnosticIter`: 0%
- Several error code mappings and FFI helper branches: 0%
- `temporal.Register`: 0%

### Robustness concerns found

- Repeated test execution (`-count`) reveals intermittent thread-affinity failures in public Go package tests.
- No deterministic tests for shutdown acceptance/drain guarantees.
- No tests for malformed custom dispatch policies.
- No explicit tests for wrong-thread behavior at Go API level.

### Engine-semantics coverage (from Go)

Go tests currently validate a subset of semantics (facts, templates, simple runs).
Still under-covered from Go binding perspective:
- Multi-module/focus-stack behavior
- Rich conflict strategy behavior differences
- Negative/exists/NCC-heavy rule shapes through Go API
- Diagnostics with real non-fatal action errors
- Larger cross-format equivalence properties beyond small fixtures

## 4. Property Testing Opportunities (Rapid + Testify)

Recommended dev dependencies:
- `pgregory.net/rapid`
- `github.com/stretchr/testify`

High-value property suites:

1. Conversion round-trips
- Property: `WireToNativeValue(NativeToWireValue(x)) == x` for supported domains.
- Include nested multifields and mixed scalar types.

2. Fact wire/native conversion stability
- Property: `FactToWire -> wireFactToNative` preserves structure/type fidelity for generated valid facts.

3. Stateless evaluate determinism
- Property: same engine spec + same request => same `RunResult`, fact multiset, and output map.

4. Snapshot equivalence
- Property: engine built from source and engine built from snapshot produce equivalent outcomes for generated fact sets.

5. Coordinator shutdown invariants
- Property: accepted requests either complete with closure error/result, never ambiguous closed-state misreporting.

6. Dispatch policy safety
- Property: any policy output (including malformed) never panics coordinator; invalid outputs fallback safely.

7. Thread-affinity contract checks
- Property-like repeated tests that wrong-thread calls always return thread violation and never mutate state.

## 5. Observability Assessment

Current state:
- Minimal built-in observability at coordinator/manager layer.
- No integrated tracing, metrics, or structured logs.

Given server-oriented usage, add optional hooks via coordinator options:

1. Tracing (OpenTelemetry)
- Spans around dispatch queue wait, engine cold start, assert/run/snapshot phases.
- Attributes: `spec`, `worker_id`, `cold_start`, `rules_fired`, `halt_reason`, `error_kind`.

2. Metrics
- Histograms: enqueue wait, execution time, total evaluate latency.
- Counters: evaluations, cancellations, thread violations, engine instantiations.
- Gauges/updown: in-flight requests, queue depth per worker.

3. Logging (`slog`)
- Optional logger option for significant lifecycle events: engine create, coordinator close, shutdown drain stats, operation failures.

4. Error/diagnostic bridging
- Optional callback to expose engine diagnostics and channel outputs for operational debugging.

## 6. Simpler API for Single-Engine Stateful Use

### Current situation

You already have two paths:
- `Engine`: direct, mutable, full power, but caller must manually pin thread.
- `NewManager`/`StandaloneManager`: safe affinity via worker thread, but ergonomics are closure-based and API intent is mostly stateless `Evaluate`.

### Feasible simpler API

Add a dedicated `PinnedEngine` (or `OwnedEngine`) type:
- Internally runs a single locked worker goroutine with one engine.
- Exposes direct methods (`Load`, `Assert*`, `Run`, `Reset`, `Facts`, etc.) that marshal onto that pinned worker.
- No manual `runtime.LockOSThread()` required by caller.
- Clearly stateful by design.

Sketch:
- `func NewPinnedEngine(opts ...EngineOption) (*PinnedEngine, error)`
- `func (p *PinnedEngine) AssertFact(ctx context.Context, relation string, fields ...any) (uint64, error)`
- `func (p *PinnedEngine) Run(ctx context.Context, limit int) (*RunResult, error)`
- `func (p *PinnedEngine) Do(ctx context.Context, fn func(*Engine) error) error` (escape hatch)

This gives you a simpler, safer bridge for TS-like bindings and stateful server workflows without requiring the full multi-spec coordinator model.

## Suggested Remediation Order

1. Fix shutdown semantics (F1) and add deterministic close/drain tests.
2. Fix structured assertion resource ownership (F2) and add leak-regression tests.
3. Harden direct engine/thread-affinity ergonomics and tests (F3).
4. Complete error taxonomy mappings (F4).
5. Remove silent data-loss paths (`buildFact`, iterators) or add strict/errorful variants (F5).
6. Align multifield wire/assertion behavior (F6).
7. Add defensive dispatch-policy bounds checking (F7).
8. Add observability option surface and baseline telemetry.
9. Introduce `PinnedEngine` stateful API for the single-engine use case.

## Prioritized Implementation Checklist (Issue-Sized)

Legend:
- Priority: `P0` (blocker), `P1` (important), `P2` (follow-up)
- Size: `S` (<= 0.5 day), `M` (1-2 days), `L` (3-5 days)

### P0 (Blockers)

- [ ] `GOB-001` `P0` `L` Coordinator close/drain correctness
  Scope: redesign shutdown so accepted requests complete with their true result, and only non-accepted requests fail closed.
  Files: `bindings/go/coordinator.go`, `bindings/go/manager.go`, `bindings/go/coordinator_test.go`.
  Depends on: none.
  Acceptance criteria: add deterministic tests where in-flight requests complete with non-closed errors; no accepted request returns `errCoordinatorClosed`; docs match behavior.

- [ ] `GOB-002` `P0` `M` Structured assertion ownership cleanup
  Scope: free caller-owned `ffi.Value` resources after `EngineAssertOrdered`/`EngineAssertTemplate` calls on all paths.
  Files: `bindings/go/engine.go`, `bindings/go/values.go`, `bindings/go/engine_test.go`.
  Depends on: none.
  Acceptance criteria: resource cleanup code present for success/error paths; stress test asserts many symbol/string/template facts without unbounded memory growth signal; tests pass.

- [ ] `GOB-003` `P0` `M` Direct-engine thread-affinity test hardening
  Scope: remove false confidence from `init()` thread lock pattern; enforce per-test owner goroutine thread pinning where direct `Engine` is used.
  Files: `bindings/go/engine_test.go`, `bindings/go/iterators_test.go`, `bindings/go/internal/ffi/ffi_test.go`.
  Depends on: none.
  Acceptance criteria: repeated runs (`go test ./... -count=30`) stable; add explicit wrong-thread behavior test(s) validating thread violation contract.

- [ ] `GOB-004` `P0` `S` Error taxonomy completion
  Scope: map `ffi.ErrThreadViolation` and `ffi.ErrInvalidArgument` to sentinel-compatible errors.
  Files: `bindings/go/errors.go`, `bindings/go/engine_test.go` (or dedicated error tests).
  Depends on: none.
  Acceptance criteria: `errors.Is(err, ErrThreadViolation)` and `errors.Is(err, ErrInvalidArgument)` pass in targeted tests.

- [ ] `GOB-005` `P0` `S` Dispatch policy bounds hardening
  Scope: validate/clamp dispatch policy index to avoid panics from malformed policies.
  Files: `bindings/go/coordinator.go`, `bindings/go/coordinator_test.go`.
  Depends on: none.
  Acceptance criteria: malformed policy test (negative/out-of-range) does not panic and routes safely.

### P1 (Important)

- [ ] `GOB-006` `P1` `M` Remove silent fact-building data loss
  Scope: make `buildFact` strict on relation/slot metadata failures (or add strict mode plus compatibility mode).
  Files: `bindings/go/engine.go`, `bindings/go/engine_test.go`.
  Depends on: `GOB-004`.
  Acceptance criteria: no partial facts returned when metadata fetch fails unless explicitly in compatibility mode; tests cover both paths.

- [ ] `GOB-007` `P1` `M` Add error-aware iterator/introspection variants
  Scope: keep existing APIs for compatibility, but add `*E` variants that surface errors (`FactIterE`, `RuleIterE`, etc. or equivalent callback/result type).
  Files: `bindings/go/iterators.go`, `bindings/go/engine.go`, `bindings/go/iterators_test.go`.
  Depends on: `GOB-006`.
  Acceptance criteria: callers can choose strict error reporting path; tests cover iterator error propagation.

- [ ] `GOB-008` `P1` `M` Multifield behavior alignment
  Scope: either implement multifield assertion support end-to-end or reject multifield wire inputs at validation boundary with explicit errors.
  Files: `bindings/go/values.go`, `bindings/go/manager.go`, `bindings/go/wire_conv.go`, tests.
  Depends on: none.
  Acceptance criteria: behavior is consistent and documented; tests cover accepted/rejected multifield cases.

- [ ] `GOB-009` `P1` `M` Shutdown/cancellation stress tests
  Scope: add focused stress tests for close races, enqueue cancellation boundaries, and accepted-request outcomes.
  Files: `bindings/go/coordinator_test.go`, `bindings/go/manager_test.go`.
  Depends on: `GOB-001`.
  Acceptance criteria: deterministic regression tests for the previously observed misreporting and race windows.

- [ ] `GOB-010` `P1` `S` Temporal registration coverage
  Scope: cover `RulesActivity.Register` behavior and naming contract (`ferric.Evaluate.<spec>`).
  Files: `bindings/go/temporal/activity.go`, `bindings/go/temporal/activity_test.go`.
  Depends on: none.
  Acceptance criteria: registration path has direct test coverage and activity naming assertions.

### P2 (Follow-Up / Enhancements)

- [ ] `GOB-011` `P2` `L` Observability baseline (OTel + metrics + slog)
  Scope: coordinator options for tracer/meter/logger hooks with minimal default-noop overhead.
  Files: `bindings/go/coordinator_options.go`, `bindings/go/manager.go`, `bindings/go/coordinator.go`, docs/tests.
  Depends on: `GOB-001`, `GOB-009`.
  Acceptance criteria: spans/metrics/log events emitted for dispatch wait, run latency, cold-start, and error outcomes; docs include setup examples.

- [ ] `GOB-012` `P2` `L` `PinnedEngine` stateful single-engine API
  Scope: add easier stateful API that preserves strict thread affinity internally without user `LockOSThread`.
  Files: new `bindings/go/pinned_engine.go` (+ tests/docs), possible reuse of worker machinery.
  Depends on: `GOB-001`, `GOB-003`.
  Acceptance criteria: direct stateful operations (`Load`, `Assert*`, `Run`, `Reset`, etc.) available via context-aware methods; examples and tests included.

- [ ] `GOB-013` `P2` `M` Property test harness (`rapid` + `testify`)
  Scope: introduce property-based tests for conversions, determinism, snapshot equivalence, and coordinator safety invariants.
  Files: `bindings/go/go.mod`, new `*_rapid_test.go` files.
  Depends on: `GOB-001`, `GOB-008`.
  Acceptance criteria: at least 4 high-value properties implemented and running in CI/local test workflow.

- [ ] `GOB-014` `P2` `S` API docs/examples refresh
  Scope: add package examples for three usage modes: `Coordinator/Manager` stateless, `Do` stateful escape hatch, `PinnedEngine` stateful (when added).
  Files: new example tests/docs in `bindings/go`.
  Depends on: `GOB-012`.
  Acceptance criteria: `go test` examples compile/run and communicate thread-affinity constraints clearly.

- [ ] `GOB-015` `P2` `S` CI resilience gate for affinity-sensitive tests
  Scope: add optional repeated-run test target (for Go bindings) to catch migration-sensitive flakes early.
  Files: `justfile`, CI workflow, `bindings/go` test docs.
  Depends on: `GOB-003`.
  Acceptance criteria: repeat-run target exists (`-count` based); documented as required for Go-binding changes.

## Suggested Milestones

1. Milestone A (stabilize correctness): `GOB-001` to `GOB-005`.
2. Milestone B (harden API behavior): `GOB-006` to `GOB-010`.
3. Milestone C (ergonomics and observability): `GOB-011` to `GOB-015`.

## Suggested Issue Titles

1. `Go bindings: make Coordinator.Close drain accepted work deterministically`
2. `Go bindings: free structured assertion FerricValue resources`
3. `Go bindings: harden direct Engine affinity tests and add wrong-thread coverage`
4. `Go bindings: map thread violation and invalid argument to sentinel errors`
5. `Go bindings: guard invalid DispatchPolicy worker indices`
6. `Go bindings: stop returning partial facts on metadata lookup errors`
7. `Go bindings: add error-aware iterator/introspection API variants`
8. `Go bindings: align multifield wire acceptance with assertion behavior`
9. `Go bindings: add close/cancel concurrency stress regression tests`
10. `Go temporal bindings: cover activity registration naming and path`
11. `Go bindings: add OTel/slog/metrics instrumentation options`
12. `Go bindings: add PinnedEngine stateful single-engine API`
13. `Go bindings: introduce rapid/testify property test suite`
14. `Go bindings: add usage examples for stateless and stateful patterns`
15. `Go bindings: add repeat-run CI target for affinity-sensitive flake detection`

## Final Assessment

The foundation is good and close to production-grade for the coordinated server path, but there are still important correctness and operability issues in shutdown behavior, resource ownership, and thread-affinity ergonomics. Addressing the high-priority items above should materially de-risk both current Go usage and upcoming TypeScript binding design work.
