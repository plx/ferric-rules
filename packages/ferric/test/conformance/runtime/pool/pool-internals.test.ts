/**
 * EnginePool defensive-branch tests with fake Worker slots.
 */
import { EventEmitter } from "node:events";
import { test } from "node:test";
import * as assert from "node:assert/strict";
import { setImmediate as yieldImmediate } from "node:timers/promises";

import {
  Encoding,
  EnginePool,
  FerricError,
  FerricParseError,
  Strategy,
} from "../../../helpers/ferric";

class FakeWorker extends EventEmitter {
  readonly messages: any[] = [];
  terminateCalls = 0;

  postMessage(message: any): void {
    this.messages.push(message);
  }

  terminate(): Promise<number> {
    this.terminateCalls += 1;
    return Promise.resolve(0);
  }
}

function makePool(): { pool: EnginePool; worker: FakeWorker; slot: any } {
  const worker = new FakeWorker();
  const slot = (EnginePool as any).createSlot(worker);
  const pool = new (EnginePool as any)([slot]) as EnginePool;
  return { pool, worker, slot };
}

async function waitForPostedMessage(worker: FakeWorker): Promise<any> {
  for (let turn = 0; turn < 20; turn++) {
    if (worker.messages.length > 0) return worker.messages[0];
    await yieldImmediate();
  }
  assert.fail("EnginePool did not post the expected fake-worker request");
}

// ---------------------------------------------------------------------------
// C-004 table-driven reconstruction for special pool-worker errors
// ---------------------------------------------------------------------------
test("C-004 table-driven EnginePool reconstructs special worker errors", async () => {
  const cases = [
    {
      payload: { name: "FerricParseError", message: "bad syntax", code: "FERRIC_PARSE_ERROR" },
      verify: (err: any) => assert.ok(err instanceof FerricParseError),
    },
    {
      payload: { name: "AbortError", message: "aborted", code: "ABORT_ERR" },
      verify: (err: any) => {
        assert.ok(err instanceof DOMException);
        assert.strictEqual(err.name, "AbortError");
      },
    },
    {
      payload: { name: "TypeError", message: "bad type", code: "ERR_TYPE" },
      verify: (err: any) => assert.ok(err instanceof TypeError),
    },
    {
      payload: { name: "UnknownPoolError", message: "custom", code: "CUSTOM" },
      verify: (err: any) => {
        assert.ok(err instanceof FerricError);
        assert.strictEqual(err.name, "UnknownPoolError");
        assert.strictEqual(err.code, "CUSTOM");
      },
    },
  ];

  for (const item of cases) {
    const { pool, worker } = makePool();
    const pending = pool.evaluate("rules", {});
    worker.emit("message", {
      id: worker.messages[0].id,
      error: item.payload,
    });

    await assert.rejects(pending, (err: any) => {
      item.verify(err);
      assert.strictEqual(err.message, item.payload.message);
      return true;
    });
  }
});

// ---------------------------------------------------------------------------
// C-004 manual reconstruction: malformed pool-worker error payloads reject
// ---------------------------------------------------------------------------
test("C-004 EnginePool reconstructs missing pool-worker error payloads", async () => {
  const { pool, worker } = makePool();
  const pending = pool.evaluate("rules", {});

  // A present error property is an error frame even if a worker bug omitted
  // the payload. The public promise should reject with a deterministic error.
  worker.emit("message", {
    id: worker.messages[0].id,
    error: undefined,
  });

  await assert.rejects(pending, /Unknown pool worker error/);
});

// ---------------------------------------------------------------------------
// E-007 manual protocol guard: stray pool replies are ignored
// ---------------------------------------------------------------------------
test("E-007 EnginePool ignores replies for unknown request ids", () => {
  const { worker } = makePool();

  // Late messages can happen after cancellation/teardown races; the handler
  // should ignore them rather than throwing from the event emitter.
  assert.doesNotThrow(() => {
    worker.emit("message", { id: 999, result: "late" });
  });
});

// ---------------------------------------------------------------------------
// E-013 manual cleanup: worker error rejects all pending pool requests
// ---------------------------------------------------------------------------
test("E-013 EnginePool rejects pending requests when worker emits error", async () => {
  const { pool, worker } = makePool();
  const pending = pool.evaluate("rules", {});

  worker.emit("error", new Error("pool worker exploded"));
  await assert.rejects(pending, /pool worker exploded/);
});

