/**
 * Logical-run parity tests for EnginePool (FR-NODE-002 / E-011).
 */
import { test } from "node:test";
import * as assert from "node:assert/strict";

import {
  Engine,
  EnginePool,
  HaltReason,
  RUN_BATCH_SIZE,
} from "../../../helpers/ferric";
import {
  haltAtActivationSource,
  logicalRunStateFacts,
} from "../../../helpers/logical-run";

function observeSync(source: string, limit?: number) {
  const engine = Engine.fromSource(source);
  try {
    return {
      result: engine.run(limit),
      stateFacts: logicalRunStateFacts(engine.facts()),
    };
  } finally {
    engine.close();
  }
}

test(
  "E-011 EnginePool proxy run matches sync at every logical-run halt boundary",
  { timeout: 10_000 },
  async (t) => {
    const boundaries = [
      1,
      RUN_BATCH_SIZE,
      RUN_BATCH_SIZE + 1,
      2 * RUN_BATCH_SIZE,
    ];
    const specs = boundaries.map((boundary) => ({
      name: `activation-${boundary}`,
      source: haltAtActivationSource(boundary),
    }));
    const pool = await EnginePool.create(specs, { threads: 1 });

    try {
      for (const boundary of boundaries) {
        await t.test(`activation-${boundary}`, async () => {
          const specName = `activation-${boundary}`;
          const sync = observeSync(haltAtActivationSource(boundary));
          const pooled = await pool.do(specName, async (proxy) => {
            await proxy.reset();
            const result = await proxy.run();
            return {
              result,
              stateFacts: logicalRunStateFacts(await proxy.facts()),
            };
          });

          assert.deepStrictEqual(sync, {
            result: {
              rulesFired: boundary,
              haltReason: HaltReason.HaltRequested,
            },
            stateFacts: [{ relation: "position", fields: [boundary - 1] }],
          });
          assert.deepStrictEqual(pooled, sync);
        });
      }
    } finally {
      await pool.close();
    }
  },
);

test(
  "E-011 EnginePool.evaluate preserves an exact batch-boundary halt",
  { timeout: 10_000 },
  async () => {
    const source = haltAtActivationSource(RUN_BATCH_SIZE);
    const pool = await EnginePool.create(
      [{ name: "boundary", source }],
      { threads: 1 },
    );
    try {
      const result = await pool.evaluate("boundary", {});
      assert.deepStrictEqual(result.runResult, {
        rulesFired: RUN_BATCH_SIZE,
        haltReason: HaltReason.HaltRequested,
      });
      assert.deepStrictEqual(logicalRunStateFacts(result.facts), [
        { relation: "position", fields: [RUN_BATCH_SIZE - 1] },
      ]);
    } finally {
      await pool.close();
    }
  },
);

test(
  "E-011 EnginePool exact caller limit wins at a pending halt boundary",
  { timeout: 10_000 },
  async () => {
    const source = haltAtActivationSource(RUN_BATCH_SIZE);
    const sync = observeSync(source, RUN_BATCH_SIZE);
    const pool = await EnginePool.create(
      [{ name: "boundary", source }],
      { threads: 1 },
    );
    try {
      const pooled = await pool.do("boundary", async (proxy) => {
        await proxy.reset();
        const result = await proxy.run({ limit: RUN_BATCH_SIZE });
        return {
          result,
          stateFacts: logicalRunStateFacts(await proxy.facts()),
        };
      });

      assert.deepStrictEqual(sync, {
        result: {
          rulesFired: RUN_BATCH_SIZE,
          haltReason: HaltReason.LimitReached,
        },
        stateFacts: [{ relation: "position", fields: [RUN_BATCH_SIZE - 1] }],
      });
      assert.deepStrictEqual(pooled, sync);
    } finally {
      await pool.close();
    }
  },
);

test(
  "E-011 a later EnginePool proxy run starts a fresh logical run",
  { timeout: 10_000 },
  async () => {
    const pool = await EnginePool.create(
      [{ name: "fresh", source: haltAtActivationSource(1) }],
      { threads: 1 },
    );
    try {
      const observed = await pool.do("fresh", async (proxy) => {
        await proxy.reset();
        const first = await proxy.run();
        const firstFacts = logicalRunStateFacts(await proxy.facts());
        const second = await proxy.run();
        const secondFacts = logicalRunStateFacts(await proxy.facts());
        return { first, firstFacts, second, secondFacts };
      });

      assert.deepStrictEqual(observed, {
        first: { rulesFired: 1, haltReason: HaltReason.HaltRequested },
        firstFacts: [{ relation: "position", fields: [0] }],
        second: { rulesFired: 1, haltReason: HaltReason.AgendaEmpty },
        secondFacts: [{ relation: "past-boundary", fields: [] }],
      });
    } finally {
      await pool.close();
    }
  },
);
