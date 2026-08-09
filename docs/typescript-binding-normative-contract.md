# TypeScript Binding Normative Contract (Revised)

Date: 2026-04-11
Updated: 2026-08-09 (FR-NODE-011 bounded pool backpressure)
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
   - `EnginePoolQueueFullError` (class)
2. These exports `MUST NOT` be typed as possibly `undefined` in public `.d.ts`.
3. Enums in public package declarations `MUST` be regular TS enums, not `const enum`.

### 2.2 Public Types
1. `ClipsValue` `MUST` include `FerricSymbol` in the public API type union.
2. Wire-only transport types `MUST NOT` replace public API value types.
3. Public API examples from this contract `MUST` compile under `tsc --strict`.
4. The package `MUST` export `FactId = bigint` and
   `FactIdInput = FactId | number`.
5. The package `MUST` export `EnginePoolOptions`, `EnginePoolMetrics`, and
   `EnginePoolSlotMetrics` as the public construction and scheduling-snapshot
   types defined in section 4.5.

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
8. The initialization-send case in item 5 defines failed-create ownership.
   Ordinary request-send rollback is governed by section 8.1. Items 5-7 also
   do not define the completion barrier for concurrent public `close()` calls,
   which remains FR-NODE-010.
9. `EngineHandle.close()` `MUST` be idempotent.
10. `EngineHandle` `MUST` support `[Symbol.asyncDispose]()` and delegate to `close()`.

### 4.3 EnginePool
1. `EnginePool.create(..., { threads })` `MUST` default to `threads = 1` only
   when `threads` is omitted or `undefined`. Every explicitly supplied count
   `MUST` satisfy `Number.isSafeInteger(threads)` and be in the inclusive range
   `1..64`. The count `MUST NOT` be coerced or clamped, and no option may
   override the upper bound. An invalid count `MUST` throw `RangeError`
   synchronously, before `create()` returns a Promise, inspects engine specs,
   constructs a Worker, or registers initialization bookkeeping.
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
    its N-10 owner-dispatch rights until its callback settles, except that its
    own cancellation signal closes future proxy admission under section 7.3.
    The pool `MUST NOT` replay failed-slot work on that healthy slot, and the
    healthy slot remains owned by the pool until close.
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
13. Synchronous main-to-Worker request-send rollback `MUST` follow section 8.1.
    Cancellation-time proxy admission and ownership `MUST` follow section 7.3.
    Generic queued AbortSignal-listener cleanup after a successful root
    dispatch remains FR-NODE-006; concurrent close callers sharing one
    completion Promise remains FR-NODE-010; and bounded admission and metrics
    follow section 4.5. FR-NODE-005 owns the corresponding cleanup only when a
    Worker terminal event occurs.

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
11. Section 7.3's abort boundary is an additional proxy-admission boundary, not
    a callback-settlement boundary. It `MUST NOT` shorten this lease: until the
    callback actually settles and its accepted calls drain, the slot `MUST NOT`
    run unrelated work and the release transition `MUST` occur exactly once.

### 4.5 EnginePool Bounded Admission and Metrics

1. The public pool options type `MUST` be:

```ts
interface EnginePoolOptions {
  threads?: number;
  queueCapacity?: number;
}
```

   `EnginePool.create(specs, options?)` `MUST` use `queueCapacity = 1024` only
   when the field is omitted or `undefined`. Every explicit value `MUST`
   satisfy `Number.isSafeInteger(queueCapacity)` and be nonnegative. Zero is a
   valid no-wait configuration; JavaScript `-0` `MUST` normalize to `0`.
2. Construction `MUST` validate `threads` first under section 4.3 item 1 and
   then `queueCapacity`. Invalid capacity `MUST` throw
   `RangeError("EnginePool.create: 'queueCapacity' must be a non-negative safe integer")`
   synchronously, before spec inspection, Promise creation, Worker
   construction, or initialization bookkeeping.
