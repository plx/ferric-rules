/**
 * Lossless fact-ID tests across the EnginePool worker boundary (FR-NODE-001).
 */
import { test } from "node:test";
import * as assert from "node:assert/strict";

import {
  EnginePool,
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

test("E-010 EnginePool proxy structured-clones high-generation bigint IDs losslessly", async () => {
  const pool = await EnginePool.create(
    [{ name: "churn", source: CHURN_SOURCE }],
    { threads: 1 },
  );
  try {
    await pool.do("churn", async (proxy) => {
      const initialId = await proxy.assertFact("generation", 0);
      assert.strictEqual(typeof initialId, "bigint");

      const legacyNumberId = Number(initialId);
      assert.ok(Number.isSafeInteger(legacyNumberId));
      assert.strictEqual((await proxy.getFact(legacyNumberId))?.id, initialId);

      const run = await proxy.run({ limit: HIGH_GENERATION_FIRINGS });
      assert.strictEqual(run.rulesFired, HIGH_GENERATION_FIRINGS);

      const [fact] = await proxy.findFacts("generation");
      assert.ok(fact, "the terminal generation fact should remain asserted");
      assert.ok(fact.id > BigInt(Number.MAX_SAFE_INTEGER));
      assert.strictEqual(typeof fact.id, "bigint");
      assert.strictEqual(structuredClone(fact.id), fact.id);
      assert.strictEqual(fact.fields[0], HIGH_GENERATION_FIRINGS);
      assert.strictEqual((await proxy.getFact(fact.id))?.id, fact.id);

      await proxy.retract(fact.id);
      assert.strictEqual(await proxy.getFact(fact.id), null);
    });

    // evaluate() uses a separate pool-worker dispatch path from do()/proxy.
    const evaluated = await pool.evaluate("churn", {
      facts: [{ kind: "ordered", relation: "evaluate-result", fields: [1] }],
    });
    const evaluatedFact = evaluated.facts.find(
      (fact) => fact.relation === "evaluate-result",
    );
    assert.ok(evaluatedFact, "evaluate() should return the asserted fact");
    assert.strictEqual(typeof evaluatedFact.id, "bigint");
  } finally {
    await pool.close();
  }
});
