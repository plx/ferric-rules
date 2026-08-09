# TypeScript Binding Test Specification (Revised)

Date: 2026-04-11
Updated: 2026-08-09 (FR-NODE-004 failed EngineHandle creation cleanup)
Status: Required for reimplementation

Companion documents:
- [Normative Contract](/Users/prb/conductor/workspaces/ferric-rules/santo-domingo/docs/typescript-binding-normative-contract.md)
- [Conformance Matrix](/Users/prb/conductor/workspaces/ferric-rules/santo-domingo/docs/typescript-binding-conformance-matrix.md)

## 1. Purpose
Define mandatory automated test coverage for the TypeScript bindings.

This is not optional guidance. The implementation is incomplete until this specification is satisfied.

## 2. Test Categories

### 2.1 Type Conformance Tests
- Validate public package `.d.ts` under `tsc --strict`.
- Ensure exported runtime values are concrete and API examples type-check.

### 2.2 Sync Runtime Unit Tests (`Engine`)
- Exercise value conversion, run semantics, errors, lifecycle, and serialization.
- Must run without worker threads.

### 2.3 Worker Runtime Unit Tests (`EngineHandle`)
- Exercise wire conversion, cancellation, worker protocol, error
  reconstruction, and failed-construction Worker ownership.

### 2.4 Pool Runtime Unit Tests (`EnginePool`)
- Exercise queueing, dispatch, cancellation states, worker-slot lease isolation,
  proxy lifetime and ordering, same-pool reentrancy, `close()` behavior, and
  stateless evaluation behavior.

### 2.5 Package/Load Tests
- Validate native load behavior and package entrypoint surface guarantees.

## 3. Required Test Layout

The binding test tree `MUST` include:

```text
packages/ferric/test/
├── conformance/
│   ├── types/
│   ├── runtime/
│   │   ├── sync/
│   │   ├── worker/
│   │   └── pool/
│   └── package/
└── helpers/
```

Every test title `MUST` include at least one Conformance Matrix ID (example: `C-001 parse errors map to FerricParseError`).

## 4. Required Coverage Inventory

At minimum, test suite `MUST` include all items below.

### 4.1 Type Tests (minimum 10 cases)
1. `A-001` concrete `Engine` export.
2. `A-002` concrete `FerricSymbol` export.
3. `A-003` `ClipsValue` includes `FerricSymbol`.
4. Public enum usability from package entrypoint.
5. `Engine` API method signatures compile for documented usage.
6. `EngineHandle` API signatures compile for documented usage.
7. `EnginePool` API signatures compile for documented usage.
8. `using`/`await using` signatures compile (`Symbol.dispose`, `Symbol.asyncDispose`).
9. Error classes are importable and constructible.
10. All code snippets from normative docs compile unchanged.
11. `A-007` `FactId`/`FactIdInput` exports and sync, worker, and pool fact-ID
    signatures compile with `bigint` outputs and safe-number-compatible inputs.

### 4.2 Sync Runtime Tests (minimum 30 cases)
Must include:
- Value conversions (`B-001`, `B-003`, `B-005`, `B-006`, `B-007`).
- Fact shape and retrieval (`B-008`, `B-009`).
- Lossless fact-ID boundaries and high-generation lifecycle round-trip
  (`B-010`, `B-011`), including every sync ID-taking API.
- Run semantics including `limit` behavior (`D-006` sync side, `N-01`).
- Fresh-run state semantics for `run(0)`, including clearing a prior halt
  request and action diagnostics (`D-006`, `N-08`).
- Error mappings for all documented error subclasses (`C-001` to `C-003`).
- Lifecycle semantics (`F-001`, `F-002`, `A-005` where applicable).
- Snapshot round-trip behavior.

### 4.3 Worker Runtime Tests (minimum 30 cases)
Must include:
- Symbol input/output round-trip across worker boundary (`B-002`, `B-004`).
- Snapshot transport using worker path (`D-002`, `D-007`).
- `source`/`snapshot` exclusivity (`D-003`).
- Cancellation pre-abort and in-flight abort (`D-004`, `D-005`), including a
  deterministic assertion that host abort does not call native `halt()` merely
  to produce the public partial `HaltRequested` result.
- Run limit behavior parity with sync (`D-006`, `N-01`), including a real fresh
  native `run(0)` and its execution-state reset.
- Logical-run parity (`D-009`, `N-08`, `N-09`): exactly one fresh chunk,
  continuation-only later chunks, total count accumulation, early diagnostic
  retention, exact-boundary halt and explicit-limit precedence at `1`, batch
  size, batch size + 1, and twice batch size, and a later invocation starting
  fresh. D-005 covers cancellation between chunks without a synthetic native
  halt.
- Error payload and reconstruction correctness (`C-001` to `C-005`).
- `FactId` structured-clone response/request round-trip (`D-008`).
- Failed `EngineHandle.create()` ownership (`D-010`), including:
  - a timeout-guarded real invalid-source subprocess that catches the
    initialization error, never calls `process.exit()` or `unref()`, returns
    active Worker resources to baseline, and exits naturally;
  - source and snapshot initialization protocol failures;
  - a synchronous initialization `postMessage` throw after request
    registration;
  - response, error, and exit listener counts plus pending initialization
    bookkeeping returning to baseline;
  - a delayed termination barrier proving `create()` remains unsettled until
    its single termination attempt completes;
  - a termination rejection that leaves the exact primary error as the public
    rejection, attaches the termination error as the cause of an extensible
    `Error`, and preserves frozen/non-extensible or non-`Error` primaries by
    identity when cause metadata cannot be attached;
  - duplicate or late protocol/error/exit signals proving request settlement
    and termination each occur exactly once;
  - pre-Worker validation and Worker-constructor throw controls; and
  - a successful construction control proving ownership transfers without
    failed-create teardown.

