# TypeScript Binding Reimplementation Plan (Sequential, Shovel-Ready)

Date: 2026-04-11
Owner model: single coding agent, single branch, linear execution

Inputs:
- [Architecture](/Users/prb/conductor/workspaces/ferric-rules/santo-domingo/docs/typescript-binding-architecture.md)
- [Normative Contract](/Users/prb/conductor/workspaces/ferric-rules/santo-domingo/docs/typescript-binding-normative-contract.md)
- [Conformance Matrix](/Users/prb/conductor/workspaces/ferric-rules/santo-domingo/docs/typescript-binding-conformance-matrix.md)
- [Test Spec](/Users/prb/conductor/workspaces/ferric-rules/santo-domingo/docs/typescript-binding-test-spec.md)

## 0) Execution Rules
1. Execute steps strictly in order.
2. Do not skip ahead when a step has failing required checks.
3. Commit at the end of each step with the step ID in the commit message.
4. If a step reveals spec ambiguity, update the normative docs first, then continue.

## 1) Bootstrapping: Test Harness and CI Guards

### Goal
Create the required test structure and command surface before behavioral changes.

### Files to touch
- `packages/ferric/package.json`
- `packages/ferric/test/conformance/types/`
- `packages/ferric/test/conformance/runtime/sync/`
- `packages/ferric/test/conformance/runtime/worker/`
- `packages/ferric/test/conformance/runtime/pool/`
- `packages/ferric/test/conformance/package/`
- `packages/ferric/test/helpers/`
- CI workflow file(s) under `.github/workflows/` that currently run package checks

### Tasks
1. Add scripts:
   - `test:types`
   - `test:runtime:sync`
   - `test:runtime:worker`
   - `test:runtime:pool`
   - `test:package`
   - `test` aggregating all above
2. Implement zero-test guards per category.
3. Add placeholder smoke tests (one per category) with matrix IDs.
4. Wire CI to run all categories and fail on zero tests.

### Required checks
- `cd packages/ferric && npm test` runs non-zero tests.
- CI config includes all categories.

### Matrix coverage target
- `G-003`, `G-004` from FAIL/UNKNOWN -> at least provisionally PASS.

---

## 2) Public API Surface Correction

### Goal
Make the public package type/runtime surface conformant (`A-001`, `A-002`, `A-003`, `G-002`).

### Files to touch
- `packages/ferric/src/native.ts`
- `packages/ferric/src/types.ts`
- `packages/ferric/src/index.ts`
- build outputs regenerated in `packages/ferric/dist/*`

### Tasks
1. Remove optional runtime export typing for `Engine`/`FerricSymbol`.
2. Fail fast when native module cannot be loaded (no silent undefined class surface).
3. Restore public `ClipsValue` union to include `FerricSymbol`.
4. Keep wire-only representations internal to transport modules.

### Required checks
- Strict type tests for:
  - `new Engine()`
  - `new FerricSymbol("x")`
  - `const v: ClipsValue = new FerricSymbol("x")`
- Package load failure test asserts explicit throw behavior.

### Matrix coverage target
- `A-001`, `A-002`, `A-003`, `G-002` -> PASS.

---

## 3) Canonical Worker Value/Wire Conversion

### Goal
Unify value conversion across worker boundaries (`B-002`, `B-004`, `N-05`).

### Files to touch
- `packages/ferric/src/wire.ts`
- `packages/ferric/src/engine-handle.ts`
- `packages/ferric/src/engine-pool.ts`
- `packages/ferric/src/worker.ts`
- `packages/ferric/src/pool-worker.ts`
- potentially `crates/ferric-napi/src/value.rs` (if native marker compatibility needed)

### Tasks
1. Enforce canonical symbol wire shape `{ __type: "FerricSymbol", value: string }`.
2. Ensure outbound request args pass through `toWire`.
3. Ensure inbound results pass through reconstruction (`fromWire` + `FerricSymbol` rehydration where required).
4. Remove/avoid alternate marker formats in TS-facing worker paths.

### Required checks
- Worker tests for symbol in:
  - ordered fact fields
  - template slots
  - nested arrays/multifields
- Worker result tests ensure symbol outputs are proper `FerricSymbol` values.

### Matrix coverage target
- `B-002`, `B-004` -> PASS.

---

## 4) Error Class Mapping End-to-End

### Goal
Make sync + worker error hierarchy deterministic (`C-001`..`C-005`).

### Files to touch
- `crates/ferric-napi/src/error.rs`
- `packages/ferric/src/types.ts`
- `packages/ferric/src/worker.ts`
- `packages/ferric/src/pool-worker.ts`
- `packages/ferric/src/engine-handle.ts`
- `packages/ferric/src/engine-pool.ts`

### Tasks
1. Define stable mapping table: native error -> `{name, code, message}`.
2. Ensure worker payload `name` is the Ferric class name, not generic `Error`.
3. Reconstruct exact classes in TS wrappers.
4. Keep unknown-name fallback to `FerricError` with preserved code/message.

### Required checks
- One test per error class for sync API.
- One test per error class for worker-backed API.
- Unknown error payload fallback test.

