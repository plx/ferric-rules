# TypeScript Binding Test Specification (Revised)

Date: 2026-04-11
Updated: 2026-08-09 (FR-NODE-011 bounded pool backpressure)
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
  reconstruction, failed-construction Worker ownership, and atomic ordinary
  request-send rollback.

### 2.4 Pool Runtime Unit Tests (`EnginePool`)
- Exercise construction bounds, queueing, dispatch, cancellation states,
  worker-slot lease isolation, cancellation-time proxy admission and accepted
  work, proxy lifetime and ordering, same-pool reentrancy, terminal Worker
  faults, synchronous send rollback, bounded mixed-queue admission and metrics,
  `close()` behavior, and stateless evaluation behavior.

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

### 4.3 Worker Runtime Tests (minimum 35 cases)
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
    rejection, attaches the termination error when cleanup can define or
    redefine an own writable/configurable cause property on the primary
    `Error`, and preserves frozen, non-configurable-cause, or non-`Error`
    primaries by identity when cleanup metadata cannot be attached;
  - duplicate or late protocol/error/exit signals proving request settlement
    and termination each occur exactly once;
  - pre-Worker validation and Worker-constructor throw controls; and
  - a successful construction control proving ownership transfers without
    failed-create teardown.

The D-010 synchronous-send case above applies to initialization ownership.

- Ordinary `EngineHandle` send rollback (`D-011`, `N-13`), including:
  - a real uncloneable ordinary argument and injected throws from the shared
    ordinary-call path and the separately implemented `run` path;
  - calls returning Promises rather than leaking a synchronous public throw,
    with rejection preserving the exact thrown value by identity;
  - the exact pending entry removed before rejection is observable, while the
    failed request ID remains consumed;
  - `run` AbortSignal listener count returning to baseline exactly once and
    ordinary message/error/exit Worker listener counts remaining unchanged;
  - no Worker termination or handle closure, followed by a valid request that
    succeeds on the same Worker; and
  - synchronous response, error, and exit before a later send throw, proving
    the first settlement wins without duplicate rejection or cleanup.

Concurrent public `close()` completion-barrier coverage remains FR-NODE-010.

### 4.4 Pool Runtime Tests (minimum 70 cases)
Must include:
- Evaluate lifecycle (`E-002`).
- Cancellation for pre-abort, queued abort, and in-flight abort (`E-003`,
  `E-004`, `E-005`), including the no-synthetic-native-halt assertion.
- `do()` cancellation behavior (`E-006`).
- Proxy behavior parity (`E-007`).
- `close()` contract (in-flight completion, admitted-callback completion, and
  idempotency) (`E-008`, `E-009`, `E-012`).
- Thread default behavior (`E-001`).
- Synchronous bounded thread-count validation (`E-014`, `N-12`), including:
  - direct, un-awaited calls proving each invalid count throws `RangeError`
    before returning a Promise;
  - `NaN`, positive and negative infinity, zero, negative integers, fractions,
    safe integers above `64`, and positive and negative unsafe integers;
  - a Worker-constructor seam proving every invalid case constructs zero
    Workers and does not start initialization bookkeeping;
  - accepted omitted/`undefined`, `1`, and `64` cases, with the maximum tested
    through deterministic Workers rather than allocating 64 real Workers; and
  - a timeout-guarded real-process invalid-count probe that catches the
    synchronous error, observes no Worker-resource increase, and exits
    naturally without `process.exit()` or `unref()`.
- Atomic pool request-send rollback (`E-015`, `N-13`), including:
  - injected synchronous throws after registration for pool initialization,
    immediate root dispatch, queued root dispatch, immediate lease dispatch,
    and queued lease dispatch;
  - real uncloneable `evaluate` and proxy arguments, plus direct assertions
    that those public/proxy calls return rejecting Promises rather than throw;
  - exact thrown-value identity and exactly-once settlement after the matching
    pending entry, in-flight unit, request abort listener, lease pending-call
    unit, and satisfied close waiter return to baseline;
  - pool initialization rejecting without publication and terminating every
    Worker constructed by that attempt;
  - request-local failure leaving the slot and pool healthy, with no terminal
    error, termination, respawn, replay, cross-slot retry, request-ID reuse, or
    round-robin rewind;
  - root FIFO and active lease-private FIFO progress through consecutive
    failed queued sends until a later valid request succeeds;
  - a failed proxy call leaving its lease active while later already-accepted
    owner calls drain in invocation order and normal callback release still
    occurs once;
  - pre-aborted queued work retaining `AbortError` precedence without a send,
    while an abort after a send attempt cannot replace its send failure; and
  - synchronous response, ordinary response error, Worker error, and exit
    before a later send throw, proving exact-entry first-settlement behavior and
    preservation of N-11's primary terminal error.
