# TypeScript Binding Conformance Matrix

Date: 2026-04-11
Updated: 2026-08-09 (FR-NODE-004 failed EngineHandle creation cleanup)

Companion documents:
- [TypeScript Binding Architecture (Revised)](/Users/prb/conductor/workspaces/ferric-rules/santo-domingo/docs/typescript-binding-architecture.md)
- [TypeScript Binding Normative Contract (Revised)](/Users/prb/conductor/workspaces/ferric-rules/santo-domingo/docs/typescript-binding-normative-contract.md)
- [TypeScript Binding Test Specification (Revised)](/Users/prb/conductor/workspaces/ferric-rules/santo-domingo/docs/typescript-binding-test-spec.md)
- [TypeScript Binding Spec Post-Mortem](/Users/prb/conductor/workspaces/ferric-rules/santo-domingo/docs/typescript-binding-spec-postmortem.md)

## Purpose
This matrix converts the API spec into executable conformance requirements.

Each item is:
- Normative (`MUST`)
- Testable (type-level and/or runtime)
- Traceable (spec reference + remediation linkage)

## Status Legend
- `PASS`: Verified conformant by the current conformance suite.
- `FAIL`: Verified non-conformant in the 2026-04-11 audit.
- `UNKNOWN`: Not yet verified by direct probe.

## Normative Clarifications (Resolved Ambiguities)

| Decision ID | Clarification |
|---|---|
| `N-01` | `Engine.run(limit)` and `EngineHandle.run({limit})` interpret `limit` as: omitted/`undefined` => unlimited, `0` => zero firings, positive integer => maximum firings. |
| `N-02` | `EvaluateRequest.limit` keeps documented convenience behavior: `0` or omitted => unlimited. |
| `N-03` | `EnginePool.do(..., { signal })` must reject with `AbortError` if aborted before completion. In-flight `run` operations must use the same logical-run batching and out-of-band host-cancellation mechanism as `EngineHandle.run`. |
| `N-04` | `EnginePool.close()` must stop accepting new requests and wait for already-dispatched requests to settle before worker teardown. |
| `N-05` | Worker symbol wire format is canonicalized as `{ __type: "FerricSymbol", value: string }` at the TS layer. |
| `N-06` | Public library API exports concrete `Engine` and `FerricSymbol` classes (not optional exports). |
| `N-07` | Fact IDs have one canonical output representation, `FactId = bigint`; ID-taking APIs additionally accept only legacy safe-integer `number` values through `FactIdInput`. This is separate from CLIPS integer values and run counts/limits. |
| `N-08` | A worker-backed run starts one fresh logical run and uses continuation for later chunks. Absent host cancellation it matches synchronous fired count, halt reason, halted state, agenda, and diagnostics, including exact batch-boundary halts. Caller-limit exhaustion has synchronous precedence over a pending boundary halt. |
| `N-09` | Host abort is an out-of-band worker outcome. Existing APIs retain their partial `HaltRequested` projection, but the worker does not call native `halt()` merely to represent host cancellation; every later public run starts fresh. |
| `N-10` | `EnginePool.do` acquires a FIFO, slot-wide exclusive lease before callback invocation. The callback's proxy serializes calls in invocation order. Normal settlement is the pool's registered reaction to the returned Promise: callback-pre-registered reactions remain within the lease, accepted calls drain before release, and the proxy is invalid before `do` delivers the callback outcome. The lease does not roll back state. Same-pool `do`/`evaluate`/`close` reentry rejects, other-pool calls remain allowed, and `close` waits for an admitted callback. Abort-driven invalidation and accepted-operation semantics remain assigned to FR-NODE-009. |

## Release Gates

A release is conformant only if all are true:
1. All `FAIL` items are remediated to `PASS`.
2. No `UNKNOWN` remains in sections A-E.
3. Type conformance examples compile under strict mode.
4. Runtime conformance suite runs non-zero tests.

## Conformance Matrix

### A) Public API and Type Surface

