import { test } from "node:test";
import * as assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { resolve } from "node:path";
import { Engine, FerricSymbol, FerricRuntimeError, FerricParseError, FerricCompileError, FerricIOError, Format, HaltReason } from "../../../helpers/ferric";

const nativeAddon = resolve(__dirname, "../../../../../../crates/ferric-rules-napi/ferric-rules-napi.node");

test("native value getters cannot close, mutate or read the reserved engine", () => {
  const source = `
    const assert = require('node:assert/strict');
    const { Engine } = require(${JSON.stringify(nativeAddon)});
    const engine = new Engine();
    let entries = 0;
    const value = { __ferric_symbol: true, get value() {
      entries++;
      for (const operation of [() => engine.close(), () => engine.reset(), () => engine.factCount]) {
        assert.throws(operation, /FerricRuntimeError: reentrant/);
      }
      return 'still-live';
    }};
    engine.assertFact('probe', [value]);
    assert.equal(entries, 1);
    assert.equal(engine.factCount, 1);
    const bad = { __ferric_symbol: true, get value() { throw new Error('getter failed'); }};
    assert.throws(() => engine.assertFact('bad', [bad]), /getter failed/);
    assert.equal(engine.factCount, 1);
    engine.assertFact('after-error', [42]);
    assert.equal(engine.factCount, 2);
    engine.close();
    engine.close();
  `;
  const child = spawnSync(process.execPath, ["-e", source], { encoding: "utf8", timeout: 10000 });
  assert.equal(child.error, undefined);
  assert.equal(child.signal, null, child.stderr);
  assert.equal(child.status, 0, child.stderr);
});

test("native reentry getter errors normalize and preserve cause", () => {
  const engine = new Engine();
  try {
    const value = { __ferric_symbol: true, get value() { return engine.factCount.toString(); } };
    assert.throws(() => engine.assertFact("bad", value), (error: unknown) => {
      assert.ok(error instanceof FerricRuntimeError);
      assert.match(error.message, /reentrant/);
      assert.ok(error.cause instanceof Error);
      return true;
    });
    assert.equal(engine.factCount, 0);
  } finally { engine.close(); }
});

test("symbol marker requires own true marker and string payload", () => {
  const engine = new Engine();
  try {
    for (const value of [
      { __ferric_symbol: false, value: "x" },
      { __ferric_symbol: 0, value: "x" },
      Object.assign(Object.create({ __ferric_symbol: true }), { value: "x" }),
      { value: "x" },
      { __ferric_symbol: true, value: 12 },
    ]) assert.throws(() => engine.assertFact("bad", value));
    assert.equal(engine.factCount, 0);
    engine.assertFact("good", new FerricSymbol("x"));
    engine.assertFact("wire", { __type: "FerricSymbol", value: "from-worker" });
    assert.equal((engine.findFacts("wire")[0].fields![0] as { value: string }).value, "from-worker");
    assert.equal((engine.facts()[0].fields![0] as { value: string }).value, "x");
  } finally { engine.close(); }
});

test("wide counts and limits stay exact and unsafe integer values are rejected", () => {
  const engine = new Engine();
  try {
    engine.load("(deffacts one (item 1)) (defrule consume ?f <- (item ?n) => (retract ?f))");
    engine.reset();
    assert.equal(engine.run(2 ** 32).rulesFired, 1);
    assert.equal(engine.run(Number.MAX_SAFE_INTEGER).rulesFired, 0);
    for (const limit of [-1, 0.5, Infinity, NaN, Number.MAX_SAFE_INTEGER + 1]) {
      assert.throws(() => engine.run(limit), /safe integer/);
    }
    assert.throws(() => engine.assertFact("unsafe", Number.MAX_SAFE_INTEGER + 1), /bigint/);
    engine.assertFact("exact", 9223372036854775807n, -9223372036854775808n);
    assert.deepEqual(engine.findFacts("exact")[0].fields, [9223372036854775807n, -9223372036854775808n]);
  } finally { engine.close(); }
});

test("known load failure families and default CBOR are explicit", () => {
  const engine = new Engine();
  try {
    assert.throws(() => engine.load("(defrule incomplete"), FerricParseError);
    assert.throws(() => engine.load("(unknown-construct x)"), FerricCompileError);
    assert.throws(() => engine.loadFile("/ferric/no-such-file.clp"), FerricIOError);
    engine.assertFact("saved", "payload");
    const snapshot = engine.serialize();
    const restored = Engine.fromSnapshot(snapshot, Format.Cbor);
    try { assert.deepEqual(restored.facts()[0].fields, ["payload"]); }
    finally { restored.close(); }
  } finally { engine.close(); }
});


