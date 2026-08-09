/**
 * EnginePool terminal worker-slot state tests (FR-NODE-005 / E-013).
 *
 * These cases intentionally keep synchronous postMessage rollback (FR-NODE-008),
 * successful root-queue AbortSignal cleanup (FR-NODE-006), cancellation-time
 * proxy invalidation (FR-NODE-009), and concurrent close barriers (FR-NODE-010)
 * out of scope.
 */
import { execFile } from "node:child_process";
import { EventEmitter, getEventListeners } from "node:events";
import { resolve } from "node:path";
import { setImmediate as yieldImmediate } from "node:timers/promises";
import { test } from "node:test";
import * as assert from "node:assert/strict";

import { EnginePool } from "../../../helpers/ferric";

const workerThreads = require("node:worker_threads") as typeof import("node:worker_threads");

interface PostedMessage {
  id: number;
  method: string;
  args: unknown[];
}

interface Deferred<T> {
  promise: Promise<T>;
  resolve: (value: T | PromiseLike<T>) => void;
  reject: (reason?: unknown) => void;
}

class FakeWorker extends EventEmitter {
  readonly messages: PostedMessage[] = [];
  terminateCalls = 0;
  onTerminate?: () => Promise<number>;

  postMessage(message: PostedMessage): void {
    this.messages.push(message);
  }

  terminate(): Promise<number> {
    this.terminateCalls += 1;
    return this.onTerminate?.() ?? Promise.resolve(0);
  }
}

interface PoolFixture {
  pool: EnginePool;
  workers: FakeWorker[];
  slots: any[];
}

interface TerminalCase {
  label: string;
  pattern: RegExp;
  emit: (worker: FakeWorker) => Error | undefined;
}

const EMPTY_EVALUATE_RESULT = {
  runResult: { rulesFired: 0, haltReason: 0 },
  facts: [],
  output: {},
};

function deferred<T>(): Deferred<T> {
  let resolvePromise!: (value: T | PromiseLike<T>) => void;
  let rejectPromise!: (reason?: unknown) => void;
  const promise = new Promise<T>((resolveValue, rejectValue) => {
    resolvePromise = resolveValue;
    rejectPromise = rejectValue;
  });
  return {
    promise,
    resolve: resolvePromise,
    reject: rejectPromise,
  };
}

function makePool(threadCount = 1): PoolFixture {
  const workers = Array.from({ length: threadCount }, () => new FakeWorker());
  const slots = workers.map((worker) =>
    (EnginePool as any).createSlot(worker),
  );
  const pool = new (EnginePool as any)(slots) as EnginePool;
  return { pool, workers, slots };
}

async function within<T>(
  promise: Promise<T>,
  label: string,
  timeoutMs = 2_000,
): Promise<T> {
  let timer: NodeJS.Timeout | undefined;
  const timeout = new Promise<never>((_resolve, reject) => {
    timer = setTimeout(
      () => reject(new Error(`Timed out waiting for ${label}`)),
      timeoutMs,
    );
  });

  try {
    return await Promise.race([promise, timeout]);
  } finally {
    if (timer !== undefined) clearTimeout(timer);
  }
}

async function rejectionOf<T>(promise: Promise<T>): Promise<unknown> {
  try {
    await promise;
  } catch (error) {
    return error;
  }
  throw new Error("Expected Promise to reject");
}

async function waitForMessage(
  worker: FakeWorker,
  index: number,
  label: string,
): Promise<PostedMessage> {
  for (let turn = 0; turn < 20; turn++) {
    const message = worker.messages[index];
    if (message !== undefined) return message;
    await yieldImmediate();
  }
  assert.fail(`EnginePool did not post ${label}`);
}

function respond(
  worker: FakeWorker,
  message: PostedMessage,
  result: unknown = EMPTY_EVALUATE_RESULT,
): void {
  worker.emit("message", { id: message.id, result });
}

function assertWorkerListenersRemoved(worker: FakeWorker): void {
  assert.strictEqual(worker.listenerCount("message"), 0);
  assert.strictEqual(worker.listenerCount("error"), 0);
  assert.strictEqual(worker.listenerCount("exit"), 0);
}

function assertCleanTerminalSlot(slot: any): void {
  assert.strictEqual(slot.state.kind, "failed");
  assert.strictEqual(slot.pending.size, 0);
  assert.strictEqual(slot.inflight, 0);
  assert.strictEqual(slot.queue.length, 0);
  assert.strictEqual(slot.activeLease, undefined);
  assert.strictEqual(slot.pendingDrainWaiters.length, 0);
  assert.strictEqual(slot.listenersAttached, false);
}

function assertAbortListenersRemoved(...signals: AbortSignal[]): void {
  for (const signal of signals) {
    assert.strictEqual(getEventListeners(signal, "abort").length, 0);
  }
}

