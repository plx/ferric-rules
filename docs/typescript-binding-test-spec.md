# TypeScript Binding Test Specification (Revised)

Date: 2026-04-11
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
- Exercise wire conversion, cancellation, worker protocol, and error reconstruction.

### 2.4 Pool Runtime Unit Tests (`EnginePool`)
- Exercise queueing, dispatch, cancellation states, `close()` behavior, and stateless evaluation behavior.

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

### 4.2 Sync Runtime Tests (minimum 30 cases)
Must include:
- Value conversions (`B-001`, `B-003`, `B-005`, `B-006`, `B-007`).
- Fact shape and retrieval (`B-008`, `B-009`).
- Run semantics including `limit` behavior (`D-006` sync side, `N-01`).
- Error mappings for all documented error subclasses (`C-001` to `C-003`).
- Lifecycle semantics (`F-001`, `F-002`, `A-005` where applicable).
- Snapshot round-trip behavior.

### 4.3 Worker Runtime Tests (minimum 30 cases)
Must include:
- Symbol input/output round-trip across worker boundary (`B-002`, `B-004`).
- Snapshot transport using worker path (`D-002`, `D-007`).
- `source`/`snapshot` exclusivity (`D-003`).
- Cancellation pre-abort and in-flight abort (`D-004`, `D-005`).
- Run limit behavior parity with sync (`D-006`, `N-01`).
- Error payload and reconstruction correctness (`C-001` to `C-005`).

### 4.4 Pool Runtime Tests (minimum 35 cases)
Must include:
- Evaluate lifecycle (`E-002`).
- Cancellation for pre-abort, queued abort, in-flight abort (`E-003`, `E-004`, `E-005`).
- `do()` cancellation behavior (`E-006`).
- Proxy behavior parity (`E-007`).
- `close()` contract (in-flight completion and idempotency) (`E-008`, `E-009`).
- Thread default behavior (`E-001`).

### 4.5 Package Tests (minimum 10 cases)
Must include:
- Entrypoint exports availability (`G-001`).
- Native load failure is explicit and deterministic (`G-002`).
- Runtime smoke checks across documented import patterns.

## 5. Test Data and Fixtures

1. Include reusable CLIPS fixtures for:
   - Symbol/string discrimination,
   - Long-running loops for cancellation,
   - Slot/template error cases,
   - Module/focus behavior,
   - Serialization round-trip.
2. Fixtures `MUST` be deterministic and avoid flaky timing assumptions.

## 6. Determinism and Flake Controls

1. Cancellation tests `MUST` use bounded deterministic waits and explicit synchronization helpers.
2. Tests `MUST NOT` rely on wall-clock races as sole pass condition.
3. Any retry logic `MUST` be explicit and justified.

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
