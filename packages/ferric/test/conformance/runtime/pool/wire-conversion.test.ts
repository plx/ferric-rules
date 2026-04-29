/**
 * Pool value/wire conversion tests (B-002, B-004 via EnginePool).
 */
import { test } from "node:test";
import * as assert from "node:assert/strict";

import {
  EnginePool,
  FerricSymbol,
  HaltReason,
} from "../../../helpers/ferric";

const SYMBOL_RULE = `
(defrule detect-sym
  (color ?c)
  =>
  (printout t "found=" ?c crlf))

(deftemplate info
  (slot key)
  (slot val))

(defrule detect-template
  (info (key ?k))
  =>
  (printout t "key=" ?k crlf))
`;

// ---------------------------------------------------------------------------
// B-002: FerricSymbol input via EnginePool.evaluate
// ---------------------------------------------------------------------------
test("B-002 FerricSymbol in ordered facts via EnginePool.evaluate", async () => {
  const pool = await EnginePool.create(
    [{ name: "test", source: SYMBOL_RULE }],
    { threads: 1 },
  );
  try {
    const result = await pool.evaluate("test", {
      facts: [
        { kind: "ordered", relation: "color", fields: [new FerricSymbol("green")] },
      ],
    });
    assert.ok(result.output.stdout?.includes("found=green"));
  } finally {
    await pool.close();
  }
});

test("B-002 FerricSymbol in template slots via EnginePool.evaluate", async () => {
  const pool = await EnginePool.create(
    [{ name: "test", source: SYMBOL_RULE }],
    { threads: 1 },
  );
  try {
    const result = await pool.evaluate("test", {
      facts: [
        {
          kind: "template",
          templateName: "info",
          slots: { key: new FerricSymbol("status"), val: new FerricSymbol("active") },
        },
      ],
    });
    assert.ok(result.output.stdout?.includes("key=status"));
  } finally {
    await pool.close();
  }
});

// ---------------------------------------------------------------------------
// B-004: Symbol values in EnginePool results are FerricSymbol instances
// ---------------------------------------------------------------------------
test("B-004 symbol outputs from EnginePool.evaluate facts are FerricSymbol", async () => {
  const pool = await EnginePool.create(
    [{ name: "test", source: "(deffacts init (color red))" }],
    { threads: 1 },
  );
  try {
    const result = await pool.evaluate("test", {});
    const colorFact = result.facts.find(
      (f: any) => f.relation === "color"
    );
    assert.ok(colorFact, "color fact should exist");
    const field = (colorFact as any).fields?.[0];
    assert.ok(
      field instanceof FerricSymbol,
      `Expected FerricSymbol, got ${field?.constructor?.name}`
    );
    assert.strictEqual(field.value, "red");
  } finally {
    await pool.close();
  }
});

test("B-004 symbol outputs from EnginePool.do proxy are FerricSymbol", async () => {
  const pool = await EnginePool.create(
    [{ name: "test", source: "(deffacts init (color blue))" }],
    { threads: 1 },
  );
  try {
    const facts = await pool.do("test", async (proxy) => {
      await proxy.reset();
      return proxy.facts();
    });
    const colorFact = facts.find(
      (f: any) => f.relation === "color"
    );
    assert.ok(colorFact, "color fact should exist");
    const field = (colorFact as any).fields?.[0];
    assert.ok(
      field instanceof FerricSymbol,
      `Expected FerricSymbol, got ${field?.constructor?.name}`
    );
    assert.strictEqual(field.value, "blue");
  } finally {
    await pool.close();
  }
});
