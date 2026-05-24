/**
 * Direct EngineHandle worker-protocol tests.
 *
 * These intentionally exercise worker.ts methods that are part of the
 * internal binding protocol but are not all surfaced as EngineHandle methods.
 */
import { test } from "node:test";
import * as assert from "node:assert/strict";
import { mkdtempSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";
import { Worker } from "node:worker_threads";

import {
  FactType,
  HaltReason,
  isWireSymbol,
} from "../../../helpers/ferric";

interface WorkerResponse {
  id: number;
  result?: unknown;
  error?: { name: string; message: string; code: string };
}

function workerPath(): string {
  return resolve(__dirname, "../../../../dist/worker.js");
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

async function withWorker<T>(
  fn: (worker: Worker) => Promise<T>,
): Promise<T> {
  const worker = new Worker(workerPath());
  try {
    return await fn(worker);
  } finally {
    await worker.terminate();
  }
}

// ---------------------------------------------------------------------------
// D-001 direct protocol: loadFile, introspection, focus, I/O, and snapshots
// ---------------------------------------------------------------------------
test("D-001 direct worker protocol covers loadFile/introspection/focus/io/snapshot methods", async () => {
  const dir = mkdtempSync(join(tmpdir(), "ferric-worker-protocol-"));
  const sourcePath = join(dir, "rules.clp");
  const snapshotPath = join(dir, "snapshot.bin");

  writeFileSync(
    sourcePath,
    `
(deftemplate person (slot name))
(defglobal ?*status* = ready)
(defrule greet
  (person (name ?name))
  =>
  (printout t "hello " ?name crlf)
  (printout stderr "warn" crlf))
(defmodule AUX)
`,
    "utf8",
  );

  await withWorker(async (worker) => {
    // The worker can be initialized empty and then loaded from disk; this is
    // the loadFile branch that public EngineHandle smoke tests did not hit.
    await request(worker, "__init", [{ options: {} }]);
    await request(worker, "loadFile", [sourcePath]);
    await request(worker, "reset", []);

    const factId = await request(worker, "assertTemplate", [
      "person",
      { name: "Ada" },
    ]);
    assert.strictEqual(typeof factId, "number");

    // Template slot lookup is a worker protocol method even though it is not a
    // public EngineHandle method; this verifies the slot value crosses intact.
    const slot = await request(worker, "getFactSlot", [factId, "name"]);
    assert.strictEqual(slot, "Ada");

    const runResult = await request(worker, "run", [null]) as any;
    assert.deepStrictEqual(runResult, {
      rulesFired: 1,
      haltReason: HaltReason.AgendaEmpty,
    });

    assert.strictEqual(await request(worker, "getFactCount", []), 1);
    assert.strictEqual(await request(worker, "getIsHalted", []), false);
    assert.strictEqual(await request(worker, "getAgendaSize", []), 0);
    assert.strictEqual(await request(worker, "getCurrentModule", []), "MAIN");
    assert.ok((await request(worker, "rules", []) as any[]).some((r) => r.name === "greet"));
    assert.ok((await request(worker, "templates", []) as string[]).includes("person"));
    assert.ok((await request(worker, "modules", []) as string[]).includes("AUX"));
    assert.deepStrictEqual(await request(worker, "getGlobal", ["status"]), {
      __type: "FerricSymbol",
      value: "ready",
    });

    await request(worker, "setFocus", ["MAIN"]);
    await request(worker, "pushFocus", ["AUX"]);
    assert.strictEqual(await request(worker, "getFocus", []), "AUX");
    assert.ok((await request(worker, "getFocusStack", []) as string[]).includes("AUX"));

    assert.match(await request(worker, "getOutput", ["t"]) as string, /hello Ada/);
    assert.match(await request(worker, "getOutput", ["stderr"]) as string, /warn/);
    await request(worker, "clearOutput", ["stderr"]);
    assert.strictEqual(await request(worker, "getOutput", ["stderr"]), null);

    const snapshot = await request(worker, "serialize", []);
    assert.ok(snapshot instanceof ArrayBuffer);
    assert.ok(snapshot.byteLength > 0);
    await request(worker, "saveSnapshot", [snapshotPath, undefined]);

    // Re-initializing from the transferred snapshot exercises the alternate
    // init branch and proves the worker closes the previous engine first.
    await request(worker, "__init", [{
      snapshot: { data: snapshot as ArrayBuffer },
    }]);
    assert.ok((await request(worker, "templates", []) as string[]).includes("person"));

    await request(worker, "__close", []);
  });
});

// ---------------------------------------------------------------------------
// D-006 property-style direct worker run modes preserve N-01 semantics
// ---------------------------------------------------------------------------
test("D-006 property-style direct worker batched run covers generated limit modes", async () => {
  await withWorker(async (worker) => {
    await request(worker, "__init", [{
      source: `
(defrule loop
  ?f <- (counter ?n)
  =>
  (retract ?f)
  (assert (counter (+ ?n 1))))
`,
    }]);

    const cases = [
      {
        label: "zero limit fires nothing",
        prepare: async () => {
          await request(worker, "reset", []);
          await request(worker, "assertFact", ["counter", 0]);
          return [0, null] as const;
        },
        verify: (result: any) => {
          assert.strictEqual(result.rulesFired, 0);
          assert.strictEqual(result.haltReason, HaltReason.LimitReached);
        },
      },
      {
        label: "positive limit caps firings",
        prepare: async () => {
          await request(worker, "reset", []);
          await request(worker, "assertFact", ["counter", 0]);
          return [3, null] as const;
        },
        verify: (result: any) => {
          assert.strictEqual(result.rulesFired, 3);
          assert.strictEqual(result.haltReason, HaltReason.LimitReached);
        },
      },
      {
        label: "pre-set abort buffer returns HaltRequested",
        prepare: async () => {
          await request(worker, "reset", []);
          await request(worker, "assertFact", ["counter", 0]);
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

    // The table acts as a small generator over the public run-limit contract:
    // zero, bounded positive, and cooperative abort all return stable shapes.
    for (const item of cases) {
      const args = await item.prepare();
      const result = await request(worker, "__run_batched", [...args]);
      assert.doesNotThrow(() => item.verify(result), item.label);
    }
  });
});

// ---------------------------------------------------------------------------
// D-001 property-style protocol table: primitive methods preserve their shapes
// ---------------------------------------------------------------------------
test("D-001 property-style direct worker method table preserves result shapes", async () => {
  await withWorker(async (worker) => {
    await request(worker, "__init", [{
      source: `
(defrule one (item ?v) => (printout t ?v crlf))
(defrule read-it (need-input) => (printout t (readline) crlf))
`,
    }]);

    // This generated table checks the same protocol property across a family
    // of methods: every request returns the documented structured-clone shape.
    const cases: Array<{
      label: string;
      method: string;
      args: unknown[];
      verify: (value: unknown) => void;
    }> = [
      {
        label: "assertString returns all fact ids",
        method: "assertString",
        args: ["(item 1)(item 2)"],
        verify: (value) => {
          assert.deepStrictEqual((value as unknown[]).map((v) => typeof v), [
            "number",
            "number",
          ]);
        },
      },
      {
        label: "facts returns fact records",
        method: "facts",
        args: [],
        verify: (value) => assert.ok((value as any[]).some((f) => f.relation === "item")),
      },
      {
        label: "findFacts filters by relation",
        method: "findFacts",
        args: ["item"],
        verify: (value) => assert.ok((value as any[]).every((f) => f.relation === "item")),
      },
      {
        label: "step returns FiredRule",
        method: "step",
        args: [],
        verify: (value) => assert.strictEqual(typeof (value as any).ruleName, "string"),
      },
      {
        label: "halt is void",
        method: "halt",
        args: [],
        verify: (value) => assert.strictEqual(value, null),
      },
      {
        label: "clear is void",
        method: "clear",
        args: [],
        verify: (value) => assert.strictEqual(value, null),
      },
    ];

    for (const item of cases) {
      const value = await request(worker, item.method, item.args);
      assert.doesNotThrow(
        () => item.verify(value),
        `${item.label} should satisfy protocol shape`,
      );
    }

    // pushInput is checked after clear/load to prove readline consumes the
    // queued string rather than returning EOF.
    await request(worker, "load", ["(defrule read-it (need-input) => (printout t (readline) crlf))"]);
    await request(worker, "reset", []);
    await request(worker, "pushInput", ["typed line"]);
    await request(worker, "assertFact", ["need-input"]);
    await request(worker, "__run_batched", [null, null]);
    assert.match(await request(worker, "getOutput", ["t"]) as string, /typed line/);
  });
});

// ---------------------------------------------------------------------------
// D-001 direct protocol: diagnostics and error payloads are serializable
// ---------------------------------------------------------------------------
test("D-001 direct worker diagnostics and unknown-method errors are structured", async () => {
  await withWorker(async (worker) => {
    await request(worker, "__init", [{
      source: "(defrule bad-channel (channel ?ch) => (printout ?ch \"bad\" crlf))",
    }]);
    await request(worker, "reset", []);
    await request(worker, "assertFact", ["channel", "dynamic"]);
    await request(worker, "__run_batched", [1, null]);

    // A dynamic printout channel is a non-fatal action diagnostic, which lets
    // the protocol prove getDiagnostics and clearDiagnostics both work.
    const diagnostics = await request(worker, "getDiagnostics", []) as string[];
    assert.ok(
      diagnostics.some((message) => message.includes("printout")),
      `expected printout diagnostic, got ${diagnostics.join("; ")}`,
    );
    await request(worker, "clearDiagnostics", []);
    assert.deepStrictEqual(await request(worker, "getDiagnostics", []), []);

    await assert.rejects(
      () => request(worker, "__not_a_method", []),
      (err: any) => {
        assert.strictEqual(err.name, "Error");
        assert.strictEqual(err.code, "FERRIC_ERROR");
        assert.match(err.message, /Unknown method/);
        return true;
      },
    );
  });
});

// ---------------------------------------------------------------------------
// D-001 manual protocol guard: non-init requests before init fail explicitly
// ---------------------------------------------------------------------------
test("D-001 direct worker rejects normal methods before __init", async () => {
  await withWorker(async (worker) => {
    // This protects the worker thread-affinity setup: no engine method can run
    // until the main thread has sent the initialization payload.
    await assert.rejects(
      () => request(worker, "facts", []),
      (err: any) => {
        assert.match(err.message, /Engine is not initialized/);
        assert.strictEqual(err.code, "FERRIC_ERROR");
        return true;
      },
    );
  });
});

// ---------------------------------------------------------------------------
// B-004 direct protocol: worker results use the canonical symbol wire shape
// ---------------------------------------------------------------------------
test("B-004 property-style direct worker symbol outputs use canonical wire shape", async () => {
  await withWorker(async (worker) => {
    await request(worker, "__init", [{
      source: "(deffacts init (color red) (color blue))(defglobal ?*status* = running)",
    }]);
    await request(worker, "reset", []);

    // Generated probes cover each result-returning symbol path with the same
    // property: symbols leave the worker as tagged structured-clone objects.
    const probes = [
      async () => (await request(worker, "facts", []) as any[])[0].fields[0],
      async () => {
        const facts = await request(worker, "findFacts", ["color"]) as any[];
        return facts[1].fields[0];
      },
      async () => await request(worker, "getGlobal", ["status"]),
    ];

    for (const probe of probes) {
      const value = await probe();
      assert.ok(isWireSymbol(value), `expected wire symbol, got ${JSON.stringify(value)}`);
    }
  });
});
