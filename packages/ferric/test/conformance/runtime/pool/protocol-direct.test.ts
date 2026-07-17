/**
 * Direct EnginePool worker-protocol tests.
 *
 * The public EnginePool proxy deliberately exposes only a subset of the worker
 * switch. These tests cover the remaining protocol branches directly.
 */
import { test } from "node:test";
import * as assert from "node:assert/strict";
import { resolve } from "node:path";
import { Worker } from "node:worker_threads";

import {
  FactType,
  HaltReason,
  FerricSymbol,
} from "../../../helpers/ferric";

interface WorkerResponse {
  id: number;
  result?: unknown;
  error?: { name: string; message: string; code: string };
}

function poolWorkerPath(): string {
  return resolve(__dirname, "../../../../dist/pool-worker.js");
}

function request(
  worker: Worker,
  method: string,
  args: unknown[],
): Promise<unknown> {
  const id = request.nextId++;
  return new Promise((resolveRequest, reject) => {
    const onMessage = (resp: WorkerResponse) => {
      if (resp.id !== id) return;
      cleanup();
      if (resp.error) {
        reject(Object.assign(new Error(resp.error.message), resp.error));
      } else {
        resolveRequest(resp.result);
      }
    };
    const onError = (err: Error) => {
      cleanup();
      reject(err);
    };
    const cleanup = () => {
      worker.off("message", onMessage);
      worker.off("error", onError);
    };
    worker.on("message", onMessage);
    worker.on("error", onError);
    worker.postMessage({ id, method, args });
  });
}
request.nextId = 1;

async function withPoolWorker<T>(
  fn: (worker: Worker) => Promise<T>,
): Promise<T> {
  const worker = new Worker(poolWorkerPath());
  try {
    return await fn(worker);
  } finally {
    await worker.terminate();
  }
}

const SOURCE = `
(deftemplate item (slot id) (slot label))
(defglobal ?*status* = running)
(defrule show-item
  (item (id ?id) (label ?label))
  =>
  (printout t "item=" ?id ":" ?label crlf)
  (printout stderr "seen" crlf))
`;

