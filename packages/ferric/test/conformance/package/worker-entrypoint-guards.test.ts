/**
 * Worker entrypoint guard tests.
 */
import { test } from "node:test";
import * as assert from "node:assert/strict";
import { EventEmitter } from "node:events";
import { dirname } from "node:path";
import { resolve } from "node:path";
import { createRequire } from "node:module";

const requireFromHere = createRequire(__filename);
const Module = requireFromHere("node:module") as any;

function withPatchedLoad<T>(
  patch: (
    request: string,
    parent: unknown,
    isMain: boolean,
    originalLoad: (request: string, parent: unknown, isMain: boolean) => unknown,
  ) => unknown,
  fn: () => T,
): T {
  const original = Module._load;
  Module._load = function patched(
    request: string,
    parent: unknown,
    isMain: boolean,
  ) {
    return patch(request, parent, isMain, original);
  };
  try {
    return fn();
  } finally {
    Module._load = original;
  }
}

// ---------------------------------------------------------------------------
// D-001 manual worker guard: worker.ts cannot be required on the main thread
// ---------------------------------------------------------------------------
test("D-001 worker entrypoint rejects main-thread require", () => {
  const path = resolve(__dirname, "../../../dist/worker.js");
  delete requireFromHere.cache[requireFromHere.resolve(path)];

  assert.throws(
    () => requireFromHere(path),
    /worker\.ts must be run as a Worker thread/,
  );
});

// ---------------------------------------------------------------------------
// E-007 manual pool-worker guard: pool-worker.ts cannot run on main thread
// ---------------------------------------------------------------------------
test("E-007 pool-worker entrypoint rejects main-thread require", () => {
  const path = resolve(__dirname, "../../../dist/pool-worker.js");
  delete requireFromHere.cache[requireFromHere.resolve(path)];

  assert.throws(
    () => requireFromHere(path),
    /pool-worker\.ts must be run as a Worker thread/,
  );
});

// ---------------------------------------------------------------------------
// D-001 manual worker init: re-init closes the previous engine defensively
// ---------------------------------------------------------------------------
test("D-001 worker re-init ignores close failures from previous engine", () => {
  const workerPath = resolve(__dirname, "../../../dist/worker.js");
  const developmentPath = resolve(
    dirname(workerPath),
    "..",
    "..",
    "..",
    "crates",
    "ferric-napi",
    "index.js",
  );

  class FakeParentPort extends EventEmitter {
    readonly responses: unknown[] = [];
    postMessage(message: unknown): void {
      this.responses.push(message);
    }
  }

  class FakeEngine {
    static fromSnapshot(): FakeEngine {
      return new FakeEngine();
    }

    close(): void {
      throw new Error("close failed");
    }
  }

  const parentPort = new FakeParentPort();
  delete requireFromHere.cache[requireFromHere.resolve(workerPath)];

  withPatchedLoad(
    (request, parent, isMain, originalLoad) => {
      if (request === "node:worker_threads") return { parentPort };
      if (request === developmentPath) {
        return {
          Engine: FakeEngine,
          FerricSymbol: class FerricSymbol {
            constructor(readonly value: string) {}
          },
        };
      }
      return originalLoad(request, parent, isMain);
    },
    () => {
      try {
        requireFromHere(workerPath);
        parentPort.emit("message", { id: 1, method: "__init", args: [{}] });
        parentPort.emit("message", { id: 2, method: "__init", args: [{}] });

        // The second init calls close() on the previous engine, catches the
        // synthetic failure, and still acknowledges initialization.
        assert.deepStrictEqual(parentPort.responses, [
          { id: 1, result: undefined },
          { id: 2, result: undefined },
        ]);
      } finally {
        delete requireFromHere.cache[requireFromHere.resolve(workerPath)];
      }
    },
  );
});