| ID | Requirement (`MUST`) | Validation | Spec Ref | Related Remediation | Status |
|---|---|---|---|---|---|
| `A-001` | Public export `Engine` is a concrete class value, not `undefined | class`. | `tsc --strict` on `new Engine()` from package entrypoint. | API: Engine class | TSB-002 | PASS |
| `A-002` | Public export `FerricSymbol` is a concrete class value. | `tsc --strict` on `new FerricSymbol("x")`. | Value types | TSB-002 | PASS |
| `A-003` | `ClipsValue` includes `FerricSymbol` in public API types. | Type assertion: `const v: ClipsValue = new FerricSymbol("x")`. | Value types | TSB-002 | PASS |
| `A-004` | Public enums are regular TS enums in the package-facing API (no `const enum` in public `dist/index.d.ts` surface). | Inspect generated public d.ts and compile consumer sample. | Implementation notes (enum guidance) | TSB-002 | PASS |
| `A-005` | `Engine` supports `[Symbol.dispose](): void` for `using`. | Runtime check + `using` integration test on supported Node. | Engine lifecycle + examples | TSB-006 | PASS |
| `A-006` | `EngineHandle` and `EnginePool` support `[Symbol.asyncDispose](): Promise<void>`. | Runtime `await using` test. | EngineHandle/Pool lifecycle | TSB-006 | PASS |
| `A-007` | Public declarations export `FactId = bigint` and `FactIdInput = FactId \| number`; all fact-ID producer, snapshot, and consumer signatures use them consistently. | Strict type assertions against sync, handle, and pool surfaces. | Fact identifier representation | FR-NODE-001 | PASS |

### B) Value Conversion and Fact Shape

| ID | Requirement (`MUST`) | Validation | Spec Ref | Related Remediation | Status |
|---|---|---|---|---|---|
| `B-001` | JS `FerricSymbol` input works in sync `Engine.assertFact` / `assertTemplate`. | Runtime assertion + rule match test. | Value conversion JS->CLIPS | TSB-001 | PASS |
| `B-002` | JS `FerricSymbol` input works in `EngineHandle` and `EnginePool` operations. | Runtime assertion via worker-backed APIs. | Worker serialization + value conversion | TSB-001 | PASS |
| `B-003` | CLIPS symbol values returned via sync `Engine` are `FerricSymbol` instances. | Assert `instanceof FerricSymbol` on fact/global output. | Value conversion CLIPS->JS | TSB-001 | PASS |
| `B-004` | CLIPS symbol values returned via worker-backed APIs are reconstructed as `FerricSymbol` values (not `{}` / untyped objects). | Assert shape/class for `facts/getFact/getGlobal/evaluate`. | Worker serialization/reconstruction | TSB-001 | PASS |
| `B-005` | `string` maps to CLIPS string, not symbol. | Rule discrimination test (`"red"` vs `red`). | Value types note | TSB-001 | PASS |
| `B-006` | `boolean` maps to CLIPS symbols `TRUE/FALSE`. | Assert facts and inspect returned symbol values. | JS->CLIPS table | TSB-001 | PASS |
| `B-007` | Integers in safe range return JS `number`; outside safe range return `bigint`. | Boundary tests around `2^53-1`. | Integer representation section | TSB-001 | PASS |
| `B-008` | `assertString` returns all asserted fact IDs. | Assert multi-fact string and verify length/IDs. | Engine API | TSB-001 | PASS |
| `B-009` | `Fact` shape conforms: ordered facts have `relation+fields`, template facts have `templateName+fields` and slot map when applicable. | Snapshot structural assertions. | Result types (`Fact`) | TSB-001 | PASS |
| `B-010` | Sync `Engine` emits every fact ID as `bigint` and round-trips IDs below, at, and above `2^53-1` through every accepting API without loss. | Native conversion boundaries plus high-generation assert/get/retract subprocess. | Fact identifier representation + N-07 | FR-NODE-001 | PASS |
| `B-011` | Every sync fact-ID consumer accepts `bigint` and legacy safe numbers, while rejecting unsafe numbers, negative/out-of-range bigints, and other types with targeted errors. | Input-kind and boundary matrix for `retract`, `getFact`, and `getFactSlot`. | Fact identifier representation + N-07 | FR-NODE-001 | PASS |

### C) Error Mapping and Hierarchy

| ID | Requirement (`MUST`) | Validation | Spec Ref | Related Remediation | Status |
|---|---|---|---|---|---|
| `C-001` | Parse failures surface as `FerricParseError` (sync and worker-backed). | Trigger parse error and assert `instanceof`, `.name`, `.code`. | Error hierarchy | TSB-003 | PASS |
| `C-002` | Compile failures surface as `FerricCompileError`. | Trigger compile error and assert class/code. | Error hierarchy | TSB-003 | PASS |
| `C-003` | Runtime/fact/template/slot/module/encoding/serialization failures map to documented subclasses. | One targeted case per class. | Error hierarchy | TSB-003 | PASS |
| `C-004` | Worker response error payload contains stable `name`, `code`, `message` used for reconstruction. | Inspect worker responses via harness. | Worker protocol | TSB-003 | PASS |
| `C-005` | Unknown worker errors degrade to base `FerricError` with preserved code/message. | Inject synthetic unknown error payload. | Error hierarchy | TSB-003 | PASS |

### D) EngineHandle and Worker Protocol

