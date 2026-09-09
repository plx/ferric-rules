import { test } from "node:test";
import * as assert from "node:assert/strict";
import {
  Engine, EngineHandle, EnginePool, FerricStringBytes, FerricSymbolBytes,
  FerricInstanceName, Format, HaltReason, toWire, fromWire,
} from "../../../helpers/ferric";

const bytes = Uint8Array.from([0x61, 0, 0xff]);
const source = "(defrule emit (bytes ?x) => (printout t ?x))";
const constructors = { FerricStringBytes, FerricSymbolBytes, FerricInstanceName };

function typedValues() {
  return [new FerricStringBytes(bytes), new FerricSymbolBytes(bytes), new FerricInstanceName(bytes)];
}

function assertTypedValues(values: readonly unknown[]) {
  for (const [index, ctor] of Object.values(constructors).entries()) {
    const value = values[index];
    assert.ok(value instanceof ctor);
    assert.deepEqual(value.bytes, bytes);
    assert.throws(() => value.value, /UTF-8|utf-8/);
  }
}

test("native byte lexemes, checked output and all snapshot codecs", () => {
  const engine = new Engine();
  try {
    engine.assertFact("typed", ...typedValues());
    assertTypedValues(engine.findFacts("typed")[0].fields);
    for (const format of [Format.Bincode, Format.Json, Format.Cbor, Format.MessagePack, Format.Postcard]) {
      const restored = Engine.fromSnapshot(engine.serialize(format), format);
      try { assertTypedValues(restored.findFacts("typed")[0].fields); }
      finally { restored.close(); }
    }
    engine.load(source);
    engine.assertFact("bytes", new FerricStringBytes(bytes));
    assert.equal(engine.run().rulesFired, 1);
    assert.deepEqual(engine.getOutputBytes("t"), bytes);
    assert.throws(() => engine.getOutput("t"), /UTF-8|utf-8/);
  } finally { engine.close(); }
});

test("byte wrappers copy inputs and wire preserves nested byte payloads", () => {
  const input = Uint8Array.from(bytes);
  const value = new FerricStringBytes(input);
  input[0] = 0;
  assert.deepEqual(value.bytes, bytes);
  const copy = value.bytes;
  copy[0] = 0;
  assert.deepEqual(value.bytes, bytes);
  const wire = structuredClone(toWire({ fields: typedValues(), output: bytes }));
  const result = fromWire(wire, undefined, constructors) as { fields: unknown[]; output: Uint8Array };
  assertTypedValues(result.fields);
  assert.deepEqual(result.output, bytes);
});

test("worker byte facts, exact output and checked text survive transport", async () => {
  const handle = await EngineHandle.create({ source });
  try {
    await handle.assertFact("typed", ...typedValues());
    assertTypedValues((await handle.findFacts("typed"))[0].fields);
    await handle.assertFact("bytes", new FerricStringBytes(bytes));
    await handle.run();
    assert.deepEqual(await handle.getOutputBytes("t"), bytes);
    await assert.rejects(handle.getOutput("t"), /UTF-8|utf-8/);
  } finally { await handle.close(); }
});

test("pool returns raw output without replacement text and rehydrates typed facts", async () => {
  const pool = await EnginePool.create([{ name: "bytes", source }], { threads: 1 });
  try {
    const result = await pool.evaluate("bytes", { facts: [
      { kind: "ordered", relation: "typed", fields: typedValues() },
      { kind: "ordered", relation: "bytes", fields: [new FerricStringBytes(bytes)] },
    ] });
    assertTypedValues(result.facts.find((fact) => fact.relation === "typed")!.fields);
    assert.deepEqual(result.outputBytes.stdout, bytes);
    assert.equal(result.output.stdout, undefined);
    await pool.do("bytes", async (proxy) => {
      await proxy.reset();
      await proxy.assertFact("bytes", new FerricStringBytes(bytes));
      await proxy.run();
      assert.deepEqual(await proxy.getOutputBytes("t"), bytes);
      await assert.rejects(proxy.getOutput("t"), /UTF-8|utf-8/);
    });
  } finally { await pool.close(); }
});


for (const operation of ["type", "named"]) {
  test(`missing-instance ${operation} preserves typed facts and halts later actions`, () => {
    const engine = new Engine();
    try {
      engine.load(`(defgeneric named)
        (defmethod named ((?x INSTANCE-NAME)) unreachable)
        (defrule check (name ?x) =>
          (printout t (instance-namep ?x) crlf) (assert (before ?x))
          (${operation} ?x) (assert (after)))`);
      engine.reset();
      const payload = new TextEncoder().encode("missing");
      engine.assertFact("name", new FerricInstanceName(payload));
      const result = engine.run();
      assert.equal(result.haltReason, HaltReason.ActionError);
      assert.equal(result.rulesFired, 1);
      assert.ok(engine.diagnostics.length > 0);
      assert.equal(engine.getOutput("t"), "TRUE\n");
      const fields = engine.findFacts("before")[0].fields;
      assert.equal(fields.length, 1);
      assert.ok(fields[0] instanceof FerricInstanceName);
      assert.deepEqual(fields[0].bytes, payload);
      assert.deepEqual(engine.findFacts("after"), []);
    } finally { engine.close(); }
  });
}
