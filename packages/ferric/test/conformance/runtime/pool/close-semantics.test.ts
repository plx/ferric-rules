/**
 * EnginePool close semantics tests (E-008, E-009).
 */
import { test } from "node:test";
import { getEventListeners } from "node:events";
import * as assert from "node:assert/strict";

import {
  EnginePool,
  HaltReason,
} from "../../../helpers/ferric";

const LONG_RUNNING = `
(defrule loop
  ?f <- (counter ?n)
  =>
  (retract ?f)
  (assert (counter (+ ?n 1))))
`;

// ---------------------------------------------------------------------------
// E-008: close() waits for in-flight requests to settle
// ---------------------------------------------------------------------------
test("E-008 close waits for in-flight request to complete", async () => {
  const pool = await EnginePool.create(
    [{ name: "test", source: LONG_RUNNING }],
    { threads: 1 },
  );

  // Start a long-running evaluation.
  const evalPromise = pool.evaluate("test", {
    facts: [{ kind: "ordered", relation: "counter", fields: [0] }],
    limit: 1000,
  });

  // Call close() while the evaluation is in progress.
  const closePromise = pool.close();

  // Both should resolve without error.
  const [result] = await Promise.all([evalPromise, closePromise]);
  assert.strictEqual(result.runResult.rulesFired, 1000);
  assert.strictEqual(result.runResult.haltReason, HaltReason.LimitReached);
});

// ---------------------------------------------------------------------------
// E-008: New requests after close reject
// ---------------------------------------------------------------------------
test("E-008 new requests after close reject", async () => {
  const pool = await EnginePool.create(
    [{ name: "test", source: LONG_RUNNING }],
    { threads: 1 },
  );
  await pool.close();

  await assert.rejects(
    () => pool.evaluate("test", {}),
    /closed/
  );
});

// ---------------------------------------------------------------------------
// E-009: close() is idempotent
// ---------------------------------------------------------------------------
test("E-009 close is idempotent", async () => {
  const pool = await EnginePool.create(
    [{ name: "test", source: LONG_RUNNING }],
    { threads: 1 },
  );

  await pool.close();
  await pool.close(); // second call should not throw
  await pool.close(); // third call should not throw
});

// ---------------------------------------------------------------------------
// E-008: Queued requests are rejected on close
// ---------------------------------------------------------------------------
test("E-008 queued requests are rejected when pool closes", async () => {
  const pool = await EnginePool.create(
    [{ name: "test", source: LONG_RUNNING }],
    { threads: 1 },
  );

  // Start a long-running evaluation to occupy the thread.
  const longRunning = pool.evaluate("test", {
    facts: [{ kind: "ordered", relation: "counter", fields: [0] }],
    limit: 5000,
  });

  // Queue a second request.
  const queued = pool.evaluate("test", {
    facts: [{ kind: "ordered", relation: "counter", fields: [0] }],
  });

  // Close the pool — the in-flight should settle, the queued should reject.
  const closePromise = pool.close();

  // The queued request should reject.
  await assert.rejects(queued, /closed/);

  // The in-flight and close should settle.
  const result = await longRunning;
  assert.ok(result.runResult.rulesFired > 0);
  await closePromise;
});


test("all pool closes wait for every worker even when one termination fails", async () => {
  const pool = await EnginePool.create([{ name: "test", source: LONG_RUNNING }], { threads: 2 });
  const slots = (pool as any).slots;
  let release!: () => void;
  const gate = new Promise<void>((resolve) => { release = resolve; });
  let enter!: () => void;
  const entered = new Promise<void>((resolve) => { enter = resolve; });
  const failure = new Error("first worker termination failed");
  const calls = [0, 0];
  for (let index = 0; index < 2; index++) {
    const original = slots[index].worker.terminate.bind(slots[index].worker);
    slots[index].worker.terminate = async () => {
      calls[index]++;
      if (index === 1) { enter(); await gate; }
      const result = await original();
      if (index === 0) throw failure;
      return result;
    };
  }
  const first = pool.close();
  const second = pool.close();
  assert.strictEqual(first, second);
  let settled = false;
  const closed = first.then(() => { settled = true; }, (error) => { settled = true; return error; });
  try {
    await entered;
    await new Promise<void>((resolve) => setImmediate(resolve));
    assert.strictEqual(settled, false);
  } finally { release(); }
  assert.strictEqual(await closed, failure);
  assert.deepStrictEqual(calls, [1, 1]);
  assert.strictEqual(pool.close(), first);
});


test("pool close publishes its barrier before queued signal cleanup reenters", { timeout: 5_000 }, async () => {
  const pool = await EnginePool.create([{ name: "test", source: "" }], { threads: 1 });
  let releaseLease!: () => void;
  const leaseGate = new Promise<void>((resolve) => { releaseLease = resolve; });
  let enterLease!: () => void;
  const leaseEntered = new Promise<void>((resolve) => { enterLease = resolve; });
  let releaseTermination!: () => void;
  const terminationGate = new Promise<void>((resolve) => { releaseTermination = resolve; });
  let enterTermination!: () => void;
  const terminationEntered = new Promise<void>((resolve) => { enterTermination = resolve; });
  const worker = (pool as any).slots[0].worker;
  const terminate = worker.terminate.bind(worker);
  let terminations = 0;
  worker.terminate = async () => {
    terminations++;
    enterTermination();
    await terminationGate;
    return terminate();
  };
  const admitted = pool.do("test", async () => { enterLease(); await leaseGate; });
  await leaseEntered;
  const controller = new AbortController();
  let nestedClose: Promise<void> | undefined;
  const remove = controller.signal.removeEventListener.bind(controller.signal);
  controller.signal.removeEventListener = (...args) => {
    nestedClose = pool.close();
    remove(...args);
  };
  const queued = pool.evaluate("test", {}, { signal: controller.signal });
  const rejected = assert.rejects(queued, /closed/);
  await new Promise<void>((resolve) => setImmediate(resolve));
  const outerClose = pool.close();
  let settled = false;
  void outerClose.then(() => { settled = true; });
  try {
    await rejected;
    assert.strictEqual(nestedClose, outerClose);
    releaseLease();
    await admitted;
    await terminationEntered;
    await new Promise<void>((resolve) => setImmediate(resolve));
    assert.strictEqual(settled, false, "close resolved before native worker termination");
    assert.strictEqual(terminations, 1);
  } finally {
    releaseLease();
    releaseTermination();
    await Promise.allSettled([admitted, queued, outerClose, nestedClose]);
  }
});


test("callback signal cleanup cannot replace settlement or create an unhandled rejection", async () => {
  const pool = await EnginePool.create([{ name: "test", source: "" }], { threads: 1 });
  try {
    for (const fails of [false, true]) {
      const controller = new AbortController();
      const remove = controller.signal.removeEventListener.bind(controller.signal);
      let callbackStarted = false;
      let cleanupThrows = 0;
      controller.signal.removeEventListener = (...args) => {
        remove(...args);
        if (callbackStarted) { cleanupThrows++; throw new Error("cleanup hook failed"); }
      };
      const failure = new Error("original callback error");
      const result = pool.do("test", async () => {
        callbackStarted = true;
        if (fails) throw failure;
        return 42;
      }, { signal: controller.signal });
      if (fails) await assert.rejects(result, (error) => error === failure);
      else assert.equal(await result, 42);
      await new Promise<void>((resolve) => setImmediate(resolve));
      assert.ok(cleanupThrows > 0);
      assert.equal(getEventListeners(controller.signal, "abort").length, 0);
    }
    assert.equal(await pool.do("test", async () => 99), 99);
  } finally { await pool.close(); }
});