3. Capacity is per selected worker slot, not pool-wide. One slot `MUST` share
   its configured budget across:
   - queued root `evaluate` requests;
   - queued, not-yet-admitted `do` lease requests; and
   - queued requests already accepted by that slot's active lease.

   A dispatched request and the admitted callback/lease itself `MUST NOT`
   consume a queue unit. Therefore at most `threads * queueCapacity` entries
   may be waiting across a pool, apart from already-dispatched work and
   arbitrary JavaScript retained by a callback.
4. Work that can dispatch immediately or acquire an idle lease immediately
   `MUST` remain admissible when `queueCapacity` is zero or the selected slot's
   waiting budget is otherwise full. Only work that would add a waiting entry
   is capacity-gated.
5. If waiting work selects a slot whose combined root and lease-private queue
   depth equals `queueCapacity`, its public or proxy call `MUST` return a
   rejected Promise containing the section 6.1 `EnginePoolQueueFullError`.
   Overflow `MUST NOT` escape as a synchronous public throw.
6. Overflow is reject-only. The pool `MUST NOT` wait, time out, probe another
   slot, retry, replay, reschedule, or rewind the completed round-robin
   selection. Selection therefore advances normally even when admission
   rejects, and available capacity on another slot does not rescue the call.
7. Existing API-specific gates retain precedence over overflow, including
   same-pool reentry, inactive proxy lifetime, closed or non-running state,
   pool terminal failure, abort, argument validation, and synchronous payload
   preprocessing. Only after those applicable gates succeed may a call that
   needs to wait test capacity. An already-full slot `MUST` reject before
   request-ID allocation, lease construction, abort-listener registration,
   pending or lease-call accounting, queue mutation, or `postMessage`.

   `evaluate` and proxy `run` `MUST` install their method-specific cooperative
   cancellation listener only after that capacity gate and before request-ID
   allocation or `postMessage`. Because `signal.addEventListener` is
   replaceable JavaScript, the listener hook may synchronously change pool
   state or admit nested work. The outer call `MUST` then reapply its applicable
   lifetime, closed/non-running, terminal, and abort gates, recompute whether it
   would dispatch or wait, and retest current capacity before allocating an ID.
   Nested work that consumed the available position during that hook linearizes
   first; the outer call `MUST` reject with the section 6.1 queue-full error and
   remove its transient cooperative listener. Abort observed by the post-hook
   signal gate retains precedence over that later capacity test.
8. An overflow rejection `MUST` increment exactly one lifetime queue-full
   rejection count for the selected slot. It `MUST NOT` enter either FIFO or
   alter the relative order of work already accepted there. It owns no request,
   lease, pending or lease-call unit, queue entry, waiter, or Worker send. An
   already-full rejection `MUST NOT` install a request listener. The post-hook
   overflow exception in item 7 may install and then remove its cooperative
   listener, but no overflow `MUST` retain a listener after settlement.
9. The source of truth for current capacity usage `MUST` be structural queue
   ownership: the root queue length plus the active lease-private queue length.
   After final admission, a queued root request or queued lease admission with
   a signal `MUST` enter its FIFO before invoking the replaceable registration
   hook for its dequeue-cancellation listener. That structural reservation
   linearizes first, counts toward capacity, and preserves FIFO while the hook
   synchronously reenters the pool.

   If registration throws while the entry remains queued, Ferric `MUST` remove
   and reclaim it, reject its Promise with the exact thrown value, release a
   queued lease exactly once, and continue eligible FIFO work. If hook reentry
   has already dequeued, dispatched, admitted, terminally discarded, or
   close-rejected the entry, that earlier transition and its own outcome
   `MUST` win; the registration throw `MUST NOT` roll it back or settle it
   again. Ferric `MUST` attempt to detach any listener made stale by that
   synchronous window. A replaceable removal hook's throw `MUST NOT` replace
   the entry's already-owned outcome; post-registration reconciliation `MUST`
   retry a detach attempted during synchronous abort, while persistently
   hostile removal remains best-effort. An abort observed while the entry is
   still queued `MUST` remove and reclaim it with `AbortError`. This hook-window
   reconciliation is part of FR-NODE-011 admission; it does not add
   FR-NODE-006's generic listener cleanup after an ordinary successful queued
   root dispatch.

   One queue unit is reclaimed exactly when its entry is removed or dequeued:
   - dispatch reclaims before its `postMessage` attempt;
   - abort removes and reclaims a queued root request or queued lease
     admission;
   - section 7.3 cancellation `MUST NOT` remove or reclaim a lease-private
     owner request already accepted before abort;
   - Worker terminal cleanup reclaims every discarded entry in both queues;
   - close reclaims rejected root work, while an admitted callback's accepted
     owner work retains sections 4.4 and 7.3 and reclaims as it dispatches or
     is terminally discarded; and
   - response settlement changes in-flight work, not queue depth, because the
     matching unit was reclaimed at dispatch.
