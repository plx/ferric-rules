# TypeScript Binding Normative Contract (Revised)

Date: 2026-04-11
Updated: 2026-08-09 (FR-NODE-005 terminal EnginePool worker faults)
Status: Draft for reimplementation

Companion documents:
- [Architecture](/Users/prb/conductor/workspaces/ferric-rules/santo-domingo/docs/typescript-binding-architecture.md)
- [Conformance Matrix](/Users/prb/conductor/workspaces/ferric-rules/santo-domingo/docs/typescript-binding-conformance-matrix.md)
- [Test Specification](/Users/prb/conductor/workspaces/ferric-rules/santo-domingo/docs/typescript-binding-test-spec.md)

## 1. Normative Language
The keywords `MUST`, `MUST NOT`, `SHOULD`, and `MAY` are normative.

If this contract conflicts with legacy design docs, this contract wins.

## 2. Public Package Contract

### 2.1 Exports
1. Package entrypoint `@ferric-rules/node` `MUST` export concrete runtime values:
   - `Engine` (class)
   - `FerricSymbol` (class)
   - `EngineHandle` (class)
   - `EnginePool` (class)
2. These exports `MUST NOT` be typed as possibly `undefined` in public `.d.ts`.
3. Enums in public package declarations `MUST` be regular TS enums, not `const enum`.

### 2.2 Public Types
1. `ClipsValue` `MUST` include `FerricSymbol` in the public API type union.
2. Wire-only transport types `MUST NOT` replace public API value types.
3. Public API examples from this contract `MUST` compile under `tsc --strict`.
4. The package `MUST` export `FactId = bigint` and
   `FactIdInput = FactId | number`.

## 3. Value Conversion Contract

### 3.1 JS -> CLIPS
1. `FerricSymbol` -> CLIPS Symbol.
2. `string` -> CLIPS String (quoted).
3. `number` -> CLIPS Integer when integral and within `i64`; otherwise Float.
4. `bigint` -> CLIPS Integer.
5. `boolean` -> CLIPS Symbols `TRUE` / `FALSE`.
6. `Array` -> CLIPS Multifield recursively.
7. `null` and `undefined` -> CLIPS Void.

### 3.2 CLIPS -> JS
1. CLIPS Symbol -> `FerricSymbol`.
2. CLIPS String -> `string`.
3. CLIPS Integer in safe range `[-(2^53-1), 2^53-1]` -> `number`.
4. CLIPS Integer outside safe range -> `bigint`.
5. CLIPS Float -> `number`.
6. CLIPS Multifield -> `ClipsValue[]` recursively.
7. CLIPS Void and ExternalAddress -> `null`.

### 3.3 Fact Identifiers
1. Every fact ID returned by `assertString`, `assertFact`, or `assertTemplate`,
   and every `Fact.id` snapshot property, `MUST` be a `FactId` (`bigint`),
   regardless of magnitude.
2. Every API that accepts a fact ID `MUST` accept `FactIdInput` and apply the
   same conversion rules.
3. A `bigint` input `MUST` be accepted only when it is in the unsigned 64-bit
   range.
4. A legacy `number` input `MUST` be accepted only when it is finite,
   integral, non-negative, and no greater than `Number.MAX_SAFE_INTEGER`.
5. Invalid and unsafe numeric inputs `MUST` fail with a targeted argument
   error and `MUST NOT` be rounded or truncated.
6. Fact-ID representation is distinct from the adaptive `number`/`bigint`
   representation for CLIPS integer values and from run counts and limits.

### 3.4 Worker Boundary
1. Worker transport `MUST` preserve the semantics in 3.1, 3.2, and 3.3.
2. Canonical symbol wire representation `MUST` be:

```ts
{ __type: "FerricSymbol", value: string }
```

3. Transport layers `MUST` convert to/from this wire representation transparently.
4. Callers of `EngineHandle` and `EnginePool` `MUST NOT` need manual symbol marshalling.
5. `EngineHandle` and `EnginePool` `MUST` preserve `FactId` values as `bigint`
   through structured clone in both request and response directions.

## 4. API Semantics

