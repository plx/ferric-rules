/**
 * Lossless fact-ID tests for the synchronous Node API (FR-NODE-001).
 */
import { test } from "node:test";
import * as assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { resolve } from "node:path";

import {
  Engine,
  FactType,
} from "../../../helpers/ferric";
import type { Fact } from "../../../helpers/ferric";

const UNSAFE_NUMBER_ERROR =
  /fact id number must be a non-negative safe integer; pass a bigint for 64-bit IDs/;
const BIGINT_RANGE_ERROR =
  /fact id bigint must be in the unsigned 64-bit range/;
const WRONG_TYPE_ERROR = /fact id must be a bigint or number/;

test("B-010 all synchronous fact-ID producers and snapshots use bigint", () => {
  const engine = new Engine();
  try {
    engine.load("(deftemplate item (slot value))");
    engine.reset();

    const stringIds = engine.assertString("(from-string 1)(from-string 2)");
    assert.strictEqual(stringIds.length, 2);
    assert.ok(stringIds.every((id) => typeof id === "bigint"));

    const orderedId = engine.assertFact("ordered", 3);
    const templateId = engine.assertTemplate("item", { value: 4 });
    assert.strictEqual(typeof orderedId, "bigint");
    assert.strictEqual(typeof templateId, "bigint");

    const ordered = engine.getFact(orderedId) as Fact | null;
    const template = engine.getFact(templateId) as Fact | null;
    assert.strictEqual(ordered?.id, orderedId);
    assert.strictEqual(template?.id, templateId);
    assert.strictEqual(template?.type, FactType.Template);
    assert.strictEqual(engine.getFactSlot(templateId, "value"), 4);

    const allFacts = engine.facts() as Fact[];
    assert.ok(allFacts.every((fact) => typeof fact.id === "bigint"));
    assert.ok(allFacts.some((fact) => fact.id === orderedId));

    const found = engine.findFacts("ordered") as Fact[];
    assert.deepStrictEqual(found.map((fact) => fact.id), [orderedId]);
  } finally {
    engine.close();
  }
});

test("B-011 safe legacy number IDs remain accepted without changing bigint outputs", () => {
  const engine = new Engine();
  try {
    engine.load("(deftemplate item (slot value))");
    engine.reset();

    const id = engine.assertTemplate("item", { value: 7 });
    const legacyNumberId = Number(id);
    assert.ok(Number.isSafeInteger(legacyNumberId));

    const fact = engine.getFact(legacyNumberId) as Fact | null;
    assert.strictEqual(fact?.id, id);
    assert.strictEqual(engine.getFactSlot(legacyNumberId, "value"), 7);
    engine.retract(legacyNumberId);
    assert.strictEqual(engine.getFact(id), null);
  } finally {
    engine.close();
  }
});

test("B-011 invalid fact-ID kinds and ranges are rejected deliberately", () => {
  const engine = new Engine();
  try {
    engine.reset();

    const consumers: Array<{
      name: string;
      run: (value: unknown) => unknown;
    }> = [
      {
        name: "getFact",
        run: (value) => engine.getFact(value as bigint),
      },
      {
        name: "retract",
        run: (value) => engine.retract(value as bigint),
      },
      {
        name: "getFactSlot",
        run: (value) => engine.getFactSlot(value as bigint, "value"),
      },
    ];

    for (const value of [
      Number.MAX_SAFE_INTEGER + 1,
      -1,
      1.5,
      Number.NaN,
      Number.POSITIVE_INFINITY,
    ]) {
      for (const consumer of consumers) {
        assert.throws(
          () => consumer.run(value),
          UNSAFE_NUMBER_ERROR,
          `${consumer.name} must reject unsafe number ${String(value)}`,
        );
      }
    }

    for (const value of [-1n, 2n ** 64n]) {
      for (const consumer of consumers) {
        assert.throws(
          () => consumer.run(value),
          BIGINT_RANGE_ERROR,
          `${consumer.name} must reject out-of-range bigint ${value}`,
        );
      }
    }

    for (const consumer of consumers) {
      // The public type rejects this call; the adapter deliberately exercises
      // the native runtime boundary for untyped JavaScript consumers.
      assert.throws(
        () => consumer.run("4294967297"),
        WRONG_TYPE_ERROR,
        `${consumer.name} must reject string IDs`,
      );
    }
  } finally {
    engine.close();
  }
});

