/**
 * Wire helper property-style tests.
 *
 * These complement the explicit package smoke tests by generating nested value
 * shapes and checking recursive conversion invariants.
 */
import { test } from "node:test";
import * as assert from "node:assert/strict";

import {
  FerricSymbol,
  fromWire,
  isWireSymbol,
  toWire,
} from "../../../dist/index";
import {
  extractFerricError,
  fromWireToNative,
} from "../../../dist/wire";

// ---------------------------------------------------------------------------
// G-004 manual wire helpers: malformed symbols are not accepted
// ---------------------------------------------------------------------------
test("G-004 malformed wire-symbol-like objects are not treated as symbols", () => {
  // The guard must be strict so arbitrary user objects with a __type property
  // do not get silently converted into native FerricSymbol instances.
  const malformed = [
    { __type: "FerricSymbol" },
    { __type: "FerricSymbol", value: 1 },
    { __type: "Other", value: "x" },
    null,
    "symbol",
  ];

  for (const value of malformed) {
    assert.strictEqual(isWireSymbol(value), false);
  }
});

// ---------------------------------------------------------------------------
// G-004 manual wire helper: existing wire symbols pass through unchanged
// ---------------------------------------------------------------------------
test("G-004 toWire returns existing wire symbols unchanged", () => {
  const wire = { __type: "FerricSymbol", value: "already-wired" } as const;
  assert.strictEqual(toWire(wire), wire);
});

// ---------------------------------------------------------------------------
// G-004 property-style wire round-trips preserve nested generated values
// ---------------------------------------------------------------------------
test("G-004 property-style toWire/fromWire preserve generated nested shapes", () => {
  const nullPrototypeRecord = Object.create(null) as Record<string, unknown>;
  nullPrototypeRecord.outer = new FerricSymbol("dictionary-symbol");

  const generated = [
    null,
    undefined,
    true,
    false,
    42,
    3.5,
    10n,
    "plain string",
    new FerricSymbol("red"),
    [new FerricSymbol("blue"), "text", [1, new FerricSymbol("green")]],
    {
      outer: new FerricSymbol("outer"),
      list: [new FerricSymbol("inner"), { deep: new FerricSymbol("deep") }],
    },
    nullPrototypeRecord,
  ];

  // This deterministic corpus acts like a small generator over every supported
  // wire primitive and nesting form. We assert that round-tripping reproduces
  // the ORIGINAL value structurally: primitives compare by value, `undefined`
  // normalizes to `null` (toWire collapses undefined), and FerricSymbols
  // rehydrate to instances with the same `.value` — including symbols nested
  // inside arrays and objects. Comparing against the input (rather than a second
  // `fromWire()` call) is what gives this test power to catch a toWire/fromWire
  // bug such as dropping recursion into nested collections.
  const assertRoundTrip = (original: unknown, restored: unknown, path: string): void => {
    if (original === undefined || original === null) {
      assert.strictEqual(restored, null, `expected null at ${path}`);
    } else if (original instanceof FerricSymbol) {
      assert.ok(restored instanceof FerricSymbol, `expected FerricSymbol at ${path}`);
      assert.strictEqual((restored as any).value, original.value, `symbol value at ${path}`);
    } else if (Array.isArray(original)) {
      assert.ok(Array.isArray(restored), `expected array at ${path}`);
      assert.strictEqual((restored as unknown[]).length, original.length, `array length at ${path}`);
      original.forEach((item, i) =>
        assertRoundTrip(item, (restored as unknown[])[i], `${path}[${i}]`),
      );
    } else if (typeof original === "object") {
      assert.ok(
        restored !== null && typeof restored === "object" && !Array.isArray(restored),
        `expected plain object at ${path}`,
      );
      const originalKeys = Object.keys(original as Record<string, unknown>).sort();
      assert.deepStrictEqual(
        Object.keys(restored as object).sort(),
        originalKeys,
        `object keys at ${path}`,
      );
      for (const key of originalKeys) {
        assertRoundTrip(
          (original as Record<string, unknown>)[key],
          (restored as Record<string, unknown>)[key],
          `${path}.${key}`,
        );
      }
    } else {
      // Primitives (string/number/bigint/boolean) must survive unchanged.
      assert.strictEqual(restored, original, `primitive at ${path}`);
    }
  };

  for (const value of generated) {
    const restored = fromWire(toWire(value), FerricSymbol as any);
    assertRoundTrip(value, restored, "root");
  }
});

