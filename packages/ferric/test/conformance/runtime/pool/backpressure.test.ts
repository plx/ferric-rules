/**
 * EnginePool bounded-backpressure tests (FR-NODE-011 / N-15 / C-006 / E-017).
 *
 * Fake workers make admission, retention, and reclamation observable without
 * wall-clock timing.  One slot may retain at most its configured number of
 * waiting root, lease-admission, and lease-private requests.
 */
import { execFile } from "node:child_process";
import { EventEmitter, getEventListeners } from "node:events";
import { resolve } from "node:path";
import { test } from "node:test";
import * as assert from "node:assert/strict";
import { setImmediate as yieldImmediate } from "node:timers/promises";

import * as Ferric from "../../../helpers/ferric";

const workerThreads = require("node:worker_threads") as typeof import("node:worker_threads");

interface PostedMessage {
  id: number;
  method: string;
  args: unknown[];
}

class ControlledWorker extends EventEmitter {
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

function makePool(queueCapacity: number, threads = 1): {
  pool: Ferric.EnginePool;
  workers: ControlledWorker[];
  slots: any[];
} {
  const workers = Array.from({ length: threads }, () => new ControlledWorker());
  const slots = workers.map((worker) =>
    (Ferric.EnginePool as any).createSlot(worker)
  );
  const pool = new (Ferric.EnginePool as any)(
    slots,
    queueCapacity,
  ) as Ferric.EnginePool;
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

interface Deferred<T> {
  promise: Promise<T>;
  resolve: (value: T | PromiseLike<T>) => void;
  reject: (reason?: unknown) => void;
}

function deferred<T>(): Deferred<T> {
  let resolvePromise!: (value: T | PromiseLike<T>) => void;
  let rejectPromise!: (reason?: unknown) => void;
  const promise = new Promise<T>((resolveValue, rejectValue) => {
    resolvePromise = resolveValue;
    rejectPromise = rejectValue;
  });
  return { promise, resolve: resolvePromise, reject: rejectPromise };
}

async function waitFor(predicate: () => boolean, label: string): Promise<void> {
  for (let turn = 0; turn < 40; turn++) {
    if (predicate()) return;
    await yieldImmediate();
  }
  assert.fail(`Timed out waiting for ${label}`);
}

function evaluationResult(marker: string): any {
  return {
    runResult: { rulesFired: 0, haltReason: marker },
    facts: [],
    output: {},
  };
}

function respond(
  worker: ControlledWorker,
  message: PostedMessage,
  result: unknown,
): void {
  worker.emit("message", { id: message.id, result });
}

function typedAbortAdd(
  listener: (
    type: string,
    callback: EventListenerOrEventListenerObject,
    options?: boolean | AddEventListenerOptions,
  ) => void,
): AbortSignal["addEventListener"] {
  return listener as AbortSignal["addEventListener"];
}

function typedAbortRemove(
  listener: (
    type: string,
    callback: EventListenerOrEventListenerObject,
    options?: boolean | EventListenerOptions,
  ) => void,
): AbortSignal["removeEventListener"] {
  return listener as AbortSignal["removeEventListener"];
}

function installThrowOnceAbortRemoval(
  signal: AbortSignal,
  failure: Error,
): () => number {
  const originalRemove = signal.removeEventListener.bind(signal);
  let calls = 0;
  signal.removeEventListener = typedAbortRemove((type, listener, options) => {
    const result = originalRemove(type, listener, options);
    if (type === "abort" && ++calls === 1) throw failure;
    return result;
  }) as typeof signal.removeEventListener;
  return () => calls;
}

function assertQueueFullError(
  reason: unknown,
  expected: { capacity: number; queued: number; slotIndex: number },
): asserts reason is Ferric.EnginePoolQueueFullError {
  assert.ok(reason instanceof Ferric.EnginePoolQueueFullError);
  assert.ok(reason instanceof Ferric.FerricError);
  assert.deepStrictEqual(
    {
      name: reason.name,
      message: reason.message,
      code: reason.code,
      capacity: reason.capacity,
      queued: reason.queued,
      slotIndex: reason.slotIndex,
    },
    {
      name: "EnginePoolQueueFullError",
      message: "EnginePool queue is full",
      code: "FERRIC_POOL_QUEUE_FULL",
      ...expected,
    },
  );
}

function assertBoundedMetrics(
  pool: Ferric.EnginePool,
  capacity: number,
  label: string,
): Ferric.EnginePoolMetrics {
  const metrics = pool.metrics();
  assert.strictEqual(metrics.queueCapacity, capacity, label);
  assert.ok(
    metrics.slots.every((slot) => slot.queued <= capacity),
    `${label}: selected-slot capacity exceeded`,
  );
  assert.strictEqual(
    metrics.queued,
    metrics.slots.reduce((sum, slot) => sum + slot.queued, 0),
    `${label}: aggregate queued drifted from slot sum`,
  );
  assert.strictEqual(
    metrics.inFlight,
    metrics.slots.reduce((sum, slot) => sum + slot.inFlight, 0),
    `${label}: aggregate inFlight drifted from slot sum`,
  );
  assert.strictEqual(
    metrics.rejected,
    metrics.slots.reduce((sum, slot) => sum + slot.rejected, 0),
    `${label}: aggregate rejected drifted from slot sum`,
  );
  return metrics;
}

async function failAndClose(
  fixture: ReturnType<typeof makePool>,
  promises: readonly Promise<unknown>[],
): Promise<void> {
  const outcomes = Promise.allSettled(promises);
  const cleanup = new Error("backpressure-test cleanup");
  for (const [index, slot] of fixture.slots.entries()) {
    if (slot.state.kind === "running") {
      fixture.workers[index].emit("error", cleanup);
    }
  }
  await outcomes;
  await fixture.pool.close();
}

test("C-006 E-017 a full root queue rejects promptly without retaining overflow", async () => {
  const capacity = 2;
  const fixture = makePool(capacity);
  const { pool } = fixture;
  const [worker] = fixture.workers;
  const [slot] = fixture.slots;
  const first = pool.evaluate("rules", {});
  const queued = [
    pool.evaluate("rules", {}),
    pool.evaluate("rules", {}),
  ];
  const overflow = pool.evaluate("rules", {});
  const overflowReason = rejectionOf(overflow);

  try {
    assert.strictEqual(worker.messages.length, 1);
    assert.strictEqual(slot.pending.size, 1);
    assert.strictEqual(slot.inflight, 1);
    assert.strictEqual(
      slot.queue.length,
      capacity,
      "the selected slot retained work beyond its configured capacity",
    );
    assert.strictEqual(
      slot.nextId,
      3,
      "overflow allocated a request ID instead of rejecting at admission",
    );

    const reason = await overflowReason;
    assertQueueFullError(reason, { capacity, queued: capacity, slotIndex: 0 });
    assert.deepStrictEqual(pool.metrics(), {
      queueCapacity: capacity,
      queued: capacity,
      inFlight: 1,
      rejected: 1,
      slots: [{ slotIndex: 0, queued: capacity, inFlight: 1, rejected: 1 }],
    });
  } finally {
    await failAndClose(fixture, [first, ...queued, overflow]);
  }
});

test("E-017 queueCapacity validates synchronously and exports detached metrics types", async () => {
  const OriginalWorker = workerThreads.Worker;

  class InitWorker extends ControlledWorker {
    static constructorCalls = 0;

    constructor(_path: string) {
      super();
      InitWorker.constructorCalls += 1;
    }

    override postMessage(message: PostedMessage): void {
      super.postMessage(message);
      if (message.method === "__init") {
        queueMicrotask(() => respond(this, message, undefined));
      }
    }
  }

  workerThreads.Worker = InitWorker as unknown as typeof workerThreads.Worker;
  const specTrap = new Proxy([] as Ferric.EngineSpec[], {
    get() {
      throw new Error("invalid queue capacity inspected specs");
    },
  });
  const capacityMessage =
    "EnginePool.create: 'queueCapacity' must be a non-negative safe integer";

  try {
    const invalid = [
      -1,
      0.5,
      Number.NaN,
      Number.POSITIVE_INFINITY,
      Number.NEGATIVE_INFINITY,
      Number.MAX_SAFE_INTEGER + 1,
      null,
      "4",
    ];
    for (const queueCapacity of invalid) {
      assert.throws(
        () => Ferric.EnginePool.create(specTrap, {
          queueCapacity: queueCapacity as number,
        }),
        (error: unknown) => {
          assert.ok(error instanceof RangeError);
          assert.strictEqual(error.message, capacityMessage);
          return true;
        },
      );
    }
    assert.strictEqual(InitWorker.constructorCalls, 0);

    assert.throws(
      () => Ferric.EnginePool.create(specTrap, {
        threads: 0,
        queueCapacity: -1,
      }),
      (error: unknown) => {
        assert.ok(error instanceof RangeError);
        assert.strictEqual(
          error.message,
          "EnginePool.create: 'threads' must be a safe integer between 1 and 64",
        );
        return true;
      },
      "thread validation did not retain precedence",
    );

    const cases: Array<{
      options?: Ferric.EnginePoolOptions;
      expected: number;
    }> = [
      { expected: 1024 },
      { options: { queueCapacity: undefined }, expected: 1024 },
      { options: { queueCapacity: -0 }, expected: 0 },
      { options: { queueCapacity: Number.MAX_SAFE_INTEGER }, expected: Number.MAX_SAFE_INTEGER },
    ];
    for (const item of cases) {
      const pool = await Ferric.EnginePool.create(
        [{ name: "rules" }],
        item.options,
      );
      const metrics: Ferric.EnginePoolMetrics = pool.metrics();
      const slotMetrics: Ferric.EnginePoolSlotMetrics = metrics.slots[0];
      assert.strictEqual(slotMetrics.slotIndex, 0);
      assert.strictEqual(metrics.queueCapacity, item.expected);
      assert.strictEqual(Object.is(metrics.queueCapacity, -0), false);
      assert.deepStrictEqual(metrics, {
        queueCapacity: item.expected,
        queued: 0,
        inFlight: 0,
        rejected: 0,
        slots: [{ slotIndex: 0, queued: 0, inFlight: 0, rejected: 0 }],
      });
      await pool.close();
    }
    assert.strictEqual(InitWorker.constructorCalls, cases.length);
  } finally {
    workerThreads.Worker = OriginalWorker;
  }
});

test("E-017 mixed root requests and lease admissions retain FIFO as capacity is reclaimed", async () => {
  const fixture = makePool(2);
  const { pool } = fixture;
  const [worker] = fixture.workers;
  const [slot] = fixture.slots;
  const callbackGate = deferred<void>();
  let callbackStarted = false;

  const first = pool.evaluate("rules", {});
  const second = pool.evaluate("rules", {});
  const leased = pool.do("rules", async () => {
    callbackStarted = true;
    await callbackGate.promise;
    return "lease-result";
  });
  const overflow = pool.evaluate("rules", {});
  let later: Promise<Ferric.EvaluateResult> | undefined;

  try {
    assertQueueFullError(await rejectionOf(overflow), {
      capacity: 2,
      queued: 2,
      slotIndex: 0,
    });
    assert.deepStrictEqual(pool.metrics(), {
      queueCapacity: 2,
      queued: 2,
      inFlight: 1,
      rejected: 1,
      slots: [{ slotIndex: 0, queued: 2, inFlight: 1, rejected: 1 }],
    });
    assert.strictEqual(worker.messages.length, 1);
    assert.strictEqual(callbackStarted, false);

    respond(worker, worker.messages[0], evaluationResult("first"));
    assert.deepStrictEqual(await first, evaluationResult("first"));
    assert.strictEqual(worker.messages.length, 2);
    assert.strictEqual(slot.queue.length, 1);

    later = pool.evaluate("rules", {});
    assert.strictEqual(
      slot.queue.length,
      2,
      "response progress did not reclaim one root waiting position",
    );
    assert.strictEqual(slot.nextId, 3, "overflow rewound or consumed an ID");

    respond(worker, worker.messages[1], evaluationResult("second"));
    assert.deepStrictEqual(await second, evaluationResult("second"));
    await waitFor(() => callbackStarted, "queued lease admission");
    assert.deepStrictEqual(pool.metrics(), {
      queueCapacity: 2,
      queued: 1,
      inFlight: 0,
      rejected: 1,
      slots: [{ slotIndex: 0, queued: 1, inFlight: 0, rejected: 1 }],
    });

    callbackGate.resolve();
    assert.strictEqual(await leased, "lease-result");
    await waitFor(() => worker.messages.length === 3, "post-lease root dispatch");
    assert.strictEqual(worker.messages[2].method, "__evaluate");
    respond(worker, worker.messages[2], evaluationResult("later"));
    assert.deepStrictEqual(await later, evaluationResult("later"));
    assert.deepStrictEqual(pool.metrics(), {
      queueCapacity: 2,
      queued: 0,
      inFlight: 0,
      rejected: 1,
      slots: [{ slotIndex: 0, queued: 0, inFlight: 0, rejected: 1 }],
    });
    await pool.close();
  } catch (error) {
    callbackGate.resolve();
    await failAndClose(
      fixture,
      [first, second, leased, overflow, ...(later ? [later] : [])],
    );
    throw error;
  }
});

test("E-017 aborting a queued root request frees capacity and its listeners", async () => {
  const fixture = makePool(1);
  const { pool } = fixture;
  const [worker] = fixture.workers;
  const [slot] = fixture.slots;
  const controller = new AbortController();
  const listenerBaseline = getEventListeners(controller.signal, "abort").length;

  const first = pool.evaluate("rules", {});
  const aborted = pool.evaluate("rules", {}, { signal: controller.signal });
  const overflow = pool.evaluate("rules", {});
  let replacement: Promise<Ferric.EvaluateResult> | undefined;

  try {
    assert.strictEqual(slot.queue.length, 1);
    assert.strictEqual(
      getEventListeners(controller.signal, "abort").length,
      listenerBaseline + 2,
      "queued evaluate did not own both cancellation listeners",
    );
    assertQueueFullError(await rejectionOf(overflow), {
      capacity: 1,
      queued: 1,
      slotIndex: 0,
    });

    controller.abort();
    const abortedReason = await rejectionOf(aborted);
    assert.ok(abortedReason instanceof DOMException);
    assert.strictEqual(abortedReason.name, "AbortError");
    assert.strictEqual(abortedReason.message, "The operation was aborted");
    assert.strictEqual(
      getEventListeners(controller.signal, "abort").length,
      listenerBaseline,
    );
    assert.strictEqual(slot.queue.length, 0);
    assert.deepStrictEqual(pool.metrics(), {
      queueCapacity: 1,
      queued: 0,
      inFlight: 1,
      rejected: 1,
      slots: [{ slotIndex: 0, queued: 0, inFlight: 1, rejected: 1 }],
    });

    replacement = pool.evaluate("rules", {});
    assert.strictEqual(slot.queue.length, 1);
    respond(worker, worker.messages[0], evaluationResult("first"));
    assert.deepStrictEqual(await first, evaluationResult("first"));
    assert.strictEqual(worker.messages.length, 2);
    respond(worker, worker.messages[1], evaluationResult("replacement"));
    assert.deepStrictEqual(await replacement, evaluationResult("replacement"));
    await pool.close();
  } catch (error) {
    await failAndClose(
      fixture,
      [first, aborted, overflow, ...(replacement ? [replacement] : [])],
    );
    throw error;
  }
});

test("E-017 aborting a queued lease admission frees capacity without invoking it", async () => {
  const fixture = makePool(1);
  const { pool } = fixture;
  const [worker] = fixture.workers;
  const [slot] = fixture.slots;
  const controller = new AbortController();
  const listenerBaseline = getEventListeners(controller.signal, "abort").length;
  let abortedCallbackCalls = 0;
  let replacementCallbackCalls = 0;

  const first = pool.evaluate("rules", {});
  const abortedLease = pool.do("rules", async () => {
    abortedCallbackCalls += 1;
  }, { signal: controller.signal });
  const overflow = pool.do("rules", async () => undefined);
  let replacement: Promise<string> | undefined;

  try {
    assert.strictEqual(slot.queue.length, 1);
    assert.strictEqual(
      getEventListeners(controller.signal, "abort").length,
      listenerBaseline + 1,
    );
    assertQueueFullError(await rejectionOf(overflow), {
      capacity: 1,
      queued: 1,
      slotIndex: 0,
    });

    controller.abort();
    const abortReason = await rejectionOf(abortedLease);
    assert.ok(abortReason instanceof DOMException);
    assert.strictEqual(abortReason.name, "AbortError");
    assert.strictEqual(abortReason.message, "The operation was aborted");
    assert.strictEqual(abortedCallbackCalls, 0);
    assert.strictEqual(
      getEventListeners(controller.signal, "abort").length,
      listenerBaseline,
    );
    assert.strictEqual(slot.queue.length, 0);

    replacement = pool.do("rules", async () => {
      replacementCallbackCalls += 1;
      return "replacement";
    });
    assert.strictEqual(slot.queue.length, 1);
    respond(worker, worker.messages[0], evaluationResult("first"));
    assert.deepStrictEqual(await first, evaluationResult("first"));
    assert.strictEqual(await replacement, "replacement");
    assert.strictEqual(replacementCallbackCalls, 1);
    assert.deepStrictEqual(pool.metrics(), {
      queueCapacity: 1,
      queued: 0,
      inFlight: 0,
      rejected: 1,
      slots: [{ slotIndex: 0, queued: 0, inFlight: 0, rejected: 1 }],
    });
    await pool.close();
  } catch (error) {
    await failAndClose(
      fixture,
      [first, abortedLease, overflow, ...(replacement ? [replacement] : [])],
    );
    throw error;
  }
});

test("C-006 E-017 root and lease-private work share one selected-slot budget", async () => {
  const fixture = makePool(2);
  const { pool } = fixture;
  const [worker] = fixture.workers;
  const [slot] = fixture.slots;
  const callbackStarted = deferred<void>();
  const callbackGate = deferred<void>();
  let proxy!: Ferric.EngineProxy;
  let firstOwner!: Promise<readonly Ferric.Fact[]>;
  let secondOwner!: Promise<readonly Ferric.Fact[]>;

  const leased = pool.do("rules", async (engine) => {
    proxy = engine;
    firstOwner = engine.facts();
    secondOwner = engine.facts();
    callbackStarted.resolve();
    await callbackGate.promise;
    return "owner-complete";
  });
  await callbackStarted.promise;

  const root = pool.evaluate("rules", {});
  const ownerOverflow = proxy.facts();
  const rootOverflow = pool.evaluate("rules", {});

  try {
    assertQueueFullError(await rejectionOf(ownerOverflow), {
      capacity: 2,
      queued: 2,
      slotIndex: 0,
    });
    assertQueueFullError(await rejectionOf(rootOverflow), {
      capacity: 2,
      queued: 2,
      slotIndex: 0,
    });
    const lease = slot.activeLease;
    assert.ok(lease);
    assert.strictEqual(worker.messages.length, 1);
    assert.strictEqual(worker.messages[0].method, "facts");
    assert.strictEqual(slot.pending.size, 1);
    assert.strictEqual(slot.queue.length, 1);
    assert.strictEqual(lease.queue.length, 1);
    assert.strictEqual(lease.pendingCalls, 2);
    assert.strictEqual(slot.nextId, 3);
    assert.deepStrictEqual(pool.metrics(), {
      queueCapacity: 2,
      queued: 2,
      inFlight: 1,
      rejected: 2,
      slots: [{ slotIndex: 0, queued: 2, inFlight: 1, rejected: 2 }],
    });

    respond(worker, worker.messages[0], []);
    assert.deepStrictEqual(await firstOwner, []);
    assert.strictEqual(worker.messages.length, 2);
    assert.strictEqual(worker.messages[1].method, "facts");
    assert.deepStrictEqual(pool.metrics(), {
      queueCapacity: 2,
      queued: 1,
      inFlight: 1,
      rejected: 2,
      slots: [{ slotIndex: 0, queued: 1, inFlight: 1, rejected: 2 }],
    });

    respond(worker, worker.messages[1], []);
    assert.deepStrictEqual(await secondOwner, []);
    callbackGate.resolve();
    assert.strictEqual(await leased, "owner-complete");
    assert.strictEqual(worker.messages.length, 3);
    assert.strictEqual(worker.messages[2].method, "__evaluate");
    respond(worker, worker.messages[2], evaluationResult("root"));
    assert.deepStrictEqual(await root, evaluationResult("root"));
    assert.deepStrictEqual(pool.metrics(), {
      queueCapacity: 2,
      queued: 0,
      inFlight: 0,
      rejected: 2,
      slots: [{ slotIndex: 0, queued: 0, inFlight: 0, rejected: 2 }],
    });
    await pool.close();
  } catch (error) {
    callbackGate.resolve();
    await failAndClose(
      fixture,
      [leased, firstOwner, secondOwner, root, ownerOverflow, rootOverflow],
    );
    throw error;
  }
});

test("E-017 queueCapacity zero is immediate-only and root overflow owns no side effects", async () => {
  const fixture = makePool(0);
  const { pool } = fixture;
  const [worker] = fixture.workers;
  const [slot] = fixture.slots;
  const first = pool.evaluate("rules", {});

  const liveController = new AbortController();
  let liveAdds = 0;
  let liveRemoves = 0;
  const liveAdd = liveController.signal.addEventListener.bind(liveController.signal);
  const liveRemove = liveController.signal.removeEventListener.bind(liveController.signal);
  liveController.signal.addEventListener = typedAbortAdd((type, listener, options) => {
    if (type === "abort") liveAdds += 1;
    return liveAdd(type, listener, options);
  }) satisfies typeof liveController.signal.addEventListener;
  liveController.signal.removeEventListener = typedAbortRemove((type, listener, options) => {
    if (type === "abort") liveRemoves += 1;
    return liveRemove(type, listener, options);
  }) as typeof liveController.signal.removeEventListener;

  const overflow = pool.evaluate("rules", {}, { signal: liveController.signal });

  const raceController = new AbortController();
  let raceAdds = 0;
  let raceRemoves = 0;
  const raceAdd = raceController.signal.addEventListener.bind(raceController.signal);
  const raceRemove = raceController.signal.removeEventListener.bind(raceController.signal);
  raceController.signal.addEventListener = typedAbortAdd((type, listener, options) => {
    if (type === "abort") raceAdds += 1;
    return raceAdd(type, listener, options);
  }) as typeof raceController.signal.addEventListener;
  raceController.signal.removeEventListener = typedAbortRemove((type, listener, options) => {
    if (type === "abort") raceRemoves += 1;
    return raceRemove(type, listener, options);
  }) as typeof raceController.signal.removeEventListener;
  const raceRequest = Object.defineProperty({}, "facts", {
    enumerable: true,
    get() {
      raceController.abort();
      return [];
    },
  }) as Ferric.EvaluateRequest;
  const raced = pool.evaluate(
    "rules",
    raceRequest,
    { signal: raceController.signal },
  );

  try {
    assert.strictEqual(
      liveAdds,
      0,
      "queue-full evaluate transiently registered an abort listener",
    );
    assert.strictEqual(liveRemoves, 0);
    assertQueueFullError(await rejectionOf(overflow), {
      capacity: 0,
      queued: 0,
      slotIndex: 0,
    });
    assert.strictEqual(liveAdds, 0);
    assert.strictEqual(liveRemoves, 0);

    const racedReason = await rejectionOf(raced);
    assert.ok(racedReason instanceof DOMException);
    assert.strictEqual(racedReason.name, "AbortError");
    assert.strictEqual(racedReason.message, "The operation was aborted");
    assert.strictEqual(
      raceAdds,
      0,
      "preprocessing abort reached in-flight listener registration",
    );
    assert.strictEqual(raceRemoves, 0);

    assert.strictEqual(worker.messages.length, 1);
    assert.strictEqual(slot.nextId, 1);
    assert.strictEqual(slot.pending.size, 1);
    assert.strictEqual(slot.queue.length, 0);
    assert.deepStrictEqual(pool.metrics(), {
      queueCapacity: 0,
      queued: 0,
      inFlight: 1,
      rejected: 1,
      slots: [{ slotIndex: 0, queued: 0, inFlight: 1, rejected: 1 }],
    });

    respond(worker, worker.messages[0], evaluationResult("first"));
    assert.deepStrictEqual(await first, evaluationResult("first"));
    await pool.close();
  } catch (error) {
    await failAndClose(fixture, [first, overflow, raced]);
    throw error;
  }
});

test("E-017 lease run overflow owns no listener while accepted send observes sync abort", async () => {
  const fixture = makePool(0);
  const { pool } = fixture;
  const [worker] = fixture.workers;
  const [slot] = fixture.slots;
  const controller = new AbortController();
  let addCalls = 0;
  let removeCalls = 0;
  const originalAdd = controller.signal.addEventListener.bind(controller.signal);
  const originalRemove = controller.signal.removeEventListener.bind(controller.signal);
  controller.signal.addEventListener = typedAbortAdd((type, listener, options) => {
    if (type === "abort") addCalls += 1;
    return originalAdd(type, listener, options);
  }) as typeof controller.signal.addEventListener;
  controller.signal.removeEventListener = typedAbortRemove((type, listener, options) => {
    if (type === "abort") removeCalls += 1;
    return originalRemove(type, listener, options);
  }) as typeof controller.signal.removeEventListener;

  const lease = await (pool as any).acquireLease(slot);
  const proxy = (pool as any).makeProxy(
    "rules",
    slot,
    lease,
    controller.signal,
  ) as Ferric.EngineProxy;
  const first = proxy.facts();
  const overflow = proxy.run({ limit: 1 });
  let accepted: Promise<Ferric.RunResult> | undefined;

  try {
    assertQueueFullError(await rejectionOf(overflow), {
      capacity: 0,
      queued: 0,
      slotIndex: 0,
    });
    assert.strictEqual(addCalls, 0, "overflow run installed a cooperative listener");
    assert.strictEqual(removeCalls, 0);
    assert.strictEqual(getEventListeners(controller.signal, "abort").length, 0);
    assert.strictEqual(slot.nextId, 1);
    assert.strictEqual(lease.pendingCalls, 1);
    assert.strictEqual(worker.messages.length, 1);

    respond(worker, worker.messages[0], []);
    assert.deepStrictEqual(await first, []);
    assert.strictEqual(lease.pendingCalls, 0);

    worker.nextPost = (message) => {
      assert.strictEqual(message.method, "__batched_run");
      assert.strictEqual(
        getEventListeners(controller.signal, "abort").length,
        1,
        "accepted run posted before installing cooperative cancellation",
      );
      const abortFlag = new Int32Array(message.args[2] as SharedArrayBuffer);
      assert.strictEqual(Atomics.load(abortFlag, 0), 0);
      controller.abort();
      assert.strictEqual(Atomics.load(abortFlag, 0), 1);
    };
    accepted = proxy.run({ limit: 1 });
    assert.strictEqual(addCalls, 1);
    assert.strictEqual(worker.messages.length, 2);
    assert.strictEqual(slot.nextId, 2);
    respond(worker, worker.messages[1], { rulesFired: 1, haltReason: 2 });
    assert.deepStrictEqual(await accepted, { rulesFired: 1, haltReason: 2 });
    assert.strictEqual(removeCalls, 1);
    assert.strictEqual(getEventListeners(controller.signal, "abort").length, 0);
    assert.strictEqual(lease.pendingCalls, 0);
    assert.deepStrictEqual(pool.metrics(), {
      queueCapacity: 0,
      queued: 0,
      inFlight: 0,
      rejected: 1,
      slots: [{ slotIndex: 0, queued: 0, inFlight: 0, rejected: 1 }],
    });

    await (pool as any).releaseLease(slot, lease);
    await pool.close();
  } catch (error) {
    await (pool as any).releaseLease(slot, lease);
    await failAndClose(
      fixture,
      [first, overflow, ...(accepted ? [accepted] : [])],
    );
    throw error;
  }
});

test("E-017 a root cooperative-listener hook that fills the slot cannot exceed capacity", async () => {
  const fixture = makePool(0);
  const { pool } = fixture;
  const [worker] = fixture.workers;
  const [slot] = fixture.slots;
  const controller = new AbortController();
  const listenerBaseline = getEventListeners(controller.signal, "abort").length;
  const originalAdd = controller.signal.addEventListener.bind(controller.signal);
  const originalRemove = controller.signal.removeEventListener.bind(controller.signal);
  const removalFailure = new Error("root cooperative removal failed");
  let addCalls = 0;
  let removeCalls = 0;
  let hookCalls = 0;
  let nested!: Promise<Ferric.EvaluateResult>;

  controller.signal.addEventListener = typedAbortAdd((type, listener, options) => {
    if (type === "abort") {
      addCalls += 1;
      if (hookCalls++ === 0) {
        nested = pool.evaluate("rules", {});
      }
    }
    return originalAdd(type, listener, options);
  }) as typeof controller.signal.addEventListener;
  controller.signal.removeEventListener = typedAbortRemove((type, listener, options) => {
    const result = originalRemove(type, listener, options);
    if (type === "abort" && ++removeCalls === 1) throw removalFailure;
    return result;
  }) as typeof controller.signal.removeEventListener;

  const outer = pool.evaluate("rules", {}, { signal: controller.signal });

  try {
    assertQueueFullError(await rejectionOf(outer), {
      capacity: 0,
      queued: 0,
      slotIndex: 0,
    });
    assert.strictEqual(worker.messages.length, 1, "both reentrant calls posted");
    assert.strictEqual(slot.nextId, 1, "rejected outer call allocated an ID");
    assert.strictEqual(slot.queue.length, 0);
    assert.strictEqual(slot.inflight, 1);
    assert.strictEqual(addCalls, 1);
    assert.strictEqual(removeCalls, 1);
    assert.strictEqual(
      getEventListeners(controller.signal, "abort").length,
      listenerBaseline,
    );
    assert.deepStrictEqual(pool.metrics(), {
      queueCapacity: 0,
      queued: 0,
      inFlight: 1,
      rejected: 1,
      slots: [{ slotIndex: 0, queued: 0, inFlight: 1, rejected: 1 }],
    });

    respond(worker, worker.messages[0], evaluationResult("nested"));
    assert.deepStrictEqual(await nested, evaluationResult("nested"));
    await pool.close();
  } catch (error) {
    await failAndClose(fixture, [outer, nested]);
    throw error;
  }
});

test("E-017 a root cooperative-listener hook that drains the slot dispatches instead of stranding", async () => {
  const fixture = makePool(1);
  const { pool } = fixture;
  const [worker] = fixture.workers;
  const [slot] = fixture.slots;
  const first = pool.evaluate("rules", {});
  const controller = new AbortController();
  const listenerBaseline = getEventListeners(controller.signal, "abort").length;
  const originalAdd = controller.signal.addEventListener.bind(controller.signal);
  const originalRemove = controller.signal.removeEventListener.bind(controller.signal);
  let addCalls = 0;
  let removeCalls = 0;

  controller.signal.addEventListener = typedAbortAdd((type, listener, options) => {
    if (type === "abort") {
      addCalls += 1;
      if (addCalls === 1) {
        respond(worker, worker.messages[0], evaluationResult("first"));
      }
    }
    return originalAdd(type, listener, options);
  }) as typeof controller.signal.addEventListener;
  controller.signal.removeEventListener = typedAbortRemove((type, listener, options) => {
    if (type === "abort") removeCalls += 1;
    return originalRemove(type, listener, options);
  }) as typeof controller.signal.removeEventListener;

  const outer = pool.evaluate("rules", {}, { signal: controller.signal });

  try {
    assert.deepStrictEqual(await first, evaluationResult("first"));
    assert.strictEqual(worker.messages.length, 2);
    assert.strictEqual(slot.queue.length, 0, "accepted work stranded on an idle slot");
    assert.strictEqual(slot.inflight, 1);
    assert.strictEqual(slot.nextId, 2);
    assert.strictEqual(addCalls, 1);
    respond(worker, worker.messages[1], evaluationResult("outer"));
    assert.deepStrictEqual(await outer, evaluationResult("outer"));
    assert.strictEqual(removeCalls, 1);
    assert.strictEqual(
      getEventListeners(controller.signal, "abort").length,
      listenerBaseline,
    );
    assert.deepStrictEqual(pool.metrics(), {
      queueCapacity: 1,
      queued: 0,
      inFlight: 0,
      rejected: 0,
      slots: [{ slotIndex: 0, queued: 0, inFlight: 0, rejected: 0 }],
    });
    await pool.close();
  } catch (error) {
    await failAndClose(fixture, [first, outer]);
    throw error;
  }
});

test("E-017 a proxy cooperative-listener hook that fills the lease cannot exceed capacity", async () => {
  const fixture = makePool(0);
  const { pool } = fixture;
  const [worker] = fixture.workers;
  const [slot] = fixture.slots;
  const controller = new AbortController();
  const listenerBaseline = getEventListeners(controller.signal, "abort").length;
  const originalAdd = controller.signal.addEventListener.bind(controller.signal);
  const originalRemove = controller.signal.removeEventListener.bind(controller.signal);
  const removalFailure = new Error("proxy cooperative removal failed");
  const lease = await (pool as any).acquireLease(slot);
  const proxy = (pool as any).makeProxy(
    "rules",
    slot,
    lease,
    controller.signal,
  ) as Ferric.EngineProxy;
  let addCalls = 0;
  let removeCalls = 0;
  let nested!: Promise<readonly Ferric.Fact[]>;

  controller.signal.addEventListener = typedAbortAdd((type, listener, options) => {
    if (type === "abort") {
      addCalls += 1;
      if (addCalls === 1) nested = proxy.facts();
    }
    return originalAdd(type, listener, options);
  }) as typeof controller.signal.addEventListener;
  controller.signal.removeEventListener = typedAbortRemove((type, listener, options) => {
    const result = originalRemove(type, listener, options);
    if (type === "abort" && ++removeCalls === 1) throw removalFailure;
    return result;
  }) as typeof controller.signal.removeEventListener;

  const outer = proxy.run({ limit: 1 });

  try {
    assertQueueFullError(await rejectionOf(outer), {
      capacity: 0,
      queued: 0,
      slotIndex: 0,
    });
    assert.strictEqual(worker.messages.length, 1, "both owner calls posted");
    assert.strictEqual(worker.messages[0].method, "facts");
    assert.strictEqual(slot.nextId, 1);
    assert.strictEqual(slot.inflight, 1);
    assert.strictEqual(lease.queue.length, 0);
    assert.strictEqual(lease.pendingCalls, 1);
    assert.strictEqual(addCalls, 1);
    assert.strictEqual(removeCalls, 1);
    assert.strictEqual(
      getEventListeners(controller.signal, "abort").length,
      listenerBaseline,
    );
    assert.strictEqual(pool.metrics().rejected, 1);

    respond(worker, worker.messages[0], []);
    assert.deepStrictEqual(await nested, []);
    assert.strictEqual(lease.pendingCalls, 0);
    await (pool as any).releaseLease(slot, lease);
    await pool.close();
  } catch (error) {
    await (pool as any).releaseLease(slot, lease);
    await failAndClose(fixture, [outer, nested]);
    throw error;
  }
});

test("E-017 a proxy cooperative-listener hook that drains the lease dispatches instead of stranding", async () => {
  const fixture = makePool(1);
  const { pool } = fixture;
  const [worker] = fixture.workers;
  const [slot] = fixture.slots;
  const controller = new AbortController();
  const listenerBaseline = getEventListeners(controller.signal, "abort").length;
  const originalAdd = controller.signal.addEventListener.bind(controller.signal);
  const originalRemove = controller.signal.removeEventListener.bind(controller.signal);
  const lease = await (pool as any).acquireLease(slot);
  const proxy = (pool as any).makeProxy(
    "rules",
    slot,
    lease,
    controller.signal,
  ) as Ferric.EngineProxy;
  const first = proxy.facts();
  let addCalls = 0;
  let removeCalls = 0;

  controller.signal.addEventListener = typedAbortAdd((type, listener, options) => {
    if (type === "abort") {
      addCalls += 1;
      if (addCalls === 1) respond(worker, worker.messages[0], []);
    }
    return originalAdd(type, listener, options);
  }) as typeof controller.signal.addEventListener;
  controller.signal.removeEventListener = typedAbortRemove((type, listener, options) => {
    if (type === "abort") removeCalls += 1;
    return originalRemove(type, listener, options);
  }) as typeof controller.signal.removeEventListener;

  const outer = proxy.run({ limit: 1 });

  try {
    assert.deepStrictEqual(await first, []);
    assert.strictEqual(worker.messages.length, 2);
    assert.strictEqual(worker.messages[1].method, "__batched_run");
    assert.strictEqual(lease.queue.length, 0, "owner work stranded on idle lease");
    assert.strictEqual(slot.inflight, 1);
    assert.strictEqual(lease.pendingCalls, 1);
    assert.strictEqual(addCalls, 1);
    respond(worker, worker.messages[1], { rulesFired: 1, haltReason: 2 });
    assert.deepStrictEqual(await outer, { rulesFired: 1, haltReason: 2 });
    assert.strictEqual(removeCalls, 1);
    assert.strictEqual(
      getEventListeners(controller.signal, "abort").length,
      listenerBaseline,
    );
    assert.deepStrictEqual(pool.metrics(), {
      queueCapacity: 1,
      queued: 0,
      inFlight: 0,
      rejected: 0,
      slots: [{ slotIndex: 0, queued: 0, inFlight: 0, rejected: 0 }],
    });
    await (pool as any).releaseLease(slot, lease);
    await pool.close();
  } catch (error) {
    await (pool as any).releaseLease(slot, lease);
    await failAndClose(fixture, [first, outer]);
    throw error;
  }
});

test("E-017 root cooperative-listener hooks preserve lifecycle and abort precedence", async (t) => {
  const cases = ["closed", "terminal", "aborted"] as const;

  for (const kind of cases) {
    await t.test(kind, async () => {
      const fixture = makePool(0);
      const { pool } = fixture;
      const [worker] = fixture.workers;
      const [slot] = fixture.slots;
      const controller = new AbortController();
      const listenerBaseline = getEventListeners(controller.signal, "abort").length;
      const originalAdd = controller.signal.addEventListener.bind(controller.signal);
      const originalRemove = controller.signal.removeEventListener.bind(controller.signal);
      const terminal = new Error("terminal during root cooperative hook");
      let closing: Promise<void> | undefined;
      let addCalls = 0;
      let removeCalls = 0;

      controller.signal.addEventListener = typedAbortAdd((type, listener, options) => {
        if (type === "abort") addCalls += 1;
        const result = originalAdd(type, listener, options);
        if (type === "abort") {
          if (kind === "closed") closing = pool.close();
          if (kind === "terminal") worker.emit("error", terminal);
          if (kind === "aborted") controller.abort();
        }
        return result;
      }) as typeof controller.signal.addEventListener;
      controller.signal.removeEventListener = typedAbortRemove((type, listener, options) => {
        if (type === "abort") removeCalls += 1;
        return originalRemove(type, listener, options);
      }) as typeof controller.signal.removeEventListener;

      const outer = pool.evaluate("rules", {}, { signal: controller.signal });

      try {
        const reason = await rejectionOf(outer);
        if (kind === "closed") {
          assert.ok(reason instanceof Error);
          assert.strictEqual(reason.message, "EnginePool has been closed");
        } else if (kind === "terminal") {
          assert.strictEqual(reason, terminal);
        } else {
          assert.ok(reason instanceof DOMException);
          assert.strictEqual(reason.name, "AbortError");
          assert.strictEqual(reason.message, "The operation was aborted");
        }
        assert.strictEqual(addCalls, 1);
        assert.strictEqual(removeCalls, 1);
        assert.strictEqual(slot.nextId, 0);
        assert.strictEqual(slot.queue.length, 0);
        assert.strictEqual(slot.pending.size, 0);
        assert.strictEqual(slot.inflight, 0);
        assert.strictEqual(worker.messages.length, 0);
        assert.strictEqual(
          getEventListeners(controller.signal, "abort").length,
          listenerBaseline,
        );
        assert.strictEqual(pool.metrics().rejected, 0);
        if (closing) await closing;
        await pool.close();
      } catch (error) {
        if (slot.state.kind === "running") {
          worker.emit("error", new Error("root cooperative cleanup"));
        }
        await Promise.allSettled([outer, ...(closing ? [closing] : [])]);
        await pool.close();
        throw error;
      }
    });
  }
});

test("E-017 proxy cooperative-listener hooks preserve lifetime terminal closed and abort precedence", async (t) => {
  const cases = ["inactive", "terminal", "non-running", "aborted"] as const;

  for (const kind of cases) {
    await t.test(kind, async () => {
      const fixture = makePool(0);
      const { pool } = fixture;
      const [worker] = fixture.workers;
      const [slot] = fixture.slots;
      const controller = new AbortController();
      const listenerBaseline = getEventListeners(controller.signal, "abort").length;
      const originalAdd = controller.signal.addEventListener.bind(controller.signal);
      const originalRemove = controller.signal.removeEventListener.bind(controller.signal);
      const terminal = new Error("terminal during proxy cooperative hook");
      const lease = await (pool as any).acquireLease(slot);
      const proxy = (pool as any).makeProxy(
        "rules",
        slot,
        lease,
        controller.signal,
      ) as Ferric.EngineProxy;
      let hookCompletion: Promise<unknown> | undefined;
      let addCalls = 0;
      let removeCalls = 0;

      controller.signal.addEventListener = typedAbortAdd((type, listener, options) => {
        if (type === "abort") addCalls += 1;
        const result = originalAdd(type, listener, options);
        if (type === "abort") {
          if (kind === "inactive") {
            hookCompletion = (pool as any).releaseLease(slot, lease);
          }
          if (kind === "terminal") worker.emit("error", terminal);
          if (kind === "non-running") {
            hookCompletion = (Ferric.EnginePool as any).terminateSlot(slot);
          }
          if (kind === "aborted") controller.abort();
        }
        return result;
      }) as typeof controller.signal.addEventListener;
      controller.signal.removeEventListener = typedAbortRemove((type, listener, options) => {
        if (type === "abort") removeCalls += 1;
        return originalRemove(type, listener, options);
      }) as typeof controller.signal.removeEventListener;

      const outer = proxy.run({ limit: 1 });

      try {
        const reason = await rejectionOf(outer);
        if (kind === "inactive") {
          assert.ok(reason instanceof Error);
          assert.strictEqual(
            reason.message,
            "EngineProxy is no longer valid outside its EnginePool.do callback",
          );
        } else if (kind === "terminal") {
          assert.strictEqual(reason, terminal);
        } else if (kind === "non-running") {
          assert.ok(reason instanceof Error);
          assert.strictEqual(reason.message, "EnginePool has been closed");
        } else {
          assert.ok(reason instanceof DOMException);
          assert.strictEqual(reason.name, "AbortError");
          assert.strictEqual(reason.message, "The operation was aborted");
        }
        assert.strictEqual(addCalls, 1);
        assert.strictEqual(removeCalls, 1);
        assert.strictEqual(slot.nextId, 0);
        assert.strictEqual(slot.queue.length, 0);
        assert.strictEqual(slot.pending.size, 0);
        assert.strictEqual(slot.inflight, 0);
        assert.strictEqual(lease.pendingCalls, 0);
        assert.strictEqual(worker.messages.length, 0);
        assert.strictEqual(
          getEventListeners(controller.signal, "abort").length,
          listenerBaseline,
        );
        assert.strictEqual(pool.metrics().rejected, 0);
        if (hookCompletion) await hookCompletion;
        await (pool as any).releaseLease(slot, lease);
        await pool.close();
      } catch (error) {
        if (hookCompletion) await Promise.allSettled([hookCompletion]);
        await (pool as any).releaseLease(slot, lease);
        if (slot.state.kind === "running") {
          worker.emit("error", new Error("proxy cooperative cleanup"));
        }
        await Promise.allSettled([outer]);
        await pool.close();
        throw error;
      }
    });
  }
});

test("E-017 a queued root listener reserves capacity before reentrant admission and synchronous abort", async () => {
  const fixture = makePool(1);
  const { pool } = fixture;
  const [worker] = fixture.workers;
  const [slot] = fixture.slots;
  const first = pool.evaluate("rules", {});
  const controller = new AbortController();
  const listenerBaseline = getEventListeners(controller.signal, "abort").length;
  const originalAdd = controller.signal.addEventListener.bind(controller.signal);
  const originalRemove = controller.signal.removeEventListener.bind(controller.signal);
  const removalFailure = new Error("replaceable root listener removal failed");
  let addCalls = 0;
  let removeCalls = 0;

  controller.signal.removeEventListener = typedAbortRemove((type, listener, options) => {
    const result = originalRemove(type, listener, options);
    if (type === "abort" && ++removeCalls === 1) throw removalFailure;
    return result;
  }) as typeof controller.signal.removeEventListener;
  let maximumQueued = 0;
  let nestedReason!: Promise<unknown>;

  controller.signal.addEventListener = typedAbortAdd((type, listener, options) => {
    if (type === "abort") {
      addCalls += 1;
      maximumQueued = Math.max(maximumQueued, pool.metrics().queued);
      if (addCalls === 2) {
        nestedReason = rejectionOf(pool.evaluate("rules", {}));
        maximumQueued = Math.max(maximumQueued, pool.metrics().queued);
      }
    }
    const result = originalAdd(type, listener, options);
    if (type === "abort" && addCalls === 2) controller.abort();
    return result;
  }) as typeof controller.signal.addEventListener;

  const outer = pool.evaluate("rules", {}, { signal: controller.signal });

  try {
    assertQueueFullError(await nestedReason, {
      capacity: 1,
      queued: 1,
      slotIndex: 0,
    });
    const outerReason = await rejectionOf(outer);
    assert.ok(outerReason instanceof DOMException);
    assert.strictEqual(outerReason.name, "AbortError");
    assert.strictEqual(outerReason.message, "The operation was aborted");
    assert.strictEqual(addCalls, 2);
    assert.strictEqual(
      removeCalls,
      3,
      "abort reconciliation did not retry the caught listener-removal failure",
    );
    assert.strictEqual(maximumQueued, 1);
    assert.strictEqual(slot.queue.length, 0);
    assert.strictEqual(slot.nextId, 2, "nested overflow consumed a request ID");
    assert.strictEqual(worker.messages.length, 1);
    assert.strictEqual(
      getEventListeners(controller.signal, "abort").length,
      listenerBaseline,
    );
    assert.deepStrictEqual(pool.metrics(), {
      queueCapacity: 1,
      queued: 0,
      inFlight: 1,
      rejected: 1,
      slots: [{ slotIndex: 0, queued: 0, inFlight: 1, rejected: 1 }],
    });

    respond(worker, worker.messages[0], evaluationResult("first"));
    assert.deepStrictEqual(await first, evaluationResult("first"));
    await pool.close();
  } catch (error) {
    await failAndClose(fixture, [first, outer]);
    throw error;
  }
});

test("E-017 queued root reconciliation removes a listener delegated after synchronous abort", async () => {
  const fixture = makePool(1);
  const { pool } = fixture;
  const [worker] = fixture.workers;
  const [slot] = fixture.slots;
  const first = pool.evaluate("rules", {});
  const controller = new AbortController();
  const listenerBaseline = getEventListeners(controller.signal, "abort").length;
  const originalAdd = controller.signal.addEventListener.bind(controller.signal);
  const originalRemove = controller.signal.removeEventListener.bind(controller.signal);
  const removalFailure = new Error("root removal failed before delegation");
  let addCalls = 0;
  let removeCalls = 0;

  controller.signal.removeEventListener = typedAbortRemove((type, listener, options) => {
    if (type === "abort" && ++removeCalls === 1) throw removalFailure;
    return originalRemove(type, listener, options);
  }) as typeof controller.signal.removeEventListener;

  controller.signal.addEventListener = typedAbortAdd((type, listener, options) => {
    if (type === "abort") {
      addCalls += 1;
      if (addCalls === 2) {
        assert.strictEqual(slot.queue.length, 1);
        controller.abort();
      }
    }
    return originalAdd(type, listener, options);
  }) as typeof controller.signal.addEventListener;

  const outer = pool.evaluate("rules", {}, { signal: controller.signal });

  try {
    const reason = await rejectionOf(outer);
    assert.ok(reason instanceof DOMException);
    assert.strictEqual(reason.name, "AbortError");
    assert.strictEqual(reason.message, "The operation was aborted");
    assert.strictEqual(addCalls, 2);
    assert.strictEqual(removeCalls, 3);
    assert.strictEqual(slot.queue.length, 0);
    assert.strictEqual(slot.nextId, 2);
    assert.strictEqual(worker.messages.length, 1);
    assert.strictEqual(
      getEventListeners(controller.signal, "abort").length,
      listenerBaseline,
    );
    assert.strictEqual(pool.metrics().rejected, 0);
    respond(worker, worker.messages[0], evaluationResult("first"));
    assert.deepStrictEqual(await first, evaluationResult("first"));
    await pool.close();
  } catch (error) {
    await failAndClose(fixture, [first, outer]);
    throw error;
  }
});

test("E-017 a queued root listener hook may dispatch before throwing without replacing its result", async () => {
  const fixture = makePool(1);
  const { pool } = fixture;
  const [worker] = fixture.workers;
  const [slot] = fixture.slots;
  const first = pool.evaluate("rules", {});
  const controller = new AbortController();
  const listenerBaseline = getEventListeners(controller.signal, "abort").length;
  const originalAdd = controller.signal.addEventListener.bind(controller.signal);
  const registrationFailure = new Error("root dequeue-listener registration failed");
  const removalFailure = new Error("root dispatch listener removal failed");
  const removalCalls = installThrowOnceAbortRemoval(
    controller.signal,
    removalFailure,
  );
  let addCalls = 0;

  controller.signal.addEventListener = typedAbortAdd((type, listener, options) => {
    if (type === "abort") {
      addCalls += 1;
      if (addCalls === 2) {
        assert.strictEqual(slot.queue.length, 1);
        respond(worker, worker.messages[0], evaluationResult("first"));
        assert.strictEqual(slot.queue.length, 0);
        assert.strictEqual(worker.messages.length, 2);
        throw registrationFailure;
      }
    }
    return originalAdd(type, listener, options);
  }) as typeof controller.signal.addEventListener;

  worker.nextPost = (message) => {
    assert.strictEqual(message.method, "__evaluate");
    assert.strictEqual(
      getEventListeners(controller.signal, "abort").length,
      listenerBaseline + 1,
      "reentrant dequeue posted before the cooperative listener existed",
    );
  };
  const outer = pool.evaluate("rules", {}, { signal: controller.signal });

  try {
    assert.deepStrictEqual(await first, evaluationResult("first"));
    assert.strictEqual(addCalls, 2);
    assert.strictEqual(slot.queue.length, 0);
    assert.strictEqual(slot.inflight, 1);
    assert.strictEqual(slot.pending.size, 1);
    respond(worker, worker.messages[1], evaluationResult("outer"));
    assert.deepStrictEqual(
      await outer,
      evaluationResult("outer"),
      "listener throw replaced an already-dispatched response",
    );
    assert.strictEqual(
      getEventListeners(controller.signal, "abort").length,
      listenerBaseline,
    );
    assert.strictEqual(removalCalls(), 2);
    assert.strictEqual(pool.metrics().rejected, 0);
    await pool.close();
  } catch (error) {
    await failAndClose(fixture, [first, outer]);
    throw error;
  }
});

test("E-017 a queued root listener throw rolls back its structural reservation", async () => {
  const fixture = makePool(1);
  const { pool } = fixture;
  const [worker] = fixture.workers;
  const [slot] = fixture.slots;
  const first = pool.evaluate("rules", {});
  const controller = new AbortController();
  const listenerBaseline = getEventListeners(controller.signal, "abort").length;
  const originalAdd = controller.signal.addEventListener.bind(controller.signal);
  const registrationFailure = new Error("root queued registration failed");
  let addCalls = 0;
  let replacement: Promise<Ferric.EvaluateResult> | undefined;

  controller.signal.addEventListener = typedAbortAdd((type, listener, options) => {
    if (type === "abort") {
      addCalls += 1;
      if (addCalls === 2) {
        assert.strictEqual(slot.queue.length, 1);
        throw registrationFailure;
      }
    }
    return originalAdd(type, listener, options);
  }) as typeof controller.signal.addEventListener;

  const outer = pool.evaluate("rules", {}, { signal: controller.signal });

  try {
    assert.strictEqual(await rejectionOf(outer), registrationFailure);
    assert.strictEqual(addCalls, 2);
    assert.strictEqual(slot.queue.length, 0);
    assert.strictEqual(slot.nextId, 2);
    assert.strictEqual(pool.metrics().rejected, 0);
    assert.strictEqual(
      getEventListeners(controller.signal, "abort").length,
      listenerBaseline,
    );

    replacement = pool.evaluate("rules", {});
    assert.strictEqual(slot.queue.length, 1, "throw did not reclaim capacity");
    respond(worker, worker.messages[0], evaluationResult("first"));
    assert.deepStrictEqual(await first, evaluationResult("first"));
    assert.strictEqual(worker.messages.length, 2);
    respond(worker, worker.messages[1], evaluationResult("replacement"));
    assert.deepStrictEqual(await replacement, evaluationResult("replacement"));
    await pool.close();
  } catch (error) {
    await failAndClose(
      fixture,
      [first, outer, ...(replacement ? [replacement] : [])],
    );
    throw error;
  }
});

test("E-017 terminal cleanup during queued root registration preserves the terminal error and no listener", async () => {
  const fixture = makePool(1);
  const { pool } = fixture;
  const [worker] = fixture.workers;
  const [slot] = fixture.slots;
  const terminal = new Error("terminal during root listener registration");
  const first = pool.evaluate("rules", {});
  const firstReason = rejectionOf(first);
  const controller = new AbortController();
  const listenerBaseline = getEventListeners(controller.signal, "abort").length;
  const originalAdd = controller.signal.addEventListener.bind(controller.signal);
  const removalFailure = new Error("root terminal listener removal failed");
  const removalCalls = installThrowOnceAbortRemoval(
    controller.signal,
    removalFailure,
  );
  let addCalls = 0;

  controller.signal.addEventListener = typedAbortAdd((type, listener, options) => {
    if (type === "abort") {
      addCalls += 1;
      if (addCalls === 2) worker.emit("error", terminal);
    }
    return originalAdd(type, listener, options);
  }) as typeof controller.signal.addEventListener;

  const outer = pool.evaluate("rules", {}, { signal: controller.signal });

  try {
    assert.strictEqual(await firstReason, terminal);
    assert.strictEqual(await rejectionOf(outer), terminal);
    assert.strictEqual(addCalls, 2);
    assert.strictEqual(slot.queue.length, 0);
    assert.strictEqual(slot.pending.size, 0);
    assert.strictEqual(slot.inflight, 0);
    assert.deepStrictEqual(slot.state, { kind: "failed", error: terminal });
    assert.strictEqual(
      getEventListeners(controller.signal, "abort").length,
      listenerBaseline,
    );
    assert.strictEqual(removalCalls(), 3);
    assert.deepStrictEqual(pool.metrics(), {
      queueCapacity: 1,
      queued: 0,
      inFlight: 0,
      rejected: 0,
      slots: [{ slotIndex: 0, queued: 0, inFlight: 0, rejected: 0 }],
    });
    await pool.close();
  } catch (error) {
    await Promise.allSettled([first, outer]);
    await pool.close();
    throw error;
  }
});

test("E-017 close during queued root registration preserves close settlement through a removal throw", async () => {
  const fixture = makePool(1);
  const { pool } = fixture;
  const [worker] = fixture.workers;
  const [slot] = fixture.slots;
  const first = pool.evaluate("rules", {});
  const controller = new AbortController();
  const listenerBaseline = getEventListeners(controller.signal, "abort").length;
  const originalAdd = controller.signal.addEventListener.bind(controller.signal);
  const removalFailure = new Error("root close listener removal failed");
  const removalCalls = installThrowOnceAbortRemoval(
    controller.signal,
    removalFailure,
  );
  let addCalls = 0;
  let closing!: Promise<void>;

  controller.signal.addEventListener = typedAbortAdd((type, listener, options) => {
    if (type === "abort" && ++addCalls === 2) closing = pool.close();
    return originalAdd(type, listener, options);
  }) as typeof controller.signal.addEventListener;

  const outer = pool.evaluate("rules", {}, { signal: controller.signal });

  try {
    const reason = await rejectionOf(outer);
    assert.ok(reason instanceof Error);
    assert.strictEqual(reason.message, "EnginePool closed");
    assert.strictEqual(addCalls, 2);
    assert.strictEqual(slot.queue.length, 0);
    assert.strictEqual(slot.pending.size, 1);
    assert.strictEqual(slot.inflight, 1);
    assert.strictEqual(
      getEventListeners(controller.signal, "abort").length,
      listenerBaseline,
    );
    assert.strictEqual(removalCalls(), 3);
    assert.deepStrictEqual(pool.metrics(), {
      queueCapacity: 1,
      queued: 0,
      inFlight: 1,
      rejected: 0,
      slots: [{ slotIndex: 0, queued: 0, inFlight: 1, rejected: 0 }],
    });

    respond(worker, worker.messages[0], evaluationResult("first"));
    assert.deepStrictEqual(await first, evaluationResult("first"));
    await closing;
    assert.strictEqual(worker.terminateCalls, 1);
    assert.deepStrictEqual(pool.metrics(), {
      queueCapacity: 1,
      queued: 0,
      inFlight: 0,
      rejected: 0,
      slots: [{ slotIndex: 0, queued: 0, inFlight: 0, rejected: 0 }],
    });
  } catch (error) {
    if (slot.state.kind === "running") {
      worker.emit("error", new Error("root close removal cleanup"));
    }
    await Promise.allSettled([first, outer, ...(closing ? [closing] : [])]);
    await pool.close();
    throw error;
  }
});

test("E-017 a queued lease listener reserves capacity before nested admission and abort releases once", async () => {
  const fixture = makePool(1);
  const { pool } = fixture;
  const [worker] = fixture.workers;
  const [slot] = fixture.slots;
  const first = pool.evaluate("rules", {});
  const controller = new AbortController();
  const listenerBaseline = getEventListeners(controller.signal, "abort").length;
  const originalAdd = controller.signal.addEventListener.bind(controller.signal);
  let nestedReason!: Promise<unknown>;
  let reservedLease: any;
  let maximumQueued = 0;
  let callbackCalls = 0;

  controller.signal.addEventListener = typedAbortAdd((type, listener, options) => {
    if (type === "abort") {
      maximumQueued = Math.max(maximumQueued, pool.metrics().queued);
      reservedLease = slot.queue[0]?.lease;
      nestedReason = rejectionOf(pool.evaluate("rules", {}));
      maximumQueued = Math.max(maximumQueued, pool.metrics().queued);
    }
    const result = originalAdd(type, listener, options);
    if (type === "abort") controller.abort();
    return result;
  }) as typeof controller.signal.addEventListener;

  const outer = pool.do("rules", async () => {
    callbackCalls += 1;
  }, { signal: controller.signal });

  try {
    assertQueueFullError(await nestedReason, {
      capacity: 1,
      queued: 1,
      slotIndex: 0,
    });
    const outerReason = await rejectionOf(outer);
    assert.ok(outerReason instanceof DOMException);
    assert.strictEqual(outerReason.name, "AbortError");
    assert.strictEqual(outerReason.message, "The operation was aborted");
    assert.strictEqual(callbackCalls, 0);
    assert.strictEqual(maximumQueued, 1);
    assert.ok(reservedLease);
    assert.strictEqual(reservedLease.released, true);
    await reservedLease.releasedPromise;
    assert.strictEqual(slot.queue.length, 0);
    assert.strictEqual(slot.nextId, 1);
    assert.strictEqual(worker.messages.length, 1);
    assert.strictEqual(
      getEventListeners(controller.signal, "abort").length,
      listenerBaseline,
    );
    assert.deepStrictEqual(pool.metrics(), {
      queueCapacity: 1,
      queued: 0,
      inFlight: 1,
      rejected: 1,
      slots: [{ slotIndex: 0, queued: 0, inFlight: 1, rejected: 1 }],
    });

    respond(worker, worker.messages[0], evaluationResult("first"));
    assert.deepStrictEqual(await first, evaluationResult("first"));
    await pool.close();
  } catch (error) {
    await failAndClose(fixture, [first, outer]);
    throw error;
  }
});

test("E-017 queued lease reconciliation removes a listener delegated after synchronous abort", async () => {
  const fixture = makePool(1);
  const { pool } = fixture;
  const [worker] = fixture.workers;
  const [slot] = fixture.slots;
  const first = pool.evaluate("rules", {});
  const controller = new AbortController();
  const listenerBaseline = getEventListeners(controller.signal, "abort").length;
  const originalAdd = controller.signal.addEventListener.bind(controller.signal);
  const originalRemove = controller.signal.removeEventListener.bind(controller.signal);
  const removalFailure = new Error("lease removal failed before delegation");
  let reservedLease: any;
  let callbackCalls = 0;
  let removeCalls = 0;

  controller.signal.removeEventListener = typedAbortRemove((type, listener, options) => {
    if (type === "abort" && ++removeCalls === 1) throw removalFailure;
    return originalRemove(type, listener, options);
  }) as typeof controller.signal.removeEventListener;

  controller.signal.addEventListener = typedAbortAdd((type, listener, options) => {
    if (type === "abort") {
      assert.strictEqual(slot.queue.length, 1);
      reservedLease = slot.queue[0]?.lease;
      controller.abort();
    }
    return originalAdd(type, listener, options);
  }) as typeof controller.signal.addEventListener;

  const outer = pool.do("rules", async () => {
    callbackCalls += 1;
  }, { signal: controller.signal });

  try {
    const reason = await rejectionOf(outer);
    assert.ok(reason instanceof DOMException);
    assert.strictEqual(reason.name, "AbortError");
    assert.strictEqual(reason.message, "The operation was aborted");
    assert.strictEqual(callbackCalls, 0);
    assert.strictEqual(removeCalls, 2);
    assert.ok(reservedLease);
    assert.strictEqual(reservedLease.released, true);
    await reservedLease.releasedPromise;
    assert.strictEqual(slot.queue.length, 0);
    assert.strictEqual(
      getEventListeners(controller.signal, "abort").length,
      listenerBaseline,
    );
    assert.strictEqual(pool.metrics().rejected, 0);
    respond(worker, worker.messages[0], evaluationResult("first"));
    assert.deepStrictEqual(await first, evaluationResult("first"));
    await pool.close();
  } catch (error) {
    await failAndClose(fixture, [first, outer]);
    throw error;
  }
});

test("E-017 a queued lease listener hook may admit before throwing without replacing callback settlement", async () => {
  const fixture = makePool(1);
  const { pool } = fixture;
  const [worker] = fixture.workers;
  const [slot] = fixture.slots;
  const first = pool.evaluate("rules", {});
  const controller = new AbortController();
  const listenerBaseline = getEventListeners(controller.signal, "abort").length;
  const originalAdd = controller.signal.addEventListener.bind(controller.signal);
  const registrationFailure = new Error("lease dequeue-listener registration failed");
  const removalFailure = new Error("lease admission listener removal failed");
  const removalCalls = installThrowOnceAbortRemoval(
    controller.signal,
    removalFailure,
  );
  const callbackGate = deferred<void>();
  const callbackStarted = deferred<void>();
  let addCalls = 0;

  controller.signal.addEventListener = typedAbortAdd((type, listener, options) => {
    if (type === "abort") {
      addCalls += 1;
      if (addCalls === 1) {
        assert.strictEqual(slot.queue.length, 1);
        respond(worker, worker.messages[0], evaluationResult("first"));
        assert.ok(slot.activeLease);
        assert.strictEqual(slot.queue.length, 0);
        throw registrationFailure;
      }
    }
    return originalAdd(type, listener, options);
  }) as typeof controller.signal.addEventListener;

  const outer = pool.do("rules", async () => {
    callbackStarted.resolve();
    await callbackGate.promise;
    return "callback-result";
  }, { signal: controller.signal });

  try {
    assert.deepStrictEqual(await first, evaluationResult("first"));
    await callbackStarted.promise;
    assert.strictEqual(addCalls, 2, "callback abort listener was not installed");
    assert.strictEqual(slot.queue.length, 0);
    assert.ok(slot.activeLease?.active);
    assert.strictEqual(
      getEventListeners(controller.signal, "abort").length,
      listenerBaseline + 1,
    );
    callbackGate.resolve();
    assert.strictEqual(await outer, "callback-result");
    assert.strictEqual(
      getEventListeners(controller.signal, "abort").length,
      listenerBaseline,
    );
    assert.strictEqual(removalCalls(), 3);
    assert.strictEqual(pool.metrics().rejected, 0);
    await pool.close();
  } catch (error) {
    callbackGate.resolve();
    await Promise.allSettled([first, outer]);
    await pool.close();
    throw error;
  }
});

test("E-017 a queued lease listener throw rolls back and releases its reservation", async () => {
  const fixture = makePool(1);
  const { pool } = fixture;
  const [worker] = fixture.workers;
  const [slot] = fixture.slots;
  const first = pool.evaluate("rules", {});
  const controller = new AbortController();
  const listenerBaseline = getEventListeners(controller.signal, "abort").length;
  const registrationFailure = new Error("queued lease registration failed");
  let reservedLease: any;
  let callbackCalls = 0;
  let replacement: Promise<string> | undefined;

  controller.signal.addEventListener = typedAbortAdd((type) => {
    if (type === "abort") {
      assert.strictEqual(slot.queue.length, 1);
      reservedLease = slot.queue[0]?.lease;
      throw registrationFailure;
    }
  }) as typeof controller.signal.addEventListener;

  const outer = pool.do("rules", async () => {
    callbackCalls += 1;
  }, { signal: controller.signal });

  try {
    assert.strictEqual(await rejectionOf(outer), registrationFailure);
    assert.strictEqual(callbackCalls, 0);
    assert.ok(reservedLease);
    assert.strictEqual(reservedLease.released, true);
    await reservedLease.releasedPromise;
    assert.strictEqual(slot.queue.length, 0);
    assert.strictEqual(pool.metrics().rejected, 0);
    assert.strictEqual(
      getEventListeners(controller.signal, "abort").length,
      listenerBaseline,
    );

    replacement = pool.do("rules", async () => "replacement");
    assert.strictEqual(slot.queue.length, 1, "throw did not reclaim lease capacity");
    respond(worker, worker.messages[0], evaluationResult("first"));
    assert.deepStrictEqual(await first, evaluationResult("first"));
    assert.strictEqual(await replacement, "replacement");
    await pool.close();
  } catch (error) {
    await Promise.allSettled([
      first,
      outer,
      ...(replacement ? [replacement] : []),
    ]);
    if (slot.state.kind === "running") {
      worker.emit("error", new Error("lease throw cleanup"));
    }
    await pool.close();
    throw error;
  }
});

test("E-017 close during queued lease registration releases it and removes the stale listener", async () => {
  const fixture = makePool(1);
  const { pool } = fixture;
  const [worker] = fixture.workers;
  const [slot] = fixture.slots;
  const first = pool.evaluate("rules", {});
  const controller = new AbortController();
  const listenerBaseline = getEventListeners(controller.signal, "abort").length;
  const originalAdd = controller.signal.addEventListener.bind(controller.signal);
  const removalFailure = new Error("lease close listener removal failed");
  const removalCalls = installThrowOnceAbortRemoval(
    controller.signal,
    removalFailure,
  );
  let reservedLease: any;
  let closing!: Promise<void>;
  let callbackCalls = 0;

  controller.signal.addEventListener = typedAbortAdd((type, listener, options) => {
    if (type === "abort") {
      assert.strictEqual(slot.queue.length, 1);
      reservedLease = slot.queue[0]?.lease;
      closing = pool.close();
    }
    return originalAdd(type, listener, options);
  }) as typeof controller.signal.addEventListener;

  const outer = pool.do("rules", async () => {
    callbackCalls += 1;
  }, { signal: controller.signal });

  try {
    const outerReason = await rejectionOf(outer);
    assert.ok(outerReason instanceof Error);
    assert.strictEqual(outerReason.message, "EnginePool closed");
    assert.strictEqual(callbackCalls, 0);
    assert.ok(reservedLease);
    assert.strictEqual(reservedLease.released, true);
    await reservedLease.releasedPromise;
    assert.strictEqual(slot.queue.length, 0);
    assert.strictEqual(
      getEventListeners(controller.signal, "abort").length,
      listenerBaseline,
    );
    assert.strictEqual(removalCalls(), 2);
    assert.deepStrictEqual(pool.metrics(), {
      queueCapacity: 1,
      queued: 0,
      inFlight: 1,
      rejected: 0,
      slots: [{ slotIndex: 0, queued: 0, inFlight: 1, rejected: 0 }],
    });

    respond(worker, worker.messages[0], evaluationResult("first"));
    assert.deepStrictEqual(await first, evaluationResult("first"));
    await closing;
    assert.strictEqual(worker.terminateCalls, 1);
    assert.deepStrictEqual(pool.metrics(), {
      queueCapacity: 1,
      queued: 0,
      inFlight: 0,
      rejected: 0,
      slots: [{ slotIndex: 0, queued: 0, inFlight: 0, rejected: 0 }],
    });
  } catch (error) {
    if (slot.state.kind === "running") {
      worker.emit("error", new Error("lease close-hook cleanup"));
    }
    await Promise.allSettled([first, outer, ...(closing ? [closing] : [])]);
    await pool.close();
    throw error;
  }
});

test("E-017 terminal during queued lease registration releases once despite a removal throw", async () => {
  const fixture = makePool(1);
  const { pool } = fixture;
  const [worker] = fixture.workers;
  const [slot] = fixture.slots;
  const terminal = new Error("terminal during queued lease registration");
  const first = pool.evaluate("rules", {});
  const firstReason = rejectionOf(first);
  const controller = new AbortController();
  const listenerBaseline = getEventListeners(controller.signal, "abort").length;
  const originalAdd = controller.signal.addEventListener.bind(controller.signal);
  const removalFailure = new Error("lease terminal listener removal failed");
  const removalCalls = installThrowOnceAbortRemoval(
    controller.signal,
    removalFailure,
  );
  let reservedLease: any;
  let callbackCalls = 0;

  controller.signal.addEventListener = typedAbortAdd((type, listener, options) => {
    if (type === "abort") {
      assert.strictEqual(slot.queue.length, 1);
      reservedLease = slot.queue[0]?.lease;
      worker.emit("error", terminal);
    }
    return originalAdd(type, listener, options);
  }) as typeof controller.signal.addEventListener;

  const outer = pool.do("rules", async () => {
    callbackCalls += 1;
  }, { signal: controller.signal });

  try {
    assert.strictEqual(await firstReason, terminal);
    assert.strictEqual(await rejectionOf(outer), terminal);
    assert.strictEqual(callbackCalls, 0);
    assert.ok(reservedLease);
    assert.strictEqual(reservedLease.released, true);
    await reservedLease.releasedPromise;
    assert.strictEqual(slot.queue.length, 0);
    assert.strictEqual(slot.pending.size, 0);
    assert.strictEqual(slot.inflight, 0);
    assert.strictEqual(
      getEventListeners(controller.signal, "abort").length,
      listenerBaseline,
    );
    assert.strictEqual(removalCalls(), 2);
    assert.deepStrictEqual(pool.metrics(), {
      queueCapacity: 1,
      queued: 0,
      inFlight: 0,
      rejected: 0,
      slots: [{ slotIndex: 0, queued: 0, inFlight: 0, rejected: 0 }],
    });
    await pool.close();
  } catch (error) {
    await Promise.allSettled([first, outer]);
    await pool.close();
    throw error;
  }
});

test("E-017 queued lease removal hooks observe published admission before root close and terminal reentry", async (t) => {
  const cases = ["nested-root", "close", "terminal"] as const;

  for (const kind of cases) {
    await t.test(kind, async () => {
      const fixture = makePool(1);
      const { pool } = fixture;
      const [worker] = fixture.workers;
      const [slot] = fixture.slots;
      const first = pool.evaluate("rules", {});
      const controller = new AbortController();
      const listenerBaseline = getEventListeners(
        controller.signal,
        "abort",
      ).length;
      const originalAdd = controller.signal.addEventListener.bind(
        controller.signal,
      );
      const originalRemove = controller.signal.removeEventListener.bind(
        controller.signal,
      );
      const removalFailure = new Error(`lease ${kind} removal failed`);
      const terminal = new Error("terminal during published lease removal");
      const callbackStarted = deferred<void>();
      const callbackGate = deferred<void>();
      let addCalls = 0;
      let removeCalls = 0;
      let publishedLease: any;
      let nestedRoot: Promise<Ferric.EvaluateResult> | undefined;
      let nestedOverflow: Promise<unknown> | undefined;
      let closing: Promise<void> | undefined;

      controller.signal.removeEventListener = typedAbortRemove((type, listener, options) => {
        if (type === "abort" && ++removeCalls === 1) {
          publishedLease = slot.activeLease;
          assert.ok(publishedLease?.active, `${kind}: lease not published`);
          if (kind === "nested-root") {
            nestedRoot = pool.evaluate("rules", {});
            nestedOverflow = rejectionOf(pool.evaluate("rules", {}));
            assert.strictEqual(slot.queue.length, 1);
            assert.strictEqual(worker.messages.length, 1);
          } else if (kind === "close") {
            closing = pool.close();
          } else {
            worker.emit("error", terminal);
          }
        }
        const result = originalRemove(type, listener, options);
        if (type === "abort" && removeCalls === 1) throw removalFailure;
        return result;
      }) as typeof controller.signal.removeEventListener;

      controller.signal.addEventListener = typedAbortAdd((type, listener, options) => {
        if (type === "abort" && ++addCalls === 1) {
          assert.strictEqual(slot.queue.length, 1);
          respond(worker, worker.messages[0], evaluationResult("first"));
        }
        return originalAdd(type, listener, options);
      }) as typeof controller.signal.addEventListener;

      const outer = pool.do("rules", async (proxy) => {
        callbackStarted.resolve();
        if (kind === "terminal") {
          assert.strictEqual(await rejectionOf(proxy.facts()), terminal);
          return "terminal-callback";
        }
        await callbackGate.promise;
        return `${kind}-callback`;
      }, { signal: controller.signal });

      try {
        assert.deepStrictEqual(await first, evaluationResult("first"));
        await callbackStarted.promise;
        assert.strictEqual(slot.activeLease, publishedLease);
        assert.strictEqual(addCalls, 2);
        assert.strictEqual(
          getEventListeners(controller.signal, "abort").length,
          listenerBaseline + 1,
        );

        if (kind === "nested-root") {
          assert.ok(nestedRoot && nestedOverflow);
          assertQueueFullError(await nestedOverflow, {
            capacity: 1,
            queued: 1,
            slotIndex: 0,
          });
          assert.deepStrictEqual(pool.metrics(), {
            queueCapacity: 1,
            queued: 1,
            inFlight: 0,
            rejected: 1,
            slots: [{ slotIndex: 0, queued: 1, inFlight: 0, rejected: 1 }],
          });
          callbackGate.resolve();
          assert.strictEqual(await outer, "nested-root-callback");
          assert.strictEqual(worker.messages.length, 2);
          assert.strictEqual(worker.messages[1].method, "__evaluate");
          respond(worker, worker.messages[1], evaluationResult("nested"));
          assert.deepStrictEqual(await nestedRoot, evaluationResult("nested"));
        } else if (kind === "close") {
          assert.ok(closing);
          let closeSettled = false;
          void closing.then(() => { closeSettled = true; });
          await yieldImmediate();
          assert.strictEqual(closeSettled, false, "close skipped admitted callback");
          callbackGate.resolve();
          assert.strictEqual(await outer, "close-callback");
          await closing;
          assert.strictEqual(worker.terminateCalls, 1);
        } else {
          assert.deepStrictEqual(slot.state, { kind: "failed", error: terminal });
          assert.strictEqual(await outer, "terminal-callback");
        }

        await publishedLease.releasedPromise;
        assert.strictEqual(publishedLease.released, true);
        assert.strictEqual(publishedLease.pendingCalls, 0);
        assert.strictEqual(slot.activeLease, undefined);
        assert.strictEqual(slot.queue.length, 0);
        assert.strictEqual(slot.pending.size, 0);
        assert.strictEqual(slot.inflight, 0);
        assert.strictEqual(removeCalls, 3);
        assert.strictEqual(
          getEventListeners(controller.signal, "abort").length,
          listenerBaseline,
        );
        assert.strictEqual(pool.metrics().queued, 0);
        assert.strictEqual(pool.metrics().inFlight, 0);
        if (!closing) await pool.close();
      } catch (error) {
        callbackGate.resolve();
        if (slot.state.kind === "running") {
          worker.emit("error", new Error(`lease remove-hook cleanup: ${kind}`));
        }
        await Promise.allSettled([
          first,
          outer,
          ...(nestedRoot ? [nestedRoot] : []),
          ...(nestedOverflow ? [nestedOverflow] : []),
          ...(closing ? [closing] : []),
        ]);
        await pool.close();
        throw error;
      }
    });
  }
});

test("E-017 a full selected slot is not bypassed and overflow advances round robin", async () => {
  const fixture = makePool(0, 2);
  const { pool } = fixture;
  const [firstWorker, secondWorker] = fixture.workers;
  const [firstSlot, secondSlot] = fixture.slots;

  const first = pool.evaluate("rules", {});
  const second = pool.evaluate("rules", {});
  respond(secondWorker, secondWorker.messages[0], evaluationResult("second"));
  assert.deepStrictEqual(await second, evaluationResult("second"));

  const overflow = pool.evaluate("rules", {});
  let later: Promise<Ferric.EvaluateResult> | undefined;
  try {
    assertQueueFullError(await rejectionOf(overflow), {
      capacity: 0,
      queued: 0,
      slotIndex: 0,
    });
    assert.strictEqual(firstWorker.messages.length, 1);
    assert.strictEqual(secondWorker.messages.length, 1);
    assert.strictEqual(firstSlot.nextId, 1, "overflow consumed a slot-local ID");
    assert.strictEqual((pool as any).roundRobin, 1, "overflow rewound selection");
    assert.deepStrictEqual(pool.metrics(), {
      queueCapacity: 0,
      queued: 0,
      inFlight: 1,
      rejected: 1,
      slots: [
        { slotIndex: 0, queued: 0, inFlight: 1, rejected: 1 },
        { slotIndex: 1, queued: 0, inFlight: 0, rejected: 0 },
      ],
    });

    later = pool.evaluate("rules", {});
    assert.strictEqual(secondWorker.messages.length, 2);
    assert.strictEqual(secondSlot.nextId, 2);
    respond(secondWorker, secondWorker.messages[1], evaluationResult("later"));
    assert.deepStrictEqual(await later, evaluationResult("later"));
    respond(firstWorker, firstWorker.messages[0], evaluationResult("first"));
    assert.deepStrictEqual(await first, evaluationResult("first"));
    assert.deepStrictEqual(pool.metrics(), {
      queueCapacity: 0,
      queued: 0,
      inFlight: 0,
      rejected: 1,
      slots: [
        { slotIndex: 0, queued: 0, inFlight: 0, rejected: 1 },
        { slotIndex: 1, queued: 0, inFlight: 0, rejected: 0 },
      ],
    });
    await pool.close();
  } catch (error) {
    await failAndClose(
      fixture,
      [first, second, overflow, ...(later ? [later] : [])],
    );
    throw error;
  }
});

test("E-017 positive capacity is enforced per slot rather than pool-wide", async () => {
  const fixture = makePool(1, 2);
  const { pool } = fixture;
  const [firstWorker, secondWorker] = fixture.workers;
  const [firstSlot, secondSlot] = fixture.slots;
  const firstOnFirst = pool.evaluate("rules", {});
  const firstOnSecond = pool.evaluate("rules", {});
  const queuedOnFirst = pool.evaluate("rules", {});
  const queuedOnSecond = pool.evaluate("rules", {});
  const overflow = pool.evaluate("rules", {});

  try {
    assertQueueFullError(await rejectionOf(overflow), {
      capacity: 1,
      queued: 1,
      slotIndex: 0,
    });
    assert.strictEqual(firstSlot.queue.length, 1);
    assert.strictEqual(secondSlot.queue.length, 1);
    assert.strictEqual(firstSlot.nextId, 2);
    assert.strictEqual(secondSlot.nextId, 2);
    assert.strictEqual(firstWorker.messages.length, 1);
    assert.strictEqual(secondWorker.messages.length, 1);
    assert.deepStrictEqual(pool.metrics(), {
      queueCapacity: 1,
      queued: 2,
      inFlight: 2,
      rejected: 1,
      slots: [
        { slotIndex: 0, queued: 1, inFlight: 1, rejected: 1 },
        { slotIndex: 1, queued: 1, inFlight: 1, rejected: 0 },
      ],
    });

    respond(firstWorker, firstWorker.messages[0], evaluationResult("first-0"));
    respond(secondWorker, secondWorker.messages[0], evaluationResult("first-1"));
    assert.deepStrictEqual(await firstOnFirst, evaluationResult("first-0"));
    assert.deepStrictEqual(await firstOnSecond, evaluationResult("first-1"));
    assert.strictEqual(firstWorker.messages.length, 2);
    assert.strictEqual(secondWorker.messages.length, 2);
    assert.deepStrictEqual(pool.metrics(), {
      queueCapacity: 1,
      queued: 0,
      inFlight: 2,
      rejected: 1,
      slots: [
        { slotIndex: 0, queued: 0, inFlight: 1, rejected: 1 },
        { slotIndex: 1, queued: 0, inFlight: 1, rejected: 0 },
      ],
    });

    respond(firstWorker, firstWorker.messages[1], evaluationResult("queued-0"));
    respond(secondWorker, secondWorker.messages[1], evaluationResult("queued-1"));
    assert.deepStrictEqual(await queuedOnFirst, evaluationResult("queued-0"));
    assert.deepStrictEqual(await queuedOnSecond, evaluationResult("queued-1"));
    assert.deepStrictEqual(pool.metrics(), {
      queueCapacity: 1,
      queued: 0,
      inFlight: 0,
      rejected: 1,
      slots: [
        { slotIndex: 0, queued: 0, inFlight: 0, rejected: 1 },
        { slotIndex: 1, queued: 0, inFlight: 0, rejected: 0 },
      ],
    });
    await pool.close();
  } catch (error) {
    await failAndClose(
      fixture,
      [firstOnFirst, firstOnSecond, queuedOnFirst, queuedOnSecond, overflow],
    );
    throw error;
  }
});

test("E-017 consecutive queued send failures reclaim capacity and preserve FIFO", async () => {
  const fixture = makePool(3);
  const { pool } = fixture;
  const [worker] = fixture.workers;
  const [slot] = fixture.slots;
  const firstFailure = new DOMException("first queued send failed", "DataCloneError");
  const secondFailure = new DOMException("second queued send failed", "DataCloneError");

  const first = pool.evaluate("rules", {});
  const failedFirst = pool.evaluate("rules", {});
  const failedSecond = pool.evaluate("rules", {});
  const survivor = pool.evaluate("rules", {});
  const overflow = pool.evaluate("rules", {});
  let later: Promise<Ferric.EvaluateResult> | undefined;

  try {
    assertQueueFullError(await rejectionOf(overflow), {
      capacity: 3,
      queued: 3,
      slotIndex: 0,
    });
    worker.postActions.push(
      () => { throw firstFailure; },
      () => { throw secondFailure; },
    );
    respond(worker, worker.messages[0], evaluationResult("first"));
    assert.deepStrictEqual(await first, evaluationResult("first"));
    assert.strictEqual(await rejectionOf(failedFirst), firstFailure);
    assert.strictEqual(await rejectionOf(failedSecond), secondFailure);
    assert.strictEqual(worker.messages.length, 4);
    assert.deepStrictEqual(
      worker.messages.map((message) => message.id),
      [0, 1, 2, 3],
      "send rollback replayed or reordered a queued request",
    );
    assert.deepStrictEqual(pool.metrics(), {
      queueCapacity: 3,
      queued: 0,
      inFlight: 1,
      rejected: 1,
      slots: [{ slotIndex: 0, queued: 0, inFlight: 1, rejected: 1 }],
    });

    later = pool.evaluate("rules", {});
    assert.strictEqual(slot.queue.length, 1);
    assert.strictEqual(slot.nextId, 5, "overflow changed the monotonic ID history");
    respond(worker, worker.messages[3], evaluationResult("survivor"));
    assert.deepStrictEqual(await survivor, evaluationResult("survivor"));
    assert.strictEqual(worker.messages.length, 5);
    respond(worker, worker.messages[4], evaluationResult("later"));
    assert.deepStrictEqual(await later, evaluationResult("later"));
    assert.deepStrictEqual(pool.metrics(), {
      queueCapacity: 3,
      queued: 0,
      inFlight: 0,
      rejected: 1,
      slots: [{ slotIndex: 0, queued: 0, inFlight: 0, rejected: 1 }],
    });
    await pool.close();
  } catch (error) {
    await failAndClose(
      fixture,
      [
        first,
        failedFirst,
        failedSecond,
        survivor,
        overflow,
        ...(later ? [later] : []),
      ],
    );
    throw error;
  }
});

test("E-017 terminal failure clears root and owner queues without changing overflow history", async () => {
  const fixture = makePool(3);
  const { pool } = fixture;
  const [worker] = fixture.workers;
  const [slot] = fixture.slots;
  const callbackStarted = deferred<void>();
  const callbackGate = deferred<void>();
  let ownerFirst!: Promise<readonly Ferric.Fact[]>;
  let ownerSecond!: Promise<readonly Ferric.Fact[]>;
  let waitingCallbackCalls = 0;

  const active = pool.do("rules", async (proxy) => {
    ownerFirst = proxy.facts();
    ownerSecond = proxy.facts();
    callbackStarted.resolve();
    await callbackGate.promise;
    return "callback-survived-terminal";
  });
  await callbackStarted.promise;
  const root = pool.evaluate("rules", {});
  const waitingLease = pool.do("rules", async () => {
    waitingCallbackCalls += 1;
  });
  const overflow = pool.evaluate("rules", {});

  try {
    assertQueueFullError(await rejectionOf(overflow), {
      capacity: 3,
      queued: 3,
      slotIndex: 0,
    });
    assert.deepStrictEqual(pool.metrics(), {
      queueCapacity: 3,
      queued: 3,
      inFlight: 1,
      rejected: 1,
      slots: [{ slotIndex: 0, queued: 3, inFlight: 1, rejected: 1 }],
    });
    const ownerFirstReason = rejectionOf(ownerFirst);
    const ownerSecondReason = rejectionOf(ownerSecond);
    const rootReason = rejectionOf(root);
    const waitingReason = rejectionOf(waitingLease);
    const terminal = new Error("backpressure terminal failure");
    const nextId = slot.nextId;

    worker.emit("error", terminal);
    assert.strictEqual(await ownerFirstReason, terminal);
    assert.strictEqual(await ownerSecondReason, terminal);
    assert.strictEqual(await rootReason, terminal);
    assert.strictEqual(await waitingReason, terminal);
    assert.strictEqual(waitingCallbackCalls, 0);
    assert.deepStrictEqual(slot.state, { kind: "failed", error: terminal });
    assert.strictEqual(slot.pending.size, 0);
    assert.strictEqual(slot.inflight, 0);
    assert.strictEqual(slot.queue.length, 0);
    assert.strictEqual(slot.activeLease.queue.length, 0);
    assert.deepStrictEqual(pool.metrics(), {
      queueCapacity: 3,
      queued: 0,
      inFlight: 0,
      rejected: 1,
      slots: [{ slotIndex: 0, queued: 0, inFlight: 0, rejected: 1 }],
    });

    const future = pool.evaluate("rules", {});
    assert.strictEqual(await rejectionOf(future), terminal);
    assert.strictEqual(slot.nextId, nextId);
    assert.strictEqual(pool.metrics().rejected, 1);

    callbackGate.resolve();
    assert.strictEqual(await active, "callback-survived-terminal");
    await pool.close();
    assert.deepStrictEqual(pool.metrics(), {
      queueCapacity: 3,
      queued: 0,
      inFlight: 0,
      rejected: 1,
      slots: [{ slotIndex: 0, queued: 0, inFlight: 0, rejected: 1 }],
    });
  } catch (error) {
    callbackGate.resolve();
    await Promise.allSettled([active, ownerFirst, ownerSecond, root, waitingLease, overflow]);
    await pool.close();
    throw error;
  }
});

test("E-017 close clears root work, drains accepted owner work, and keeps metrics detached", async () => {
  const fixture = makePool(2);
  const { pool } = fixture;
  const [worker] = fixture.workers;
  const [slot] = fixture.slots;
  const callbackStarted = deferred<void>();
  const observeMetrics = deferred<void>();
  const metricsObserved = deferred<void>();
  const callbackGate = deferred<void>();
  let ownerFirst!: Promise<readonly Ferric.Fact[]>;
  let ownerSecond!: Promise<readonly Ferric.Fact[]>;
  let callbackMetrics!: Ferric.EnginePoolMetrics;

  const active = pool.do("rules", async (proxy) => {
    ownerFirst = proxy.facts();
    ownerSecond = proxy.facts();
    callbackStarted.resolve();
    await observeMetrics.promise;
    callbackMetrics = pool.metrics();
    metricsObserved.resolve();
    await callbackGate.promise;
    return "closed-owner-finished";
  });
  await callbackStarted.promise;
  const root = pool.evaluate("rules", {});
  const overflow = pool.evaluate("rules", {});
  let closing: Promise<void> | undefined;

  try {
    assertQueueFullError(await rejectionOf(overflow), {
      capacity: 2,
      queued: 2,
      slotIndex: 0,
    });
    closing = pool.close();
    const rootReason = await rejectionOf(root);
    assert.ok(rootReason instanceof Error);
    assert.strictEqual(rootReason.message, "EnginePool closed");
    assert.strictEqual(slot.queue.length, 0, "close retained root work");
    assert.strictEqual(slot.activeLease.queue.length, 1, "close discarded accepted owner work");

    observeMetrics.resolve();
    await metricsObserved.promise;
    assert.deepStrictEqual(callbackMetrics, {
      queueCapacity: 2,
      queued: 1,
      inFlight: 1,
      rejected: 1,
      slots: [{ slotIndex: 0, queued: 1, inFlight: 1, rejected: 1 }],
    });
    const mutableSnapshot = callbackMetrics as any;
    mutableSnapshot.queued = 999;
    mutableSnapshot.slots[0].queued = 999;
    mutableSnapshot.slots.push({
      slotIndex: 999,
      queued: 999,
      inFlight: 999,
      rejected: 999,
    });
    assert.deepStrictEqual(pool.metrics(), {
      queueCapacity: 2,
      queued: 1,
      inFlight: 1,
      rejected: 1,
      slots: [{ slotIndex: 0, queued: 1, inFlight: 1, rejected: 1 }],
    });

    respond(worker, worker.messages[0], []);
    assert.deepStrictEqual(await ownerFirst, []);
    assert.strictEqual(worker.messages.length, 2);
    respond(worker, worker.messages[1], []);
    assert.deepStrictEqual(await ownerSecond, []);
    callbackGate.resolve();
    assert.strictEqual(await active, "closed-owner-finished");
    await closing;
    assert.strictEqual(worker.terminateCalls, 1);
    assert.deepStrictEqual(pool.metrics(), {
      queueCapacity: 2,
      queued: 0,
      inFlight: 0,
      rejected: 1,
      slots: [{ slotIndex: 0, queued: 0, inFlight: 0, rejected: 1 }],
    });
  } catch (error) {
    observeMetrics.resolve();
    callbackGate.resolve();
    const outcomes = Promise.allSettled([
      active,
      ownerFirst,
      ownerSecond,
      root,
      overflow,
      ...(closing ? [closing] : []),
    ]);
    if (slot.state.kind === "running") {
      worker.emit("error", new Error("close backpressure-test cleanup"));
    }
    await outcomes;
    await pool.close();
    throw error;
  }
});

test("E-017 callback abort does not reclaim already-accepted owner queue capacity", async () => {
  const fixture = makePool(1);
  const { pool } = fixture;
  const [worker] = fixture.workers;
  const [slot] = fixture.slots;
  const controller = new AbortController();
  const callbackStarted = deferred<void>();
  const callbackGate = deferred<void>();
  let retained!: Ferric.EngineProxy;
  let ownerFirst!: Promise<readonly Ferric.Fact[]>;
  let ownerSecond!: Promise<readonly Ferric.Fact[]>;

  const active = pool.do("rules", async (proxy) => {
    retained = proxy;
    ownerFirst = proxy.facts();
    ownerSecond = proxy.facts();
    callbackStarted.resolve();
    await callbackGate.promise;
  }, { signal: controller.signal });
  const activeReason = rejectionOf(active);
  await callbackStarted.promise;
  const overflow = retained.facts();

  try {
    assertQueueFullError(await rejectionOf(overflow), {
      capacity: 1,
      queued: 1,
      slotIndex: 0,
    });
    const lease = slot.activeLease;
    assert.ok(lease);
    controller.abort();
    const abortReason = await activeReason;
    assert.ok(abortReason instanceof DOMException);
    assert.strictEqual(abortReason.name, "AbortError");
    assert.strictEqual(abortReason.message, "The operation was aborted");
    assert.strictEqual(lease.queue.length, 1);
    assert.strictEqual(lease.pendingCalls, 2);
    assert.deepStrictEqual(pool.metrics(), {
      queueCapacity: 1,
      queued: 1,
      inFlight: 1,
      rejected: 1,
      slots: [{ slotIndex: 0, queued: 1, inFlight: 1, rejected: 1 }],
    });

    const rootOverflow = pool.evaluate("rules", {});
    assertQueueFullError(await rejectionOf(rootOverflow), {
      capacity: 1,
      queued: 1,
      slotIndex: 0,
    });
    assert.strictEqual(lease.queue.length, 1);

    respond(worker, worker.messages[0], []);
    assert.deepStrictEqual(await ownerFirst, []);
    assert.strictEqual(worker.messages.length, 2);
    respond(worker, worker.messages[1], []);
    assert.deepStrictEqual(await ownerSecond, []);
    callbackGate.resolve();
    await lease.releasedPromise;
    assert.deepStrictEqual(pool.metrics(), {
      queueCapacity: 1,
      queued: 0,
      inFlight: 0,
      rejected: 2,
      slots: [{ slotIndex: 0, queued: 0, inFlight: 0, rejected: 2 }],
    });
    await pool.close();
  } catch (error) {
    callbackGate.resolve();
    await failAndClose(fixture, [active, ownerFirst, ownerSecond, overflow]);
    throw error;
  }
});

test("E-017 seeded mixed root owner abort send fault and close schedules stay bounded", async () => {
  const initialSeed = 0x141c0de;
  let randomState = initialSeed;
  const random = () => {
    randomState = (
      Math.imul(randomState, 1_664_525) + 1_013_904_223
    ) >>> 0;
    return randomState;
  };

  for (let iteration = 0; iteration < 16; iteration += 1) {
    const fixture = makePool(3);
    const { pool } = fixture;
    const [worker] = fixture.workers;
    const [slot] = fixture.slots;
    const callbackStarted = deferred<void>();
    const callbackGate = deferred<void>();
    const abortController = new AbortController();
    const listenerBaseline = getEventListeners(
      abortController.signal,
      "abort",
    ).length;
    const sendFailure = new Error(`seeded send failure ${iteration}`);
    const terminal = new Error(`seeded terminal failure ${iteration}`);
    let proxy!: Ferric.EngineProxy;
    let ownerFirst!: Promise<readonly Ferric.Fact[]>;
    let ownerSend: Promise<readonly Ferric.Fact[]> | undefined;
    let rootAbort: Promise<Ferric.EvaluateResult> | undefined;
    let rootSurvivor: Promise<Ferric.EvaluateResult> | undefined;
    let rootFault: Promise<Ferric.EvaluateResult> | undefined;
    let overflow: Promise<Ferric.EvaluateResult> | undefined;

    const active = pool.do("rules", async (engine) => {
      proxy = engine;
      ownerFirst = engine.facts();
      callbackStarted.resolve();
      await callbackGate.promise;
      return `callback-${iteration}`;
    });
    await callbackStarted.promise;

    const admissionOrder: Array<"owner" | "abort" | "root"> = [
      "owner",
      "abort",
      "root",
    ];
    for (let index = admissionOrder.length - 1; index > 0; index -= 1) {
      const swapIndex = random() % (index + 1);
      [admissionOrder[index], admissionOrder[swapIndex]] = [
        admissionOrder[swapIndex],
        admissionOrder[index],
      ];
    }
    const trace =
      `seed=0x${initialSeed.toString(16)} iteration=${iteration} ` +
      `order=${admissionOrder.join(",")}`;

    try {
      for (const operation of admissionOrder) {
        if (operation === "owner") ownerSend = proxy.facts();
        if (operation === "abort") {
          rootAbort = pool.evaluate(
            "rules",
            {},
            { signal: abortController.signal },
          );
        }
        if (operation === "root") rootSurvivor = pool.evaluate("rules", {});
        assertBoundedMetrics(pool, 3, `${trace} admitted ${operation}`);
      }
      assert.ok(ownerSend && rootAbort && rootSurvivor, trace);
      assert.strictEqual(
        assertBoundedMetrics(pool, 3, `${trace} saturated`).queued,
        3,
      );

      overflow = pool.evaluate("rules", {});
      assertQueueFullError(await rejectionOf(overflow), {
        capacity: 3,
        queued: 3,
        slotIndex: 0,
      });
      assert.strictEqual(
        assertBoundedMetrics(pool, 3, `${trace} overflow`).rejected,
        1,
      );

      const rootAbortReason = rejectionOf(rootAbort);
      abortController.abort();
      const aborted = await rootAbortReason;
      assert.ok(aborted instanceof DOMException, trace);
      assert.strictEqual(aborted.name, "AbortError", trace);
      assert.strictEqual(
        getEventListeners(abortController.signal, "abort").length,
        listenerBaseline,
        trace,
      );
      assert.strictEqual(
        assertBoundedMetrics(pool, 3, `${trace} root abort`).queued,
        2,
      );

      const ownerSendReason = rejectionOf(ownerSend);
      worker.nextPost = () => { throw sendFailure; };
      respond(worker, worker.messages[0], []);
      assert.deepStrictEqual(await ownerFirst, [], trace);
      assert.strictEqual(await ownerSendReason, sendFailure, trace);
      assert.strictEqual(
        assertBoundedMetrics(pool, 3, `${trace} owner send rollback`).queued,
        1,
      );

      callbackGate.resolve();
      assert.strictEqual(await active, `callback-${iteration}`, trace);
      assert.strictEqual(worker.messages.length, 3, trace);
      assert.strictEqual(worker.messages[2].method, "__evaluate", trace);
      assert.deepStrictEqual(
        assertBoundedMetrics(pool, 3, `${trace} root dispatch`),
        {
          queueCapacity: 3,
          queued: 0,
          inFlight: 1,
          rejected: 1,
          slots: [{ slotIndex: 0, queued: 0, inFlight: 1, rejected: 1 }],
        },
      );

      rootFault = pool.evaluate("rules", {});
      const survivorReason = rejectionOf(rootSurvivor);
      const faultReason = rejectionOf(rootFault);
      assert.strictEqual(
        assertBoundedMetrics(pool, 3, `${trace} before terminal`).queued,
        1,
      );
      worker.emit("error", terminal);
      assert.strictEqual(await survivorReason, terminal, trace);
      assert.strictEqual(await faultReason, terminal, trace);
      assert.deepStrictEqual(
        assertBoundedMetrics(pool, 3, `${trace} terminal`),
        {
          queueCapacity: 3,
          queued: 0,
          inFlight: 0,
          rejected: 1,
          slots: [{ slotIndex: 0, queued: 0, inFlight: 0, rejected: 1 }],
        },
      );

      await pool.close();
      assert.strictEqual(worker.terminateCalls, 1, trace);
      assertBoundedMetrics(pool, 3, `${trace} close`);
    } catch (error) {
      callbackGate.resolve();
      if (slot.state.kind === "running") {
        worker.emit("error", new Error(`seeded cleanup: ${trace}`));
      }
      await Promise.allSettled([
        active,
        ownerFirst,
        ...(ownerSend ? [ownerSend] : []),
        ...(rootAbort ? [rootAbort] : []),
        ...(rootSurvivor ? [rootSurvivor] : []),
        ...(rootFault ? [rootFault] : []),
        ...(overflow ? [overflow] : []),
      ]);
      await pool.close();
      throw error;
    }
  }
});

function runMemoryLimitedNodeScript(script: string): Promise<{
  stdout: string;
  stderr: string;
}> {
  const packageRoot = resolve(__dirname, "../../../..");
  const childEnv = { ...process.env };
  childEnv.NODE_V8_COVERAGE = "";
  delete childEnv.NODE_TEST_CONTEXT;

  return new Promise((resolveRun, rejectRun) => {
    execFile(
      process.execPath,
      ["--max-old-space-size=64", "-e", script],
      {
        cwd: packageRoot,
        env: childEnv,
        timeout: 15_000,
        killSignal: "SIGKILL",
        windowsHide: true,
      },
      (error, stdout, stderr) => {
        if (error) {
          rejectRun(
            new Error(
              `Memory-limited Node subprocess failed: ${error.message}\n` +
                `stdout:\n${stdout}\nstderr:\n${stderr}`,
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

test("E-017 default capacity bounds a memory-limited overload burst and exits naturally", async () => {
  const burstSize = 50_000;
  const script = `
const { EventEmitter } = require("node:events");
const workerThreads = require("node:worker_threads");

class StalledWorker extends EventEmitter {
  static instance;
  ordinaryMessages = [];
  terminateCalls = 0;

  constructor() {
    super();
    StalledWorker.instance = this;
  }

  postMessage(message) {
    if (message.method === "__init") {
      queueMicrotask(() => this.emit("message", { id: message.id, result: undefined }));
      return;
    }
    this.ordinaryMessages.push(message);
  }

  terminate() {
    this.terminateCalls += 1;
    return Promise.resolve(0);
  }
}

workerThreads.Worker = StalledWorker;
const { EnginePool, EnginePoolQueueFullError } = require("./dist");
const activePorts = () => process.getActiveResourcesInfo()
  .filter((resource) => resource === "MessagePort").length;

(async () => {
  const before = activePorts();
  const pool = await EnginePool.create([{ name: "rules" }], { threads: 1 });
  const worker = StalledWorker.instance;
  const accepted = [];
  for (let index = 0; index < 1025; index += 1) {
    accepted.push(pool.evaluate("rules", {}).then(
      () => { throw new Error("stalled accepted request unexpectedly resolved"); },
      (error) => error,
    ));
  }

  const saturated = pool.metrics();
  let exactOverflowErrors = 0;
  for (let index = 0; index < ${burstSize}; index += 1) {
    const error = await pool.evaluate("rules", {}).then(
      () => { throw new Error("overflow unexpectedly resolved"); },
      (reason) => reason,
    );
    if (
      error instanceof EnginePoolQueueFullError &&
      error.capacity === 1024 &&
      error.queued === 1024 &&
      error.slotIndex === 0
    ) {
      exactOverflowErrors += 1;
    }
  }

  const terminal = new Error("bounded burst cleanup");
  worker.emit("error", terminal);
  const acceptedReasons = await Promise.all(accepted);
  await pool.close();
  await new Promise((resolveValue) => setImmediate(resolveValue));
  await new Promise((resolveValue) => setImmediate(resolveValue));
  const finalMetrics = pool.metrics();
  console.log(JSON.stringify({
    before,
    after: activePorts(),
    saturated,
    exactOverflowErrors,
    posted: worker.ordinaryMessages.length,
    allAcceptedUsedTerminal: acceptedReasons.every((reason) => reason === terminal),
    finalMetrics,
    terminateCalls: worker.terminateCalls,
    heapUsed: process.memoryUsage().heapUsed,
  }));
})().catch((error) => {
  console.error(error);
  process.exitCode = 1;
});
`;

  const { stdout } = await runMemoryLimitedNodeScript(script);
  assert.notStrictEqual(stdout.trim(), "", "bounded subprocess exited without a report");
  const report = JSON.parse(stdout.trim()) as {
    before: number;
    after: number;
    saturated: Ferric.EnginePoolMetrics;
    exactOverflowErrors: number;
    posted: number;
    allAcceptedUsedTerminal: boolean;
    finalMetrics: Ferric.EnginePoolMetrics;
    terminateCalls: number;
    heapUsed: number;
  };
  assert.deepStrictEqual(report.saturated, {
    queueCapacity: 1024,
    queued: 1024,
    inFlight: 1,
    rejected: 0,
    slots: [{ slotIndex: 0, queued: 1024, inFlight: 1, rejected: 0 }],
  });
  assert.strictEqual(report.exactOverflowErrors, burstSize);
  assert.strictEqual(report.posted, 1);
  assert.strictEqual(report.allAcceptedUsedTerminal, true);
  assert.deepStrictEqual(report.finalMetrics, {
    queueCapacity: 1024,
    queued: 0,
    inFlight: 0,
    rejected: burstSize,
    slots: [{ slotIndex: 0, queued: 0, inFlight: 0, rejected: burstSize }],
  });
  assert.strictEqual(report.terminateCalls, 1);
  assert.strictEqual(report.after, report.before);
  assert.ok(report.heapUsed > 0);
});