10. Section 8.1 send rollback `MUST NOT` reclaim a queue unit a second time.
    Its request-local pending/in-flight cleanup and applicable FIFO progress
    remain required after the queued entry was already removed for dispatch.
11. The public scheduling snapshot types `MUST` be:

```ts
interface EnginePoolSlotMetrics {
  readonly slotIndex: number;
  readonly queued: number;
  readonly inFlight: number;
  readonly rejected: number;
}

interface EnginePoolMetrics {
  readonly queueCapacity: number;
  readonly queued: number;
  readonly inFlight: number;
  readonly rejected: number;
  readonly slots: readonly EnginePoolSlotMetrics[];
}
```

12. `EnginePool.metrics(): EnginePoolMetrics` `MUST` be synchronous and
    side-effect-free. Each invocation `MUST` return a fresh detached snapshot;
    casting around readonly types and mutating that value `MUST NOT` mutate the
    pool or a later snapshot. Runtime `Object.freeze` is not required.
13. Snapshot `queueCapacity` is the configured per-slot limit. Top-level
    `queued`, `inFlight`, and `rejected` are sums of the corresponding stable
    `slotIndex` entries. `rejected` counts only queue-full admissions since
    successful pool creation, not abort, response, send, terminal, or close
    failures.
14. `metrics()` is observation, not admission. It `MUST` remain callable from
    an active callback, during shutdown, after a Worker terminal failure, and
    after close without triggering the same-pool reentrancy or lifecycle
    guards. A completed close snapshot `MUST` report zero queued and in-flight
    work.
15. This section bounds retained queue-entry count, not request payload bytes,
    arbitrary callback-owned JavaScript, or Worker/native memory. It adds no
    admission wait, timeout, caller-selected policy, cancellation semantics,
    generic successful-dispatch listener cleanup, or concurrent-close Promise
    sharing beyond the contracts owned by FR-NODE-006, FR-NODE-009, and
    FR-NODE-010.

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
- `EnginePoolQueueFullError`

`EnginePoolQueueFullError` `MUST` extend `FerricError`, set `name` to
`EnginePoolQueueFullError`, set `code` to `FERRIC_POOL_QUEUE_FULL`, and use the
exact message `EnginePool queue is full`. Its public constructor `MUST` be
`(capacity: number, queued: number, slotIndex: number)`, and it `MUST` expose
those three values as readonly fields describing the selected slot at the
rejection point. Under the section 4.5 invariant, `queued` equals `capacity`.

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
5. `EnginePoolQueueFullError` is a main-thread scheduler error. It `MUST NOT`
   be added to or reconstructed through the Worker `ERROR_REGISTRY` merely to
   satisfy section 4.5.

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
1. Cancellation `MUST` reject with a `DOMException` whose `name` is
   `AbortError` and whose message is `The operation was aborted`.
2. After the existing same-pool reentry, closed-pool, and pool-terminal
   guards, an otherwise-admissible `do` whose signal is already aborted at
   public entry `MUST` reject before round-robin selection, lease construction,
   listener registration, callback invocation, request-ID allocation, or
   Worker dispatch.
3. If abort wins while lease admission is queued, that waiter `MUST` be removed,
   its callback `MUST NOT` run, its lease `MUST` be marked released exactly
   once, and its admission listener `MUST` be removed. The same result applies
   when abort wins the admission-to-callback microtask race after an idle lease
   was assigned but before callback invocation. A close or Worker-terminal
   transition that already settled the waiter retains its existing first
   settlement instead.
