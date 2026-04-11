/**
 * Additional package-level conformance tests.
 *
 * Covers: G-004, G-001 (wire utilities, ERROR_REGISTRY, import patterns).
 */
import { test } from "node:test";
import * as assert from "node:assert/strict";

// ---------------------------------------------------------------------------
// G-001: Named import pattern works alongside require()
// ---------------------------------------------------------------------------
test("G-001 named ESM-style import from dist/index succeeds", async () => {
  const mod = await import("../../../dist/index");
  assert.strictEqual(typeof mod.Engine, "function", "Engine should be importable");
  assert.strictEqual(typeof mod.FerricSymbol, "function", "FerricSymbol should be importable");
  assert.strictEqual(typeof mod.EngineHandle, "function", "EngineHandle should be importable");
  assert.strictEqual(typeof mod.EnginePool, "function", "EnginePool should be importable");
});

// ---------------------------------------------------------------------------
// G-001: Wire utility exports are present and callable
// ---------------------------------------------------------------------------
test("G-001 wire utilities toWire, fromWire, isWireSymbol are exported", async () => {
  const mod = await import("../../../dist/index");
  assert.strictEqual(typeof mod.toWire, "function", "toWire should be a function");
  assert.strictEqual(typeof mod.fromWire, "function", "fromWire should be a function");
  assert.strictEqual(typeof mod.isWireSymbol, "function", "isWireSymbol should be a function");
});

// ---------------------------------------------------------------------------
// G-001: ERROR_REGISTRY is exported and maps known error names
// ---------------------------------------------------------------------------
test("G-001 ERROR_REGISTRY is exported and maps known Ferric error names", async () => {
  const { ERROR_REGISTRY } = await import("../../../dist/index");
  assert.ok(typeof ERROR_REGISTRY === "object" && ERROR_REGISTRY !== null, "ERROR_REGISTRY should be an object");

  // Each of the standard error class names must appear as a key.
  const requiredKeys = [
    "FerricParseError",
    "FerricCompileError",
    "FerricRuntimeError",
    "FerricFactNotFoundError",
    "FerricTemplateNotFoundError",
    "FerricSlotNotFoundError",
    "FerricModuleNotFoundError",
    "FerricEncodingError",
    "FerricSerializationError",
  ];
  for (const key of requiredKeys) {
    assert.ok(
      key in ERROR_REGISTRY,
      `ERROR_REGISTRY should contain key "${key}"`
    );
    assert.strictEqual(
      typeof ERROR_REGISTRY[key],
      "function",
      `ERROR_REGISTRY["${key}"] should be a constructor`
    );
  }
});

// ---------------------------------------------------------------------------
// G-004: Wire round-trip: toWire/fromWire preserves FerricSymbol identity
// ---------------------------------------------------------------------------
test("G-004 toWire/fromWire round-trip preserves FerricSymbol identity", async () => {
  const { FerricSymbol, toWire, fromWire, isWireSymbol } = await import("../../../dist/index");

  const sym = new FerricSymbol("hello");
  const wire = toWire(sym);

  // isWireSymbol should identify the wire representation.
  assert.ok(isWireSymbol(wire), "isWireSymbol() should return true for a wired symbol");

  // fromWire reconstructs FerricSymbol when passed the constructor.
  const restored = fromWire(wire, FerricSymbol as any);
  assert.ok(
    restored instanceof FerricSymbol,
    `fromWire with FerricSymbol ctor should return FerricSymbol, got ${(restored as any)?.constructor?.name}`
  );
  assert.strictEqual((restored as any).value, "hello");

  // Without the constructor, wire symbols stay as plain wire objects.
  const asPlain = fromWire(wire);
  assert.ok(isWireSymbol(asPlain), "without ctor, fromWire returns the wire object unchanged");
});

// ---------------------------------------------------------------------------
// G-004: toWire/fromWire preserves primitive JS values
// ---------------------------------------------------------------------------
test("G-004 toWire/fromWire round-trip preserves JS primitive values", async () => {
  const { toWire, fromWire } = await import("../../../dist/index");

  // Numbers pass through unchanged.
  const n = toWire(42);
  assert.strictEqual(fromWire(n), 42);

  // Strings pass through unchanged.
  const s = toWire("hello world");
  assert.strictEqual(fromWire(s), "hello world");

  // null/undefined both map to null in the wire format.
  assert.strictEqual(fromWire(toWire(null)), null);
  assert.strictEqual(fromWire(toWire(undefined)), null);

  // Booleans pass through as booleans in the wire format
  // (boolean→FerricSymbol conversion happens in the native addon, not toWire/fromWire).
  const t = fromWire(toWire(true));
  assert.strictEqual(t, true, "booleans pass through toWire/fromWire unchanged");

  const f = fromWire(toWire(false));
  assert.strictEqual(f, false, "booleans pass through toWire/fromWire unchanged");
});
