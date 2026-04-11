/**
 * Engine lifecycle and closed-state behavior tests (F-001, F-002).
 */
import { test } from "node:test";
import * as assert from "node:assert/strict";

import { Engine } from "../../../helpers/ferric";

// ---------------------------------------------------------------------------
// F-001: Engine.close() is idempotent
// ---------------------------------------------------------------------------
test("F-001 Engine.close() is idempotent", () => {
  const e = new Engine();
  e.close();
  e.close(); // should not throw
  e.close(); // should not throw
});

// ---------------------------------------------------------------------------
// F-002: After close(), operational methods throw
// ---------------------------------------------------------------------------
test("F-002 Engine.load() throws after close", () => {
  const e = new Engine();
  e.close();
  assert.throws(() => e.load("(defrule x =>)"), /closed|destroyed/i);
});

test("F-002 Engine.reset() throws after close", () => {
  const e = new Engine();
  e.close();
  assert.throws(() => e.reset(), /closed|destroyed/i);
});

test("F-002 Engine.run() throws after close", () => {
  const e = new Engine();
  e.close();
  assert.throws(() => e.run(), /closed|destroyed/i);
});

test("F-002 Engine.assertString() throws after close", () => {
  const e = new Engine();
  e.close();
  assert.throws(() => e.assertString("(a 1)"), /closed|destroyed/i);
});

test("F-002 Engine.facts() throws after close", () => {
  const e = new Engine();
  e.close();
  assert.throws(() => e.facts(), /closed|destroyed/i);
});