4. Once the callback begins, the first event observed between the signal's
   abort transition and the pool's registered reaction to callback settlement
   `MUST` fix the outer `do` outcome. If callback settlement is observed first,
   its value or error wins, the outer abort listener is removed immediately,
   and a later signal change `MUST NOT` replace that outcome while accepted
   calls drain. If abort is observed first, the outer Promise `MUST` reject
   promptly with the item 1 `AbortError`; that rejection `MUST NOT` wait for
   arbitrary callback JavaScript or already-accepted proxy calls.
5. Abort observed first `MUST` immediately close admission through the active
   callback proxy. Every proxy method invoked afterward `MUST` return a
   rejected Promise with the item 1 `AbortError`; it `MUST NOT` throw that
   public failure synchronously or reach method-specific argument validation,
   request-ID allocation, lease-call accounting, abort-listener registration,
   either queue, pending registration, or `postMessage`.
   Each request-bearing method `MUST` also recheck the signal after its
   synchronous preprocessing and immediately before request-ID allocation. If
   abort occurs during preprocessing and that preprocessing completes, this
   final gate `MUST` reject with the item 1 `AbortError` before the request is
   accepted or any request bookkeeping begins.
6. A proxy request is accepted when it passes that final lifetime, slot-state,
   and signal gate immediately before request-ID allocation. Every request
   accepted before abort, including work waiting in the lease-private FIFO,
   `MUST` keep its invocation order and ordinary response, send-failure, or
   Worker-terminal outcome. Such work `MAY` finish mutating engine state after
   the signal has aborted and the outer Promise has rejected; cancellation
   supplies no rollback. The `do` signal `MUST NOT` be used to dequeue or
   replace the outcome of that accepted work.
7. An accepted proxy `run` `MUST` additionally set its existing out-of-band
   abort flag when the signal fires. Absent a send, response, or Worker-terminal
   failure, it `MUST` resolve with the partial `HaltRequested` projection after
   the worker stops continuation chunks. Host abort `MUST NOT` set the native
   halt latch merely to represent this result.
8. Abort is not callback preemption. The callback remains admitted work and its
   whole worker slot `MUST` remain exclusively leased until the callback
   actually settles and every accepted proxy call drains. That barrier owns one
   idempotent release transition; abort `MUST NOT` release the slot early, and
   unrelated root or lease work `MUST NOT` enter it merely because the outer
   `do` Promise has rejected.
9. Proxy failure precedence `MUST` be evaluated in this order:
   - after pool-observed callback settlement or lease release, reject with
     `Error("EngineProxy is no longer valid outside its EnginePool.do callback")`;
   - while the lease remains active on a failed slot, reject with the exact
     retained pool terminal failure;
   - while the active slot is no longer running because close owns it, reject
     with the deterministic closed-pool error;
   - while the lease and slot remain active/running but the signal has aborted,
     reject with the item 1 `AbortError`; and
   - only after those gates may method validation and request acceptance occur.

   Consequently an abort-first retained proxy rejects with `AbortError` while
   its callback remains active and with the lifetime error after callback
   settlement. A failed-slot error takes precedence while that callback is
   active. Neither transition replaces the already-fixed outer `do` outcome.
10. Once an accepted request begins its `postMessage` attempt, section 8.1's
    exact-entry first-settlement and exact synchronous-send-error rules
    `MUST` win for that request even if abort occurs during the attempt. Abort
    may independently fix the outer `do` outcome under item 4.
11. The pool `MUST` remove its outer `do` abort listener when abort or callback
    settlement wins and remove each accepted `run` flag listener when that run
    settles. Cancellation completion, callback release, and queued-admission
    cancellation `MUST` return their owned listener and lease bookkeeping to
    baseline exactly once.
12. Items 1-11 do not require generic cleanup of a root request's queue
    listener after successful dispatch (FR-NODE-006), do not change concurrent
    `close()` Promise sharing (FR-NODE-010), and do not let abort dequeue owner
    work already accepted under section 4.5. Capacity admission for a later
    proxy call occurs only after this section's gates succeed.

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

