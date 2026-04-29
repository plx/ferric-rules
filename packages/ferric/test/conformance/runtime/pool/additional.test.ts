/**
 * Additional EnginePool tests.
 *
 * Covers: E-007 (proxy operation parity), E-002 variants, multi-thread,
 * and do() callback completion semantics.
 */
import { test } from "node:test";
import * as assert from "node:assert/strict";

import {
  EnginePool,
  FerricSymbol,
  HaltReason,
} from "../../../helpers/ferric";

const BASIC_RULE = `
(defrule hello-world
  (initial-fact)
  =>
  (printout t "hello from pool" crlf))
`;

const LOOP_RULE = `
(defrule loop
  ?f <- (counter ?n)
  =>
  (retract ?f)
  (assert (counter (+ ?n 1))))
`;

const TEMPLATE_SRC = `
(deftemplate item
  (slot id)
  (slot label))

(defrule show-item
  (item (id ?i) (label ?l))
  =>
  (printout t "item=" ?i " label=" ?l crlf))
`;

// ---------------------------------------------------------------------------
// E-007: proxy run matches Engine semantics
// ---------------------------------------------------------------------------
test("E-007 proxy run matches Engine semantics for basic rule", async () => {
  const pool = await EnginePool.create(
    [{ name: "basic", source: BASIC_RULE }],
    { threads: 1 },
  );
  try {
    const result = await pool.do("basic", async (proxy) => {
      await proxy.reset();
      return proxy.run();
    });
    assert.strictEqual(result.rulesFired, 1);
    assert.strictEqual(result.haltReason, HaltReason.AgendaEmpty);
  } finally {
    await pool.close();
  }
});

// ---------------------------------------------------------------------------
// E-007: proxy assertFact and retract work
// ---------------------------------------------------------------------------
test("E-007 proxy assertFact and retract work correctly", async () => {
  const pool = await EnginePool.create(
    [{ name: "basic", source: "(defrule x (color ?c) =>)" }],
    { threads: 1 },
  );
  try {
    const factId = await pool.do("basic", async (proxy) => {
      await proxy.reset();
      const id = await proxy.assertFact("color", new FerricSymbol("red"));
      return id;
    });
    assert.strictEqual(typeof factId, "number", "assertFact should return a number ID");

    // Retract the fact in a fresh do() callback.
    await pool.do("basic", async (proxy) => {
      await proxy.reset();
      const id = await proxy.assertFact("color", new FerricSymbol("blue"));
      await proxy.retract(id);
      const facts = await proxy.facts() as any[];
      const colorFacts = facts.filter((f: any) => f.relation === "color");
      assert.strictEqual(colorFacts.length, 0, "Retracted fact should not appear in facts()");
    });
  } finally {
    await pool.close();
  }
});

// ---------------------------------------------------------------------------
// E-007: proxy assertTemplate works
// ---------------------------------------------------------------------------
test("E-007 proxy assertTemplate populates template slots", async () => {
  const pool = await EnginePool.create(
    [{ name: "tmpl", source: TEMPLATE_SRC }],
    { threads: 1 },
  );
  try {
    const result = await pool.do("tmpl", async (proxy) => {
      await proxy.reset();
      await proxy.assertTemplate("item", { id: 42, label: new FerricSymbol("widget") });
      return proxy.run();
    });
    assert.ok(result.rulesFired >= 1, `Expected >=1 firings, got ${result.rulesFired}`);
  } finally {
    await pool.close();
  }
});

// ---------------------------------------------------------------------------
// E-007: proxy assertString and facts() work
// ---------------------------------------------------------------------------
test("E-007 proxy assertString and facts round-trip work", async () => {
  const pool = await EnginePool.create(
    [{ name: "basic", source: "(defrule x (data ?v) =>)" }],
    { threads: 1 },
  );
  try {
    const facts = await pool.do("basic", async (proxy) => {
      await proxy.reset();
      await proxy.assertString("(data 1)(data 2)(data 3)");
      return proxy.facts();
    }) as any[];
    const dataFacts = facts.filter((f: any) => f.relation === "data");
    assert.strictEqual(dataFacts.length, 3, `Expected 3 data facts, got ${dataFacts.length}`);
  } finally {
    await pool.close();
  }
});

// ---------------------------------------------------------------------------
// E-007: proxy getOutput and clearOutput work
// ---------------------------------------------------------------------------
test("E-007 proxy getOutput and clearOutput work correctly", async () => {
  const pool = await EnginePool.create(
    [{ name: "basic", source: BASIC_RULE }],
    { threads: 1 },
  );
  try {
    await pool.do("basic", async (proxy) => {
      await proxy.reset();
      await proxy.run();
      const output = await proxy.getOutput("t");
      assert.ok(output?.includes("hello from pool"), `Missing output: ${output}`);
      await proxy.clearOutput("t");
      const cleared = await proxy.getOutput("t");
      assert.ok(
        !cleared || cleared.length === 0,
        `Expected empty output after clear, got: ${cleared}`
      );
    });
  } finally {
    await pool.close();
  }
});