| ID | Requirement (`MUST`) | Validation | Spec Ref | Related Remediation | Status |
|---|---|---|---|---|---|
| `D-001` | `EngineHandle.create({source})` performs load + reset. | Rules available immediately after creation. | EngineHandleOptions | TSB-008 | PASS |
| `D-002` | `EngineHandle.create({snapshot})` restores from snapshot. | Snapshot round-trip and rule presence check. | EngineHandleOptions | TSB-008 | PASS |
| `D-003` | `source` and `snapshot` are mutually exclusive; passing both throws argument error. | Construct with both and assert rejection. | EngineHandleOptions | TSB-008 | PASS |
| `D-004` | `run({signal})` rejects immediately with `AbortError` if already aborted. | Pre-aborted signal test. | Cancellation semantics | TSB-004 | PASS |
| `D-005` | `run({signal})` abort during execution returns a partial result with `HaltReason.HaltRequested` without calling native `halt()` merely to represent host cancellation. | Long-running rule plus abort; deterministic worker seam asserts no native-halt call. | Cancellation semantics + N-09 | FR-NODE-002 | PASS |
| `D-006` | `run({limit: 0})` follows `N-01`: it fires zero rules but starts a fresh native run that clears the previous halt request and diagnostics. | Compare sync `Engine` and `EngineHandle`, including post-halt/diagnostic state. | Engine run contract + N-01 | FR-NODE-002 | PASS |
| `D-007` | Buffer snapshot transfer across worker boundary functions correctly. | `serialize()` and `fromSnapshot` path via worker. | Worker protocol (Buffer transfer) | TSB-001 | PASS |
| `D-008` | `EngineHandle` preserves `FactId` bigint values through structured clone in responses and ID-taking requests. | High-generation worker assert/get/retract round-trip. | Worker boundary + N-07 | FR-NODE-001 | PASS |
| `D-009` | `EngineHandle.run` uses one fresh chunk followed by continuation chunks and is observationally equivalent to synchronous execution at halt boundaries `1`, batch size, batch size + 1, and twice batch size. | Compare total/result, halted state, agenda, diagnostics, exact-limit precedence, and a later fresh run; D-005 covers cancellation between chunks. | Logical-run batching + N-08/N-09 | FR-NODE-002 | PASS |
| `D-010` | Once Worker construction succeeds, every rejected `EngineHandle.create()` clears its initialization bookkeeping and registered listeners, invokes and awaits `terminate()` exactly once before rejecting, and preserves the primary failure by identity. A termination failure is attached when cleanup can define or redefine an own writable/configurable cause property on the primary `Error`; attachment is best-effort when that descriptor update is rejected and for non-`Error` primaries because identity takes precedence. Successful initialization transfers the live Worker unchanged. Pre-Worker validation and Worker-constructor throws own no Worker; generic send rollback and concurrent public close barriers remain FR-NODE-008 and FR-NODE-010. | Real invalid-source subprocess exits naturally with active resources back at baseline; injected source/snapshot protocol failures, synchronous init send failure, delayed/rejected termination, replaceable/immutable/locked-cause primary failures, duplicate terminal signals, constructor/pre-spawn failures, and successful construction controls. | EngineHandle failed-create ownership | FR-NODE-004 | PASS |

### E) EnginePool Semantics