// ---------------------------------------------------------------------------
// E-012/E-013 lease cleanup: a worker error rejecting the owner request
// invalidates the callback proxy. Comprehensive failed-slot queue and future
// admission coverage lives in terminal-state.test.ts.
// ---------------------------------------------------------------------------
test("E-012 E-013 EnginePool worker error invalidates the active callback lease", async () => {
  const { pool, worker } = makePool();
  let retained: any;

  const active = pool.do("rules", async (proxy) => {
    retained = proxy;
    return Promise.all([proxy.facts(), proxy.facts()]);
  });
  await waitForPostedMessage(worker);

  worker.emit("error", new Error("leased worker exploded"));
  await assert.rejects(active, /leased worker exploded/);

  const postsBeforeRetainedCall = worker.messages.length;
  await assert.rejects(
    () => retained.facts(),
    /EngineProxy is no longer valid outside its EnginePool\.do callback/,
  );
  assert.strictEqual(
    worker.messages.length,
    postsBeforeRetainedCall,
    "a retained proxy posted work after its callback failed",
  );

  await pool.close();
});

test("E-012 defensive drain rejects already-aborted owner and lease work", async () => {
  const { pool, slot } = makePool();

  const activeLease = await (pool as any).acquireLease(slot);
  const ownerAbort = new AbortController();
  ownerAbort.abort();
  const ownerPending = new Promise((_resolve, reject) => {
    activeLease.queue.push({
      kind: "request",
      req: { id: 0, method: "facts", args: ["rules"] },
      entry: { resolve: () => undefined, reject },
      signal: ownerAbort.signal,
      onAbort: () => undefined,
    });
  });
  (EnginePool as any).drainQueue(slot);
  await assert.rejects(ownerPending, (error: any) => error?.name === "AbortError");
  await (pool as any).releaseLease(slot, activeLease);

  const waitingLease = (EnginePool as any).createLease();
  const leaseAbort = new AbortController();
  leaseAbort.abort();
  const leasePending = new Promise((_resolve, reject) => {
    slot.queue.push({
      kind: "lease",
      lease: waitingLease,
      resolve: () => undefined,
      reject,
      signal: leaseAbort.signal,
      onAbort: () => undefined,
    });
  });
  (EnginePool as any).drainQueue(slot);
  await assert.rejects(leasePending, (error: any) => error?.name === "AbortError");
  assert.strictEqual(waitingLease.released, true);

  await pool.close();
});

test("E-012 private lease send guards reject before bookkeeping", async () => {
  const { pool, worker, slot } = makePool();
  const inactiveLease = (EnginePool as any).createLease();
  const snapshot = () => ({
    activeLease: slot.activeLease,
    inflight: slot.inflight,
    messages: worker.messages.length,
    nextId: slot.nextId,
    rootQueue: slot.queue.length,
  });

  const beforeInactiveSend = snapshot();
  await assert.rejects(
    () => (pool as any).sendOnLease(slot, inactiveLease, "facts", ["rules"]),
    /EngineProxy is no longer valid/,
  );
  assert.deepStrictEqual(snapshot(), beforeInactiveSend);
  assert.strictEqual(inactiveLease.pendingCalls, 0);
  assert.strictEqual(inactiveLease.queue.length, 0);

  const preAborted = new AbortController();
  preAborted.abort();
  const beforePreAbortedAcquire = snapshot();
  await assert.rejects(
    () => (pool as any).acquireLease(slot, preAborted.signal),
    (error: any) => error?.name === "AbortError",
  );
  assert.deepStrictEqual(snapshot(), beforePreAbortedAcquire);

  (pool as any).closed = true;
  const beforeClosedGuards = snapshot();
  await assert.rejects(
    () => (pool as any).acquireLease(slot),
    /EnginePool has been closed/,
  );
  await assert.rejects(
    () => (pool as any).sendToSlot(slot, "facts", ["rules"]),
    /EnginePool has been closed/,
  );
  assert.deepStrictEqual(snapshot(), beforeClosedGuards);
  (pool as any).closed = false;
  await pool.close();
});

test("E-012 lease release and terminal marking are idempotent", async () => {
  const { pool, slot } = makePool();
  const lease = await (pool as any).acquireLease(slot);

  const firstRelease = (pool as any).releaseLease(slot, lease);
  const repeatedRelease = (pool as any).releaseLease(slot, lease);
  assert.strictEqual(repeatedRelease, firstRelease);
  await firstRelease;

  assert.strictEqual(lease.released, true);
  assert.doesNotThrow(() => (EnginePool as any).markLeaseReleased(lease));
  assert.strictEqual(lease.released, true);

  await pool.close();
});