// ---------------------------------------------------------------------------
// D-001 property-style mocked worker protocol: generated branches stay stable
// ---------------------------------------------------------------------------
test("D-001 property-style mocked worker protocol covers init/run/close branches", () => {
  const workerPath = resolve(__dirname, "../../../dist/worker.js");
  const developmentPath = resolve(
    dirname(workerPath),
    "..",
    "..",
    "..",
    "crates",
    "ferric-napi",
    "index.js",
  );

  class FakeParentPort extends EventEmitter {
    readonly responses: any[] = [];
    postMessage(message: unknown): void {
      this.responses.push(message);
    }
  }

  class FakeEngine {
    static readonly instances: FakeEngine[] = [];
    static readonly snapshots: Array<{ data: number[]; format: number | undefined }> = [];

    static fromSnapshot(data: Buffer, format?: number): FakeEngine {
      FakeEngine.snapshots.push({ data: [...data], format });
      return new FakeEngine({ restored: true });
    }

    closeThrows = false;
    halted = false;
    loadCalls: string[] = [];
    resetCalls = 0;
    runResults: Array<{ rulesFired: number; haltReason: number }> = [];

    constructor(readonly options: Record<string, unknown> = {}) {
      FakeEngine.instances.push(this);
    }

    close(): void {
      if (this.closeThrows) throw new Error("close failed");
    }

    load(source: string): void {
      this.loadCalls.push(source);
    }

    reset(): void {
      this.resetCalls += 1;
    }

    run(limit?: number): { rulesFired: number; haltReason: number } {
      return this.runResults.shift() ?? {
        rulesFired: typeof limit === "number" ? limit : 0,
        haltReason: 1,
      };
    }

    halt(): void {
      this.halted = true;
    }

    facts(): unknown[] {
      throw "string failure";
    }

    serialize(): Buffer {
      return Buffer.from([1, 2, 3]);
    }
  }

  const parentPort = new FakeParentPort();
  delete requireFromHere.cache[requireFromHere.resolve(workerPath)];

  withPatchedLoad(
    (request, parent, isMain, originalLoad) => {
      if (request === "node:worker_threads") return { parentPort };
      if (request === developmentPath) {
        return {
          Engine: FakeEngine,
          FerricSymbol: class FerricSymbol {
            constructor(readonly value: string) {}
          },
        };
      }
      return originalLoad(request, parent, isMain);
    },
    () => {
      try {
        requireFromHere(workerPath);

        // Generated frames cover the less common protocol branches without
        // depending on native timing: pre-init rejection, source init, snapshot
        // init, all run modes, serialize transfer, non-Error catches, and close.
        parentPort.emit("message", { id: 1, method: "facts", args: [] });
        assert.match(parentPort.responses.pop().error.message, /not initialized/);

        parentPort.emit("message", {
          id: 2,
          method: "__init",
          args: [{ options: { maxCallDepth: 3 }, source: "(defrule ok =>)" }],
        });
        const sourceEngine = FakeEngine.instances.at(-1)!;
        assert.deepStrictEqual(sourceEngine.loadCalls, ["(defrule ok =>)"]);
        assert.strictEqual(sourceEngine.resetCalls, 1);

        parentPort.emit("message", { id: 21, method: "load", args: ["(defrule later =>)"] });
        assert.deepStrictEqual(parentPort.responses.pop(), { id: 21, result: null });
        assert.deepStrictEqual(sourceEngine.loadCalls, [
          "(defrule ok =>)",
          "(defrule later =>)",
        ]);

        parentPort.emit("message", {
          id: 3,
          method: "__init",
          args: [{ snapshot: { data: Uint8Array.from([9, 8]).buffer, format: 1 } }],
        });
        assert.deepStrictEqual(FakeEngine.snapshots, [{ data: [9, 8], format: 1 }]);
        const engine = FakeEngine.instances.at(-1)!;

        parentPort.emit("message", { id: 4, method: "__run_batched", args: [0, null] });
        assert.deepStrictEqual(parentPort.responses.pop(), {
          id: 4,
          result: { rulesFired: 0, haltReason: 1 },
        });

        parentPort.emit("message", { id: 5, method: "__run_batched", args: [5, null] });
        assert.deepStrictEqual(parentPort.responses.pop(), {
          id: 5,
          result: { rulesFired: 5, haltReason: 1 },
        });

        engine.runResults.push({ rulesFired: 1, haltReason: 0 });
        parentPort.emit("message", { id: 6, method: "__run_batched", args: [null, null] });
        assert.deepStrictEqual(parentPort.responses.pop(), {
          id: 6,
          result: { rulesFired: 1, haltReason: 0 },
        });

        const sab = new SharedArrayBuffer(Int32Array.BYTES_PER_ELEMENT);
        Atomics.store(new Int32Array(sab), 0, 1);
        parentPort.emit("message", { id: 7, method: "__run_batched", args: [10, sab] });
        assert.strictEqual(parentPort.responses.pop().result.haltReason, 2);
        assert.strictEqual(engine.halted, true);

        parentPort.emit("message", { id: 8, method: "serialize", args: [] });
        assert.ok(parentPort.responses.pop().result instanceof ArrayBuffer);

        parentPort.emit("message", { id: 9, method: "facts", args: [] });
        assert.deepStrictEqual(parentPort.responses.pop().error, {
          name: "Error",
          message: "string failure",
          code: "FERRIC_ERROR",
        });

        engine.closeThrows = true;
        parentPort.emit("message", { id: 10, method: "__close", args: [] });
        assert.deepStrictEqual(parentPort.responses.pop(), { id: 10, result: undefined });
      } finally {
        delete requireFromHere.cache[requireFromHere.resolve(workerPath)];
      }
    },
  );
});