### Matrix coverage target
- `C-001`..`C-005` -> PASS.

---

## 5) Run-Limit Semantic Alignment

### Goal
Resolve and implement limit semantics (`D-006`, `N-01`, `N-02`).

### Files to touch
- `packages/ferric/src/engine-handle.ts`
- `packages/ferric/src/worker.ts`
- `packages/ferric/src/engine-pool.ts`
- `packages/ferric/src/pool-worker.ts`
- optional native-side validation paths if required

### Tasks
1. Implement:
   - `Engine.run/EngineHandle.run`: `undefined` unlimited, `0` zero firings.
   - `EvaluateRequest.limit`: `0` or omitted unlimited.
2. Ensure proxy `run` follows `Engine/EngineHandle` semantics, not evaluate semantics.

### Required checks
- Matrix tests for `undefined`, `0`, positive limit across:
  - sync engine
  - engine handle
  - engine pool proxy run
  - engine pool evaluate

### Matrix coverage target
- `D-006`, plus guard for `E-002` behavior consistency.

---

## 6) EngineHandle Create Validation

### Goal
Enforce mutual exclusivity for source/snapshot (`D-003`).

### Files to touch
- `packages/ferric/src/engine-handle.ts`

### Tasks
1. Add explicit argument validation in `create(options)`.
2. Reject `{ source, snapshot }` with deterministic argument error.

### Required checks
- Dedicated test expecting rejection and stable error shape.

### Matrix coverage target
- `D-003` -> PASS.

---

## 7) EnginePool Cancellation Semantics

### Goal
Implement queued and in-flight cancellation behavior per contract (`E-004`, `E-006`, `N-03`).

### Files to touch
- `packages/ferric/src/engine-pool.ts`
- `packages/ferric/src/pool-worker.ts`
- maybe helper queue structures in `packages/ferric/src/`

### Tasks
1. Track request lifecycle states: queued, dispatched, settled.
2. For `evaluate`:
   - pre-abort reject,
   - queued abort reject/dequeue,
   - in-flight cooperative halt.
3. For `do`:
   - abort before completion rejects with `AbortError`.
4. Ensure no silent successful resolution after abort.

### Required checks
- Deterministic queue tests with `threads: 1`.
- Abort-before-dispatch, abort-while-queued, abort-during-execution.

### Matrix coverage target
- `E-004`, `E-006` -> PASS.

---

## 8) EnginePool Close Semantics

### Goal
Implement graceful close contract (`E-008`, `E-009`, `N-04`).

### Files to touch
- `packages/ferric/src/engine-pool.ts`

### Tasks
1. `close()` rejects new requests immediately.
2. `close()` waits for dispatched in-flight requests to settle.
3. After in-flight settle, teardown workers.
4. Ensure idempotency.

### Required checks
- Long-running in-flight request started before `close()` completes successfully.
- New requests after close reject.
- Multiple `close()` calls succeed.

### Matrix coverage target
- `E-008`, `E-009` -> PASS.

---

## 9) Explicit Resource Management for Sync Engine

### Goal
Implement `[Symbol.dispose]` support (`A-005`).

### Files to touch
- `packages/ferric/src/native.ts` (or wrapper layer)
- optional `crates/ferric-napi` if needed

### Tasks
1. Ensure `Engine.prototype[Symbol.dispose]` exists.
2. Ensure behavior delegates to `close()` and is idempotent.

### Required checks
- Runtime test for `typeof engine[Symbol.dispose] === "function"`.
- `using` block behavior test (where runtime supports it).

### Matrix coverage target
- `A-005` -> PASS.

---

## 10) Remaining Unknowns and Lifecycle Hardening

### Goal
Eliminate all `UNKNOWN` statuses in matrix sections A-E/F where required.

### Files to touch
- whatever tests/implementation required to close unknowns

### Required checks
- `B-007`, `E-001`, `C-005`, `F-002`, `E-009` and any remaining unknowns are covered and passing.

---

## 11) Final Conformance Sweep

### Goal
Declare implementation complete against revised spec.

### Tasks
1. Update [Conformance Matrix] statuses to reflect final results.
2. Ensure every A-E item has at least one test reference.
3. Run full package test suite.
4. Verify docs/examples compile with strict type tests.

### Required checks
- All A-E rows = `PASS`.
- No required category has zero tests.
- `npm test` passes.

---

## 12) Suggested Commit Sequence
1. `STEP-01 test harness + CI gates`
2. `STEP-02 public API typing/runtime surface`
3. `STEP-03 worker value conversion`
4. `STEP-04 error mapping`
5. `STEP-05 run limit semantics`
6. `STEP-06 create options validation`
7. `STEP-07 pool cancellation`
8. `STEP-08 pool close semantics`
9. `STEP-09 Symbol.dispose`
10. `STEP-10 unknown closures + lifecycle`
11. `STEP-11 final conformance update`

## Handoff Note (for an agent)
Treat this file as the execution order, and the Conformance Matrix as the source of truth for pass/fail. If a task seems “done” but its matrix IDs are not demonstrably passing in tests, it is not done.
