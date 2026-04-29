/**
 * Integer boundary tests (B-007).
 *
 * Integers in safe range [-(2^53-1), 2^53-1] -> number.
 * Integers outside safe range -> bigint.
 */
import { test } from "node:test";
import * as assert from "node:assert/strict";

import { Engine, FerricSymbol } from "../../../helpers/ferric";

// ---------------------------------------------------------------------------
// B-007: Safe integers come back as number
// ---------------------------------------------------------------------------
test("B-007 safe integer returns as number", () => {
  const e = new Engine();
  e.reset();
  e.assertString("(val 42)");
  const facts = e.facts() as any[];
  const valFact = facts.find((f: any) => f.relation === "val");
  assert.ok(valFact, "val fact should exist");
  const field = valFact.fields[0];
  assert.strictEqual(typeof field, "number");
  assert.strictEqual(field, 42);
  e.close();
});

test("B-007 max safe integer returns as number", () => {
  const e = new Engine();
  e.reset();
  // 2^53 - 1 = 9007199254740991
  e.assertString("(val 9007199254740991)");
  const facts = e.facts() as any[];
  const valFact = facts.find((f: any) => f.relation === "val");
  assert.ok(valFact);
  const field = valFact.fields[0];
  assert.strictEqual(typeof field, "number");
  assert.strictEqual(field, Number.MAX_SAFE_INTEGER);
  e.close();
});

test("B-007 fact ids above Number.MAX_SAFE_INTEGER are rejected", () => {
  const e = new Engine();
  e.reset();
  assert.throws(
    () => e.getFact(2 ** 53),
    /fact id must be a finite non-negative integer/,
  );
  e.close();
});

test("B-007 negative safe integer returns as number", () => {
  const e = new Engine();
  e.reset();
  e.assertString("(val -42)");
  const facts = e.facts() as any[];
  const valFact = facts.find((f: any) => f.relation === "val");
  assert.ok(valFact);
  assert.strictEqual(typeof valFact.fields[0], "number");
  assert.strictEqual(valFact.fields[0], -42);
  e.close();
});

// ---------------------------------------------------------------------------
// B-007: bigint input works
// ---------------------------------------------------------------------------
test("B-007 bigint input is accepted", () => {
  const e = new Engine();
  e.reset();
  const id = e.assertFact("big", BigInt("9007199254740992"));
  assert.strictEqual(typeof id, "number");
  e.close();
});

test("B-007 bigint outside i64 range is rejected", () => {
  const e = new Engine();
  e.reset();
  // 2n ** 63n is the first positive value that overflows signed 64-bit.
  assert.throws(
    () => e.assertFact("big", 2n ** 63n),
    /BigInt value is outside the signed 64-bit integer range/,
  );
  // -(2n ** 63n) - 1n is the first negative value that overflows signed 64-bit.
  assert.throws(
    () => e.assertFact("big", -(2n ** 63n) - 1n),
    /BigInt value is outside the signed 64-bit integer range/,
  );
  e.close();
});

test("B-007 whole numbers at 2^63 stay floats instead of saturating", () => {
  const e = new Engine();
  e.reset();
  e.assertFact("big", 2 ** 63);
  const [fact] = e.findFacts("big") as any[];
  assert.ok(fact, "big fact should exist");
  assert.strictEqual(typeof fact.fields[0], "number");
  assert.strictEqual(fact.fields[0], 2 ** 63);
  e.close();
});

// ---------------------------------------------------------------------------
// B-006: boolean maps to CLIPS symbols TRUE/FALSE
// ---------------------------------------------------------------------------
test("B-006 boolean maps to TRUE/FALSE symbols", () => {
  const e = new Engine();
  e.reset();
  e.assertFact("flag", true);
  e.assertFact("flag", false);
  const facts = e.facts() as any[];
  const flagFacts = facts.filter((f: any) => f.relation === "flag");
  assert.strictEqual(flagFacts.length, 2);

  // TRUE and FALSE should come back as FerricSymbol instances
  for (const f of flagFacts) {
    const val = f.fields[0];
    assert.ok(
      val instanceof FerricSymbol,
      `Expected FerricSymbol, got ${typeof val}`
    );
    assert.ok(
      val.value === "TRUE" || val.value === "FALSE",
      `Expected TRUE or FALSE, got ${val.value}`
    );
  }
  e.close();
});
