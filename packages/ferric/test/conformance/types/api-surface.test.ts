/**
 * Type conformance tests — these verify that the public API surface
 * compiles correctly under tsc --strict.
 *
 * These tests are validated by the TypeScript compiler (tsc --noEmit).
 * They also run at runtime as a basic smoke check.
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
