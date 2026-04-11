/**
 * EnginePool runtime smoke tests.
 */
import { test } from "node:test";
import * as assert from "node:assert/strict";

import {
  EnginePool,
  HaltReason,
} from "../../../helpers/ferric";

const BASIC_RULE = `
(defrule hello-world
  (initial-fact)
  =>
  (printout t "hello from ferric" crlf))
`;

// ---------------------------------------------------------------------------
// E-002: evaluate performs reset -> assert -> run -> collect
// ---------------------------------------------------------------------------
test("E-002 evaluate performs reset/assert/run/collect lifecycle", async () => {
  const pool = await EnginePool.create(
    [{ name: "basic", source: BASIC_RULE }],
    { threads: 1 },
  );
  try {
    const result = await pool.evaluate("basic", {});
    assert.strictEqual(result.runResult.rulesFired, 1);
    assert.strictEqual(result.runResult.haltReason, HaltReason.AgendaEmpty);
    assert.ok(result.output.stdout?.includes("hello from ferric"));
  } finally {
    await pool.close();
  }
});

// ---------------------------------------------------------------------------
// E-003: Pre-aborted signal rejects
// ---------------------------------------------------------------------------
test("E-003 evaluate with pre-aborted signal rejects", async () => {
  const pool = await EnginePool.create(
    [{ name: "basic", source: BASIC_RULE }],
    { threads: 1 },
  );
  try {
    const ac = new AbortController();
    ac.abort();
    await assert.rejects(
      () => pool.evaluate("basic", {}, { signal: ac.signal }),
      (err: any) => err.name === "AbortError"
    );
  } finally {
    await pool.close();
  }
});

// ---------------------------------------------------------------------------
// F-004: close prevents new submissions
// ---------------------------------------------------------------------------
test("F-004 close prevents new submissions", async () => {
  const pool = await EnginePool.create(
    [{ name: "basic", source: BASIC_RULE }],
    { threads: 1 },
  );
  await pool.close();
  await assert.rejects(
    () => pool.evaluate("basic", {}),
    /closed/
  );
});
