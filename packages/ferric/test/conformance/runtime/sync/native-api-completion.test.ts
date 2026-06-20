/**
 * Sync native Engine API completion tests.
 *
 * These cover native proxy branches and error-class constructors that are not
 * fully exercised by the higher-level smoke tests.
 */
import { test } from "node:test";
import * as assert from "node:assert/strict";
import { mkdtempSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

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
  FerricSerializationError,
  Format,
  HaltReason,
} from "../../../helpers/ferric";
import { convertNativeError } from "../../../../dist/types";

const SOURCE = "(defrule hello (initial-fact) => (printout t \"hello\" crlf))";

// ---------------------------------------------------------------------------
// G-001 manual native proxy: static factory methods return wrapped Engines
// ---------------------------------------------------------------------------
test("G-001 Engine static factories return wrapped native Engine instances", () => {
  const dir = mkdtempSync(join(tmpdir(), "ferric-native-api-"));
  const path = join(dir, "snapshot.bin");

  const fromSource = Engine.fromSource(SOURCE);
  try {
    const result = fromSource.run();
    assert.strictEqual(result.rulesFired, 1);
    assert.strictEqual(result.haltReason, HaltReason.AgendaEmpty);

    const snapshot = fromSource.serialize(Format.Bincode);
    const fromSnapshot = Engine.fromSnapshot(snapshot, Format.Bincode);
    try {
      assert.strictEqual(fromSnapshot.run().haltReason, HaltReason.AgendaEmpty);
      fromSnapshot.saveSnapshot(path, Format.Bincode);

      const fromSnapshotFile = Engine.fromSnapshotFile(path, Format.Bincode);
      try {
        assert.strictEqual(fromSnapshotFile.run().haltReason, HaltReason.AgendaEmpty);
      } finally {
        fromSnapshotFile.close();
      }
    } finally {
      fromSnapshot.close();
    }
  } finally {
    fromSource.close();
  }
});

// ---------------------------------------------------------------------------
// G-001 manual native proxy: instanceof identity survives the wrapper
// ---------------------------------------------------------------------------
test("G-001 wrapped Engine instances are recognized by instanceof Engine", () => {
  // Every engine the wrapper hands back is a Proxy over a raw native engine,
  // so its prototype chain never contains WrappedEngine.prototype. A custom
  // Symbol.hasInstance must keep `engine instanceof Engine` true across all
  // construction paths, otherwise downstream `instanceof Engine` guards
  // misclassify valid engines. Regression test for PR #82.
  const dir = mkdtempSync(join(tmpdir(), "ferric-instanceof-"));
  const path = join(dir, "snapshot.bin");

  const constructed = new Engine();
  const fromSource = Engine.fromSource(SOURCE);
  const snapshot = fromSource.serialize(Format.Bincode);
  const fromSnapshot = Engine.fromSnapshot(snapshot, Format.Bincode);
  fromSnapshot.saveSnapshot(path, Format.Bincode);
  const fromSnapshotFile = Engine.fromSnapshotFile(path, Format.Bincode);

  try {
    const engines: Array<[string, unknown]> = [
      ["new Engine()", constructed],
      ["Engine.fromSource", fromSource],
      ["Engine.fromSnapshot", fromSnapshot],
      ["Engine.fromSnapshotFile", fromSnapshotFile],
    ];
    for (const [label, engine] of engines) {
      assert.ok(engine instanceof Engine, `${label} should be instanceof Engine`);
    }

    // Non-engine values must still be rejected by the custom hasInstance.
    assert.ok(!({} instanceof Engine), "plain object is not an Engine");
    assert.ok(!(null instanceof Engine), "null is not an Engine");
    assert.ok(!(0 instanceof Engine), "primitive is not an Engine");
  } finally {
    constructed.close();
    fromSource.close();
    fromSnapshot.close();
    fromSnapshotFile.close();
  }
});

// ---------------------------------------------------------------------------
// G-001 manual native proxy: getter properties pass through unchanged
// ---------------------------------------------------------------------------
test("G-001 Engine native getter properties pass through proxy", () => {
  const engine = new Engine();
  try {
    // Non-function property reads use the proxy's Reflect.get fallback rather
    // than method wrapping; factCount is a stable public getter for that path.
    engine.reset();
    assert.strictEqual(typeof engine.factCount, "number");
    assert.strictEqual(engine.isHalted, false);
  } finally {
    engine.close();
  }
});

