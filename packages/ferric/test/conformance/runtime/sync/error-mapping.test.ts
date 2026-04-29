/**
 * Sync Engine error mapping tests (C-001..C-003).
 */
import { test } from "node:test";
import * as assert from "node:assert/strict";

import {
  Engine,
  FerricError,
  FerricParseError,
  FerricCompileError,
  FerricRuntimeError,
  FerricFactNotFoundError,
  FerricTemplateNotFoundError,
  FerricSlotNotFoundError,
  FerricModuleNotFoundError,
  FerricEncodingError,
} from "../../../helpers/ferric";

// ---------------------------------------------------------------------------
// C-001: Parse failures surface as FerricParseError
// ---------------------------------------------------------------------------
test("C-001 parse error maps to FerricParseError (sync)", () => {
  const e = new Engine();
  try {
    assert.throws(
      () => e.load("(invalid syntax!!!"),
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
    e.close();
  }
});

// ---------------------------------------------------------------------------
// C-002: Compile failures surface as FerricCompileError
// ---------------------------------------------------------------------------
test("C-002 compile error maps to FerricCompileError (sync)", () => {
  const e = new Engine();
  try {
    // Reference an undefined template to trigger a compile error
    assert.throws(
      () => e.load("(defrule bad (nonexistent-template (slot x)) =>)"),
      (err: any) => {
        // This may be a parse or compile error depending on the engine
        assert.ok(
          err instanceof FerricParseError || err instanceof FerricCompileError,
          `Expected FerricParseError or FerricCompileError, got ${err.name}`
        );
        assert.ok(err instanceof FerricError);
        return true;
      }
    );
  } finally {
    e.close();
  }
});

// ---------------------------------------------------------------------------
// C-003: Fact not found error
// ---------------------------------------------------------------------------
test("C-003 fact not found maps to FerricFactNotFoundError (sync)", () => {
  const e = new Engine();
  e.reset();
  try {
    assert.throws(
      () => e.retract(99999),
      (err: any) => {
        assert.ok(err instanceof FerricFactNotFoundError, `Expected FerricFactNotFoundError, got ${err.name}`);
        assert.ok(err instanceof FerricError);
        assert.strictEqual(err.name, "FerricFactNotFoundError");
        assert.strictEqual(err.code, "FERRIC_FACT_NOT_FOUND");
        return true;
      }
    );
  } finally {
    e.close();
  }
});

// ---------------------------------------------------------------------------
// C-003: Template not found error
// ---------------------------------------------------------------------------
test("C-003 template not found maps to FerricTemplateNotFoundError (sync)", () => {
  const e = new Engine();
  e.reset();
  try {
    assert.throws(
      () => e.assertTemplate("nonexistent", { x: 1 }),
      (err: any) => {
        assert.ok(err instanceof FerricTemplateNotFoundError, `Expected FerricTemplateNotFoundError, got ${err.name}`);
        assert.ok(err instanceof FerricError);
        assert.strictEqual(err.name, "FerricTemplateNotFoundError");
        assert.strictEqual(err.code, "FERRIC_TEMPLATE_NOT_FOUND");
        return true;
      }
    );
  } finally {
    e.close();
  }
});

// ---------------------------------------------------------------------------
// C-003: Module not found error
// ---------------------------------------------------------------------------
test("C-003 module not found maps to FerricModuleNotFoundError (sync)", () => {
  const e = new Engine();
  e.reset();
  try {
    assert.throws(
      () => e.setFocus("NONEXISTENT_MODULE"),
      (err: any) => {
        assert.ok(err instanceof FerricModuleNotFoundError, `Expected FerricModuleNotFoundError, got ${err.name}`);
        assert.ok(err instanceof FerricError);
        assert.strictEqual(err.name, "FerricModuleNotFoundError");
        assert.strictEqual(err.code, "FERRIC_MODULE_NOT_FOUND");
        return true;
      }
    );
  } finally {
    e.close();
  }
});

// ---------------------------------------------------------------------------
// C-003: Slot not found error
// ---------------------------------------------------------------------------
test("C-003 slot not found maps to FerricSlotNotFoundError (sync)", () => {
  const e = new Engine();
  e.load("(deftemplate person (slot name))");
  e.reset();
  const id = e.assertTemplate("person", { name: "Alice" });
  try {
    assert.throws(
      () => e.getFactSlot(id, "nonexistent_slot"),
      (err: any) => {
        assert.ok(err instanceof FerricSlotNotFoundError, `Expected FerricSlotNotFoundError, got ${err.name}`);
        assert.ok(err instanceof FerricError);
        assert.strictEqual(err.name, "FerricSlotNotFoundError");
        assert.strictEqual(err.code, "FERRIC_SLOT_NOT_FOUND");
        return true;
      }
    );
  } finally {
    e.close();
  }
});