### 4.1 Engine
1. `Engine` methods are synchronous and execute on caller thread.
2. `Engine.fromSource(source, options)` `MUST` be equivalent to `new Engine(options); load(source); reset();`.
3. `Engine.close()` `MUST` be idempotent.
4. After `close()`, all operational methods/getters `MUST` throw deterministic errors.
5. `Engine` `MUST` support `[Symbol.dispose](): void`, equivalent to `close()`.

### 4.2 EngineHandle
1. `EngineHandle.create({ source })` `MUST` load and reset source before resolve.
2. `EngineHandle.create({ snapshot })` `MUST` restore from snapshot before resolve.
3. `source` and `snapshot` `MUST` be mutually exclusive; passing both `MUST` reject with argument error.
4. Pre-Worker validation and a synchronous Worker-constructor throw `MUST`
   reject without establishing `EngineHandle` ownership of a Worker. The
   constructor failure `MUST` be propagated unchanged because no Worker was
   returned to the handle for cleanup.
5. Once Worker construction succeeds, `EngineHandle.create()` `MUST` own that
   Worker until initialization either succeeds or its failed-create cleanup
   completes. Any setup or initialization failure after that point `MUST`:
   - remove any registered initialization request from pending bookkeeping and
     settle that request exactly once,
   - remove the response, error, and exit listeners registered for the
     unpublished handle,
   - invoke `terminate()` exactly once and await it before rejecting, and
   - leave no live Worker owned by the rejected creation attempt.
6. The primary setup or initialization error object `MUST` remain the
   `create()` rejection, preserving its identity, class, and message. A
   simultaneous termination failure `MUST NOT` replace it. When the primary is
   an `Error` on which cleanup can define or redefine an own writable and
   configurable `cause` property, the termination failure `MUST` be attached as
   its `cause`; if the primary already has a replaceable cause, that cause and
   the termination failure `MUST` both remain available through an aggregate
   cause. For an `Error` that does not permit that descriptor update (including
   a frozen error or one with a non-configurable own cause), or a non-`Error`
   thrown value, cause attachment is best-effort and falls outside that
   guarantee. Preserving the exact primary identity `MUST` take precedence when
   attachment is impossible.
7. Successful initialization `MUST` transfer the live Worker and its ordinary
   request listeners to the returned `EngineHandle`; failed-create cleanup
   `MUST NOT` run on that success path.
8. The initialization-send case in item 5 defines only failed-create
   ownership. Atomic synchronous-`postMessage` rollback for ordinary handle
   requests and pool requests remains FR-NODE-008. Items 5-7 also do not
   define the completion barrier for concurrent public `close()` calls, which
   remains FR-NODE-010.
9. `EngineHandle.close()` `MUST` be idempotent.
10. `EngineHandle` `MUST` support `[Symbol.asyncDispose]()` and delegate to `close()`.

### 4.3 EnginePool
1. `EnginePool.create(..., { threads })` `MUST` default to `threads = 1` when omitted.
2. `evaluate(spec, req)` `MUST` perform: `reset -> assert facts -> run -> collect facts/output`.
3. `EnginePool.close()` `MUST`:
   - reject new requests after close starts,
   - allow already-dispatched requests and admitted `do` callbacks to settle,
   - then terminate workers.
4. `EnginePool.close()` `MUST` be idempotent.
5. `EnginePool` `MUST` support `[Symbol.asyncDispose]()` and delegate to `close()`.
6. Every pool Worker slot `MUST` have an explicit `running`, `failed`,
   `terminating`, or `closed` state. A Worker `error`, or an `exit` before the
   pool deliberately begins terminating that Worker, `MUST` move a running
   slot to `failed` even when it has no pending request and even when the exit
   code is zero. A Worker response carrying an ordinary request error
   `MUST NOT` fail the slot. An exit caused after the pool deliberately begins
   terminating that Worker `MUST NOT` establish a pool failure.
7. The first unexpected Worker terminal event observed after successful pool
   creation `MUST` establish one immutable pool-wide terminal failure. If the
   event is `error`, the exact emitted error object `MUST` be retained. If it
   is `exit`, the pool `MUST` construct and retain one deterministic `Error`
   describing that exit. Later `error`, `exit`, response, abort, and close
   races `MUST NOT` replace the primary failure or settle affected work twice.
   A later terminal event from another running slot `MUST` still perform that
   slot's local cleanup using the original pool failure.
