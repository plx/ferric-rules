/**
 * EngineHandle (worker-backed) runtime smoke tests.
 */
import { test } from "node:test";
import * as assert from "node:assert/strict";

import {
  EngineHandle,
  HaltReason,
} from "../../../helpers/ferric";

const BASIC_RULE = `
(defrule hello-world
  (initial-fact)
  =>
  (printout t "hello from ferric" crlf))
`;

// ---------------------------------------------------------------------------
// D-001: EngineHandle.create({source}) performs load + reset
// ---------------------------------------------------------------------------
test("D-001 EngineHandle.create with source loads and resets", async () => {
  const handle = await EngineHandle.create({ source: BASIC_RULE });
  try {
    const result = await handle.run();
    assert.strictEqual(result.rulesFired, 1);
    assert.strictEqual(result.haltReason, HaltReason.AgendaEmpty);
  } finally {
    await handle.close();
  }
});

// ---------------------------------------------------------------------------
// D-004: Pre-aborted signal rejects with AbortError
// ---------------------------------------------------------------------------
test("D-004 run with pre-aborted signal rejects with AbortError", async () => {
  const handle = await EngineHandle.create({ source: BASIC_RULE });
  try {
    const ac = new AbortController();
    ac.abort();
    await assert.rejects(
      () => handle.run({ signal: ac.signal }),
      (err: any) => err.name === "AbortError"
    );
  } finally {
    await handle.close();
  }
});

// ---------------------------------------------------------------------------
// F-003: EngineHandle.close() is idempotent
// ---------------------------------------------------------------------------
test("F-003 EngineHandle.close is idempotent", async () => {
  const handle = await EngineHandle.create();
  await handle.close();
  await handle.close(); // second call should not throw
});
