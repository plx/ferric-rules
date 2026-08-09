/**
 * EnginePool synchronous postMessage rollback tests (FR-NODE-008 / E-015).
 *
 * The fake Worker throws only from the selected send attempt.  A recoverable
 * send failure must reject that work by exact identity without poisoning the
 * slot, stranding capacity, or preventing the next FIFO item from running.
 */
import { getEventListeners, EventEmitter } from "node:events";
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

class PlannedWorker extends EventEmitter {
  readonly messages: PostedMessage[] = [];
  readonly postActions: Array<(message: PostedMessage) => void> = [];
  terminateCalls = 0;
  nextPost: ((message: PostedMessage) => void) | undefined;

  postMessage(message: PostedMessage): void {
    this.messages.push(message);
    const action = this.nextPost ?? this.postActions.shift();
    this.nextPost = undefined;
    action?.(message);
  }

  terminate(): Promise<number> {
    this.terminateCalls += 1;
    return Promise.resolve(0);
  }
}

function makePool(): {
  pool: EnginePool;
  worker: PlannedWorker;
  slot: any;
} {
  const worker = new PlannedWorker();
  const slot = (EnginePool as any).createSlot(worker);
  const pool = new (EnginePool as any)([slot]) as EnginePool;
  return { pool, worker, slot };
}

function makeMultiWorkerPool(threadCount: number): {
  pool: EnginePool;
  workers: PlannedWorker[];
  slots: any[];
} {
  const workers = Array.from(
    { length: threadCount },
    () => new PlannedWorker(),
  );
  const slots = workers.map((worker) =>
    (EnginePool as any).createSlot(worker),
  );
  const pool = new (EnginePool as any)(slots) as EnginePool;
  return { pool, workers, slots };
}

async function rejectionOf<T>(promise: Promise<T>): Promise<unknown> {
  try {
    await promise;
  } catch (error) {
    return error;
  }
  throw new Error("Expected Promise to reject");
}

async function waitFor(
  predicate: () => boolean,
  label: string,
): Promise<void> {
  for (let turn = 0; turn < 40; turn++) {
    if (predicate()) return;
    await yieldImmediate();
  }
  assert.fail(`Timed out waiting for ${label}`);
}

function assertWorkerListenersIntact(worker: PlannedWorker): void {
  assert.strictEqual(worker.listenerCount("message"), 1);
  assert.strictEqual(worker.listenerCount("error"), 1);
  assert.strictEqual(worker.listenerCount("exit"), 1);
}

function assertRunningSlotIdle(pool: EnginePool, slot: any): void {
  assert.deepStrictEqual(slot.state, { kind: "running" });
  assert.strictEqual((pool as any).terminalError, undefined);
  assert.strictEqual(slot.pending.size, 0);
  assert.strictEqual(slot.inflight, 0);
  assert.strictEqual(slot.queue.length, 0);
  assert.strictEqual(slot.activeLease, undefined);
  assert.strictEqual(slot.pendingDrainWaiters.length, 0);
}

function evaluateResult(marker: string): any {
  return {
    runResult: { fired: 0, haltReason: marker, errors: [] },
    facts: [],
    output: {},
  };
}

async function completeEvaluation(
  pool: EnginePool,
  worker: PlannedWorker,
  marker: string,
): Promise<void> {
  const pending = pool.evaluate("rules", {});
  const request = worker.messages.at(-1);
  assert.ok(request);
  assert.strictEqual(request.method, "__evaluate");
  const result = evaluateResult(marker);
  worker.emit("message", { id: request.id, result });
  assert.deepStrictEqual(await pending, result);
}

function assertLeaseReleased(lease: any): void {
  assert.strictEqual(lease.active, false);
  assert.strictEqual(lease.released, true);
  assert.strictEqual(lease.pendingCalls, 0);
  assert.strictEqual(lease.queue.length, 0);
  assert.strictEqual(lease.drainWaiters.length, 0);
}