function terminalCases(): TerminalCase[] {
  return [
    {
      label: "Worker error",
      pattern: /forced worker error/,
      emit: (worker) => {
        const error = new Error("forced worker error");
        worker.emit("error", error);
        return error;
      },
    },
    {
      label: "nonzero exit",
      pattern: /Pool worker exited unexpectedly with code 9/,
      emit: (worker) => {
        worker.emit("exit", 9);
        return undefined;
      },
    },
    {
      label: "zero exit with pending work",
      pattern: /Pool worker exited before responding to pending request/,
      emit: (worker) => {
        worker.emit("exit", 0);
        return undefined;
      },
    },
  ];
}

test("E-013 terminal faults reject pending, queued request, and queued lease work", async (t) => {
  for (const item of terminalCases()) {
    await t.test(item.label, async () => {
      const { pool, workers: [worker], slots: [slot] } = makePool();
      const pendingAbort = new AbortController();
      const queuedAbort = new AbortController();
      const leaseAbort = new AbortController();
      let queuedCallbackEntered = false;

      const pendingOutcome = rejectionOf(
        pool.evaluate("rules", {}, { signal: pendingAbort.signal }),
      );
      const queuedOutcome = rejectionOf(
        pool.evaluate("rules", {}, { signal: queuedAbort.signal }),
      );
      const queuedLeaseOutcome = rejectionOf(
        pool.do(
          "rules",
          async () => {
            queuedCallbackEntered = true;
          },
          { signal: leaseAbort.signal },
        ),
      );

      assert.strictEqual(slot.pending.size, 1);
      assert.strictEqual(slot.inflight, 1);
      assert.strictEqual(slot.queue.length, 2);
      const queuedLease = slot.queue.find(
        (queued: { kind: string }) => queued.kind === "lease",
      ).lease;

      const emittedError = item.emit(worker);
      const terminalError = await within(
        pendingOutcome,
        `${item.label} pending rejection`,
      );
      const queuedError = await within(
        queuedOutcome,
        `${item.label} queued rejection`,
      );
      const queuedLeaseError = await within(
        queuedLeaseOutcome,
        `${item.label} queued lease rejection`,
      );

      assert.ok(terminalError instanceof Error);
      assert.match(terminalError.message, item.pattern);
      assert.strictEqual(queuedError, terminalError);
      assert.strictEqual(queuedLeaseError, terminalError);
      if (emittedError !== undefined) {
        assert.strictEqual(terminalError, emittedError);
      }
      assert.strictEqual(slot.state.error, terminalError);
      assert.strictEqual((pool as any).terminalError, terminalError);
      assert.strictEqual(queuedCallbackEntered, false);
      assert.strictEqual(queuedLease.active, false);
      assert.strictEqual(queuedLease.released, true);
      assertCleanTerminalSlot(slot);
      assertAbortListenersRemoved(
        pendingAbort.signal,
        queuedAbort.signal,
        leaseAbort.signal,
      );
      assertWorkerListenersRemoved(worker);

      const postsBeforeFutureWork = worker.messages.length;
      let futureCallbackEntered = false;
      assert.strictEqual(
        await rejectionOf(pool.evaluate("rules", {})),
        terminalError,
      );
      assert.strictEqual(
        await rejectionOf(
          pool.do("rules", async () => {
            futureCallbackEntered = true;
          }),
        ),
        terminalError,
      );
      assert.strictEqual(futureCallbackEntered, false);
      assert.strictEqual(worker.messages.length, postsBeforeFutureWork);

      await within(pool.close(), `${item.label} close after terminal fault`);
      assert.strictEqual(slot.state.kind, "closed");
    });
  }
});

test("E-013 an idle zero exit is terminal and future admission fails fast", async () => {
  const { pool, workers: [worker], slots: [slot] } = makePool();
  const nextIdBeforeExit = slot.nextId;

  worker.emit("exit", 0);

  assertCleanTerminalSlot(slot);
  assertWorkerListenersRemoved(worker);
  const terminalError = slot.state.error;
  assert.match(
    terminalError.message,
    /Pool worker exited unexpectedly with code 0/,
  );

  let callbackEntered = false;
  assert.strictEqual(
    await rejectionOf(pool.evaluate("rules", {})),
    terminalError,
  );
  assert.strictEqual(
    await rejectionOf(
      pool.do("rules", async () => {
        callbackEntered = true;
      }),
    ),
    terminalError,
  );
  assert.strictEqual(callbackEntered, false);
  assert.strictEqual(worker.messages.length, 0);
  assert.strictEqual(slot.nextId, nextIdBeforeExit);

  await within(pool.close(), "close after idle zero exit");
});