test("E-012 dispatch removes a queued owner abort listener", async () => {
  const { pool, worker, slot } = makePool();
  const abortController = new AbortController();
  let removed = false;
  const originalRemove = abortController.signal.removeEventListener.bind(
    abortController.signal,
  );
  abortController.signal.removeEventListener = ((type, listener, options) => {
    if (type === "abort") removed = true;
    return originalRemove(type, listener, options);
  }) as typeof abortController.signal.removeEventListener;
  const lease = await (pool as any).acquireLease(slot);
  const proxy = (pool as any).makeProxy(
    "rules",
    slot,
    lease,
    abortController.signal,
  );

  const first = proxy.facts();
  const second = proxy.facts();
  assert.strictEqual(lease.queue.length, 1);

  const firstMessage = worker.messages[0];
  worker.emit("message", { id: firstMessage.id, result: [] });
  assert.deepStrictEqual(await first, []);
  assert.strictEqual(removed, true);
  assert.strictEqual(worker.messages.length, 2);

  const secondMessage = worker.messages[1];
  worker.emit("message", { id: secondMessage.id, result: [] });
  assert.deepStrictEqual(await second, []);
  await (pool as any).releaseLease(slot, lease);
  await pool.close();
});

test("E-012 abort listener removes a queued owner request", async () => {
  const { pool, worker, slot } = makePool();
  const abortController = new AbortController();
  const lease = await (pool as any).acquireLease(slot);
  const proxy = (pool as any).makeProxy(
    "rules",
    slot,
    lease,
    abortController.signal,
  );

  const inflight = proxy.facts();
  const queued = proxy.facts();
  assert.strictEqual(lease.queue.length, 1);

  abortController.abort();
  await assert.rejects(queued, (error: any) => error?.name === "AbortError");
  assert.strictEqual(lease.queue.length, 0);

  const message = worker.messages[0];
  worker.emit("message", { id: message.id, result: [] });
  assert.deepStrictEqual(await inflight, []);
  await (pool as any).releaseLease(slot, lease);
  await pool.close();
});

test("E-012 retained proxy rejects before request allocation or postMessage", async () => {
  const { pool, worker, slot } = makePool();
  let retained: any;

  await pool.do("rules", async (proxy) => {
    retained = proxy;
  });

  const nextIdBeforeRetainedCall = slot.nextId;
  const postsBeforeRetainedCall = worker.messages.length;
  await assert.rejects(
    () => retained.run({ limit: Number.NaN }),
    /EngineProxy is no longer valid outside its EnginePool\.do callback/,
  );
  assert.strictEqual(slot.nextId, nextIdBeforeRetainedCall);
  assert.strictEqual(worker.messages.length, postsBeforeRetainedCall);

  await pool.close();
});

test("E-012 an earlier returned-Promise reaction drains before proxy invalidation", { timeout: 5_000 }, async () => {
  const { pool, worker, slot } = makePool();
  let resolveReturned!: (value: string) => void;
  const returned = new Promise<string>((resolve) => {
    resolveReturned = resolve;
  });
  let resolveCallbackEntered!: () => void;
  const callbackEntered = new Promise<void>((resolve) => {
    resolveCallbackEntered = resolve;
  });
  let retained: any;
  let reactionCall!: Promise<unknown>;
  let reactionIssued = false;
  let reactionSettled = false;

  const pending = pool.do("rules", (proxy) => {
    retained = proxy;
    // This reaction is registered before do() attaches its own settlement
    // reaction to the returned Promise, so it still belongs to the callback's
    // achievable lifetime boundary.
    void returned.then(() => {
      reactionIssued = true;
      reactionCall = proxy.facts().then((value) => {
        reactionSettled = true;
        return value;
      });
    });
    resolveCallbackEntered();
    return returned;
  });
  await callbackEntered;

  let doSettled = false;
  void pending.then(
    () => { doSettled = true; },
    () => { doSettled = true; },
  );
  resolveReturned("callback result");

  const message = await waitForPostedMessage(worker);
  assert.strictEqual(reactionIssued, true);
  assert.strictEqual(doSettled, false, "do() settled before the accepted reaction call drained");

  worker.emit("message", { id: message.id, result: [] });
  assert.deepStrictEqual(await reactionCall, []);
  assert.strictEqual(reactionSettled, true);
  assert.strictEqual(await pending, "callback result");

  const bookkeepingAfterSettlement = {
    inflight: slot.inflight,
    messages: worker.messages.length,
    nextId: slot.nextId,
    ownerQueue: slot.activeLease?.queue.length ?? 0,
    rootQueue: slot.queue.length,
  };
  await assert.rejects(
    () => retained.facts(),
    /EngineProxy is no longer valid outside its EnginePool\.do callback/,
  );
  assert.deepStrictEqual(
    {
      inflight: slot.inflight,
      messages: worker.messages.length,
      nextId: slot.nextId,
      ownerQueue: slot.activeLease?.queue.length ?? 0,
      rootQueue: slot.queue.length,
    },
    bookkeepingAfterSettlement,
  );

  await pool.close();
});