| ID | Requirement (`MUST`) | Validation | Spec Ref | Related Remediation | Status |
|---|---|---|---|---|---|
| `E-001` | `EnginePool.create(..., {threads})` defaults to `1` when omitted. | Behavioral check with one-worker queueing. | EnginePool API | TSB-004 | PASS |
| `E-002` | `evaluate()` performs `reset -> assert -> run -> collect facts/output`. | Stateful contamination test across calls. | EnginePool evaluate contract | TSB-004 | PASS |
| `E-003` | `evaluate(..., {signal})` rejects immediately if already aborted. | Pre-aborted signal test. | Cancellation semantics | TSB-004 | PASS |
| `E-004` | `evaluate` queued-and-aborted requests reject with `AbortError` when dequeuable. | Single-thread queue test with abort while waiting. | Cancellation semantics | TSB-004 | PASS |
| `E-005` | `evaluate` in-execution abort uses out-of-band batched cancellation and retains the partial `HaltRequested` projection without a synthetic native halt. | Long-running evaluation plus abort; deterministic seam asserts no native-halt call. | Cancellation semantics + N-09 | FR-NODE-002 | PASS |
| `E-006` | `do(..., {signal})` enforces cancellation through completion (per `N-03`). | Abort during callback/proxy operations; expect rejection. | Cancellation semantics + N-03 | TSB-004 | PASS |
| `E-007` | `EngineProxy` operation semantics match documented subset. | Signature/runtime parity checks. | EngineProxy interface | TSB-004 | PASS |
| `E-008` | `close()` waits for in-flight requests to settle before teardown (per `N-04`). | Start long run, call close, verify request completion. | EnginePool close contract | TSB-005 | PASS |
| `E-009` | `close()` is idempotent. | Multiple `close()` calls succeed. | EnginePool lifecycle | TSB-005 | PASS |
| `E-010` | `EngineProxy` preserves `FactId` bigint values through pool structured clone in responses and ID-taking requests. | High-generation pool assert/get/retract round-trip. | Worker boundary + N-07 | FR-NODE-001 | PASS |
| `E-011` | Pool `evaluate` and proxy runs preserve one logical run across chunks and match synchronous exact-boundary results and state. | Exercise proxy/direct runs at `1`, batch size, batch size + 1, and twice batch size, plus `evaluate` at an exact batch boundary; compare totals/state/diagnostics, exact-limit precedence, and a later fresh run. E-005 covers cancellation. | Logical-run batching + N-08/N-09 | FR-NODE-002 | PASS |
| `E-012` | A `do` callback exclusively leases its whole worker slot through the pool-observed normal-settlement boundary and accepted-call drain, with per-slot FIFO admission, serial proxy calls, deterministic invalidation before `do` delivers the outcome, no rollback, topology-independent same-pool reentry rejection, and close waiting for admitted callbacks. | One- and multi-thread delayed callbacks across same and different specs; FIFO and parallel-call ordering; callback-pre-registered Promise reaction ordering; fulfillment/rejection release; retained-proxy method table after the `do` barrier; same-pool `do`/`evaluate`/`close` versus other-pool nesting; close during an idle callback gap; seeded randomized-await stress. | EnginePool worker-slot lease + N-10 | FR-NODE-003 | PASS |

### F) Lifecycle and Closed-State Behavior

| ID | Requirement (`MUST`) | Validation | Spec Ref | Related Remediation | Status |
|---|---|---|---|---|---|
| `F-001` | `Engine.close()` is idempotent. | Call twice; no throw. | Engine lifecycle | TSB-006 | PASS |
| `F-002` | After `Engine.close()`, all operational methods throw deterministic errors. | Method matrix post-close. | Engine lifecycle | TSB-006 | PASS |
| `F-003` | `EngineHandle.close()` is idempotent and releases worker resources. | Call twice; subsequent calls reject appropriately. | EngineHandle lifecycle | TSB-005 | PASS |
| `F-004` | `EnginePool.close()` prevents new submissions after close. | Call evaluate/do post-close. | EnginePool lifecycle | TSB-005 | PASS |

### G) Packaging and Test Coverage

| ID | Requirement (`MUST`) | Validation | Spec Ref | Related Remediation | Status |
|---|---|---|---|---|---|
| `G-001` | Packed main and exact-version native artifacts install without source or network access and expose the barrel, sync engine, and worker API through CommonJS and dynamic import. | Pack the release tarballs, install them offline with an empty npm cache, create/run/close sync and worker engines, and compare manifest/embedded versions. | Package layout sections; FR-DIST-001 | TSB-009 | PASS |
| `G-002` | Native load failure is explicit at runtime for missing, unsupported, or version-skewed value imports (no silent undefined API surface). | Simulate missing packages, unsupported targets, and metadata/embedded binary version mismatch. | Public API expectations; FR-DIST-001 | TSB-002 | PASS |
| `G-003` | TS binding tests execute non-zero cases in CI/local scripts. | `npm test` should report >0 tests. | Process quality requirement | TSB-009 | PASS |
| `G-004` | Conformance matrix items map to concrete test files and are tracked in CI. | CI config + test manifest check. | This document | TSB-009 | PASS |

## Recommended Test Artifact Layout

- `packages/ferric/test/conformance/types/*.ts` for strict type tests (`A-*`).
- `packages/ferric/test/conformance/runtime/sync/*.test.ts` for `Engine` (`B-*`, `C-*`, `F-*`).
- `packages/ferric/test/conformance/runtime/worker/*.test.ts` for `EngineHandle` (`B-*`, `C-*`, `D-*`).
- `packages/ferric/test/conformance/runtime/pool/*.test.ts` for `EnginePool` (`B-*`, `C-*`, `E-*`).
- `packages/ferric/test/conformance/package/*.test.ts` for packaging/load behavior (`G-*`).

Each test should include the matrix ID in its title, for example `E-004 queued evaluate abort rejects`.

## Implementation Plan Tie-In

Recommended remediation sequence aligned with risk:
1. `A-001` / `A-002` / `A-003` / `B-002` / `B-004` / `C-*`
2. `D-003` / `D-006` / `D-009` / `D-010` / `E-004` / `E-006` / `E-008` / `E-011` / `E-012`
3. `A-005` / `F-*`
4. `G-*` hardening and CI gating