test("E-013 duplicate terminal signals and late responses settle entries once", async (t) => {
  const cases = [
    {
      label: "error then exit",
      pattern: /first worker error/,
      first: (worker: FakeWorker) => {
        const error = new Error("first worker error");
        worker.emit("error", error);
        return error;
      },
      late: (listeners: any) => {
        listeners.exit(9);
        listeners.error(new Error("late worker error"));
      },
    },
    {
      label: "nonzero exit then error",
      pattern: /Pool worker exited unexpectedly with code 9/,
      first: (worker: FakeWorker) => {
        worker.emit("exit", 9);
        return undefined;
      },
      late: (listeners: any) => {
        listeners.error(new Error("late worker error"));
        listeners.exit(0);
      },
    },
    {
      label: "zero exit then error",
      pattern: /Pool worker exited before responding to pending request/,
      first: (worker: FakeWorker) => {
        worker.emit("exit", 0);
        return undefined;
      },
      late: (listeners: any) => {
        listeners.error(new Error("late worker error"));
        listeners.exit(7);
      },
    },
  ];

  for (const item of cases) {
    await t.test(item.label, async () => {
      const { pool, workers: [worker], slots: [slot] } = makePool();
      let queuedLeaseEntered = false;
      const pendingOutcome = rejectionOf(pool.evaluate("rules", {}));
      const queuedOutcome = rejectionOf(pool.evaluate("rules", {}));
      const queuedLeaseOutcome = rejectionOf(
        pool.do("rules", async () => {
          queuedLeaseEntered = true;
        }),
      );

      const pendingEntry = [...slot.pending.values()][0];
      const queuedRequest = slot.queue.find(
        (queued: { kind: string }) => queued.kind === "request",
      );
      const queuedLease = slot.queue.find(
        (queued: { kind: string }) => queued.kind === "lease",
      );
      const listeners = { ...slot.listeners };
      const pendingId = worker.messages[0].id;
      let pendingRejectCalls = 0;
      let queuedRejectCalls = 0;
      let queuedLeaseRejectCalls = 0;

      const originalPendingReject = pendingEntry.reject;
      pendingEntry.reject = (error: Error): void => {
        pendingRejectCalls += 1;
        originalPendingReject(error);
      };
      const originalQueuedReject = queuedRequest.entry.reject;
      queuedRequest.entry.reject = (error: Error): void => {
        queuedRejectCalls += 1;
        originalQueuedReject(error);
      };
      const originalLeaseReject = queuedLease.reject;
      queuedLease.reject = (error: Error): void => {
        queuedLeaseRejectCalls += 1;
        originalLeaseReject(error);
      };

      const emittedError = item.first(worker);
      item.late(listeners);
      listeners.message({ id: pendingId, result: EMPTY_EVALUATE_RESULT });

      const terminalError = await within(
        pendingOutcome,
        `${item.label} pending rejection`,
      );
      assert.ok(terminalError instanceof Error);
      assert.match(terminalError.message, item.pattern);
      assert.strictEqual(
        await within(queuedOutcome, `${item.label} queued rejection`),
        terminalError,
      );
      assert.strictEqual(
        await within(
          queuedLeaseOutcome,
          `${item.label} queued lease rejection`,
        ),
        terminalError,
      );
      if (emittedError !== undefined) {
        assert.strictEqual(terminalError, emittedError);
      }
      assert.strictEqual(pendingRejectCalls, 1);
      assert.strictEqual(queuedRejectCalls, 1);
      assert.strictEqual(queuedLeaseRejectCalls, 1);
      assert.strictEqual(queuedLeaseEntered, false);
      assertCleanTerminalSlot(slot);
      assertWorkerListenersRemoved(worker);

      assert.strictEqual(
        await rejectionOf(pool.evaluate("rules", {})),
        terminalError,
      );
      await within(pool.close(), `${item.label} close`);
    });
  }
});

test("E-013 a failed active lease rejects owner and root queues before release", async () => {
  const { pool, workers: [worker], slots: [slot] } = makePool();
  const rootAbort = new AbortController();
  const waitingLeaseAbort = new AbortController();
  let retainedProxy: any;
  let ownerErrors: unknown[] | undefined;
  let postFaultOwnerError: unknown;
  let waitingLeaseEntered = false;

  const activeOutcome = rejectionOf(
    pool.do("rules", async (proxy) => {
      retainedProxy = proxy;
      const first = proxy.facts();
      const second = proxy.facts();
      ownerErrors = await Promise.all([
        rejectionOf(first),
        rejectionOf(second),
      ]);
      postFaultOwnerError = await rejectionOf(proxy.facts());
      throw ownerErrors[0];
    }),
  );
  await waitForMessage(worker, 0, "the active lease request");
  const activeLease = slot.activeLease;

  const rootOutcome = rejectionOf(
    pool.evaluate("rules", {}, { signal: rootAbort.signal }),
  );
  const waitingLeaseOutcome = rejectionOf(
    pool.do(
      "rules",
      async () => {
        waitingLeaseEntered = true;
      },
      { signal: waitingLeaseAbort.signal },
    ),
  );
  assert.strictEqual(activeLease.queue.length, 1);
  assert.strictEqual(slot.queue.length, 2);

  const terminalError = new Error("active lease worker failed");
  worker.emit("error", terminalError);

  assert.strictEqual(
    await within(rootOutcome, "failed-slot root request rejection"),
    terminalError,
  );
  assert.strictEqual(
    await within(waitingLeaseOutcome, "failed-slot lease waiter rejection"),
    terminalError,
  );
  assert.strictEqual(
    await within(activeOutcome, "failed active lease rejection"),
    terminalError,
  );
  assert.deepStrictEqual(ownerErrors, [terminalError, terminalError]);
  assert.strictEqual(postFaultOwnerError, terminalError);
  assert.strictEqual(waitingLeaseEntered, false);
  assert.strictEqual(activeLease.pendingCalls, 0);
  assert.strictEqual(activeLease.queue.length, 0);
  assert.strictEqual(activeLease.released, true);
  assertCleanTerminalSlot(slot);
  assertAbortListenersRemoved(rootAbort.signal, waitingLeaseAbort.signal);
  assertWorkerListenersRemoved(worker);

  await assert.rejects(
    () => retainedProxy.facts(),
    /EngineProxy is no longer valid outside its EnginePool\.do callback/,
  );
  await within(pool.close(), "close after active lease failure");
});