8. Establishing the terminal failure `MUST` atomically:
   - clear and reject every pending request on the failed slot;
   - set that slot's in-flight count to zero;
   - clear and reject all ordinary requests, lease admissions, and
     lease-private proxy requests already assigned to the failed slot;
   - mark rejected lease admissions released and remove every abort listener
     owned by discarded work;
   - wake any pending-drain waiter used by `close()`; and
   - detach the failed Worker's message, error, and exit listeners.

   Every rejection caused by this transition `MUST` use the retained primary
   failure and occur exactly once.
9. EnginePool `MUST NOT` respawn or silently replace a failed Worker. After the
   pool-wide terminal failure is established, every later `evaluate` or `do`
   admission `MUST` reject with the retained primary failure before
   round-robin selection, request-ID allocation, abort-listener registration,
   or `postMessage`. Callers recover by closing the failed pool and creating a
   new one.
10. Work accepted on another still-running slot before the failure was
    observed, including that slot's existing root FIFO, `MUST` remain eligible
    to finish normally. An already-admitted healthy active lease `MUST` retain
    its N-10 owner-dispatch rights until its callback settles. The pool
    `MUST NOT` replay failed-slot work on that healthy slot, and the healthy
    slot remains owned by the pool until close.
11. A Worker fault `MUST` reject in-flight and queued proxy operations of an
    admitted `do` callback on the failed slot and make any later proxy send
    through that callback fail with the retained pool failure. It `MUST NOT`
    forcibly settle arbitrary JavaScript in that callback, such as an unrelated
    user Promise awaited while the slot is idle. The callback's existing
    lease/release barrier remains in force until the callback settles. Item 10
    continues to govern an admitted callback on another healthy slot.
12. `close()` begun before or after a Worker fault `MUST` be able to observe
    the terminal cleanup, finish terminating all owned Workers, and complete;
    it `MUST NOT` wait forever on pending bookkeeping cleared by the fault.
    This requirement does not define the shared completion barrier for
    concurrent public close calls.
13. Atomic rollback when an ordinary `postMessage` throws synchronously remains
    FR-NODE-008. Generic queued AbortSignal-listener cleanup remains
    FR-NODE-006; cancellation-time proxy invalidation remains FR-NODE-009;
    bounded queue capacity remains FR-NODE-011; and concurrent close callers
    sharing one completion Promise remains FR-NODE-010. FR-NODE-005 owns the
    corresponding cleanup only when a Worker terminal event occurs.

### 4.4 EnginePool.do Worker-Slot Lease

1. Before invoking a `do` callback, the pool `MUST` acquire an exclusive lease
   on the selected worker slot. The lease covers the whole slot, including all
   named engines hosted by that worker, rather than only the requested spec.
2. From callback invocation until the pool's registered reaction to its
   returned Promise begins, no unrelated `do`, `evaluate`, or other pool task
   `MAY` execute on the leased worker. Time spent awaiting non-Ferric work and
   callbacks that issue no proxy calls remain inside the lease lifetime.
3. Lease acquisitions and ordinary work assigned to one worker slot `MUST` be
   admitted in FIFO order. The pool does not guarantee global start or
   completion order across different worker slots.
4. Calls made through the active callback's proxy `MUST` execute serially in
   invocation order, including calls submitted concurrently without awaiting
   the preceding call.
5. Normal callback settlement is the pool-observed boundary at which the
   pool's registered fulfillment or rejection reaction to the returned Promise
   begins. Promise reactions that the callback registered on that Promise
   before returning it execute first under JavaScript's FIFO reaction ordering
   and `MUST` remain inside the lease. Proxy calls they invoke are accepted as
   callback work.
6. At the pool-observed settlement boundary, the proxy `MUST` become invalid
   before `do` delivers the callback's value or error. Every proxy method call
   after that boundary `MUST` reject with the same deterministic lifetime error
   before allocating, validating, or enqueueing a worker request.
7. Proxy calls accepted before the pool-observed settlement boundary `MUST`
   drain in invocation order before the worker-slot lease is released. The
   callback's result or exception is delivered only after that release barrier
   completes.
8. A lease is an execution-isolation boundary, not a database transaction.
   Engine mutations persist after callback fulfillment or rejection; the pool
   performs no automatic rollback.
