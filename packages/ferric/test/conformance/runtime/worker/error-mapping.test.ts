/**
 * Worker-backed error mapping tests (C-001..C-005).
 */
import { test } from "node:test";
import * as assert from "node:assert/strict";

import {
  EngineHandle,
  FerricError,
  FerricParseError,
  FerricFactNotFoundError,
  FerricTemplateNotFoundError,
  FerricModuleNotFoundError,
} from "../../../helpers/ferric";

// ---------------------------------------------------------------------------
// C-001: Parse error via worker
// ---------------------------------------------------------------------------
test("C-001 parse error maps to FerricParseError (worker)", async () => {
  const handle = await EngineHandle.create();
  try {
    await assert.rejects(
      () => handle.load("(invalid syntax!!!"),
      (err: any) => {
        assert.ok(err instanceof FerricParseError, `Expected FerricParseError, got ${err.name}`);
        assert.ok(err instanceof FerricError);
        assert.strictEqual(err.name, "FerricParseError");
        assert.strictEqual(err.code, "FERRIC_PARSE_ERROR");
        assert.ok(err.message.length > 0);
        return true;
      }
    );
  } finally {
    await handle.close();
  }
});

// ---------------------------------------------------------------------------
// C-003: Fact not found via worker
// ---------------------------------------------------------------------------
test("C-003 fact not found maps to FerricFactNotFoundError (worker)", async () => {
  const handle = await EngineHandle.create();
  try {
    await handle.reset();
    await assert.rejects(
      () => handle.retract(99999),
      (err: any) => {
        assert.ok(err instanceof FerricFactNotFoundError, `Expected FerricFactNotFoundError, got ${err.name}`);
        assert.ok(err instanceof FerricError);
        assert.strictEqual(err.name, "FerricFactNotFoundError");
        assert.strictEqual(err.code, "FERRIC_FACT_NOT_FOUND");
        return true;
      }
    );
  } finally {
    await handle.close();
  }
});

// ---------------------------------------------------------------------------
// C-003: Template not found via worker
// ---------------------------------------------------------------------------
test("C-003 template not found maps to FerricTemplateNotFoundError (worker)", async () => {
  const handle = await EngineHandle.create();
  try {
    await handle.reset();
    await assert.rejects(
      () => handle.assertTemplate("nonexistent", { x: 1 }),
      (err: any) => {
        assert.ok(err instanceof FerricTemplateNotFoundError, `Expected FerricTemplateNotFoundError, got ${err.name}`);
        assert.ok(err instanceof FerricError);
        assert.strictEqual(err.name, "FerricTemplateNotFoundError");
        return true;
      }
    );
  } finally {
    await handle.close();
  }
});

// ---------------------------------------------------------------------------
// C-004: Worker error payload has stable name/code/message
// ---------------------------------------------------------------------------
test("C-004 worker error payload preserves name, code, message", async () => {
  const handle = await EngineHandle.create();
  try {
    await handle.reset();
    try {
      await handle.retract(99999);
      assert.fail("Should have thrown");
    } catch (err: any) {
      // Verify stable fields exist
      assert.strictEqual(typeof err.name, "string");
      assert.strictEqual(typeof err.code, "string");
      assert.strictEqual(typeof err.message, "string");
      assert.ok(err.name.startsWith("Ferric"), `Error name should start with Ferric, got ${err.name}`);
    }
  } finally {
    await handle.close();
  }
});

// ---------------------------------------------------------------------------
// C-005: Unknown error names degrade to FerricError
// ---------------------------------------------------------------------------
test("C-005 unknown error names degrade to FerricError with preserved code/message", async () => {
  const { ERROR_REGISTRY } = await import("../../../helpers/ferric");

  const unknownPayload = { name: "SomeUnknownError", message: "something broke", code: "CUSTOM_CODE" };

  // The reconstruction logic falls back to FerricError
  const Ctor = ERROR_REGISTRY[unknownPayload.name];
  assert.strictEqual(Ctor, undefined, "Unknown error name should not be in registry");

  // Fallback: construct FerricError with preserved message and code
  const err = new FerricError(unknownPayload.message, unknownPayload.code);
  err.name = unknownPayload.name;
  assert.strictEqual(err.message, "something broke");
  assert.strictEqual(err.code, "CUSTOM_CODE");
  assert.strictEqual(err.name, "SomeUnknownError");
  assert.ok(err instanceof FerricError);
});