test("E-013 a fault wakes one close already waiting for pending work", async () => {
  const { pool, workers: [worker], slots: [slot] } = makePool();
  const requestOutcome = rejectionOf(pool.evaluate("rules", {}));
  const closePromise = pool.close();

  assert.strictEqual(slot.pendingDrainWaiters.length, 1);
  const terminalError = new Error("worker failed during close");
  worker.emit("error", terminalError);

  assert.strictEqual(
    await within(requestOutcome, "request rejection during close"),
    terminalError,
  );
  await within(closePromise, "close waiting across a terminal fault");
  assert.strictEqual(slot.pendingDrainWaiters.length, 0);
  assert.strictEqual(slot.pending.size, 0);
  assert.strictEqual(slot.inflight, 0);
  assert.strictEqual(slot.queue.length, 0);
  assert.strictEqual(slot.state.kind, "closed");
  assert.strictEqual(slot.listenersAttached, false);
  assertWorkerListenersRemoved(worker);
});

test("E-013 close after a terminal fault completes without another message", async () => {
  const { pool, workers: [worker], slots: [slot] } = makePool();
  const requestOutcome = rejectionOf(pool.evaluate("rules", {}));
  worker.emit("exit", 9);

  const terminalError = await within(
    requestOutcome,
    "request rejection before close",
  );
  assert.strictEqual(slot.state.kind, "failed");
  assert.strictEqual(slot.state.error, terminalError);

  await within(pool.close(), "close after terminal exit");
  assert.strictEqual(slot.state.kind, "closed");
  assert.strictEqual(slot.pendingDrainWaiters.length, 0);
  assertWorkerListenersRemoved(worker);
});

test("E-013 an expected zero exit while terminating does not poison close", async () => {
  const { pool, workers: [worker], slots: [slot] } = makePool();
  worker.onTerminate = async () => {
    worker.emit("exit", 0);
    return 0;
  };

  await within(pool.close(), "normal close with an exit event");

  assert.strictEqual(worker.terminateCalls, 1);
  assert.strictEqual(slot.state.kind, "closed");
  assert.strictEqual((pool as any).terminalError, undefined);
  assert.strictEqual(slot.listenersAttached, false);
  assertWorkerListenersRemoved(worker);
});

test("E-013 protocol error responses are nonterminal and the slot stays usable", async () => {
  const { pool, workers: [worker], slots: [slot] } = makePool();
  const first = pool.evaluate("rules", {});
  const firstMessage = await waitForMessage(worker, 0, "the first request");

  worker.emit("message", {
    id: firstMessage.id,
    error: {
      name: "FerricRuntimeError",
      message: "ordinary protocol failure",
      code: "FERRIC_RUNTIME_ERROR",
    },
  });

  await assert.rejects(first, /ordinary protocol failure/);
  assert.strictEqual(slot.state.kind, "running");
  assert.strictEqual((pool as any).terminalError, undefined);
  assert.strictEqual(slot.pending.size, 0);
  assert.strictEqual(slot.inflight, 0);
  assert.strictEqual(slot.queue.length, 0);
  assert.strictEqual(slot.listenersAttached, true);
  assert.deepStrictEqual(
    ["message", "error", "exit"].map((event) =>
      worker.listenerCount(event),
    ),
    [1, 1, 1],
  );

  const second = pool.evaluate("rules", {});
  const secondMessage = await waitForMessage(worker, 1, "the follow-up request");
  respond(worker, secondMessage);
  assert.deepStrictEqual(await second, EMPTY_EVALUATE_RESULT);
  assert.strictEqual(slot.state.kind, "running");

  await within(pool.close(), "close after a protocol response error");
  assert.strictEqual(slot.state.kind, "closed");
  assertWorkerListenersRemoved(worker);
});

