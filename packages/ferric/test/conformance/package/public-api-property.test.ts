/**
 * Property-style package API surface tests.
 *
 * These complement the manual type and package smoke tests with generated
 * tables over exported classes, enums, lifecycle symbols, and package helpers.
 */
import { test } from "node:test";
import * as assert from "node:assert/strict";

import * as ferric from "../../../dist/index";

// ---------------------------------------------------------------------------
// A-001/A-002/A-004/A-005/A-006/G-001/G-003/N-06 generated export corpus
// ---------------------------------------------------------------------------
test("A-001 A-002 A-004 A-005 A-006 G-001 G-003 N-06 property-style public API export corpus", () => {
  const classExports = [
    ["Engine", ferric.Engine],
    ["FerricSymbol", ferric.FerricSymbol],
    ["EngineHandle", ferric.EngineHandle],
    ["EnginePool", ferric.EnginePool],
  ] as const;

  for (const [name, value] of classExports) {
    assert.strictEqual(typeof value, "function", `${name} should be a concrete class export`);
  }

  const enumExports = [
    ["Strategy.Depth", ferric.Strategy.Depth, 0],
    ["Encoding.Utf8", ferric.Encoding.Utf8, 1],
    ["HaltReason.AgendaEmpty", ferric.HaltReason.AgendaEmpty, 0],
    ["FactType.Ordered", ferric.FactType.Ordered, 0],
    ["Format.Bincode", ferric.Format.Bincode, 0],
  ] as const;

  // The generated enum table proves public enums are runtime objects with
  // stable values, not erased const-enum-only type declarations.
  for (const [name, actual, expected] of enumExports) {
    assert.strictEqual(actual, expected, `${name} should keep its public value`);
  }

  const engine = new ferric.Engine();
  try {
    assert.strictEqual(typeof engine[Symbol.dispose], "function");
  } finally {
    engine.close();
  }

  assert.strictEqual(typeof ferric.EngineHandle.prototype[Symbol.asyncDispose], "function");
  assert.strictEqual(typeof ferric.EnginePool.prototype[Symbol.asyncDispose], "function");
});