// ---------------------------------------------------------------------------
// E-007 direct pool protocol: method table preserves result shapes
// ---------------------------------------------------------------------------
test("E-007 table-driven direct pool worker method table preserves result shapes", async () => {
  await withPoolWorker(async (worker) => {
    await request(worker, "__init", [{
      specs: [{ name: "rules", source: SOURCE }],
    }]);

    const factId = await request(worker, "assertTemplate", [
      "rules",
      "item",
      { id: 1, label: { __type: "FerricSymbol", value: "widget" } },
    ]);
    assert.strictEqual(typeof factId, "number");

    // This table checks the same structured-clone contract across
    // the less frequently used pool-worker methods.
    const cases: Array<{
      label: string;
      method: string;
      args: unknown[];
      verify: (value: unknown) => void;
    }> = [
      {
        label: "getFact returns the requested template fact",
        method: "getFact",
        args: ["rules", factId],
        verify: (value) => {
          assert.strictEqual((value as any).type, FactType.Template);
          assert.strictEqual((value as any).templateName, "item");
        },
      },
      {
        label: "facts returns all facts",
        method: "facts",
        args: ["rules"],
        verify: (value) => assert.ok((value as any[]).some((f) => f.templateName === "item")),
      },
      {
        label: "findFacts filters by relation",
        method: "findFacts",
        args: ["rules", "item"],
        verify: (value) => assert.ok((value as any[]).every((f) => f.templateName === "item")),
      },
      {
        label: "getFactCount returns a number",
        method: "getFactCount",
        args: ["rules"],
        verify: (value) => assert.strictEqual(typeof value, "number"),
      },
      {
        label: "getIsHalted returns a boolean",
        method: "getIsHalted",
        args: ["rules"],
        verify: (value) => assert.strictEqual(typeof value, "boolean"),
      },
      {
        label: "getAgendaSize returns a number",
        method: "getAgendaSize",
        args: ["rules"],
        verify: (value) => assert.strictEqual(typeof value, "number"),
      },
      {
        label: "getCurrentModule returns MAIN",
        method: "getCurrentModule",
        args: ["rules"],
        verify: (value) => assert.strictEqual(value, "MAIN"),
      },
      {
        label: "getFocus returns current focus",
        method: "getFocus",
        args: ["rules"],
        verify: (value) => assert.strictEqual(value, "MAIN"),
      },
      {
        label: "getFocusStack returns a stack",
        method: "getFocusStack",
        args: ["rules"],
        verify: (value) => assert.ok((value as string[]).includes("MAIN")),
      },
      {
        label: "rules lists rule metadata",
        method: "rules",
        args: ["rules"],
        verify: (value) => assert.ok((value as any[]).some((r) => r.name === "show-item")),
      },
      {
        label: "templates lists template names",
        method: "templates",
        args: ["rules"],
        verify: (value) => assert.ok((value as string[]).includes("item")),
      },
      {
        label: "modules lists MAIN",
        method: "modules",
        args: ["rules"],
        verify: (value) => assert.ok((value as string[]).includes("MAIN")),
      },
      {
        label: "getGlobal returns a value",
        method: "getGlobal",
        args: ["rules", "status"],
        verify: (value) => assert.deepStrictEqual(value, { __type: "FerricSymbol", value: "running" }),
      },
    ];

    for (const item of cases) {
      const value = await request(worker, item.method, item.args);
      assert.doesNotThrow(
        () => item.verify(value),
        `${item.label} should satisfy protocol shape`,
      );
    }

    const runResult = await request(worker, "__batched_run", [
      "rules",
      null,
      null,
    ]) as any;
    assert.strictEqual(runResult.haltReason, HaltReason.AgendaEmpty);

    assert.match(await request(worker, "getOutput", ["rules", "t"]) as string, /item=1/);
    assert.match(await request(worker, "getOutput", ["rules", "stderr"]) as string, /seen/);
    await request(worker, "clearOutput", ["rules", "t"]);
    assert.strictEqual(await request(worker, "getOutput", ["rules", "t"]), null);

    await request(worker, "halt", ["rules"]);
    assert.strictEqual(await request(worker, "getIsHalted", ["rules"]), true);
    await request(worker, "clear", ["rules"]);
    assert.deepStrictEqual(await request(worker, "rules", ["rules"]), []);

    await request(worker, "load", [
      "rules",
      "(defrule read-it (need-input) => (printout t (readline) crlf))",
    ]);
    await request(worker, "reset", ["rules"]);
    await request(worker, "pushInput", ["rules", "from pool worker"]);
    await request(worker, "assertFact", ["rules", "need-input"]);
    await request(worker, "run", ["rules", null]);
    assert.match(await request(worker, "getOutput", ["rules", "t"]) as string, /from pool worker/);

    const snapshot = await request(worker, "serialize", ["rules"]);
    assert.ok(snapshot instanceof ArrayBuffer);
    assert.ok(snapshot.byteLength > 0);
  });
});

// ---------------------------------------------------------------------------
// E-002 direct evaluate: output maps both stdout and stderr and clears channels
// ---------------------------------------------------------------------------
test("E-002 direct pool evaluate returns stdout/stderr and clears worker output", async () => {
  await withPoolWorker(async (worker) => {
    await request(worker, "__init", [{
      specs: [{ name: "rules", source: SOURCE }],
    }]);

    // The direct evaluate path is one round-trip reset/assert/run/collect; this
    // explicit case verifies stderr is surfaced as well as stdout.
    const result = await request(worker, "__evaluate", [
      "rules",
      {
        facts: [{
          kind: "template",
          templateName: "item",
          slots: { id: 2, label: { __type: "FerricSymbol", value: "gadget" } },
        }],
      },
      null,
    ]) as any;

    assert.strictEqual(result.runResult.rulesFired, 1);
    assert.match(result.output.stdout, /item=2/);
    assert.match(result.output.stderr, /seen/);

    // handleEvaluate clears output channels after collecting them so later
    // stateful protocol calls do not inherit stale text.
    assert.strictEqual(await request(worker, "getOutput", ["rules", "t"]), null);
    assert.strictEqual(await request(worker, "getOutput", ["rules", "stderr"]), null);
  });
});

