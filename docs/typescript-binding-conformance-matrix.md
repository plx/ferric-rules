# TypeScript Binding Conformance Matrix

Date: 2026-04-11

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
- `PASS`: Verified conformant in the 2026-04-11 audit.
- `FAIL`: Verified non-conformant in the 2026-04-11 audit.
- `UNKNOWN`: Not yet verified by direct probe.

## Normative Clarifications (Resolved Ambiguities)

| Decision ID | Clarification |
|---|---|
| `N-01` | `Engine.run(limit)` and `EngineHandle.run({limit})` interpret `limit` as: omitted/`undefined` => unlimited, `0` => zero firings, positive integer => maximum firings. |
| `N-02` | `EvaluateRequest.limit` keeps documented convenience behavior: `0` or omitted => unlimited. |
| `N-03` | `EnginePool.do(..., { signal })` must reject with `AbortError` if aborted before completion. In-flight `run` operations must use the same cooperative halt mechanism as `EngineHandle.run`. |
| `N-04` | `EnginePool.close()` must stop accepting new requests and wait for already-dispatched requests to settle before worker teardown. |
| `N-05` | Worker symbol wire format is canonicalized as `{ __type: "FerricSymbol", value: string }` at the TS layer. |
| `N-06` | Public library API exports concrete `Engine` and `FerricSymbol` classes (not optional exports). |

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
| `A-001` | Public export `Engine` is a concrete class value, not `undefined | class`. | `tsc --strict` on `new Engine()` from package entrypoint. | API: Engine class | TSB-002 | FAIL |
| `A-002` | Public export `FerricSymbol` is a concrete class value. | `tsc --strict` on `new FerricSymbol("x")`. | Value types | TSB-002 | FAIL |
| `A-003` | `ClipsValue` includes `FerricSymbol` in public API types. | Type assertion: `const v: ClipsValue = new FerricSymbol("x")`. | Value types | TSB-002 | FAIL |
| `A-004` | Public enums are regular TS enums in the package-facing API (no `const enum` in public `dist/index.d.ts` surface). | Inspect generated public d.ts and compile consumer sample. | Implementation notes (enum guidance) | TSB-002 | PASS |
| `A-005` | `Engine` supports `[Symbol.dispose](): void` for `using`. | Runtime check + `using` integration test on supported Node. | Engine lifecycle + examples | TSB-006 | FAIL |
| `A-006` | `EngineHandle` and `EnginePool` support `[Symbol.asyncDispose](): Promise<void>`. | Runtime `await using` test. | EngineHandle/Pool lifecycle | TSB-006 | PASS |

### B) Value Conversion and Fact Shape

| ID | Requirement (`MUST`) | Validation | Spec Ref | Related Remediation | Status |
|---|---|---|---|---|---|
| `B-001` | JS `FerricSymbol` input works in sync `Engine.assertFact` / `assertTemplate`. | Runtime assertion + rule match test. | Value conversion JS->CLIPS | TSB-001 | PASS |
| `B-002` | JS `FerricSymbol` input works in `EngineHandle` and `EnginePool` operations. | Runtime assertion via worker-backed APIs. | Worker serialization + value conversion | TSB-001 | FAIL |
| `B-003` | CLIPS symbol values returned via sync `Engine` are `FerricSymbol` instances. | Assert `instanceof FerricSymbol` on fact/global output. | Value conversion CLIPS->JS | TSB-001 | PASS |
| `B-004` | CLIPS symbol values returned via worker-backed APIs are reconstructed as `FerricSymbol` values (not `{}` / untyped objects). | Assert shape/class for `facts/getFact/getGlobal/evaluate`. | Worker serialization/reconstruction | TSB-001 | FAIL |
| `B-005` | `string` maps to CLIPS string, not symbol. | Rule discrimination test (`"red"` vs `red`). | Value types note | TSB-001 | PASS |
| `B-006` | `boolean` maps to CLIPS symbols `TRUE/FALSE`. | Assert facts and inspect returned symbol values. | JS->CLIPS table | TSB-001 | PASS |
| `B-007` | Integers in safe range return JS `number`; outside safe range return `bigint`. | Boundary tests around `2^53-1`. | Integer representation section | TSB-001 | UNKNOWN |
| `B-008` | `assertString` returns all asserted fact IDs. | Assert multi-fact string and verify length/IDs. | Engine API | TSB-001 | PASS |
| `B-009` | `Fact` shape conforms: ordered facts have `relation+fields`, template facts have `templateName+fields` and slot map when applicable. | Snapshot structural assertions. | Result types (`Fact`) | TSB-001 | PASS |

### C) Error Mapping and Hierarchy

| ID | Requirement (`MUST`) | Validation | Spec Ref | Related Remediation | Status |
|---|---|---|---|---|---|
| `C-001` | Parse failures surface as `FerricParseError` (sync and worker-backed). | Trigger parse error and assert `instanceof`, `.name`, `.code`. | Error hierarchy | TSB-003 | FAIL |
| `C-002` | Compile failures surface as `FerricCompileError`. | Trigger compile error and assert class/code. | Error hierarchy | TSB-003 | FAIL |
| `C-003` | Runtime/fact/template/slot/module/encoding/serialization failures map to documented subclasses. | One targeted case per class. | Error hierarchy | TSB-003 | FAIL |
| `C-004` | Worker response error payload contains stable `name`, `code`, `message` used for reconstruction. | Inspect worker responses via harness. | Worker protocol | TSB-003 | FAIL |
| `C-005` | Unknown worker errors degrade to base `FerricError` with preserved code/message. | Inject synthetic unknown error payload. | Error hierarchy | TSB-003 | UNKNOWN |

