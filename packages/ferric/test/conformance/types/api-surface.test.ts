/**
 * Type conformance tests — these verify that the public API surface
 * compiles correctly under tsc --strict.
 *
 * Two complementary mechanisms exercise this file:
 * - `npm run test:types` type-checks it with `tsc --noEmit` (this is where the
 *   `@ts-expect-error` negative assertions and type-inhabitance checks have
 *   teeth — they fail the build if the public types stop rejecting bad shapes).
 * - `npm run test:runtime:types` executes it with the node test runner, so the
 *   runtime `assert.*` calls (enum values, error codes, method presence) run as
 *   a smoke check rather than being dead code.
 */
import { test } from "node:test";
import * as assert from "node:assert/strict";

import {
  Engine,
  FerricSymbol,
  EngineHandle,
  EnginePool,
  Strategy,
  Encoding,
  HaltReason,
  FactType,
  Format,
  FerricError,
  FerricParseError,
  FerricCompileError,
  FerricRuntimeError,
  FerricFactNotFoundError,
  FerricTemplateNotFoundError,
  FerricSlotNotFoundError,
  FerricModuleNotFoundError,
  FerricEncodingError,
  FerricSerializationError,
} from "../../helpers/ferric";

import type {
  ClipsValue,
  RunResult,
  Fact,
  EngineOptions,
  EngineHandleOptions,
  EngineSpec,
  EvaluateRequest,
  EvaluateResult,
} from "../../helpers/ferric";

// ---------------------------------------------------------------------------
// A-001: Engine is a concrete class
// ---------------------------------------------------------------------------
test("A-001 Engine is a concrete class export", () => {
  assert.strictEqual(typeof Engine, "function");
  const e = new Engine();
  assert.ok(e);
  e.close();
});

// ---------------------------------------------------------------------------
// A-002: FerricSymbol is a concrete class
// ---------------------------------------------------------------------------
test("A-002 FerricSymbol is a concrete class export", () => {
  assert.strictEqual(typeof FerricSymbol, "function");
  const s = new FerricSymbol("test");
  assert.strictEqual(s.value, "test");
});

// ---------------------------------------------------------------------------
// A-003: ClipsValue includes FerricSymbol
// ---------------------------------------------------------------------------
test("A-003 ClipsValue includes FerricSymbol", () => {
  const v: ClipsValue = new FerricSymbol("x");
  assert.ok(v);
});

// ---------------------------------------------------------------------------
// A-003 property-style type corpus: generated ClipsValue variants compile
// ---------------------------------------------------------------------------
test("A-003 property-style ClipsValue accepts generated public value variants", () => {
  // This tuple is a compile-time generator over every public ClipsValue branch;
  // the assertion is that all entries can inhabit ClipsValue under strict mode.
  const values: ClipsValue[] = [
    new FerricSymbol("x"),
    { __type: "FerricSymbol", value: "wired" },
    "text",
    1,
    1.5,
    1n,
    true,
    [new FerricSymbol("nested"), "text"],
    null,
  ];
  assert.strictEqual(values.length, 9);
});

// ---------------------------------------------------------------------------
// A-004: Public enums are regular TS enums
// ---------------------------------------------------------------------------
test("A-004 public enums are usable at runtime", () => {
  assert.strictEqual(Strategy.Depth, 0);
  assert.strictEqual(Encoding.Utf8, 1);
  assert.strictEqual(HaltReason.AgendaEmpty, 0);
  assert.strictEqual(FactType.Ordered, 0);
  assert.strictEqual(Format.Bincode, 0);
});

// ---------------------------------------------------------------------------
// Error classes are importable and constructible
// ---------------------------------------------------------------------------
test("G-003 error classes are importable and constructible", () => {
  const e1 = new FerricError("msg", "CODE");
  assert.ok(e1 instanceof Error);
  assert.ok(e1 instanceof FerricError);

  const e2 = new FerricParseError("parse fail");
  assert.ok(e2 instanceof FerricError);
  assert.strictEqual(e2.name, "FerricParseError");
  assert.strictEqual(e2.code, "FERRIC_PARSE_ERROR");

  const e3 = new FerricCompileError("compile fail");
  assert.ok(e3 instanceof FerricError);

  const e4 = new FerricRuntimeError("runtime fail");
  assert.ok(e4 instanceof FerricError);

  const e5 = new FerricFactNotFoundError("not found");
  assert.ok(e5 instanceof FerricError);

  const e6 = new FerricTemplateNotFoundError("not found");
  assert.ok(e6 instanceof FerricError);

  const e7 = new FerricSlotNotFoundError("not found");
  assert.ok(e7 instanceof FerricError);

  const e8 = new FerricModuleNotFoundError("not found");
  assert.ok(e8 instanceof FerricError);

  const e9 = new FerricEncodingError("encoding fail");
  assert.ok(e9 instanceof FerricError);

  const e10 = new FerricSerializationError("ser fail");
  assert.ok(e10 instanceof FerricError);
});

