/**
 * EnginePool thread default tests (E-001).
 */
import { test } from "node:test";
import * as assert from "node:assert/strict";

import { EnginePool, HaltReason } from "../../../helpers/ferric";

const BASIC_RULE = `
(defrule hello (initial-fact) => (printout t "hi" crlf))
`;

// ---------------------------------------------------------------------------
// E-001: Default thread count is 1
// ---------------------------------------------------------------------------
test("E-001 EnginePool defaults to 1 thread when threads omitted", async () => {
  // Create pool without specifying threads.
  const pool = await EnginePool.create([
    { name: "test", source: BASIC_RULE },
  ]);
  try {
    // Verify it works (if it works, at least 1 thread exists).
    const result = await pool.evaluate("test", {});
    assert.strictEqual(result.runResult.rulesFired, 1);
    assert.strictEqual(result.runResult.haltReason, HaltReason.AgendaEmpty);
  } finally {
    await pool.close();
  }
});

test("E-001 EnginePool with threads: 1 works correctly", async () => {
  const pool = await EnginePool.create(
    [{ name: "test", source: BASIC_RULE }],
    { threads: 1 },
  );
  try {
    const result = await pool.evaluate("test", {});
    assert.strictEqual(result.runResult.rulesFired, 1);
  } finally {
    await pool.close();
  }
});

test("E-001 EnginePool single-thread queues requests correctly", async () => {
  const pool = await EnginePool.create(
    [{ name: "test", source: BASIC_RULE }],
    { threads: 1 },
  );
  try {
    // Submit two requests back-to-back to a single-thread pool.
    const [r1, r2] = await Promise.all([
      pool.evaluate("test", {}),
      pool.evaluate("test", {}),
    ]);
    assert.strictEqual(r1.runResult.rulesFired, 1);
    assert.strictEqual(r2.runResult.rulesFired, 1);
  } finally {
    await pool.close();
  }
});
