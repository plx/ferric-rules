/**
 * Snapshot round-trip tests (D-002 analogue, serialization).
 *
 * Engine.serialize() is the sync serialization path. The resulting Buffer is
 * consumed by EngineHandle.create({ snapshot: { data } }) to restore state
 * across the worker boundary — this is the canonical round-trip used in
 * production.
 */
import { test } from "node:test";
import * as assert from "node:assert/strict";

import {
  Engine,
  EngineHandle,
  HaltReason,
} from "../../../helpers/ferric";

const BASIC_RULE = `
(defrule hello-world
  (initial-fact)
  =>
  (printout t "hello from snapshot" crlf))
`;

// ---------------------------------------------------------------------------
// D-002 / serialization: sync Engine.serialize() produces a non-empty Buffer
// ---------------------------------------------------------------------------
test("D-002 Engine.serialize() returns a non-empty Buffer", () => {
  const src = new Engine();
  src.load(BASIC_RULE);
  src.reset();
  const snap = src.serialize();
  src.close();

  assert.ok(Buffer.isBuffer(snap), "serialize() should return a Buffer");
  assert.ok(snap.length > 0, "snapshot buffer must be non-empty");
});

// ---------------------------------------------------------------------------
// D-002: snapshot produced by sync Engine is consumable by EngineHandle.create
// ---------------------------------------------------------------------------
test("D-002 Engine.serialize() snapshot round-trips through EngineHandle", async () => {
  // Serialize from the sync engine (post-reset so initial-fact is in the WM).
  const src = new Engine();
  src.load(BASIC_RULE);
  src.reset();
  const snap = src.serialize();
  src.close();

  // Restore into a worker-backed handle and run.
  const handle = await EngineHandle.create({ snapshot: { data: snap } });
  try {
    const result = await handle.run();
    assert.strictEqual(result.rulesFired, 1);
    assert.strictEqual(result.haltReason, HaltReason.AgendaEmpty);
    const output = await handle.getOutput("t");
    assert.ok(
      output?.includes("hello from snapshot"),
      `Expected snapshot output, got: ${output}`
    );
  } finally {
    await handle.close();
  }
});