9. While a callback is active, reentering the same `EnginePool` through
   `do`, `evaluate`, or `close` `MUST` reject deterministically rather than
   wait for the callback's own lease. Calling another `EnginePool` from the
   callback remains supported.
10. A callback whose lease was acquired before `close()` began is admitted
   work. `close()` `MUST` wait for that callback and its accepted proxy calls;
   those proxy calls remain permitted as part of the admitted callback. A
   queued callback that has not acquired a lease is new work and `MUST` reject.
11. Abort-driven proxy invalidation and the settlement rule for work accepted
    before an abort are specified by FR-NODE-009. This lease contract does not
    redefine that cancellation boundary; until the callback actually settles,
    its slot nevertheless `MUST NOT` run unrelated work.

## 5. Run Limit Semantics

1. For `Engine.run(limit?)` and `EngineHandle.run({limit})`:
   - omitted or `undefined` => unlimited,
   - `0` => zero firings,
   - positive integer => max firings.
2. For `EvaluateRequest.limit` in `EnginePool.evaluate`:
   - omitted or `0` => unlimited,
   - positive integer => max firings.
3. These semantics `MUST` be documented and tested explicitly.
4. Every uncanceled public run invocation `MUST` begin with one fresh native
   run. Starting a fresh run clears the previous halt request and action
   diagnostics, but does not otherwise clear working memory or the agenda.
5. `EngineHandle.run({ limit: 0 })` and `EngineProxy.run({ limit: 0 })` `MUST`
   invoke a fresh native zero-limit run rather than synthesize a result. They
   `MUST` fire zero rules, return `LimitReached`, and perform the same fresh-run
   execution-state reset as `Engine.run(0)`.

### 5.1 Worker Logical-Run Batching

1. A worker-backed run `MAY` split one public run into bounded chunks to poll
   for host cancellation.
2. The first executed chunk `MUST` start the fresh logical run. Every later
   chunk in that invocation `MUST` continue that same logical run and `MUST NOT`
   clear its halt request or accumulated action diagnostics.
3. Per-chunk fired counts `MUST` be accumulated into the public run total.
4. Absent host cancellation, synchronous, `EngineHandle`, and `EnginePool`
   execution `MUST` agree in total fired count, halt reason, halted state,
   agenda state, and action diagnostics.
5. If a count-limited chunk's last activation requests a halt, the worker
   `MUST` observe the pending halt before submitting another continuation; it
   `MUST NOT` fire a post-halt activation.
6. After a native chunk returns a terminal reason, that reason `MUST` be
   returned immediately. After `LimitReached`, the worker `MUST` apply this
   precedence before continuing:
   1. an exhausted caller limit returns `LimitReached`, matching synchronous
      execution even if the limit-th activation also set the halt latch;
   2. a host abort stops the logical run under section 7;
   3. a pending native halt returns `HaltRequested`;
   4. otherwise, the worker submits a continuation chunk.
7. A later public `run()` invocation `MUST` start fresh, never continue a prior
   completed or host-canceled invocation.
8. Logical-run continuation is an internal implementation mechanism and
   `MUST NOT` add or change a public method, result type, enum member, or
   versioned API contract.

## 6. Error Contract

### 6.1 Error Classes
The following classes `MUST` exist and be constructible in JS:
- `FerricError`
- `FerricParseError`
- `FerricCompileError`
- `FerricRuntimeError`
- `FerricFactNotFoundError`
- `FerricTemplateNotFoundError`
- `FerricSlotNotFoundError`
- `FerricModuleNotFoundError`
- `FerricEncodingError`
- `FerricSerializationError`

### 6.2 Mapping Rules
1. Native failures `MUST` map to the correct class above.
2. Worker responses `MUST` include stable payload:

```ts
{
  id: number;
  error?: {
    name: string;   // class name above or AbortError
    code: string;   // stable machine code
    message: string;
  }
}
```

3. Worker-side reconstruction `MUST` instantiate the class identified by `name`.
4. Unknown error names `MUST` degrade to `FerricError` while preserving `code` and `message`.

## 7. Cancellation Contract

### 7.1 EngineHandle.run
1. If signal is already aborted before dispatch, `MUST` reject with `AbortError`.
2. In-flight cancellation `MUST` use cooperative logical-run batching with a
   shared, out-of-band abort flag.
