/**
 * EngineHandle create validation tests (D-003).
 */
import { test } from "node:test";
import * as assert from "node:assert/strict";

import { Engine, EngineHandle } from "../../../helpers/ferric";

// ---------------------------------------------------------------------------
// D-003: source and snapshot are mutually exclusive
// ---------------------------------------------------------------------------
test("D-003 create with both source and snapshot rejects", async () => {
  // Create a snapshot to use
  const e = new Engine();
  e.load("(defrule x (initial-fact) =>)");
  e.reset();
  const snapshot = e.serialize();
  e.close();

  await assert.rejects(
    () =>
      EngineHandle.create({
        source: "(defrule y =>)",
        snapshot: { data: snapshot },
      }),
    (err: any) => {
      assert.ok(err instanceof TypeError, `Expected TypeError, got ${err.constructor.name}`);
      assert.ok(err.message.includes("mutually exclusive"));
      return true;
    }
  );
});

// ---------------------------------------------------------------------------
// D-003: source alone works
// ---------------------------------------------------------------------------
test("D-003 create with source only succeeds", async () => {
  const handle = await EngineHandle.create({
    source: "(defrule x (initial-fact) =>)",
  });
  await handle.close();
});

// ---------------------------------------------------------------------------
// D-003: snapshot alone works
// ---------------------------------------------------------------------------
test("D-003 create with snapshot only succeeds", async () => {
  const e = new Engine();
  e.load("(defrule x (initial-fact) =>)");
  e.reset();
  const snapshot = e.serialize();
  e.close();

  const handle = await EngineHandle.create({
    snapshot: { data: snapshot },
  });
  await handle.close();
});