test("E-015 EnginePool initSlot rolls back a synchronous send failure", async () => {
  const worker = new PlannedWorker();
  const slot = (EnginePool as any).createSlot(worker);
  const failure = new DOMException(
    "pool initialization could not be cloned",
    "DataCloneError",
  );
  worker.nextPost = () => {
    throw failure;
  };

  let initialization: Promise<void> | undefined;
  assert.doesNotThrow(() => {
    initialization = (EnginePool as any).initSlot(slot, { specs: [] });
  });
  assert.ok(initialization);
  assert.strictEqual(await rejectionOf(initialization), failure);
  assert.strictEqual(slot.pending.size, 0);
  assert.strictEqual(slot.inflight, 0);
  assert.strictEqual(slot.pendingDrainWaiters.length, 0);
  assert.deepStrictEqual(slot.state, { kind: "running" });
  assertWorkerListenersIntact(worker);

  const retry = (EnginePool as any).initSlot(slot, { specs: [] });
  const retryRequest = worker.messages.at(-1);
  assert.ok(retryRequest);
  assert.strictEqual(retryRequest.method, "__init");
  worker.emit("message", { id: retryRequest.id, result: undefined });
  await retry;
  assert.strictEqual(slot.pending.size, 0);
  assert.strictEqual(slot.inflight, 0);

  await (EnginePool as any).terminateSlot(slot);
  assert.strictEqual(worker.terminateCalls, 1);
});

test("E-015 immediate root dispatch rejects by identity, restores capacity, and recovers", async () => {
  const { pool, worker, slot } = makePool();
  const failure = new DOMException(
    "evaluation request could not be cloned",
    "DataCloneError",
  );
  worker.nextPost = () => {
    throw failure;
  };

  const reason = await rejectionOf(pool.evaluate("rules", {}));
  assert.strictEqual(reason, failure);
  assertRunningSlotIdle(pool, slot);
  assertWorkerListenersIntact(worker);

  await completeEvaluation(pool, worker, "immediate-root-recovery");
  assertRunningSlotIdle(pool, slot);
  await pool.close();
  assert.strictEqual(worker.terminateCalls, 1);
});

test("E-015 queued root dispatch survives an abort/send race and advances FIFO", async () => {
  const { pool, worker, slot } = makePool();
  const controller = new AbortController();
  const abortListenerBaseline = getEventListeners(
    controller.signal,
    "abort",
  ).length;
  const failure = new DOMException(
    "queued evaluation could not be cloned",
    "DataCloneError",
  );
  const secondFailure = new DOMException(
    "next queued evaluation also could not be cloned",
    "DataCloneError",
  );

  const first = pool.evaluate("rules", {});
  const failed = pool.evaluate("rules", {}, { signal: controller.signal });
  const failedSecond = pool.evaluate("rules", {});
  const later = pool.evaluate("rules", {});
  assert.strictEqual(worker.messages.length, 1);
  assert.strictEqual(slot.pending.size, 1);
  assert.strictEqual(slot.inflight, 1);
  assert.strictEqual(slot.queue.length, 3);
  assert.strictEqual(
    getEventListeners(controller.signal, "abort").length,
    abortListenerBaseline + 2,
    "a queued evaluation owns its queue listener and in-flight abort flag listener",
  );

  worker.postActions.push(
    () => {
      // drainQueue removes dequeue cancellation before attempting postMessage.
      // An abort in this narrow window must not replace the send failure.
      controller.abort();
      throw failure;
    },
    () => {
      throw secondFailure;
    },
  );

  const firstRequest = worker.messages[0];
  const firstResult = evaluateResult("first-root");
  assert.doesNotThrow(() => {
    worker.emit("message", { id: firstRequest.id, result: firstResult });
  });

  assert.deepStrictEqual(await first, firstResult);
  assert.strictEqual(await rejectionOf(failed), failure);
  assert.strictEqual(await rejectionOf(failedSecond), secondFailure);
  assert.strictEqual(
    getEventListeners(controller.signal, "abort").length,
    abortListenerBaseline,
  );
  assert.strictEqual(worker.messages.length, 4);
  assert.deepStrictEqual(
    worker.messages.map((message) => message.id),
    [0, 1, 2, 3],
    "failed request IDs remain consumed and no request is replayed",
  );
  assert.strictEqual(slot.nextId, 4);
  assert.strictEqual(slot.pending.size, 1);
  assert.strictEqual(slot.inflight, 1);
  assert.strictEqual(slot.queue.length, 0);
  assert.strictEqual(slot.pending.has(worker.messages[3].id), true);
  assertWorkerListenersIntact(worker);

  const laterResult = evaluateResult("later-root");
  worker.emit("message", { id: worker.messages[3].id, result: laterResult });
  assert.deepStrictEqual(await later, laterResult);
  assertRunningSlotIdle(pool, slot);

  await completeEvaluation(pool, worker, "post-fifo-recovery");
  assert.strictEqual(worker.messages.at(-1)?.id, 4);
  assert.strictEqual(slot.nextId, 5);
  assertRunningSlotIdle(pool, slot);
  await pool.close();
});