// ---------------------------------------------------------------------------
// E-013 table-driven cleanup: worker exit rejects pending pool requests
// ---------------------------------------------------------------------------
test("E-013 table-driven EnginePool rejects pending requests on worker exit", async () => {
  for (const [code, pattern] of [
    [0, /exited before responding/],
    [9, /unexpectedly with code 9/],
  ] as const) {
    const { pool, worker } = makePool();
    const pending = pool.evaluate("rules", {});
    worker.emit("exit", code);
    await assert.rejects(pending, pattern);
  }
});

// ---------------------------------------------------------------------------
// E-004 manual queue cleanup: drainQueue rejects already-aborted queued work
// ---------------------------------------------------------------------------
test("E-004 EnginePool drainQueue rejects a queued request whose signal aborted", async () => {
  const { slot } = makePool();
  const ac = new AbortController();
  ac.abort();

  const pending = new Promise((_resolve, reject) => {
    slot.queue.push({
      kind: "request",
      req: { id: 0, method: "facts", args: ["rules"] },
      entry: { resolve: () => undefined, reject },
      signal: ac.signal,
      onAbort: () => undefined,
    });
  });

  // This covers the defensive drain-time check: even if an abort listener did
  // not remove a queued request, the pool still rejects it before dispatch.
  (EnginePool as any).drainQueue(slot);
  await assert.rejects(pending, (err: any) => {
    assert.strictEqual(err.name, "AbortError");
    return true;
  });
});

// ---------------------------------------------------------------------------
// E-006 manual do(): rejection path removes abort listener and rejects
// ---------------------------------------------------------------------------
test("E-006 EnginePool.do with signal propagates callback rejection", async () => {
  const { pool } = makePool();
  const ac = new AbortController();

  await assert.rejects(
    () => pool.do("rules", async () => {
      throw new Error("callback failed");
    }, { signal: ac.signal }),
    /callback failed/,
  );
});

test("E-006 E-012 abort after callback settlement cannot replace its outcome", async () => {
  for (const callbackError of [undefined, new Error("callback outcome")]) {
    const { pool, worker } = makePool();
    const ac = new AbortController();
    let acceptedCall!: Promise<unknown>;
    let outerAbortListenerRemoved = false;
    const originalRemove = ac.signal.removeEventListener.bind(ac.signal);
    ac.signal.removeEventListener = ((type, listener, options) => {
      if (type === "abort") outerAbortListenerRemoved = true;
      return originalRemove(type, listener, options);
    }) as typeof ac.signal.removeEventListener;

    const pending = pool.do("rules", async (proxy) => {
      acceptedCall = proxy.facts();
      if (callbackError) throw callbackError;
      return "callback result";
    }, { signal: ac.signal });
    let settled = false;
    const outcome = pending.then(
      (value) => {
        settled = true;
        return { status: "fulfilled" as const, value };
      },
      (error: unknown) => {
        settled = true;
        return { status: "rejected" as const, error };
      },
    );
    const message = await waitForPostedMessage(worker);

    // The raw callback has settled, but do() remains pending until its accepted
    // worker request drains. Cancellation after this boundary cannot replace
    // either the callback's value or its error.
    assert.strictEqual(outerAbortListenerRemoved, true);
    ac.abort();
    await Promise.resolve();
    assert.strictEqual(settled, false);

    worker.emit("message", { id: message.id, result: [] });
    assert.deepStrictEqual(await acceptedCall, []);
    const observed = await outcome;
    if (callbackError) {
      assert.strictEqual(observed.status, "rejected");
      assert.strictEqual((observed as { error: unknown }).error, callbackError);
    } else {
      assert.deepStrictEqual(observed, {
        status: "fulfilled",
        value: "callback result",
      });
    }
    await pool.close();
  }
});

