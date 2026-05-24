/**
 * EngineHandle public API completion tests.
 *
 * The existing worker suite covers the primary behavior. These cases fill in
 * public methods whose bodies were otherwise only indirectly exercised.
 */
import { test } from "node:test";
import * as assert from "node:assert/strict";
import { mkdtempSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

import {
  EngineHandle,
  FactType,
  FerricSymbol,
  HaltReason,
} from "../../../helpers/ferric";

// ---------------------------------------------------------------------------
// D-001 manual API sweep: loadFile, step, halt, clear, and introspection
// ---------------------------------------------------------------------------
test("D-001 EngineHandle public methods cover loadFile/step/halt/clear/introspection", async () => {
  const dir = mkdtempSync(join(tmpdir(), "ferric-handle-api-"));
  const sourcePath = join(dir, "rules.clp");
  writeFileSync(
    sourcePath,
    `
(deftemplate person (slot name))
(defglobal ?*status* = active)
(defrule greet (person (name ?name)) => (printout t "hi " ?name crlf))
`,
    "utf8",
  );

  const handle = await EngineHandle.create();
  try {
    // loadFile proves the file-loading method is wired, then reset activates
    // the loaded constructs for the rest of the public-method sweep.
    await handle.loadFile(sourcePath);
    await handle.reset();
    const id = await handle.assertTemplate("person", {
      name: new FerricSymbol("Ada"),
    });
    assert.strictEqual(typeof id, "number");

    assert.strictEqual(await handle.getFactCount(), 1);
    assert.strictEqual(await handle.getIsHalted(), false);
    assert.strictEqual(await handle.getCurrentModule(), "MAIN");
    assert.strictEqual(await handle.getFocus(), "MAIN");
    assert.ok((await handle.getFocusStack()).includes("MAIN"));
    assert.ok((await handle.rules()).some((rule) => rule.name === "greet"));
    assert.ok((await handle.templates()).includes("person"));
    assert.ok((await handle.modules()).includes("MAIN"));
    assert.ok((await handle.getGlobal("status")) instanceof FerricSymbol);
    assert.strictEqual(await handle.getGlobal("missing"), null);

    const fired = await handle.step();
    assert.strictEqual(typeof fired?.ruleName, "string");
    assert.strictEqual(await handle.getAgendaSize(), 0);
    assert.match(await handle.getOutput("t") as string, /hi Ada/);

    await handle.halt();
    assert.strictEqual(await handle.getIsHalted(), true);

    await handle.clear();
    assert.deepStrictEqual(await handle.rules(), []);
    assert.deepStrictEqual(await handle.templates(), []);

    // load() is the in-memory sibling of loadFile(); this success path is
    // separate from the parse-error tests that exercise rejected loads.
    await handle.load("(defrule loaded-from-string (initial-fact) =>)");
    assert.ok((await handle.rules()).some((rule) => rule.name === "loaded-from-string"));
  } finally {
    await handle.close();
  }
});

// ---------------------------------------------------------------------------
// D-001 property-style API table: fact operations preserve documented shapes
// ---------------------------------------------------------------------------
test("D-001 property-style EngineHandle fact operations preserve documented shapes", async () => {
  const handle = await EngineHandle.create({
    source: "(defrule item-rule (item ?v) =>)",
  });
  try {
    await handle.reset();

    // Generated fact cases exercise the same round-trip property over several
    // value kinds: assert -> getFact/findFacts/facts returns a valid Fact.
    const cases = [
      { relation: "item", value: 1 },
      { relation: "item", value: "text" },
      { relation: "item", value: new FerricSymbol("symbolic") },
      { relation: "item", value: [1, new FerricSymbol("nested")] },
    ];

    for (const item of cases) {
      const id = await handle.assertFact(item.relation, item.value);
      const fact = await handle.getFact(id) as any;
      assert.strictEqual(fact.id, id);
      assert.strictEqual(fact.type, FactType.Ordered);
      assert.strictEqual(fact.relation, item.relation);
      assert.ok(Array.isArray(fact.fields));
    }

    const facts = await handle.findFacts("item");
    assert.strictEqual(facts.length, cases.length);
    assert.strictEqual((await handle.facts()).filter((f) => f.relation === "item").length, cases.length);

    const retractedId = (facts[0] as any).id;
    await handle.retract(retractedId);
    assert.strictEqual(await handle.getFact(retractedId), null);
  } finally {
    await handle.close();
  }
});

// ---------------------------------------------------------------------------
// D-001 manual API: pushInput feeds readline in a worker-backed engine
// ---------------------------------------------------------------------------
test("D-001 EngineHandle.pushInput feeds readline", async () => {
  const handle = await EngineHandle.create({
    source: "(defrule read-it (initial-fact) => (printout t (readline) crlf))",
  });
  try {
    // pushInput exists for rule I/O; this proves a queued line is consumed by
    // readline instead of the worker returning EOF.
    await handle.pushInput("queued input");
    await handle.reset();
    const result = await handle.run();
    assert.strictEqual(result.haltReason, HaltReason.AgendaEmpty);
    assert.match(await handle.getOutput("t") as string, /queued input/);
  } finally {
    await handle.close();
  }
});

// ---------------------------------------------------------------------------
// F-003 manual lifecycle: asyncDispose delegates to close
// ---------------------------------------------------------------------------
test("F-003 EngineHandle Symbol.asyncDispose delegates to close", async () => {
  const handle = await EngineHandle.create();
  await handle[Symbol.asyncDispose]();
  await assert.rejects(
    () => handle.run(),
    /EngineHandle has been closed/,
  );
});