### 8.1 Synchronous Main-to-Worker Request-Send Failure

1. Main-to-Worker submission `MUST` be treated as a transaction from host
   request registration until `postMessage` accepts the request. This section
   governs ordinary `EngineHandle` methods, `EngineHandle.run`, every
   `EnginePool` initialization/root/lease request, and both immediate and
   queued pool dispatch. `EngineHandle` initialization remains additionally
   governed by the failed-create ownership requirements in section 4.2.
2. Existing checks that run before registration retain their existing
   precedence, including the proxy ordering in section 7.3 item 9. A closed or
   terminal object, inactive or canceled proxy, invalid argument, or
   dequeuable pre-abort `MUST` fail without calling `postMessage`. For every
   valid ordinary or proxy call that reaches request submission, a synchronous
   `postMessage` failure `MUST` reject its Promise; it `MUST NOT` escape as a
   separate synchronous public throw. A valid-thread `EnginePool.create()`
   whose initialization send fails `MUST` return a Promise that rejects.
3. If `postMessage` throws synchronously while the pending map still associates
   that request ID with the same registered entry, rollback `MUST`, before the
   rejection is observable:
   - delete that exact pending entry;
   - decrement that request's pool in-flight unit exactly once, when present;
   - remove every abort listener owned by that request;
   - reject the registered Promise exactly once with the exact thrown value,
     preserving its identity without reconstruction or wrapping; and
   - notify any pending-drain waiter whose condition is now satisfied.
4. Rollback `MUST` be conditional on ownership of that same `(request ID,
   entry)`. If a synchronous response, Worker terminal event, close path, or
   other settlement has already removed or replaced it, that first settlement
   `MUST` win and the send catch `MUST NOT` reject, decrement, notify, or drain
   the request a second time. If send rollback wins and a later terminal event
   occurs, that request retains the send failure while the terminal event
   governs remaining and future work under section 4.3.
5. An ordinary send failure on a returned `EngineHandle` `MUST NOT` close or
   terminate its Worker or detach the handle's shared Worker listeners.
   `EngineHandle.run` `MUST` remove its request-owned AbortSignal listener. A
   subsequent valid handle request `MUST` remain eligible to succeed.
6. An ordinary pool send failure `MUST` be request-local. It `MUST NOT` mark the
   slot failed, establish or replace the pool terminal error, terminate or
   respawn a Worker, replay the request, or move it to another slot. The
   allocated request ID and completed round-robin selection `MUST NOT` be
   rewound or reused.
7. After an owned pool rollback restores its pending/in-flight ownership, the
   pool `MUST` continue the same scheduling transition as an immediate rejected
   response: while a lease is active, continue its lease-private FIFO;
   otherwise continue the root FIFO. A queued entry's section 4.5 capacity unit
   was already reclaimed when it was removed for dispatch and `MUST NOT` be
   reclaimed twice. Repeated queued send failures `MUST` be rolled back without
   escaping the dispatcher until a request is accepted or the applicable FIFO
   is empty.
   A failed proxy operation settles only that operation; it `MUST NOT` release
   or invalidate the lease, and later already-accepted owner operations remain
   eligible to drain in invocation order.
8. A pool initialization send failure `MUST` reject pool creation with the
   exact thrown value and enter the existing failed-create transaction, which
   terminates every unpublished Worker constructed by that attempt. It
   `MUST NOT` publish a partially initialized or terminal pool.
9. Once a send attempt begins, its synchronous failure wins over an abort that
   occurs later. This section requires listener removal for a request whose
   send failed; generic listener cleanup after a successful queued root
   dispatch remains FR-NODE-006. Section 7.3 governs proxy admission and
   accepted-mutation semantics, and a send failure alone does not alter them.
10. Worker-to-main response `postMessage` calls create no host pending entry and
    are outside this rollback contract. Their failure remains governed by the
    response protocol and Worker error/exit lifecycle. Queue admission and
    structural capacity reclamation follow section 4.5; the Promise shared by
    concurrent public close callers remains FR-NODE-010. Rollback `MUST` still
    wake existing close waiters whose pending condition it satisfies.

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
