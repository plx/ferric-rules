/**
 * Explicit resource management tests (A-005).
 */
import { test } from "node:test";
import * as assert from "node:assert/strict";

import { Engine } from "../../../helpers/ferric";

// ---------------------------------------------------------------------------
// A-005: Engine supports Symbol.dispose
// ---------------------------------------------------------------------------
test("A-005 Engine has Symbol.dispose method", () => {
  const e = new Engine();
  assert.strictEqual(typeof e[Symbol.dispose], "function");
  e.close();
});

test("A-005 Engine Symbol.dispose delegates to close", () => {
  const e = new Engine();
  e[Symbol.dispose]();
  // After dispose, operations should throw.
  assert.throws(
    () => e.reset(),
    /closed|destroyed/i
  );
});

test("A-005 Engine Symbol.dispose is idempotent", () => {
  const e = new Engine();
  e[Symbol.dispose]();
  e[Symbol.dispose](); // should not throw
});