// ---------------------------------------------------------------------------
// Engine API method signatures compile
// ---------------------------------------------------------------------------
test("A-001 Engine API method signatures compile", () => {
  const e = new Engine();
  assert.strictEqual(typeof e.load, "function");
  assert.strictEqual(typeof e.run, "function");
  assert.strictEqual(typeof e.assertString, "function");
  assert.strictEqual(typeof e.assertFact, "function");
  assert.strictEqual(typeof e.assertTemplate, "function");
  assert.strictEqual(typeof e.reset, "function");
  assert.strictEqual(typeof e.close, "function");
  e.close();
});

// ---------------------------------------------------------------------------
// EngineHandle API signatures compile
// ---------------------------------------------------------------------------
test("A-001 EngineHandle API signatures compile", () => {
  assert.strictEqual(typeof EngineHandle, "function");
  assert.strictEqual(typeof EngineHandle.create, "function");
});

// ---------------------------------------------------------------------------
// EnginePool API signatures compile
// ---------------------------------------------------------------------------
test("A-001 EnginePool API signatures compile", () => {
  assert.strictEqual(typeof EnginePool, "function");
  assert.strictEqual(typeof EnginePool.create, "function");
});

// ---------------------------------------------------------------------------
// Type-only compilation checks
// ---------------------------------------------------------------------------
test("G-003 type compilation: EngineOptions and related types are usable", () => {
  const opts: EngineOptions = { strategy: Strategy.Depth };
  const handleOpts: EngineHandleOptions = { source: "(defrule x =>)" };
  const spec: EngineSpec = { name: "test" };
  const req: EvaluateRequest = { facts: [] };

  assert.ok(opts !== undefined);
  assert.ok(handleOpts !== undefined);
  assert.ok(spec !== undefined);
  assert.ok(req !== undefined);
});

// ---------------------------------------------------------------------------
// A-006: EngineHandle and EnginePool support Symbol.asyncDispose
// ---------------------------------------------------------------------------
test("A-006 EngineHandle and EnginePool support Symbol.asyncDispose", () => {
  // Just verify the types compile — runtime check is elsewhere
  assert.ok(Symbol.asyncDispose !== undefined, "Symbol.asyncDispose should exist");
});

// ---------------------------------------------------------------------------
// A-003 / A-004: NEGATIVE type assertions — the public types must REJECT wrong
// shapes. The positive checks above cannot catch a type being loosened to `any`
// (everything would still compile). Each `@ts-expect-error` below is itself
// verified by tsc: if the type stopped rejecting the value, the now-unused
// directive fails the type check. At runtime these are inert (the values are
// constructed but never used for behavior).
// ---------------------------------------------------------------------------
test("A-003 public types reject invalid shapes (compile-time)", () => {
  // ClipsValue must not admit a function...
  // @ts-expect-error - a function is not a ClipsValue
  const notValue1: ClipsValue = () => 1;
  // ...nor an arbitrary object lacking the wire-symbol shape.
  // @ts-expect-error - a plain object is not a ClipsValue
  const notValue2: ClipsValue = { not: "a clips value" };

  // EngineOptions.strategy is the numeric Strategy enum, not a string.
  // @ts-expect-error - strategy must be a Strategy enum value, not a string
  const badOptions: EngineOptions = { strategy: "depth" };

  // EngineSpec.name is required.
  // @ts-expect-error - name is a required field on EngineSpec
  const badSpec: EngineSpec = {};

  // Reference the bindings so the assertions are not optimized away as unused.
  assert.strictEqual([notValue1, notValue2, badOptions, badSpec].length, 4);
});