### D) EngineHandle and Worker Protocol

| ID | Requirement (`MUST`) | Validation | Spec Ref | Related Remediation | Status |
|---|---|---|---|---|---|
| `D-001` | `EngineHandle.create({source})` performs load + reset. | Rules available immediately after creation. | EngineHandleOptions | TSB-008 | PASS |
| `D-002` | `EngineHandle.create({snapshot})` restores from snapshot. | Snapshot round-trip and rule presence check. | EngineHandleOptions | TSB-008 | PASS |
| `D-003` | `source` and `snapshot` are mutually exclusive; passing both throws argument error. | Construct with both and assert rejection. | EngineHandleOptions | TSB-008 | FAIL |
| `D-004` | `run({signal})` rejects immediately with `AbortError` if already aborted. | Pre-aborted signal test. | Cancellation semantics | TSB-004 | PASS |
| `D-005` | `run({signal})` abort during execution returns partial result with `HaltReason.HaltRequested`. | Long-running rule + timed abort. | Cancellation semantics | TSB-004 | PASS |
| `D-006` | `run({limit: 0})` follows `N-01` (`0` means zero firings). | Compare sync `Engine` and `EngineHandle`. | Engine run contract + N-01 | TSB-007 | FAIL |
| `D-007` | Buffer snapshot transfer across worker boundary functions correctly. | `serialize()` and `fromSnapshot` path via worker. | Worker protocol (Buffer transfer) | TSB-001 | PASS |

### E) EnginePool Semantics

| ID | Requirement (`MUST`) | Validation | Spec Ref | Related Remediation | Status |
|---|---|---|---|---|---|
| `E-001` | `EnginePool.create(..., {threads})` defaults to `1` when omitted. | Behavioral check with one-worker queueing. | EnginePool API | TSB-004 | UNKNOWN |
| `E-002` | `evaluate()` performs `reset -> assert -> run -> collect facts/output`. | Stateful contamination test across calls. | EnginePool evaluate contract | TSB-004 | PASS |
| `E-003` | `evaluate(..., {signal})` rejects immediately if already aborted. | Pre-aborted signal test. | Cancellation semantics | TSB-004 | PASS |
| `E-004` | `evaluate` queued-and-aborted requests reject with `AbortError` when dequeuable. | Single-thread queue test with abort while waiting. | Cancellation semantics | TSB-004 | FAIL |
| `E-005` | `evaluate` in-execution abort uses batched halt semantics. | Long-running evaluation + abort. | Cancellation semantics | TSB-004 | PASS |
| `E-006` | `do(..., {signal})` enforces cancellation through completion (per `N-03`). | Abort during callback/proxy operations; expect rejection. | Cancellation semantics + N-03 | TSB-004 | FAIL |
| `E-007` | `EngineProxy` operation semantics match documented subset. | Signature/runtime parity checks. | EngineProxy interface | TSB-004 | PASS |
| `E-008` | `close()` waits for in-flight requests to settle before teardown (per `N-04`). | Start long run, call close, verify request completion. | EnginePool close contract | TSB-005 | FAIL |
| `E-009` | `close()` is idempotent. | Multiple `close()` calls succeed. | EnginePool lifecycle | TSB-005 | UNKNOWN |

### F) Lifecycle and Closed-State Behavior

| ID | Requirement (`MUST`) | Validation | Spec Ref | Related Remediation | Status |
|---|---|---|---|---|---|
| `F-001` | `Engine.close()` is idempotent. | Call twice; no throw. | Engine lifecycle | TSB-006 | PASS |
| `F-002` | After `Engine.close()`, all operational methods throw deterministic errors. | Method matrix post-close. | Engine lifecycle | TSB-006 | UNKNOWN |
| `F-003` | `EngineHandle.close()` is idempotent and releases worker resources. | Call twice; subsequent calls reject appropriately. | EngineHandle lifecycle | TSB-005 | PASS |
| `F-004` | `EnginePool.close()` prevents new submissions after close. | Call evaluate/do post-close. | EnginePool lifecycle | TSB-005 | PASS |

### G) Packaging and Test Coverage

| ID | Requirement (`MUST`) | Validation | Spec Ref | Related Remediation | Status |
|---|---|---|---|---|---|
| `G-001` | Published package contains barrel exports, async wrappers, and native loader assets per layout intent. | Package content audit. | Package layout sections | TSB-009 | PASS |
| `G-002` | Native load failure is explicit at runtime for value imports (no silent undefined API surface). | Simulate missing native binary import path. | Public API expectations | TSB-002 | FAIL |
| `G-003` | TS binding tests execute non-zero cases in CI/local scripts. | `npm test` should report >0 tests. | Process quality requirement | TSB-009 | FAIL |
| `G-004` | Conformance matrix items map to concrete test files and are tracked in CI. | CI config + test manifest check. | This document | TSB-009 | UNKNOWN |

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
2. `D-003` / `D-006` / `E-004` / `E-006` / `E-008`
3. `A-005` / `F-*`
4. `G-*` hardening and CI gating
