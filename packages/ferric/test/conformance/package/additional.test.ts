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
// G-001 table-driven registry: every exported error factory is callable
// ---------------------------------------------------------------------------
test("G-001 table-driven ERROR_REGISTRY factories construct named Ferric errors", async () => {
  const { ERROR_REGISTRY } = await import("../../../dist/index");
  assert.ok(typeof ERROR_REGISTRY === "object" && ERROR_REGISTRY !== null, "ERROR_REGISTRY should be an object");

  // This table proves the registry is not just present: every
  // exported factory returns the documented class name and stable error code.
  const required = [
    ["FerricError", "FERRIC_ERROR"],
    ["FerricParseError", "FERRIC_PARSE_ERROR"],
    ["FerricCompileError", "FERRIC_COMPILE_ERROR"],
    ["FerricRuntimeError", "FERRIC_RUNTIME_ERROR"],
    ["FerricFactNotFoundError", "FERRIC_FACT_NOT_FOUND"],
    ["FerricTemplateNotFoundError", "FERRIC_TEMPLATE_NOT_FOUND"],
    ["FerricSlotNotFoundError", "FERRIC_SLOT_NOT_FOUND"],
    ["FerricModuleNotFoundError", "FERRIC_MODULE_NOT_FOUND"],
    ["FerricEncodingError", "FERRIC_ENCODING_ERROR"],
    ["FerricSerializationError", "FERRIC_SERIALIZATION_ERROR"],
  ] as const;
  for (const [key, code] of required) {
    assert.ok(
      key in ERROR_REGISTRY,
      `ERROR_REGISTRY should contain key "${key}"`
    );
    assert.strictEqual(
      typeof ERROR_REGISTRY[key],
      "function",
      `ERROR_REGISTRY["${key}"] should be a constructor`
    );
    const err = ERROR_REGISTRY[key]("registry probe");
    assert.ok(err instanceof Error);
    assert.strictEqual(err.name, key);
    assert.strictEqual(err.code, code);
    assert.strictEqual(err.message, "registry probe");
  }

  // The registry must expose exactly the documented factories — no stray or
  // mis-named extra, which the per-key loop above would not catch.
  assert.strictEqual(Object.keys(ERROR_REGISTRY).length, required.length);
});

// ---------------------------------------------------------------------------
// C-003: convertNativeError is the production consumer of ERROR_REGISTRY
// ---------------------------------------------------------------------------
test("C-003 convertNativeError extracts the class prefix and rebuilds via ERROR_REGISTRY", async () => {
  // convertNativeError is an internal helper (not re-exported from the barrel),
  // so import it from the types module where it is defined.
  const { convertNativeError, FerricParseError } = await import("../../../dist/types");

  // napi surfaces errors as plain Error objects whose message is prefixed with
  // the Ferric class name ("FerricParseError: ..."). This is the production path
  // ERROR_REGISTRY exists to serve: convertNativeError must strip the prefix and
  // reconstruct the correct subclass with a cleaned message and stable code.
  const converted = convertNativeError(new Error("FerricParseError: unexpected token"));
  assert.ok(converted instanceof FerricParseError);
  assert.strictEqual((converted as any).name, "FerricParseError");
  assert.strictEqual((converted as any).code, "FERRIC_PARSE_ERROR");
  assert.strictEqual(converted.message, "unexpected token");

  // An Error without a recognized Ferric prefix falls through unchanged.
  const passthrough = convertNativeError(new TypeError("not a ferric error"));
  assert.ok(passthrough instanceof TypeError);
  assert.strictEqual(passthrough.message, "not a ferric error");

  // A non-Error input is wrapped in a generic Error rather than thrown.
  const wrapped = convertNativeError("raw string failure");
  assert.ok(wrapped instanceof Error);
  assert.strictEqual(wrapped.message, "raw string failure");
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
