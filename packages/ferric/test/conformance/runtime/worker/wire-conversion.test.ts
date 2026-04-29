/**
 * Worker value/wire conversion tests (B-002, B-004).
 *
 * Verifies that FerricSymbol values survive worker boundaries
 * in both directions.
 */
import { test } from "node:test";
import * as assert from "node:assert/strict";

import {
  EngineHandle,
  FerricSymbol,
  HaltReason,
} from "../../../helpers/ferric";

const SYMBOL_RULE = `
(defrule detect-sym
  (color ?c)
  =>
  (printout t "color=" ?c crlf))
`;

const TEMPLATE_RULE = `
(deftemplate person
  (slot name)
  (slot age)
  (slot active))

(defrule greet
  (person (name ?n) (active TRUE))
  =>
  (printout t "hi " ?n crlf))
`;

// ---------------------------------------------------------------------------
// B-002: FerricSymbol input works across worker boundary
// ---------------------------------------------------------------------------
test("B-002 FerricSymbol in ordered fact fields via EngineHandle", async () => {
  const handle = await EngineHandle.create({ source: SYMBOL_RULE });
  try {
    await handle.reset();
    const id = await handle.assertFact("color", new FerricSymbol("red"));
    assert.strictEqual(typeof id, "number");
    const result = await handle.run();
    assert.strictEqual(result.rulesFired, 1);
  } finally {
    await handle.close();
  }
});

test("B-002 FerricSymbol in template slots via EngineHandle", async () => {
  const handle = await EngineHandle.create({ source: TEMPLATE_RULE });
  try {
    await handle.reset();
    const id = await handle.assertTemplate("person", {
      name: new FerricSymbol("Alice"),
      age: 30,
      active: new FerricSymbol("TRUE"),
    });
    assert.strictEqual(typeof id, "number");
    const result = await handle.run();
    assert.strictEqual(result.rulesFired, 1);
  } finally {
    await handle.close();
  }
});

test("B-002 FerricSymbol in nested arrays/multifields via EngineHandle", async () => {
  const handle = await EngineHandle.create({
    source: "(defrule x (data $?d) =>)",
  });
  try {
    await handle.reset();
    const id = await handle.assertFact(
      "data",
      new FerricSymbol("a"),
      new FerricSymbol("b"),
      new FerricSymbol("c"),
    );
    assert.strictEqual(typeof id, "number");
  } finally {
    await handle.close();
  }
});

// ---------------------------------------------------------------------------
// B-004: CLIPS symbols returned via worker are reconstructed as FerricSymbol
// ---------------------------------------------------------------------------
test("B-004 symbol outputs from EngineHandle.facts are FerricSymbol instances", async () => {
  const handle = await EngineHandle.create({
    source: "(deffacts init (color red))",
  });
  try {
    await handle.reset();
    const facts = await handle.facts();
    const colorFact = facts.find(
      (f: any) => f.relation === "color"
    );
    assert.ok(colorFact, "color fact should exist");
    const firstField = (colorFact as any).fields?.[0];
    assert.ok(
      firstField instanceof FerricSymbol,
      `Expected FerricSymbol instance, got ${firstField?.constructor?.name}`
    );
    assert.strictEqual(firstField.value, "red");
  } finally {
    await handle.close();
  }
});

test("B-004 symbol outputs from EngineHandle.getFact are FerricSymbol instances", async () => {
  const handle = await EngineHandle.create();
  try {
    await handle.reset();
    const [factId] = await handle.assertString("(color blue)");
    const fact = await handle.getFact(factId) as any;
    assert.ok(fact);
    // 'blue' should come back as FerricSymbol
    const blueField = fact.fields?.find(
      (f: any) => f instanceof FerricSymbol && f.value === "blue"
    );
    assert.ok(blueField, "blue should be a FerricSymbol instance");
  } finally {
    await handle.close();
  }
});

test("B-004 symbol outputs from EngineHandle.getGlobal are FerricSymbol instances", async () => {
  const handle = await EngineHandle.create({
    source: "(defglobal ?*status* = running)",
  });
  try {
    await handle.reset();
    const val = await handle.getGlobal("status");
    assert.ok(
      val instanceof FerricSymbol,
      `Expected FerricSymbol, got ${typeof val} (${(val as any)?.constructor?.name})`
    );
    assert.strictEqual((val as any).value, "running");
  } finally {
    await handle.close();
  }
});
