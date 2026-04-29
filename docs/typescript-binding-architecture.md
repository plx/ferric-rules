# TypeScript Binding Architecture (Revised)

Date: 2026-04-11
Status: Draft for reimplementation

Companion documents:
- [Normative Contract](/Users/prb/conductor/workspaces/ferric-rules/santo-domingo/docs/typescript-binding-normative-contract.md)
- [Conformance Matrix](/Users/prb/conductor/workspaces/ferric-rules/santo-domingo/docs/typescript-binding-conformance-matrix.md)
- [Test Specification](/Users/prb/conductor/workspaces/ferric-rules/santo-domingo/docs/typescript-binding-test-spec.md)

Supersedes as implementation target:
- [Legacy API Design Draft](/Users/prb/conductor/workspaces/ferric-rules/santo-domingo/docs/typescript-binding-api.md)

## Purpose
Define the high-level architecture for Node.js/TypeScript bindings to `ferric-rules` while delegating all strict behavior to the Normative Contract.

This document is intentionally descriptive. If this document conflicts with the Normative Contract, the Normative Contract wins.

## Goals
1. Provide a TypeScript-native API that is ergonomic in Node.js.
2. Preserve Ferric thread-affinity constraints.
3. Support both synchronous and non-blocking worker-backed usage.
4. Keep native binding minimal and deterministic.

## Non-Goals
1. Browser/Wasm support.
2. Deno/Bun compatibility as a release target.
3. Rule-firing streaming callbacks in v1.
4. Rete internals exposure.

## Layered Design

### Layer 1: Native `Engine` (napi-rs)
- Rust crate: `crates/ferric-napi`
- Exposes synchronous API directly backed by Ferric runtime.
- Holds engine ownership and performs core value conversion.
- No pooling/orchestration logic in Rust.

### Layer 2: `EngineHandle` (worker-backed async)
- TypeScript wrapper over a dedicated worker thread.
- Owns request/response transport, cancellation handling, and result/error reconstruction.
- Provides Promise-based API matching `Engine` semantics where applicable.

### Layer 3: `EnginePool` (multi-worker concurrency)
- TypeScript orchestration over multiple workers.
- Dispatches work round-robin across worker slots.
- Supports stateless one-shot evaluation plus stateful proxy operations.

## Package Layout

Expected source layout:

```text
packages/ferric/
├── src/
│   ├── index.ts
│   ├── native.ts
│   ├── engine-handle.ts
│   ├── engine-pool.ts
│   ├── worker.ts
│   ├── pool-worker.ts
│   ├── wire.ts
│   └── types.ts
├── native/
│   ├── index.js
│   └── index.d.ts
└── dist/
```

## Ownership Boundaries
- Rust owns engine correctness and low-level conversion primitives.
- TypeScript owns worker protocol, cancellation orchestration, and high-level lifecycle semantics.
- Public API typing is owned by TypeScript package surface (`dist/index.d.ts`), not by generated native d.ts alone.

## Design Constraints
1. Canonical value wire schema must be single-source-of-truth.
2. Sync and async layers must not diverge semantically unless explicitly documented.
3. Error behavior must be class-stable across boundaries.
4. Lifecycle behavior (`close`, dispose, post-close failures) must be deterministic.

## Risk Seams (Must Receive Focused Review)
1. Symbol/value conversion across worker boundaries.
2. Error class mapping across native and workers.
3. Cancellation semantics for queued vs in-flight operations.
4. `EnginePool.close()` behavior under concurrency.
5. Public TS API shape drift (`undefined` exports, mismatched unions).

## Delivery Model
Reimplementation should be staged and gated:
1. Native sync correctness and typing.
2. EngineHandle transport and cancellation.
3. EnginePool concurrency semantics.
4. Packaging and distribution hardening.

Each stage is complete only when its corresponding rows in the Conformance Matrix are `PASS` and required tests from the Test Specification are green.
