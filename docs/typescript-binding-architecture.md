# TypeScript Binding Architecture (Revised)

Date: 2026-04-11
Updated: 2026-08-09 (FR-NODE-004 failed EngineHandle creation cleanup)
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
- Rust crate: `crates/ferric-rules-napi`
- Exposes synchronous API directly backed by Ferric runtime.
- Holds engine ownership and performs core value conversion.
- No pooling/orchestration logic in Rust.

### Layer 2: `EngineHandle` (worker-backed async)
- TypeScript wrapper over a dedicated worker thread.
- Owns request/response transport, cancellation handling, and result/error reconstruction.
- Owns a constructed Worker throughout asynchronous initialization and
  transfers that ownership to the caller only after initialization succeeds.
- On any post-construction setup or initialization failure, clears the init
  request and registered listeners, awaits exactly one termination attempt,
  and preserves the primary failure while attaching a termination failure as
  its cause.
- Provides Promise-based API matching `Engine` semantics where applicable.

### Layer 3: `EnginePool` (multi-worker concurrency)
- TypeScript orchestration over multiple workers.
- Dispatches work round-robin across worker slots.
- Supports stateless one-shot evaluation plus stateful proxy operations.
- Gives each `do` callback an exclusive, FIFO lease over its selected worker
  slot and serializes that callback's proxy operations for the lease lifetime.
- Defines normal callback completion at the pool's registered settlement
  reaction, so callback-pre-registered Promise reactions remain inside the
  lease and the proxy is invalid before `do` delivers the callback outcome.
- Treats the lease as scheduling isolation only: state mutations persist and
  are not rolled back when a callback rejects.

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
│   ├── runtime-target.js
│   └── targets.json
└── dist/
```

`native/index.js` is a platform-neutral loader shipped in the main package.
The release pipeline generates one optional npm package per entry in
`native/targets.json`; each generated package contains exactly one `.node`
addon. The main package pins every optional package to its own exact version,
and the loader verifies both package metadata and the version embedded in the
Rust addon before exposing it. No host-specific binary is committed to or
packed inside the main package.

The declared targets are macOS arm64 and x64; Linux arm64 and x64 with glibc;
Linux arm64 and x64 with musl; and Windows x64 with MSVC. Linux runtime
selection includes libc as well as OS and architecture. Future matrix changes
must extend the same target metadata, loader selection, package validation, and
per-runtime artifact-smoke contract.

## Ownership Boundaries
- Rust owns engine correctness and low-level conversion primitives.
- TypeScript owns worker protocol, worker-slot leasing, cancellation
  orchestration, and high-level lifecycle semantics.
- `EngineHandle.create()` owns a Worker from successful Worker construction
  through successful initialization. A failed transaction tears down that
  unpublished Worker before rejecting; a successful transaction transfers the
  live Worker and its ordinary listeners to the returned handle.
- Public API typing is owned by TypeScript package surface (`dist/index.d.ts`), not by generated native d.ts alone.

Pre-Worker argument validation and a synchronous Worker-constructor throw do
not establish `EngineHandle` ownership. Failed-create cleanup covers an
initialization send that throws, but generic atomic `postMessage` rollback for
ordinary handle and pool requests remains FR-NODE-008. The shared completion
barrier for concurrent public `close()` calls remains FR-NODE-010; unpublished
failed-create teardown does not define that behavior.

## Design Constraints
1. Canonical value wire schema must be single-source-of-truth.
2. Sync and async layers must not diverge semantically unless explicitly documented.
3. Error behavior must be class-stable across boundaries.
4. Lifecycle behavior (`close`, dispose, post-close failures) must be deterministic.
5. One worker slot is the isolation unit: unrelated pool work must not execute
   there while a `do` callback owns its lease, even when it targets another
   named engine.

## Risk Seams (Must Receive Focused Review)
1. Symbol/value conversion across worker boundaries.
2. Error class mapping across native and workers.
3. Cancellation semantics for queued vs in-flight operations.
4. `EngineHandle.create()` ownership transfer and exactly-once failed-init
   cleanup, including secondary termination failure reporting.
5. `EnginePool.do()` lease admission, proxy lifetime, and reentrant calls.
6. `EnginePool.close()` behavior under active callbacks and concurrency.
7. Public TS API shape drift (`undefined` exports, mismatched unions).
8. JavaScript/native package version skew or a missing optional native package.

## Delivery Model
Reimplementation should be staged and gated:
1. Native sync correctness and typing.
2. EngineHandle transport and cancellation.
3. EnginePool concurrency semantics, including exclusive callback leases.
4. Packaging and distribution hardening.

Each stage is complete only when its corresponding rows in the Conformance Matrix are `PASS` and required tests from the Test Specification are green.
