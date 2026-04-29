/**
 * Additional EngineHandle (worker-backed) tests.
 *
 * Covers: D-001, D-002, D-005, D-007, B-002, B-004
 */
import { test } from "node:test";
import * as assert from "node:assert/strict";

import {
  Engine,
  EngineHandle,
  FerricSymbol,
  HaltReason,
} from "../../../helpers/ferric";

const BASIC_RULE = `
(defrule hello-world
  (initial-fact)
  =>
  (printout t "hello from ferric" crlf))
`;

const LOOP_RULE = `
(defrule loop
  ?f <- (counter ?n)
  =>
  (retract ?f)
  (assert (counter (+ ?n 1))))
`;

const DEFFACTS_RULE = `
(deffacts colors (color red) (color blue))
(defrule count-colors
  (color ?c)
  =>
  (printout t "saw=" ?c crlf))
`;

// ---------------------------------------------------------------------------
// D-001 variant: create with empty options (no source, no snapshot)
// ---------------------------------------------------------------------------
test("D-001 EngineHandle.create() with no arguments succeeds", async () => {
  const handle = await EngineHandle.create();
  try {
    await handle.reset();
    // Should be a clean engine — no rules, no facts beyond initial-fact.
    const result = await handle.run();
    assert.strictEqual(result.rulesFired, 0);
    assert.strictEqual(result.haltReason, HaltReason.AgendaEmpty);
  } finally {
    await handle.close();
  }
});

test("D-001 EngineHandle.create({}) with explicit empty options succeeds", async () => {
  const handle = await EngineHandle.create({});
  try {
    await handle.reset();
    const result = await handle.run();
    assert.strictEqual(result.rulesFired, 0);
    assert.strictEqual(result.haltReason, HaltReason.AgendaEmpty);
  } finally {
    await handle.close();
  }
});

// ---------------------------------------------------------------------------
// D-002: snapshot restore via worker — rules survive the worker boundary
// ---------------------------------------------------------------------------
test("D-002 EngineHandle.create with snapshot restores rules", async () => {
  // Serialize from a sync Engine.
  const src = new Engine();
  src.load(BASIC_RULE);
  src.reset();
  const snap = src.serialize();
  src.close();

  // Restore into a worker-backed handle.
  const handle = await EngineHandle.create({ snapshot: { data: snap } });
  try {
    // The snapshot already includes a reset state; run directly.
    const result = await handle.run();
    assert.strictEqual(result.rulesFired, 1);
    assert.strictEqual(result.haltReason, HaltReason.AgendaEmpty);
    const output = await handle.getOutput("t");
    assert.ok(
      output?.includes("hello from ferric"),
      `Expected rule output, got: ${output}`
    );
  } finally {
    await handle.close();
  }
});

test("D-002 EngineHandle snapshot restore preserves deffacts on reset", async () => {
  // Serialize an engine with deffacts but WITHOUT resetting after load.
  const src = new Engine();
  src.load(DEFFACTS_RULE);
  // Serialize the loaded-but-not-reset state.
  const snap = src.serialize();
  src.close();

  const handle = await EngineHandle.create({ snapshot: { data: snap } });
  try {
    // Reset inside the handle — deffacts should fire.
    await handle.reset();
    const result = await handle.run();
    // Two colors → two rule firings.
    assert.ok(result.rulesFired >= 2, `Expected >=2 firings, got ${result.rulesFired}`);
  } finally {
    await handle.close();
  }
});

// ---------------------------------------------------------------------------
// D-005: in-flight cancellation returns partial result with HaltRequested
// ---------------------------------------------------------------------------
test("D-005 EngineHandle.run with in-flight abort signal returns HaltRequested", async () => {
  const handle = await EngineHandle.create({ source: LOOP_RULE });
  try {
    await handle.reset();
    await handle.assertString("(counter 0)");

    const ac = new AbortController();
    // Abort after a short delay to let the rule loop start.
    setTimeout(() => ac.abort(), 20);

    const result = await handle.run({ signal: ac.signal });

    // The run should have been interrupted and return a partial result.
    assert.ok(result.rulesFired > 0, "Should have fired at least one rule before abort");
    assert.strictEqual(
      result.haltReason,
      HaltReason.HaltRequested,
      `Expected HaltRequested, got ${result.haltReason}`
    );
  } finally {
    await handle.close();
  }
});

