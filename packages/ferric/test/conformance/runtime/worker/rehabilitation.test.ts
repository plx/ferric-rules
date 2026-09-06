import { test } from "node:test";
import * as assert from "node:assert/strict";
import { EngineHandle, EnginePool, FerricCompileError, FerricIOError, FerricRuntimeError } from "../../../helpers/ferric";
import { addFiredCount } from "../../../../dist/limit-validation";

const LOOP = "(defrule count ?f <- (counter ?n) => (retract ?f) (assert (counter (+ ?n 1))))";

test("worker execution permits event-loop progress and retains exact bounded work", { timeout: 15000 }, async () => {
  const engine = await EngineHandle.create({ source: LOOP });
  let turns = 0;
  const timer = setInterval(() => { turns++; }, 0);
  try {
    await engine.assertFact("counter", 0);
    const before = turns;
    const result = await engine.run({ limit: 100000 });
    assert.equal(result.rulesFired, 100000);
    assert.ok(turns > before, "the calling event loop must progress during native work");
    assert.deepEqual((await engine.findFacts("counter"))[0].fields, [100000]);
    assert.ok((await engine.serialize()).length > 0);
    await assert.rejects(engine.load("(unknown-construct x)"), FerricCompileError);
    await assert.rejects(engine.loadFile("/ferric/no-such-file.clp"), FerricIOError);
  } finally { clearInterval(timer); await engine.close(); }
});

test("worker and pool limits reject unsafe numbers and accumulated progress cannot round", async () => {
  const engine = await EngineHandle.create();
  const pool = await EnginePool.create([{ name: "test", source: LOOP }], { threads: 1 });
  try {
    await assert.rejects(engine.run({ limit: Number.MAX_SAFE_INTEGER + 1 }), /safe integer/);
    await assert.rejects(engine.run(100 as unknown as { limit?: number }), /options must be an object/);
    await assert.rejects(pool.evaluate("test", { limit: Number.MAX_SAFE_INTEGER + 1 }), /safe integer/);
    assert.equal(addFiredCount(Number.MAX_SAFE_INTEGER - 2, 2), Number.MAX_SAFE_INTEGER);
    assert.throws(() => addFiredCount(Number.MAX_SAFE_INTEGER, 1), FerricRuntimeError);
  } finally { await Promise.all([engine.close(), pool.close()]); }
});