test("E-015 immediate lease-owner dispatch rolls back lease and slot accounting", async () => {
  const { pool, worker, slot } = makePool();
  const failure = new DOMException(
    "owner request could not be cloned",
    "DataCloneError",
  );
  let capturedLease: any;
  worker.nextPost = () => {
    capturedLease = slot.activeLease;
    throw failure;
  };

  const reason = await rejectionOf(
    pool.do("rules", async (proxy) => proxy.facts()),
  );
  assert.strictEqual(reason, failure);
  assert.ok(capturedLease);
  assertLeaseReleased(capturedLease);
  assertRunningSlotIdle(pool, slot);
  assertWorkerListenersIntact(worker);

  await completeEvaluation(pool, worker, "immediate-owner-recovery");
  assertRunningSlotIdle(pool, slot);
  await pool.close();
});

test("E-015 queued lease-owner dispatch advances its private FIFO and releases", async () => {
  const { pool, worker, slot } = makePool();
  const failure = new DOMException(
    "queued owner request could not be cloned",
    "DataCloneError",
  );
  const secondFailure = new DOMException(
    "next queued owner request also could not be cloned",
    "DataCloneError",
  );
  let failedReasons: unknown[] = [];

  const owner = pool.do("rules", async (proxy) => {
    const first = proxy.facts();
    const failed = proxy.facts();
    const failedSecond = proxy.facts();
    const later = proxy.facts();

    const firstValue = await first;
    failedReasons = [
      await rejectionOf(failed),
      await rejectionOf(failedSecond),
    ];
    const laterValue = await later;
    return { firstValue, laterValue };
  });

  await waitFor(
    () => worker.messages.length === 1 && slot.activeLease?.queue.length === 3,
    "one dispatched and three queued owner requests",
  );
  const lease = slot.activeLease;
  assert.ok(lease);
  assert.strictEqual(lease.pendingCalls, 4);

  worker.postActions.push(
    () => {
      throw failure;
    },
    () => {
      throw secondFailure;
    },
  );
  const firstResult: any[] = [];
  assert.doesNotThrow(() => {
    worker.emit("message", { id: worker.messages[0].id, result: firstResult });
  });

  assert.strictEqual(worker.messages.length, 4);
  assert.deepStrictEqual(
    worker.messages.map((message) => message.id),
    [0, 1, 2, 3],
  );
  assert.strictEqual(slot.nextId, 4);
  assert.strictEqual(slot.pending.size, 1);
  assert.strictEqual(slot.inflight, 1);
  assert.strictEqual(lease.queue.length, 0);
  assert.strictEqual(lease.active, true);
  assert.strictEqual(lease.released, false);
  assert.strictEqual(slot.pending.has(worker.messages[3].id), true);

  const laterResult: any[] = [];
  worker.emit("message", { id: worker.messages[3].id, result: laterResult });
  assert.deepStrictEqual(await owner, {
    firstValue: firstResult,
    laterValue: laterResult,
  });
  assert.deepStrictEqual(failedReasons, [failure, secondFailure]);
  assertLeaseReleased(lease);
  assertRunningSlotIdle(pool, slot);
  assertWorkerListenersIntact(worker);

  await completeEvaluation(pool, worker, "queued-owner-recovery");
  assertRunningSlotIdle(pool, slot);
  await pool.close();
});