// ---------------------------------------------------------------------------
// G-004 property-style wire edge corpus covers absent constructors and records
// ---------------------------------------------------------------------------
test("G-004 property-style wire helpers preserve generated edge values", () => {
  const wireSymbol = { __type: "FerricSymbol", value: "plain" };
  const cases = [
    {
      label: "fromWire without constructor leaves symbols tagged",
      value: { nested: [wireSymbol] },
      exercise: () => fromWire({ nested: [wireSymbol] }),
      verify: (value: any) => assert.deepStrictEqual(value, { nested: [wireSymbol] }),
    },
    {
      label: "fromWireToNative preserves null",
      value: null,
      exercise: () => fromWireToNative(null, FerricSymbol as any),
      verify: (value: unknown) => assert.strictEqual(value, null),
    },
    {
      label: "fromWireToNative preserves primitives",
      value: 12,
      exercise: () => fromWireToNative(12, FerricSymbol as any),
      verify: (value: unknown) => assert.strictEqual(value, 12),
    },
    {
      label: "fromWireToNative preserves arrays while rehydrating symbols",
      value: [wireSymbol, "text"],
      exercise: () => fromWireToNative([wireSymbol, "text"], FerricSymbol as any),
      verify: (value: any[]) => {
        assert.ok(value[0] instanceof FerricSymbol);
        assert.strictEqual(value[1], "text");
      },
    },
  ];

  // This table is deliberately deterministic: each case represents one branch
  // in the recursive wire contract, and every branch must preserve shape.
  for (const item of cases) {
    assert.doesNotThrow(
      () => item.verify(item.exercise() as never),
      item.label,
    );
  }
});

// ---------------------------------------------------------------------------
// B-002 property-style fromWireToNative rehydrates generated wire symbols
// ---------------------------------------------------------------------------
test("B-002 property-style fromWireToNative rehydrates generated symbols", () => {
  const generated = {
    one: { __type: "FerricSymbol", value: "one" },
    many: [
      { __type: "FerricSymbol", value: "two" },
      { nested: { __type: "FerricSymbol", value: "three" } },
    ],
  };

  const restored = fromWireToNative(generated, FerricSymbol as any) as any;
  assert.ok(restored.one instanceof FerricSymbol);
  assert.ok(restored.many[0] instanceof FerricSymbol);
  assert.ok(restored.many[1].nested instanceof FerricSymbol);
});

// ---------------------------------------------------------------------------
// C-004 property-style error extraction preserves names/codes/messages
// ---------------------------------------------------------------------------
test("C-004 property-style extractFerricError handles prefixed and generic errors", () => {
  const prefixed = [
    ["FerricParseError: bad syntax", "FerricParseError", "bad syntax", "FERRIC_PARSE_ERROR"],
    ["FerricEncodingError: ascii only", "FerricEncodingError", "ascii only", "FERRIC_ENCODING_ERROR"],
    ["FerricImaginaryError: future", "FerricImaginaryError", "future", "FERRIC_ERROR"],
  ];

  for (const [message, name, clean, code] of prefixed) {
    assert.deepStrictEqual(extractFerricError("Error", message), {
      name,
      message: clean,
      code,
    });
  }

  assert.deepStrictEqual(
    extractFerricError("TypeError", "plain message", "ERR_PLAIN"),
    { name: "TypeError", message: "plain message", code: "ERR_PLAIN" },
  );
  assert.deepStrictEqual(
    extractFerricError("Error", "plain message"),
    { name: "Error", message: "plain message", code: "FERRIC_ERROR" },
  );
});

// ---------------------------------------------------------------------------
// G-004 manual wire helper: non-plain objects pass through fromWire unchanged
// ---------------------------------------------------------------------------
test("G-004 fromWire leaves non-plain objects unchanged", () => {
  // Buffers and ArrayBuffers are transfer payloads, not CLIPS values; fromWire
  // must not walk their internal fields as if they were plain records.
  const buffer = Buffer.from("abc");
  const arrayBuffer = new ArrayBuffer(4);
  assert.strictEqual(fromWire(buffer, FerricSymbol as any), buffer);
  assert.strictEqual(fromWire(arrayBuffer, FerricSymbol as any), arrayBuffer);
});

// ---------------------------------------------------------------------------
// G-004 manual wire helper: unsupported primitives pass through unchanged
// ---------------------------------------------------------------------------
test("G-004 unsupported wire helper inputs pass through unchanged", () => {
  const symbol = Symbol("not-clips");
  const fn = () => "not-clips";
  assert.strictEqual(toWire(symbol), symbol);
  assert.strictEqual(fromWire(symbol, FerricSymbol as any), symbol);
  assert.strictEqual(toWire(fn), fn);
  assert.strictEqual(fromWire(fn, FerricSymbol as any), fn);
});