// ---------------------------------------------------------------------------
// C-003 manual native proxy: static factory errors are converted
// ---------------------------------------------------------------------------
test("C-003 Engine static factory errors are converted to FerricError subclasses", () => {
  // Static factory calls are wrapped separately from instance methods; this
  // verifies thrown native errors still become package-level FerricError types.
  assert.throws(
    () => Engine.fromSource("(defrule broken ("),
    (err: any) => {
      assert.ok(err instanceof FerricParseError);
      assert.ok(err instanceof FerricError);
      assert.strictEqual(err.code, "FERRIC_PARSE_ERROR");
      return true;
    },
  );

  assert.throws(
    () => Engine.fromSnapshot(Buffer.from("not a ferric snapshot"), Format.Bincode),
    (err: any) => {
      assert.ok(err instanceof FerricSerializationError);
      assert.strictEqual(err.code, "FERRIC_SERIALIZATION_ERROR");
      return true;
    },
  );

  assert.throws(
    () => Engine.fromSnapshotFile("/path/that/does/not/exist/ferric.snapshot", Format.Bincode),
    (err: any) => {
      assert.ok(err instanceof Error);
      assert.match(err.message, /No such file|not exist|no such file/i);
      return true;
    },
  );
});

// ---------------------------------------------------------------------------
// A-005 manual native proxy: Symbol.dispose converts close errors
// ---------------------------------------------------------------------------
test("A-005 Engine Symbol.dispose converts close errors from native proxy", () => {
  const engine = new Engine();
  const dispose = (engine as any)[Symbol.dispose];

  // Replace close on the proxied object with a synthetic prefixed native error
  // to cover the Symbol.dispose catch/conversion branch deterministically.
  (engine as any).close = () => {
    throw new Error("FerricRuntimeError: synthetic close failure");
  };

  assert.throws(
    () => dispose.call(engine),
    (err: any) => {
      assert.ok(err instanceof FerricRuntimeError);
      assert.strictEqual(err.message, "synthetic close failure");
      return true;
    },
  );
});

// ---------------------------------------------------------------------------
// C-003 property-style error constructors preserve name/code/prototype
// ---------------------------------------------------------------------------
test("C-003 property-style FerricError subclasses preserve name/code/prototype", () => {
  // Deterministic generation over every exported error class proves the error
  // hierarchy contract for constructors, registry factories, and instanceof.
  const cases: Array<[new (message: string) => FerricError, string, string]> = [
    [FerricParseError, "FerricParseError", "FERRIC_PARSE_ERROR"],
    [FerricCompileError, "FerricCompileError", "FERRIC_COMPILE_ERROR"],
    [FerricRuntimeError, "FerricRuntimeError", "FERRIC_RUNTIME_ERROR"],
    [FerricFactNotFoundError, "FerricFactNotFoundError", "FERRIC_FACT_NOT_FOUND"],
    [FerricTemplateNotFoundError, "FerricTemplateNotFoundError", "FERRIC_TEMPLATE_NOT_FOUND"],
    [FerricSlotNotFoundError, "FerricSlotNotFoundError", "FERRIC_SLOT_NOT_FOUND"],
    [FerricModuleNotFoundError, "FerricModuleNotFoundError", "FERRIC_MODULE_NOT_FOUND"],
    [FerricEncodingError, "FerricEncodingError", "FERRIC_ENCODING_ERROR"],
    [FerricSerializationError, "FerricSerializationError", "FERRIC_SERIALIZATION_ERROR"],
  ];

  for (const [Ctor, name, code] of cases) {
    const error = new Ctor(`message for ${name}`);
    assert.ok(error instanceof Ctor);
    assert.ok(error instanceof FerricError);
    assert.strictEqual(error.name, name);
    assert.strictEqual(error.code, code);
    assert.strictEqual(error.message, `message for ${name}`);
  }
});

// ---------------------------------------------------------------------------
// C-003 property-style convertNativeError handles non-registry inputs
// ---------------------------------------------------------------------------
test("C-003 property-style convertNativeError preserves unknown native errors", () => {
  const nonError = convertNativeError("plain failure");
  assert.ok(nonError instanceof Error);
  assert.strictEqual(nonError.message, "plain failure");

  const unknownPrefixed = new Error("FerricImaginaryError: future failure");
  assert.strictEqual(convertNativeError(unknownPrefixed), unknownPrefixed);

  const ordinary = new TypeError("ordinary");
  assert.strictEqual(convertNativeError(ordinary), ordinary);
});