test("native continuation and factories cannot confuse wrapped Rust types", () => {
  const source = `
    const assert = require('node:assert/strict');
    const fs = require('node:fs');
    const os = require('node:os');
    const path = require('node:path');
    const native = require(${JSON.stringify(nativeAddon)});
    const { Engine, FerricSymbol, __continueRun: continueRun } = native;
    const engine = new Engine();
    const temp = fs.mkdtempSync(path.join(os.tmpdir(), 'ferric-factory-'));
    try {
      assert.equal(Object.hasOwn(Engine.prototype, '__continueRun'), false);
      const foreign = new FerricSymbol('wrong native type');
      const spoofed = new FerricSymbol('spoofed');
      Object.setPrototypeOf(spoofed, Engine.prototype);
      for (const receiver of [foreign, spoofed, Object.create(Engine.prototype), {}]) {
        assert.throws(() => continueRun.call(receiver, 1), /Illegal invocation|Invalid argument/);
        assert.throws(() => Engine.prototype.close.call(receiver), /Illegal invocation|Invalid argument/);
      }
      const getter = { __ferric_symbol: true, get value() {
        assert.throws(() => continueRun.call(engine, 1), /FerricRuntimeError: reentrant/);
        return 'guarded';
      }};
      engine.assertFact('input', [getter]);
      const snapshot = engine.serialize();
      const file = path.join(temp, 'snapshot');
      fs.writeFileSync(file, snapshot);
      class Derived extends Engine {}
      class Other {}
      for (const receiver of [FerricSymbol, Other, Derived, null]) {
        const results = [
          Engine.fromSource.call(receiver, '(defrule one => (assert (result 42)))'),
          Engine.fromSnapshot.call(receiver, snapshot),
          Engine.fromSnapshotFile.call(receiver, file),
        ];
        for (const result of results) {
          assert.equal(Object.getPrototypeOf(result), Engine.prototype);
          assert.equal(typeof result.factCount, 'number');
          result.close();
        }
      }
      const continued = Engine.fromSource('(defrule once => (assert (result 42)))');
      assert.equal(continued.run(0).rulesFired, 0);
      assert.equal(continueRun.call(continued, 1).rulesFired, 1);
      assert.equal(continued.factCount, 1);
      continued.close();
    } finally { engine.close(); fs.rmSync(temp, { recursive: true, force: true }); }
  `;
  const child = spawnSync(process.execPath, ["-e", source], { encoding: "utf8", timeout: 10000 });
  assert.equal(child.error, undefined);
  assert.equal(child.signal, null, child.stderr);
  assert.equal(child.status, 0, child.stderr);
});


test("configuration numbers are validated before native narrowing", () => {
  const native = require(nativeAddon);
  const source = "(deffunction answer () 42) (defrule once => (assert (result (answer))))";
  for (const Binding of [Engine, native.Engine]) {
    for (const create of [
      (options: object) => new Binding(options),
      (options: object) => Binding.fromSource("(defrule once => (assert (result 42)))", options),
    ]) {
      for (const maxCallDepth of [-1, 1.5, 2 ** 32, Infinity, NaN]) {
        assert.throws(() => create({ maxCallDepth }), /maxCallDepth.*integer/);
      }
      for (const field of ["strategy", "encoding"]) {
        for (const value of [-1, 0.5, 2 ** 32, -(2 ** 32), Infinity, NaN, 99]) {
          assert.throws(() => create({ [field]: value }), new RegExp(field));
        }
      }
      const valid = create({ maxCallDepth: 2 ** 32 - 1 });
      valid.close();
    }
    for (const depth of [0, 1]) {
      const engine = Binding.fromSource(source, { maxCallDepth: depth });
      try {
        const result = engine.run(1);
        if (depth === 0) {
          assert.equal(result.haltReason, HaltReason.ActionError);
          assert.equal(engine.findFacts("result").length, 0);
          assert.match(engine.diagnostics.join(" "), /depth/);
        } else {
          assert.equal(engine.findFacts("result")[0].fields[0], 42);
        }
      } finally { engine.close(); }
    }
  }
});

test("snapshot selectors reject fractional and wrapped enum values before file access", () => {
  const native = require(nativeAddon);
  for (const Binding of [Engine, native.Engine]) {
    const engine = new Binding();
    const saved = engine.serialize();
    try {
      for (const format of [-1, 0.5, 2 ** 32, -(2 ** 32), Infinity, NaN, 99]) {
        assert.throws(() => engine.serialize(format), /snapshot format/);
        assert.throws(() => Binding.fromSnapshot(saved, format), /snapshot format/);
        assert.throws(() => Binding.fromSnapshotFile("/ferric/no-such-snapshot", format), /snapshot format/);
        assert.throws(() => engine.saveSnapshot("/ferric/no-such-snapshot", format), /snapshot format/);
      }
    } finally { engine.close(); }
  }
});
