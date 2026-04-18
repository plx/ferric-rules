/**
 * Sync Engine runtime smoke tests.
 */
import { test } from "node:test";
import * as assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { resolve } from "node:path";

import {
  Engine,
  FerricSymbol,
  HaltReason,
} from "../../../helpers/ferric";

const FIXTURES = resolve(__dirname, "../../../fixtures");

function loadFixture(name: string): string {
  return readFileSync(resolve(FIXTURES, name), "utf-8");
}

// ---------------------------------------------------------------------------
// B-001: FerricSymbol works in sync Engine
// ---------------------------------------------------------------------------
test("fact id with fractional part is rejected", () => {
  const e = new Engine();
  e.reset();
  assert.throws(
    () => e.retract(1.5),
    /fact id must be a finite non-negative integer/,
  );
  assert.throws(
    () => e.getFact(Number.NaN),
    /fact id must be a finite non-negative integer/,
  );
  assert.throws(
    () => e.retract(-1),
    /fact id must be a finite non-negative integer/,
  );
  e.close();
});

test("B-001 FerricSymbol input works in sync assertFact", () => {
  const e = new Engine();
  e.load("(defrule sym-test (color ?c) =>)");
  e.reset();
  const id = e.assertFact("color", new FerricSymbol("red"));
  assert.ok(typeof id === "number");
  e.close();
});

// ---------------------------------------------------------------------------
// B-003: CLIPS symbols returned as FerricSymbol
// ---------------------------------------------------------------------------
test("B-003 CLIPS symbols returned via sync Engine are FerricSymbol", () => {
  const e = new Engine();
  e.load("(deffacts init (color red))");
  e.reset();
  const facts = e.facts() as any[];
  const colorFact = facts.find(
    (f: any) => f.relation === "color" || f.templateName === "color"
  );
  assert.ok(colorFact, "color fact should exist");
  // The symbol 'red' should come back as FerricSymbol
  const firstField = colorFact.fields?.[0];
  assert.ok(
    firstField && typeof firstField === "object" && firstField.constructor?.name === "FerricSymbol",
    `Expected FerricSymbol, got ${typeof firstField} (${firstField?.constructor?.name})`
  );
  e.close();
});

// ---------------------------------------------------------------------------
// B-005: string maps to CLIPS string, not symbol
// ---------------------------------------------------------------------------
test("B-005 string maps to CLIPS string, not symbol", () => {
  const e = new Engine();
  e.load(loadFixture("symbol-string-discrimination.clp"));
  e.reset();
  // Assert with symbol red and string "red"
  e.assertTemplate("color-info", {
    "color-sym": new FerricSymbol("red"),
    "color-str": "red",
  });
  const result = e.run();
  const output = e.getOutput("t");
  assert.ok(output?.includes("symbol-match"), "Symbol rule should fire");
  assert.ok(output?.includes("string-match"), "String rule should fire");
  e.close();
});

// ---------------------------------------------------------------------------
// B-008: assertString returns all asserted fact IDs
// ---------------------------------------------------------------------------
test("B-008 assertString returns all asserted fact IDs", () => {
  const e = new Engine();
  e.reset();
  const ids = e.assertString("(a 1)(b 2)(c 3)");
  assert.strictEqual(ids.length, 3);
  for (const id of ids) {
    assert.strictEqual(typeof id, "number");
  }
  e.close();
});

// ---------------------------------------------------------------------------
// B-009: Fact shape
// ---------------------------------------------------------------------------
test("B-009 fact shape conforms: ordered facts have relation+fields", () => {
  const e = new Engine();
  e.reset();
  const [id] = e.assertString("(color red blue)");
  const fact = e.getFact(id) as any;
  assert.ok(fact);
  assert.strictEqual(typeof fact.id, "number");
  assert.ok(Array.isArray(fact.fields));
  e.close();
});

// ---------------------------------------------------------------------------
// Basic run test
// ---------------------------------------------------------------------------
test("G-003 basic engine run smoke test", () => {
  const e = new Engine();
  e.load(loadFixture("basic-rule.clp"));
  e.reset();
  const result = e.run();
  assert.strictEqual(result.rulesFired, 1);
  assert.strictEqual(result.haltReason, HaltReason.AgendaEmpty);
  const output = e.getOutput("t");
  assert.ok(output?.includes("hello from ferric"));
  e.close();
});