test("E-013 a failed slot preserves healthy already-accepted work but poisons future admission", async () => {
  const {
    pool,
    workers: [failedWorker, healthyWorker],
    slots: [failedSlot, healthySlot],
  } = makePool(2);
  const failedQueueAbort = new AbortController();
  const failedLeaseAbort = new AbortController();
  let failedLeaseEntered = false;
  let healthyLeaseEntered = false;
  let futureLeaseEntered = false;

  const failedPendingOutcome = rejectionOf(pool.evaluate("rules", {}));
  const healthyPending = pool.evaluate("rules", {});
  const failedQueuedOutcome = rejectionOf(
    pool.evaluate("rules", {}, { signal: failedQueueAbort.signal }),
  );
  const healthyQueued = pool.evaluate("rules", {});
  const failedQueuedLeaseOutcome = rejectionOf(
    pool.do(
      "rules",
      async () => {
        failedLeaseEntered = true;
      },
      { signal: failedLeaseAbort.signal },
    ),
  );
  const healthyQueuedLease = pool.do("rules", async (proxy) => {
    healthyLeaseEntered = true;
    return proxy.facts();
  });

  assert.strictEqual(failedSlot.pending.size, 1);
  assert.strictEqual(failedSlot.queue.length, 2);
  assert.strictEqual(healthySlot.pending.size, 1);
  assert.strictEqual(healthySlot.queue.length, 2);

  const terminalError = new Error("first slot failed");
  failedWorker.emit("error", terminalError);

  assert.strictEqual(
    await within(failedPendingOutcome, "failed-slot pending rejection"),
    terminalError,
  );
  assert.strictEqual(
    await within(failedQueuedOutcome, "failed-slot queued rejection"),
    terminalError,
  );
  assert.strictEqual(
    await within(
      failedQueuedLeaseOutcome,
      "failed-slot queued lease rejection",
    ),
    terminalError,
  );
  assert.strictEqual(failedLeaseEntered, false);
  assertCleanTerminalSlot(failedSlot);
  assertAbortListenersRemoved(
    failedQueueAbort.signal,
    failedLeaseAbort.signal,
  );
  assertWorkerListenersRemoved(failedWorker);

  // A terminal fault poisons only later public admissions. Work already
  // accepted by another slot keeps its established FIFO and lease semantics.
  assert.strictEqual(healthySlot.state.kind, "running");
  assert.strictEqual(healthySlot.pending.size, 1);
  assert.strictEqual(healthySlot.queue.length, 2);
  assert.strictEqual(healthyLeaseEntered, false);
  const failedPostsBeforeFutureWork = failedWorker.messages.length;
  const healthyPostsBeforeFutureWork = healthyWorker.messages.length;
  assert.strictEqual(
    await rejectionOf(pool.evaluate("rules", {})),
    terminalError,
  );
  assert.strictEqual(
    await rejectionOf(
      pool.do("rules", async () => {
        futureLeaseEntered = true;
      }),
    ),
    terminalError,
  );
  assert.strictEqual(futureLeaseEntered, false);
  assert.strictEqual(failedWorker.messages.length, failedPostsBeforeFutureWork);
  assert.strictEqual(healthyWorker.messages.length, healthyPostsBeforeFutureWork);

  const healthyPendingMessage = await waitForMessage(
    healthyWorker,
    0,
    "the healthy in-flight request",
  );
  respond(healthyWorker, healthyPendingMessage);
  assert.deepStrictEqual(await healthyPending, EMPTY_EVALUATE_RESULT);

  const healthyQueuedMessage = await waitForMessage(
    healthyWorker,
    1,
    "the healthy queued request",
  );
  respond(healthyWorker, healthyQueuedMessage);
  assert.deepStrictEqual(await healthyQueued, EMPTY_EVALUATE_RESULT);

  // The lease admission was accepted and queued before the other slot failed.
  // Its callback begins after the global poison and its owner send is
  // nevertheless still accepted.
  const healthyLeaseMessage = await waitForMessage(
    healthyWorker,
    2,
    "the healthy admitted lease owner request",
  );
  assert.strictEqual(healthyLeaseEntered, true);
  respond(healthyWorker, healthyLeaseMessage, []);
  assert.deepStrictEqual(await healthyQueuedLease, []);
  assert.strictEqual(healthySlot.state.kind, "running");
  assert.strictEqual(healthySlot.pending.size, 0);
  assert.strictEqual(healthySlot.inflight, 0);
  assert.strictEqual(healthySlot.queue.length, 0);
  assert.strictEqual(healthySlot.activeLease, undefined);

  // A later terminal signal from another slot cannot replace the pool's
  // canonical first failure, including in that second slot's failed state.
  healthyWorker.emit("error", new Error("later healthy-slot failure"));
  assert.strictEqual(healthySlot.state.kind, "failed");
  assert.strictEqual(healthySlot.state.error, terminalError);
  assert.strictEqual((pool as any).terminalError, terminalError);
  assertWorkerListenersRemoved(healthyWorker);

  await within(pool.close(), "close mixed failed and healthy slots");
  assert.strictEqual(failedSlot.state.kind, "closed");
  assert.strictEqual(healthySlot.state.kind, "closed");
  assertWorkerListenersRemoved(healthyWorker);
});

