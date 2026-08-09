# TypeScript Binding Architecture (Revised)

Date: 2026-04-11
Updated: 2026-08-09 (FR-NODE-008 atomic request-send rollback)
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
  the cause of an `Error` that permits cleanup to define or redefine an own
  writable/configurable cause property. Attachment is best-effort for errors
  that reject that descriptor update and for non-`Error` primaries because
  preserving exact identity takes precedence.
- Treats every ordinary request registration plus main-to-Worker send as one
  transaction. A synchronous send failure removes only that request and its
  abort listener, rejects with the exact thrown value, and leaves the returned
  handle and its shared Worker listeners usable.
- Provides Promise-based API matching `Engine` semantics where applicable.

### Layer 3: `EnginePool` (multi-worker concurrency)
- TypeScript orchestration over multiple workers.
- Defaults to one Worker and accepts only safe-integer thread counts in the
  fixed inclusive range `1..64`. Validation throws synchronously before the
  asynchronous creation transaction or Worker ownership begins; counts are
  never coerced or clamped, and there is no large-pool override.
- Dispatches work round-robin across worker slots.
- Supports stateless one-shot evaluation plus stateful proxy operations.
- Gives every slot an explicit running, failed, terminating, or closed state.
  The first unexpected Worker `error` or `exit` observed by a returned pool
  establishes one pool-wide terminal failure; an `error` retains the exact
  emitted object, while an unexpected exit creates one stable error for its
  code. Later terminal signals cannot replace that primary failure.
- Does not respawn a failed Worker. Recreating a slot would silently discard
  the mutable engines owned by that Worker, so recovery requires constructing
  a new pool explicitly.
- Rejects every request and lease already assigned to the failed slot,
  including its undispatched root and lease-private queues. Work already
  accepted on another healthy slot remains eligible to finish through that
  slot's existing FIFO, and an already-admitted healthy active lease may keep
  accepting owner calls until its callback settles. All later `evaluate` and
  `do` admissions fail before round-robin selection, request allocation,
  listener registration, or Worker dispatch.
- Gives each `do` callback an exclusive, FIFO lease over its selected worker
  slot and serializes that callback's proxy operations for the lease lifetime.
- Defines normal callback completion at the pool's registered settlement
  reaction, so callback-pre-registered Promise reactions remain inside the
  lease and the proxy is invalid before `do` delivers the callback outcome.
- Treats the lease as scheduling isolation only: state mutations persist and
  are not rolled back when a callback rejects.
- Does not forcibly settle arbitrary JavaScript performed by an admitted `do`
  callback on the failed slot. Its pending or queued proxy operations reject,
  later proxy sends through that callback fail fast, and the existing callback
  release barrier remains in force until the callback itself settles.

An ordinary Worker response containing an engine/protocol error rejects only
the matching request and leaves the pool usable. An `exit` caused after
`EnginePool.close()` deliberately starts Worker termination is likewise an
expected lifecycle transition, not a pool failure.

A synchronous main-to-Worker send failure is also request-local, not a Worker
terminal event. The pool conditionally removes the exact pending entry,
restores its in-flight capacity, removes that request's abort listener, wakes
pending-drain waiters, and continues the active lease-private or root FIFO. It
does not respawn, replay, or move the request to another slot, release an active
lease, or rewind request-ID and round-robin admission history. Repeated failed
queued sends are skipped until one request is accepted or the applicable FIFO
is empty.

The internal pool dispatch primitive is total with respect to synchronous
`postMessage` failure. It returns `false` only when that invocation still owned
the exact pending entry and performed rollback; it returns `true` when the send
was accepted or an earlier synchronous settlement already removed the entry.
An immediate caller applies the normal queue-drain transition after `false`.
The queue drainer handles `false` by iteration, not recursive redispatch, so
consecutive failed sends cannot escape the dispatcher or grow the call stack.

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
- `EnginePool` owns every returned pool slot until close completes. A terminal
  slot transition clears pending counters and all root/lease work assigned to
  that slot, removes its request and abort listeners, and wakes any close
  waiter. The pool retains failed Worker objects only so explicit close can
  finish their lifecycle alongside the remaining healthy Workers.