The synchronous-send case above applies only to initialization cleanup.
Ordinary handle/pool send rollback remains FR-NODE-008, and concurrent public
`close()` completion-barrier coverage remains FR-NODE-010.

### 4.4 Pool Runtime Tests (minimum 45 cases)
Must include:
- Evaluate lifecycle (`E-002`).
- Cancellation for pre-abort, queued abort, and in-flight abort (`E-003`,
  `E-004`, `E-005`), including the no-synthetic-native-halt assertion.
- `do()` cancellation behavior (`E-006`).
- Proxy behavior parity (`E-007`).
- `close()` contract (in-flight completion, admitted-callback completion, and
  idempotency) (`E-008`, `E-009`, `E-012`).
- Thread default behavior (`E-001`).
- `FactId` structured-clone response/request round-trip through a proxy
  (`E-010`).
- Logical-run parity for both `evaluate` and proxy/direct run paths (`E-011`,
  `N-08`, `N-09`), covering exact-boundary result/state equivalence,
  diagnostics, accumulated totals, explicit-limit precedence, and a later
  fresh run. E-005 covers cancellation between chunks without a synthetic
  native halt.
- Exclusive `do` worker-slot leases (`E-012`, `N-10`), including:
  - deterministic one-thread and multi-thread delayed callbacks;
  - exclusion across both the same spec and different specs on one worker;
  - per-slot FIFO callback admission and serialization of parallel proxy calls
    in invocation order;
  - lease release after callback fulfillment, synchronous throw, and
    asynchronous rejection;
  - callback-pre-registered Promise reactions remaining inside the lease,
    followed by invalidation at the pool's registered settlement reaction;
  - draining calls accepted before that pool-observed settlement boundary;
  - a retained-proxy method table proving the same lifetime error and no new
    request allocation or dispatch after `do` delivers the callback outcome;
  - persistent state after rejection, proving that leases do not roll back;
  - deterministic rejection for same-pool `do`, `evaluate`, and `close`
    reentry with one or multiple threads, while another pool remains usable;
  - `close()` waiting through an admitted callback's idle await and accepted
    proxy calls; and
  - seeded randomized-await stress whose failure identifies the seed.

Cancellation-time proxy invalidation, mutation by work accepted before abort,
and cancellation-triggered lease release are FR-NODE-009 coverage. E-012 covers
the lease and normal callback-settlement boundary without preempting that work.

### 4.5 Package Tests (minimum 10 cases)
Must include:
- Entrypoint exports availability (`G-001`).
- Native load failure is explicit and deterministic (`G-002`).
- Runtime smoke checks across documented import patterns.

## 5. Test Data and Fixtures

1. Include reusable CLIPS fixtures for:
   - Symbol/string discrimination,
   - Long-running loops for cancellation,
   - Exact-boundary halts at `1`, batch size, batch size + 1, and twice batch
     size, with a post-halt activation that must remain queued,
   - A diagnostic emitted before a continuation boundary,
   - Slot/template error cases,
   - Module/focus behavior,
   - Serialization round-trip.
2. Fixtures `MUST` be deterministic and avoid flaky timing assumptions.
3. A Node subprocess `MUST` drive a real engine generation above
   `Number.MAX_SAFE_INTEGER` and prove assert/get/retract round-trip without
   relying only on synthetic conversion helpers.

## 6. Determinism and Flake Controls

1. Cancellation tests `MUST` use bounded deterministic waits and explicit synchronization helpers.
2. Tests `MUST NOT` rely on wall-clock races as sole pass condition.
3. Any retry logic `MUST` be explicit and justified.
4. Logical-run routing tests `MUST` use deterministic seams or direct worker
   protocol observation to distinguish fresh-run, continuation, halt-query, and
   host-abort paths.
5. Lease tests `MUST` use explicit callback-entry and release barriers rather
   than sleeps as their correctness oracle.
6. Randomized-await lease stress `MUST` use a recorded deterministic seed and
   bounded iteration count.
7. Failed-create subprocess tests `MUST` use a parent-enforced timeout as a
   failure guard while requiring natural child exit as the passing condition.

## 7. CI and Local Gates

### 7.1 Required Commands
The package `MUST` provide commands equivalent to:
1. `npm run test:types`
2. `npm run test:runtime:sync`
3. `npm run test:runtime:worker`
4. `npm run test:runtime:pool`
5. `npm run test:package`
6. `npm test` (runs all above)

### 7.2 Zero-Test Guard
1. CI `MUST` fail when discovered test count is zero for any required category.
2. Local `npm test` `MUST` report non-zero total tests.

### 7.3 Conformance Mapping Gate
1. CI `MUST` validate that every matrix item in sections A-E is referenced by at least one test title.
2. Missing mapping `MUST` fail CI.

## 8. Exit Criteria for Reimplementation

All must be true:
1. Conformance Matrix sections A-E are all `PASS`.
2. No required test category has zero tests.
3. Test minimum counts in section 4 are met or exceeded.
4. All normative examples compile under strict mode.
5. No known flaky test in mainline.
