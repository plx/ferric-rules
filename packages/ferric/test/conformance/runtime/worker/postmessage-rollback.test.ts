/**
 * EngineHandle synchronous postMessage rollback tests (FR-NODE-008 / D-011).
 *
 * A request is registered before it is sent so a very fast Worker response
 * cannot beat the pending map.  The registration must be rolled back again
 * when postMessage itself throws synchronously.
 */
import { getEventListeners, EventEmitter } from "node:events";
import { test } from "node:test";
import * as assert from "node:assert/strict";

import { EngineHandle } from "../../../helpers/ferric";

interface PostedMessage {
  id: number;
  method: string;
  args: unknown[];
}

class PlannedWorker extends EventEmitter {
  readonly messages: PostedMessage[] = [];
  terminateCalls = 0;
  nextPost: ((message: PostedMessage) => void) | undefined;

  postMessage(message: PostedMessage): void {
    this.messages.push(message);
    const action = this.nextPost;
    this.nextPost = undefined;
    action?.(message);
  }

  terminate(): Promise<number> {
    this.terminateCalls += 1;
    return Promise.resolve(0);
  }
}

function makeHandle(): { handle: EngineHandle; worker: PlannedWorker } {
  const worker = new PlannedWorker();
  const handle = new (EngineHandle as any)(worker) as EngineHandle;
  return { handle, worker };
}

async function rejectionOf<T>(promise: Promise<T>): Promise<unknown> {
  try {
    await promise;
  } catch (error) {
    return error;
  }
  throw new Error("Expected Promise to reject");
}

function assertHandleIdle(handle: EngineHandle): void {
  assert.strictEqual((handle as any).pending.size, 0);
}

function assertWorkerListenersIntact(worker: PlannedWorker): void {
  assert.strictEqual(worker.listenerCount("message"), 1);
  assert.strictEqual(worker.listenerCount("error"), 1);
  assert.strictEqual(worker.listenerCount("exit"), 1);
}

async function completeFacts(
  handle: EngineHandle,
  worker: PlannedWorker,
): Promise<void> {
  const pending = handle.facts();
  const request = worker.messages.at(-1);
  assert.ok(request);
  assert.strictEqual(request.method, "facts");
  worker.emit("message", { id: request.id, result: [] });
  assert.deepStrictEqual(await pending, []);
  assertHandleIdle(handle);
}

test("D-011 EngineHandle ordinary calls roll back synchronous send failures and recover", async (t) => {
  const cases: Array<{ name: string; failure: unknown }> = [
    {
      name: "DataCloneError",
      failure: new DOMException("value could not be cloned", "DataCloneError"),
    },
    {
      name: "non-Error thrown value",
      failure: Object.freeze({ kind: "synthetic postMessage failure" }),
    },
  ];

  for (const item of cases) {
    await t.test(`D-011 preserves ${item.name} by identity`, async () => {
      const { handle, worker } = makeHandle();
      const listenerBaseline = {
        message: worker.listenerCount("message"),
        error: worker.listenerCount("error"),
        exit: worker.listenerCount("exit"),
      };

      worker.nextPost = () => {
        throw item.failure;
      };

      const failure = await rejectionOf(handle.facts());
      assert.strictEqual(failure, item.failure);
      assertHandleIdle(handle);
      assert.deepStrictEqual(
        {
          message: worker.listenerCount("message"),
          error: worker.listenerCount("error"),
          exit: worker.listenerCount("exit"),
        },
        listenerBaseline,
      );
      assertWorkerListenersIntact(worker);

      await completeFacts(handle, worker);
      await handle.close();
      assert.strictEqual(worker.terminateCalls, 1);
    });
  }
});

test("D-011 EngineHandle.run rolls back its pending entry and AbortSignal listener", async () => {
  const { handle, worker } = makeHandle();
  const controller = new AbortController();
  const abortListenerBaseline = getEventListeners(
    controller.signal,
    "abort",
  ).length;
  const failure = new DOMException(
    "batched run request could not be cloned",
    "DataCloneError",
  );

  worker.nextPost = (request) => {
    assert.strictEqual(request.method, "__run_batched");
    assert.strictEqual(
      getEventListeners(controller.signal, "abort").length,
      abortListenerBaseline + 1,
      "run must install cancellation before attempting the send",
    );
    throw failure;
  };

  const reason = await rejectionOf(handle.run({ signal: controller.signal }));
  assert.strictEqual(reason, failure);
  assertHandleIdle(handle);
  assert.strictEqual(
    getEventListeners(controller.signal, "abort").length,
    abortListenerBaseline,
  );
  assertWorkerListenersIntact(worker);

  await completeFacts(handle, worker);
  await handle.close();
  assert.strictEqual(worker.terminateCalls, 1);
});