test("E-015 terminal-before-throw interleaving retains the first terminal error", async () => {
  const { pool, worker, slot } = makePool();
  const terminal = new Error("worker failed during queued dispatch");
  const sendFailure = new DOMException(
    "postMessage also failed",
    "DataCloneError",
  );

  const first = pool.evaluate("rules", {});
  const terminalRequest = pool.evaluate("rules", {});
  const queuedBehindTerminal = pool.evaluate("rules", {});
  const terminalReason = rejectionOf(terminalRequest);
  const queuedReason = rejectionOf(queuedBehindTerminal);
  assert.strictEqual(slot.queue.length, 2);

  worker.nextPost = () => {
    worker.emit("error", terminal);
    throw sendFailure;
  };
  const firstResult = evaluateResult("before-terminal");
  assert.doesNotThrow(() => {
    worker.emit("message", { id: worker.messages[0].id, result: firstResult });
  });

  assert.deepStrictEqual(await first, firstResult);
  assert.strictEqual(await terminalReason, terminal);
  assert.strictEqual(await queuedReason, terminal);
  assert.strictEqual((pool as any).terminalError, terminal);
  assert.deepStrictEqual(slot.state, { kind: "failed", error: terminal });
  assert.strictEqual(slot.pending.size, 0);
  assert.strictEqual(slot.inflight, 0);
  assert.strictEqual(slot.queue.length, 0);
  assert.strictEqual(worker.listenerCount("message"), 0);
  assert.strictEqual(worker.listenerCount("error"), 0);
  assert.strictEqual(worker.listenerCount("exit"), 0);

  assert.strictEqual(
    await rejectionOf(pool.evaluate("rules", {})),
    terminal,
  );
  await pool.close();
  assert.strictEqual(worker.terminateCalls, 1);
  assert.deepStrictEqual(slot.state, { kind: "closed" });
});

test("E-015 synchronous success and ordinary error responses beat a later send throw", async (t) => {
  const sendFailure = new DOMException(
    "postMessage threw after a response",
    "DataCloneError",
  );
  const cases = [
    {
      name: "success response",
      settle: (worker: PlannedWorker, request: PostedMessage) => {
        worker.emit("message", {
          id: request.id,
          result: evaluateResult("synchronous-success"),
        });
      },
      verify: async (returned: Promise<unknown>) => {
        assert.deepStrictEqual(
          await returned,
          evaluateResult("synchronous-success"),
        );
      },
    },
    {
      name: "ordinary error response",
      settle: (worker: PlannedWorker, request: PostedMessage) => {
        worker.emit("message", {
          id: request.id,
          error: {
            name: "TypeError",
            message: "ordinary response won",
            code: "ERR_SYNTHETIC",
          },
        });
      },
      verify: async (returned: Promise<unknown>) => {
        const reason = await rejectionOf(returned);
        assert.ok(reason instanceof TypeError);
        assert.strictEqual(reason.message, "ordinary response won");
        assert.notStrictEqual(reason, sendFailure);
      },
    },
  ];

  for (const item of cases) {
    await t.test(`E-015 ${item.name} wins first settlement`, async () => {
      const { pool, worker, slot } = makePool();
      const nextIdBefore = slot.nextId;
      worker.nextPost = (request) => {
        item.settle(worker, request);
        throw sendFailure;
      };

      let returned: Promise<unknown> | undefined;
      assert.doesNotThrow(() => {
        returned = pool.evaluate("rules", {});
      });
      assert.ok(returned instanceof Promise);
      await item.verify(returned);
      assert.strictEqual(slot.nextId, nextIdBefore + 1);
      assertRunningSlotIdle(pool, slot);
      assertWorkerListenersIntact(worker);

      await completeEvaluation(pool, worker, `${item.name}-recovery`);
      assertRunningSlotIdle(pool, slot);
      await pool.close();
    });
  }
});

test("E-015 an exit before a send throw keeps the synthesized terminal error", async () => {
  const { pool, worker, slot } = makePool();
  const sendFailure = new DOMException(
    "postMessage threw after exit",
    "DataCloneError",
  );
  worker.nextPost = () => {
    worker.emit("exit", 9);
    throw sendFailure;
  };

  const reason = await rejectionOf(pool.evaluate("rules", {}));
  assert.ok(reason instanceof Error);
  assert.match(reason.message, /unexpectedly with code 9/);
  assert.notStrictEqual(reason, sendFailure);
  assert.strictEqual((pool as any).terminalError, reason);
  assert.deepStrictEqual(slot.state, { kind: "failed", error: reason });
  assert.strictEqual(slot.pending.size, 0);
  assert.strictEqual(slot.inflight, 0);
  assert.strictEqual(slot.queue.length, 0);

  await pool.close();
  assert.strictEqual(worker.terminateCalls, 1);
});