- Callback-proxy cancellation (`E-016`, `N-14`), including:
  - already-aborted public entry, queued lease cancellation, and the
    admission-to-callback microtask race, proving the callback is not invoked,
    the lease is released once, no round-robin/request allocation or Worker
    dispatch occurs, and admission listeners return to baseline;
  - a real-Worker reproduction in which the outer `do` Promise has already
    rejected, the still-running callback attempts a new mutation, that proxy
    call rejects with `DOMException` name `AbortError` and message
    `The operation was aborted`, and later work proves the fact was not added;
  - abort between callback awaits followed by every `EngineProxy` method,
    including `run({limit: NaN})`, proving each public call returns a rejecting
    Promise and cancellation wins before validation, request-ID allocation,
    lease-call accounting, listener registration, either queue, pending-map
    insertion, or Worker post;
  - a user-observable `run` option getter that aborts after the initial proxy
    gate but returns a valid limit, proving the final pre-allocation signal
    recheck rejects without an ID, lease-call unit, queue entry, or Worker post;
  - immediate and lease-private queued calls invoked before abort, proving both
    remain accepted, serialize in invocation order, drain/apply after the outer
    rejection, and preserve their own response or ordinary error rather than
    being dequeued or replaced by `AbortError`;
  - abort during an accepted native run, proving its shared flag changes, its
    per-run listener returns to baseline, it resolves with the partial
    `HaltRequested` projection absent another failure, and the outer `do`
    independently rejects with `AbortError`;
  - callback-settlement-first controls for fulfillment and rejection while an
    accepted call still drains, proving listener removal and that a later abort
    replaces neither callback outcome nor post-settlement lifetime error;
  - abort-first controls proving `AbortError` while the callback remains active,
    then the existing exact lifetime error after callback settlement;
  - active failed-slot, closed-state, and synchronous-send race controls proving
    exact terminal/lifetime/send outcomes retain N-11/N-13 precedence while the
    outer cancellation result remains independently fixed;
  - an idle callback held after outer cancellation, proving its selected slot
    remains exclusively leased, unrelated work cannot enter, and actual
    callback settlement plus accepted-call drain performs one release; and
  - listener, lease queue/pending-call/waiter, slot pending/in-flight, and
    Worker post baselines after each phase, plus bounded seeded interleaving
    stress whose failure reports its seed.