test("E-013 a failed idle callback retains its normal lease lifetime", async () => {
  const { pool, workers: [worker], slots: [slot] } = makePool();
  const callbackEntered = deferred<void>();
  const continueCallback = deferred<void>();
  let postFaultOwnerError: unknown;

  const active = pool.do("rules", async (proxy) => {
    callbackEntered.resolve();
    await continueCallback.promise;
    postFaultOwnerError = await rejectionOf(proxy.run({ limit: Number.NaN }));
    return "callback handled terminal failure";
  });
  await callbackEntered.promise;
  const activeLease = slot.activeLease;

  const terminalError = new Error("idle lease worker failed");
  worker.emit("error", terminalError);
  assert.strictEqual(slot.state.kind, "failed");
  assert.strictEqual(slot.activeLease, activeLease);
  assert.strictEqual(activeLease.released, false);

  const closePromise = pool.close();
  assert.strictEqual(activeLease.released, false);
  continueCallback.resolve();

  assert.strictEqual(
    await within(active, "idle callback settlement after worker failure"),
    "callback handled terminal failure",
  );
  assert.strictEqual(postFaultOwnerError, terminalError);
  assert.strictEqual(activeLease.released, true);
  await within(closePromise, "close waiting for failed idle callback release");
  assert.strictEqual(slot.state.kind, "closed");
});

test("E-013 terminal cleanup removes only pool-owned Worker listeners", async () => {
  const worker = new FakeWorker();
  const externalMessage = (): void => undefined;
  const externalError = (): void => undefined;
  const externalExit = (): void => undefined;
  worker.on("message", externalMessage);
  worker.on("error", externalError);
  worker.on("exit", externalExit);

  const slot = (EnginePool as any).createSlot(worker);
  const pool = new (EnginePool as any)([slot]) as EnginePool;
  const requestOutcome = rejectionOf(pool.evaluate("rules", {}));
  const terminalError = new Error("preserve external listeners");
  worker.emit("error", terminalError);

  assert.strictEqual(await requestOutcome, terminalError);
  assert.deepStrictEqual(worker.listeners("message"), [externalMessage]);
  assert.deepStrictEqual(worker.listeners("error"), [externalError]);
  assert.deepStrictEqual(worker.listeners("exit"), [externalExit]);
  assert.strictEqual(slot.listenersAttached, false);

  await within(pool.close(), "close with external Worker listeners");
  assert.deepStrictEqual(worker.listeners("message"), [externalMessage]);
  assert.deepStrictEqual(worker.listeners("error"), [externalError]);
  assert.deepStrictEqual(worker.listeners("exit"), [externalExit]);
});

test("E-013 a slot that fails before pool publication poisons construction", async () => {
  const worker = new FakeWorker();
  const slot = (EnginePool as any).createSlot(worker);
  const terminalError = new Error("slot failed before pool publication");
  const secondWorker = new FakeWorker();
  const secondSlot = (EnginePool as any).createSlot(secondWorker);
  const secondError = new Error("second prepublication slot failure");

  // No pool callback is installed yet, so the slot-local failure fallback owns
  // these transitions. The later constructor must adopt only the first error.
  worker.emit("error", terminalError);
  secondWorker.emit("error", secondError);
  assertCleanTerminalSlot(slot);
  assertCleanTerminalSlot(secondSlot);
  const pool = new (EnginePool as any)([slot, secondSlot]) as EnginePool;
  assert.strictEqual((pool as any).terminalError, terminalError);

  const nextIdBeforeGuards = slot.nextId;
  const postsBeforeGuards = worker.messages.length;
  assert.strictEqual(
    await rejectionOf((pool as any).acquireLease(slot)),
    terminalError,
  );
  assert.strictEqual(
    await rejectionOf(
      (pool as any).sendToSlot(slot, "facts", ["rules"]),
    ),
    terminalError,
  );

  // Direct owner-send defense is deliberately covered independently of the
  // proxy's earlier withActiveLease guard.
  const lease = (EnginePool as any).createLease();
  lease.active = true;
  slot.activeLease = lease;
  assert.strictEqual(
    await rejectionOf(
      (pool as any).sendOnLease(slot, lease, "facts", ["rules"]),
    ),
    terminalError,
  );
  slot.activeLease = undefined;
  (EnginePool as any).markLeaseReleased(lease);

  assert.strictEqual(slot.nextId, nextIdBeforeGuards);
  assert.strictEqual(worker.messages.length, postsBeforeGuards);
  await within(pool.close(), "close a prefailed constructed pool");
  assert.strictEqual(slot.state.kind, "closed");
  assert.strictEqual(secondSlot.state.kind, "closed");
});