3. On in-flight cancellation, the worker `MUST` stop submitting continuation
   chunks and `MUST NOT` call native `halt()` or otherwise set the engine halt
   latch merely to represent host cancellation.
4. The promise `MUST` preserve the existing public projection by resolving
   with a partial `RunResult` and `haltReason = HaltRequested`.
5. Host cancellation does not guarantee an unhalted engine: a completed chunk
   may itself have executed a rule-side halt. The next public run `MUST` start
   fresh and clear the documented execution state.

### 7.2 EnginePool.evaluate
1. If already aborted before dispatch, `MUST` reject with `AbortError`.
2. If aborted while queued and request is not yet dispatched, `MUST` reject with `AbortError`.
3. If aborted during execution, `MUST` use the same cooperative logical-run
   batching and out-of-band cancellation model as `EngineHandle.run`.
4. A partial `EvaluateResult.runResult` produced by in-flight host cancellation
   `MUST` retain the existing `HaltRequested` projection without setting the
   native halt latch merely to represent that cancellation.

### 7.3 EnginePool.do
1. If already aborted before dispatch, `MUST` reject with `AbortError`.
2. If aborted before the pool observes the callback settlement boundary,
   the returned promise `MUST` reject with `AbortError`.
3. Proxy `run` operations issued during `do` `MUST` use the same fresh-run and
   continuation contract when cancellation is active.
4. Rejecting the outer `do` promise for host cancellation `MUST NOT` require
   setting the proxied native engine's halt latch.
5. Rejection of the outer promise does not by itself end the callback's
   worker-slot lease. Unrelated work `MUST` remain excluded until the
   pool-observed callback settlement boundary and accepted-call drain.
6. FR-NODE-009 defines when abort invalidates the proxy, whether an operation
   accepted before abort may mutate engine state, and when cancellation may
   release the lease. FR-NODE-003 defines only the lease and normal
   callback-settlement lifetime rules.

## 8. Worker Protocol Contract

1. Main->worker request shape `MUST` be:

```ts
interface WorkerRequest {
  id: number;
  method: string;
  args: unknown[];
}
```

2. Worker->main response shape `MUST` be:

```ts
interface WorkerResponse {
  id: number;
  result?: unknown;
  error?: {
    name: string;
    code: string;
    message: string;
  };
}
```

3. Request IDs `MUST` be unique per worker slot among in-flight requests.
4. Snapshot payload transfers `SHOULD` use `ArrayBuffer` transfer for zero-copy.

## 9. Packaging and Runtime Load Contract

1. Missing native binaries at runtime `MUST` fail fast for value imports (`Engine`, `FerricSymbol`), not expose optional undefined runtime API.
2. Package `npm test` `MUST` execute non-zero binding tests.
3. CI `MUST` fail if binding test count is zero.
4. The main package `MUST` ship its platform-neutral native loader and declare
   every native payload package as an optional dependency at the exact same
   version as the main package.
5. Every native addon `MUST` embed its Ferric package version. The loader
   `MUST` reject mismatched main-package metadata, native-package metadata, or
   embedded addon versions before exposing an engine.
6. Unsupported targets `MUST` fail with an actionable error that names the
   detected OS and architecture, the detected libc on Linux, and the supported
   target alternatives. A missing optional package `MUST` name the full
   detected target and exact expected package.
7. CI `MUST` pack, install, and execute the exact main and native tarballs from
   a clean consumer directory without a source checkout or native build.
8. The declared target set is the versioned `native/targets.json` file.
   Extending the platform, architecture, or libc matrix requires the
   validation in this contract.
9. Every declared Linux target `MUST` name exactly one npm libc selector. The
   loader and release-artifact smoke `MUST` distinguish glibc from musl and
   fail before loading a native package when libc detection is inconclusive.
10. CI `MUST` build, pack, install, load, and run the exact artifact for every
    declared OS, architecture, and libc combination on a matching runtime.

## 10. Required Test Gating

1. Every requirement ID in the Conformance Matrix sections A-E `MUST` have at least one automated test case.
2. All `FAIL` and `UNKNOWN` statuses from the matrix `MUST` be eliminated before declaring implementation complete.
3. Test suite requirements are defined in [Test Specification](/Users/prb/conductor/workspaces/ferric-rules/santo-domingo/docs/typescript-binding-test-spec.md).
