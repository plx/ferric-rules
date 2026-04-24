/**
 * Run-limit semantics tests for EnginePool (N-01, N-02).
 */
import { test } from "node:test";
import * as assert from "node:assert/strict";

import { EnginePool, HaltReason } from "../../../helpers/ferric";

const LOOP_RULE = `
(defrule loop
  ?f <- (counter ?n)
  =>
  (retract ?f)
  (assert (counter (+ ?n 1))))
`;

// ---------------------------------------------------------------------------
// N-02: evaluate limit=0 means unlimited
// ---------------------------------------------------------------------------
test("E-002 evaluate with limit=0 runs unlimited", async () => {
  const pool = await EnginePool.create(
    [{
      name: "test",
      source: "(defrule once (initial-fact) => (printout t \"done\" crlf))",
    }],
    { threads: 1 },
  );
  try {
    const result = await pool.evaluate("test", { limit: 0 });
    assert.strictEqual(result.runResult.rulesFired, 1);
    assert.strictEqual(result.runResult.haltReason, HaltReason.AgendaEmpty);
  } finally {
    await pool.close();
  }
});

// ---------------------------------------------------------------------------
// N-02: evaluate with omitted limit means unlimited
// ---------------------------------------------------------------------------
test("E-002 evaluate with no limit runs unlimited", async () => {
  const pool = await EnginePool.create(
    [{
      name: "test",
      source: "(defrule once (initial-fact) => (printout t \"done\" crlf))",
    }],
    { threads: 1 },
  );
  try {
    const result = await pool.evaluate("test", {});
    assert.strictEqual(result.runResult.rulesFired, 1);
    assert.strictEqual(result.runResult.haltReason, HaltReason.AgendaEmpty);
  } finally {
    await pool.close();
  }
});

test("E-002 evaluate with limit=null runs unlimited", async () => {
  const pool = await EnginePool.create(
    [{
      name: "test",
      source: "(defrule once (initial-fact) => (printout t \"done\" crlf))",
    }],
    { threads: 1 },
  );
  try {
    const result = await pool.evaluate("test", { limit: null as unknown as number });
    assert.strictEqual(result.runResult.rulesFired, 1);
    assert.strictEqual(result.runResult.haltReason, HaltReason.AgendaEmpty);
  } finally {
    await pool.close();
  }
});

// ---------------------------------------------------------------------------
// N-02: evaluate with positive limit caps firings
// ---------------------------------------------------------------------------
test("E-002 evaluate with limit=5 caps at 5 firings", async () => {
  const pool = await EnginePool.create(
    [{ name: "test", source: LOOP_RULE }],
    { threads: 1 },
  );
  try {
    const result = await pool.evaluate("test", {
      facts: [{ kind: "ordered", relation: "counter", fields: [0] }],
      limit: 5,
    });
    assert.strictEqual(result.runResult.rulesFired, 5);
    assert.strictEqual(result.runResult.haltReason, HaltReason.LimitReached);
  } finally {
    await pool.close();
  }
});

// ---------------------------------------------------------------------------
// N-01: proxy run with limit=0 means zero firings
// ---------------------------------------------------------------------------
test("D-006 pool proxy run({limit:0}) fires zero rules", async () => {
  const pool = await EnginePool.create(
    [{ name: "test", source: LOOP_RULE }],
    { threads: 1 },
  );
  try {
    const result = await pool.do("test", async (proxy) => {
      await proxy.reset();
      await proxy.assertFact("counter", 0);
      return proxy.run({ limit: 0 });
    });
    assert.strictEqual(result.rulesFired, 0);
    assert.strictEqual(result.haltReason, HaltReason.LimitReached);
  } finally {
    await pool.close();
  }
});

test("E-002 evaluate rejects invalid limits", async () => {
  const pool = await EnginePool.create(
    [{ name: "test", source: LOOP_RULE }],
    { threads: 1 },
  );
  try {
    for (const limit of [-1, 1.5, Number.NaN, Number.POSITIVE_INFINITY]) {
      await assert.rejects(
        () => pool.evaluate("test", { limit }),
        (err: unknown) => err instanceof TypeError && /limit/.test(err.message),
      );
    }
  } finally {
    await pool.close();
  }
});

test("D-006 pool proxy run rejects invalid limits", async () => {
  const pool = await EnginePool.create(
    [{ name: "test", source: LOOP_RULE }],
    { threads: 1 },
  );
  try {
    await assert.rejects(
      () => pool.do("test", async (proxy) => {
        await proxy.reset();
        await proxy.assertFact("counter", 0);
        return proxy.run({ limit: Number.NaN });
      }),
      (err: unknown) => err instanceof TypeError && /limit/.test(err.message),
    );
  } finally {
    await pool.close();
  }
});