// ---------------------------------------------------------------------------
// E-007 direct pool protocol: diagnostics and unknown errors are structured
// ---------------------------------------------------------------------------
test("E-007 direct pool worker diagnostics and unknown errors are structured", async () => {
  await withPoolWorker(async (worker) => {
    await request(worker, "__init", [{
      specs: [{
        name: "rules",
        source: "(defrule bad-channel (channel ?ch) => (printout ?ch \"bad\" crlf))",
      }],
    }]);

    await request(worker, "assertFact", ["rules", "channel", "dynamic"]);
    await request(worker, "__batched_run", ["rules", 1, null]);

    // Dynamic printout channel errors are accumulated as non-fatal diagnostics;
    // this covers the diagnostics protocol without relying on a thrown error.
    const diagnostics = await request(worker, "getDiagnostics", ["rules"]) as string[];
    assert.ok(
      diagnostics.some((message) => message.includes("printout")),
      `expected printout diagnostic, got ${diagnostics.join("; ")}`,
    );
    await request(worker, "clearDiagnostics", ["rules"]);
    assert.deepStrictEqual(await request(worker, "getDiagnostics", ["rules"]), []);

    await assert.rejects(
      () => request(worker, "not-a-method", ["rules"]),
      (err: any) => {
        assert.strictEqual(err.name, "Error");
        assert.strictEqual(err.code, "FERRIC_ERROR");
        assert.match(err.message, /Unknown method/);
        return true;
      },
    );

    await assert.rejects(
      () => request(worker, "facts", ["missing"]),
      (err: any) => {
        assert.strictEqual(err.name, "Error");
        assert.match(err.message, /Unknown engine spec/);
        return true;
      },
    );
  });
});

// ---------------------------------------------------------------------------
// B-002 direct pool protocol: wire symbols are accepted in table-driven facts
// ---------------------------------------------------------------------------
test("B-002 table-driven direct pool worker accepts wire-symbol facts", async () => {
  await withPoolWorker(async (worker) => {
    await request(worker, "__init", [{
      specs: [{
        name: "rules",
        source: "(defrule color (color ?c) => (printout t ?c crlf))",
      }],
    }]);

    // A fixed corpus of symbols exercise the same conversion
    // property over multiple values without adding flake-prone randomness.
    const colors = ["red", "green", "blue", "amber"];
    for (const color of colors) {
      await request(worker, "assertFact", [
        "rules",
        "color",
        { __type: "FerricSymbol", value: color },
      ]);
    }

    const result = await request(worker, "run", ["rules", null]) as any;
    assert.strictEqual(result.rulesFired, colors.length);
    const output = await request(worker, "getOutput", ["rules", "t"]) as string;
    for (const color of colors) {
      assert.match(output, new RegExp(`\\b${color}\\b`));
    }
  });
});

// ---------------------------------------------------------------------------
// E-007 table-driven direct pool run modes preserve proxy N-01 semantics
// ---------------------------------------------------------------------------
test("E-007 table-driven direct pool worker batched run covers limit modes", async () => {
  await withPoolWorker(async (worker) => {
    await request(worker, "__init", [{
      specs: [{
        name: "loop",
        source: `
(defrule loop
  ?f <- (counter ?n)
  =>
  (retract ?f)
  (assert (counter (+ ?n 1))))
`,
      }],
    }]);

    const cases = [
      {
        label: "zero limit fires nothing",
        prepare: async () => {
          await request(worker, "reset", ["loop"]);
          await request(worker, "assertFact", ["loop", "counter", 0]);
          return [0, null] as const;
        },
        verify: (result: any) => {
          assert.strictEqual(result.rulesFired, 0);
          assert.strictEqual(result.haltReason, HaltReason.LimitReached);
        },
      },
      {
        label: "bounded limit caps firings",
        prepare: async () => {
          await request(worker, "reset", ["loop"]);
          await request(worker, "assertFact", ["loop", "counter", 0]);
          return [4, null] as const;
        },
        verify: (result: any) => {
          assert.strictEqual(result.rulesFired, 4);
          assert.strictEqual(result.haltReason, HaltReason.LimitReached);
        },
      },
      {
        label: "pre-set abort buffer returns HaltRequested",
        prepare: async () => {
          await request(worker, "reset", ["loop"]);
          await request(worker, "assertFact", ["loop", "counter", 0]);
          const sab = new SharedArrayBuffer(Int32Array.BYTES_PER_ELEMENT);
          Atomics.store(new Int32Array(sab), 0, 1);
          return [10, sab] as const;
        },
        verify: (result: any) => {
          assert.strictEqual(result.rulesFired, 0);
          assert.strictEqual(result.haltReason, HaltReason.HaltRequested);
        },
      },
    ];

    // These fixed modes mirror EngineHandle's direct protocol coverage and
    // establish the pool-worker batched run contract over its edge values.
    for (const item of cases) {
      const args = await item.prepare();
      const result = await request(worker, "__batched_run", ["loop", ...args]);
      assert.doesNotThrow(() => item.verify(result), item.label);
    }
  });
});