test("E-015 send rollback stays first settlement when a terminal event arrives later", async () => {
  const { pool, worker, slot } = makePool();
  const sendFailure = new DOMException(
    "send failed before terminal event",
    "DataCloneError",
  );
  const terminal = new Error("later terminal event");
  worker.nextPost = () => {
    throw sendFailure;
  };

  const returned = pool.evaluate("rules", {});
  assert.strictEqual(await rejectionOf(returned), sendFailure);
  assertRunningSlotIdle(pool, slot);

  worker.emit("error", terminal);
  assert.strictEqual(await rejectionOf(returned), sendFailure);
  assert.strictEqual((pool as any).terminalError, terminal);
  assert.deepStrictEqual(slot.state, { kind: "failed", error: terminal });
  assert.strictEqual(
    await rejectionOf(pool.evaluate("rules", {})),
    terminal,
  );

  await pool.close();
});

test("E-015 request IDs and round-robin selections are never rewound or replayed", async () => {
  const { pool, workers, slots } = makeMultiWorkerPool(2);
  const failure = new DOMException(
    "slot zero rejected its first send",
    "DataCloneError",
  );
  workers[0].nextPost = () => {
    throw failure;
  };

  assert.strictEqual(await rejectionOf(pool.evaluate("rules", {})), failure);
  assert.strictEqual((pool as any).roundRobin, 1);
  assert.deepStrictEqual(workers[0].messages.map((message) => message.id), [0]);
  assert.deepStrictEqual(workers[1].messages, []);

  const second = pool.evaluate("rules", {});
  assert.strictEqual((pool as any).roundRobin, 0);
  workers[1].emit("message", {
    id: workers[1].messages[0].id,
    result: evaluateResult("slot-one"),
  });
  assert.deepStrictEqual(await second, evaluateResult("slot-one"));

  const third = pool.evaluate("rules", {});
  assert.strictEqual((pool as any).roundRobin, 1);
  workers[0].emit("message", {
    id: workers[0].messages[1].id,
    result: evaluateResult("slot-zero-recovery"),
  });
  assert.deepStrictEqual(await third, evaluateResult("slot-zero-recovery"));

  assert.deepStrictEqual(
    workers[0].messages.map((message) => message.id),
    [0, 1],
  );
  assert.deepStrictEqual(
    workers[1].messages.map((message) => message.id),
    [0],
  );
  assert.strictEqual(slots[0].nextId, 2);
  assert.strictEqual(slots[1].nextId, 1);
  assertRunningSlotIdle(pool, slots[0]);
  assertRunningSlotIdle(pool, slots[1]);

  await pool.close();
  assert.deepStrictEqual(
    workers.map((worker) => worker.terminateCalls),
    [1, 1],
  );
});

test("E-015 pool creation tears down every unpublished Worker after an init send throw", async () => {
  const OriginalWorker = workerThreads.Worker;
  const failure = new DOMException(
    "second Worker init could not be cloned",
    "DataCloneError",
  );

  class InitWorker extends EventEmitter {
    static readonly instances: InitWorker[] = [];

    readonly index: number;
    readonly messages: PostedMessage[] = [];
    terminateCalls = 0;

    constructor(_filename: string) {
      super();
      this.index = InitWorker.instances.length;
      InitWorker.instances.push(this);
    }

    postMessage(message: PostedMessage): void {
      this.messages.push(message);
      assert.strictEqual(message.method, "__init");
      if (this.index === 1) throw failure;
      queueMicrotask(() => {
        this.emit("message", { id: message.id, result: undefined });
      });
    }

    terminate(): Promise<number> {
      this.terminateCalls += 1;
      return Promise.resolve(0);
    }
  }

  workerThreads.Worker = InitWorker as unknown as typeof workerThreads.Worker;
  try {
    let creation: Promise<EnginePool> | undefined;
    assert.doesNotThrow(() => {
      creation = EnginePool.create([{ name: "rules" }], { threads: 3 });
    });
    assert.ok(creation instanceof Promise);
    assert.strictEqual(await rejectionOf(creation), failure);

    assert.strictEqual(InitWorker.instances.length, 3);
    assert.deepStrictEqual(
      InitWorker.instances.map((worker) => worker.messages.length),
      [1, 1, 1],
    );
    assert.deepStrictEqual(
      InitWorker.instances.map((worker) => worker.terminateCalls),
      [1, 1, 1],
    );
    for (const worker of InitWorker.instances) {
      assert.strictEqual(worker.listenerCount("message"), 0);
      assert.strictEqual(worker.listenerCount("error"), 0);
      assert.strictEqual(worker.listenerCount("exit"), 0);
    }
  } finally {
    workerThreads.Worker = OriginalWorker;
  }
});