test("B-011 bigint conversion is accepted below, at, and above the safe-number boundary", () => {
  const engine = new Engine();
  try {
    engine.reset();
    const maxSafe = BigInt(Number.MAX_SAFE_INTEGER);
    const boundaries = [
      maxSafe - 1n,
      maxSafe,
      maxSafe + 1n,
      (2n ** 64n) - 1n,
    ];

    for (const id of boundaries) {
      assert.strictEqual(engine.getFact(id), null, `${id}n must be accepted losslessly`);
    }
    assert.strictEqual(engine.getFact(Number.MAX_SAFE_INTEGER), null);
    assert.throws(
      () => engine.getFact(Number.MAX_SAFE_INTEGER + 1),
      UNSAFE_NUMBER_ERROR,
    );
  } finally {
    engine.close();
  }
});

test("B-010 high-generation fact IDs round-trip exactly in a Node subprocess", () => {
  const packageEntry = resolve(__dirname, "../../../../dist/index.js");
  const script = `
    const assert = require("node:assert/strict");
    const { Engine, Format } = require(${JSON.stringify(packageEntry)});
    const iterations = 1_048_577;
    const engine = new Engine();
    let id;
    try {
      for (let index = 0; index < iterations; index += 1) {
        id = engine.assertFact("generation");
        if (index + 1 < iterations) engine.retract(id);
      }

      assert.ok(id > BigInt(Number.MAX_SAFE_INTEGER));
      assert.strictEqual(typeof id, "bigint");
      assert.strictEqual(structuredClone(id), id);
      assert.strictEqual(engine.getFact(id).id, id);
      assert.deepStrictEqual(engine.facts().map((fact) => fact.id), [id]);
      assert.deepStrictEqual(engine.findFacts("generation").map((fact) => fact.id), [id]);

      for (const format of [Format.Bincode, Format.Json]) {
        const restored = Engine.fromSnapshot(engine.serialize(format), format);
        try {
          assert.strictEqual(restored.getFact(id).id, id);
          restored.retract(id);
          assert.strictEqual(restored.getFact(id), null);
        } finally {
          restored.close();
        }
      }

      engine.retract(id);
      assert.strictEqual(engine.getFact(id), null);
    } finally {
      engine.close();
    }

    // A template-loaded engine reserves a second fact slot. Churn that slot to
    // the same generation boundary and prove getFactSlot accepts its returned
    // above-safe-range bigint without a second cross-process fixture.
    const templateEngine = new Engine();
    try {
      templateEngine.load("(deftemplate high-template (slot value))");
      let templateId;
      for (let index = 0; index < iterations; index += 1) {
        templateId = templateEngine.assertTemplate("high-template", { value: 42 });
        if (index + 1 < iterations) templateEngine.retract(templateId);
      }
      assert.ok(templateId > BigInt(Number.MAX_SAFE_INTEGER));
      assert.strictEqual(templateEngine.getFact(templateId).id, templateId);
      assert.strictEqual(templateEngine.getFactSlot(templateId, "value"), 42);
      templateEngine.retract(templateId);
      assert.strictEqual(templateEngine.getFact(templateId), null);
    } finally {
      templateEngine.close();
    }

    process.stdout.write("ok");
  `;

  const childEnv = { ...process.env };
  // The aggregate coverage lane already instruments this test process. The
  // high-generation child is an isolation/reproduction boundary, not another
  // coverage shard; inheriting these variables contaminates the parent's
  // merged coverage and can also alter slot allocation through test bootstrap.
  // Node's test runner re-injects its coverage directory when the key is
  // absent, so an explicit empty value is required to opt this child out.
  childEnv.NODE_V8_COVERAGE = "";
  delete childEnv.NODE_TEST_CONTEXT;

  const child = spawnSync(process.execPath, ["-e", script], {
    encoding: "utf8",
    env: childEnv,
    timeout: 30_000,
  });

  assert.strictEqual(
    child.status,
    0,
    `high-generation subprocess failed:\nstdout: ${child.stdout}\nstderr: ${child.stderr}`,
  );
  assert.strictEqual(child.stdout, "ok");
});
