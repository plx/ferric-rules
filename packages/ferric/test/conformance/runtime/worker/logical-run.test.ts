/**
 * Logical-run parity tests for EngineHandle (FR-NODE-002 / D-009).
 */
import { test } from "node:test";
import * as assert from "node:assert/strict";

import {
  Engine,
  EngineHandle,
  HaltReason,
  RUN_BATCH_SIZE,
} from "../../../helpers/ferric";
import {
  HALT_WITH_DIAGNOSTIC_SOURCE,
  haltAtActivationSource,
  logicalRunStateFacts,
} from "../../../helpers/logical-run";

function observeSync(source: string, limit?: number) {
  const engine = Engine.fromSource(source);
  try {
    return {
      result: engine.run(limit),
      halted: engine.isHalted,
      agendaSize: engine.agendaSize,
      stateFacts: logicalRunStateFacts(engine.facts()),
    };
  } finally {
    engine.close();
  }
}

async function observeHandle(source: string, limit?: number) {
  const handle = await EngineHandle.create({ source });
  try {
    return {
      result: await handle.run(limit === undefined ? undefined : { limit }),
      halted: await handle.getIsHalted(),
      agendaSize: await handle.getAgendaSize(),
      stateFacts: logicalRunStateFacts(await handle.facts()),
    };
  } finally {
    await handle.close();
  }
}

test(
  "D-009 EngineHandle matches sync at every logical-run halt boundary",
  { timeout: 10_000 },
  async (t) => {
    const boundaries = [
      1,
      RUN_BATCH_SIZE,
      RUN_BATCH_SIZE + 1,
      2 * RUN_BATCH_SIZE,
    ];

    for (const boundary of boundaries) {
      await t.test(`activation-${boundary}`, async () => {
        const source = haltAtActivationSource(boundary);
        const sync = observeSync(source);
        const worker = await observeHandle(source);

        assert.deepStrictEqual(sync, {
          result: {
            rulesFired: boundary,
            haltReason: HaltReason.HaltRequested,
          },
          halted: true,
          agendaSize: 1,
          stateFacts: [{ relation: "position", fields: [boundary - 1] }],
        });
        assert.deepStrictEqual(worker, sync);
      });
    }
  },
);

test(
  "D-009 EngineHandle exact caller limit wins at a pending halt boundary",
  { timeout: 10_000 },
  async () => {
    const source = haltAtActivationSource(RUN_BATCH_SIZE);
    const sync = observeSync(source, RUN_BATCH_SIZE);
    const worker = await observeHandle(source, RUN_BATCH_SIZE);

    assert.deepStrictEqual(sync, {
      result: {
        rulesFired: RUN_BATCH_SIZE,
        haltReason: HaltReason.LimitReached,
      },
      halted: true,
      agendaSize: 1,
      stateFacts: [{ relation: "position", fields: [RUN_BATCH_SIZE - 1] }],
    });
    assert.deepStrictEqual(worker, sync);
  },
);

test(
  "D-009 a later EngineHandle.run starts a fresh logical run",
  { timeout: 10_000 },
  async () => {
    const source = haltAtActivationSource(1);
    const handle = await EngineHandle.create({ source });
    try {
      assert.deepStrictEqual(await handle.run(), {
        rulesFired: 1,
        haltReason: HaltReason.HaltRequested,
      });
      assert.strictEqual(await handle.getIsHalted(), true);
      assert.strictEqual(await handle.getAgendaSize(), 1);

      assert.deepStrictEqual(await handle.run(), {
        rulesFired: 1,
        haltReason: HaltReason.AgendaEmpty,
      });
      assert.strictEqual(await handle.getIsHalted(), false);
      assert.strictEqual(await handle.getAgendaSize(), 0);
      assert.deepStrictEqual(logicalRunStateFacts(await handle.facts()), [
        { relation: "past-boundary", fields: [] },
      ]);
    } finally {
      await handle.close();
    }
  },
);

test(
  "D-006 EngineHandle.run({limit:0}) starts a fresh logical run",
  { timeout: 10_000 },
  async () => {
    const handle = await EngineHandle.create({
      source: HALT_WITH_DIAGNOSTIC_SOURCE,
    });
    try {
      assert.deepStrictEqual(await handle.run(), {
        rulesFired: 1,
        haltReason: HaltReason.HaltRequested,
      });
      assert.strictEqual(await handle.getIsHalted(), true);
      assert.strictEqual(await handle.getAgendaSize(), 1);

      assert.deepStrictEqual(await handle.run({ limit: 0 }), {
        rulesFired: 0,
        haltReason: HaltReason.LimitReached,
      });
      assert.strictEqual(await handle.getIsHalted(), false);
      assert.strictEqual(
        await handle.getAgendaSize(),
        1,
        "a fresh zero-limit run must not consume the pending activation",
      );
    } finally {
      await handle.close();
    }
  },
);