- Invalid EnginePool thread counts are rejected before any Worker is
  constructed, so no pool Worker ownership or failed-create cleanup begins.
- Once a host request is registered, its pending-map entry, pool in-flight unit,
  and request-owned abort listener form one ownership unit until a response,
  terminal event, close path, abort-before-dispatch, or synchronous send
  rollback settles it. Rollback is allowed only while the map still contains
  the same entry for that ID, so a synchronously emitted response or terminal
  event wins without double cleanup.
- Public API typing is owned by TypeScript package surface (`dist/index.d.ts`), not by generated native d.ts alone.

Pre-Worker argument validation and a synchronous Worker-constructor throw do
not establish `EngineHandle` ownership. Failed-create cleanup covers an
initialization send that throws. FR-NODE-008 adds request-local rollback to
ordinary handle sends and every pool send, including pool initialization, but
does not change failed-create termination ownership. The shared completion
barrier for concurrent public `close()` calls remains FR-NODE-010; unpublished
failed-create teardown does not define that behavior.

EnginePool terminal cleanup removes abort listeners from work discarded by a
Worker fault, as FR-NODE-005 requires. FR-NODE-008 removes a queued listener
when that request's dispatch itself throws; generic listener cleanup after a
successful queued dispatch remains FR-NODE-006. Abort-driven proxy lifetime and
mutation semantics remain FR-NODE-009; queue capacity and metrics remain
FR-NODE-011. FR-NODE-005 and FR-NODE-008 must wake a close waiting on
bookkeeping they clear, but neither adds the shared concurrent-close completion
Promise assigned to FR-NODE-010. Worker-to-main response sends own no host
request registration and are outside FR-NODE-008.
The `1..64` construction bound limits one pool's Worker allocation; it does not
bound queued work or define the backpressure policy assigned to FR-NODE-011.

## Design Constraints
1. Canonical value wire schema must be single-source-of-truth.
2. Sync and async layers must not diverge semantically unless explicitly documented.
3. Error behavior must be class-stable across boundaries.
4. Lifecycle behavior (`close`, dispose, post-close failures) must be deterministic.
5. One worker slot is the isolation unit: unrelated pool work must not execute
   there while a `do` callback owns its lease, even when it targets another
   named engine.
6. One unexpected Worker terminal event poisons future admission for the whole
   pool; silent slot replacement and partially available round-robin behavior
   are not recovery mechanisms.
7. EnginePool Worker count is a fixed, non-overridable safe-integer range of
   `1..64`, validated synchronously before Worker construction.
8. Every registered main-to-Worker request has an exactly-once settlement path
   when `postMessage` throws synchronously; transport admission history is never
   rewound or replayed.

## Risk Seams (Must Receive Focused Review)
1. Symbol/value conversion across worker boundaries.
2. Error class mapping across native and workers.
3. Cancellation semantics for queued vs in-flight operations.
4. `EngineHandle.create()` ownership transfer and exactly-once failed-init
   cleanup, including secondary termination failure reporting.
5. `EnginePool.do()` lease admission, proxy lifetime, and reentrant calls.
6. First-event-wins EnginePool Worker failure cleanup across pending requests,
   both queue levels, leases, listeners, counters, and close waiters.
7. `EnginePool.close()` behavior under active callbacks and concurrency.
8. Public TS API shape drift (`undefined` exports, mismatched unions).
9. JavaScript/native package version skew or a missing optional native package.
10. EnginePool construction limits and validation-before-ownership ordering.
11. Synchronous request-send rollback across handle run listeners, pool
    initialization, both pool FIFOs, in-flight counts, lease calls, terminal
    races, and close waiters.

## Delivery Model
Reimplementation should be staged and gated:
1. Native sync correctness and typing.
2. EngineHandle transport and cancellation.
3. EnginePool concurrency semantics, including exclusive callback leases.
4. Packaging and distribution hardening.

Each stage is complete only when its corresponding rows in the Conformance Matrix are `PASS` and required tests from the Test Specification are green.
