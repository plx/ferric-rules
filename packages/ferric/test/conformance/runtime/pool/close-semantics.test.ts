/**
 * EnginePool close semantics tests (E-008, E-009).
 */
import { test } from "node:test";
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