test("E-013 non-running active-owner guards reject before validation or send", async () => {
  const { pool, workers: [worker], slots: [slot] } = makePool();
  const lease = await (pool as any).acquireLease(slot);
  const proxy = (pool as any).makeProxy("rules", slot, lease);
  const nextIdBeforeGuards = slot.nextId;
  const postsBeforeGuards = worker.messages.length;
  slot.state = { kind: "terminating" };

  await assert.rejects(
    () => (pool as any).sendOnLease(slot, lease, "facts", ["rules"]),
    /EnginePool has been closed/,
  );
  await assert.rejects(
    () => proxy.run({ limit: Number.NaN }),
    /EnginePool has been closed/,
  );
  assert.strictEqual(slot.nextId, nextIdBeforeGuards);
  assert.strictEqual(worker.messages.length, postsBeforeGuards);

  // Restore the seam so ordinary lease release and close own final cleanup.
  slot.state = { kind: "running" };
  await (pool as any).releaseLease(slot, lease);
  await within(pool.close(), "close after non-running owner guards");
});

test("E-013 private transition helpers are defensive and idempotent", async () => {
  const { pool, workers: [worker], slots: [slot] } = makePool();

  // createSlot already attached the exact three handlers.
  (EnginePool as any).attachSlotListeners(slot);
  assert.deepStrictEqual(
    ["message", "error", "exit"].map((event) =>
      worker.listenerCount(event),
    ),
    [1, 1, 1],
  );

  // A defensive drain attempt cannot bypass work already in flight.
  slot.inflight = 1;
  (EnginePool as any).drainQueue(slot);
  assert.strictEqual(worker.messages.length, 0);
  slot.inflight = 0;

  let prematurelyNotified = false;
  slot.pending.set(999, {
    resolve: () => undefined,
    reject: () => undefined,
  });
  slot.pendingDrainWaiters.push(() => {
    prematurelyNotified = true;
  });
  (EnginePool as any).notifyPendingDrained(slot);
  assert.strictEqual(prematurelyNotified, false);
  assert.strictEqual(slot.pendingDrainWaiters.length, 1);
  slot.pending.delete(999);
  (EnginePool as any).notifyPendingDrained(slot);
  assert.strictEqual(prematurelyNotified, true);
  assert.strictEqual(slot.pendingDrainWaiters.length, 0);

  const terminalError = new Error("idempotent transition failure");
  worker.emit("error", terminalError);
  (pool as any).handleSlotTerminal(slot, new Error("late pool failure"));
  (EnginePool as any).failSlot(slot, new Error("late slot failure"));
  (EnginePool as any).detachSlotListeners(slot);
  assert.strictEqual(slot.state.error, terminalError);
  assertWorkerListenersRemoved(worker);

  await (EnginePool as any).terminateSlot(slot);
  assert.strictEqual(slot.state.kind, "closed");
  const terminateCallsAfterClose = worker.terminateCalls;
  await (EnginePool as any).terminateSlot(slot);
  assert.strictEqual(worker.terminateCalls, terminateCallsAfterClose);
  slot.state = { kind: "terminating" };
  await (EnginePool as any).terminateSlot(slot);
  assert.strictEqual(worker.terminateCalls, terminateCallsAfterClose);
  slot.state = { kind: "closed" };

  await within(pool.close(), "close after direct terminal helper coverage");
});

test("E-013 create rejects a synchronous post-init terminal fault before publication", async () => {
  const OriginalWorker = workerThreads.Worker;
  const terminalError = new Error("synchronous post-init terminal fault");

  class InitThenFailWorker extends FakeWorker {
    static instances: InitThenFailWorker[] = [];

    constructor(_filename: string) {
      super();
      InitThenFailWorker.instances.push(this);
    }

    override postMessage(message: PostedMessage): void {
      super.postMessage(message);
      if (message.method === "__init") {
        this.emit("message", { id: message.id, result: undefined });
        this.emit("error", terminalError);
      }
    }
  }

  workerThreads.Worker = InitThenFailWorker as unknown as typeof workerThreads.Worker;
  try {
    assert.strictEqual(
      await rejectionOf(
        EnginePool.create([{ name: "rules" }], { threads: 1 }),
      ),
      terminalError,
    );
    assert.strictEqual(InitThenFailWorker.instances.length, 1);
    assert.strictEqual(InitThenFailWorker.instances[0].terminateCalls, 1);
    assertWorkerListenersRemoved(InitThenFailWorker.instances[0]);
  } finally {
    workerThreads.Worker = OriginalWorker;
  }
});