- Bounded pool backpressure and metrics (`C-006`, `E-017`, `N-15`), including:
  - strict public type/entrypoint checks for `EnginePoolOptions`,
    `EnginePoolQueueFullError`, `EnginePoolMetrics`,
    `EnginePoolSlotMetrics`, and `metrics()`;
  - omitted and explicit `undefined` capacity defaulting to `1024`, accepted
    zero (including normalized `-0`) and positive safe integers, plus `NaN`,
    both infinities, negatives, fractions, and unsafe integers rejecting with
    the exact synchronous `RangeError` before spec access, Promise return,
    Worker construction, or initialization bookkeeping;
  - zero-capacity immediate root dispatch and immediate lease acquisition,
    followed by prompt rejection of every request that would have to wait;
  - a deterministic one-slot capacity-plus-N burst proving at most one request
    is dispatched and exactly `queueCapacity` additional entries are retained;
  - mixed queued `evaluate`, queued `do` lease admission, and active
    lease-private owner work proving all three share one selected-slot budget,
    while the dispatched request and admitted callback/lease do not count;
  - root FIFO and lease-private invocation-order controls proving overflow is
    never inserted and cannot reorder already-accepted work;
  - multi-slot controls proving a full selected slot rejects without probing,
    retrying, replaying, or rescheduling to available capacity elsewhere, while
    the completed round-robin selection remains advanced;
  - every root and proxy overflow path returning a rejected Promise, not a
    synchronous throw, with exact `EnginePoolQueueFullError` prototype, name,
    `FERRIC_POOL_QUEUE_FULL` code, `EnginePool queue is full` message, and
    `capacity`/`queued`/`slotIndex` rejection snapshot;
  - precedence controls for reentry, inactive lifetime, closed/non-running
    state, terminal failure, abort, argument validation, and synchronous
    preprocessing, followed by proof that overflow allocates no request ID,
    lease, pending/lease-call unit, queue entry, or Worker post, retains no
    abort listener, and installs none when the slot is already full;
  - replaceable cooperative-listener hooks for root `evaluate` and proxy `run`
    that synchronously fill or drain the selected FIFO, proving post-hook
    lifecycle/signal/scheduling/capacity revalidation, nested-work-first
    linearization on fill, prompt outer queue-full rejection after transient
    listener removal, abort-before-ID precedence, and no stranded dispatch on
    drain;
  - replaceable queued-root and queued-lease dequeue-listener hooks proving the
    accepted entry reserves structural capacity first; nested admission cannot
    exceed the bound; synchronous abort and a registration throw reclaim it;
    an exact hook throw rejects only while the entry remains queued; and an
    earlier hook-driven dispatch, lease admission, terminal failure, or close
    outcome wins without double settlement or rollback. Include a removal hook
    that removes then throws, proving its failure cannot replace `AbortError`,
    post-registration cleanup retries, and no stale listener remains;
  - exact structural reclamation when queued root work or lease admission
    aborts, a request dequeues for dispatch, Worker terminal cleanup clears
    both queue levels, and close rejects root work while admitted owner work
    retains its existing drain barrier;
  - an N-14 control proving callback abort does not dequeue or reclaim owner
    work accepted before abort, and an N-13 control proving a synchronous send
    throw after dequeue cannot double-reclaim capacity and still advances the
    applicable FIFO;
  - fresh `metrics()` snapshots before/after every admission and reclamation,
    with per-slot `slotIndex`/`queued`/`inFlight`/`rejected`, matching aggregate
    sums, the top-level configured per-slot `queueCapacity`, queue-full-only
    rejection counting, stable slot order, and mutation of a cast snapshot
    unable to affect later reads;
  - metrics called inside an active callback, during shutdown, after terminal
    failure, and after close without admission/reentrancy errors, with completed
    close reporting zero queued and in-flight work;
  - a memory-limited public-API subprocess that stalls a Worker, submits a
    bounded burst far beyond capacity, verifies prompt typed overflow and the
    retained-count bound, closes naturally, and does not use `process.exit()`
    or `unref()`; and
  - deterministic mixed root/owner/abort/send/fault/close stress with explicit
    barriers and a recorded seed, asserting queue depth never exceeds capacity
    and all aggregate metrics equal their slot sums.
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
- Terminal pool Worker state (`E-013`, `N-11`), including:
  - Worker `error`, unexpected nonzero exit, unexpected zero exit, and the
    normal error-followed-by-exit duplicate signal sequence;
  - first-event-wins preservation of the exact emitted error object or one
    stable synthesized exit error across every affected rejection and every
    later root admission, including local cleanup after a second slot faults;
  - one or more failed-slot pending requests together with ordinary root FIFO
    work, queued lease admissions, and active-lease proxy work, all settling
    exactly once;
  - failed-slot pending, in-flight, root-queue, lease-queue, lease-call,
    request-listener, and abort-listener bookkeeping returning to its terminal
    baseline;
  - a multi-worker failure proving already accepted healthy-slot FIFO work and
    active-lease owner work remain eligible to finish, while later
    `evaluate()` and `do()` admissions reject before round-robin selection,
    request allocation, listener registration, or dispatch;
  - no Worker reconstruction, failed-work replay, or post-failure dispatch to
    the failed slot;
  - ordinary response errors leaving the pool healthy and an exit caused by
    deliberate close-time termination not establishing a failure;
  - an admitted failed-slot callback whose proxy calls reject while unrelated
    callback JavaScript is not forcibly settled and its existing release
    barrier remains intact;
  - close-before-fault and close-after-fault races proving cleared pending
    bookkeeping wakes close, every owned Worker is terminated, and no
    temporary waiter/listener remains; and
  - a timeout-guarded real-Worker subprocess that injects a terminal exit with
    pending and queued work, closes the pool, returns active Worker resources
    to baseline, and exits naturally without `process.exit()` or `unref()`.

E-016 covers cancellation-time proxy admission and work accepted before abort.
E-012 continues to own the lease and normal callback-settlement boundary:
FR-NODE-009 invalidates new proxy calls but does not preempt the callback or
release that lease early.
E-015 removes an abort listener from a request whose send fails; generic
root-queue listener cleanup after a successful dispatch remains FR-NODE-006.
E-017 owns structural queue-capacity reclamation but does not complete that
successful-dispatch listener cleanup. Concurrent close calls sharing one
completion Promise remain FR-NODE-010. E-013 covers corresponding resources
only on a Worker terminal transition. E-014 bounds Worker construction;
E-017 independently validates the waiting-entry limit before construction.

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
8. E-016 ordering tests `MUST` use controlled Worker/event seams and explicit
   callback, abort, dispatch, response, and release barriers. A wall-clock abort
   cannot be the sole evidence for proxy-admission or first-settlement order.
9. E-017 overload tests `MUST` use controlled dispatch/dequeue barriers and
   inspect both queue levels. The memory subprocess may use a parent timeout as
   a failure guard, but retained-count, typed-overflow, cleanup, and natural
   child exit are the passing conditions; wall-clock delay alone is not proof.

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