// ---------------------------------------------------------------------------
// D-007: buffer snapshot transfer across worker boundary
// ---------------------------------------------------------------------------
// D-007: A Buffer produced by the sync Engine can be transferred to a new
// worker via EngineHandle.create({ snapshot: { data } }), crossing the
// thread boundary without modification.
test("D-007 sync Engine Buffer snapshot transfers to EngineHandle across worker boundary", async () => {
  // Step 1: produce a snapshot on the main thread using the sync engine.
  const src = new Engine();
  src.load(BASIC_RULE);
  src.reset();
  const snap = src.serialize();
  src.close();

  assert.ok(Buffer.isBuffer(snap), "sync Engine.serialize() must return a Buffer");
  assert.ok(snap.length > 0, "snapshot must be non-empty before transfer");

  // Step 2: transfer the Buffer to a new worker thread via EngineHandle.create.
  const handle = await EngineHandle.create({ snapshot: { data: snap } });
  try {
    const result = await handle.run();
    assert.strictEqual(result.rulesFired, 1);
    assert.strictEqual(result.haltReason, HaltReason.AgendaEmpty);
  } finally {
    await handle.close();
  }
});

test("D-007 transferred snapshot preserves rule firings on restored handle", async () => {
  const RULE_SRC = `
(deffacts colors (color red) (color blue) (color green))
(defrule count-color (color ?c) => (printout t ?c crlf))
`;
  const src = new Engine();
  src.load(RULE_SRC);
  // Serialize the loaded-but-not-yet-reset engine — deffacts will replay on reset.
  const snap = src.serialize();
  src.close();

  const handle = await EngineHandle.create({ snapshot: { data: snap } });
  try {
    await handle.reset();
    const result = await handle.run();
    // Three color facts → three rule firings.
    assert.strictEqual(result.rulesFired, 3);
    assert.strictEqual(result.haltReason, HaltReason.AgendaEmpty);
  } finally {
    await handle.close();
  }
});

// ---------------------------------------------------------------------------
// B-002 deeper coverage: numeric values cross worker boundary
// ---------------------------------------------------------------------------
test("B-002 integer and float values survive worker boundary in ordered facts", async () => {
  const handle = await EngineHandle.create({
    source: "(defrule nums (data ?n) => (printout t \"n=\" ?n crlf))",
  });
  try {
    await handle.reset();
    await handle.assertFact("data", 42);
    await handle.assertFact("data", 3.14);
    const result = await handle.run();
    assert.ok(result.rulesFired >= 2, `Expected >=2 firings, got ${result.rulesFired}`);
    const output = await handle.getOutput("t");
    assert.ok(output?.includes("n=42"), `Missing n=42 in output: ${output}`);
  } finally {
    await handle.close();
  }
});

// ---------------------------------------------------------------------------
// B-004 deeper coverage: findFacts returns FerricSymbol instances
// ---------------------------------------------------------------------------
test("B-004 EngineHandle.getOutput returns string after rule fires", async () => {
  const handle = await EngineHandle.create({
    source: '(defrule greet (initial-fact) => (printout t "greetings" crlf))',
  });
  try {
    await handle.reset();
    await handle.run();
    const output = await handle.getOutput("t");
    assert.ok(output?.includes("greetings"), `Expected greetings, got: ${output}`);
    // Clear and verify it's gone
    await handle.clearOutput("t");
    const after = await handle.getOutput("t");
    assert.strictEqual(after, null);
  } finally {
    await handle.close();
  }
});

test("B-004 EngineHandle.findFacts returns FerricSymbol instances in matching facts", async () => {
  const handle = await EngineHandle.create({
    source: "(deffacts init (shape circle) (shape square))",
  });
  try {
    await handle.reset();
    const results = await handle.findFacts("shape") as any[];
    assert.ok(Array.isArray(results));
    assert.ok(results.length >= 2, `Expected >=2 shape facts, got ${results.length}`);
    for (const f of results) {
      const field = f.fields?.[0];
      assert.ok(
        field instanceof FerricSymbol,
        `Expected FerricSymbol, got ${field?.constructor?.name}`
      );
    }
  } finally {
    await handle.close();
  }
});