test("E-015 rollback wakes close already waiting for the failed request", async () => {
  const { pool, worker, slot } = makePool();
  const failure = new DOMException(
    "send failed while close waited",
    "DataCloneError",
  );
  let closePromise: Promise<void> | undefined;

  worker.nextPost = () => {
    closePromise = pool.close();
    assert.strictEqual(slot.pendingDrainWaiters.length, 1);
    throw failure;
  };

  assert.strictEqual(
    await rejectionOf(pool.evaluate("rules", {})),
    failure,
  );
  assert.ok(closePromise instanceof Promise);
  assert.strictEqual(slot.pending.size, 0);
  assert.strictEqual(slot.inflight, 0);
  assert.strictEqual(slot.pendingDrainWaiters.length, 0);
  await closePromise;

  assert.strictEqual(worker.terminateCalls, 1);
  assert.deepStrictEqual(slot.state, { kind: "closed" });
  assert.strictEqual(worker.listenerCount("message"), 0);
  assert.strictEqual(worker.listenerCount("error"), 0);
  assert.strictEqual(worker.listenerCount("exit"), 0);
});

test("E-015 real root and proxy DataCloneErrors preserve pool and lease health", { timeout: 8_000 }, async () => {
  const pool = await EnginePool.create([{ name: "rules" }]);
  const slot = (pool as any).slots[0];
  const worker = slot.worker;
  const workerListenerBaseline = {
    message: worker.listenerCount("message"),
    error: worker.listenerCount("error"),
    exit: worker.listenerCount("exit"),
  };

  const rootIdBefore = slot.nextId;
  let invalidRoot: Promise<unknown> | undefined;
  assert.doesNotThrow(() => {
    invalidRoot = pool.evaluate("rules", {
      facts: [{
        kind: "ordered",
        relation: "uncloneable-root",
        fields: [(() => undefined) as any],
      }],
    });
  });
  assert.ok(invalidRoot instanceof Promise);
  const rootReason = await rejectionOf(invalidRoot);
  assert.ok(rootReason instanceof DOMException);
  assert.strictEqual(rootReason.name, "DataCloneError");
  assert.strictEqual(slot.nextId, rootIdBefore + 1);
  assertRunningSlotIdle(pool, slot);

  const validRoot = await pool.evaluate("rules", {});
  assert.deepStrictEqual(validRoot.facts, []);

  await pool.do("rules", async (proxy) => {
    const lease = slot.activeLease;
    assert.ok(lease);
    const proxyIdBefore = slot.nextId;
    let invalidProxy: Promise<unknown> | undefined;
    assert.doesNotThrow(() => {
      invalidProxy = proxy.assertFact(
        "uncloneable-proxy",
        (() => undefined) as any,
      );
    });
    assert.ok(invalidProxy instanceof Promise);

    const proxyReason = await rejectionOf(invalidProxy);
    assert.ok(proxyReason instanceof DOMException);
    assert.strictEqual(proxyReason.name, "DataCloneError");
    assert.strictEqual(slot.nextId, proxyIdBefore + 1);
    assert.strictEqual(slot.pending.size, 0);
    assert.strictEqual(slot.inflight, 0);
    assert.strictEqual(lease.active, true);
    assert.strictEqual(lease.released, false);
    assert.strictEqual(lease.pendingCalls, 0);

    const ids = await proxy.assertString("(proxy-clone-recovery)");
    assert.strictEqual(ids.length, 1);
  });

  assertRunningSlotIdle(pool, slot);
  assert.deepStrictEqual(
    {
      message: worker.listenerCount("message"),
      error: worker.listenerCount("error"),
      exit: worker.listenerCount("exit"),
    },
    workerListenerBaseline,
  );
  await pool.close();
});
