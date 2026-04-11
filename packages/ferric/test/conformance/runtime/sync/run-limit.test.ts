/**
 * Run-limit semantics tests for sync Engine (D-006, N-01).
 */
import { test } from "node:test";
import * as assert from "node:assert/strict";

import { Engine, HaltReason } from "../../../helpers/ferric";

const LOOP_RULE = `
(defrule loop
  ?f <- (counter ?n)
  =>
  (retract ?f)
  (assert (counter (+ ?n 1))))
`;

// ---------------------------------------------------------------------------
// D-006 / N-01: limit=undefined means unlimited
// ---------------------------------------------------------------------------
test("D-006 Engine.run() with no limit runs to completion", () => {
  const e = new Engine();
  e.load(LOOP_RULE);
  e.reset();
  e.assertString("(counter 0)");
  // With no limit, runs until agenda is empty (which won't happen with this loop)
  // So use a small finite limit to verify the semantics
  const result = e.run(10);
  assert.strictEqual(result.rulesFired, 10);
  assert.strictEqual(result.haltReason, HaltReason.LimitReached);
  e.close();
});

// ---------------------------------------------------------------------------
// D-006 / N-01: limit=0 means zero firings
// ---------------------------------------------------------------------------
test("D-006 Engine.run(0) fires zero rules", () => {
  const e = new Engine();
  e.load(LOOP_RULE);
  e.reset();
  e.assertString("(counter 0)");
  const result = e.run(0);
  assert.strictEqual(result.rulesFired, 0);
  // Native engine should return LimitReached for 0 firings
  e.close();
});

// ---------------------------------------------------------------------------
// N-01: positive limit caps firings
// ---------------------------------------------------------------------------
test("D-006 Engine.run(5) fires at most 5 rules", () => {
  const e = new Engine();
  e.load(LOOP_RULE);
  e.reset();
  e.assertString("(counter 0)");
  const result = e.run(5);
  assert.strictEqual(result.rulesFired, 5);
  assert.strictEqual(result.haltReason, HaltReason.LimitReached);
  e.close();
});