// ---------------------------------------------------------------------------
// E-007: proxy step fires exactly one rule
// ---------------------------------------------------------------------------
test("E-007 proxy step fires exactly one rule", async () => {
  const pool = await EnginePool.create(
    [{ name: "basic", source: LOOP_RULE }],
    { threads: 1 },
  );
  try {
    const result = await pool.do("basic", async (proxy) => {
      await proxy.reset();
      await proxy.assertString("(counter 0)");
      return proxy.step();
    });
    // step() returns FiredRule | null; a non-null result means exactly one rule fired.
    assert.ok(result !== null, "step() should have fired one rule");
    assert.strictEqual(typeof (result as any).ruleName, "string", "FiredRule should have a ruleName");
  } finally {
    await pool.close();
  }
});

// ---------------------------------------------------------------------------
// E-007: proxy getFact retrieves a specific fact by ID
// ---------------------------------------------------------------------------
test("E-007 proxy getFact retrieves a specific fact by ID", async () => {
  const pool = await EnginePool.create(
    [{ name: "basic", source: "(defrule x (item ?v) =>)" }],
    { threads: 1 },
  );
  try {
    const fact = await pool.do("basic", async (proxy) => {
      await proxy.reset();
      const id = await proxy.assertFact("item", 99);
      return proxy.getFact(id);
    }) as any;
    assert.ok(fact, "getFact should return a fact");
    assert.strictEqual(typeof fact.id, "number");
    assert.ok(Array.isArray(fact.fields));
  } finally {
    await pool.close();
  }
});

// ---------------------------------------------------------------------------
// E-002 additional variant: evaluate with template facts
// ---------------------------------------------------------------------------
test("E-002 evaluate with template facts fires matching rules", async () => {
  const pool = await EnginePool.create(
    [{ name: "tmpl", source: TEMPLATE_SRC }],
    { threads: 1 },
  );
  try {
    const result = await pool.evaluate("tmpl", {
      facts: [
        {
          kind: "template",
          templateName: "item",
          slots: { id: 7, label: new FerricSymbol("gadget") },
        },
      ],
    });
    assert.ok(result.runResult.rulesFired >= 1);
    assert.ok(result.output.stdout?.includes("item=7"), `Missing item=7 in output: ${result.output.stdout}`);
  } finally {
    await pool.close();
  }
});

// ---------------------------------------------------------------------------
// Multi-thread pool: requests are distributed and all complete
// ---------------------------------------------------------------------------
test("E-001 multi-thread pool with threads:2 handles concurrent requests", async () => {
  const pool = await EnginePool.create(
    [{ name: "basic", source: BASIC_RULE }],
    { threads: 2 },
  );
  try {
    // Submit 4 requests to a 2-thread pool and verify all complete.
    const results = await Promise.all([
      pool.evaluate("basic", {}),
      pool.evaluate("basic", {}),
      pool.evaluate("basic", {}),
      pool.evaluate("basic", {}),
    ]);
    for (const r of results) {
      assert.strictEqual(r.runResult.rulesFired, 1);
      assert.strictEqual(r.runResult.haltReason, HaltReason.AgendaEmpty);
    }
  } finally {
    await pool.close();
  }
});

// ---------------------------------------------------------------------------
// do() callback return value is the resolved result
// ---------------------------------------------------------------------------
test("E-007 do() callback return value is forwarded as the resolved result", async () => {
  const pool = await EnginePool.create(
    [{ name: "basic", source: "(defrule x (data ?v) =>)" }],
    { threads: 1 },
  );
  try {
    const value = await pool.do("basic", async (_proxy) => {
      // The callback can return any value — the pool forwards it.
      return 42 as unknown as any;
    });
    assert.strictEqual(value, 42);
  } finally {
    await pool.close();
  }
});

// ---------------------------------------------------------------------------
// E-007: proxy load adds rules dynamically within a do() session
// ---------------------------------------------------------------------------
test("E-007 proxy load adds rules dynamically within a do() session", async () => {
  const pool = await EnginePool.create(
    [{ name: "basic", source: "" }],
    { threads: 1 },
  );
  try {
    const result = await pool.do("basic", async (proxy) => {
      // Load a rule dynamically inside the do() session.
      await proxy.load("(defrule dynamic (initial-fact) => (printout t \"dynamic\" crlf))");
      await proxy.reset();
      return proxy.run();
    });
    assert.strictEqual(result.rulesFired, 1);
    assert.strictEqual(result.haltReason, HaltReason.AgendaEmpty);
  } finally {
    await pool.close();
  }
});

// ---------------------------------------------------------------------------
// E-007: proxy run with limit caps firings
// ---------------------------------------------------------------------------
test("E-007 proxy run with limit caps firings correctly", async () => {
  const pool = await EnginePool.create(
    [{ name: "loop", source: LOOP_RULE }],
    { threads: 1 },
  );
  try {
    const result = await pool.do("loop", async (proxy) => {
      await proxy.reset();
      await proxy.assertFact("counter", 0);
      return proxy.run({ limit: 7 });
    });
    assert.strictEqual(result.rulesFired, 7);
    assert.strictEqual(result.haltReason, HaltReason.LimitReached);
  } finally {
    await pool.close();
  }
});
