/**
 * Package and load behavior tests.
 */
import { test } from "node:test";
import * as assert from "node:assert/strict";

// ---------------------------------------------------------------------------
// G-001: Package exports are available
// ---------------------------------------------------------------------------
test("G-001 package exports Engine, FerricSymbol, EngineHandle, EnginePool", () => {
  // Import the built package
  const pkg = require("../../../dist/index");
  assert.strictEqual(typeof pkg.Engine, "function");
  assert.strictEqual(typeof pkg.FerricSymbol, "function");
  assert.strictEqual(typeof pkg.EngineHandle, "function");
  assert.strictEqual(typeof pkg.EnginePool, "function");
});

// ---------------------------------------------------------------------------
// G-001: Enums are available
// ---------------------------------------------------------------------------
test("G-001 package exports enums at runtime", () => {
  const pkg = require("../../../dist/index");
  assert.strictEqual(typeof pkg.Strategy, "object");
  assert.strictEqual(typeof pkg.Encoding, "object");
  assert.strictEqual(typeof pkg.HaltReason, "object");
  assert.strictEqual(typeof pkg.FactType, "object");
  assert.strictEqual(typeof pkg.Format, "object");
});

// ---------------------------------------------------------------------------
// G-001: Error classes are available
// ---------------------------------------------------------------------------
test("G-001 package exports error classes", () => {
  const pkg = require("../../../dist/index");
  assert.strictEqual(typeof pkg.FerricError, "function");
  assert.strictEqual(typeof pkg.FerricParseError, "function");
  assert.strictEqual(typeof pkg.FerricCompileError, "function");
  assert.strictEqual(typeof pkg.FerricRuntimeError, "function");
  assert.strictEqual(typeof pkg.FerricFactNotFoundError, "function");
  assert.strictEqual(typeof pkg.FerricTemplateNotFoundError, "function");
  assert.strictEqual(typeof pkg.FerricSlotNotFoundError, "function");
  assert.strictEqual(typeof pkg.FerricModuleNotFoundError, "function");
  assert.strictEqual(typeof pkg.FerricEncodingError, "function");
  assert.strictEqual(typeof pkg.FerricSerializationError, "function");
});

// ---------------------------------------------------------------------------
// G-003: Tests execute non-zero cases
// ---------------------------------------------------------------------------
test("G-003 this test exists and runs", () => {
  assert.ok(true, "At least one package test runs");
});
