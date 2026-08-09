/**
 * Lossless fact-ID tests across the EngineHandle worker boundary (FR-NODE-001).
 */
import { test } from "node:test";
import * as assert from "node:assert/strict";

import {
  EngineHandle,
} from "../../../helpers/ferric";

const HIGH_GENERATION_FIRINGS = 1_048_577;
const CHURN_SOURCE = `
(defrule churn
  ?fact <- (generation ?value)
  (test (< ?value ${HIGH_GENERATION_FIRINGS}))
  =>
  (retract ?fact)
  (assert (generation (+ ?value 1))))
`;

test("D-008 EngineHandle structured-clones high-generation bigint IDs losslessly", async () => {
  const handle = await EngineHandle.create({ source: CHURN_SOURCE });
  let restored: EngineHandle | undefined;
  try {
    const initialId = await handle.assertFact("generation", 0);
    assert.strictEqual(typeof initialId, "bigint");

    // Safe legacy numbers still cross the worker boundary on input, while the
    // returned Fact snapshot remains canonical bigint.
    const legacyNumberId = Number(initialId);
    assert.ok(Number.isSafeInteger(legacyNumberId));
    assert.strictEqual((await handle.getFact(legacyNumberId))?.id, initialId);

    const run = await handle.run({ limit: HIGH_GENERATION_FIRINGS });
    assert.strictEqual(run.rulesFired, HIGH_GENERATION_FIRINGS);

    const [fact] = await handle.findFacts("generation");
    assert.ok(fact, "the terminal generation fact should remain asserted");
    assert.ok(fact.id > BigInt(Number.MAX_SAFE_INTEGER));
    assert.strictEqual(typeof fact.id, "bigint");
    assert.strictEqual(structuredClone(fact.id), fact.id);
    assert.strictEqual(fact.fields[0], HIGH_GENERATION_FIRINGS);
    assert.strictEqual((await handle.getFact(fact.id))?.id, fact.id);

    const snapshot = await handle.serialize();
    restored = await EngineHandle.create({ snapshot: { data: snapshot } });
    assert.strictEqual((await restored.getFact(fact.id))?.id, fact.id);
    await restored.retract(fact.id);
    assert.strictEqual(await restored.getFact(fact.id), null);

    await handle.retract(fact.id);
    assert.strictEqual(await handle.getFact(fact.id), null);
  } finally {
    await restored?.close();
    await handle.close();
  }
});