test("D-011 a real DataCloneError is request-local and consumes its request ID", { timeout: 5_000 }, async () => {
  const handle = await EngineHandle.create();
  const worker = (handle as any).worker;
  const nextIdBefore = (handle as any).nextId;
  const listenerBaseline = {
    message: worker.listenerCount("message"),
    error: worker.listenerCount("error"),
    exit: worker.listenerCount("exit"),
  };

  let returned: Promise<unknown> | undefined;
  assert.doesNotThrow(() => {
    returned = handle.assertFact(
      "uncloneable",
      (() => undefined) as any,
    );
  });
  assert.ok(returned instanceof Promise);

  const reason = await rejectionOf(returned);
  assert.ok(reason instanceof DOMException);
  assert.strictEqual(reason.name, "DataCloneError");
  assertHandleIdle(handle);
  assert.strictEqual((handle as any).nextId, nextIdBefore + 1);
  assert.strictEqual((handle as any).worker, worker);
  assert.deepStrictEqual(
    {
      message: worker.listenerCount("message"),
      error: worker.listenerCount("error"),
      exit: worker.listenerCount("exit"),
    },
    listenerBaseline,
  );

  const validId = await handle.assertString("(clone-recovery)");
  assert.strictEqual(validId.length, 1);
  assert.strictEqual((handle as any).nextId, nextIdBefore + 2);
  await handle.close();
});

test("D-011 synchronous response, error, and exit settlements beat a later send throw", async (t) => {
  const sendFailure = new DOMException(
    "postMessage threw after settling",
    "DataCloneError",
  );
  const workerFailure = new Error("synchronous Worker error");
  const cases = [
    {
      name: "response",
      settle: (worker: PlannedWorker, request: PostedMessage) => {
        worker.emit("message", { id: request.id, result: ["first"] });
      },
      verify: async (returned: Promise<unknown>) => {
        assert.deepStrictEqual(await returned, ["first"]);
      },
      keepsWorker: true,
    },
    {
      name: "error",
      settle: (worker: PlannedWorker) => {
        worker.emit("error", workerFailure);
      },
      verify: async (returned: Promise<unknown>) => {
        assert.strictEqual(await rejectionOf(returned), workerFailure);
      },
      keepsWorker: true,
    },
    {
      name: "exit",
      settle: (worker: PlannedWorker) => {
        worker.emit("exit", 7);
      },
      verify: async (returned: Promise<unknown>) => {
        const reason = await rejectionOf(returned);
        assert.ok(reason instanceof Error);
        assert.match(reason.message, /unexpectedly with code 7/);
        assert.notStrictEqual(reason, sendFailure);
      },
      keepsWorker: false,
    },
  ];

  for (const item of cases) {
    await t.test(`D-011 ${item.name} wins first settlement`, async () => {
      const { handle, worker } = makeHandle();
      worker.nextPost = (request) => {
        item.settle(worker, request);
        throw sendFailure;
      };

      let returned: Promise<unknown> | undefined;
      assert.doesNotThrow(() => {
        returned = handle.facts();
      });
      assert.ok(returned instanceof Promise);
      await item.verify(returned);
      assertHandleIdle(handle);

      if (item.keepsWorker) {
        assertWorkerListenersIntact(worker);
        await completeFacts(handle, worker);
      } else {
        assert.strictEqual((handle as any).worker, null);
      }
      await handle.close();
    });
  }
});

test("D-011 EngineHandle.run keeps a synchronous response that precedes a send throw", async () => {
  const { handle, worker } = makeHandle();
  const controller = new AbortController();
  const abortListenerBaseline = getEventListeners(
    controller.signal,
    "abort",
  ).length;
  const runResult = {
    fired: 0,
    haltReason: "AgendaEmpty",
    firedRules: [],
    errors: [],
  };
  const sendFailure = new DOMException(
    "run send threw after response",
    "DataCloneError",
  );

  worker.nextPost = (request) => {
    assert.strictEqual(request.method, "__run_batched");
    worker.emit("message", { id: request.id, result: runResult });
    throw sendFailure;
  };

  assert.deepStrictEqual(
    await handle.run({ signal: controller.signal }),
    runResult,
  );
  assertHandleIdle(handle);
  assert.strictEqual(
    getEventListeners(controller.signal, "abort").length,
    abortListenerBaseline,
  );
  assertWorkerListenersIntact(worker);

  await completeFacts(handle, worker);
  await handle.close();
});

test("D-011 send rollback remains first settlement when a Worker error arrives later", async () => {
  const { handle, worker } = makeHandle();
  const sendFailure = new DOMException(
    "send failed first",
    "DataCloneError",
  );
  worker.nextPost = () => {
    throw sendFailure;
  };

  const returned = handle.facts();
  assert.strictEqual(await rejectionOf(returned), sendFailure);
  assertHandleIdle(handle);

  worker.emit("error", new Error("later Worker error"));
  assert.strictEqual(await rejectionOf(returned), sendFailure);
  assertHandleIdle(handle);
  assertWorkerListenersIntact(worker);

  await completeFacts(handle, worker);
  await handle.close();
});