test("E-013 create preserves a non-Error Worker constructor failure", async () => {
  const OriginalWorker = workerThreads.Worker;
  const constructorFailure = { kind: "non-Error constructor failure" };

  class ThrowingWorker {
    constructor(_filename: string) {
      throw constructorFailure;
    }
  }

  workerThreads.Worker = ThrowingWorker as unknown as typeof workerThreads.Worker;
  try {
    assert.strictEqual(
      await rejectionOf(
        EnginePool.create([{ name: "rules" }], { threads: 1 }),
      ),
      constructorFailure,
    );
  } finally {
    workerThreads.Worker = OriginalWorker;
  }
});

function runNodeScript(
  script: string,
): Promise<{ stdout: string; stderr: string }> {
  const packageRoot = resolve(__dirname, "../../../..");
  const childEnv = { ...process.env };
  // This subprocess is a fault-isolation oracle rather than another coverage
  // shard. Inheriting the parent runner's state can contaminate merged V8 data
  // and prevent the child from demonstrating a natural standalone exit.
  childEnv.NODE_V8_COVERAGE = "";
  delete childEnv.NODE_TEST_CONTEXT;
  return new Promise((resolveRun, rejectRun) => {
    execFile(
      process.execPath,
      ["-e", script],
      {
        cwd: packageRoot,
        env: childEnv,
        timeout: 8_000,
        killSignal: "SIGKILL",
        windowsHide: true,
      },
      (error, stdout, stderr) => {
        if (error) {
          rejectRun(
            new Error(
              `Node subprocess failed: ${error.message}\nstdout:\n${stdout}\nstderr:\n${stderr}`,
              { cause: error },
            ),
          );
          return;
        }
        resolveRun({ stdout, stderr });
      },
    );
  });
}

function realWorkerFaultScript(
  mode: "error" | "exit-9" | "exit-0",
): string {
  const terminalAction = mode === "error"
    ? 'throw new Error("real worker uncaught failure");'
    : mode === "exit-9"
      ? "process.exitCode = 9; parentPort.close();"
      : "parentPort.close();";
  const workerSource = `
const { parentPort } = require("node:worker_threads");
parentPort.on("message", (request) => {
  if (request.method === "__init") {
    parentPort.postMessage({ id: request.id, result: undefined });
    return;
  }
  ${terminalAction}
});
`;

  return `
const workerThreads = require("node:worker_threads");
const OriginalWorker = workerThreads.Worker;
class FaultWorker {
  constructor() {
    return new OriginalWorker(${JSON.stringify(workerSource)}, { eval: true });
  }
}
workerThreads.Worker = FaultWorker;
const { EnginePool } = require("./dist");
const messagePortCount = () => process.getActiveResourcesInfo()
  .filter((resource) => resource === "MessagePort").length;

(async () => {
  const before = messagePortCount();
  const pool = await EnginePool.create([{ name: "rules" }], { threads: 1 });
  const requestOutcome = pool.evaluate("rules", {}).then(
    () => { throw new Error("fault-injected request unexpectedly resolved"); },
    (error) => error,
  );
  const queuedOutcome = pool.evaluate("rules", {}).then(
    () => { throw new Error("fault-injected queued request unexpectedly resolved"); },
    (error) => error,
  );
  const [failure, queuedFailure] = await Promise.all([
    requestOutcome,
    queuedOutcome,
  ]);
  await pool.close();
  await new Promise((resolveValue) => setImmediate(resolveValue));
  await new Promise((resolveValue) => setImmediate(resolveValue));
  const after = messagePortCount();
  console.log(JSON.stringify({
    before,
    after,
    name: failure.name,
    message: failure.message,
    queuedSameFailure: queuedFailure === failure,
  }));
})().catch((error) => {
  console.error(error);
  process.exitCode = 1;
});
`;
}

test("E-013 real Worker faults let close finish and the process exit naturally", async (t) => {
  const cases = [
    {
      mode: "error" as const,
      pattern: /real worker uncaught failure/,
    },
    {
      mode: "exit-9" as const,
      pattern: /Pool worker exited unexpectedly with code 9/,
    },
    {
      mode: "exit-0" as const,
      pattern: /Pool worker exited before responding to pending request/,
    },
  ];

  for (const item of cases) {
    await t.test(item.mode, async () => {
      const script = realWorkerFaultScript(item.mode);
      const { stdout } = await runNodeScript(script);
      assert.notStrictEqual(
        stdout.trim(),
        "",
        "the subprocess exited before its request and close settled",
      );
      const report = JSON.parse(stdout.trim()) as {
        before: number;
        after: number;
        name: string;
        message: string;
        queuedSameFailure: boolean;
      };
      assert.strictEqual(report.name, "Error");
      assert.match(report.message, item.pattern);
      assert.strictEqual(report.queuedSameFailure, true);
      assert.strictEqual(report.after, report.before);
    });
  }
});
