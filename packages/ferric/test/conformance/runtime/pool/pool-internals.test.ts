/**
 * EnginePool defensive-branch tests with fake Worker slots.
 */
import { EventEmitter } from "node:events";
import { test } from "node:test";
import * as assert from "node:assert/strict";

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

// ---------------------------------------------------------------------------
// C-004 property-style reconstruction for special pool-worker errors
// ---------------------------------------------------------------------------
test("C-004 property-style EnginePool reconstructs special worker errors", async () => {
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
// E-008 manual cleanup: worker error rejects all pending pool requests
// ---------------------------------------------------------------------------
test("E-008 EnginePool rejects pending requests when worker emits error", async () => {
  const { pool, worker } = makePool();
  const pending = pool.evaluate("rules", {});

  worker.emit("error", new Error("pool worker exploded"));
  await assert.rejects(pending, /pool worker exploded/);
});

// ---------------------------------------------------------------------------
// E-008 property-style cleanup: worker exit rejects pending pool requests
// ---------------------------------------------------------------------------
test("E-008 property-style EnginePool rejects pending requests on worker exit", async () => {
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
// E-008 manual send guard: retained proxy calls reject after close
// ---------------------------------------------------------------------------
test("E-008 EnginePool retained proxy calls use closed send guard", async () => {
  const { pool, slot } = makePool();
  const proxy = (pool as any).makeProxy("rules", slot);
  (pool as any).closed = true;

  await assert.rejects(() => proxy.facts(), /EnginePool has been closed/);
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
  const proxy = (pool as any).makeProxy("rules", slot, ac.signal);

  const pending = proxy.run({ limit: 3 });
  const message = worker.messages[0];
  const abortBuffer = new Int32Array(message.args[2]);
  assert.strictEqual(Atomics.load(abortBuffer, 0), 1);

  worker.emit("message", {
    id: message.id,
    result: { rulesFired: 0, haltReason: 2 },
  });
  assert.deepStrictEqual(await pending, { rulesFired: 0, haltReason: 2 });
});

// ---------------------------------------------------------------------------
// D-006 property-style proxy run: live abort signals set and remove listeners
// ---------------------------------------------------------------------------
test("D-006 property-style EnginePool proxy run handles generated abort states", async () => {
  const { pool, worker, slot } = makePool();
  const ac = new AbortController();
  const proxy = (pool as any).makeProxy("rules", slot, ac.signal);

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
