/**
 * Run-limit semantics tests for EngineHandle (D-006, N-01).
 */
import { test } from "node:test";
import * as assert from "node:assert/strict";

import { EngineHandle, HaltReason } from "../../../helpers/ferric";

const LOOP_RULE = `
(defrule loop
  ?f <- (counter ?n)
  =>
  (retract ?f)
  (assert (counter (+ ?n 1))))
`;

// ---------------------------------------------------------------------------
// D-006 / N-01: limit=0 means zero firings
// ---------------------------------------------------------------------------
test("D-006 EngineHandle.run({limit:0}) fires zero rules", async () => {
  const handle = await EngineHandle.create({ source: LOOP_RULE });
  try {
    await handle.reset();
    await handle.assertString("(counter 0)");
    const result = await handle.run({ limit: 0 });
    assert.strictEqual(result.rulesFired, 0);
    assert.strictEqual(result.haltReason, HaltReason.LimitReached);
  } finally {
    await handle.close();
  }
});

// ---------------------------------------------------------------------------
// N-01: undefined limit means unlimited
// ---------------------------------------------------------------------------
test("D-006 EngineHandle.run() without limit runs agenda", async () => {
  const handle = await EngineHandle.create({
    source: "(defrule once (initial-fact) => (printout t \"done\" crlf))",
  });
  try {
    await handle.reset();
    const result = await handle.run();
    assert.strictEqual(result.rulesFired, 1);
    assert.strictEqual(result.haltReason, HaltReason.AgendaEmpty);
  } finally {
    await handle.close();
  }
});

// ---------------------------------------------------------------------------
// N-01: positive limit caps firings
// ---------------------------------------------------------------------------
test("D-006 EngineHandle.run({limit:3}) fires at most 3 rules", async () => {
  const handle = await EngineHandle.create({ source: LOOP_RULE });
  try {
    await handle.reset();
    await handle.assertString("(counter 0)");
    const result = await handle.run({ limit: 3 });
    assert.strictEqual(result.rulesFired, 3);
    assert.strictEqual(result.haltReason, HaltReason.LimitReached);
  } finally {
    await handle.close();
  }
});
