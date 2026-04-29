/**
 * EnginePool cancellation semantics tests (E-004, E-006).
 */
import { test } from "node:test";
import * as assert from "node:assert/strict";
import { setTimeout as delay } from "node:timers/promises";

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
// E-004: Queued evaluate aborted while waiting rejects with AbortError
// ---------------------------------------------------------------------------
test("E-004 queued evaluate abort rejects with AbortError", async () => {
  const pool = await EnginePool.create(
    [{ name: "test", source: LONG_RUNNING }],
    { threads: 1 },
  );
  try {
    // Start a long-running evaluate to occupy the single thread.
    const longRunning = pool.evaluate("test", {
      facts: [{ kind: "ordered", relation: "counter", fields: [0] }],
      limit: 10000,
    });

    // Submit a second evaluate with an abort signal.
    const ac = new AbortController();
    const queued = pool.evaluate("test", {
      facts: [{ kind: "ordered", relation: "counter", fields: [0] }],
    }, { signal: ac.signal });

    // Abort the second request while it's queued.
    ac.abort();

    // The queued request should reject with AbortError.
    await assert.rejects(queued, (err: any) => {
      assert.strictEqual(err.name, "AbortError");
      return true;
    });

    // The first request should still complete.
    const result = await longRunning;
    assert.ok(result.runResult.rulesFired > 0);
  } finally {
    await pool.close();
  }
});

// ---------------------------------------------------------------------------
// E-003: Already-aborted evaluate rejects immediately
// ---------------------------------------------------------------------------
test("E-003 pre-aborted evaluate rejects immediately", async () => {
  const pool = await EnginePool.create(
    [{ name: "test", source: LONG_RUNNING }],
    { threads: 1 },
  );
  try {
    const ac = new AbortController();
    ac.abort();
    await assert.rejects(
      () => pool.evaluate("test", {}, { signal: ac.signal }),
      (err: any) => err.name === "AbortError"
    );
  } finally {
    await pool.close();
  }
});

// ---------------------------------------------------------------------------
// E-006: do() with abort rejects with AbortError
// ---------------------------------------------------------------------------
test("E-006 do() pre-aborted rejects with AbortError", async () => {
  const pool = await EnginePool.create(
    [{ name: "test", source: LONG_RUNNING }],
    { threads: 1 },
  );
  try {
    const ac = new AbortController();
    ac.abort();
    await assert.rejects(
      () => pool.do("test", async (proxy) => {
        await proxy.reset();
      }, { signal: ac.signal }),
      (err: any) => err.name === "AbortError"
    );
  } finally {
    await pool.close();
  }
});

test("E-006 do() abort during callback rejects with AbortError", async () => {
  const pool = await EnginePool.create(
    [{ name: "test", source: LONG_RUNNING }],
    { threads: 1 },
  );
  try {
    const ac = new AbortController();
    // Abort after a short delay to catch the callback mid-execution.
    setTimeout(() => ac.abort(), 10);

    await assert.rejects(
      () => pool.do("test", async (proxy) => {
        await proxy.reset();
        await proxy.assertFact("counter", 0);
        // This long-running step should be interrupted
        await proxy.run({ limit: 100000 });
      }, { signal: ac.signal }),
      (err: any) => err.name === "AbortError"
    );
  } finally {
    await pool.close();
  }
});

// ---------------------------------------------------------------------------
// E-006: do() abort propagates to worker's batched_run (cooperative halt)
// ---------------------------------------------------------------------------
test("E-006 do() abort halts worker run so slot frees quickly", async () => {
  const pool = await EnginePool.create(
    [{ name: "test", source: LONG_RUNNING }],
    { threads: 1 },
  );
  try {
    const ac = new AbortController();
    setTimeout(() => ac.abort(), 20);

    const longRun = pool.do("test", async (proxy) => {
      await proxy.reset();
      await proxy.assertFact("counter", 0);
      await proxy.run({ limit: 10_000_000 });
    }, { signal: ac.signal });

    await assert.rejects(longRun, (err: any) => err.name === "AbortError");

    // If the worker kept running the orphaned 10M-firing loop, this
    // follow-up call would sit in the queue for a very long time. With
    // cooperative abort wired through proxy.run(), the worker halts
    // promptly and the slot becomes available.
    const started = Date.now();
    const followUp = pool.do("test", async (proxy) => {
      await proxy.reset();
      return "ok";
    });
    const result = await Promise.race([
      followUp,
      delay(2000).then(() => "timeout"),
    ]);
    const elapsed = Date.now() - started;
    assert.strictEqual(result, "ok", `follow-up did not complete in time (elapsed=${elapsed}ms)`);
  } finally {
    await pool.close();
  }
});

// ---------------------------------------------------------------------------
// E-005: In-execution abort uses cooperative halt
// ---------------------------------------------------------------------------
test("E-005 evaluate in-execution abort uses cooperative halt", async () => {
  const pool = await EnginePool.create(
    [{ name: "test", source: LONG_RUNNING }],
    { threads: 1 },
  );
  try {
    const ac = new AbortController();
    // Abort after a short delay.
    setTimeout(() => ac.abort(), 50);

    const result = await pool.evaluate("test", {
      facts: [{ kind: "ordered", relation: "counter", fields: [0] }],
    }, { signal: ac.signal });

    // Should complete with partial results and halt.
    assert.ok(result.runResult.rulesFired > 0);
    assert.strictEqual(result.runResult.haltReason, HaltReason.HaltRequested);
  } finally {
    await pool.close();
  }
});