// ---------------------------------------------------------------------------
// F-004 manual closed-state: do() uses the same closed guard as evaluate()
// ---------------------------------------------------------------------------
test("F-004 EnginePool.do rejects after close", async () => {
  const { pool } = makePool();
  (pool as any).closed = true;

  // evaluate() and do() have separate public entry guards; this explicit case
  // prevents one from regressing while the other remains covered.
  await assert.rejects(
    () => pool.do("rules", async () => undefined),
    /EnginePool has been closed/,
  );
});

// ---------------------------------------------------------------------------
// D-006 manual proxy run: already-aborted signal sets abort buffer before send
// ---------------------------------------------------------------------------
test("D-006 EnginePool proxy run honors already-aborted retained signal", async () => {
  const { pool, worker, slot } = makePool();
  const ac = new AbortController();
  ac.abort();
  const lease = await (pool as any).acquireLease(slot);
  const proxy = (pool as any).makeProxy("rules", slot, lease, ac.signal);

  const pending = proxy.run({ limit: 3 });
  const message = worker.messages[0];
  const abortBuffer = new Int32Array(message.args[2]);
  assert.strictEqual(Atomics.load(abortBuffer, 0), 1);

  worker.emit("message", {
    id: message.id,
    result: { rulesFired: 0, haltReason: 2 },
  });
  assert.deepStrictEqual(await pending, { rulesFired: 0, haltReason: 2 });
  await (pool as any).releaseLease(slot, lease);
  await pool.close();
});

// ---------------------------------------------------------------------------
// D-006 table-driven proxy run: live abort signals set and remove listeners
// ---------------------------------------------------------------------------
test("D-006 table-driven EnginePool proxy run handles abort states", async () => {
  const { pool, worker, slot } = makePool();
  const ac = new AbortController();
  const lease = await (pool as any).acquireLease(slot);
  const proxy = (pool as any).makeProxy("rules", slot, lease, ac.signal);

  const pending = proxy.run({ limit: 5 });
  const message = worker.messages[0];
  const abortBuffer = new Int32Array(message.args[2]);
  assert.strictEqual(Atomics.load(abortBuffer, 0), 0);

  ac.abort();
  assert.strictEqual(Atomics.load(abortBuffer, 0), 1);

  worker.emit("message", {
    id: message.id,
    result: { rulesFired: 2, haltReason: 2 },
  });
  assert.deepStrictEqual(await pending, { rulesFired: 2, haltReason: 2 });
  await (pool as any).releaseLease(slot, lease);
  await pool.close();
});

// ---------------------------------------------------------------------------
// E-009 manual lifecycle: Symbol.asyncDispose delegates to close
// ---------------------------------------------------------------------------
test("E-009 EnginePool Symbol.asyncDispose delegates to close", async () => {
  const { pool, worker } = makePool();
  await pool[Symbol.asyncDispose]();
  assert.strictEqual(worker.terminateCalls, 1);
});

// ---------------------------------------------------------------------------
// E-008 manual close cleanup: queued abort listeners are removed on close
// ---------------------------------------------------------------------------
test("E-008 EnginePool close removes queued abort listeners", async () => {
  const { pool, slot } = makePool();
  const ac = new AbortController();
  let removed = false;
  const originalRemove = ac.signal.removeEventListener.bind(ac.signal);
  ac.signal.removeEventListener = ((type, listener, options) => {
    if (type === "abort") removed = true;
    return originalRemove(type, listener, options);
  }) as typeof ac.signal.removeEventListener;

  slot.queue.push({
    kind: "request",
    req: { id: 0, method: "facts", args: ["rules"] },
    entry: { resolve: () => undefined, reject: () => undefined },
    signal: ac.signal,
    onAbort: () => undefined,
  });

  // close() must clean queued abort listeners so callers do not retain signals
  // after the pool is torn down.
  await pool.close();
  assert.strictEqual(removed, true);
});

// ---------------------------------------------------------------------------
// E-001 manual creation: spec options are forwarded into worker init payload
// ---------------------------------------------------------------------------
test("E-001 EnginePool.create accepts explicit EngineSpec options", async () => {
  const pool = await EnginePool.create(
    [{
      name: "configured",
      source: "(defrule ok (initial-fact) =>)",
      options: {
        strategy: Strategy.Breadth,
        encoding: Encoding.Utf8,
        maxCallDepth: 8,
      },
    }],
    { threads: 1 },
  );
  try {
    const result = await pool.evaluate("configured", {});
    assert.strictEqual(result.runResult.rulesFired, 1);
  } finally {
    await pool.close();
  }
});
